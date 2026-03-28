mod blockers;
mod compat;
mod helpers;
mod mutations;
mod queries;
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
    HandoffAcceptInput, HandoffInput, Plan, PlanCreateInput, PlanUpdateInput, PolicyViolation,
    PolicyViolationCode, PolicyViolationRecord, TaskBlocker, TaskCompletionContext,
    TaskCreateInput, TaskUpdateInput, WorkClaim,
};
pub use compat::{
    coordination_snapshot_from_plan_graphs, execution_overlays_from_tasks,
    plan_graph_from_coordination, snapshot_plan_graphs,
};
pub use runtime::CoordinationRuntimeState;
