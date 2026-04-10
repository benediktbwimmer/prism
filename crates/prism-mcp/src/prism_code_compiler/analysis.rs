use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use deno_ast::swc::ast::{
    ArrayLit, ArrowExpr, AssignPatProp, BinaryOp, BlockStmt, BlockStmtOrExpr, CallExpr, Callee,
    CatchClause, ClassDecl, ClassExpr, ClassMember, ClassMethod, ClassProp, ComputedPropName,
    CondExpr, Constructor, Decl, DoWhileStmt, Expr, ExprOrSpread, FnDecl, FnExpr, ForHead,
    ForInStmt, ForOfStmt, ForStmt, Function, Ident, IfStmt, ImportDecl, ImportSpecifier,
    KeyValuePatProp, MemberExpr, MemberProp, MethodProp, ModuleDecl, ModuleExportName, NewExpr,
    ObjectPatProp, OptCall, OptChainBase, Param, Pat, PrivateMethod, PrivateProp, Prop,
    PropOrSpread, Stmt, SwitchStmt, VarDeclKind, VarDeclarator, WhileStmt,
};
use deno_ast::swc::common::Spanned;
use deno_ast::swc::common::{BytePos, Span};
use deno_ast::{
    parse_program, MediaType, ModuleSpecifier, ParseParams, ParsedSource, ProgramRef, SourcePos,
};
use prism_js::{prism_compiler_method_spec, PrismCompilerEffectKind};

use super::{
    program_ir::{
        PrismProgramBindingId, PrismProgramBindingKind, PrismProgramBranchKind,
        PrismProgramCallbackBoundaryKind, PrismProgramCompetitionKind,
        PrismProgramCompetitionSemantics, PrismProgramEffectKind, PrismProgramExitMode,
        PrismProgramFunctionBoundaryKind, PrismProgramHandleKind, PrismProgramInvocationId,
        PrismProgramInvocationKind, PrismProgramIr, PrismProgramLoopKind,
        PrismProgramOperationKind, PrismProgramParallelKind, PrismProgramReductionKind,
        PrismProgramRegionControl, PrismProgramRegionId, PrismProgramRegionKind,
        PrismProgramSequenceKind, PrismProgramShortCircuitKind, PrismProgramSourceSpan,
    },
    resolve_repo_module_import, PreparedTypescriptProgram, PrismCodeResolvedImport,
};

#[derive(Debug, Clone)]
pub(crate) struct AnalyzedPrismProgram {
    pub(crate) ir: PrismProgramIr,
}

#[derive(Debug, Clone)]
struct CallableRegionSummary {
    effect_kinds: Vec<PrismProgramEffectKind>,
    exit_modes: Vec<PrismProgramExitMode>,
}

#[derive(Debug, Clone)]
struct ScopeFrame {
    bindings: HashMap<String, PrismProgramBindingId>,
    function_depth: usize,
}

#[derive(Debug, Clone)]
struct ImportedBindingTarget {
    specifier: String,
    import_name: String,
    is_namespace: bool,
}

#[derive(Debug, Clone)]
struct ThisContext {
    class_binding_id: Option<PrismProgramBindingId>,
    is_static: bool,
}

#[derive(Debug, Clone)]
enum ClassTarget {
    Local {
        class_binding_id: PrismProgramBindingId,
        is_static: bool,
    },
    Imported {
        import_specifier: String,
        import_name: String,
        is_static: bool,
    },
}

struct ProgramAnalyzer<'a> {
    parsed: &'a ParsedSource,
    repo_module_path: Option<PathBuf>,
    ir: PrismProgramIr,
    scopes: Vec<ScopeFrame>,
    region_stack: Vec<PrismProgramRegionId>,
    function_region_stack: Vec<PrismProgramRegionId>,
    function_depth: usize,
    region_capture_keys: HashSet<(PrismProgramRegionId, PrismProgramBindingId)>,
    import_binding_targets: HashMap<PrismProgramBindingId, ImportedBindingTarget>,
    this_context_stack: Vec<Option<ThisContext>>,
    pending_label: Option<String>,
    errors: Vec<String>,
}

impl<'a> ProgramAnalyzer<'a> {
    fn new(
        parsed: &'a ParsedSource,
        display_path: impl Into<String>,
        repo_module_path: Option<PathBuf>,
    ) -> Self {
        let full_span = Span::new(
            BytePos(1),
            BytePos(parsed.text_info_lazy().text_str().len() as u32 + 1),
        );
        let root_span = source_span(parsed, full_span);
        let display_path = display_path.into();
        let mut ir = PrismProgramIr::new(parsed.specifier().to_string(), root_span);
        ir.register_module(parsed.specifier().to_string(), display_path.clone(), 0);
        Self {
            ir,
            parsed,
            repo_module_path,
            scopes: vec![ScopeFrame {
                bindings: HashMap::new(),
                function_depth: 0,
            }],
            region_stack: vec![0],
            function_region_stack: vec![0],
            function_depth: 0,
            region_capture_keys: HashSet::new(),
            import_binding_targets: HashMap::new(),
            this_context_stack: vec![None],
            pending_label: None,
            errors: Vec::new(),
        }
    }

    fn analyze_program_ref(mut self, program: ProgramRef<'_>) -> Result<PrismProgramIr> {
        let root_span = self.ir.regions[0].span.clone();
        self.with_sequence_region(
            root_span,
            PrismProgramSequenceKind::ModuleBody,
            |this, _| match program {
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
            },
        );
        if self.errors.is_empty() {
            Ok(self.ir)
        } else {
            Err(anyhow!(self.errors.join("\n")))
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

    fn reject_construct(&mut self, span: Span, message: impl Into<String>) {
        let span = source_span(self.parsed, span);
        self.errors.push(format!(
            "{}:{}:{}-{}:{}: {}",
            span.specifier,
            span.start_line,
            span.start_column,
            span.end_line,
            span.end_column,
            message.into()
        ));
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
    ) -> PrismProgramRegionId
    where
        F: FnOnce(&mut Self),
    {
        self.with_function_boundary_in_context(span, kind, name, None, body_kind, f)
    }

    fn with_function_boundary_in_context<F>(
        &mut self,
        span: Span,
        kind: PrismProgramFunctionBoundaryKind,
        name: Option<String>,
        this_context: Option<ThisContext>,
        body_kind: PrismProgramSequenceKind,
        f: F,
    ) -> PrismProgramRegionId
    where
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
        self.this_context_stack.push(this_context);
        self.push_scope();
        let sequence_span = self.ir.regions[boundary_id].span.clone();
        self.with_sequence_region(sequence_span, body_kind, |this, _| f(this));
        self.pop_scope();
        self.this_context_stack.pop();
        self.function_depth -= 1;
        self.function_region_stack.pop();
        self.region_stack.pop();
        boundary_id
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
        self.with_callback_boundary_in_context(span, kind, origin, None, f);
    }

    fn with_callback_boundary_in_context<F>(
        &mut self,
        span: Span,
        kind: PrismProgramCallbackBoundaryKind,
        origin: Option<String>,
        this_context: Option<ThisContext>,
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
        self.this_context_stack.push(this_context);
        self.push_scope();
        let sequence_span = self.ir.regions[boundary_id].span.clone();
        self.with_sequence_region(
            sequence_span,
            PrismProgramSequenceKind::CallbackBody,
            |this, _| f(this),
        );
        self.pop_scope();
        self.this_context_stack.pop();
        self.function_depth -= 1;
        self.function_region_stack.pop();
        self.region_stack.pop();
    }

    fn current_this_context(&self) -> Option<&ThisContext> {
        self.this_context_stack.last().and_then(Option::as_ref)
    }

    fn bind(
        &mut self,
        name: &str,
        kind: PrismProgramBindingKind,
        span: Span,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        self.bind_with_callability(name, kind, span, false, handle_kind)
    }

    fn bind_callable(
        &mut self,
        name: &str,
        kind: PrismProgramBindingKind,
        span: Span,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        self.bind_with_callability(name, kind, span, true, handle_kind)
    }

    fn bind_with_callability(
        &mut self,
        name: &str,
        kind: PrismProgramBindingKind,
        span: Span,
        is_callable: bool,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        let region_id = self.current_region();
        let binding_id = self.ir.add_binding(
            region_id,
            name.to_string(),
            kind,
            source_span(self.parsed, span),
            is_callable,
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
            None,
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
            None,
        );
    }

    fn record_guard(&mut self, span: Span, binding_ids: Vec<PrismProgramBindingId>) {
        let region_id = self.current_region();
        self.ir
            .set_guard_span(region_id, source_span(self.parsed, span));
        self.ir.add_operation(
            region_id,
            PrismProgramOperationKind::GuardEvaluation,
            source_span(self.parsed, span),
            None,
            binding_ids,
            None,
        );
    }

    fn record_await_boundary(&mut self, span: Span) {
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::AwaitBoundary,
            source_span(self.parsed, span),
            None,
            Vec::new(),
            None,
        );
    }

