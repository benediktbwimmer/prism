mod blockers;
mod compat;
mod event_replay;
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
    AcceptanceCriterion, Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput,
    ArtifactSupersedeInput, BlockerKind, ClaimAcquireInput, CoordinationConflict,
    CoordinationEvent, CoordinationPolicy, CoordinationSnapshot, CoordinationTask,
    HandoffAcceptInput, HandoffInput, Plan, PlanCreateInput, PlanScheduling, PlanUpdateInput,
    PolicyViolation, PolicyViolationCode, PolicyViolationRecord, TaskBlocker,
    TaskCompletionContext, TaskCreateInput, TaskReclaimInput, TaskResumeInput, TaskUpdateInput,
    WorkClaim,
};
pub use compat::{
    coordination_snapshot_from_plan_graphs, execution_overlays_from_tasks,
    plan_graph_from_coordination, snapshot_plan_graphs,
};
pub use event_replay::coordination_snapshot_from_events;
pub use git_execution::{
    GitExecutionCompletionMode, GitExecutionPolicy, GitExecutionStartMode, GitPreflightReport,
    GitPublishReport, TaskGitExecution,
};
pub use lease::{
    assisted_heartbeat_window, claim_heartbeat_due_state, claim_lease_state,
    heartbeat_due_soon_window, task_heartbeat_due_state, task_lease_state, LeaseHeartbeatDueState,
    LeaseState,
};
pub use queue_read_model::{
    coordination_queue_read_model_from_seed, coordination_queue_read_model_from_snapshot,
    CoordinationQueueReadModel,
};
pub use read_model::{
    coordination_read_model_from_seed, coordination_read_model_from_snapshot,
    ready_task_count_for_active_plans, CoordinationReadModel,
};
pub use runtime::CoordinationRuntimeState;
