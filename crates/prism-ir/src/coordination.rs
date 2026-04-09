use crate::identity::{PlanId, PrincipalId, TaskId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanStatus {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationTaskStatus {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanOperatorState {
    #[default]
    None,
    Abandoned,
    Archived,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleStatus {
    #[default]
    Pending,
    Active,
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveTaskStatus {
    Pending,
    Active,
    Blocked,
    BrokenDependency,
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DerivedPlanStatus {
    Pending,
    Active,
    Blocked,
    BrokenDependency,
    Completed,
    Failed,
    Abandoned,
    Archived,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorClass {
    Human,
    #[default]
    WorktreeExecutor,
    Service,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutorPolicy {
    #[serde(default)]
    pub executor_class: ExecutorClass,
    #[serde(default)]
    pub target_label: Option<String>,
    #[serde(default)]
    pub allowed_principals: Vec<PrincipalId>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum NodeRefKind {
    Plan,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeRef {
    pub kind: NodeRefKind,
    pub id: String,
}

impl NodeRef {
    pub fn plan(id: impl Into<PlanId>) -> Self {
        let id = id.into();
        Self {
            kind: NodeRefKind::Plan,
            id: id.0.to_string(),
        }
    }

    pub fn task(id: impl Into<TaskId>) -> Self {
        let id = id.into();
        Self {
            kind: NodeRefKind::Task,
            id: id.0.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimMode {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Capability {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ClaimStatus {
    Active,
    Released,
    Expired,
    Contended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LeaseRenewalMode {
    #[default]
    Strict,
    Assisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConflictSeverity {
    Info,
    Warn,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ConflictOverlapKind {
    Node,
    Lineage,
    File,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ArtifactStatus {
    Proposed,
    InReview,
    Approved,
    Rejected,
    Superseded,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReviewVerdict {
    Approved,
    ChangesRequested,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CoordinationEventKind {
    PlanCreated,
    PlanUpdated,
    TaskCreated,
    TaskAssigned,
    TaskStatusChanged,
    TaskBlocked,
    TaskUnblocked,
    TaskHeartbeated,
    TaskResumed,
    TaskReclaimed,
    ClaimAcquired,
    ClaimRenewed,
    ClaimReleased,
    ClaimContended,
    ArtifactProposed,
    ArtifactReviewed,
    ArtifactSuperseded,
    HandoffRequested,
    HandoffAccepted,
    MutationRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventTriggerKind {
    TaskBecameActionable,
    ClaimExpired,
    RecurringPlanTick,
    RuntimeBecameStale,
    HookRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventExecutionStatus {
    Claimed,
    Running,
    Succeeded,
    Failed,
    Expired,
    Abandoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_executor_policy_defaults_to_worktree_executor() {
        let policy = TaskExecutorPolicy::default();
        assert_eq!(policy.executor_class, ExecutorClass::WorktreeExecutor);
        assert!(policy.target_label.is_none());
        assert!(policy.allowed_principals.is_empty());
    }

    #[test]
    fn node_ref_helpers_preserve_plan_and_task_kinds() {
        let plan_ref = NodeRef::plan(PlanId::new("plan:test"));
        let task_ref = NodeRef::task(TaskId::new("task:test"));

        assert_eq!(plan_ref.kind, NodeRefKind::Plan);
        assert_eq!(plan_ref.id, "plan:test");
        assert_eq!(task_ref.kind, NodeRefKind::Task);
        assert_eq!(task_ref.id, "task:test");
    }

    #[test]
    fn event_execution_status_serializes_with_snake_case_names() {
        let status = EventExecutionStatus::Succeeded;
        let value = serde_json::to_string(&status).expect("status should serialize");
        assert_eq!(value, "\"succeeded\"");
    }
}
