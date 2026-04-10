use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use deno_ast::swc::ast::{
    ArrayLit, ArrowExpr, AssignPatProp, BinaryOp, BlockStmt, BlockStmtOrExpr, CallExpr, Callee,
    CatchClause, ClassDecl, ClassExpr, ClassMember, ClassMethod, ClassProp, ComputedPropName,
    CondExpr, Constructor, Decl, DoWhileStmt, Expr, ExprOrSpread, FnDecl, FnExpr, ForHead,
    ForInStmt, ForOfStmt, ForStmt, Function, Ident, IfStmt, ImportDecl, ImportSpecifier,
    KeyValuePatProp, MemberExpr, MemberProp, MethodProp, ModuleDecl, NewExpr, ObjectPatProp,
    OptCall, OptChainBase, Param, Pat, Prop, PropOrSpread, Stmt, SwitchStmt, VarDeclKind,
    VarDeclarator, WhileStmt,
};
use deno_ast::swc::common::{BytePos, Span};
use deno_ast::swc::common::Spanned;
use deno_ast::{
    MediaType, ModuleSpecifier, ParseParams, ParsedSource, ProgramRef, SourcePos, parse_program,
};
use prism_js::{PrismCompilerEffectKind, prism_compiler_method_spec};

use super::{
    PreparedTypescriptProgram,
    program_ir::{
        PrismProgramBindingId, PrismProgramBindingKind, PrismProgramBranchKind,
        PrismProgramCallbackBoundaryKind, PrismProgramCompetitionKind,
        PrismProgramEffectKind, PrismProgramExitMode, PrismProgramFunctionBoundaryKind,
        PrismProgramHandleKind, PrismProgramIr, PrismProgramLoopKind, PrismProgramOperationKind,
        PrismProgramParallelKind, PrismProgramReductionKind, PrismProgramRegionControl,
        PrismProgramRegionId, PrismProgramRegionKind, PrismProgramSequenceKind,
        PrismProgramShortCircuitKind, PrismProgramSourceSpan,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct AnalyzedPrismProgram {
    pub(crate) ir: PrismProgramIr,
}

#[derive(Debug, Clone)]
struct ScopeFrame {
    bindings: HashMap<String, PrismProgramBindingId>,
    function_depth: usize,
}

struct ProgramAnalyzer<'a> {
    parsed: &'a ParsedSource,
    ir: PrismProgramIr,
    scopes: Vec<ScopeFrame>,
    region_stack: Vec<PrismProgramRegionId>,
    function_region_stack: Vec<PrismProgramRegionId>,
    function_depth: usize,
    region_capture_keys: HashSet<(PrismProgramRegionId, PrismProgramBindingId)>,
}

impl<'a> ProgramAnalyzer<'a> {
    fn new(parsed: &'a ParsedSource) -> Self {
        let full_span = Span::new(
            BytePos(1),
            BytePos(parsed.text_info_lazy().text_str().len() as u32 + 1),
        );
        let root_span = source_span(parsed, full_span);
        Self {
            ir: PrismProgramIr::new(parsed.specifier().to_string(), root_span),
            parsed,
            scopes: vec![ScopeFrame {
                bindings: HashMap::new(),
                function_depth: 0,
            }],
            region_stack: vec![0],
            function_region_stack: vec![0],
            function_depth: 0,
            region_capture_keys: HashSet::new(),
        }
    }

    fn analyze_program_ref(mut self, program: ProgramRef<'_>) -> PrismProgramIr {
        let root_span = self.ir.regions[0].span.clone();
        self.with_sequence_region(root_span, PrismProgramSequenceKind::ModuleBody, |this, _| {
            match program {
                ProgramRef::Module(module) => {
                    for item in &module.body {
                        match item {
                            deno_ast::swc::ast::ModuleItem::ModuleDecl(decl) => {
                                this.analyze_module_decl(decl)
                            }
                            deno_ast::swc::ast::ModuleItem::Stmt(stmt) => this.analyze_stmt(stmt),
                        }
                    }
                }
                ProgramRef::Script(script) => {
                    for stmt in &script.body {
                        this.analyze_stmt(stmt);
                    }
                }
            }
        });
        self.ir
    }

    fn current_region(&self) -> PrismProgramRegionId {
        *self
            .region_stack
            .last()
            .expect("region stack should not be empty")
    }

    fn current_function_region(&self) -> PrismProgramRegionId {
        *self
            .function_region_stack
            .last()
            .expect("function region stack should not be empty")
    }

    fn push_scope(&mut self) {
        self.scopes.push(ScopeFrame {
            bindings: HashMap::new(),
            function_depth: self.function_depth,
        });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn with_region_control<F>(
        &mut self,
        kind: PrismProgramRegionKind,
        control: PrismProgramRegionControl,
        span: Span,
        f: F,
    ) where
        F: FnOnce(&mut Self, PrismProgramRegionId),
    {
        let parent = self.current_region();
        let region_id = self
            .ir
            .add_region(parent, kind, control, source_span(self.parsed, span));
        self.region_stack.push(region_id);
        f(self, region_id);
        self.finalize_region(region_id);
        self.region_stack.pop();
    }

    fn with_sequence_region<F>(
        &mut self,
        span: PrismProgramSourceSpan,
        kind: PrismProgramSequenceKind,
        f: F,
    ) where
        F: FnOnce(&mut Self, PrismProgramRegionId),
    {
        let parent = self.current_region();
        let region_id = self.ir.add_region(
            parent,
            PrismProgramRegionKind::Sequence,
            PrismProgramRegionControl::Sequence { kind },
            span,
        );
        self.region_stack.push(region_id);
        f(self, region_id);
        self.finalize_region(region_id);
        self.region_stack.pop();
    }

    fn with_function_boundary<F>(
        &mut self,
        span: Span,
        kind: PrismProgramFunctionBoundaryKind,
        name: Option<String>,
        body_kind: PrismProgramSequenceKind,
        f: F,
    ) where
        F: FnOnce(&mut Self),
    {
        let parent = self.current_region();
        let boundary_id = self.ir.add_region(
            parent,
            PrismProgramRegionKind::FunctionBoundary,
            PrismProgramRegionControl::FunctionBoundary { kind, name },
            source_span(self.parsed, span),
        );
        self.region_stack.push(boundary_id);
        self.function_region_stack.push(boundary_id);
        self.function_depth += 1;
        self.push_scope();
        let sequence_span = self.ir.regions[boundary_id].span.clone();
        self.with_sequence_region(sequence_span, body_kind, |this, _| f(this));
        self.pop_scope();
        self.function_depth -= 1;
        self.function_region_stack.pop();
        self.region_stack.pop();
    }

    fn with_callback_boundary<F>(
        &mut self,
        span: Span,
        kind: PrismProgramCallbackBoundaryKind,
        origin: Option<String>,
        f: F,
    ) where
        F: FnOnce(&mut Self),
    {
        let parent = self.current_region();
        let boundary_id = self.ir.add_region(
            parent,
            PrismProgramRegionKind::CallbackBoundary,
            PrismProgramRegionControl::CallbackBoundary { kind, origin },
            source_span(self.parsed, span),
        );
        self.region_stack.push(boundary_id);
        self.function_region_stack.push(boundary_id);
        self.function_depth += 1;
        self.push_scope();
        let sequence_span = self.ir.regions[boundary_id].span.clone();
        self.with_sequence_region(sequence_span, PrismProgramSequenceKind::CallbackBody, |this, _| {
            f(this)
        });
        self.pop_scope();
        self.function_depth -= 1;
        self.function_region_stack.pop();
        self.region_stack.pop();
    }

    fn bind(
        &mut self,
        name: &str,
        kind: PrismProgramBindingKind,
        span: Span,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        let region_id = self.current_region();
        let binding_id = self.ir.add_binding(
            region_id,
            name.to_string(),
            kind,
            source_span(self.parsed, span),
            handle_kind,
        );
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name.to_string(), binding_id);
        }
        self.ir.add_operation(
            region_id,
            PrismProgramOperationKind::BindingWrite,
            source_span(self.parsed, span),
            None,
            vec![binding_id],
        );
        binding_id
    }

    fn finalize_region(&mut self, region_id: PrismProgramRegionId) {
        let child_regions = self.ir.regions[region_id].child_regions.clone();
        for child_id in child_regions {
            let child = self.ir.regions[child_id].clone();
            for binding_id in child.input_bindings {
                self.ir.record_input_binding(region_id, binding_id);
            }
            for binding_id in child.output_bindings {
                self.ir.record_output_binding(region_id, binding_id);
            }
            for exit_mode in child.exit_modes {
                self.ir.record_exit_mode(region_id, exit_mode);
            }
        }
    }

    fn resolve_binding(&self, name: &str) -> Option<(PrismProgramBindingId, usize)> {
        self.scopes.iter().rev().find_map(|scope| {
            scope
                .bindings
                .get(name)
                .copied()
                .map(|id| (id, scope.function_depth))
        })
    }

    fn record_binding_read(&mut self, ident: &Ident) {
        let name = ident.sym.to_string();
        let Some((binding_id, binding_function_depth)) = self.resolve_binding(&name) else {
            return;
        };
        if self.function_depth > binding_function_depth {
            let region_id = self.current_function_region();
            if self.region_capture_keys.insert((region_id, binding_id)) {
                self.ir.add_capture(
                    region_id,
                    binding_id,
                    name,
                    source_span(self.parsed, ident.span),
                );
            }
        }
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::BindingRead,
            source_span(self.parsed, ident.span),
            None,
            vec![binding_id],
        );
    }

    fn record_guard(&mut self, span: Span, binding_ids: Vec<PrismProgramBindingId>) {
        let region_id = self.current_region();
        self.ir.set_guard_span(region_id, source_span(self.parsed, span));
        self.ir.add_operation(
            region_id,
            PrismProgramOperationKind::GuardEvaluation,
            source_span(self.parsed, span),
            None,
            binding_ids,
        );
    }

    fn record_await_boundary(&mut self, span: Span) {
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::AwaitBoundary,
            source_span(self.parsed, span),
            None,
            Vec::new(),
        );
    }

    fn record_exit(&mut self, kind: PrismProgramOperationKind, span: Span) {
        let exit_mode = match kind {
            PrismProgramOperationKind::Return => PrismProgramExitMode::Return,
            PrismProgramOperationKind::Throw => PrismProgramExitMode::Throw,
            PrismProgramOperationKind::Break => PrismProgramExitMode::Break,
            PrismProgramOperationKind::Continue => PrismProgramExitMode::Continue,
            PrismProgramOperationKind::GuardEvaluation
            | PrismProgramOperationKind::AwaitBoundary
            | PrismProgramOperationKind::BindingRead
            | PrismProgramOperationKind::BindingWrite
            | PrismProgramOperationKind::PureCompute => PrismProgramExitMode::Normal,
        };
        let span = source_span(self.parsed, span);
        self.ir.add_operation(
            self.current_region(),
            kind,
            span,
            None,
            Vec::new(),
        );
        for region_id in self.region_stack.iter().copied() {
            self.ir.record_exit_mode(region_id, exit_mode);
        }
    }

    fn effect_kind_for_method_path(path: &str) -> PrismProgramEffectKind {
        if let Some(spec) = prism_compiler_method_spec(path) {
            return match spec.effect {
                PrismCompilerEffectKind::CoordinationRead => PrismProgramEffectKind::CoordinationRead,
                PrismCompilerEffectKind::CoordinationWrite => {
                    PrismProgramEffectKind::AuthoritativeWrite
                }
            };
        }
        if path.starts_with("prism.fs.")
            || path.starts_with("prism.time.")
            || path.starts_with("prism.random.")
            || path.starts_with("prism.config.")
            || path.starts_with("prism.env.")
        {
            return PrismProgramEffectKind::HostedDeterministicInput;
        }
        if path.starts_with("prism.actions.") || path.starts_with("prism.runners.") {
            return PrismProgramEffectKind::ExternalExecutionIntent;
        }
        if path.starts_with("prism.coordination.compile")
            || path.starts_with("prism.plans.compile")
            || path.starts_with("prism.plan.compile")
            || path.starts_with("prism.code.compile")
        {
            return PrismProgramEffectKind::ReusableArtifactEmission;
        }
        if path.starts_with("prism.") {
            return PrismProgramEffectKind::CoordinationRead;
        }
        PrismProgramEffectKind::PureCompute
    }

    fn record_call_effect(&mut self, call_span: Span, method_path: Option<String>) {
        let effect_kind = method_path
            .as_deref()
            .map(Self::effect_kind_for_method_path)
            .unwrap_or(PrismProgramEffectKind::PureCompute);
        self.ir.add_effect(
            self.current_region(),
            effect_kind,
            source_span(self.parsed, call_span),
            method_path.clone(),
        );
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::PureCompute,
            source_span(self.parsed, call_span),
            method_path,
            Vec::new(),
        );
    }

    fn analyze_module_decl(&mut self, decl: &ModuleDecl) {
        match decl {
            ModuleDecl::Import(import) => self.analyze_import(import),
            ModuleDecl::ExportDecl(export) => self.analyze_decl(&export.decl),
            ModuleDecl::ExportDefaultDecl(export) => match &export.decl {
                deno_ast::swc::ast::DefaultDecl::Fn(fn_expr) => self.analyze_default_fn(fn_expr),
                deno_ast::swc::ast::DefaultDecl::Class(class) => {
                    self.analyze_class_decl_body(None, &class.class)
                }
                _ => {}
            },
            ModuleDecl::ExportDefaultExpr(expr) => self.analyze_expr(&expr.expr),
            _ => {}
        }
    }

    fn analyze_import(&mut self, import: &ImportDecl) {
        for specifier in &import.specifiers {
            match specifier {
                ImportSpecifier::Default(default) => {
                    self.bind(
                        &default.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        default.local.span,
                        None,
                    );
                }
                ImportSpecifier::Named(named) => {
                    self.bind(
                        &named.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        named.local.span,
                        None,
                    );
                }
                ImportSpecifier::Namespace(namespace) => {
                    self.bind(
                        &namespace.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        namespace.local.span,
                        None,
                    );
                }
            }
        }
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(block) => self.analyze_block_stmt(block, PrismProgramSequenceKind::Block),
            Stmt::Decl(decl) => self.analyze_decl(decl),
            Stmt::Expr(expr) => self.analyze_expr(&expr.expr),
            Stmt::If(if_stmt) => self.analyze_if(if_stmt),
            Stmt::For(for_stmt) => self.analyze_for(for_stmt),
            Stmt::ForOf(for_of) => self.analyze_for_of(for_of),
            Stmt::ForIn(for_in) => self.analyze_for_in(for_in),
            Stmt::While(while_stmt) => self.analyze_while(while_stmt),
            Stmt::DoWhile(do_while) => self.analyze_do_while(do_while),
            Stmt::Try(try_stmt) => self.analyze_try(try_stmt),
            Stmt::Switch(switch_stmt) => self.analyze_switch(switch_stmt),
            Stmt::Return(return_stmt) => {
                if let Some(arg) = &return_stmt.arg {
                    self.analyze_expr(arg);
                }
                self.record_exit(PrismProgramOperationKind::Return, return_stmt.span);
            }
            Stmt::Throw(throw_stmt) => {
                self.analyze_expr(&throw_stmt.arg);
                self.record_exit(PrismProgramOperationKind::Throw, throw_stmt.span);
            }
            Stmt::Break(break_stmt) => self.record_exit(PrismProgramOperationKind::Break, break_stmt.span),
            Stmt::Continue(continue_stmt) => {
                self.record_exit(PrismProgramOperationKind::Continue, continue_stmt.span)
            }
            Stmt::Labeled(labeled) => self.analyze_stmt(&labeled.body),
            Stmt::With(with_stmt) => {
                self.analyze_expr(&with_stmt.obj);
                self.analyze_stmt(&with_stmt.body);
            }
            _ => {}
        }
    }

    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(fn_decl) => self.analyze_fn_decl(fn_decl),
            Decl::Var(var_decl) => {
                for decl in &var_decl.decls {
                    self.analyze_var_decl(var_decl.kind, decl);
                }
            }
            Decl::Class(class_decl) => self.analyze_class_decl(class_decl),
            _ => {}
        }
    }

    fn analyze_fn_decl(&mut self, fn_decl: &FnDecl) {
        self.bind(
            &fn_decl.ident.sym.to_string(),
            PrismProgramBindingKind::Function,
            fn_decl.ident.span,
            None,
        );
        self.analyze_function(
            &fn_decl.function,
            PrismProgramFunctionBoundaryKind::NamedFunction,
            Some(fn_decl.ident.sym.to_string()),
        );
    }

    fn analyze_default_fn(&mut self, fn_expr: &FnExpr) {
        let name = fn_expr.ident.as_ref().map(|ident| ident.sym.to_string());
        self.analyze_function(
            &fn_expr.function,
            if name.is_some() {
                PrismProgramFunctionBoundaryKind::NamedFunction
            } else {
                PrismProgramFunctionBoundaryKind::AnonymousFunction
            },
            name,
        );
    }

    fn analyze_class_decl(&mut self, class_decl: &ClassDecl) {
        self.bind(
            &class_decl.ident.sym.to_string(),
            PrismProgramBindingKind::Function,
            class_decl.ident.span,
            None,
        );
        self.analyze_class_decl_body(Some(class_decl.ident.sym.to_string()), &class_decl.class);
    }

    fn analyze_class_decl_body(
        &mut self,
        _name: Option<String>,
        class: &deno_ast::swc::ast::Class,
    ) {
        for member in &class.body {
            match member {
                ClassMember::Constructor(Constructor { body: Some(body), span, .. }) => {
                    self.with_function_boundary(
                        *span,
                        PrismProgramFunctionBoundaryKind::Constructor,
                        Some("constructor".to_string()),
                        PrismProgramSequenceKind::FunctionBody,
                        |this| this.analyze_block_contents(body),
                    );
                }
                ClassMember::Method(ClassMethod { key, function, .. }) => {
                    let name = class_member_name(key);
                    self.analyze_function(
                        function,
                        PrismProgramFunctionBoundaryKind::ClassMethod,
                        name,
                    );
                }
                ClassMember::ClassProp(ClassProp { value: Some(value), .. }) => {
                    self.analyze_expr(value);
                }
                _ => {}
            }
        }
    }

    fn analyze_var_decl(&mut self, kind: VarDeclKind, decl: &VarDeclarator) {
        let handle_kind = decl.init.as_deref().and_then(|expr| infer_handle_kind(self, expr));
        let binding_kind = match kind {
            VarDeclKind::Const => PrismProgramBindingKind::LexicalConst,
            VarDeclKind::Let => PrismProgramBindingKind::LexicalLet,
            VarDeclKind::Var => PrismProgramBindingKind::LexicalVar,
        };
        self.bind_pattern(&decl.name, binding_kind, handle_kind);
        if let Some(init) = &decl.init {
            self.analyze_expr(init);
        }
    }

    fn bind_pattern(
        &mut self,
        pat: &Pat,
        kind: PrismProgramBindingKind,
        handle_kind: Option<PrismProgramHandleKind>,
    ) {
        match pat {
            Pat::Ident(ident) => {
                self.bind(&ident.id.sym.to_string(), kind, ident.id.span, handle_kind);
            }
            Pat::Array(array) => {
                for elem in array.elems.iter().flatten() {
                    self.bind_pattern(elem, kind, handle_kind);
                }
            }
            Pat::Object(object) => {
                for prop in &object.props {
                    match prop {
                        ObjectPatProp::Assign(AssignPatProp { key, .. }) => {
                            self.bind(&key.sym.to_string(), kind, key.span, handle_kind);
                        }
                        ObjectPatProp::KeyValue(KeyValuePatProp { value, .. }) => {
                            self.bind_pattern(value, kind, handle_kind);
                        }
                        ObjectPatProp::Rest(rest) => self.bind_pattern(&rest.arg, kind, handle_kind),
                    }
                }
            }
            Pat::Rest(rest) => self.bind_pattern(&rest.arg, kind, handle_kind),
            Pat::Assign(assign) => self.bind_pattern(&assign.left, kind, handle_kind),
            _ => {}
        }
    }

    fn analyze_if(&mut self, if_stmt: &IfStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Branch,
            PrismProgramRegionControl::Branch {
                kind: PrismProgramBranchKind::IfElse,
            },
            if_stmt.span,
            |this, region_id| {
                let guard_bindings = collect_expr_binding_reads(this, &if_stmt.test);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, if_stmt.test.span()));
                this.ir.add_operation(
                    region_id,
                    PrismProgramOperationKind::GuardEvaluation,
                    source_span(this.parsed, if_stmt.test.span()),
                    None,
                    guard_bindings,
                );
                this.analyze_expr(&if_stmt.test);
                this.analyze_stmt(&if_stmt.cons);
                if let Some(alt) = &if_stmt.alt {
                    this.analyze_stmt(alt);
                }
            },
        );
    }

    fn analyze_switch(&mut self, switch_stmt: &SwitchStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Branch,
            PrismProgramRegionControl::Branch {
                kind: PrismProgramBranchKind::Switch,
            },
            switch_stmt.span,
            |this, region_id| {
                let guard_bindings = collect_expr_binding_reads(this, &switch_stmt.discriminant);
                this.ir.set_guard_span(
                    region_id,
                    source_span(this.parsed, switch_stmt.discriminant.span()),
                );
                this.ir.add_operation(
                    region_id,
                    PrismProgramOperationKind::GuardEvaluation,
                    source_span(this.parsed, switch_stmt.discriminant.span()),
                    None,
                    guard_bindings,
                );
                this.analyze_expr(&switch_stmt.discriminant);
                for case in &switch_stmt.cases {
                    let case_span = source_span(this.parsed, case.span);
                    this.with_sequence_region(case_span, PrismProgramSequenceKind::SwitchCase, |inner, _| {
                        if let Some(test) = &case.test {
                            inner.record_guard(test.span(), collect_expr_binding_reads(inner, test));
                            inner.analyze_expr(test);
                        }
                        for stmt in &case.cons {
                            inner.analyze_stmt(stmt);
                        }
                    });
                }
            },
        );
    }

    fn analyze_for(&mut self, for_stmt: &ForStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::For,
            },
            for_stmt.span,
            |this, region_id| {
                this.push_scope();
                if let Some(init) = &for_stmt.init {
                    match init {
                        deno_ast::swc::ast::VarDeclOrExpr::VarDecl(var_decl) => {
                            for decl in &var_decl.decls {
                                this.analyze_var_decl(var_decl.kind, decl);
                            }
                        }
                        deno_ast::swc::ast::VarDeclOrExpr::Expr(expr) => this.analyze_expr(expr),
                    }
                }
                if let Some(test) = &for_stmt.test {
                    this.ir
                        .set_guard_span(region_id, source_span(this.parsed, test.span()));
                    this.record_guard(test.span(), collect_expr_binding_reads(this, test));
                    this.analyze_expr(test);
                }
                if let Some(update) = &for_stmt.update {
                    this.analyze_expr(update);
                }
                this.analyze_stmt(&for_stmt.body);
                this.pop_scope();
            },
        );
    }

    fn analyze_for_of(&mut self, for_of: &ForOfStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForOf,
            },
            for_of.span,
            |this, region_id| {
                this.push_scope();
                this.bind_for_head(&for_of.left);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, for_of.right.span()));
                this.record_guard(for_of.right.span(), collect_expr_binding_reads(this, &for_of.right));
                this.analyze_expr(&for_of.right);
                this.analyze_stmt(&for_of.body);
                this.pop_scope();
            },
        );
    }

    fn analyze_for_in(&mut self, for_in: &ForInStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForIn,
            },
            for_in.span,
            |this, region_id| {
                this.push_scope();
                this.bind_for_head(&for_in.left);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, for_in.right.span()));
                this.record_guard(for_in.right.span(), collect_expr_binding_reads(this, &for_in.right));
                this.analyze_expr(&for_in.right);
                this.analyze_stmt(&for_in.body);
                this.pop_scope();
            },
        );
    }

    fn analyze_while(&mut self, while_stmt: &WhileStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::While,
            },
            while_stmt.span,
            |this, region_id| {
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, while_stmt.test.span()));
                this.record_guard(
                    while_stmt.test.span(),
                    collect_expr_binding_reads(this, &while_stmt.test),
                );
                this.analyze_expr(&while_stmt.test);
                this.analyze_stmt(&while_stmt.body);
            },
        );
    }

    fn analyze_do_while(&mut self, do_while: &DoWhileStmt) {
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::DoWhile,
            },
            do_while.span,
            |this, region_id| {
                this.analyze_stmt(&do_while.body);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, do_while.test.span()));
                this.record_guard(
                    do_while.test.span(),
                    collect_expr_binding_reads(this, &do_while.test),
                );
                this.analyze_expr(&do_while.test);
            },
        );
    }

    fn analyze_try(&mut self, try_stmt: &deno_ast::swc::ast::TryStmt) {
        self.with_region_control(
            PrismProgramRegionKind::TryCatchFinally,
            PrismProgramRegionControl::TryCatchFinally,
            try_stmt.span,
            |this, _| {
                this.analyze_block_stmt(&try_stmt.block, PrismProgramSequenceKind::TryBody);
                if let Some(handler) = &try_stmt.handler {
                    this.analyze_catch(handler);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    this.analyze_block_stmt(finalizer, PrismProgramSequenceKind::FinallyBody);
                }
            },
        );
    }

    fn analyze_catch(&mut self, catch_clause: &CatchClause) {
        self.push_scope();
        if let Some(param) = &catch_clause.param {
            self.bind_pattern(param, PrismProgramBindingKind::CatchParameter, None);
        }
        self.analyze_block_stmt(&catch_clause.body, PrismProgramSequenceKind::CatchBody);
        self.pop_scope();
    }

    fn analyze_block_stmt(&mut self, block: &BlockStmt, kind: PrismProgramSequenceKind) {
        let span = source_span(self.parsed, block.span);
        self.with_sequence_region(span, kind, |this, _| {
            this.push_scope();
            this.analyze_block_contents(block);
            this.pop_scope();
        });
    }

    fn analyze_block_contents(&mut self, block: &BlockStmt) {
        for stmt in &block.stmts {
            self.analyze_stmt(stmt);
        }
    }

    fn bind_for_head(&mut self, left: &ForHead) {
        match left {
            ForHead::VarDecl(var_decl) => {
                for decl in &var_decl.decls {
                    self.analyze_var_decl(var_decl.kind, decl);
                }
            }
            ForHead::Pat(pat) => self.bind_pattern(pat, PrismProgramBindingKind::LexicalLet, None),
            ForHead::UsingDecl(_) => {}
        }
    }

    fn analyze_function(
        &mut self,
        function: &Function,
        kind: PrismProgramFunctionBoundaryKind,
        name: Option<String>,
    ) {
        self.with_function_boundary(
            function.span,
            kind,
            name,
            PrismProgramSequenceKind::FunctionBody,
            |this| {
                for param in &function.params {
                    this.bind_param(param);
                }
                if let Some(body) = &function.body {
                    this.analyze_block_contents(body);
                }
            },
        );
    }

    fn analyze_arrow_function(&mut self, arrow: &ArrowExpr) {
        self.with_function_boundary(
            arrow.span,
            PrismProgramFunctionBoundaryKind::ArrowFunction,
            None,
            PrismProgramSequenceKind::FunctionBody,
            |this| {
                for param in &arrow.params {
                    this.bind_pattern(param, PrismProgramBindingKind::Parameter, None);
                }
                match &*arrow.body {
                    BlockStmtOrExpr::BlockStmt(block) => this.analyze_block_contents(block),
                    BlockStmtOrExpr::Expr(expr) => this.analyze_expr(expr),
                }
            },
        );
    }

    fn analyze_callback_arrow(&mut self, arrow: &ArrowExpr, origin: Option<String>) {
        self.with_callback_boundary(
            arrow.span,
            PrismProgramCallbackBoundaryKind::CallbackArrow,
            origin,
            |this| {
                for param in &arrow.params {
                    this.bind_pattern(param, PrismProgramBindingKind::Parameter, None);
                }
                match &*arrow.body {
                    BlockStmtOrExpr::BlockStmt(block) => this.analyze_block_contents(block),
                    BlockStmtOrExpr::Expr(expr) => this.analyze_expr(expr),
                }
            },
        );
    }

    fn analyze_callback_fn(&mut self, function: &Function, origin: Option<String>) {
        self.with_callback_boundary(
            function.span,
            PrismProgramCallbackBoundaryKind::CallbackFunction,
            origin,
            |this| {
                for param in &function.params {
                    this.bind_param(param);
                }
                if let Some(body) = &function.body {
                    this.analyze_block_contents(body);
                }
            },
        );
    }

    fn bind_param(&mut self, param: &Param) {
        self.bind_pattern(&param.pat, PrismProgramBindingKind::Parameter, None);
    }

    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(ident) => self.record_binding_read(ident),
            Expr::Call(call) => self.analyze_call(call),
            Expr::OptChain(chain) => match &*chain.base {
                OptChainBase::Call(call) => self.analyze_opt_call(call),
                OptChainBase::Member(member) => self.analyze_member(member),
            },
            Expr::Await(await_expr) => {
                self.record_await_boundary(await_expr.span);
                self.analyze_expr(&await_expr.arg);
            }
            Expr::Fn(fn_expr) => {
                if let Some(ident) = &fn_expr.ident {
                    self.bind(
                        &ident.sym.to_string(),
                        PrismProgramBindingKind::Function,
                        ident.span,
                        None,
                    );
                }
                self.analyze_function(
                    &fn_expr.function,
                    if fn_expr.ident.is_some() {
                        PrismProgramFunctionBoundaryKind::NamedFunction
                    } else {
                        PrismProgramFunctionBoundaryKind::AnonymousFunction
                    },
                    fn_expr.ident.as_ref().map(|ident| ident.sym.to_string()),
                );
            }
            Expr::Arrow(arrow) => self.analyze_arrow_function(arrow),
            Expr::Cond(cond) => self.analyze_conditional_expr(cond),
            Expr::Bin(bin) if is_short_circuit_op(bin.op) => self.analyze_short_circuit_expr(bin),
            Expr::Array(array) => self.analyze_array(array),
            Expr::Object(object) => {
                for prop in &object.props {
                    self.analyze_object_prop(prop);
                }
            }
            Expr::Assign(assign) => {
                self.analyze_assign_target(&assign.left);
                self.analyze_expr(&assign.right);
            }
            Expr::Member(member) => self.analyze_member(member),
            Expr::Paren(paren) => self.analyze_expr(&paren.expr),
            Expr::Unary(unary) => self.analyze_expr(&unary.arg),
            Expr::Update(update) => self.analyze_expr(&update.arg),
            Expr::Seq(seq) => {
                for expr in &seq.exprs {
                    self.analyze_expr(expr);
                }
            }
            Expr::Tpl(tpl) => {
                for expr in &tpl.exprs {
                    self.analyze_expr(expr);
                }
            }
            Expr::TaggedTpl(tagged) => {
                self.analyze_expr(&tagged.tag);
                for expr in &tagged.tpl.exprs {
                    self.analyze_expr(expr);
                }
            }
            Expr::Class(class_expr) => self.analyze_class_expr(class_expr),
            Expr::New(new_expr) => self.analyze_new_expr(new_expr),
            _ => {}
        }
    }

    fn analyze_conditional_expr(&mut self, cond: &CondExpr) {
        self.with_region_control(
            PrismProgramRegionKind::Branch,
            PrismProgramRegionControl::Branch {
                kind: PrismProgramBranchKind::ConditionalExpression,
            },
            cond.span,
            |this, region_id| {
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, cond.test.span()));
                this.record_guard(cond.test.span(), collect_expr_binding_reads(this, &cond.test));
                this.analyze_expr(&cond.test);
                this.analyze_expr(&cond.cons);
                this.analyze_expr(&cond.alt);
            },
        );
    }

    fn analyze_short_circuit_expr(&mut self, bin: &deno_ast::swc::ast::BinExpr) {
        let kind = match bin.op {
            BinaryOp::LogicalAnd => PrismProgramShortCircuitKind::LogicalAnd,
            BinaryOp::LogicalOr => PrismProgramShortCircuitKind::LogicalOr,
            BinaryOp::NullishCoalescing => PrismProgramShortCircuitKind::NullishCoalescing,
            _ => return,
        };
        self.with_region_control(
            PrismProgramRegionKind::ShortCircuit,
            PrismProgramRegionControl::ShortCircuit { kind },
            bin.span,
            |this, region_id| {
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, bin.left.span()));
                this.record_guard(bin.left.span(), collect_expr_binding_reads(this, &bin.left));
                this.analyze_expr(&bin.left);
                this.analyze_expr(&bin.right);
                this.ir.record_exit_mode(region_id, PrismProgramExitMode::ShortCircuit);
            },
        );
    }

    fn analyze_assign_target(&mut self, left: &deno_ast::swc::ast::AssignTarget) {
        match left {
            deno_ast::swc::ast::AssignTarget::Simple(simple) => match simple {
                deno_ast::swc::ast::SimpleAssignTarget::Ident(ident) => {
                    if let Some((binding_id, _)) = self.resolve_binding(&ident.id.sym.to_string()) {
                        self.ir.add_operation(
                            self.current_region(),
                            PrismProgramOperationKind::BindingWrite,
                            source_span(self.parsed, ident.id.span),
                            None,
                            vec![binding_id],
                        );
                    }
                }
                deno_ast::swc::ast::SimpleAssignTarget::Member(member) => self.analyze_member(member),
                deno_ast::swc::ast::SimpleAssignTarget::Paren(paren) => self.analyze_expr(&paren.expr),
                deno_ast::swc::ast::SimpleAssignTarget::OptChain(chain) => match &*chain.base {
                    OptChainBase::Call(call) => self.analyze_opt_call(call),
                    OptChainBase::Member(member) => self.analyze_member(member),
                },
                _ => {}
            },
            deno_ast::swc::ast::AssignTarget::Pat(_) => {}
        }
    }

    fn analyze_array(&mut self, array: &ArrayLit) {
        for element in array.elems.iter().flatten() {
            self.analyze_expr(&element.expr);
        }
    }

    fn analyze_object_prop(&mut self, prop: &PropOrSpread) {
        match prop {
            PropOrSpread::Prop(prop) => match &**prop {
                Prop::KeyValue(prop) => self.analyze_expr(&prop.value),
                Prop::Assign(assign) => self.record_binding_read(&assign.key),
                Prop::Getter(getter) => {
                    if let Some(body) = &getter.body {
                        self.analyze_block_stmt(body, PrismProgramSequenceKind::Block);
                    }
                }
                Prop::Setter(setter) => {
                    self.bind_pattern(&setter.param, PrismProgramBindingKind::Parameter, None);
                    if let Some(body) = &setter.body {
                        self.analyze_block_stmt(body, PrismProgramSequenceKind::Block);
                    }
                }
                Prop::Method(MethodProp { function, .. }) => self.analyze_function(
                    function,
                    PrismProgramFunctionBoundaryKind::AnonymousFunction,
                    None,
                ),
                _ => {}
            },
            PropOrSpread::Spread(spread) => self.analyze_expr(&spread.expr),
        }
    }

    fn analyze_member(&mut self, member: &MemberExpr) {
        self.analyze_expr(&member.obj);
        if let MemberProp::Computed(ComputedPropName { expr, .. }) = &member.prop {
            self.analyze_expr(expr);
        }
    }

    fn analyze_class_expr(&mut self, class_expr: &ClassExpr) {
        if let Some(ident) = &class_expr.ident {
            self.bind(
                &ident.sym.to_string(),
                PrismProgramBindingKind::Function,
                ident.span,
                None,
            );
        }
        self.analyze_class_decl_body(
            class_expr.ident.as_ref().map(|ident| ident.sym.to_string()),
            &class_expr.class,
        );
    }

    fn analyze_new_expr(&mut self, new_expr: &NewExpr) {
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::PureCompute,
            source_span(self.parsed, new_expr.span),
            None,
            Vec::new(),
        );
        self.analyze_expr(&new_expr.callee);
        if let Some(args) = &new_expr.args {
            for arg in args {
                self.analyze_expr(&arg.expr);
            }
        }
    }

    fn analyze_call(&mut self, call: &CallExpr) {
        let call_path = call_path_from_callee(self, &call.callee);
        if let Some(control) = call_path
            .as_deref()
            .and_then(program_region_control_for_call_path)
        {
            let region_kind = region_kind_for_control(&control);
            self.with_region_control(region_kind, control, call.span, |this, region_id| {
                this.ir.set_guard_span(region_id, source_span(this.parsed, call.span));
                this.record_call_effect(call.span, call_path.clone());
                this.analyze_call_children(call, call_path.as_deref(), Some(region_id));
            });
            return;
        }
        self.record_call_effect(call.span, call_path.clone());
        self.analyze_call_children(call, call_path.as_deref(), None);
    }

    fn analyze_opt_call(&mut self, call: &OptCall) {
        let call_path = call_path_from_expr(self, &call.callee);
        self.record_call_effect(call.span, call_path.clone());
        self.analyze_expr(&call.callee);
        for arg in &call.args {
            self.analyze_call_arg(arg, call_path.as_deref(), None);
        }
    }

    fn analyze_call_children(
        &mut self,
        call: &CallExpr,
        call_path: Option<&str>,
        region_id: Option<PrismProgramRegionId>,
    ) {
        match &call.callee {
            Callee::Expr(expr) => self.analyze_expr(expr),
            Callee::Super(_) | Callee::Import(_) => {}
        }
        if let Some(region_id) = region_id {
            let binding_ids = call
                .args
                .iter()
                .flat_map(|arg| collect_expr_binding_reads(self, &arg.expr))
                .collect::<Vec<_>>();
            self.ir.add_operation(
                region_id,
                PrismProgramOperationKind::GuardEvaluation,
                source_span(self.parsed, call.span),
                call_path.map(ToOwned::to_owned),
                binding_ids,
            );
        }
        for arg in &call.args {
            self.analyze_call_arg(arg, call_path, region_id);
        }
    }

    fn analyze_call_arg(
        &mut self,
        arg: &ExprOrSpread,
        call_path: Option<&str>,
        _region_id: Option<PrismProgramRegionId>,
    ) {
        if is_callback_argument(arg) {
            match &*arg.expr {
                Expr::Fn(fn_expr) => self.analyze_callback_fn(
                    &fn_expr.function,
                    call_path.map(ToOwned::to_owned),
                ),
                Expr::Arrow(arrow) => {
                    self.analyze_callback_arrow(arrow, call_path.map(ToOwned::to_owned))
                }
                other => self.analyze_expr(other),
            }
        } else {
            self.analyze_expr(&arg.expr);
        }
    }
}