    fn record_exit(
        &mut self,
        kind: PrismProgramOperationKind,
        span: Span,
        target_label: Option<String>,
    ) {
        let exit_mode = match kind {
            PrismProgramOperationKind::Return => PrismProgramExitMode::Return,
            PrismProgramOperationKind::Throw => PrismProgramExitMode::Throw,
            PrismProgramOperationKind::Break => PrismProgramExitMode::Break,
            PrismProgramOperationKind::Continue => PrismProgramExitMode::Continue,
            PrismProgramOperationKind::GuardEvaluation
            | PrismProgramOperationKind::AwaitBoundary
            | PrismProgramOperationKind::BindingRead
            | PrismProgramOperationKind::BindingWrite
            | PrismProgramOperationKind::Invocation
            | PrismProgramOperationKind::PureCompute => PrismProgramExitMode::Normal,
        };
        let span = source_span(self.parsed, span);
        self.ir.add_operation(
            self.current_region(),
            kind,
            span,
            None,
            Vec::new(),
            target_label,
        );
        for region_id in self.region_stack.iter().copied() {
            self.ir.record_exit_mode(region_id, exit_mode);
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
            || path.starts_with("prism.code.compile")
        {
            return PrismProgramEffectKind::ReusableArtifactEmission;
        }
        if path.starts_with("prism.") {
            return PrismProgramEffectKind::CoordinationRead;
        }
        PrismProgramEffectKind::PureCompute
    }

    fn resolve_import_target_for_binding(
        &self,
        binding_id: PrismProgramBindingId,
    ) -> Option<&ImportedBindingTarget> {
        self.import_binding_targets.get(&binding_id)
    }

    fn resolve_invocation(
        &self,
        callee: &Expr,
        method_path: Option<&str>,
    ) -> (
        PrismProgramInvocationKind,
        Option<PrismProgramBindingId>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<bool>,
    ) {
        if let Some(path) = method_path {
            if path.starts_with("prism.")
                || matches!(
                    path,
                    "Promise.all"
                        | "Promise.allSettled"
                        | "Promise.any"
                        | "Promise.race"
                        | "Promise.resolve"
                        | "Promise.reject"
                )
                || path.starts_with("plan.")
                || path.starts_with("task.")
                || path.starts_with("claim.")
                || path.starts_with("artifact.")
                || path.starts_with("review.")
                || path.starts_with("work.")
            {
                return (
                    PrismProgramInvocationKind::SdkMethod,
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
        }
        match callee {
            Expr::Ident(ident) => {
                if let Some((binding_id, _)) = self.resolve_binding(&ident.sym.to_string()) {
                    if let Some(target) = self.resolve_import_target_for_binding(binding_id) {
                        return (
                            PrismProgramInvocationKind::ImportedFunction,
                            Some(binding_id),
                            Some(target.specifier.clone()),
                            Some(target.import_name.clone()),
                            None,
                            None,
                        );
                    }
                    let binding = &self.ir.bindings[binding_id];
                    if binding.is_callable {
                        return (
                            PrismProgramInvocationKind::LocalFunction,
                            Some(binding_id),
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                }
            }
            Expr::Member(member) => {
                let member_name = member_prop_name(&member.prop);
                if let Some(method_name) = member_name.as_deref() {
                    if let Some(target) = self.resolve_method_target(&member.obj, method_name) {
                        return match target {
                            ClassTarget::Local {
                                class_binding_id,
                                is_static,
                            } => (
                                PrismProgramInvocationKind::LocalMethod,
                                Some(class_binding_id),
                                None,
                                None,
                                Some(method_name.to_string()),
                                Some(is_static),
                            ),
                            ClassTarget::Imported {
                                import_specifier,
                                import_name,
                                is_static,
                            } => (
                                PrismProgramInvocationKind::ImportedMethod,
                                None,
                                Some(import_specifier),
                                Some(import_name),
                                Some(method_name.to_string()),
                                Some(is_static),
                            ),
                        };
                    }
                }
                if let Expr::Ident(object_ident) = &*member.obj {
                    if let Some((binding_id, _)) =
                        self.resolve_binding(&object_ident.sym.to_string())
                    {
                        if let Some(target) = self.resolve_import_target_for_binding(binding_id) {
                            if target.is_namespace {
                                return (
                                    PrismProgramInvocationKind::NamespaceImportMethod,
                                    Some(binding_id),
                                    Some(target.specifier.clone()),
                                    Some(target.import_name.clone()),
                                    member_name,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        (
            PrismProgramInvocationKind::DynamicCall,
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn resolve_method_target(&self, object: &Expr, method_name: &str) -> Option<ClassTarget> {
        match object {
            Expr::Ident(ident) => {
                let (binding_id, _) = self.resolve_binding(&ident.sym.to_string())?;
                let binding = self.ir.bindings.get(binding_id)?;
                if let Some(class_binding_id) = binding.instance_class_binding_id {
                    return self
                        .class_method_region_for_local_class(class_binding_id, method_name, false)
                        .map(|_| ClassTarget::Local {
                            class_binding_id,
                            is_static: false,
                        });
                }
                if let Some(import_specifier) = binding.instance_import_specifier.as_ref() {
                    let import_name = binding.instance_import_name.as_ref()?;
                    return Some(ClassTarget::Imported {
                        import_specifier: import_specifier.clone(),
                        import_name: import_name.clone(),
                        is_static: false,
                    });
                }
                if self
                    .class_method_region_for_local_class(binding_id, method_name, true)
                    .is_some()
                {
                    return Some(ClassTarget::Local {
                        class_binding_id: binding_id,
                        is_static: true,
                    });
                }
                if let Some(import_target) = self.resolve_import_target_for_binding(binding_id) {
                    if !import_target.is_namespace {
                        return Some(ClassTarget::Imported {
                            import_specifier: import_target.specifier.clone(),
                            import_name: import_target.import_name.clone(),
                            is_static: true,
                        });
                    }
                }
                None
            }
            Expr::Member(member) => {
                let class_name = member_prop_name(&member.prop)?;
                match &*member.obj {
                    Expr::Ident(namespace_ident) => {
                        let (binding_id, _) =
                            self.resolve_binding(&namespace_ident.sym.to_string())?;
                        let import_target = self.resolve_import_target_for_binding(binding_id)?;
                        if !import_target.is_namespace {
                            return None;
                        }
                        Some(ClassTarget::Imported {
                            import_specifier: import_target.specifier.clone(),
                            import_name: class_name,
                            is_static: true,
                        })
                    }
                    _ => None,
                }
            }
            Expr::This(_) => {
                let this_context = self.current_this_context()?;
                if let Some(class_binding_id) = this_context.class_binding_id {
                    return self
                        .class_method_region_for_local_class(
                            class_binding_id,
                            method_name,
                            this_context.is_static,
                        )
                        .map(|_| ClassTarget::Local {
                            class_binding_id,
                            is_static: this_context.is_static,
                        });
                }
                None
            }
            Expr::New(new_expr) => {
                let class_target = self.resolve_constructor_target(&new_expr.callee)?;
                match class_target {
                    ClassTarget::Local {
                        class_binding_id, ..
                    } => self
                        .class_method_region_for_local_class(class_binding_id, method_name, false)
                        .map(|_| ClassTarget::Local {
                            class_binding_id,
                            is_static: false,
                        }),
                    ClassTarget::Imported {
                        import_specifier,
                        import_name,
                        ..
                    } => Some(ClassTarget::Imported {
                        import_specifier,
                        import_name,
                        is_static: false,
                    }),
                }
            }
            Expr::Paren(paren) => self.resolve_method_target(&paren.expr, method_name),
            _ => None,
        }
    }

    fn resolve_constructor_target(&self, callee: &Expr) -> Option<ClassTarget> {
        match callee {
            Expr::Ident(ident) => {
                let (binding_id, _) = self.resolve_binding(&ident.sym.to_string())?;
                if self.class_declares_methods(binding_id) {
                    return Some(ClassTarget::Local {
                        class_binding_id: binding_id,
                        is_static: true,
                    });
                }
                let import_target = self.resolve_import_target_for_binding(binding_id)?;
                if import_target.is_namespace {
                    return None;
                }
                Some(ClassTarget::Imported {
                    import_specifier: import_target.specifier.clone(),
                    import_name: import_target.import_name.clone(),
                    is_static: true,
                })
            }
            Expr::Member(member) => {
                let class_name = member_prop_name(&member.prop)?;
                if let Expr::Ident(namespace_ident) = &*member.obj {
                    let (binding_id, _) = self.resolve_binding(&namespace_ident.sym.to_string())?;
                    let import_target = self.resolve_import_target_for_binding(binding_id)?;
                    if !import_target.is_namespace {
                        return None;
                    }
                    Some(ClassTarget::Imported {
                        import_specifier: import_target.specifier.clone(),
                        import_name: class_name,
                        is_static: true,
                    })
                } else {
                    None
                }
            }
            Expr::Paren(paren) => self.resolve_constructor_target(&paren.expr),
            _ => None,
        }
    }

    fn class_declares_methods(&self, class_binding_id: PrismProgramBindingId) -> bool {
        self.ir
            .class_methods
            .iter()
            .any(|method| method.class_binding_id == Some(class_binding_id))
    }
    fn class_method_region_for_local_class(
        &self,
        class_binding_id: PrismProgramBindingId,
        method_name: &str,
        is_static: bool,
    ) -> Option<PrismProgramRegionId> {
        self.ir
            .class_methods
            .iter()
            .find(|method| {
                method.class_binding_id == Some(class_binding_id)
                    && method.method_name == method_name
                    && method.is_static == is_static
            })
            .map(|method| method.region_id)
    }

    fn record_call_effect(&mut self, call_span: Span, callee: &Expr, method_path: Option<String>) {
        let effect_kind = method_path
            .as_deref()
            .map(Self::effect_kind_for_method_path)
            .unwrap_or(PrismProgramEffectKind::PureCompute);
        let (
            invocation_kind,
            callee_binding_id,
            import_specifier,
            import_name,
            namespace_member,
            class_method_is_static,
        ) = self.resolve_invocation(callee, method_path.as_deref());
        self.ir.add_effect(
            self.current_region(),
            effect_kind,
            source_span(self.parsed, call_span),
            method_path.clone(),
        );
        self.ir.add_invocation(
            self.current_region(),
            invocation_kind,
            source_span(self.parsed, call_span),
            method_path.clone(),
            callee_binding_id,
            import_specifier,
            import_name,
            namespace_member,
            class_method_is_static,
            effect_kind,
        );
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::Invocation,
            source_span(self.parsed, call_span),
            method_path,
            callee_binding_id.into_iter().collect(),
            None,
        );
    }

    fn analyze_module_decl(&mut self, decl: &ModuleDecl) {
        match decl {
            ModuleDecl::Import(import) => self.analyze_import(import),
            ModuleDecl::ExportDecl(export) => self.analyze_export_decl(&export.decl),
            ModuleDecl::ExportNamed(named) => self.analyze_named_export(named),
            ModuleDecl::ExportAll(all) => self.analyze_export_all(all),
            ModuleDecl::ExportDefaultDecl(export) => match &export.decl {
                deno_ast::swc::ast::DefaultDecl::Fn(fn_expr) => {
                    let (binding_id, region_id) = self.analyze_default_fn(fn_expr);
                    self.ir.add_export(
                        self.parsed.specifier().to_string(),
                        "default",
                        binding_id,
                        Some(region_id),
                    );
                }
                deno_ast::swc::ast::DefaultDecl::Class(class) => {
                    let region_id = self.analyze_class_decl_body(None, None, &class.class);
                    self.ir.add_export(
                        self.parsed.specifier().to_string(),
                        "default",
                        None,
                        region_id,
                    );
                }
                _ => {}
            },
            ModuleDecl::ExportDefaultExpr(expr) => {
                let target_region_id = match &*expr.expr {
                    Expr::Fn(fn_expr) => Some(self.analyze_default_fn(fn_expr).1),
                    Expr::Arrow(arrow) => Some(self.analyze_arrow_function(arrow)),
                    Expr::Class(class_expr) => self.analyze_class_expr(class_expr, None),
                    _ => {
                        self.analyze_expr(&expr.expr);
                        None
                    }
                };
                self.ir.add_export(
                    self.parsed.specifier().to_string(),
                    "default",
                    None,
                    target_region_id,
                );
            }
            _ => {}
        }
    }

    fn analyze_named_export(&mut self, export: &deno_ast::swc::ast::NamedExport) {
        if let Some(src) = &export.src {
            let resolved_import = self.repo_module_path.as_deref().and_then(|module_path| {
                resolve_repo_module_import(module_path, &src.value.to_atom_lossy()).ok()
            });
            let Some(resolved_import) = resolved_import else {
                return;
            };
            let specifier = match resolved_import {
                PrismCodeResolvedImport::RepoModule(path) => {
                    format!(
                        "file:///prism/code/{}",
                        path.to_string_lossy().replace('\\', "/")
                    )
                }
                PrismCodeResolvedImport::RuntimeModule(specifier) => specifier,
            };
            for specifier_entry in &export.specifiers {
                match specifier_entry {
                    deno_ast::swc::ast::ExportSpecifier::Named(named_specifier) => {
                        let import_name =
                            import_name_from_module_export_name(&named_specifier.orig);
                        let export_name = named_specifier
                            .exported
                            .as_ref()
                            .map(import_name_from_module_export_name)
                            .unwrap_or_else(|| import_name.clone());
                        self.ir.add_reexport(
                            self.parsed.specifier().to_string(),
                            export_name,
                            specifier.clone(),
                            import_name,
                        );
                    }
                    deno_ast::swc::ast::ExportSpecifier::Default(default_specifier) => {
                        self.ir.add_reexport(
                            self.parsed.specifier().to_string(),
                            default_specifier.exported.sym.to_string(),
                            specifier.clone(),
                            "default",
                        );
                    }
                    deno_ast::swc::ast::ExportSpecifier::Namespace(namespace_specifier) => {
                        let export_name = match &namespace_specifier.name {
                            ModuleExportName::Ident(ident) => ident.sym.to_string(),
                            ModuleExportName::Str(value) => value.value.to_atom_lossy().to_string(),
                        };
                        self.ir.add_reexport(
                            self.parsed.specifier().to_string(),
                            export_name,
                            specifier.clone(),
                            "*",
                        );
                    }
                }
            }
            return;
        }

        for specifier in &export.specifiers {
            match specifier {
                deno_ast::swc::ast::ExportSpecifier::Named(named_specifier) => {
                    let local_name = import_name_from_module_export_name(&named_specifier.orig);
                    if let Some((binding_id, _)) = self.resolve_binding(&local_name) {
                        let export_name = named_specifier
                            .exported
                            .as_ref()
                            .map(import_name_from_module_export_name)
                            .unwrap_or_else(|| local_name.clone());
                        let target_region_id = self.ir.bindings[binding_id].callable_region_id;
                        self.ir.add_export(
                            self.parsed.specifier().to_string(),
                            export_name,
                            Some(binding_id),
                            target_region_id,
                        );
                    }
                }
                deno_ast::swc::ast::ExportSpecifier::Default(default_specifier) => {
                    let local_name = default_specifier.exported.sym.to_string();
                    if let Some((binding_id, _)) = self.resolve_binding(&local_name) {
                        let target_region_id = self.ir.bindings[binding_id].callable_region_id;
                        self.ir.add_export(
                            self.parsed.specifier().to_string(),
                            "default",
                            Some(binding_id),
                            target_region_id,
                        );
                    }
                }
                deno_ast::swc::ast::ExportSpecifier::Namespace(_) => {}
            }
        }
    }

    fn analyze_export_all(&mut self, export: &deno_ast::swc::ast::ExportAll) {
        let resolved_import = self.repo_module_path.as_deref().and_then(|module_path| {
            resolve_repo_module_import(module_path, &export.src.value.to_atom_lossy()).ok()
        });
        let Some(resolved_import) = resolved_import else {
            return;
        };
        let specifier = match resolved_import {
            PrismCodeResolvedImport::RepoModule(path) => {
                format!(
                    "file:///prism/code/{}",
                    path.to_string_lossy().replace('\\', "/")
                )
            }
            PrismCodeResolvedImport::RuntimeModule(specifier) => specifier,
        };
        self.ir
            .add_reexport(self.parsed.specifier().to_string(), "*", specifier, "*");
    }

    fn analyze_export_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(fn_decl) => {
                let binding_id = self.analyze_fn_decl(fn_decl);
                let target_region_id = self.ir.bindings[binding_id].callable_region_id;
                self.ir.add_export(
                    self.parsed.specifier().to_string(),
                    fn_decl.ident.sym.to_string(),
                    Some(binding_id),
                    target_region_id,
                );
            }
            Decl::Var(var_decl) => {
                for decl in &var_decl.decls {
                    let binding_ids = self.analyze_var_decl(var_decl.kind, decl);
                    for binding_id in binding_ids {
                        let target_region_id = self.ir.bindings[binding_id].callable_region_id;
                        self.ir.add_export(
                            self.parsed.specifier().to_string(),
                            self.ir.bindings[binding_id].name.clone(),
                            Some(binding_id),
                            target_region_id,
                        );
                    }
                }
            }
            Decl::Class(class_decl) => {
                let binding_id = self.analyze_class_decl(class_decl);
                let target_region_id = self.ir.bindings[binding_id].callable_region_id;
                self.ir.add_export(
                    self.parsed.specifier().to_string(),
                    class_decl.ident.sym.to_string(),
                    Some(binding_id),
                    target_region_id,
                );
            }
            _ => self.analyze_decl(decl),
        }
    }

    fn analyze_import(&mut self, import: &ImportDecl) {
        let raw_specifier = import.src.value.to_atom_lossy().to_string();
        if let Some(reason) = forbidden_runtime_module_reason(&raw_specifier) {
            self.reject_construct(import.span, reason);
        }
        let resolved_import = self.repo_module_path.as_deref().and_then(|module_path| {
            resolve_repo_module_import(module_path, &import.src.value.to_atom_lossy()).ok()
        });
        if let Some(PrismCodeResolvedImport::RuntimeModule(specifier)) = resolved_import.as_ref() {
            if let Some(reason) = forbidden_runtime_module_reason(specifier) {
                self.reject_construct(import.span, reason);
            }
        }
        for specifier in &import.specifiers {
            let (binding_id, import_name, is_namespace) = match specifier {
                ImportSpecifier::Default(default) => (
                    self.bind(
                        &default.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        default.local.span,
                        None,
                    ),
                    "default".to_string(),
                    false,
                ),
                ImportSpecifier::Named(named) => (
                    self.bind(
                        &named.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        named.local.span,
                        None,
                    ),
                    named
                        .imported
                        .as_ref()
                        .map(import_name_from_module_export_name)
                        .unwrap_or_else(|| named.local.sym.to_string()),
                    false,
                ),
                ImportSpecifier::Namespace(namespace) => (
                    self.bind(
                        &namespace.local.sym.to_string(),
                        PrismProgramBindingKind::Import,
                        namespace.local.span,
                        None,
                    ),
                    "*".to_string(),
                    true,
                ),
            };
            if let Some(target) = resolved_import.as_ref() {
                let specifier = match target {
                    PrismCodeResolvedImport::RepoModule(path) => {
                        format!(
                            "file:///prism/code/{}",
                            path.to_string_lossy().replace('\\', "/")
                        )
                    }
                    PrismCodeResolvedImport::RuntimeModule(specifier) => specifier.clone(),
                };
                self.import_binding_targets.insert(
                    binding_id,
                    ImportedBindingTarget {
                        specifier,
                        import_name,
                        is_namespace,
                    },
                );
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
                self.record_exit(PrismProgramOperationKind::Return, return_stmt.span, None);
            }
            Stmt::Throw(throw_stmt) => {
                self.analyze_expr(&throw_stmt.arg);
                self.record_exit(PrismProgramOperationKind::Throw, throw_stmt.span, None);
            }
            Stmt::Break(break_stmt) => self.record_exit(
                PrismProgramOperationKind::Break,
                break_stmt.span,
                break_stmt.label.as_ref().map(|label| label.sym.to_string()),
            ),
            Stmt::Continue(continue_stmt) => self.record_exit(
                PrismProgramOperationKind::Continue,
                continue_stmt.span,
                continue_stmt
                    .label
                    .as_ref()
                    .map(|label| label.sym.to_string()),
            ),
            Stmt::Labeled(labeled) => {
                let previous = self.pending_label.replace(labeled.label.sym.to_string());
                self.analyze_stmt(&labeled.body);
                self.pending_label = previous;
            }
            Stmt::With(with_stmt) => {
                self.reject_construct(
                    with_stmt.span,
                    "`with` is not allowed because it destroys analyzability",
                );
            }
            _ => {}
        }
    }

    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(fn_decl) => {
                self.analyze_fn_decl(fn_decl);
            }
            Decl::Var(var_decl) => {
                for decl in &var_decl.decls {
                    self.analyze_var_decl(var_decl.kind, decl);
                }
            }
            Decl::Class(class_decl) => {
                self.analyze_class_decl(class_decl);
            }
            _ => {}
        }
    }

    fn analyze_fn_decl(&mut self, fn_decl: &FnDecl) -> PrismProgramBindingId {
        let binding_id = self.bind_callable(
            &fn_decl.ident.sym.to_string(),
            PrismProgramBindingKind::Function,
            fn_decl.ident.span,
            None,
        );
        let region_id = self.analyze_function(
            &fn_decl.function,
            PrismProgramFunctionBoundaryKind::NamedFunction,
            Some(fn_decl.ident.sym.to_string()),
        );
        self.ir.set_binding_callable_region(binding_id, region_id);
        binding_id
    }

    fn analyze_default_fn(
        &mut self,
        fn_expr: &FnExpr,
    ) -> (Option<PrismProgramBindingId>, PrismProgramRegionId) {
        let binding_id = fn_expr.ident.as_ref().map(|ident| {
            self.bind_callable(
                &ident.sym.to_string(),
                PrismProgramBindingKind::Function,
                ident.span,
                None,
            )
        });
        let name = fn_expr.ident.as_ref().map(|ident| ident.sym.to_string());
        let region_id = self.analyze_function(
            &fn_expr.function,
            if name.is_some() {
                PrismProgramFunctionBoundaryKind::NamedFunction
            } else {
                PrismProgramFunctionBoundaryKind::AnonymousFunction
            },
            name,
        );
        if let Some(binding_id) = binding_id {
            self.ir.set_binding_callable_region(binding_id, region_id);
        }
        (binding_id, region_id)
    }

    fn analyze_class_decl(&mut self, class_decl: &ClassDecl) -> PrismProgramBindingId {
        let binding_id = self.bind_callable(
            &class_decl.ident.sym.to_string(),
            PrismProgramBindingKind::Function,
            class_decl.ident.span,
            None,
        );
        if let Some(region_id) = self.analyze_class_decl_body(
            Some(binding_id),
            Some(class_decl.ident.sym.to_string()),
            &class_decl.class,
        ) {
            self.ir.set_binding_callable_region(binding_id, region_id);
        }
        binding_id
    }

    fn analyze_class_decl_body(
        &mut self,
        class_binding_id: Option<PrismProgramBindingId>,
        _name: Option<String>,
        class: &deno_ast::swc::ast::Class,
    ) -> Option<PrismProgramRegionId> {
        let mut constructor_region_id = None;
        for member in &class.body {
            match member {
                ClassMember::Constructor(Constructor {
                    body: Some(body),
                    span,
                    ..
                }) => {
                    let region_id = self.with_function_boundary_in_context(
                        *span,
                        PrismProgramFunctionBoundaryKind::Constructor,
                        Some("constructor".to_string()),
                        Some(ThisContext {
                            class_binding_id,
                            is_static: false,
                        }),
                        PrismProgramSequenceKind::FunctionBody,
                        |this| this.analyze_block_contents(body),
                    );
                    constructor_region_id = Some(region_id);
                }
                ClassMember::Method(ClassMethod {
                    key,
                    function,
                    is_static,
                    ..
                }) => {
                    let name = class_member_name(key);
                    let region_id = self.with_function_boundary_in_context(
                        function.span,
                        PrismProgramFunctionBoundaryKind::ClassMethod,
                        name.clone(),
                        Some(ThisContext {
                            class_binding_id,
                            is_static: *is_static,
                        }),
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
                    if let Some(method_name) = class_member_name(key) {
                        self.ir.add_class_method(
                            class_binding_id,
                            self.parsed.specifier().to_string(),
                            method_name,
                            *is_static,
                            region_id,
                        );
                    }
                }
                ClassMember::PrivateMethod(PrivateMethod {
                    key,
                    function,
                    is_static,
                    ..
                }) => {
                    let name = Some(format!("#{}", key.name));
                    let region_id = self.with_function_boundary_in_context(
                        function.span,
                        PrismProgramFunctionBoundaryKind::ClassMethod,
                        name.clone(),
                        Some(ThisContext {
                            class_binding_id,
                            is_static: *is_static,
                        }),
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
                    self.ir.add_class_method(
                        class_binding_id,
                        self.parsed.specifier().to_string(),
                        format!("#{}", key.name),
                        *is_static,
                        region_id,
                    );
                }
                ClassMember::ClassProp(ClassProp {
                    value: Some(value), ..
                }) => {
                    self.analyze_expr(value);
                }
                ClassMember::PrivateProp(PrivateProp {
                    value: Some(value), ..
                }) => {
                    self.analyze_expr(value);
                }
                _ => {}
            }
        }
        constructor_region_id
    }

    fn analyze_var_decl(
        &mut self,
        kind: VarDeclKind,
        decl: &VarDeclarator,
    ) -> Vec<PrismProgramBindingId> {
        let handle_kind = decl
            .init
            .as_deref()
            .and_then(|expr| infer_handle_kind(self, expr));
        let is_callable = decl
            .init
            .as_deref()
            .is_some_and(|expr| matches!(expr, Expr::Fn(_) | Expr::Arrow(_) | Expr::Class(_)));
        let binding_kind = match kind {
            VarDeclKind::Const => PrismProgramBindingKind::LexicalConst,
            VarDeclKind::Let => PrismProgramBindingKind::LexicalLet,
            VarDeclKind::Var => PrismProgramBindingKind::LexicalVar,
        };
        let binding_ids =
            self.bind_pattern_collect(&decl.name, binding_kind, is_callable, handle_kind);
        if let Some(init) = &decl.init {
            match &**init {
                Expr::Fn(fn_expr) => {
                    let region_id = self.analyze_function(
                        &fn_expr.function,
                        if fn_expr.ident.is_some() {
                            PrismProgramFunctionBoundaryKind::NamedFunction
                        } else {
                            PrismProgramFunctionBoundaryKind::AnonymousFunction
                        },
                        fn_expr.ident.as_ref().map(|ident| ident.sym.to_string()),
                    );
                    if binding_ids.len() == 1 {
                        self.ir
                            .set_binding_callable_region(binding_ids[0], region_id);
                    }
                }
                Expr::Arrow(arrow) => {
                    let region_id = self.analyze_arrow_function(arrow);
                    if binding_ids.len() == 1 {
                        self.ir
                            .set_binding_callable_region(binding_ids[0], region_id);
                    }
                }
                Expr::Class(class_expr) => {
                    let region_id = self.analyze_class_expr(
                        class_expr,
                        (binding_ids.len() == 1).then_some(binding_ids[0]),
                    );
                    if binding_ids.len() == 1 {
                        if let Some(region_id) = region_id {
                            self.ir
                                .set_binding_callable_region(binding_ids[0], region_id);
                        }
                    }
                }
                Expr::New(new_expr) => {
                    self.analyze_new_expr(new_expr);
                    if binding_ids.len() == 1 {
                        if let Some(class_target) =
                            self.resolve_constructor_target(&new_expr.callee)
                        {
                            match class_target {
                                ClassTarget::Local {
                                    class_binding_id, ..
                                } => self.ir.set_binding_instance_class(
                                    binding_ids[0],
                                    Some(class_binding_id),
                                    None,
                                    None,
                                ),
                                ClassTarget::Imported {
                                    import_specifier,
                                    import_name,
                                    ..
                                } => self.ir.set_binding_instance_class(
                                    binding_ids[0],
                                    None,
                                    Some(import_specifier),
                                    Some(import_name),
                                ),
                            }
                        }
                    }
                }
                _ => self.analyze_expr(init),
            }
        }
        binding_ids
    }

    fn bind_pattern(
        &mut self,
        pat: &Pat,
        kind: PrismProgramBindingKind,
        is_callable: bool,
        handle_kind: Option<PrismProgramHandleKind>,
    ) {
        self.bind_pattern_collect(pat, kind, is_callable, handle_kind);
    }

    fn bind_pattern_collect(
        &mut self,
        pat: &Pat,
        kind: PrismProgramBindingKind,
        is_callable: bool,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> Vec<PrismProgramBindingId> {
        let mut binding_ids = Vec::new();
        match pat {
            Pat::Ident(ident) => {
                binding_ids.push(self.bind_with_callability(
                    &ident.id.sym.to_string(),
                    kind,
                    ident.id.span,
                    is_callable,
                    handle_kind,
                ));
            }
            Pat::Array(array) => {
                for elem in array.elems.iter().flatten() {
                    binding_ids.extend(self.bind_pattern_collect(
                        elem,
                        kind,
                        is_callable,
                        handle_kind,
                    ));
                }
            }
            Pat::Object(object) => {
                for prop in &object.props {
                    match prop {
                        ObjectPatProp::Assign(AssignPatProp { key, .. }) => {
                            binding_ids.push(self.bind_with_callability(
                                &key.sym.to_string(),
                                kind,
                                key.span,
                                is_callable,
                                handle_kind,
                            ));
                        }
                        ObjectPatProp::KeyValue(KeyValuePatProp { value, .. }) => {
                            binding_ids.extend(self.bind_pattern_collect(
                                value,
                                kind,
                                is_callable,
                                handle_kind,
                            ));
                        }
                        ObjectPatProp::Rest(rest) => binding_ids.extend(self.bind_pattern_collect(
                            &rest.arg,
                            kind,
                            is_callable,
                            handle_kind,
                        )),
                    }
                }
            }
            Pat::Rest(rest) => binding_ids.extend(self.bind_pattern_collect(
                &rest.arg,
                kind,
                is_callable,
                handle_kind,
            )),
            Pat::Assign(assign) => binding_ids.extend(self.bind_pattern_collect(
                &assign.left,
                kind,
                is_callable,
                handle_kind,
            )),
            _ => {}
        }
        binding_ids
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
                    None,
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
                    None,
                );
                this.analyze_expr(&switch_stmt.discriminant);
                for case in &switch_stmt.cases {
                    let case_span = source_span(this.parsed, case.span);
                    this.with_sequence_region(
                        case_span,
                        PrismProgramSequenceKind::SwitchCase,
                        |inner, _| {
                            if let Some(test) = &case.test {
                                inner.record_guard(
                                    test.span(),
                                    collect_expr_binding_reads(inner, test),
                                );
                                inner.analyze_expr(test);
                            }
                            for stmt in &case.cons {
                                inner.analyze_stmt(stmt);
                            }
                        },
                    );
                }
            },
        );
    }

    fn analyze_for(&mut self, for_stmt: &ForStmt) {
        let label = self.pending_label.take();
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::For,
                is_async: false,
                label,
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
        let label = self.pending_label.take();
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForOf,
                is_async: for_of.is_await,
                label,
            },
            for_of.span,
            |this, region_id| {
                this.push_scope();
                this.bind_for_head(&for_of.left);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, for_of.right.span()));
                this.record_guard(
                    for_of.right.span(),
                    collect_expr_binding_reads(this, &for_of.right),
                );
                this.analyze_expr(&for_of.right);
                this.analyze_stmt(&for_of.body);
                this.pop_scope();
            },
        );
    }

    fn analyze_for_in(&mut self, for_in: &ForInStmt) {
        let label = self.pending_label.take();
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForIn,
                is_async: false,
                label,
            },
            for_in.span,
            |this, region_id| {
                this.push_scope();
                this.bind_for_head(&for_in.left);
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, for_in.right.span()));
                this.record_guard(
                    for_in.right.span(),
                    collect_expr_binding_reads(this, &for_in.right),
                );
                this.analyze_expr(&for_in.right);
                this.analyze_stmt(&for_in.body);
                this.pop_scope();
            },
        );
    }

    fn analyze_while(&mut self, while_stmt: &WhileStmt) {
        let label = self.pending_label.take();
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::While,
                is_async: false,
                label,
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
        let label = self.pending_label.take();
        self.with_region_control(
            PrismProgramRegionKind::Loop,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::DoWhile,
                is_async: false,
                label,
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
            self.bind_pattern(param, PrismProgramBindingKind::CatchParameter, false, None);
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
            ForHead::Pat(pat) => {
                self.bind_pattern(pat, PrismProgramBindingKind::LexicalLet, false, None)
            }
            ForHead::UsingDecl(_) => {}
        }
    }

    fn analyze_function(
        &mut self,
        function: &Function,
        kind: PrismProgramFunctionBoundaryKind,
        name: Option<String>,
    ) -> PrismProgramRegionId {
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
        )
    }

