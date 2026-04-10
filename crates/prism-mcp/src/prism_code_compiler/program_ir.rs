use serde::Serialize;

pub(crate) type PrismProgramRegionId = usize;
pub(crate) type PrismProgramBindingId = usize;
pub(crate) type PrismProgramEffectId = usize;
pub(crate) type PrismProgramCaptureId = usize;
pub(crate) type PrismProgramOperationId = usize;
pub(crate) type PrismProgramModuleId = usize;
pub(crate) type PrismProgramInvocationId = usize;
pub(crate) type PrismProgramExportId = usize;
pub(crate) type PrismProgramClassMethodId = usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PrismProgramSourceSpan {
    pub(crate) specifier: String,
    pub(crate) start_line: usize,
    pub(crate) start_column: usize,
    pub(crate) end_line: usize,
    pub(crate) end_column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramRegionKind {
    Module,
    Sequence,
    Parallel,
    Branch,
    Loop,
    ShortCircuit,
    TryCatchFinally,
    FunctionBoundary,
    CallbackBoundary,
    Reduction,
    Competition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramSequenceKind {
    ModuleBody,
    Block,
    FunctionBody,
    CallbackBody,
    TryBody,
    CatchBody,
    FinallyBody,
    SwitchCase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramParallelKind {
    PromiseAll,
    PromiseAllSettled,
    Map,
    FlatMap,
    Filter,
    ForEach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramBranchKind {
    IfElse,
    Switch,
    ConditionalExpression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramLoopKind {
    For,
    ForOf,
    ForIn,
    While,
    DoWhile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramShortCircuitKind {
    LogicalAnd,
    LogicalOr,
    NullishCoalescing,
    Some,
    Every,
    Find,
    FindIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramReductionKind {
    Reduce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramCompetitionKind {
    PromiseRace,
    PromiseAny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramFunctionBoundaryKind {
    NamedFunction,
    AnonymousFunction,
    ArrowFunction,
    ClassMethod,
    Constructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramCallbackBoundaryKind {
    CallbackFunction,
    CallbackArrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramExitMode {
    Normal,
    Return,
    Throw,
    Break,
    Continue,
    ShortCircuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramBindingKind {
    Function,
    Parameter,
    LexicalLet,
    LexicalConst,
    LexicalVar,
    CatchParameter,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramHandleKind {
    Plan,
    Task,
    Claim,
    Artifact,
    Work,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramEffectKind {
    PureCompute,
    HostedDeterministicInput,
    CoordinationRead,
    AuthoritativeWrite,
    ReusableArtifactEmission,
    ExternalExecutionIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramOperationKind {
    GuardEvaluation,
    AwaitBoundary,
    BindingRead,
    BindingWrite,
    Invocation,
    Return,
    Throw,
    Break,
    Continue,
    PureCompute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramCompetitionSemantics {
    FirstCompletion,
    FirstSuccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramInvocationKind {
    SdkMethod,
    LocalFunction,
    ImportedFunction,
    LocalMethod,
    ImportedMethod,
    NamespaceImportMethod,
    Constructor,
    DynamicCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum PrismProgramRegionControl {
    Module,
    Sequence {
        kind: PrismProgramSequenceKind,
    },
    Parallel {
        kind: PrismProgramParallelKind,
        origin: Option<String>,
        aggregates_outcomes: bool,
    },
    Branch {
        kind: PrismProgramBranchKind,
    },
    Loop {
        kind: PrismProgramLoopKind,
        is_async: bool,
        label: Option<String>,
    },
    ShortCircuit {
        kind: PrismProgramShortCircuitKind,
    },
    TryCatchFinally,
    FunctionBoundary {
        kind: PrismProgramFunctionBoundaryKind,
        name: Option<String>,
    },
    CallbackBoundary {
        kind: PrismProgramCallbackBoundaryKind,
        origin: Option<String>,
    },
    Reduction {
        kind: PrismProgramReductionKind,
        origin: Option<String>,
        callback_origin: Option<String>,
        accumulator_binding_name: Option<String>,
        element_binding_name: Option<String>,
        index_binding_name: Option<String>,
        has_initial_value: bool,
        preserves_iteration_order: bool,
    },
    Competition {
        kind: PrismProgramCompetitionKind,
        origin: Option<String>,
        semantics: PrismProgramCompetitionSemantics,
    },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramModule {
    pub(crate) id: PrismProgramModuleId,
    pub(crate) specifier: String,
    pub(crate) display_path: String,
    pub(crate) root_region_id: PrismProgramRegionId,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramExport {
    pub(crate) id: PrismProgramExportId,
    pub(crate) module_specifier: String,
    pub(crate) export_name: String,
    pub(crate) binding_id: Option<PrismProgramBindingId>,
    pub(crate) target_region_id: Option<PrismProgramRegionId>,
    pub(crate) reexport_specifier: Option<String>,
    pub(crate) reexport_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramRegion {
    pub(crate) id: PrismProgramRegionId,
    pub(crate) kind: PrismProgramRegionKind,
    pub(crate) control: PrismProgramRegionControl,
    pub(crate) parent: Option<PrismProgramRegionId>,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) child_regions: Vec<PrismProgramRegionId>,
    pub(crate) bindings: Vec<PrismProgramBindingId>,
    pub(crate) effects: Vec<PrismProgramEffectId>,
    pub(crate) captures: Vec<PrismProgramCaptureId>,
    pub(crate) invocations: Vec<PrismProgramInvocationId>,
    pub(crate) operations: Vec<PrismProgramOperationId>,
    pub(crate) input_bindings: Vec<PrismProgramBindingId>,
    pub(crate) output_bindings: Vec<PrismProgramBindingId>,
    pub(crate) guard_span: Option<PrismProgramSourceSpan>,
    pub(crate) exit_modes: Vec<PrismProgramExitMode>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramBinding {
    pub(crate) id: PrismProgramBindingId,
    pub(crate) name: String,
    pub(crate) kind: PrismProgramBindingKind,
    pub(crate) declared_in_region: PrismProgramRegionId,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) is_callable: bool,
    pub(crate) handle_kind: Option<PrismProgramHandleKind>,
    pub(crate) callable_region_id: Option<PrismProgramRegionId>,
    pub(crate) instance_class_binding_id: Option<PrismProgramBindingId>,
    pub(crate) instance_import_specifier: Option<String>,
    pub(crate) instance_import_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramBindingCapture {
    pub(crate) id: PrismProgramCaptureId,
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) binding_id: PrismProgramBindingId,
    pub(crate) binding_name: String,
    pub(crate) span: PrismProgramSourceSpan,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramEffectSite {
    pub(crate) id: PrismProgramEffectId,
    pub(crate) kind: PrismProgramEffectKind,
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) method_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramOperation {
    pub(crate) id: PrismProgramOperationId,
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) kind: PrismProgramOperationKind,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) method_path: Option<String>,
    pub(crate) binding_ids: Vec<PrismProgramBindingId>,
    pub(crate) target_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramInvocation {
    pub(crate) id: PrismProgramInvocationId,
    pub(crate) region_id: PrismProgramRegionId,
    pub(crate) kind: PrismProgramInvocationKind,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) method_path: Option<String>,
    pub(crate) callee_binding_id: Option<PrismProgramBindingId>,
    pub(crate) import_specifier: Option<String>,
    pub(crate) import_name: Option<String>,
    pub(crate) namespace_member: Option<String>,
    pub(crate) class_method_is_static: Option<bool>,
    pub(crate) effect_kind: PrismProgramEffectKind,
    pub(crate) target_region_id: Option<PrismProgramRegionId>,
    pub(crate) visible_effect_kinds: Vec<PrismProgramEffectKind>,
    pub(crate) possible_exit_modes: Vec<PrismProgramExitMode>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramClassMethod {
    pub(crate) id: PrismProgramClassMethodId,
    pub(crate) class_binding_id: Option<PrismProgramBindingId>,
    pub(crate) module_specifier: String,
    pub(crate) method_name: String,
    pub(crate) is_static: bool,
    pub(crate) region_id: PrismProgramRegionId,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramIr {
    pub(crate) source_specifier: String,
    pub(crate) root_region_id: PrismProgramRegionId,
    pub(crate) modules: Vec<PrismProgramModule>,
    pub(crate) exports: Vec<PrismProgramExport>,
    pub(crate) class_methods: Vec<PrismProgramClassMethod>,
    pub(crate) regions: Vec<PrismProgramRegion>,
    pub(crate) bindings: Vec<PrismProgramBinding>,
    pub(crate) captures: Vec<PrismProgramBindingCapture>,
    pub(crate) effects: Vec<PrismProgramEffectSite>,
    pub(crate) invocations: Vec<PrismProgramInvocation>,
    pub(crate) operations: Vec<PrismProgramOperation>,
}

impl PrismProgramIr {
    pub(crate) fn new(
        source_specifier: impl Into<String>,
        root_span: PrismProgramSourceSpan,
    ) -> Self {
        let root_region = PrismProgramRegion {
            id: 0,
            kind: PrismProgramRegionKind::Module,
            control: PrismProgramRegionControl::Module,
            parent: None,
            span: root_span,
            child_regions: Vec::new(),
            bindings: Vec::new(),
            effects: Vec::new(),
            captures: Vec::new(),
            invocations: Vec::new(),
            operations: Vec::new(),
            input_bindings: Vec::new(),
            output_bindings: Vec::new(),
            guard_span: None,
            exit_modes: vec![PrismProgramExitMode::Normal],
        };
        Self {
            source_specifier: source_specifier.into(),
            root_region_id: 0,
            modules: Vec::new(),
            exports: Vec::new(),
            class_methods: Vec::new(),
            regions: vec![root_region],
            bindings: Vec::new(),
            captures: Vec::new(),
            effects: Vec::new(),
            invocations: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub(crate) fn register_module(
        &mut self,
        specifier: impl Into<String>,
        display_path: impl Into<String>,
        root_region_id: PrismProgramRegionId,
    ) -> PrismProgramModuleId {
        let id = self.modules.len();
        self.modules.push(PrismProgramModule {
            id,
            specifier: specifier.into(),
            display_path: display_path.into(),
            root_region_id,
        });
        id
    }

    pub(crate) fn add_export(
        &mut self,
        module_specifier: impl Into<String>,
        export_name: impl Into<String>,
        binding_id: Option<PrismProgramBindingId>,
        target_region_id: Option<PrismProgramRegionId>,
    ) -> PrismProgramExportId {
        let id = self.exports.len();
        self.exports.push(PrismProgramExport {
            id,
            module_specifier: module_specifier.into(),
            export_name: export_name.into(),
            binding_id,
            target_region_id,
            reexport_specifier: None,
            reexport_name: None,
        });
        id
    }

    pub(crate) fn add_reexport(
        &mut self,
        module_specifier: impl Into<String>,
        export_name: impl Into<String>,
        reexport_specifier: impl Into<String>,
        reexport_name: impl Into<String>,
    ) -> PrismProgramExportId {
        let id = self.exports.len();
        self.exports.push(PrismProgramExport {
            id,
            module_specifier: module_specifier.into(),
            export_name: export_name.into(),
            binding_id: None,
            target_region_id: None,
            reexport_specifier: Some(reexport_specifier.into()),
            reexport_name: Some(reexport_name.into()),
        });
        id
    }

    pub(crate) fn add_region(
        &mut self,
        parent: PrismProgramRegionId,
        kind: PrismProgramRegionKind,
        control: PrismProgramRegionControl,
        span: PrismProgramSourceSpan,
    ) -> PrismProgramRegionId {
        let id = self.regions.len();
        self.regions.push(PrismProgramRegion {
            id,
            kind,
            control,
            parent: Some(parent),
            span,
            child_regions: Vec::new(),
            bindings: Vec::new(),
            effects: Vec::new(),
            captures: Vec::new(),
            invocations: Vec::new(),
            operations: Vec::new(),
            input_bindings: Vec::new(),
            output_bindings: Vec::new(),
            guard_span: None,
            exit_modes: vec![PrismProgramExitMode::Normal],
        });
        self.regions[parent].child_regions.push(id);
        id
    }

    pub(crate) fn add_binding(
        &mut self,
        region_id: PrismProgramRegionId,
        name: impl Into<String>,
        kind: PrismProgramBindingKind,
        span: PrismProgramSourceSpan,
        is_callable: bool,
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        let id = self.bindings.len();
        self.bindings.push(PrismProgramBinding {
            id,
            name: name.into(),
            kind,
            declared_in_region: region_id,
            span,
            is_callable,
            handle_kind,
            callable_region_id: None,
            instance_class_binding_id: None,
            instance_import_specifier: None,
            instance_import_name: None,
        });
        self.regions[region_id].bindings.push(id);
        self.record_output_binding(region_id, id);
        id
    }

    pub(crate) fn set_binding_callable_region(
        &mut self,
        binding_id: PrismProgramBindingId,
        region_id: PrismProgramRegionId,
    ) {
        if let Some(binding) = self.bindings.get_mut(binding_id) {
            binding.callable_region_id = Some(region_id);
        }
    }

    pub(crate) fn set_binding_instance_class(
        &mut self,
        binding_id: PrismProgramBindingId,
        class_binding_id: Option<PrismProgramBindingId>,
        import_specifier: Option<String>,
        import_name: Option<String>,
    ) {
        if let Some(binding) = self.bindings.get_mut(binding_id) {
            binding.instance_class_binding_id = class_binding_id;
            binding.instance_import_specifier = import_specifier;
            binding.instance_import_name = import_name;
        }
    }

    pub(crate) fn add_effect(
        &mut self,
        region_id: PrismProgramRegionId,
        kind: PrismProgramEffectKind,
        span: PrismProgramSourceSpan,
        method_path: Option<String>,
    ) -> PrismProgramEffectId {
        let id = self.effects.len();
        self.effects.push(PrismProgramEffectSite {
            id,
            kind,
            region_id,
            span,
            method_path,
        });
        self.regions[region_id].effects.push(id);
        id
    }

    pub(crate) fn add_capture(
        &mut self,
        region_id: PrismProgramRegionId,
        binding_id: PrismProgramBindingId,
        binding_name: impl Into<String>,
        span: PrismProgramSourceSpan,
    ) -> PrismProgramCaptureId {
        let id = self.captures.len();
        self.captures.push(PrismProgramBindingCapture {
            id,
            region_id,
            binding_id,
            binding_name: binding_name.into(),
            span,
        });
        self.regions[region_id].captures.push(id);
        self.record_input_binding(region_id, binding_id);
        id
    }

    pub(crate) fn add_operation(
        &mut self,
        region_id: PrismProgramRegionId,
        kind: PrismProgramOperationKind,
        span: PrismProgramSourceSpan,
        method_path: Option<String>,
        binding_ids: Vec<PrismProgramBindingId>,
        target_label: Option<String>,
    ) -> PrismProgramOperationId {
        let id = self.operations.len();
        self.operations.push(PrismProgramOperation {
            id,
            region_id,
            kind,
            span,
            method_path,
            binding_ids: binding_ids.clone(),
            target_label,
        });
        self.regions[region_id].operations.push(id);
        for binding_id in binding_ids {
            match kind {
                PrismProgramOperationKind::BindingWrite => {
                    self.record_output_binding(region_id, binding_id);
                }
                PrismProgramOperationKind::BindingRead
                | PrismProgramOperationKind::GuardEvaluation
                | PrismProgramOperationKind::Invocation
                | PrismProgramOperationKind::Return
                | PrismProgramOperationKind::Throw
                | PrismProgramOperationKind::PureCompute
                | PrismProgramOperationKind::AwaitBoundary
                | PrismProgramOperationKind::Break
                | PrismProgramOperationKind::Continue => {
                    self.record_input_binding(region_id, binding_id);
                }
            }
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_invocation(
        &mut self,
        region_id: PrismProgramRegionId,
        kind: PrismProgramInvocationKind,
        span: PrismProgramSourceSpan,
        method_path: Option<String>,
        callee_binding_id: Option<PrismProgramBindingId>,
        import_specifier: Option<String>,
        import_name: Option<String>,
        namespace_member: Option<String>,
        class_method_is_static: Option<bool>,
        effect_kind: PrismProgramEffectKind,
    ) -> PrismProgramInvocationId {
        let id = self.invocations.len();
        self.invocations.push(PrismProgramInvocation {
            id,
            region_id,
            kind,
            span,
            method_path,
            callee_binding_id,
            import_specifier,
            import_name,
            namespace_member,
            class_method_is_static,
            effect_kind,
            target_region_id: None,
            visible_effect_kinds: Vec::new(),
            possible_exit_modes: Vec::new(),
        });
        self.regions[region_id].invocations.push(id);
        id
    }

    pub(crate) fn add_class_method(
        &mut self,
        class_binding_id: Option<PrismProgramBindingId>,
        module_specifier: impl Into<String>,
        method_name: impl Into<String>,
        is_static: bool,
        region_id: PrismProgramRegionId,
    ) -> PrismProgramClassMethodId {
        let id = self.class_methods.len();
        self.class_methods.push(PrismProgramClassMethod {
            id,
            class_binding_id,
            module_specifier: module_specifier.into(),
            method_name: method_name.into(),
            is_static,
            region_id,
        });
        id
    }

    pub(crate) fn set_guard_span(
        &mut self,
        region_id: PrismProgramRegionId,
        span: PrismProgramSourceSpan,
    ) {
        self.regions[region_id].guard_span = Some(span);
    }

    pub(crate) fn record_exit_mode(
        &mut self,
        region_id: PrismProgramRegionId,
        exit_mode: PrismProgramExitMode,
    ) {
        insert_unique(&mut self.regions[region_id].exit_modes, exit_mode);
    }

    pub(crate) fn record_input_binding(
        &mut self,
        region_id: PrismProgramRegionId,
        binding_id: PrismProgramBindingId,
    ) {
        insert_unique(&mut self.regions[region_id].input_bindings, binding_id);
    }

    pub(crate) fn record_output_binding(
        &mut self,
        region_id: PrismProgramRegionId,
        binding_id: PrismProgramBindingId,
    ) {
        insert_unique(&mut self.regions[region_id].output_bindings, binding_id);
    }
}

fn insert_unique<T>(items: &mut Vec<T>, value: T)
where
    T: Copy + PartialEq,
{
    if !items.contains(&value) {
        items.push(value);
    }
}
