use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use deno_ast::swc::ast::{
    ArrayLit, ArrowExpr, AssignPatProp, BinaryOp, BlockStmtOrExpr, CallExpr, Callee, CatchClause,
    ClassMethod, ClassProp, Constructor, Decl, DoWhileStmt, Expr, ExprOrSpread, FnDecl, FnExpr,
    ForInStmt, ForOfStmt, ForStmt, Function, Ident, ImportDecl, ImportSpecifier, KeyValuePatProp,
    MemberExpr, MemberProp, MethodProp, ModuleDecl, ObjectPatProp, OptChainBase, Param, Pat, Prop,
    PropOrSpread, Stmt, SwitchStmt, TryStmt, VarDeclKind, VarDeclarator, WhileStmt,
};
use deno_ast::swc::common::{BytePos, Span};
use deno_ast::{
    parse_program, MediaType, ModuleSpecifier, ParseParams, ParsedSource, ProgramRef, SourcePos,
};
use prism_js::{prism_compiler_method_spec, PrismCompilerEffectKind};

use super::{
    program_ir::{
        PrismProgramBindingId, PrismProgramBindingKind, PrismProgramEffectKind,
        PrismProgramHandleKind, PrismProgramIr, PrismProgramRegionId, PrismProgramRegionKind,
        PrismProgramSourceSpan,
    },
    PreparedTypescriptProgram,
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

    fn with_region<F>(&mut self, kind: PrismProgramRegionKind, span: Span, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let parent = self.current_region();
        let region_id = self
            .ir
            .add_region(parent, kind, source_span(self.parsed, span));
        self.region_stack.push(region_id);
        f(self);
        self.region_stack.pop();
    }

    fn with_function_region<F>(&mut self, kind: PrismProgramRegionKind, span: Span, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let parent = self.current_region();
        let region_id = self
            .ir
            .add_region(parent, kind, source_span(self.parsed, span));
        self.region_stack.push(region_id);
        self.function_region_stack.push(region_id);
        self.function_depth += 1;
        self.push_scope();
        f(self);
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
        binding_id
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

    fn capture_if_needed(&mut self, ident: &Ident) {
        let name = ident.sym.to_string();
        let Some((binding_id, binding_function_depth)) = self.resolve_binding(&name) else {
            return;
        };
        if self.function_depth <= binding_function_depth {
            return;
        }
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

    fn effect_kind_for_method_path(path: &str) -> PrismProgramEffectKind {
        if let Some(spec) = prism_compiler_method_spec(path) {
            return match spec.effect {
                PrismCompilerEffectKind::CoordinationRead => {
                    PrismProgramEffectKind::CoordinationRead
                }
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
        {
            return PrismProgramEffectKind::ReusableArtifactEmission;
        }
        PrismProgramEffectKind::PureCompute
    }

    fn record_call_effect(&mut self, call: &CallExpr) {
        let method_path = call_path_from_callee(self, &call.callee);
        let effect_kind = method_path
            .as_deref()
            .map(Self::effect_kind_for_method_path)
            .unwrap_or(PrismProgramEffectKind::PureCompute);
        self.ir.add_effect(
            self.current_region(),
            effect_kind,
            source_span(self.parsed, call.span),
            method_path,
        );
    }

    fn analyze_program_ref(mut self, program: ProgramRef<'_>) -> PrismProgramIr {
        match program {
            ProgramRef::Module(module) => {
                for item in &module.body {
                    match item {
                        deno_ast::swc::ast::ModuleItem::ModuleDecl(decl) => {
                            self.analyze_module_decl(decl)
                        }
                        deno_ast::swc::ast::ModuleItem::Stmt(stmt) => self.analyze_stmt(stmt),
                    }
                }
            }
            ProgramRef::Script(script) => {
                for stmt in &script.body {
                    self.analyze_stmt(stmt);
                }
            }
        }
        self.ir
    }

    fn analyze_module_decl(&mut self, decl: &ModuleDecl) {
        match decl {
            ModuleDecl::Import(import) => self.analyze_import(import),
            ModuleDecl::ExportDecl(export) => self.analyze_decl(&export.decl),
            ModuleDecl::ExportDefaultDecl(export) => match &export.decl {
                deno_ast::swc::ast::DefaultDecl::Fn(fn_expr) => {
                    if let Some(ident) = &fn_expr.ident {
                        self.bind(
                            &ident.sym.to_string(),
                            PrismProgramBindingKind::Function,
                            ident.span,
                            None,
                        );
                    }
                    self.analyze_default_fn(fn_expr);
                }
                deno_ast::swc::ast::DefaultDecl::Class(_) => {}
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
            Stmt::Block(block) => {
                self.push_scope();
                for stmt in &block.stmts {
                    self.analyze_stmt(stmt);
                }
                self.pop_scope();
            }
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
            }
            Stmt::Throw(throw_stmt) => self.analyze_expr(&throw_stmt.arg),
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
            Decl::Class(class_decl) => {
                self.bind(
                    &class_decl.ident.sym.to_string(),
                    PrismProgramBindingKind::Function,
                    class_decl.ident.span,
                    None,
                );
            }
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
        self.analyze_function(&fn_decl.function, PrismProgramRegionKind::FunctionBoundary);
    }

    fn analyze_default_fn(&mut self, fn_expr: &FnExpr) {
        self.analyze_function(&fn_expr.function, PrismProgramRegionKind::FunctionBoundary);
    }

    fn analyze_var_decl(&mut self, kind: VarDeclKind, decl: &VarDeclarator) {
        let handle_kind = decl
            .init
            .as_deref()
            .and_then(|expr| infer_handle_kind(self, expr));
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
                        ObjectPatProp::Rest(rest) => {
                            self.bind_pattern(&rest.arg, kind, handle_kind)
                        }
                    }
                }
            }
            Pat::Rest(rest) => self.bind_pattern(&rest.arg, kind, handle_kind),
            Pat::Assign(assign) => self.bind_pattern(&assign.left, kind, handle_kind),
            _ => {}
        }
    }

    fn analyze_if(&mut self, if_stmt: &deno_ast::swc::ast::IfStmt) {
        self.with_region(PrismProgramRegionKind::Branch, if_stmt.span, |this| {
            this.analyze_expr(&if_stmt.test);
            this.analyze_stmt(&if_stmt.cons);
            if let Some(alt) = &if_stmt.alt {
                this.analyze_stmt(alt);
            }
        });
    }

    fn analyze_switch(&mut self, switch_stmt: &SwitchStmt) {
        self.with_region(PrismProgramRegionKind::Branch, switch_stmt.span, |this| {
            this.analyze_expr(&switch_stmt.discriminant);
            for case in &switch_stmt.cases {
                if let Some(test) = &case.test {
                    this.analyze_expr(test);
                }
                for stmt in &case.cons {
                    this.analyze_stmt(stmt);
                }
            }
        });
    }

    fn analyze_for(&mut self, for_stmt: &ForStmt) {
        self.with_region(PrismProgramRegionKind::Loop, for_stmt.span, |this| {
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
                this.analyze_expr(test);
            }
            if let Some(update) = &for_stmt.update {
                this.analyze_expr(update);
            }
            this.analyze_stmt(&for_stmt.body);
            this.pop_scope();
        });
    }

    fn analyze_for_of(&mut self, for_of: &ForOfStmt) {
        self.with_region(PrismProgramRegionKind::Loop, for_of.span, |this| {
            this.push_scope();
            this.bind_for_head(&for_of.left);
            this.analyze_expr(&for_of.right);
            this.analyze_stmt(&for_of.body);
            this.pop_scope();
        });
    }

    fn analyze_for_in(&mut self, for_in: &ForInStmt) {
        self.with_region(PrismProgramRegionKind::Loop, for_in.span, |this| {
            this.push_scope();
            this.bind_for_head(&for_in.left);
            this.analyze_expr(&for_in.right);
            this.analyze_stmt(&for_in.body);
            this.pop_scope();
        });
    }

    fn analyze_while(&mut self, while_stmt: &WhileStmt) {
        self.with_region(PrismProgramRegionKind::Loop, while_stmt.span, |this| {
            this.analyze_expr(&while_stmt.test);
            this.analyze_stmt(&while_stmt.body);
        });
    }

    fn analyze_do_while(&mut self, do_while: &DoWhileStmt) {
        self.with_region(PrismProgramRegionKind::Loop, do_while.span, |this| {
            this.analyze_stmt(&do_while.body);
            this.analyze_expr(&do_while.test);
        });
    }

    fn analyze_try(&mut self, try_stmt: &TryStmt) {
        self.with_region(
            PrismProgramRegionKind::TryCatchFinally,
            try_stmt.span,
            |this| {
                this.analyze_block_stmt(&try_stmt.block);
                if let Some(handler) = &try_stmt.handler {
                    this.analyze_catch(handler);
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    this.analyze_block_stmt(finalizer);
                }
            },
        );
    }

    fn analyze_catch(&mut self, catch_clause: &CatchClause) {
        self.push_scope();
        if let Some(param) = &catch_clause.param {
            self.bind_pattern(param, PrismProgramBindingKind::CatchParameter, None);
        }
        self.analyze_block_stmt(&catch_clause.body);
        self.pop_scope();
    }

    fn analyze_block_stmt(&mut self, block: &deno_ast::swc::ast::BlockStmt) {
        self.push_scope();
        for stmt in &block.stmts {
            self.analyze_stmt(stmt);
        }
        self.pop_scope();
    }

    fn bind_for_head(&mut self, left: &deno_ast::swc::ast::ForHead) {
        match left {
            deno_ast::swc::ast::ForHead::VarDecl(var_decl) => {
                for decl in &var_decl.decls {
                    self.analyze_var_decl(var_decl.kind, decl);
                }
            }
            deno_ast::swc::ast::ForHead::Pat(pat) => {
                self.bind_pattern(pat, PrismProgramBindingKind::LexicalLet, None);
            }
            deno_ast::swc::ast::ForHead::UsingDecl(_) => {}
        }
    }

    fn analyze_function(&mut self, function: &Function, kind: PrismProgramRegionKind) {
        self.with_function_region(kind, function.span, |this| {
            for param in &function.params {
                this.bind_param(param);
            }
            if let Some(body) = &function.body {
                this.analyze_block_stmt(body);
            }
        });
    }

    fn analyze_arrow(&mut self, arrow: &ArrowExpr, kind: PrismProgramRegionKind) {
        self.with_function_region(kind, arrow.span, |this| {
            for param in &arrow.params {
                this.bind_pattern(param, PrismProgramBindingKind::Parameter, None);
            }
            match &*arrow.body {
                BlockStmtOrExpr::BlockStmt(block) => this.analyze_block_stmt(block),
                BlockStmtOrExpr::Expr(expr) => this.analyze_expr(expr),
            }
        });
    }

    fn bind_param(&mut self, param: &Param) {
        self.bind_pattern(&param.pat, PrismProgramBindingKind::Parameter, None);
    }

    fn analyze_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(ident) => self.capture_if_needed(ident),
            Expr::Call(call) => self.analyze_call(call),
            Expr::OptChain(chain) => match &*chain.base {
                OptChainBase::Call(call) => self.analyze_opt_call(call),
                OptChainBase::Member(member) => self.analyze_member(member),
            },
            Expr::Await(await_expr) => self.analyze_expr(&await_expr.arg),
            Expr::Fn(fn_expr) => {
                if let Some(ident) = &fn_expr.ident {
                    self.bind(
                        &ident.sym.to_string(),
                        PrismProgramBindingKind::Function,
                        ident.span,
                        None,
                    );
                }
                self.analyze_function(&fn_expr.function, PrismProgramRegionKind::FunctionBoundary);
            }
            Expr::Arrow(arrow) => {
                self.analyze_arrow(arrow, PrismProgramRegionKind::FunctionBoundary)
            }
            Expr::Cond(cond) => {
                self.with_region(PrismProgramRegionKind::Branch, cond.span, |this| {
                    this.analyze_expr(&cond.test);
                    this.analyze_expr(&cond.cons);
                    this.analyze_expr(&cond.alt);
                })
            }
            Expr::Bin(bin) if is_short_circuit_op(bin.op) => {
                self.with_region(PrismProgramRegionKind::ShortCircuit, bin.span, |this| {
                    this.analyze_expr(&bin.left);
                    this.analyze_expr(&bin.right);
                });
            }
            Expr::Array(array) => self.analyze_array(array),
            Expr::Object(object) => {
                for prop in &object.props {
                    self.analyze_object_prop(prop);
                }
            }
            Expr::Assign(assign) => {
                self.analyze_pat_or_expr(&assign.left);
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
            Expr::Class(class_expr) => {
                if let Some(ident) = &class_expr.ident {
                    self.bind(
                        &ident.sym.to_string(),
                        PrismProgramBindingKind::Function,
                        ident.span,
                        None,
                    );
                }
                self.analyze_class_members(&class_expr.class.body);
            }
            Expr::New(new_expr) => {
                self.record_pure_effect_for_span(new_expr.span, None);
                self.analyze_expr(&new_expr.callee);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.analyze_expr(&arg.expr);
                    }
                }
            }
            _ => {}
        }
    }

    fn analyze_pat_or_expr(&mut self, left: &deno_ast::swc::ast::AssignTarget) {
        match left {
            deno_ast::swc::ast::AssignTarget::Simple(simple) => match simple {
                deno_ast::swc::ast::SimpleAssignTarget::Ident(ident) => {
                    self.capture_if_needed(&ident.id)
                }
                deno_ast::swc::ast::SimpleAssignTarget::Member(member) => {
                    self.analyze_member(member)
                }
                deno_ast::swc::ast::SimpleAssignTarget::Paren(paren) => {
                    self.analyze_expr(&paren.expr)
                }
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
                Prop::Assign(assign) => self.capture_if_needed(&assign.key),
                Prop::Getter(getter) => {
                    if let Some(body) = &getter.body {
                        self.analyze_block_stmt(body);
                    }
                }
                Prop::Setter(setter) => {
                    self.bind_pattern(&setter.param, PrismProgramBindingKind::Parameter, None);
                    if let Some(body) = &setter.body {
                        self.analyze_block_stmt(body);
                    }
                }
                Prop::Method(MethodProp { function, .. }) => {
                    self.analyze_function(function, PrismProgramRegionKind::FunctionBoundary);
                }
                _ => {}
            },
            PropOrSpread::Spread(spread) => self.analyze_expr(&spread.expr),
        }
    }

    fn analyze_class_members(&mut self, members: &[deno_ast::swc::ast::ClassMember]) {
        for member in members {
            match member {
                deno_ast::swc::ast::ClassMember::Constructor(Constructor {
                    body: Some(body),
                    ..
                }) => {
                    self.analyze_block_stmt(body);
                }
                deno_ast::swc::ast::ClassMember::Method(ClassMethod { function, .. }) => {
                    self.analyze_function(function, PrismProgramRegionKind::FunctionBoundary);
                }
                deno_ast::swc::ast::ClassMember::ClassProp(ClassProp {
                    value: Some(value),
                    ..
                }) => {
                    self.analyze_expr(value);
                }
                _ => {}
            }
        }
    }

    fn analyze_member(&mut self, member: &MemberExpr) {
        self.analyze_expr(&member.obj);
        if let MemberProp::Computed(computed) = &member.prop {
            self.analyze_expr(&computed.expr);
        }
    }

    fn analyze_call(&mut self, call: &CallExpr) {
        let call_path = call_path_from_callee(self, &call.callee);
        if let Some(path) = call_path.as_deref() {
            if let Some(region_kind) = region_kind_for_call_path(path) {
                self.with_region(region_kind, call.span, |this| {
                    this.record_call_effect(call);
                    this.analyze_call_children(call, path);
                });
                return;
            }
        }
        self.record_call_effect(call);
        self.analyze_call_children(call, call_path.as_deref().unwrap_or_default());
    }

    fn analyze_opt_call(&mut self, call: &deno_ast::swc::ast::OptCall) {
        let effect_kind = call_path_from_expr(self, &call.callee)
            .as_deref()
            .map(Self::effect_kind_for_method_path)
            .unwrap_or(PrismProgramEffectKind::PureCompute);
        self.ir.add_effect(
            self.current_region(),
            effect_kind,
            source_span(self.parsed, call.span),
            call_path_from_expr(self, &call.callee),
        );
        self.analyze_expr(&call.callee);
        for arg in &call.args {
            if is_callback_argument(arg) {
                match &*arg.expr {
                    Expr::Fn(fn_expr) => self.analyze_function(
                        &fn_expr.function,
                        PrismProgramRegionKind::CallbackBoundary,
                    ),
                    Expr::Arrow(arrow) => {
                        self.analyze_arrow(arrow, PrismProgramRegionKind::CallbackBoundary)
                    }
                    other => self.analyze_expr(other),
                }
            } else {
                self.analyze_expr(&arg.expr);
            }
        }
    }

    fn analyze_call_children(&mut self, call: &CallExpr, call_path: &str) {
        match &call.callee {
            Callee::Expr(expr) => self.analyze_expr(expr),
            Callee::Super(_) | Callee::Import(_) => {}
        }
        for arg in &call.args {
            if is_callback_argument(arg) {
                match &*arg.expr {
                    Expr::Fn(fn_expr) => self.analyze_function(
                        &fn_expr.function,
                        PrismProgramRegionKind::CallbackBoundary,
                    ),
                    Expr::Arrow(arrow) => {
                        let region_kind = callback_region_kind_for_call_path(call_path);
                        self.analyze_arrow(arrow, region_kind);
                    }
                    other => self.analyze_expr(other),
                }
            } else {
                self.analyze_expr(&arg.expr);
            }
        }
    }

    fn record_pure_effect_for_span(&mut self, span: Span, method_path: Option<String>) {
        self.ir.add_effect(
            self.current_region(),
            PrismProgramEffectKind::PureCompute,
            source_span(self.parsed, span),
            method_path,
        );
    }
}