    fn analyze_arrow_function(&mut self, arrow: &ArrowExpr) -> PrismProgramRegionId {
        self.with_function_boundary_in_context(
            arrow.span,
            PrismProgramFunctionBoundaryKind::ArrowFunction,
            None,
            self.current_this_context().cloned(),
            PrismProgramSequenceKind::FunctionBody,
            |this| {
                for param in &arrow.params {
                    this.bind_pattern(param, PrismProgramBindingKind::Parameter, false, None);
                }
                match &*arrow.body {
                    BlockStmtOrExpr::BlockStmt(block) => this.analyze_block_contents(block),
                    BlockStmtOrExpr::Expr(expr) => this.analyze_expr(expr),
                }
            },
        )
    }

    fn analyze_callback_arrow(&mut self, arrow: &ArrowExpr, origin: Option<String>) {
        self.with_callback_boundary_in_context(
            arrow.span,
            PrismProgramCallbackBoundaryKind::CallbackArrow,
            origin,
            self.current_this_context().cloned(),
            |this| {
                for param in &arrow.params {
                    this.bind_pattern(param, PrismProgramBindingKind::Parameter, false, None);
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
        self.bind_pattern(&param.pat, PrismProgramBindingKind::Parameter, false, None);
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
                    self.bind_callable(
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
            Expr::Arrow(arrow) => {
                self.analyze_arrow_function(arrow);
            }
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
            Expr::Update(update) => {
                self.reject_forbidden_write_expr(update.span, &update.arg);
                self.analyze_expr(&update.arg);
            }
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
                self.analyze_class_expr(class_expr, None);
            }
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
                this.record_guard(
                    cond.test.span(),
                    collect_expr_binding_reads(this, &cond.test),
                );
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
                this.ir
                    .record_exit_mode(region_id, PrismProgramExitMode::ShortCircuit);
            },
        );
    }

    fn analyze_assign_target(&mut self, left: &deno_ast::swc::ast::AssignTarget) {
        match left {
            deno_ast::swc::ast::AssignTarget::Simple(simple) => match simple {
                deno_ast::swc::ast::SimpleAssignTarget::Ident(ident) => {
                    if let Some(path) = ident_path(self, &ident.id) {
                        self.reject_forbidden_write_target_path(ident.id.span, &path);
                    }
                    if let Some((binding_id, _)) = self.resolve_binding(&ident.id.sym.to_string()) {
                        self.ir.add_operation(
                            self.current_region(),
                            PrismProgramOperationKind::BindingWrite,
                            source_span(self.parsed, ident.id.span),
                            None,
                            vec![binding_id],
                            None,
                        );
                    }
                }
                deno_ast::swc::ast::SimpleAssignTarget::Member(member) => {
                    if let Some(path) = member_expr_path(self, member) {
                        self.reject_forbidden_write_target_path(member.span, &path);
                    }
                    self.analyze_member(member)
                }
                deno_ast::swc::ast::SimpleAssignTarget::Paren(paren) => {
                    self.analyze_expr(&paren.expr)
                }
                deno_ast::swc::ast::SimpleAssignTarget::OptChain(chain) => match &*chain.base {
                    OptChainBase::Call(call) => self.analyze_opt_call(call),
                    OptChainBase::Member(member) => {
                        if let Some(path) = member_expr_path(self, member) {
                            self.reject_forbidden_write_target_path(member.span, &path);
                        }
                        self.analyze_member(member)
                    }
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
                    self.bind_pattern(
                        &setter.param,
                        PrismProgramBindingKind::Parameter,
                        false,
                        None,
                    );
                    if let Some(body) = &setter.body {
                        self.analyze_block_stmt(body, PrismProgramSequenceKind::Block);
                    }
                }
                Prop::Method(MethodProp { function, .. }) => {
                    self.analyze_function(
                        function,
                        PrismProgramFunctionBoundaryKind::AnonymousFunction,
                        None,
                    );
                }
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

    fn analyze_class_expr(
        &mut self,
        class_expr: &ClassExpr,
        class_binding_override: Option<PrismProgramBindingId>,
    ) -> Option<PrismProgramRegionId> {
        let class_binding_id = class_binding_override.or_else(|| {
            class_expr.ident.as_ref().map(|ident| {
                self.bind_callable(
                    &ident.sym.to_string(),
                    PrismProgramBindingKind::Function,
                    ident.span,
                    None,
                )
            })
        });
        self.analyze_class_decl_body(
            class_binding_id,
            class_expr.ident.as_ref().map(|ident| ident.sym.to_string()),
            &class_expr.class,
        )
    }

    fn analyze_new_expr(&mut self, new_expr: &NewExpr) {
        if matches!(&*new_expr.callee, Expr::Ident(ident) if ident.sym.as_ref() == "Date") {
            self.reject_construct(
                new_expr.span,
                "ambient nondeterminism via new Date() is not allowed; use prism.time.* instead",
            );
        }
        if matches!(&*new_expr.callee, Expr::Ident(ident) if ident.sym.as_ref() == "Proxy") {
            self.reject_construct(
                new_expr.span,
                "Proxy is not allowed because it destroys analyzability",
            );
        }
        if matches!(&*new_expr.callee, Expr::Ident(ident) if ident.sym.as_ref() == "Function") {
            self.reject_construct(
                new_expr.span,
                "dynamic code execution via Function constructor is not allowed",
            );
        }
        let (callee_binding_id, import_specifier, import_name) =
            match self.resolve_constructor_target(&new_expr.callee) {
                Some(ClassTarget::Local {
                    class_binding_id, ..
                }) => (Some(class_binding_id), None, None),
                Some(ClassTarget::Imported {
                    import_specifier,
                    import_name,
                    ..
                }) => (None, Some(import_specifier), Some(import_name)),
                None => (None, None, None),
            };
        self.ir.add_invocation(
            self.current_region(),
            PrismProgramInvocationKind::Constructor,
            source_span(self.parsed, new_expr.span),
            call_path_from_expr(self, &new_expr.callee),
            callee_binding_id,
            import_specifier,
            import_name,
            None,
            None,
            PrismProgramEffectKind::PureCompute,
        );
        self.ir.add_operation(
            self.current_region(),
            PrismProgramOperationKind::Invocation,
            source_span(self.parsed, new_expr.span),
            call_path_from_expr(self, &new_expr.callee),
            callee_binding_id.into_iter().collect(),
            None,
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
        let callee_expr = match &call.callee {
            Callee::Expr(expr) => Some(&**expr),
            Callee::Super(_) => None,
            Callee::Import(_) => {
                self.reject_construct(call.span, "dynamic import targets are not allowed");
                None
            }
        };
        if let Some(path) = call_path.as_deref() {
            self.reject_forbidden_call_path(call.span, path);
            if path == "Object.assign" {
                if let Some(first_arg) = call.args.first() {
                    self.reject_forbidden_write_expr(call.span, &first_arg.expr);
                }
            }
        } else if matches!(&call.callee, Callee::Expr(expr) if matches!(&**expr, Expr::Ident(ident) if matches!(ident.sym.as_ref(), "eval" | "Function")))
        {
            let label = match &call.callee {
                Callee::Expr(expr) => match &**expr {
                    Expr::Ident(ident) if ident.sym.as_ref() == "eval" => "eval",
                    _ => "Function",
                },
                _ => unreachable!(),
            };
            self.reject_construct(
                call.span,
                format!("dynamic code execution via {label} is not allowed"),
            );
        }
        if let Some(control) = call_path
            .as_deref()
            .and_then(|path| program_region_control_for_call(call, path))
        {
            let region_kind = region_kind_for_control(&control);
            self.with_region_control(region_kind, control, call.span, |this, region_id| {
                this.ir
                    .set_guard_span(region_id, source_span(this.parsed, call.span));
                if let Some(callee_expr) = callee_expr {
                    this.record_call_effect(call.span, callee_expr, call_path.clone());
                }
                this.analyze_call_children(call, call_path.as_deref(), Some(region_id));
            });
            return;
        }
        if let Some(callee_expr) = callee_expr {
            self.record_call_effect(call.span, callee_expr, call_path.clone());
        }
        self.analyze_call_children(call, call_path.as_deref(), None);
    }

    fn analyze_opt_call(&mut self, call: &OptCall) {
        let call_path = call_path_from_expr(self, &call.callee);
        if let Some(path) = call_path.as_deref() {
            self.reject_forbidden_call_path(call.span, path);
        } else if matches!(&*call.callee, Expr::Ident(ident) if matches!(ident.sym.as_ref(), "eval" | "Function"))
        {
            let label = match &*call.callee {
                Expr::Ident(ident) if ident.sym.as_ref() == "eval" => "eval",
                _ => "Function",
            };
            self.reject_construct(
                call.span,
                format!("dynamic code execution via {label} is not allowed"),
            );
        }
        self.record_call_effect(call.span, &call.callee, call_path.clone());
        self.analyze_expr(&call.callee);
        for arg in &call.args {
            self.analyze_call_arg(arg, call_path.as_deref(), None);
        }
    }

    fn reject_forbidden_call_path(&mut self, span: Span, path: &str) {
        match path {
            "fetch" | "globalThis.fetch" | "window.fetch" => self.reject_construct(
                span,
                "uncontrolled network fetch is not allowed; use explicit PRISM-hosted capabilities instead",
            ),
            "Date.now" => self.reject_construct(
                span,
                "ambient nondeterminism via Date.now() is not allowed; use prism.time.* instead",
            ),
            "Math.random" => self.reject_construct(
                span,
                "ambient nondeterminism via Math.random() is not allowed; use prism.random.* instead",
            ),
            "crypto.randomUUID" | "globalThis.crypto.randomUUID" | "window.crypto.randomUUID" => {
                self.reject_construct(
                    span,
                    "ambient UUID generation is not allowed; use prism.random.uuid() instead",
                )
            }
            path if path.starts_with("Deno.") => self.reject_construct(
                span,
                "uncontrolled Deno runtime access is not allowed on the authored-code surface",
            ),
            path if path.starts_with("Bun.") => self.reject_construct(
                span,
                "uncontrolled Bun runtime access is not allowed on the authored-code surface",
            ),
            path if path.starts_with("process.") => self.reject_construct(
                span,
                "ambient process access is not allowed on the authored-code surface",
            ),
            "Reflect.defineProperty"
            | "Reflect.set"
            | "Reflect.setPrototypeOf"
            | "Reflect.deleteProperty"
            | "Object.defineProperty"
            | "Object.defineProperties"
            | "Object.setPrototypeOf" => self.reject_construct(
                span,
                "reflection-driven object semantics mutation is not allowed on the authored-code surface",
            ),
            "eval" => self.reject_construct(
                span,
                "dynamic code execution via eval is not allowed",
            ),
            "Function" => self.reject_construct(
                span,
                "dynamic code execution via Function constructor is not allowed",
            ),
            _ => {}
        }
    }

    fn reject_forbidden_write_target_path(&mut self, span: Span, path: &str) {
        if path == "prism" || path.starts_with("prism.") {
            self.reject_construct(
                span,
                "monkey-patching the PRISM SDK is not allowed on the authored-code surface",
            );
            return;
        }
        if matches!(path, "globalThis" | "window" | "global")
            || path.starts_with("globalThis.")
            || path.starts_with("window.")
            || path.starts_with("global.")
            || path.starts_with("process.env.")
        {
            self.reject_construct(
                span,
                "mutation of ambient globals is not allowed on the authored-code surface",
            );
        }
    }

    fn reject_forbidden_write_expr(&mut self, span: Span, expr: &Expr) {
        if let Some(path) = call_path_from_expr(self, expr) {
            self.reject_forbidden_write_target_path(span, &path);
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
                None,
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
                Expr::Fn(fn_expr) => {
                    self.analyze_callback_fn(&fn_expr.function, call_path.map(ToOwned::to_owned))
                }
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

fn import_name_from_module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_atom_lossy().to_string(),
    }
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
        PrismProgramRegionControl::FunctionBoundary { .. } => {
            PrismProgramRegionKind::FunctionBoundary
        }
        PrismProgramRegionControl::CallbackBoundary { .. } => {
            PrismProgramRegionKind::CallbackBoundary
        }
        PrismProgramRegionControl::Reduction { .. } => PrismProgramRegionKind::Reduction,
        PrismProgramRegionControl::Competition { .. } => PrismProgramRegionKind::Competition,
    }
}

fn program_region_control_for_call(
    call: &CallExpr,
    path: &str,
) -> Option<PrismProgramRegionControl> {
    match path {
        "Promise.all" => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::PromiseAll,
            origin: Some(path.to_string()),
            aggregates_outcomes: false,
        }),
        "Promise.allSettled" => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::PromiseAllSettled,
            origin: Some(path.to_string()),
            aggregates_outcomes: true,
        }),
        "Promise.race" => Some(PrismProgramRegionControl::Competition {
            kind: PrismProgramCompetitionKind::PromiseRace,
            origin: Some(path.to_string()),
            semantics: PrismProgramCompetitionSemantics::FirstCompletion,
        }),
        "Promise.any" => Some(PrismProgramRegionControl::Competition {
            kind: PrismProgramCompetitionKind::PromiseAny,
            origin: Some(path.to_string()),
            semantics: PrismProgramCompetitionSemantics::FirstSuccess,
        }),
        _ if path.ends_with(".map") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::Map,
            origin: Some(path.to_string()),
            aggregates_outcomes: false,
        }),
        _ if path.ends_with(".flatMap") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::FlatMap,
            origin: Some(path.to_string()),
            aggregates_outcomes: false,
        }),
        _ if path.ends_with(".filter") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::Filter,
            origin: Some(path.to_string()),
            aggregates_outcomes: false,
        }),
        _ if path.ends_with(".forEach") => Some(PrismProgramRegionControl::Parallel {
            kind: PrismProgramParallelKind::ForEach,
            origin: Some(path.to_string()),
            aggregates_outcomes: false,
        }),
        _ if path.ends_with(".reduce") => Some(PrismProgramRegionControl::Reduction {
            kind: PrismProgramReductionKind::Reduce,
            origin: Some(path.to_string()),
            callback_origin: Some(path.to_string()),
            accumulator_binding_name: reduction_callback_param_name(call, 0),
            element_binding_name: reduction_callback_param_name(call, 1),
            index_binding_name: reduction_callback_param_name(call, 2),
            has_initial_value: call.args.len() > 1,
            preserves_iteration_order: true,
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

fn reduction_callback_param_name(call: &CallExpr, index: usize) -> Option<String> {
    let callback = call.args.first()?;
    match &*callback.expr {
        Expr::Fn(fn_expr) => fn_expr
            .function
            .params
            .get(index)
            .and_then(|param| pattern_primary_name(&param.pat))
            .map(ToOwned::to_owned),
        Expr::Arrow(arrow) => arrow
            .params
            .get(index)
            .and_then(pattern_primary_name)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn pattern_primary_name(pattern: &Pat) -> Option<&str> {
    match pattern {
        Pat::Ident(ident) => Some(ident.id.sym.as_ref()),
        Pat::Assign(assign) => pattern_primary_name(&assign.left),
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
        Expr::Await(await_expr) => call_path_from_expr(analyzer, &await_expr.arg),
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
        MemberProp::PrivateName(private) => format!("#{}", private.name),
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
    if matches!(
        name.as_str(),
        "prism"
            | "Promise"
            | "Date"
            | "Math"
            | "Object"
            | "Reflect"
            | "Proxy"
            | "crypto"
            | "eval"
            | "Function"
            | "fetch"
            | "Deno"
            | "Bun"
            | "process"
            | "globalThis"
            | "window"
            | "global"
    ) {
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
            OptChainBase::Member(member) => {
                collect_expr_binding_reads_into(analyzer, &member.obj, out)
            }
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

fn member_prop_name(prop: &MemberProp) -> Option<String> {
    match prop {
        MemberProp::Ident(ident) => Some(ident.sym.to_string()),
        MemberProp::PrivateName(private) => Some(format!("#{}", private.name)),
        MemberProp::Computed(computed) => match &*computed.expr {
            Expr::Lit(deno_ast::swc::ast::Lit::Str(value)) => {
                Some(value.value.to_atom_lossy().to_string())
            }
            _ => None,
        },
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

fn forbidden_runtime_module_reason(specifier: &str) -> Option<&'static str> {
    let exact_match = matches!(
        specifier,
        "fs" | "fs/promises"
            | "node:fs"
            | "node:fs/promises"
            | "crypto"
            | "node:crypto"
            | "uuid"
            | "child_process"
            | "node:child_process"
            | "http"
            | "node:http"
            | "https"
            | "node:https"
            | "net"
            | "node:net"
            | "tls"
            | "node:tls"
            | "dgram"
            | "node:dgram"
            | "process"
            | "node:process"
    );
    if exact_match {
        return Some(
            "uncontrolled runtime modules for filesystem, network, process, or ambient randomness access are not allowed on the authored-code surface",
        );
    }
    None
}

pub(crate) fn analyze_prepared_typescript_program(
    prepared: &PreparedTypescriptProgram,
) -> Result<AnalyzedPrismProgram> {
    let mut module_irs = Vec::new();
    for source in prepared.source_bundle().units() {
        let parsed = parse_program(ParseParams {
            specifier: ModuleSpecifier::parse(source.specifier())?,
            text: source.source_text().into(),
            media_type: MediaType::TypeScript,
            capture_tokens: false,
            maybe_syntax: None,
            scope_analysis: false,
        })
        .map_err(|err| anyhow!(err.to_string()))?;
        let analyzer = ProgramAnalyzer::new(
            &parsed,
            source.display_path(),
            source.repo_module_path().map(PathBuf::from),
        );
        module_irs.push(analyzer.analyze_program_ref(parsed.program_ref())?);
    }

    let mut ir = if module_irs.len() == 1 {
        module_irs
            .pop()
            .expect("single analyzed module should exist")
    } else {
        merge_module_irs(prepared.root_source().specifier(), module_irs)
    };
    finalize_program_ir(&mut ir)?;

    Ok(AnalyzedPrismProgram { ir })
}

fn finalize_program_ir(ir: &mut PrismProgramIr) -> Result<()> {
    resolve_invocation_targets(ir);
    populate_invocation_summaries(ir);
    reject_unbounded_effectful_recursion(ir)?;
    Ok(())
}

fn resolve_invocation_targets(ir: &mut PrismProgramIr) {
    let export_lookup = ir.exports.iter().enumerate().fold(
        HashMap::<String, Vec<usize>>::new(),
        |mut acc, (index, export)| {
            acc.entry(export.module_specifier.clone())
                .or_default()
                .push(index);
            acc
        },
    );
    let local_class_method_lookup = ir.class_methods.iter().fold(
        HashMap::<(PrismProgramBindingId, String, bool), PrismProgramRegionId>::new(),
        |mut acc, method| {
            if let Some(class_binding_id) = method.class_binding_id {
                acc.insert(
                    (
                        class_binding_id,
                        method.method_name.clone(),
                        method.is_static,
                    ),
                    method.region_id,
                );
            }
            acc
        },
    );
    for invocation_id in 0..ir.invocations.len() {
        let invocation_kind = ir.invocations[invocation_id].kind.clone();
        let callee_binding_id = ir.invocations[invocation_id].callee_binding_id;
        let import_specifier = ir.invocations[invocation_id].import_specifier.clone();
        let import_name = ir.invocations[invocation_id].import_name.clone();
        let namespace_member = ir.invocations[invocation_id].namespace_member.clone();
        let class_method_is_static = ir.invocations[invocation_id].class_method_is_static;
        ir.invocations[invocation_id].target_region_id = match invocation_kind {
            PrismProgramInvocationKind::LocalFunction => callee_binding_id
                .and_then(|binding_id| ir.bindings.get(binding_id))
                .and_then(|binding| binding.callable_region_id),
            PrismProgramInvocationKind::Constructor => callee_binding_id
                .and_then(|binding_id| ir.bindings.get(binding_id))
                .and_then(|binding| binding.callable_region_id)
                .or_else(|| {
                    import_specifier
                        .as_ref()
                        .zip(import_name.as_ref())
                        .and_then(|(specifier, import_name)| {
                            resolve_export_target(
                                ir,
                                &export_lookup,
                                specifier,
                                import_name,
                                &mut HashSet::new(),
                            )
                        })
                }),
            PrismProgramInvocationKind::LocalMethod => callee_binding_id
                .zip(namespace_member.as_ref())
                .zip(class_method_is_static)
                .and_then(|((class_binding_id, method_name), is_static)| {
                    local_class_method_lookup
                        .get(&(class_binding_id, method_name.clone(), is_static))
                        .copied()
                }),
            PrismProgramInvocationKind::ImportedFunction => import_specifier
                .as_ref()
                .zip(import_name.as_ref())
                .and_then(|(specifier, import_name)| {
                    resolve_export_target(
                        ir,
                        &export_lookup,
                        specifier,
                        import_name,
                        &mut HashSet::new(),
                    )
                }),
            PrismProgramInvocationKind::ImportedMethod => import_specifier
                .as_ref()
                .zip(import_name.as_ref())
                .zip(namespace_member.as_ref())
                .zip(class_method_is_static)
                .and_then(|(((specifier, class_name), method_name), is_static)| {
                    resolve_class_method_target(
                        ir,
                        &export_lookup,
                        &local_class_method_lookup,
                        specifier,
                        class_name,
                        method_name,
                        is_static,
                        &mut HashSet::new(),
                    )
                }),
            PrismProgramInvocationKind::NamespaceImportMethod => import_specifier
                .as_ref()
                .zip(namespace_member.as_ref())
                .and_then(|(specifier, member)| {
                    resolve_export_target(
                        ir,
                        &export_lookup,
                        specifier,
                        member,
                        &mut HashSet::new(),
                    )
                }),
            PrismProgramInvocationKind::SdkMethod | PrismProgramInvocationKind::DynamicCall => None,
        };
    }
}

fn resolve_export_target(
    ir: &PrismProgramIr,
    export_lookup: &HashMap<String, Vec<usize>>,
    module_specifier: &str,
    export_name: &str,
    visiting: &mut HashSet<(String, String)>,
) -> Option<PrismProgramRegionId> {
    let key = (module_specifier.to_string(), export_name.to_string());
    if !visiting.insert(key.clone()) {
        return None;
    }
    let candidates = export_lookup.get(module_specifier)?;
    for index in candidates {
        let export = &ir.exports[*index];
        if export.export_name != export_name {
            continue;
        }
        if let Some(target_region_id) = export.target_region_id {
            return Some(target_region_id);
        }
        if let Some(reexport_specifier) = export.reexport_specifier.as_deref() {
            let reexport_name = export.reexport_name.as_deref().unwrap_or(export_name);
            if reexport_name == "*" {
                if let Some(target_region_id) = resolve_export_target(
                    ir,
                    export_lookup,
                    reexport_specifier,
                    export_name,
                    visiting,
                ) {
                    return Some(target_region_id);
                }
            } else if let Some(target_region_id) = resolve_export_target(
                ir,
                export_lookup,
                reexport_specifier,
                reexport_name,
                visiting,
            ) {
                return Some(target_region_id);
            }
        }
    }
    for index in candidates {
        let export = &ir.exports[*index];
        if export.export_name != "*" || export.reexport_name.as_deref() != Some("*") {
            continue;
        }
        if let Some(reexport_specifier) = export.reexport_specifier.as_deref() {
            if let Some(target_region_id) =
                resolve_export_target(ir, export_lookup, reexport_specifier, export_name, visiting)
            {
                return Some(target_region_id);
            }
        }
    }
    None
}

fn resolve_export_binding_id(
    ir: &PrismProgramIr,
    export_lookup: &HashMap<String, Vec<usize>>,
    module_specifier: &str,
    export_name: &str,
    visiting: &mut HashSet<(String, String)>,
) -> Option<PrismProgramBindingId> {
    let key = (module_specifier.to_string(), export_name.to_string());
    if !visiting.insert(key.clone()) {
        return None;
    }
    let candidates = export_lookup.get(module_specifier)?;
    for index in candidates {
        let export = &ir.exports[*index];
        if export.export_name != export_name {
            continue;
        }
        if let Some(binding_id) = export.binding_id {
            return Some(binding_id);
        }
        if let Some(reexport_specifier) = export.reexport_specifier.as_deref() {
            let reexport_name = export.reexport_name.as_deref().unwrap_or(export_name);
            if reexport_name == "*" {
                if let Some(binding_id) = resolve_export_binding_id(
                    ir,
                    export_lookup,
                    reexport_specifier,
                    export_name,
                    visiting,
                ) {
                    return Some(binding_id);
                }
            } else if let Some(binding_id) = resolve_export_binding_id(
                ir,
                export_lookup,
                reexport_specifier,
                reexport_name,
                visiting,
            ) {
                return Some(binding_id);
            }
        }
    }
    None
}

fn resolve_class_method_target(
    ir: &PrismProgramIr,
    export_lookup: &HashMap<String, Vec<usize>>,
    class_method_lookup: &HashMap<(PrismProgramBindingId, String, bool), PrismProgramRegionId>,
    module_specifier: &str,
    class_name: &str,
    method_name: &str,
    is_static: bool,
    visiting: &mut HashSet<(String, String)>,
) -> Option<PrismProgramRegionId> {
    let class_binding_id =
        resolve_export_binding_id(ir, export_lookup, module_specifier, class_name, visiting)?;
    class_method_lookup
        .get(&(class_binding_id, method_name.to_string(), is_static))
        .copied()
}

fn populate_invocation_summaries(ir: &mut PrismProgramIr) {
    let mut cache = HashMap::new();
    let mut mirrored_effects = Vec::new();
    for invocation_id in 0..ir.invocations.len() {
        let target_region_id = ir.invocations[invocation_id].target_region_id;
        let mut effect_kinds = Vec::new();
        let mut exit_modes = Vec::new();
        if let Some(method_path) = ir.invocations[invocation_id].method_path.as_deref() {
            let (intrinsic_effects, intrinsic_exit_modes) =
                intrinsic_invocation_summary(method_path);
            effect_kinds.extend(intrinsic_effects);
            exit_modes.extend(intrinsic_exit_modes);
        }
        if let Some(target_region_id) = target_region_id {
            let summary =
                summarize_callable_region(ir, target_region_id, &mut cache, &mut HashSet::new());
            effect_kinds.extend(summary.effect_kinds);
            exit_modes.extend(summary.exit_modes);
        } else if ir.invocations[invocation_id].effect_kind != PrismProgramEffectKind::PureCompute {
            effect_kinds.push(ir.invocations[invocation_id].effect_kind);
        }
        effect_kinds.sort_unstable_by_key(effect_sort_key);
        effect_kinds.dedup();
        exit_modes.sort_unstable_by_key(exit_mode_sort_key);
        exit_modes.dedup();
        ir.invocations[invocation_id].visible_effect_kinds = effect_kinds.clone();
        ir.invocations[invocation_id].possible_exit_modes = exit_modes;
        let invocation = &ir.invocations[invocation_id];
        for kind in effect_kinds {
            mirrored_effects.push((
                invocation.region_id,
                kind,
                invocation.span.clone(),
                invocation.method_path.clone(),
            ));
        }
    }
    for (region_id, kind, span, method_path) in mirrored_effects {
        if !ir.effects.iter().any(|effect| {
            effect.region_id == region_id
                && effect.kind == kind
                && effect.method_path == method_path
                && effect.span.specifier == span.specifier
                && effect.span.start_line == span.start_line
                && effect.span.start_column == span.start_column
                && effect.span.end_line == span.end_line
                && effect.span.end_column == span.end_column
        }) {
            ir.add_effect(region_id, kind, span, method_path);
        }
    }
}

fn intrinsic_invocation_summary(
    method_path: &str,
) -> (Vec<PrismProgramEffectKind>, Vec<PrismProgramExitMode>) {
    match method_path {
        "Promise.resolve" => (Vec::new(), vec![PrismProgramExitMode::Normal]),
        "Promise.reject" => (Vec::new(), vec![PrismProgramExitMode::Throw]),
        _ => (Vec::new(), Vec::new()),
    }
}

fn reject_unbounded_effectful_recursion(ir: &PrismProgramIr) -> Result<()> {
    let mut visited = HashSet::new();
    let mut active = Vec::new();
    for region_id in 0..ir.regions.len() {
        if ir.regions[region_id].kind != PrismProgramRegionKind::FunctionBoundary {
            continue;
        }
        detect_effectful_recursive_cycle(ir, region_id, &mut visited, &mut active)?;
    }
    Ok(())
}

fn detect_effectful_recursive_cycle(
    ir: &PrismProgramIr,
    region_id: PrismProgramRegionId,
    visited: &mut HashSet<PrismProgramRegionId>,
    active: &mut Vec<PrismProgramRegionId>,
) -> Result<()> {
    if active.contains(&region_id) {
        return Ok(());
    }
    if !visited.insert(region_id) {
        return Ok(());
    }
    active.push(region_id);
    let mut invocation_ids = Vec::new();
    collect_callable_invocations(ir, region_id, &mut invocation_ids);
    for invocation_id in invocation_ids {
        let invocation = &ir.invocations[invocation_id];
        let Some(target_region_id) = invocation.target_region_id else {
            continue;
        };
        if let Some(cycle_start) = active
            .iter()
            .position(|candidate| *candidate == target_region_id)
        {
            let cycle_regions = &active[cycle_start..];
            if recursion_cycle_is_effectful(ir, cycle_regions) {
                return Err(anyhow!(
                    "{}:{}:{}: effectful recursion is unbounded and not allowed on the authored-code surface",
                    invocation.span.specifier,
                    invocation.span.start_line,
                    invocation.span.start_column
                ));
            }
            continue;
        }
        detect_effectful_recursive_cycle(ir, target_region_id, visited, active)?;
    }
    active.pop();
    Ok(())
}

fn collect_callable_invocations(
    ir: &PrismProgramIr,
    region_id: PrismProgramRegionId,
    out: &mut Vec<PrismProgramInvocationId>,
) {
    let region = &ir.regions[region_id];
    out.extend(region.invocations.iter().copied());
    for child_region_id in &region.child_regions {
        let child = &ir.regions[*child_region_id];
        if child.kind == PrismProgramRegionKind::FunctionBoundary {
            continue;
        }
        collect_callable_invocations(ir, *child_region_id, out);
    }
}

fn recursion_cycle_is_effectful(
    ir: &PrismProgramIr,
    cycle_regions: &[PrismProgramRegionId],
) -> bool {
    cycle_regions
        .iter()
        .any(|region_id| callable_region_has_effects(ir, *region_id))
}

fn callable_region_has_effects(ir: &PrismProgramIr, region_id: PrismProgramRegionId) -> bool {
    let region = &ir.regions[region_id];
    if !region_direct_effects(ir, region_id).is_empty()
        || region.invocations.iter().any(|invocation_id| {
            ir.invocations[*invocation_id]
                .visible_effect_kinds
                .iter()
                .any(|kind| *kind != PrismProgramEffectKind::PureCompute)
        })
    {
        return true;
    }
    region.child_regions.iter().any(|child_region_id| {
        let child = &ir.regions[*child_region_id];
        child.kind != PrismProgramRegionKind::FunctionBoundary
            && callable_region_has_effects(ir, *child_region_id)
    })
}

fn summarize_callable_region(
    ir: &PrismProgramIr,
    region_id: PrismProgramRegionId,
    cache: &mut HashMap<PrismProgramRegionId, CallableRegionSummary>,
    visiting: &mut HashSet<PrismProgramRegionId>,
) -> CallableRegionSummary {
    if let Some(summary) = cache.get(&region_id) {
        return summary.clone();
    }
    if !visiting.insert(region_id) {
        return CallableRegionSummary {
            effect_kinds: region_direct_effects(ir, region_id),
            exit_modes: ir.regions[region_id].exit_modes.clone(),
        };
    }
    let region = &ir.regions[region_id];
    let mut effect_kinds = region_direct_effects(ir, region_id);
    let mut exit_modes = region.exit_modes.clone();
    for child_region_id in &region.child_regions {
        let child_region = &ir.regions[*child_region_id];
        if child_region.kind == PrismProgramRegionKind::FunctionBoundary {
            continue;
        }
        let child_summary = summarize_callable_region(ir, *child_region_id, cache, visiting);
        effect_kinds.extend(child_summary.effect_kinds);
        exit_modes.extend(child_summary.exit_modes);
    }
    for invocation_id in &region.invocations {
        let invocation = &ir.invocations[*invocation_id];
        if let Some(target_region_id) = invocation.target_region_id {
            let summary = summarize_callable_region(ir, target_region_id, cache, visiting);
            effect_kinds.extend(summary.effect_kinds);
            exit_modes.extend(summary.exit_modes);
        } else if invocation.effect_kind != PrismProgramEffectKind::PureCompute {
            effect_kinds.push(invocation.effect_kind);
        }
    }
    effect_kinds.sort_unstable_by_key(effect_sort_key);
    effect_kinds.dedup();
    exit_modes.sort_unstable_by_key(exit_mode_sort_key);
    exit_modes.dedup();
    visiting.remove(&region_id);
    let summary = CallableRegionSummary {
        effect_kinds,
        exit_modes,
    };
    cache.insert(region_id, summary.clone());
    summary
}

fn region_direct_effects(
    ir: &PrismProgramIr,
    region_id: PrismProgramRegionId,
) -> Vec<PrismProgramEffectKind> {
    ir.regions[region_id]
        .effects
        .iter()
        .map(|effect_id| ir.effects[*effect_id].kind)
        .filter(|kind| *kind != PrismProgramEffectKind::PureCompute)
        .collect()
}

fn effect_sort_key(kind: &PrismProgramEffectKind) -> usize {
    match kind {
        PrismProgramEffectKind::PureCompute => 0,
        PrismProgramEffectKind::HostedDeterministicInput => 1,
        PrismProgramEffectKind::CoordinationRead => 2,
        PrismProgramEffectKind::AuthoritativeWrite => 3,
        PrismProgramEffectKind::ReusableArtifactEmission => 4,
        PrismProgramEffectKind::ExternalExecutionIntent => 5,
    }
}

fn exit_mode_sort_key(mode: &PrismProgramExitMode) -> usize {
    match mode {
        PrismProgramExitMode::Normal => 0,
        PrismProgramExitMode::Return => 1,
        PrismProgramExitMode::Throw => 2,
        PrismProgramExitMode::Break => 3,
        PrismProgramExitMode::Continue => 4,
        PrismProgramExitMode::ShortCircuit => 5,
    }
}

fn merge_module_irs(source_specifier: &str, module_irs: Vec<PrismProgramIr>) -> PrismProgramIr {
    let root_span = module_irs
        .first()
        .map(|ir| ir.regions[ir.root_region_id].span.clone())
        .expect("module IRs should not be empty");
    let mut merged = PrismProgramIr::new(source_specifier.to_string(), root_span);
    for module_ir in module_irs {
        let PrismProgramIr {
            source_specifier: _,
            root_region_id,
            modules,
            exports,
            class_methods,
            regions,
            bindings,
            captures,
            effects,
            invocations,
            operations,
        } = module_ir;
        let region_offset = merged.regions.len();
        let binding_offset = merged.bindings.len();
        let capture_offset = merged.captures.len();
        let effect_offset = merged.effects.len();
        let invocation_offset = merged.invocations.len();
        let operation_offset = merged.operations.len();
        let export_offset = merged.exports.len();
        let class_method_offset = merged.class_methods.len();
        let root_region_id = root_region_id + region_offset;

        let mut regions = regions;
        for region in &mut regions {
            region.id += region_offset;
            region.parent = region.parent.map(|parent| parent + region_offset);
            if region.parent.is_none() {
                region.parent = Some(merged.root_region_id);
            }
            region.child_regions = region
                .child_regions
                .iter()
                .map(|id| id + region_offset)
                .collect();
            region.bindings = region
                .bindings
                .iter()
                .map(|id| id + binding_offset)
                .collect();
            region.effects = region.effects.iter().map(|id| id + effect_offset).collect();
            region.captures = region
                .captures
                .iter()
                .map(|id| id + capture_offset)
                .collect();
            region.invocations = region
                .invocations
                .iter()
                .map(|id| id + invocation_offset)
                .collect();
            region.operations = region
                .operations
                .iter()
                .map(|id| id + operation_offset)
                .collect();
            region.input_bindings = region
                .input_bindings
                .iter()
                .map(|id| id + binding_offset)
                .collect();
            region.output_bindings = region
                .output_bindings
                .iter()
                .map(|id| id + binding_offset)
                .collect();
        }
        merged.regions[merged.root_region_id]
            .child_regions
            .push(root_region_id);
        merged.regions.extend(regions);

        let mut modules = modules;
        for (index, module) in modules.iter_mut().enumerate() {
            module.id = merged.modules.len() + index;
            module.root_region_id += region_offset;
        }
        merged.modules.extend(modules);

        let mut bindings = bindings;
        for binding in &mut bindings {
            binding.id += binding_offset;
            binding.declared_in_region += region_offset;
            binding.callable_region_id = binding.callable_region_id.map(|id| id + region_offset);
            binding.instance_class_binding_id = binding
                .instance_class_binding_id
                .map(|id| id + binding_offset);
        }
        merged.bindings.extend(bindings);

        let mut exports = exports;
        for (index, export) in exports.iter_mut().enumerate() {
            export.id = export_offset + index;
            export.binding_id = export.binding_id.map(|id| id + binding_offset);
            export.target_region_id = export.target_region_id.map(|id| id + region_offset);
        }
        merged.exports.extend(exports);

        let mut captures = captures;
        for capture in &mut captures {
            capture.id += capture_offset;
            capture.region_id += region_offset;
            capture.binding_id += binding_offset;
        }
        merged.captures.extend(captures);

        let mut effects = effects;
        for effect in &mut effects {
            effect.id += effect_offset;
            effect.region_id += region_offset;
        }
        merged.effects.extend(effects);

        let mut invocations = invocations;
        for invocation in &mut invocations {
            invocation.id += invocation_offset;
            invocation.region_id += region_offset;
            invocation.callee_binding_id = invocation
                .callee_binding_id
                .map(|binding_id| binding_id + binding_offset);
            invocation.target_region_id = invocation.target_region_id.map(|id| id + region_offset);
        }
        merged.invocations.extend(invocations);

        let mut class_methods = class_methods;
        for (index, method) in class_methods.iter_mut().enumerate() {
            method.id = class_method_offset + index;
            method.class_binding_id = method.class_binding_id.map(|id| id + binding_offset);
            method.region_id += region_offset;
        }
        merged.class_methods.extend(class_methods);

        let mut operations = operations;
        for operation in &mut operations {
            operation.id += operation_offset;
            operation.region_id += region_offset;
            operation.binding_ids = operation
                .binding_ids
                .iter()
                .map(|id| id + binding_offset)
                .collect();
        }
        merged.operations.extend(operations);
    }
    merged
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::prism_code_compiler::{
        prepare_typescript_program, PrismCodeCompilerInput, PrismTypescriptProgramMode,
    };
    use crate::QueryLanguage;

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism_code_compiler_analysis_tests_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    fn analyze(code: &str) -> AnalyzedPrismProgram {
        let input = PrismCodeCompilerInput::inline("prism_code", code, QueryLanguage::Ts, true);
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");
        analyze_prepared_typescript_program(&prepared).expect("program should analyze")
    }

    fn analyze_error(code: &str) -> String {
        let input = PrismCodeCompilerInput::inline("prism_code", code, QueryLanguage::Ts, true);
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");
        analyze_prepared_typescript_program(&prepared)
            .expect_err("program should fail semantic analysis")
            .to_string()
    }

    fn analyze_repo_module(
        workspace_root: &std::path::Path,
        module_path: &str,
    ) -> AnalyzedPrismProgram {
        let input = PrismCodeCompilerInput::repo_module_compile(
            "prism_code",
            module_path,
            Some("default".to_string()),
            QueryLanguage::Ts,
        );
        let prepared = prepare_typescript_program(
            &input,
            Some(workspace_root),
            PrismTypescriptProgramMode::StatementBody,
        )
        .expect("repo module program should prepare");
        analyze_prepared_typescript_program(&prepared).expect("repo module should analyze")
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
        assert!(analyzed
            .ir
            .bindings
            .iter()
            .any(|binding| binding.name == "plan"
                && binding.handle_kind == Some(PrismProgramHandleKind::Plan)));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(
                |effect| effect.method_path.as_deref() == Some("plan.addTask")
                    && effect.kind == PrismProgramEffectKind::AuthoritativeWrite
            ));
        assert!(analyzed
            .ir
            .operations
            .iter()
            .any(|operation| operation.kind == PrismProgramOperationKind::AwaitBoundary));
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
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Parallel {
                kind: PrismProgramParallelKind::PromiseAll,
                ..
            }
        )));
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Parallel {
                kind: PrismProgramParallelKind::Map,
                ..
            }
        )));
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Reduction {
                kind: PrismProgramReductionKind::Reduce,
                accumulator_binding_name: Some(ref accumulator),
                element_binding_name: Some(ref element),
                has_initial_value: true,
                preserves_iteration_order: true,
                ..
            } if accumulator == "count" && element == "item"
        )));
        assert!(analyzed
            .ir
            .captures
            .iter()
            .any(|capture| capture.binding_name == "plan"));
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::CallbackBoundary {
                kind: PrismProgramCallbackBoundaryKind::CallbackArrow,
                ..
            }
        )));
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
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::FunctionBoundary {
                kind: PrismProgramFunctionBoundaryKind::ArrowFunction,
                ..
            }
        )));
        assert!(!analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::CallbackBoundary {
                kind: PrismProgramCallbackBoundaryKind::CallbackArrow,
                origin: None,
            }
        )));
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
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForOf,
                ..
            }
        )));
        let try_region = analyzed
            .ir
            .regions
            .iter()
            .find(|region| region.control == PrismProgramRegionControl::TryCatchFinally)
            .expect("try region should exist");
        assert!(try_region
            .exit_modes
            .contains(&PrismProgramExitMode::Return));
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::ShortCircuit {
                kind: PrismProgramShortCircuitKind::LogicalAnd
            }
        )));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::HostedDeterministicInput));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::CoordinationRead));
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
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Competition {
                kind: PrismProgramCompetitionKind::PromiseAny,
                ..
            }
        )));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::ReusableArtifactEmission));
        assert!(analyzed
            .ir
            .effects
            .iter()
            .any(|effect| effect.kind == PrismProgramEffectKind::ExternalExecutionIntent));
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Competition {
                kind: PrismProgramCompetitionKind::PromiseAny,
                semantics: PrismProgramCompetitionSemantics::FirstSuccess,
                ..
            }
        )));
    }

    #[test]
    fn analysis_tracks_local_function_invocation_as_first_class_semantics() {
        let analyzed = analyze(
            r#"
const helper = async (taskId) => prism.coordination.openTask(taskId);
await helper("task-1");
"#,
        );
        let helper_binding = analyzed
            .ir
            .bindings
            .iter()
            .find(|binding| binding.name == "helper")
            .expect("helper binding should exist");
        assert!(helper_binding.is_callable);
        let invocation = analyzed
            .ir
            .invocations
            .iter()
            .find(|invocation| {
                invocation.kind == PrismProgramInvocationKind::LocalFunction
                    && invocation.callee_binding_id == Some(helper_binding.id)
            })
            .expect("local helper invocation should exist");
        assert_eq!(
            invocation.target_region_id, helper_binding.callable_region_id,
            "local helper invocation should resolve to the helper boundary"
        );
        assert!(
            invocation
                .visible_effect_kinds
                .contains(&PrismProgramEffectKind::CoordinationRead),
            "local helper invocation should surface callee effects"
        );
    }

    #[test]
    fn analysis_tracks_imported_helpers_across_repo_modules() {
        let root = unique_temp_dir();
        let plans_dir = root.join(".prism/code/plans");
        let libraries_dir = root.join(".prism/code/libraries");
        fs::create_dir_all(&plans_dir).expect("plans directory should create");
        fs::create_dir_all(&libraries_dir).expect("libraries directory should create");
        fs::write(
            plans_dir.join("deploy.ts"),
            r#"
import { helper } from "../libraries/helper";

export default async function deploy() {
  return helper("task-1");
}
"#,
        )
        .expect("root module should write");
        fs::write(
            libraries_dir.join("helper.ts"),
            r#"
export function helper(taskId: string) {
  prism.time.now();
  return prism.coordination.openTask(taskId);
}
"#,
        )
        .expect("helper module should write");

        let analyzed = analyze_repo_module(&root, "plans/deploy.ts");
        assert_eq!(analyzed.ir.modules.len(), 2);
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedFunction
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/helper.ts")
                && invocation.import_name.as_deref() == Some("helper")
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));
        assert!(analyzed.ir.effects.iter().any(|effect| effect.kind
            == PrismProgramEffectKind::HostedDeterministicInput
            && effect.span.specifier == "file:///prism/code/libraries/helper.ts"));
        assert!(analyzed.ir.effects.iter().any(|effect| effect.kind
            == PrismProgramEffectKind::CoordinationRead
            && effect.span.specifier == "file:///prism/code/libraries/helper.ts"));
        assert!(analyzed.ir.effects.iter().any(|effect| effect.kind
            == PrismProgramEffectKind::HostedDeterministicInput
            && effect.span.specifier == "file:///prism/code/plans/deploy.ts"));
        assert!(analyzed.ir.effects.iter().any(|effect| effect.kind
            == PrismProgramEffectKind::CoordinationRead
            && effect.span.specifier == "file:///prism/code/plans/deploy.ts"));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn analysis_tracks_exported_const_arrow_helpers_across_repo_modules() {
        let root = unique_temp_dir();
        let plans_dir = root.join(".prism/code/plans");
        let libraries_dir = root.join(".prism/code/libraries");
        fs::create_dir_all(&plans_dir).expect("plans directory should create");
        fs::create_dir_all(&libraries_dir).expect("libraries directory should create");
        fs::write(
            plans_dir.join("deploy.ts"),
            r#"
import { helper } from "../libraries/helper";

export default async function deploy() {
  return helper("task-1");
}
"#,
        )
        .expect("root module should write");
        fs::write(
            libraries_dir.join("helper.ts"),
            r#"
export const helper = async (taskId: string) => {
  await prism.time.now();
  return prism.coordination.openTask(taskId);
};
"#,
        )
        .expect("helper module should write");

        let analyzed = analyze_repo_module(&root, "plans/deploy.ts");
        let helper_export = analyzed
            .ir
            .exports
            .iter()
            .find(|export| {
                export.module_specifier == "file:///prism/code/libraries/helper.ts"
                    && export.export_name == "helper"
            })
            .expect("helper export should be recorded");
        assert!(helper_export.target_region_id.is_some());
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedFunction
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/helper.ts")
                && invocation.target_region_id == helper_export.target_region_id
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn analysis_tracks_reexported_helpers_across_repo_modules() {
        let root = unique_temp_dir();
        let plans_dir = root.join(".prism/code/plans");
        let libraries_dir = root.join(".prism/code/libraries");
        fs::create_dir_all(&plans_dir).expect("plans directory should create");
        fs::create_dir_all(&libraries_dir).expect("libraries directory should create");
        fs::write(
            plans_dir.join("deploy.ts"),
            r#"
import { helper } from "../libraries/index";

export default async function deploy() {
  return helper("task-1");
}
"#,
        )
        .expect("root module should write");
        fs::write(
            libraries_dir.join("index.ts"),
            r#"
export { helper } from "./helper";
"#,
        )
        .expect("index module should write");
        fs::write(
            libraries_dir.join("helper.ts"),
            r#"
export function helper(taskId: string) {
  prism.time.now();
  return prism.coordination.openTask(taskId);
}
"#,
        )
        .expect("helper module should write");

        let analyzed = analyze_repo_module(&root, "plans/deploy.ts");
        assert!(analyzed
            .ir
            .modules
            .iter()
            .any(|module| { module.specifier == "file:///prism/code/libraries/index.ts" }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedFunction
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/index.ts")
                && invocation.import_name.as_deref() == Some("helper")
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn analysis_tracks_async_iteration_and_labeled_loop_control() {
        let analyzed = analyze(
            r#"
outer: for await (const item of items) {
  if (item.done) break outer;
  if (item.skip) continue outer;
  await prism.time.now();
}
"#,
        );
        assert!(analyzed.ir.regions.iter().any(|region| matches!(
            region.control,
            PrismProgramRegionControl::Loop {
                kind: PrismProgramLoopKind::ForOf,
                is_async: true,
                label: Some(ref label),
            } if label == "outer"
        )));
        assert!(analyzed.ir.operations.iter().any(|operation| operation.kind
            == PrismProgramOperationKind::Break
            && operation.target_label.as_deref() == Some("outer")));
        assert!(analyzed.ir.operations.iter().any(|operation| operation.kind
            == PrismProgramOperationKind::Continue
            && operation.target_label.as_deref() == Some("outer")));
    }

    #[test]
    fn analysis_tracks_local_class_expression_instance_and_static_methods() {
        let analyzed = analyze(
            r#"
const Helper = class {
  run(taskId) {
    prism.time.now();
    return prism.coordination.openTask(taskId);
  }

  static boot() {
    return prism.fs.readText("Cargo.toml");
  }
};

const helper = new Helper();
await helper.run("task-1");
await Helper.boot();
"#,
        );
        assert!(analyzed.ir.class_methods.iter().any(|method| {
            method.class_binding_id
                == analyzed
                    .ir
                    .bindings
                    .iter()
                    .find(|binding| binding.name == "Helper")
                    .map(|binding| binding.id)
                && method.method_name == "run"
                && !method.is_static
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("run")
                && invocation.class_method_is_static == Some(false)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("boot")
                && invocation.class_method_is_static == Some(true)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
        }));
    }

    #[test]
    fn analysis_tracks_this_dispatch_inside_class_helpers() {
        let analyzed = analyze(
            r#"
class Helper {
  run(taskId) {
    prism.time.now();
    return prism.coordination.openTask(taskId);
  }

  callRun(taskId) {
    return this.run(taskId);
  }

  static boot() {
    return prism.fs.readText("Cargo.toml");
  }

  static callBoot() {
    return this.boot();
  }
}

const helper = new Helper();
await helper.callRun("task-1");
await Helper.callBoot();
"#,
        );
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("run")
                && invocation.class_method_is_static == Some(false)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("boot")
                && invocation.class_method_is_static == Some(true)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
        }));
    }

    #[test]
    fn analysis_tracks_imported_class_methods_across_repo_modules_and_reexports() {
        let root = unique_temp_dir();
        let plans_dir = root.join(".prism/code/plans");
        let libraries_dir = root.join(".prism/code/libraries");
        fs::create_dir_all(&plans_dir).expect("plans directory should create");
        fs::create_dir_all(&libraries_dir).expect("libraries directory should create");
        fs::write(
            plans_dir.join("deploy.ts"),
            r#"
import { Helper } from "../libraries/index";

export default async function deploy() {
  const helper = new Helper();
  await helper.run("task-1");
  return Helper.boot();
}
"#,
        )
        .expect("root module should write");
        fs::write(
            libraries_dir.join("index.ts"),
            r#"
export { Helper } from "./helper";
"#,
        )
        .expect("index module should write");
        fs::write(
            libraries_dir.join("helper.ts"),
            r#"
export class Helper {
  run(taskId: string) {
    prism.time.now();
    return prism.coordination.openTask(taskId);
  }

  static boot() {
    return prism.fs.readText("Cargo.toml");
  }
}
"#,
        )
        .expect("helper module should write");

        let analyzed = analyze_repo_module(&root, "plans/deploy.ts");
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedMethod
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/index.ts")
                && invocation.import_name.as_deref() == Some("Helper")
                && invocation.namespace_member.as_deref() == Some("run")
                && invocation.class_method_is_static == Some(false)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedMethod
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/index.ts")
                && invocation.import_name.as_deref() == Some("Helper")
                && invocation.namespace_member.as_deref() == Some("boot")
                && invocation.class_method_is_static == Some(true)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
        }));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn analysis_tracks_namespace_imported_class_methods() {
        let root = unique_temp_dir();
        let plans_dir = root.join(".prism/code/plans");
        let libraries_dir = root.join(".prism/code/libraries");
        fs::create_dir_all(&plans_dir).expect("plans directory should create");
        fs::create_dir_all(&libraries_dir).expect("libraries directory should create");
        fs::write(
            plans_dir.join("deploy.ts"),
            r#"
import * as helpers from "../libraries/helper";

export default async function deploy() {
  const helper = new helpers.Helper();
  await helper.run("task-1");
  return helpers.Helper.boot();
}
"#,
        )
        .expect("root module should write");
        fs::write(
            libraries_dir.join("helper.ts"),
            r#"
export class Helper {
  run(taskId: string) {
    prism.time.now();
    return prism.coordination.openTask(taskId);
  }

  static boot() {
    return prism.fs.readText("Cargo.toml");
  }
}
"#,
        )
        .expect("helper module should write");

        let analyzed = analyze_repo_module(&root, "plans/deploy.ts");
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedMethod
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/helper.ts")
                && invocation.import_name.as_deref() == Some("Helper")
                && invocation.namespace_member.as_deref() == Some("run")
                && invocation.class_method_is_static == Some(false)
                && invocation.target_region_id.is_some()
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::ImportedMethod
                && invocation.import_specifier.as_deref()
                    == Some("file:///prism/code/libraries/helper.ts")
                && invocation.import_name.as_deref() == Some("Helper")
                && invocation.namespace_member.as_deref() == Some("boot")
                && invocation.class_method_is_static == Some(true)
                && invocation.target_region_id.is_some()
        }));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn analysis_tracks_private_class_methods_and_fields() {
        let analyzed = analyze(
            r#"
class Helper {
  #seed = prism.time.now();

  #run(taskId) {
    return prism.coordination.openTask(taskId);
  }

  async call(taskId) {
    const task = await this.#run(taskId);
    return this.#seed && task;
  }

  static #boot() {
    return prism.fs.readText("Cargo.toml");
  }

  static start() {
    return this.#boot();
  }
}

