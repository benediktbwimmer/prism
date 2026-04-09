use prism_coordination::{
    CoordinationEvent, CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor,
};
use prism_ir::SessionId;
use prism_store::CoordinationPersistResult;

use crate::coordination_reads::{CoordinationReadConsistency, CoordinationReadFreshness};
use crate::published_plans::HydratedCoordinationPlanState;
use crate::shared_coordination_ref::SharedCoordinationRefDiagnostics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationAuthorityBackendKind {
    GitSharedRefs,
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationAuthorityCapabilities {
    pub supports_eventual_reads: bool,
    pub supports_transactions: bool,
    pub supports_runtime_descriptors: bool,
    pub supports_retained_history: bool,
    pub supports_diagnostics: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoordinationAuthorityProvenance {
    pub ref_name: Option<String>,
    pub head_commit: Option<String>,
    pub manifest_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationAuthorityStamp {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub logical_repo_id: String,
    pub snapshot_id: String,
    pub transaction_id: Option<String>,
    pub committed_at: Option<u64>,
    pub provenance: CoordinationAuthorityProvenance,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationCurrentState {
    pub snapshot: CoordinationSnapshot,
    pub canonical_snapshot_v2: CoordinationSnapshotV2,
    pub runtime_descriptors: Vec<RuntimeDescriptor>,
}

impl From<HydratedCoordinationPlanState> for CoordinationCurrentState {
    fn from(value: HydratedCoordinationPlanState) -> Self {
        Self {
            snapshot: value.snapshot,
            canonical_snapshot_v2: value.canonical_snapshot_v2,
            runtime_descriptors: value.runtime_descriptors,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationStateView {
    Snapshot,
    SnapshotV2,
    PlanState,
    RuntimeDescriptors,
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinationReadRequest {
    pub consistency: CoordinationReadConsistency,
    pub view: CoordinationStateView,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationReadEnvelope<T> {
    pub consistency: CoordinationReadConsistency,
    pub freshness: CoordinationReadFreshness,
    pub authority: Option<CoordinationAuthorityStamp>,
    pub value: Option<T>,
    pub refresh_error: Option<String>,
}

impl<T> CoordinationReadEnvelope<T> {
    pub fn verified_current(
        consistency: CoordinationReadConsistency,
        authority: Option<CoordinationAuthorityStamp>,
        value: T,
    ) -> Self {
        Self {
            consistency,
            freshness: CoordinationReadFreshness::VerifiedCurrent,
            authority,
            value: Some(value),
            refresh_error: None,
        }
    }

    pub fn unavailable(
        consistency: CoordinationReadConsistency,
        authority: Option<CoordinationAuthorityStamp>,
        refresh_error: Option<String>,
    ) -> Self {
        Self {
            consistency,
            freshness: CoordinationReadFreshness::Unavailable,
            authority,
            value: None,
            refresh_error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationDerivedStateMode {
    Inline,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinationTransactionBase {
    LatestStrong,
    ExpectedRevision(u64),
    ExpectedAuthorityStamp(CoordinationAuthorityStamp),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationTransactionRequest {
    pub base: CoordinationTransactionBase,
    pub session_id: Option<SessionId>,
    pub snapshot: CoordinationSnapshot,
    pub canonical_snapshot_v2: CoordinationSnapshotV2,
    pub appended_events: Vec<CoordinationEvent>,
    pub derived_state_mode: CoordinationDerivedStateMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationTransactionStatus {
    Committed,
    Conflict,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationConflictInfo {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationTransactionDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationTransactionResult {
    pub status: CoordinationTransactionStatus,
    pub committed: bool,
    pub authority: Option<CoordinationAuthorityStamp>,
    pub snapshot: Option<CoordinationCurrentState>,
    pub persisted: Option<CoordinationPersistResult>,
    pub conflict: Option<CoordinationConflictInfo>,
    pub diagnostics: Vec<CoordinationTransactionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDescriptorPublishRequest {
    pub base: CoordinationTransactionBase,
    pub descriptor: RuntimeDescriptor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDescriptorClearRequest {
    pub base: CoordinationTransactionBase,
    pub runtime_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeDescriptorQuery {
    pub consistency: CoordinationReadConsistency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoordinationHistoryRequest {
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationHistoryEntry {
    pub transaction_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub committed_at: Option<u64>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationHistoryEnvelope {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub entries: Vec<CoordinationHistoryEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoordinationDiagnosticsRequest {
    pub include_backend_details: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoordinationAuthorityBackendDetails {
    GitSharedRefs(SharedCoordinationRefDiagnostics),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationAuthorityDiagnostics {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub latest_authority: Option<CoordinationAuthorityStamp>,
    pub runtime_descriptor_count: usize,
    pub backend_details: CoordinationAuthorityBackendDetails,
}

#[cfg(test)]
mod tests {
    use prism_coordination::{
        CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor, RuntimeDiscoveryMode,
    };

    use super::CoordinationCurrentState;
    use crate::published_plans::HydratedCoordinationPlanState;

    #[test]
    fn current_state_converts_from_hydrated_plan_state() {
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
        let state = CoordinationCurrentState::from(HydratedCoordinationPlanState {
            snapshot: CoordinationSnapshot::default(),
            canonical_snapshot_v2: CoordinationSnapshotV2::default(),
            runtime_descriptors: runtime_descriptors.clone(),
        });

        assert_eq!(state.snapshot.events.len(), 0);
        assert_eq!(state.canonical_snapshot_v2.events.len(), 0);
        assert_eq!(state.runtime_descriptors, runtime_descriptors);
    }
}