fn is_callback_argument(arg: &ExprOrSpread) -> bool {
    matches!(&*arg.expr, Expr::Fn(_) | Expr::Arrow(_))
}

fn region_kind_for_control(control: &PrismProgramRegionControl) -> PrismProgramRegionKind {
    match control {
        PrismProgramRegionControl::Module => PrismProgramRegionKind::Module,
        PrismProgramRegionControl::Sequence { .. } => PrismProgramRegionKind::Sequence,
        PrismProgramRegionControl::Parallel { .. } => PrismProgramRegionKind::Parallel,
        PrismProgramRegionControl::Branch { .. } => PrismProgramRegionKind::Branch,
        PrismProgramRegionControl::Loop { .. } => PrismProgramRegionKind::Loop,
        PrismProgramRegionControl::ShortCircuit { .. } => PrismProgramRegionKind::ShortCircuit,
        PrismProgramRegionControl::TryCatchFinally => PrismProgramRegionKind::TryCatchFinally,
        PrismProgramRegionControl::FunctionBoundary { .. } => PrismProgramRegionKind::FunctionBoundary,
        PrismProgramRegionControl::CallbackBoundary { .. } => PrismProgramRegionKind::CallbackBoundary,
        PrismProgramRegionControl::Reduction { .. } => PrismProgramRegionKind::Reduction,
        PrismProgramRegionControl::Competition { .. } => PrismProgramRegionKind::Competition,
    }
}

