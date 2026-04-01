use std::path::PathBuf;

use super::generation::{RuntimeDomain, WorkspaceGenerationId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceRuntimePathRequest {
    pub path: PathBuf,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkspaceRuntimeQueueClass {
    InteractiveMutation,
    FollowUpMutation,
    FastPrepare,
    Settle,
    CheckpointMaterialization,
}

impl WorkspaceRuntimeQueueClass {
    pub const fn priority_rank(self) -> u8 {
        match self {
            Self::InteractiveMutation => 0,
            Self::FollowUpMutation => 1,
            Self::FastPrepare => 2,
            Self::Settle => 3,
            Self::CheckpointMaterialization => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkspaceRuntimeCoalescingKey {
    Path(PathBuf),
    Domain(RuntimeDomain),
    WorktreeContext,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkspaceRuntimeCommandKind {
    InteractiveMutation,
    FollowUpMutation,
    PreparePaths,
    ApplyPreparedDelta,
    SettleDomain(RuntimeDomain),
    MaterializeCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeCommand {
    pub kind: WorkspaceRuntimeCommandKind,
    pub queue_class: WorkspaceRuntimeQueueClass,
    pub coalescing_key: WorkspaceRuntimeCoalescingKey,
    pub target_generation: Option<WorkspaceGenerationId>,
    pub path_requests: Vec<WorkspaceRuntimePathRequest>,
}

impl WorkspaceRuntimeCommand {
    pub fn new(
        kind: WorkspaceRuntimeCommandKind,
        queue_class: WorkspaceRuntimeQueueClass,
        coalescing_key: WorkspaceRuntimeCoalescingKey,
    ) -> Self {
        Self {
            kind,
            queue_class,
            coalescing_key,
            target_generation: None,
            path_requests: Vec::new(),
        }
    }

    pub fn with_paths(
        kind: WorkspaceRuntimeCommandKind,
        queue_class: WorkspaceRuntimeQueueClass,
        coalescing_key: WorkspaceRuntimeCoalescingKey,
        paths: Vec<PathBuf>,
    ) -> Self {
        Self::with_path_requests(
            kind,
            queue_class,
            coalescing_key,
            paths
                .into_iter()
                .map(|path| WorkspaceRuntimePathRequest { path, revision: 0 })
                .collect(),
        )
    }

    pub fn with_path_requests(
        kind: WorkspaceRuntimeCommandKind,
        queue_class: WorkspaceRuntimeQueueClass,
        coalescing_key: WorkspaceRuntimeCoalescingKey,
        path_requests: Vec<WorkspaceRuntimePathRequest>,
    ) -> Self {
        Self {
            kind,
            queue_class,
            coalescing_key,
            target_generation: None,
            path_requests,
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.path_requests
            .iter()
            .map(|request| request.path.clone())
            .collect()
    }

    pub fn with_target_generation(mut self, generation: WorkspaceGenerationId) -> Self {
        self.target_generation = Some(generation);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeQueueDepth {
    pub queue_class: WorkspaceRuntimeQueueClass,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRuntimeQueueSnapshot {
    pub active: Option<WorkspaceRuntimeCommand>,
    pub queued: Vec<WorkspaceRuntimeQueueDepth>,
    pub total_depth: usize,
}