fn is_callback_argument(arg: &ExprOrSpread) -> bool {
    matches!(&*arg.expr, Expr::Fn(_) | Expr::Arrow(_))
}

fn callback_region_kind_for_call_path(call_path: &str) -> PrismProgramRegionKind {
    if call_path.ends_with(".reduce") {
        PrismProgramRegionKind::Reduction
    } else {
        PrismProgramRegionKind::CallbackBoundary
    }
}

fn is_short_circuit_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
    )
}

fn region_kind_for_call_path(path: &str) -> Option<PrismProgramRegionKind> {
    match path {
        "Promise.all" | "Promise.allSettled" => Some(PrismProgramRegionKind::Parallel),
        "Promise.race" | "Promise.any" => Some(PrismProgramRegionKind::Competition),
        _ if path.ends_with(".map")
            || path.ends_with(".flatMap")
            || path.ends_with(".filter")
            || path.ends_with(".forEach") =>
        {
            Some(PrismProgramRegionKind::Parallel)
        }
        _ if path.ends_with(".reduce") => Some(PrismProgramRegionKind::Reduction),
        _ if path.ends_with(".some") || path.ends_with(".every") || path.ends_with(".find") => {
            Some(PrismProgramRegionKind::ShortCircuit)
        }
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
    use crate::prism_code_compiler::{
        prepare_typescript_program, PrismCodeCompilerInput, PrismTypescriptProgramMode,
    };
    use crate::QueryLanguage;

    fn analyze(code: &str) -> AnalyzedPrismProgram {
        let input = PrismCodeCompilerInput::inline("prism_code", code, QueryLanguage::Ts, true);
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");
        analyze_prepared_typescript_program(&prepared).expect("program should analyze")
    }

    #[test]
    fn analysis_tracks_bindings_effects_and_regions() {
        let analyzed = analyze(
            r#"
const plan = prism.coordination.createPlan({ title: "Ship" });
if (flag) {
  const task = plan.addTask({ title: "Build" });
  await task.complete({ summary: "done" });
}
"#,
        );
        let plan_binding = analyzed
            .ir
            .bindings
            .iter()
            .find(|binding| binding.name == "plan")
            .expect("plan binding should exist");
        assert_eq!(plan_binding.handle_kind, Some(PrismProgramHandleKind::Plan));
        assert_eq!(plan_binding.span.start_line, 2);
        assert!(analyzed
            .ir
            .bindings
            .iter()
            .any(|binding| binding.name == "task"));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::AuthoritativeWrite));
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::Branch));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.method_path.as_deref() == Some("plan.addTask")));
    }

    #[test]
    fn analysis_tracks_closure_captures_parallel_and_reduction_regions() {
        let analyzed = analyze(
            r#"
const plan = prism.coordination.createPlan({ title: "Ship" });
async function build(items) {
  await Promise.all(items.map(async (item) => plan.addTask({ title: item })));
  return items.reduce((count, item) => count + item.length, 0);
}
"#,
        );
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::FunctionBoundary));
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::Parallel));
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::Reduction));
        assert!(analyzed
            .ir
            .captures
            .iter()
            .any(|capture| capture.binding_name == "plan"));
        assert!(analyzed
            .ir
            .captures
            .iter()
            .all(|capture| capture.span.start_line > 0));
    }

    #[test]
    fn analysis_tracks_loop_try_and_short_circuit_regions() {
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
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::Loop));
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::TryCatchFinally));
        assert!(analyzed
            .ir
            .regions
            .iter()
            .any(|region| region.kind == PrismProgramRegionKind::ShortCircuit));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| { effect.kind == PrismProgramEffectKind::HostedDeterministicInput }));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::CoordinationRead));
        assert!(analyzed
            .ir
            .bindings
            .iter()
            .any(|binding| binding.kind == PrismProgramBindingKind::CatchParameter));
    }
}