fn program_region_control_for_call_path(path: &str) -> Option<PrismProgramRegionControl> {
    match path {
        "Promise.all" => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::PromiseAll,
            origin: Some(path.to_string()),
        }),
        "Promise.allSettled" => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::PromiseAllSettled,
            origin: Some(path.to_string()),
        }),
        "Promise.race" => Some(PrismProgramRegionControl::Competition {
            kind: PrismProgramCompetitionKind::PromiseRace,
            origin: Some(path.to_string()),
        }),
        "Promise.any" => Some(PrismProgramRegionControl::Competition {
            kind: PrismProgramCompetitionKind::PromiseAny,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".map") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::Map,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".flatMap") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::FlatMap,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".filter") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::Filter,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".forEach") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::ForEach,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".reduce") => Some(PrismProgramRegionControl::Reduction {
            kind: PrismProgramReductionKind::Reduce,
            origin: Some(path.to_string()),
        }),
        _ if path.ends_with(".some") => Some(PrismProgramRegionControl::ShortCircuit {
            kind: PrismProgramShortCircuitKind::Some,
        }),
        _ if path.ends_with(".every") => Some(PrismProgramRegionControl::ShortCircuit {
            kind: PrismProgramShortCircuitKind::Every,
        }),
        _ if path.ends_with(".find") => Some(PrismProgramRegionControl::ShortCircuit {
            kind: PrismProgramShortCircuitKind::Find,
        }),
        _ if path.ends_with(".findIndex") => Some(PrismProgramRegionControl::ShortCircuit {
            kind: PrismProgramShortCircuitKind::FindIndex,
        }),
        _ => None,
    }
}

