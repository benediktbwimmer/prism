use prism_coordination::{
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2, RuntimeDescriptor,
};
use prism_store::CoordinationStartupCheckpointAuthority;

use crate::published_plans::HydratedCoordinationPlanState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationMaterializedBackendKind {
    Sqlite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationMaterializedCapabilities {
    pub supports_eventual_snapshots: bool,
    pub supports_read_models: bool,
    pub supports_startup_checkpoints: bool,
    pub supports_metadata: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationMaterializedState {
    pub snapshot: CoordinationSnapshot,
    pub canonical_snapshot_v2: CoordinationSnapshotV2,
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

impl From<HydratedCoordinationPlanState> for CoordinationMaterializedState {
    fn from(value: HydratedCoordinationPlanState) -> Self {
        Self {
            snapshot: value.snapshot,
            canonical_snapshot_v2: value.canonical_snapshot_v2,
            runtime_descriptors: value.runtime_descriptors,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationMaterializationMetadata {
    pub backend_kind: CoordinationMaterializedBackendKind,
    pub coordination_revision: Option<u64>,
    pub startup_checkpoint_coordination_revision: Option<u64>,
    pub startup_checkpoint_version: Option<u32>,
    pub startup_checkpoint_materialized_at: Option<u64>,
    pub startup_checkpoint_authority: Option<CoordinationStartupCheckpointAuthority>,
    pub has_snapshot: bool,
    pub has_canonical_snapshot_v2: bool,
    pub runtime_descriptor_count: usize,
    pub has_read_model: bool,
    pub has_queue_read_model: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationMaterializedReadEnvelope<T> {
    pub metadata: CoordinationMaterializationMetadata,
    pub value: Option<T>,
}

impl<T> CoordinationMaterializedReadEnvelope<T> {
    pub fn new(metadata: CoordinationMaterializationMetadata, value: Option<T>) -> Self {
        Self { metadata, value }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationStartupCheckpointWriteRequest {
    pub snapshot: CoordinationSnapshot,
    pub canonical_snapshot_v2: CoordinationSnapshotV2,
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationReadModelsWriteRequest {
    pub read_model: CoordinationReadModel,
    pub queue_read_model: CoordinationQueueReadModel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationCompactionWriteRequest {
    pub snapshot: CoordinationSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationMaterializedWriteResult {
    pub metadata: CoordinationMaterializationMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationMaterializedClearRequest {
    pub clear_startup_checkpoint: bool,
    pub clear_read_models: bool,
    pub clear_compaction: bool,
}

impl CoordinationMaterializedClearRequest {
    pub const fn all() -> Self {
        Self {
            clear_startup_checkpoint: true,
            clear_read_models: true,
            clear_compaction: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use prism_coordination::{
        CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
        CoordinationSnapshotV2, RuntimeDescriptor, RuntimeDiscoveryMode,
    };

    use super::{CoordinationMaterializedReadEnvelope, CoordinationMaterializedState};
    use crate::published_plans::HydratedCoordinationPlanState;

    #[test]
    fn materialized_state_converts_from_hydrated_plan_state() {
        let runtime_descriptors = vec![RuntimeDescriptor {
            runtime_id: "runtime:test".to_string(),
            repo_id: "repo:test".to_string(),
            worktree_id: "worktree:test".to_string(),
            principal_id: "principal:test".to_string(),
            instance_started_at: 1,
            last_seen_at: 2,
            branch_ref: Some("refs/heads/main".to_string()),
            checked_out_commit: Some("abc123".to_string()),
            capabilities: Vec::new(),
            discovery_mode: RuntimeDiscoveryMode::None,
            peer_endpoint: None,
            public_endpoint: None,
            peer_transport_identity: None,
            blob_snapshot_head: None,
            export_policy: None,
        }];
        let state = CoordinationMaterializedState::from(HydratedCoordinationPlanState {
            snapshot: CoordinationSnapshot::default(),
            canonical_snapshot_v2: CoordinationSnapshotV2::default(),
            runtime_descriptors: runtime_descriptors.clone(),
        });

        assert_eq!(state.snapshot.events.len(), 0);
        assert_eq!(state.canonical_snapshot_v2.events.len(), 0);
        assert_eq!(state.runtime_descriptors, runtime_descriptors);
    }

    #[test]
    fn read_envelope_wraps_optional_values() {
        let metadata = super::CoordinationMaterializationMetadata {
            backend_kind: super::CoordinationMaterializedBackendKind::Sqlite,
            coordination_revision: Some(7),
            startup_checkpoint_coordination_revision: Some(6),
            startup_checkpoint_version: None,
            startup_checkpoint_materialized_at: None,
            startup_checkpoint_authority: None,
            has_snapshot: true,
            has_canonical_snapshot_v2: false,
            runtime_descriptor_count: 0,
            has_read_model: true,
            has_queue_read_model: true,
        };

        let read_model = CoordinationReadModel::default();
        let queue_model = CoordinationQueueReadModel::default();

        assert_eq!(
            CoordinationMaterializedReadEnvelope::new(metadata.clone(), Some(read_model))
                .metadata
                .coordination_revision,
            Some(7)
        );
        assert!(
            CoordinationMaterializedReadEnvelope::new(metadata, Some(queue_model))
                .value
                .is_some()
        );
    }
}
