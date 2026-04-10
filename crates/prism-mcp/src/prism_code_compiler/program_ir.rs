use serde::Serialize;

pub(crate) type PrismProgramRegionId = usize;
pub(crate) type PrismProgramBindingId = usize;
pub(crate) type PrismProgramEffectId = usize;
pub(crate) type PrismProgramCaptureId = usize;

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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrismProgramRegion {
    pub(crate) id: PrismProgramRegionId,
    pub(crate) kind: PrismProgramRegionKind,
    pub(crate) parent: Option<PrismProgramRegionId>,
    pub(crate) span: PrismProgramSourceSpan,
    pub(crate) child_regions: Vec<PrismProgramRegionId>,
    pub(crate) bindings: Vec<PrismProgramBindingId>,
    pub(crate) effects: Vec<PrismProgramEffectId>,
    pub(crate) captures: Vec<PrismProgramCaptureId>,
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
pub(crate) struct PrismProgramIr {
    pub(crate) source_specifier: String,
    pub(crate) root_region_id: PrismProgramRegionId,
    pub(crate) regions: Vec<PrismProgramRegion>,
    pub(crate) bindings: Vec<PrismProgramBinding>,
    pub(crate) captures: Vec<PrismProgramBindingCapture>,
    pub(crate) effects: Vec<PrismProgramEffectSite>,
}

impl PrismProgramIr {
    pub(crate) fn new(
        source_specifier: impl Into<String>,
        root_span: PrismProgramSourceSpan,
    ) -> Self {
        let root_region = PrismProgramRegion {
            id: 0,
            kind: PrismProgramRegionKind::Module,
            parent: None,
            span: root_span,
            child_regions: Vec::new(),
            bindings: Vec::new(),
            effects: Vec::new(),
            captures: Vec::new(),
        };
        Self {
            source_specifier: source_specifier.into(),
            root_region_id: 0,
            regions: vec![root_region],
            bindings: Vec::new(),
            captures: Vec::new(),
            effects: Vec::new(),
        }
    }

    pub(crate) fn add_region(
        &mut self,
        parent: PrismProgramRegionId,
        kind: PrismProgramRegionKind,
        span: PrismProgramSourceSpan,
    ) -> PrismProgramRegionId {
        let id = self.regions.len();
        self.regions.push(PrismProgramRegion {
            id,
            kind,
            parent: Some(parent),
            span,
            child_regions: Vec::new(),
            bindings: Vec::new(),
            effects: Vec::new(),
            captures: Vec::new(),
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
        id
    }
}