fn infer_handle_kind(
    analyzer: &ProgramAnalyzer<'_>,
    expr: &Expr,
) -> Option<PrismProgramHandleKind> {
    let path = call_path_from_expr(analyzer, expr)?;
    match path.as_str() {
        "prism.coordination.createPlan" | "prism.coordination.openPlan" => {
            Some(PrismProgramHandleKind::Plan)
        }
        "plan.addTask" | "prism.coordination.openTask" => Some(PrismProgramHandleKind::Task),
        "prism.claim.acquire" | "prism.claim.renew" | "prism.claim.release" => {
            Some(PrismProgramHandleKind::Claim)
        }
        "prism.artifact.propose" | "prism.artifact.supersede" | "prism.artifact.review" => {
            Some(PrismProgramHandleKind::Artifact)
        }
        "prism.work.declare" => Some(PrismProgramHandleKind::Work),
        _ => None,
    }
}

fn call_path_from_expr(analyzer: &ProgramAnalyzer<'_>, expr: &Expr) -> Option<String> {
    match expr {
        Expr::Call(call) => call_path_from_callee(analyzer, &call.callee),
        Expr::Member(member) => member_expr_path(analyzer, member),
        Expr::Ident(ident) => ident_path(analyzer, ident),
        Expr::Paren(paren) => call_path_from_expr(analyzer, &paren.expr),
        _ => None,
    }
}

