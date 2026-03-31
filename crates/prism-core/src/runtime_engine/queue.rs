use std::path::PathBuf;

use super::generation::RuntimeDomain;

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
        }
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