const helper = new Helper();
await helper.call("task-1");
await Helper.start();
"#,
        );
        assert!(analyzed
            .ir
            .class_methods
            .iter()
            .any(|method| { method.method_name == "#run" && !method.is_static }));
        assert!(analyzed
            .ir
            .class_methods
            .iter()
            .any(|method| { method.method_name == "#boot" && method.is_static }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("#run")
                && invocation.class_method_is_static == Some(false)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::CoordinationRead)
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalMethod
                && invocation.namespace_member.as_deref() == Some("#boot")
                && invocation.class_method_is_static == Some(true)
                && invocation.target_region_id.is_some()
                && invocation
                    .visible_effect_kinds
                    .contains(&PrismProgramEffectKind::HostedDeterministicInput)
        }));
    }

    #[test]
    fn analysis_tracks_promise_resolve_and_reject_semantics() {
        let analyzed = analyze(
            r#"
await Promise.resolve("ok");
await Promise.reject(new Error("boom"));
"#,
        );
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.method_path.as_deref() == Some("Promise.resolve")
                && invocation.possible_exit_modes == vec![PrismProgramExitMode::Normal]
        }));
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.method_path.as_deref() == Some("Promise.reject")
                && invocation
                    .possible_exit_modes
                    .contains(&PrismProgramExitMode::Throw)
        }));
    }

    #[test]
    fn analysis_rejects_ambient_nondeterminism_and_dynamic_code() {
        let date_now = analyze_error("Date.now();");
        assert!(date_now.contains("Date.now() is not allowed"));

        let math_random = analyze_error("Math.random();");
        assert!(math_random.contains("Math.random() is not allowed"));

        let eval_error = analyze_error("eval('1 + 1');");
        assert!(eval_error.contains("eval is not allowed"));

        let function_error = analyze_error("new Function('return 1');");
        assert!(function_error.contains("Function constructor is not allowed"));
    }

    #[test]
    fn analysis_rejects_dynamic_import_calls() {
        let error = analyze_error("await import('./helper');");
        assert!(error.contains("dynamic import targets are not allowed"));
    }

    #[test]
    fn analysis_rejects_new_date_and_with() {
        let new_date = analyze_error("new Date();");
        assert!(new_date.contains("new Date() is not allowed"));

        let with_error = analyze_error(
            r#"
with (obj) {
  task.complete({ summary: "done" });
}
"#,
        );
        assert!(with_error.contains("`with` is not allowed"));
    }

    #[test]
    fn analysis_rejects_uncontrolled_network_and_runtime_modules() {
        let fetch_error = analyze_error("await fetch('https://example.com');");
        assert!(fetch_error.contains("network fetch is not allowed"));

        let deno_error = analyze_error("await Deno.readTextFile('secret.txt');");
        assert!(deno_error.contains("Deno runtime access is not allowed"));

        let import_error = analyze_error("import { readFile } from 'node:fs/promises';");
        assert!(import_error.contains(
            "runtime modules for filesystem, network, process, or ambient randomness access are not allowed"
        ));
    }

    #[test]
    fn analysis_infers_handle_paths_through_awaited_sdk_results() {
        let analyzed = analyze(
            r#"
const plan = await prism.coordination.createPlan({ title: "Ship" });
const baseline = await plan.addTask({ title: "Baseline" });
const compare = await plan.addTask({ title: "Compare" });
await compare.dependsOn(baseline);
"#,
        );
        assert!(analyzed.ir.effects.iter().any(|effect| {
            effect.method_path.as_deref() == Some("task.dependsOn")
                && effect.kind == PrismProgramEffectKind::AuthoritativeWrite
        }));
    }

    #[test]
    fn analysis_rejects_ambient_uuid_and_meta_object_tricks() {
        let uuid_error = analyze_error("crypto.randomUUID();");
        assert!(uuid_error.contains("ambient UUID generation is not allowed"));

        let uuid_import_error = analyze_error("import { v4 } from 'uuid';");
        assert!(uuid_import_error.contains(
            "runtime modules for filesystem, network, process, or ambient randomness access are not allowed"
        ));

        let proxy_error = analyze_error("new Proxy({}, {});");
        assert!(proxy_error.contains("Proxy is not allowed"));

        let reflect_error = analyze_error("Reflect.defineProperty(obj, 'x', { value: 1 });");
        assert!(
            reflect_error.contains("reflection-driven object semantics mutation is not allowed")
        );
    }

    #[test]
    fn analysis_rejects_sdk_and_ambient_global_mutation() {
        let sdk_assign_error = analyze_error("prism.coordination = {};");
        assert!(sdk_assign_error.contains("monkey-patching the PRISM SDK is not allowed"));

        let sdk_assign_call_error = analyze_error("Object.assign(prism, { coordination: {} });");
        assert!(sdk_assign_call_error.contains("monkey-patching the PRISM SDK is not allowed"));

        let global_assign_error = analyze_error("globalThis.runtimeFlag = true;");
        assert!(global_assign_error.contains("mutation of ambient globals is not allowed"));

        let global_update_error = analyze_error("window.counter++;");
        assert!(global_update_error.contains("mutation of ambient globals is not allowed"));
    }

    #[test]
    fn analysis_rejects_unbounded_effectful_recursion_but_allows_pure_recursion() {
        let recursion_error = analyze_error(
            r#"
async function expand(taskId) {
  await prism.coordination.openTask(taskId);
  return expand(taskId);
}

await expand("task-1");
"#,
        );
        assert!(recursion_error.contains("effectful recursion is unbounded"));

        let analyzed = analyze(
            r#"
function fib(n) {
  if (n <= 1) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

fib(5);
"#,
        );
        assert!(analyzed.ir.invocations.iter().any(|invocation| {
            invocation.kind == PrismProgramInvocationKind::LocalFunction
                && invocation.method_path.as_deref() == Some("fib")
        }));
    }
}