fn call_path_from_callee(analyzer: &ProgramAnalyzer<'_>, callee: &Callee) -> Option<String> {
    match callee {
        Callee::Expr(expr) => call_path_from_expr(analyzer, expr),
        _ => None,
    }
}

fn member_expr_path(analyzer: &ProgramAnalyzer<'_>, member: &MemberExpr) -> Option<String> {
    let object_path = call_path_from_expr(analyzer, &member.obj)?;
    let property = match &member.prop {
        MemberProp::Ident(ident) => ident.sym.to_string(),
        MemberProp::PrivateName(private) => private.name.to_string(),
        MemberProp::Computed(computed) => match &*computed.expr {
            Expr::Lit(deno_ast::swc::ast::Lit::Str(value)) => {
                value.value.to_atom_lossy().to_string()
            }
            _ => return None,
        },
    };
    Some(format!("{object_path}.{property}"))
}

fn ident_path(analyzer: &ProgramAnalyzer<'_>, ident: &Ident) -> Option<String> {
    let name = ident.sym.to_string();
    if name == "prism" || name == "Promise" {
        return Some(name);
    }
    let (binding_id, _) = analyzer.resolve_binding(&name)?;
    let binding = analyzer.ir.bindings.get(binding_id)?;
    match binding.handle_kind {
        Some(PrismProgramHandleKind::Plan) => Some("plan".to_string()),
        Some(PrismProgramHandleKind::Task) => Some("task".to_string()),
        Some(PrismProgramHandleKind::Claim) => Some("claim".to_string()),
        Some(PrismProgramHandleKind::Artifact) => Some("artifact".to_string()),
        Some(PrismProgramHandleKind::Work) => Some("work".to_string()),
        None => Some(name),
    }
}

