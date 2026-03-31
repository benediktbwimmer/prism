use std::collections::BTreeMap;
use std::path::PathBuf;

use super::context::WorkspaceRuntimeContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceGenerationId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceRuntimeDeltaSequence(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeDomain {
    FileFacts,
    CrossFileEdges,
    Projections,
    MemoryReanchor,
    Checkpoint,
    Coordination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeFreshnessState {
    Current,
    Pending,
    Stale,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeMaterializationDepth {
    Shallow,
    Medium,
    Deep,
    KnownUnmaterialized,
    OutOfScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeDomainState {
    pub freshness: RuntimeFreshnessState,
    pub materialization: RuntimeMaterializationDepth,
}

impl RuntimeDomainState {
    pub const fn new(
        freshness: RuntimeFreshnessState,
        materialization: RuntimeMaterializationDepth,
    ) -> Self {
        Self {
            freshness,
            materialization,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePublishedGeneration {
    pub context: WorkspaceRuntimeContext,
    pub id: WorkspaceGenerationId,
    pub parent_id: Option<WorkspaceGenerationId>,
    pub committed_delta: Option<WorkspaceRuntimeDeltaSequence>,
    pub domain_states: BTreeMap<RuntimeDomain, RuntimeDomainState>,
}

impl WorkspacePublishedGeneration {
    pub fn initial(context: WorkspaceRuntimeContext) -> Self {
        Self {
            context,
            id: WorkspaceGenerationId(0),
            parent_id: None,
            committed_delta: None,
            domain_states: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeDeltaBatch {
    pub sequence: WorkspaceRuntimeDeltaSequence,
    pub parent_generation: WorkspaceGenerationId,
    pub committed_generation: WorkspaceGenerationId,
    pub changed_paths: Vec<PathBuf>,
    pub domain_states: BTreeMap<RuntimeDomain, RuntimeDomainState>,
}
