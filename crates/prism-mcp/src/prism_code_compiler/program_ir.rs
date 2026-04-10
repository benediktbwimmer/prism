use serde::Serialize;

pub(crate) type PrismProgramRegionId = usize;
pub(crate) type PrismProgramBindingId = usize;
pub(crate) type PrismProgramEffectId = usize;
pub(crate) type PrismProgramCaptureId = usize;
pub(crate) type PrismProgramOperationId = usize;

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
    Return,
    Throw,
    Break,
    Continue,
    PureCompute,
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
    },
    Branch {
        kind: PrismProgramBranchKind,
    },
    Loop {
        kind: PrismProgramLoopKind,
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
    },
    Competition {
        kind: PrismProgramCompetitionKind,
        origin: Option<String>,
    },
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
    pub(crate) handle_kind: Option<PrismProgramHandleKind>,
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
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramIr {
    pub(crate) source_specifier: String,
    pub(crate) root_region_id: PrismProgramRegionId,
    pub(crate) regions: Vec<PrismProgramRegion>,
    pub(crate) bindings: Vec<PrismProgramBinding>,
    pub(crate) captures: Vec<PrismProgramBindingCapture>,
    pub(crate) effects: Vec<PrismProgramEffectSite>,
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
            operations: Vec::new(),
            input_bindings: Vec::new(),
            output_bindings: Vec::new(),
            guard_span: None,
            exit_modes: vec![PrismProgramExitMode::Normal],
        };
        Self {
            source_specifier: source_specifier.into(),
            root_region_id: 0,
            regions: vec![root_region],
            bindings: Vec::new(),
            captures: Vec::new(),
            effects: Vec::new(),
            operations: Vec::new(),
        }
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
        handle_kind: Option<PrismProgramHandleKind>,
    ) -> PrismProgramBindingId {
        let id = self.bindings.len();
        self.bindings.push(PrismProgramBinding {
            id,
            name: name.into(),
            kind,
            declared_in_region: region_id,
            span,
            handle_kind,
        });
        self.regions[region_id].bindings.push(id);
        self.record_output_binding(region_id, id);
        id
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
    ) -> PrismProgramOperationId {
        let id = self.operations.len();
        self.operations.push(PrismProgramOperation {
            id,
            region_id,
            kind,
            span,
            method_path,
            binding_ids: binding_ids.clone(),
        });
        self.regions[region_id].operations.push(id);
        for binding_id in binding_ids {
            match kind {
                PrismProgramOperationKind::BindingWrite => {
                    self.record_output_binding(region_id, binding_id);
                }
                PrismProgramOperationKind::BindingRead
                | PrismProgramOperationKind::GuardEvaluation
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