fn collect_expr_binding_reads(
    analyzer: &ProgramAnalyzer<'_>,
    expr: &Expr,
) -> Vec<PrismProgramBindingId> {
    let mut bindings = Vec::new();
    collect_expr_binding_reads_into(analyzer, expr, &mut bindings);
    bindings.sort_unstable();
    bindings.dedup();
    bindings
}

fn collect_expr_binding_reads_into(
    analyzer: &ProgramAnalyzer<'_>,
    expr: &Expr,
    out: &mut Vec<PrismProgramBindingId>,
) {
    match expr {
        Expr::Ident(ident) => {
            if let Some((binding_id, _)) = analyzer.resolve_binding(&ident.sym.to_string()) {
                out.push(binding_id);
            }
        }
        Expr::Member(member) => {
            collect_expr_binding_reads_into(analyzer, &member.obj, out);
            if let MemberProp::Computed(computed) = &member.prop {
                collect_expr_binding_reads_into(analyzer, &computed.expr, out);
            }
        }
        Expr::Call(call) => {
            if let Callee::Expr(expr) = &call.callee {
                collect_expr_binding_reads_into(analyzer, expr, out);
            }
            for arg in &call.args {
                collect_expr_binding_reads_into(analyzer, &arg.expr, out);
            }
        }
        Expr::Await(await_expr) => collect_expr_binding_reads_into(analyzer, &await_expr.arg, out),
        Expr::Cond(cond) => {
            collect_expr_binding_reads_into(analyzer, &cond.test, out);
            collect_expr_binding_reads_into(analyzer, &cond.cons, out);
            collect_expr_binding_reads_into(analyzer, &cond.alt, out);
        }
        Expr::Bin(bin) => {
            collect_expr_binding_reads_into(analyzer, &bin.left, out);
            collect_expr_binding_reads_into(analyzer, &bin.right, out);
        }
        Expr::Seq(seq) => {
            for expr in &seq.exprs {
                collect_expr_binding_reads_into(analyzer, expr, out);
            }
        }
        Expr::Object(object) => {
            for prop in &object.props {
                match prop {
                    PropOrSpread::Prop(prop) => match &**prop {
                        Prop::KeyValue(prop) => {
                            collect_expr_binding_reads_into(analyzer, &prop.value, out)
                        }
                        Prop::Assign(assign) => {
                            if let Some((binding_id, _)) =
                                analyzer.resolve_binding(&assign.key.sym.to_string())
                            {
                                out.push(binding_id);
                            }
                        }
                        Prop::Method(method) => {
                            if let Some(body) = &method.function.body {
                                for stmt in &body.stmts {
                                    collect_stmt_binding_reads(analyzer, stmt, out);
                                }
                            }
                        }
                        _ => {}
                    },
                    PropOrSpread::Spread(spread) => {
                        collect_expr_binding_reads_into(analyzer, &spread.expr, out)
                    }
                }
            }
        }
        Expr::Array(array) => {
            for elem in array.elems.iter().flatten() {
                collect_expr_binding_reads_into(analyzer, &elem.expr, out);
            }
        }
        Expr::Paren(paren) => collect_expr_binding_reads_into(analyzer, &paren.expr, out),
        Expr::OptChain(chain) => match &*chain.base {
            OptChainBase::Call(call) => {
                collect_expr_binding_reads_into(analyzer, &call.callee, out);
                for arg in &call.args {
                    collect_expr_binding_reads_into(analyzer, &arg.expr, out);
                }
            }
            OptChainBase::Member(member) => collect_expr_binding_reads_into(analyzer, &member.obj, out),
        },
        Expr::Tpl(tpl) => {
            for expr in &tpl.exprs {
                collect_expr_binding_reads_into(analyzer, expr, out);
            }
        }
        Expr::TaggedTpl(tagged) => {
            collect_expr_binding_reads_into(analyzer, &tagged.tag, out);
            for expr in &tagged.tpl.exprs {
                collect_expr_binding_reads_into(analyzer, expr, out);
            }
        }
        Expr::Unary(unary) => collect_expr_binding_reads_into(analyzer, &unary.arg, out),
        Expr::Update(update) => collect_expr_binding_reads_into(analyzer, &update.arg, out),
        Expr::Assign(assign) => {
            collect_assign_target_binding_reads(analyzer, &assign.left, out);
            collect_expr_binding_reads_into(analyzer, &assign.right, out);
        }
        _ => {}
    }
}

