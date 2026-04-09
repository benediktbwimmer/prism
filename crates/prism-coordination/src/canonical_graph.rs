use std::collections::BTreeSet;

use anyhow::Result;
use prism_ir::{
    AgentId, AnchorRef, CoordinationTaskId, NodeRef, PlanBinding, PlanId, PlanKind,
    PlanOperatorState, PlanScope, TaskExecutorPolicy, TaskId, TaskLifecycleStatus, Timestamp,
    ValidationRef, WorkspaceRevision,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical_graph_traversal::CanonicalCoordinationGraph;
use crate::executor_routing::task_executor_policy;
use crate::git_execution::TaskGitExecution;
use crate::types::{
    AcceptanceCriterion, Artifact, ArtifactReview, CoordinationEvent, CoordinationPolicy,
    CoordinationSnapshot, CoordinationSpecRef, CoordinationTask, CoordinationTaskSpecRef,
    LeaseHolder, Plan, PlanScheduling, WorkClaim,
};

pub const COORDINATION_SCHEMA_V2: u64 = 2;

fn default_schema_version() -> u64 {
    COORDINATION_SCHEMA_V2
}

fn default_plan_scope() -> PlanScope {
    PlanScope::Repo
}

fn default_plan_kind() -> PlanKind {
    PlanKind::TaskExecution
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalPlanRecord {
    pub id: PlanId,
    #[serde(default)]
    pub parent_plan_id: Option<PlanId>,
    pub title: String,
    pub goal: String,
    #[serde(default = "default_plan_scope")]
    pub scope: PlanScope,
    #[serde(default = "default_plan_kind")]
    pub kind: PlanKind,
    #[serde(default)]
    pub policy: CoordinationPolicy,
    #[serde(default)]
    pub scheduling: PlanScheduling,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_from: Option<String>,
    #[serde(default)]
    pub spec_refs: Vec<CoordinationSpecRef>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub operator_state: PlanOperatorState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTaskRecord {
    pub id: TaskId,
    pub parent_plan_id: PlanId,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub lifecycle_status: TaskLifecycleStatus,
    #[serde(default)]
    pub estimated_minutes: u32,
    #[serde(default)]
    pub executor: TaskExecutorPolicy,
    #[serde(default)]
    pub assignee: Option<AgentId>,
    #[serde(default)]
    pub pending_handoff_to: Option<AgentId>,
    #[serde(default)]
    pub session: Option<prism_ir::SessionId>,
    #[serde(default)]
    pub lease_holder: Option<LeaseHolder>,
    #[serde(default)]
    pub lease_started_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_refreshed_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_stale_at: Option<Timestamp>,
    #[serde(default)]
    pub lease_expires_at: Option<Timestamp>,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub branch_ref: Option<String>,
    #[serde(default)]
    pub anchors: Vec<AnchorRef>,
    #[serde(default)]
    pub bindings: PlanBinding,
    #[serde(default)]
    pub acceptance: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub validation_refs: Vec<ValidationRef>,
    #[serde(default)]
    pub base_revision: WorkspaceRevision,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub spec_refs: Vec<CoordinationTaskSpecRef>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub git_execution: TaskGitExecution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationDependencyRecord {
    pub source: NodeRef,
    pub target: NodeRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationSnapshotV2 {
    #[serde(default = "default_schema_version")]
    pub schema_version: u64,
    #[serde(default)]
    pub plans: Vec<CanonicalPlanRecord>,
    #[serde(default)]
    pub tasks: Vec<CanonicalTaskRecord>,
    #[serde(default)]
    pub dependencies: Vec<CoordinationDependencyRecord>,
    #[serde(default)]
    pub claims: Vec<WorkClaim>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub reviews: Vec<ArtifactReview>,
    #[serde(default)]
    pub events: Vec<CoordinationEvent>,
    #[serde(default)]
    pub next_plan: u64,
    #[serde(default)]
    pub next_task: u64,
    #[serde(default)]
    pub next_claim: u64,
    #[serde(default)]
    pub next_artifact: u64,
    #[serde(default)]
    pub next_review: u64,
}

impl Default for CoordinationSnapshotV2 {
    fn default() -> Self {
        Self {
            schema_version: COORDINATION_SCHEMA_V2,
            plans: Vec::new(),
            tasks: Vec::new(),
            dependencies: Vec::new(),
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 0,
            next_task: 0,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        }
    }
}

impl CoordinationSnapshotV2 {
    pub fn graph(&self) -> Result<CanonicalCoordinationGraph<'_>> {
        CanonicalCoordinationGraph::new(self)
    }

    pub fn validate_graph(&self) -> Result<()> {
        self.graph().map(|_| ())
    }

    pub fn derive_statuses(&self) -> Result<crate::CoordinationDerivations> {
        crate::CoordinationDerivations::derive(self)
    }
}

impl CoordinationSnapshot {
    pub fn to_canonical_snapshot_v2(&self) -> CoordinationSnapshotV2 {
        let mut dependencies = legacy_dependency_records(&self.tasks);
        dependencies.sort_by(|left, right| {
            let left_key = (
                &left.source.kind,
                &left.source.id,
                &left.target.kind,
                &left.target.id,
            );
            let right_key = (
                &right.source.kind,
                &right.source.id,
                &right.target.kind,
                &right.target.id,
            );
            left_key.cmp(&right_key)
        });
        CoordinationSnapshotV2 {
            schema_version: COORDINATION_SCHEMA_V2,
            plans: self
                .plans
                .iter()
                .map(CanonicalPlanRecord::from_legacy_plan)
                .collect(),
            tasks: self
                .tasks
                .iter()
                .map(CanonicalTaskRecord::from_legacy_task)
                .collect(),
            dependencies,
            claims: self.claims.clone(),
            artifacts: self.artifacts.clone(),
            reviews: self.reviews.clone(),
            events: self.events.clone(),
            next_plan: self.next_plan,
            next_task: self.next_task,
            next_claim: self.next_claim,
            next_artifact: self.next_artifact,
            next_review: self.next_review,
        }
    }

    pub fn validate_canonical_projection(&self) -> Result<()> {
        validate_legacy_projection_dependencies(&self.tasks)
    }
}

impl CanonicalPlanRecord {
    pub fn from_legacy_plan(plan: &Plan) -> Self {
        Self {
            id: plan.id.clone(),
            parent_plan_id: None,
            title: plan.title.clone(),
            goal: plan.goal.clone(),
            scope: plan.scope,
            kind: plan.kind,
            policy: plan.policy.clone(),
            scheduling: plan.scheduling.clone(),
            tags: plan.tags.clone(),
            created_from: plan.created_from.clone(),
            spec_refs: plan.spec_refs.clone(),
            metadata: plan.metadata.clone(),
            operator_state: legacy_plan_status_to_operator_state(plan.status),
        }
    }
}

impl CanonicalTaskRecord {
    pub fn from_legacy_task(task: &CoordinationTask) -> Self {
        Self {
            id: task_id_from_legacy(&task.id),
            parent_plan_id: task.plan.clone(),
            title: task.title.clone(),
            summary: task.summary.clone(),
            lifecycle_status: legacy_task_status_to_lifecycle(task.status),
            estimated_minutes: legacy_estimated_minutes(task),
            executor: task_executor_policy(task),
            assignee: task.assignee.clone(),
            pending_handoff_to: task.pending_handoff_to.clone(),
            session: task.session.clone(),
            lease_holder: task.lease_holder.clone(),
            lease_started_at: task.lease_started_at,
            lease_refreshed_at: task.lease_refreshed_at,
            lease_stale_at: task.lease_stale_at,
            lease_expires_at: task.lease_expires_at,
            worktree_id: task.worktree_id.clone(),
            branch_ref: task.branch_ref.clone(),
            anchors: task.anchors.clone(),
            bindings: task.bindings.clone(),
            acceptance: task.acceptance.clone(),
            validation_refs: task.validation_refs.clone(),
            base_revision: task.base_revision.clone(),
            priority: task.priority,
            tags: task.tags.clone(),
            spec_refs: task.spec_refs.clone(),
            metadata: canonical_task_metadata(task),
            git_execution: task.git_execution.clone(),
        }
    }
}

fn legacy_plan_status_to_operator_state(status: prism_ir::PlanStatus) -> PlanOperatorState {
    match status {
        prism_ir::PlanStatus::Abandoned => PlanOperatorState::Abandoned,
        prism_ir::PlanStatus::Archived => PlanOperatorState::Archived,
        prism_ir::PlanStatus::Draft
        | prism_ir::PlanStatus::Active
        | prism_ir::PlanStatus::Blocked
        | prism_ir::PlanStatus::Completed => PlanOperatorState::None,
    }
}

fn legacy_task_status_to_lifecycle(
    status: prism_ir::CoordinationTaskStatus,
) -> TaskLifecycleStatus {
    match status {
        prism_ir::CoordinationTaskStatus::Proposed
        | prism_ir::CoordinationTaskStatus::Ready
        | prism_ir::CoordinationTaskStatus::Blocked => TaskLifecycleStatus::Pending,
        prism_ir::CoordinationTaskStatus::InProgress
        | prism_ir::CoordinationTaskStatus::InReview
        | prism_ir::CoordinationTaskStatus::Validating => TaskLifecycleStatus::Active,
        prism_ir::CoordinationTaskStatus::Completed => TaskLifecycleStatus::Completed,
        prism_ir::CoordinationTaskStatus::Abandoned => TaskLifecycleStatus::Abandoned,
    }
}

fn task_id_from_legacy(task_id: &CoordinationTaskId) -> TaskId {
    TaskId::new(task_id.0.clone())
}

fn legacy_estimated_minutes(task: &CoordinationTask) -> u32 {
    task.metadata
        .get("estimatedMinutes")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn canonical_task_metadata(task: &CoordinationTask) -> Value {
    let mut metadata = task.metadata.clone();
    if let Some(object) = metadata.as_object_mut() {
        object
            .entry("legacy_kind".to_string())
            .or_insert_with(|| Value::String(format!("{:?}", task.kind).to_ascii_lowercase()));
        if let Some(phase) = legacy_phase(task.status) {
            object
                .entry("legacy_phase".to_string())
                .or_insert_with(|| Value::String(phase.to_string()));
        }
        if let Some(status) = task.published_task_status {
            object
                .entry("legacy_published_task_status".to_string())
                .or_insert_with(|| Value::String(format!("{:?}", status).to_ascii_lowercase()));
        }
        if let Some(agent) = &task.pending_handoff_to {
            object
                .entry("legacy_pending_handoff_to".to_string())
                .or_insert_with(|| Value::String(agent.0.to_string()));
        }
        if task.is_abstract {
            object
                .entry("legacy_is_abstract".to_string())
                .or_insert(Value::Bool(true));
        }
    } else {
        metadata = serde_json::json!({
            "legacy_kind": format!("{:?}", task.kind).to_ascii_lowercase(),
            "legacy_phase": legacy_phase(task.status),
            "legacy_published_task_status": task.published_task_status.map(|status| format!("{:?}", status).to_ascii_lowercase()),
            "legacy_pending_handoff_to": task.pending_handoff_to.as_ref().map(|agent| agent.0.to_string()),
            "legacy_is_abstract": task.is_abstract,
        });
    }
    metadata
}

fn legacy_phase(status: prism_ir::CoordinationTaskStatus) -> Option<&'static str> {
    match status {
        prism_ir::CoordinationTaskStatus::Blocked => Some("blocked"),
        prism_ir::CoordinationTaskStatus::InReview => Some("in_review"),
        prism_ir::CoordinationTaskStatus::Validating => Some("validating"),
        prism_ir::CoordinationTaskStatus::Proposed
        | prism_ir::CoordinationTaskStatus::Ready
        | prism_ir::CoordinationTaskStatus::InProgress
        | prism_ir::CoordinationTaskStatus::Completed
        | prism_ir::CoordinationTaskStatus::Abandoned => None,
    }
}

fn legacy_dependency_records(tasks: &[CoordinationTask]) -> Vec<CoordinationDependencyRecord> {
    let mut dependencies = Vec::new();
    let mut seen = BTreeSet::new();
    for task in tasks {
        let source = NodeRef::task(task_id_from_legacy(&task.id));
        for dependency in task
            .depends_on
            .iter()
            .chain(task.coordination_depends_on.iter())
            .chain(task.integrated_depends_on.iter())
        {
            let target = NodeRef::task(task_id_from_legacy(dependency));
            let edge_key = (
                source.kind,
                source.id.clone(),
                target.kind,
                target.id.clone(),
            );
            if seen.insert(edge_key) {
                dependencies.push(CoordinationDependencyRecord {
                    source: source.clone(),
                    target,
                });
            }
        }
    }
    dependencies
}

fn validate_legacy_projection_dependencies(tasks: &[CoordinationTask]) -> Result<()> {
    for task in tasks {
        let source = NodeRef::task(task_id_from_legacy(&task.id));
        let mut seen = BTreeSet::new();
        for dependency in task
            .depends_on
            .iter()
            .chain(task.coordination_depends_on.iter())
            .chain(task.integrated_depends_on.iter())
        {
            let target = NodeRef::task(task_id_from_legacy(dependency));
            let edge_key = (
                source.kind,
                source.id.clone(),
                target.kind,
                target.id.clone(),
            );
            if !seen.insert(edge_key) {
                return Err(anyhow::anyhow!(
                    "duplicate canonical dependency `{}` ({:?}) -> `{}` ({:?})",
                    source.id,
                    source.kind,
                    target.id,
                    target.kind
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{
        CoordinationTaskStatus, ExecutorClass, NodeRefKind, PlanNodeKind, PlanStatus, SessionId,
    };
    use serde_json::json;

    #[test]
    fn v2_snapshot_defaults_to_schema_version_2() {
        let snapshot = CoordinationSnapshotV2::default();
        assert_eq!(snapshot.schema_version, COORDINATION_SCHEMA_V2);
        assert!(snapshot.dependencies.is_empty());
    }

    #[test]
    fn v2_dependency_records_round_trip_typed_node_refs() {
        let dependency = CoordinationDependencyRecord {
            source: NodeRef::task(TaskId::new("task:alpha")),
            target: NodeRef::plan(PlanId::new("plan:beta")),
        };

        assert_eq!(dependency.source.kind, NodeRefKind::Task);
        assert_eq!(dependency.target.kind, NodeRefKind::Plan);
    }

    #[test]
    fn canonical_task_defaults_to_worktree_executor_policy() {
        let task = CanonicalTaskRecord {
            id: TaskId::new("task:alpha"),
            parent_plan_id: PlanId::new("plan:beta"),
            title: "task".to_string(),
            summary: None,
            lifecycle_status: TaskLifecycleStatus::Pending,
            estimated_minutes: 0,
            executor: TaskExecutorPolicy::default(),
            assignee: None,
            pending_handoff_to: None,
            session: None,
            lease_holder: None,
            lease_started_at: None,
            lease_refreshed_at: None,
            lease_stale_at: None,
            lease_expires_at: None,
            worktree_id: None,
            branch_ref: None,
            anchors: Vec::new(),
            bindings: PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            spec_refs: Vec::new(),
            metadata: Value::Null,
            git_execution: TaskGitExecution::default(),
        };

        assert_eq!(
            task.executor.executor_class,
            ExecutorClass::WorktreeExecutor
        );
    }

    #[test]
    fn legacy_snapshot_projects_into_v2_canonical_records() {
        let snapshot = CoordinationSnapshot {
            plans: vec![Plan {
                id: PlanId::new("plan:demo"),
                goal: "goal".into(),
                title: "title".into(),
                status: PlanStatus::Archived,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: vec!["rewrite".into()],
                created_from: Some("spec".into()),
                spec_refs: Vec::new(),
                metadata: json!({"source": "legacy"}),
            }],
            tasks: vec![CoordinationTask {
                id: CoordinationTaskId::new("coord-task:demo"),
                plan: PlanId::new("plan:demo"),
                kind: PlanNodeKind::Edit,
                title: "task".into(),
                summary: Some("summary".into()),
                status: CoordinationTaskStatus::InReview,
                published_task_status: Some(CoordinationTaskStatus::Ready),
                assignee: Some(AgentId::new("agent:demo")),
                pending_handoff_to: None,
                session: Some(SessionId::new("session:demo")),
                lease_holder: None,
                lease_started_at: None,
                lease_refreshed_at: None,
                lease_stale_at: None,
                lease_expires_at: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: PlanBinding::default(),
                depends_on: vec![CoordinationTaskId::new("coord-task:dep")],
                coordination_depends_on: vec![CoordinationTaskId::new("coord-task:coord")],
                integrated_depends_on: vec![CoordinationTaskId::new("coord-task:int")],
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: Some(3),
                tags: vec!["v2".into()],
                spec_refs: Vec::new(),
                metadata: json!({"estimatedMinutes": 25}),
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 1,
            next_task: 2,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };

        let v2 = snapshot.to_canonical_snapshot_v2();
        assert_eq!(v2.schema_version, COORDINATION_SCHEMA_V2);
        assert_eq!(v2.plans[0].operator_state, PlanOperatorState::Archived);
        assert_eq!(v2.plans[0].spec_refs.len(), 0);
        assert_eq!(v2.tasks[0].id, TaskId::new("coord-task:demo"));
        assert_eq!(v2.tasks[0].parent_plan_id, PlanId::new("plan:demo"));
        assert_eq!(v2.tasks[0].lifecycle_status, TaskLifecycleStatus::Active);
        assert_eq!(v2.tasks[0].estimated_minutes, 25);
        assert_eq!(v2.tasks[0].spec_refs.len(), 0);
        assert_eq!(v2.dependencies.len(), 3);
        assert_eq!(v2.dependencies[0].source.kind, NodeRefKind::Task);
        assert_eq!(v2.dependencies[0].target.kind, NodeRefKind::Task);
    }

    #[test]
    fn legacy_projection_dedupes_duplicate_logical_dependencies_across_buckets() {
        let snapshot = CoordinationSnapshot {
            plans: vec![Plan {
                id: PlanId::new("plan:demo"),
                goal: "goal".into(),
                title: "title".into(),
                status: PlanStatus::Active,
                policy: CoordinationPolicy::default(),
                scope: PlanScope::Repo,
                kind: PlanKind::TaskExecution,
                revision: 0,
                scheduling: PlanScheduling::default(),
                tags: Vec::new(),
                created_from: None,
                spec_refs: Vec::new(),
                metadata: Value::Null,
            }],
            tasks: vec![CoordinationTask {
                id: CoordinationTaskId::new("coord-task:demo"),
                plan: PlanId::new("plan:demo"),
                kind: PlanNodeKind::Edit,
                title: "task".into(),
                summary: None,
                status: CoordinationTaskStatus::Ready,
                published_task_status: None,
                assignee: None,
                pending_handoff_to: None,
                session: None,
                lease_holder: None,
                lease_started_at: None,
                lease_refreshed_at: None,
                lease_stale_at: None,
                lease_expires_at: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                bindings: PlanBinding::default(),
                depends_on: vec![CoordinationTaskId::new("coord-task:dep")],
                coordination_depends_on: vec![CoordinationTaskId::new("coord-task:dep")],
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                spec_refs: Vec::new(),
                metadata: Value::Null,
                git_execution: TaskGitExecution::default(),
            }],
            claims: Vec::new(),
            artifacts: Vec::new(),
            reviews: Vec::new(),
            events: Vec::new(),
            next_plan: 0,
            next_task: 0,
            next_claim: 0,
            next_artifact: 0,
            next_review: 0,
        };

        let v2 = snapshot.to_canonical_snapshot_v2();
        assert_eq!(v2.dependencies.len(), 1);
        assert_eq!(v2.dependencies[0].source.id, "coord-task:demo");
        assert_eq!(v2.dependencies[0].target.id, "coord-task:dep");
    }
}
