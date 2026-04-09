mod blockers;
mod canonical_graph;
mod canonical_graph_traversal;
mod derived_status;
mod event_replay;
mod evidence;
mod executor_routing;
mod git_execution;
mod helpers;
mod lease;
mod mutations;
mod queries;
mod queue_read_model;
mod read_model;
mod runtime;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub use crate::state::CoordinationStore;
pub use crate::types::{
    AcceptanceCriterion, Artifact, ArtifactEvidenceType, ArtifactProposeInput, ArtifactRequirement,
    ArtifactRequirementKind, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    BlockerKind, ClaimAcquireInput, CoordinationConflict, CoordinationEvent, CoordinationPolicy,
    CoordinationSnapshot, CoordinationSpecRef, CoordinationTask, CoordinationTaskSpecRef,
    EventExecutionOwner, EventExecutionRecord, HandoffAcceptInput, HandoffInput, LeaseHolder, Plan,
    PlanCreateInput, PlanScheduling, PlanUpdateInput, PolicyViolation, PolicyViolationCode,
    PolicyViolationRecord, ReviewRequirement, ReviewerClass, RuntimeDescriptor,
    RuntimeDescriptorCapability, RuntimeDiscoveryMode, TaskBlocker, TaskCompletionContext,
    TaskCreateInput, TaskReclaimInput, TaskResumeInput, TaskUpdateInput, WorkClaim,
};
pub use canonical_graph::{
    CanonicalPlanRecord, CanonicalTaskRecord, CoordinationDependencyRecord, CoordinationSnapshotV2,
    COORDINATION_SCHEMA_V2,
};
pub use canonical_graph_traversal::{CanonicalCoordinationGraph, CanonicalNodeRecord};
pub use derived_status::{CoordinationDerivations, DerivedPlanState, DerivedTaskState};
pub use event_replay::coordination_snapshot_from_events;
pub use executor_routing::{
    caller_matches_task_executor_policy, executor_mismatch_reasons, task_executor_policy,
    ExecutorMismatchReason, TaskExecutorCaller,
};
pub use git_execution::{
    GitExecutionCompletionMode, GitExecutionPolicy, GitExecutionStartMode, GitPreflightReport,
    GitPublishReport, TaskGitExecution,
};
pub use lease::{
    assisted_heartbeat_window, canonical_authoritative_task_holder, canonical_current_task_holder,
    canonical_task_heartbeat_due_state,
    canonical_task_heartbeat_due_state_with_runtime_descriptors, canonical_task_lease_state,
    canonical_task_lease_state_with_runtime_descriptors, claim_heartbeat_due_state,
    claim_heartbeat_due_state_with_runtime_descriptors, claim_lease_state,
    claim_lease_state_with_runtime_descriptors, heartbeat_due_soon_window, same_holder,
    task_heartbeat_due_state, task_heartbeat_due_state_with_runtime_descriptors, task_lease_state,
    task_lease_state_with_runtime_descriptors, LeaseHeartbeatDueState, LeaseState,
};
pub use queue_read_model::{
    coordination_queue_read_model_from_seed, coordination_queue_read_model_from_snapshot,
    coordination_queue_read_model_from_snapshot_v2, CoordinationQueueReadModel,
};
pub use read_model::{
    coordination_read_model_from_seed, coordination_read_model_from_snapshot,
    coordination_read_model_from_snapshot_v2, ready_task_count_for_active_plans,
    CoordinationReadModel,
};
pub use runtime::CoordinationRuntimeState;