fn collect_assign_target_binding_reads(
    analyzer: &ProgramAnalyzer<'_>,
    target: &deno_ast::swc::ast::AssignTarget,
    out: &mut Vec<PrismProgramBindingId>,
) {
    match target {
        deno_ast::swc::ast::AssignTarget::Simple(simple) => match simple {
            deno_ast::swc::ast::SimpleAssignTarget::Ident(ident) => {
                if let Some((binding_id, _)) = analyzer.resolve_binding(&ident.id.sym.to_string()) {
                    out.push(binding_id);
                }
            }
            deno_ast::swc::ast::SimpleAssignTarget::Member(member) => {
                collect_expr_binding_reads_into(analyzer, &member.obj, out)
            }
            deno_ast::swc::ast::SimpleAssignTarget::Paren(paren) => {
                collect_expr_binding_reads_into(analyzer, &paren.expr, out)
            }
            deno_ast::swc::ast::SimpleAssignTarget::OptChain(chain) => match &*chain.base {
                OptChainBase::Call(call) => {
                    collect_expr_binding_reads_into(analyzer, &call.callee, out)
                }
                OptChainBase::Member(member) => {
                    collect_expr_binding_reads_into(analyzer, &member.obj, out)
                }
            },
            _ => {}
        },
        deno_ast::swc::ast::AssignTarget::Pat(_) => {}
    }
}

fn collect_stmt_binding_reads(
    analyzer: &ProgramAnalyzer<'_>,
    stmt: &Stmt,
    out: &mut Vec<PrismProgramBindingId>,
) {
    match stmt {
        Stmt::Expr(expr) => collect_expr_binding_reads_into(analyzer, &expr.expr, out),
        Stmt::Return(return_stmt) => {
            if let Some(arg) = &return_stmt.arg {
                collect_expr_binding_reads_into(analyzer, arg, out);
            }
        }
        Stmt::Throw(throw_stmt) => collect_expr_binding_reads_into(analyzer, &throw_stmt.arg, out),
        Stmt::If(if_stmt) => {
            collect_expr_binding_reads_into(analyzer, &if_stmt.test, out);
            collect_stmt_binding_reads(analyzer, &if_stmt.cons, out);
            if let Some(alt) = &if_stmt.alt {
                collect_stmt_binding_reads(analyzer, alt, out);
            }
        }
        Stmt::Block(block) => {
            for stmt in &block.stmts {
                collect_stmt_binding_reads(analyzer, stmt, out);
            }
        }
        _ => {}
    }
}

fn is_short_circuit_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
    )
}

fn class_member_name(key: &deno_ast::swc::ast::PropName) -> Option<String> {
    match key {
        deno_ast::swc::ast::PropName::Ident(ident) => Some(ident.sym.to_string()),
        deno_ast::swc::ast::PropName::Str(value) => Some(value.value.to_atom_lossy().to_string()),
        deno_ast::swc::ast::PropName::Num(value) => Some(value.value.to_string()),
        deno_ast::swc::ast::PropName::Computed(_) => None,
        deno_ast::swc::ast::PropName::BigInt(value) => Some(value.value.to_string()),
    }
}

fn source_span(parsed: &ParsedSource, span: Span) -> PrismProgramSourceSpan {
    let start = parsed
        .text_info_lazy()
        .line_and_column_display(SourcePos::unsafely_from_byte_pos(span.lo));
    let end = parsed
        .text_info_lazy()
        .line_and_column_display(SourcePos::unsafely_from_byte_pos(span.hi));
    PrismProgramSourceSpan {
        specifier: parsed.specifier().to_string(),
        start_line: start.line_number,
        start_column: start.column_number,
        end_line: end.line_number,
        end_column: end.column_number,
    }
}

pub(crate) fn analyze_prepared_typescript_program(
    prepared: &PreparedTypescriptProgram,
) -> Result<AnalyzedPrismProgram> {
    let parsed = parse_program(ParseParams {
        specifier: ModuleSpecifier::parse(prepared.root_source().specifier())?,
        text: prepared.root_source().source_text().into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    })
    .map_err(|err| anyhow!(err.to_string()))?;
    let analyzer = ProgramAnalyzer::new(&parsed);
    Ok(AnalyzedPrismProgram {
        ir: analyzer.analyze_program_ref(parsed.program_ref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueryLanguage;
    use crate::prism_code_compiler::{
        PrismCodeCompilerInput, PrismTypescriptProgramMode, prepare_typescript_program,
    };

    fn analyze(code: &str) -> AnalyzedPrismProgram {
        let input = PrismCodeCompilerInput::inline("prism_code", code, QueryLanguage::Ts, true);
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");
        analyze_prepared_typescript_program(&prepared).expect("program should analyze")
    }

    #[test]
    fn analysis_tracks_branch_regions_guards_bindings_and_writes() {
        let analyzed = analyze(
            r#"
const plan = prism.coordination.createPlan({ title: "Ship" });
if (flag) {
  const task = plan.addTask({ title: "Build" });
  await task.complete({ summary: "done" });
}
"#,
        );
        let branch = analyzed
            .ir
            .regions
            .iter()
            .find(|region| {
                region.control
                    == PrismProgramRegionControl::Branch {
                        kind: PrismProgramBranchKind::IfElse,
                    }
            })
            .expect("if branch region should exist");
        assert!(branch.guard_span.is_some());
        assert!(!branch.output_bindings.is_empty());
        assert!(
            analyzed
                .ir
                .bindings
                .iter()
                .any(|binding| binding.name == "plan"
                    && binding.handle_kind == Some(PrismProgramHandleKind::Plan))
        );
        assert!(
            analyzed
                .ir
                .effects
                .iter()
                .any(|effect| effect.method_path.as_deref() == Some("plan.addTask")
                    && effect.kind == PrismProgramEffectKind::AuthoritativeWrite)
        );
        assert!(
            analyzed
                .ir
                .operations
                .iter()
                .any(|operation| operation.kind == PrismProgramOperationKind::AwaitBoundary)
        );
    }

    #[test]
    fn analysis_tracks_parallel_reduction_callbacks_and_captures() {
        let analyzed = analyze(
            r#"
const plan = prism.coordination.createPlan({ title: "Ship" });
async function build(items) {
  await Promise.all(items.map(async (item) => plan.addTask({ title: item })));
  return items.reduce((count, item) => count + item.length, 0);
}
"#,
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::Parallel {
                        kind: PrismProgramParallelKind::PromiseAll,
                        ..
                    }
                ))
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::Parallel {
                        kind: PrismProgramParallelKind::Map,
                        ..
                    }
                ))
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::Reduction {
                        kind: PrismProgramReductionKind::Reduce,
                        ..
                    }
                ))
        );
        assert!(
            analyzed
                .ir
                .captures
                .iter()
                .any(|capture| capture.binding_name == "plan")
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::CallbackBoundary {
                        kind: PrismProgramCallbackBoundaryKind::CallbackArrow,
                        ..
                    }
                ))
        );
    }

    #[test]
    fn analysis_distinguishes_plain_arrow_functions_from_callbacks() {
        let analyzed = analyze(
            r#"
const project = (items) => {
  if (items.length > 0) {
    return prism.coordination.openTask(items[0]);
  }
  return null;
};
"#,
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::FunctionBoundary {
                        kind: PrismProgramFunctionBoundaryKind::ArrowFunction,
                        ..
                    }
                ))
        );
        assert!(
            !analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::CallbackBoundary {
                        kind: PrismProgramCallbackBoundaryKind::CallbackArrow,
                        origin: None,
                    }
                ))
        );
    }

    #[test]
    fn analysis_tracks_loop_try_short_circuit_and_exit_modes() {
        let analyzed = analyze(
            r#"
for (const taskId of taskIds) {
  try {
    ready && prism.coordination.openTask(taskId);
  } catch (error) {
    return error;
  } finally {
    await prism.time.now();
  }
}
"#,
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::Loop {
                        kind: PrismProgramLoopKind::ForOf
                    }
                ))
        );
        let try_region = analyzed
            .ir
            .regions
            .iter()
            .find(|region| region.control == PrismProgramRegionControl::TryCatchFinally)
            .expect("try region should exist");
        assert!(try_region.exit_modes.contains(&PrismProgramExitMode::Return));
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::ShortCircuit {
                        kind: PrismProgramShortCircuitKind::LogicalAnd
                    }
                ))
        );
        assert!(
            analyzed
                .ir
                .effects
                .iter()
                .any(|effect| effect.kind == PrismProgramEffectKind::HostedDeterministicInput)
        );
        assert!(
            analyzed
                .ir
                .effects
                .iter()
                .any(|effect| effect.kind == PrismProgramEffectKind::CoordinationRead)
        );
    }

    #[test]
    fn analysis_tracks_competition_and_effect_taxonomy() {
        let analyzed = analyze(
            r#"
const winner = await Promise.any([
  prism.coordination.openTask(taskId),
  prism.time.now(),
]);
const emitted = prism.code.compilePlan?.("release");
const scheduled = prism.actions.deploy?.("staging");
return { winner, emitted, scheduled };
"#,
        );
        assert!(
            analyzed
                .ir
                .regions
                .iter()
                .any(|region| matches!(
                    region.control,
                    PrismProgramRegionControl::Competition {
                        kind: PrismProgramCompetitionKind::PromiseAny,
                        ..
                    }
                ))
        );
        assert!(
            analyzed
                .ir
                .effects
                .iter()
                .any(|effect| effect.kind == PrismProgramEffectKind::ReusableArtifactEmission)
        );
        assert!(
            analyzed
                .ir
                .effects
                .iter()
                .any(|effect| effect.kind == PrismProgramEffectKind::ExternalExecutionIntent)
        );
    }
}
