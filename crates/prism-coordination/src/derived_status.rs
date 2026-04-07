use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use prism_ir::{
    BlockerCause, BlockerCauseSource, DerivedPlanStatus, EffectiveTaskStatus, NodeRef, PlanId,
    PlanOperatorState, TaskId, TaskLifecycleStatus,
};

use crate::{
    CanonicalCoordinationGraph, CanonicalNodeRecord, CanonicalTaskRecord, CoordinationSnapshotV2,
};

#[derive(Debug, Clone, PartialEq)]
pub struct DerivedTaskState {
    pub effective_status: EffectiveTaskStatus,
    pub graph_actionable: bool,
    pub blocker_causes: Vec<BlockerCause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DerivedPlanState {
    pub derived_status: DerivedPlanStatus,
    pub estimated_minutes_total: u32,
    pub remaining_estimated_minutes: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationDerivations {
    task_states: BTreeMap<String, DerivedTaskState>,
    plan_states: BTreeMap<String, DerivedPlanState>,
    graph_actionable_tasks: Vec<TaskId>,
}

impl CoordinationDerivations {
    pub fn derive(snapshot: &CoordinationSnapshotV2) -> Result<Self> {
        let graph = snapshot.graph()?;
        let descendant_tasks_by_plan = descendant_tasks_by_plan(snapshot, &graph);
        let ancestor_plans_by_task = ancestor_plans_by_task(snapshot);

        let mut task_statuses = snapshot
            .tasks
            .iter()
            .map(|task| (task.id.0.to_string(), base_task_status(task)))
            .collect::<BTreeMap<_, _>>();
        let mut plan_statuses = snapshot
            .plans
            .iter()
            .map(|plan| {
                let status = match plan.operator_state {
                    PlanOperatorState::Archived => DerivedPlanStatus::Archived,
                    PlanOperatorState::Abandoned => DerivedPlanStatus::Abandoned,
                    PlanOperatorState::None => DerivedPlanStatus::Pending,
                };
                (plan.id.0.to_string(), status)
            })
            .collect::<BTreeMap<_, _>>();
        let mut graph_actionable = BTreeSet::<String>::new();

        let max_iterations = snapshot.plans.len() + snapshot.tasks.len() + 1;
        for _ in 0..max_iterations {
            let next_graph_actionable = compute_graph_actionable(
                snapshot,
                &graph,
                &task_statuses,
                &plan_statuses,
                &ancestor_plans_by_task,
            );
            let next_plan_statuses = derive_plan_statuses(
                snapshot,
                &graph,
                &descendant_tasks_by_plan,
                &task_statuses,
                &plan_statuses,
                &next_graph_actionable,
            );
            let next_task_statuses = derive_task_statuses(
                snapshot,
                &graph,
                &task_statuses,
                &next_plan_statuses,
                &ancestor_plans_by_task,
            );
            if next_graph_actionable == graph_actionable
                && next_plan_statuses == plan_statuses
                && next_task_statuses == task_statuses
            {
                graph_actionable = next_graph_actionable;
                plan_statuses = next_plan_statuses;
                task_statuses = next_task_statuses;
                break;
            }
            graph_actionable = next_graph_actionable;
            plan_statuses = next_plan_statuses;
            task_statuses = next_task_statuses;
        }

        let task_states = snapshot
            .tasks
            .iter()
            .map(|task| {
                let effective_status = task_statuses[&task.id.0.to_string()];
                let graph_actionable = graph_actionable.contains(task.id.0.as_str());
                let blocker_causes = task_blocker_causes(
                    task,
                    &graph,
                    &task_statuses,
                    &plan_statuses,
                    &ancestor_plans_by_task,
                );
                (
                    task.id.0.to_string(),
                    DerivedTaskState {
                        effective_status,
                        graph_actionable,
                        blocker_causes,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let plan_states = snapshot
            .plans
            .iter()
            .map(|plan| {
                let descendant_tasks = descendant_tasks_by_plan
                    .get(plan.id.0.as_str())
                    .cloned()
                    .unwrap_or_default();
                let estimated_minutes_total = descendant_tasks
                    .iter()
                    .map(|task| task.estimated_minutes)
                    .sum();
                let remaining_estimated_minutes = descendant_tasks
                    .iter()
                    .filter(|task| {
                        matches!(
                            task_states[task.id.0.as_str()].effective_status,
                            EffectiveTaskStatus::Pending
                                | EffectiveTaskStatus::Active
                                | EffectiveTaskStatus::Blocked
                                | EffectiveTaskStatus::BrokenDependency
                        )
                    })
                    .map(|task| task.estimated_minutes)
                    .sum();
                (
                    plan.id.0.to_string(),
                    DerivedPlanState {
                        derived_status: plan_statuses[&plan.id.0.to_string()],
                        estimated_minutes_total,
                        remaining_estimated_minutes,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let graph_actionable_tasks = snapshot
            .tasks
            .iter()
            .filter(|task| graph_actionable.contains(task.id.0.as_str()))
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();

        Ok(Self {
            task_states,
            plan_states,
            graph_actionable_tasks,
        })
    }

    pub fn task_state(&self, task_id: &TaskId) -> Option<&DerivedTaskState> {
        self.task_states.get(task_id.0.as_str())
    }

    pub fn plan_state(&self, plan_id: &PlanId) -> Option<&DerivedPlanState> {
        self.plan_states.get(plan_id.0.as_str())
    }

    pub fn graph_actionable_tasks(&self) -> &[TaskId] {
        &self.graph_actionable_tasks
    }
}

fn descendant_tasks_by_plan<'a>(
    snapshot: &'a CoordinationSnapshotV2,
    graph: &CanonicalCoordinationGraph<'a>,
) -> BTreeMap<String, Vec<&'a CanonicalTaskRecord>> {
    let task_records = snapshot
        .tasks
        .iter()
        .map(|task| (task.id.0.to_string(), task))
        .collect::<BTreeMap<_, _>>();
    let mut descendant_tasks = BTreeMap::<String, Vec<&CanonicalTaskRecord>>::new();
    for node in graph.topological_order().iter().rev() {
        let Some(CanonicalNodeRecord::Plan(plan)) = graph.node(node) else {
            continue;
        };
        let mut tasks = Vec::new();
        for child in graph.children_of_plan(&plan.id) {
            match child.kind {
                prism_ir::NodeRefKind::Task => {
                    if let Some(task) = task_records.get(child.id.as_str()) {
                        tasks.push(*task);
                    }
                }
                prism_ir::NodeRefKind::Plan => {
                    if let Some(child_tasks) = descendant_tasks.get(child.id.as_str()) {
                        tasks.extend(child_tasks.iter().copied());
                    }
                }
            }
        }
        tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        tasks.dedup_by(|left, right| left.id == right.id);
        descendant_tasks.insert(plan.id.0.to_string(), tasks);
    }
    descendant_tasks
}

fn ancestor_plans_by_task(snapshot: &CoordinationSnapshotV2) -> BTreeMap<String, Vec<PlanId>> {
    let parent_by_plan = snapshot
        .plans
        .iter()
        .filter_map(|plan| {
            plan.parent_plan_id
                .as_ref()
                .map(|parent| (plan.id.0.to_string(), parent.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut ancestors = BTreeMap::new();
    for task in &snapshot.tasks {
        let mut lineage = Vec::new();
        let mut current = Some(task.parent_plan_id.clone());
        while let Some(plan_id) = current {
            lineage.push(plan_id.clone());
            current = parent_by_plan.get(plan_id.0.as_str()).cloned();
        }
        ancestors.insert(task.id.0.to_string(), lineage);
    }
    ancestors
}

fn derive_task_statuses(
    snapshot: &CoordinationSnapshotV2,
    graph: &CanonicalCoordinationGraph<'_>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    ancestor_plans_by_task: &BTreeMap<String, Vec<PlanId>>,
) -> BTreeMap<String, EffectiveTaskStatus> {
    snapshot
        .tasks
        .iter()
        .map(|task| {
            let status = derive_task_status(
                task,
                graph,
                task_statuses,
                plan_statuses,
                ancestor_plans_by_task,
            );
            (task.id.0.to_string(), status)
        })
        .collect()
}

fn derive_task_status(
    task: &CanonicalTaskRecord,
    graph: &CanonicalCoordinationGraph<'_>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    ancestor_plans_by_task: &BTreeMap<String, Vec<PlanId>>,
) -> EffectiveTaskStatus {
    match task.lifecycle_status {
        TaskLifecycleStatus::Completed => return EffectiveTaskStatus::Completed,
        TaskLifecycleStatus::Failed => return EffectiveTaskStatus::Failed,
        TaskLifecycleStatus::Abandoned => return EffectiveTaskStatus::Abandoned,
        TaskLifecycleStatus::Pending | TaskLifecycleStatus::Active => {}
    }

    let node_ref = NodeRef::task(task.id.clone());
    let dependencies = graph.dependency_targets(&node_ref);
    if dependencies
        .iter()
        .any(|target| node_status(target, plan_statuses, task_statuses) == NodeStatus::Abandoned)
    {
        return EffectiveTaskStatus::BrokenDependency;
    }
    if dependencies.iter().any(|target| {
        node_status(target, plan_statuses, task_statuses) == NodeStatus::BrokenDependency
    }) {
        return EffectiveTaskStatus::Blocked;
    }
    if dependencies
        .iter()
        .any(|target| !node_status(target, plan_statuses, task_statuses).is_completed())
    {
        return EffectiveTaskStatus::Blocked;
    }

    if ancestor_plans_by_task
        .get(task.id.0.as_str())
        .into_iter()
        .flatten()
        .any(|plan_id| plan_statuses.get(plan_id.0.as_str()) == Some(&DerivedPlanStatus::Blocked))
    {
        return EffectiveTaskStatus::Blocked;
    }
    if ancestor_plans_by_task
        .get(task.id.0.as_str())
        .into_iter()
        .flatten()
        .any(|plan_id| {
            plan_statuses.get(plan_id.0.as_str()) == Some(&DerivedPlanStatus::BrokenDependency)
        })
    {
        return EffectiveTaskStatus::BrokenDependency;
    }

    match task.lifecycle_status {
        TaskLifecycleStatus::Active => EffectiveTaskStatus::Active,
        TaskLifecycleStatus::Pending => EffectiveTaskStatus::Pending,
        TaskLifecycleStatus::Completed
        | TaskLifecycleStatus::Failed
        | TaskLifecycleStatus::Abandoned => unreachable!(),
    }
}

fn derive_plan_statuses(
    snapshot: &CoordinationSnapshotV2,
    graph: &CanonicalCoordinationGraph<'_>,
    descendant_tasks_by_plan: &BTreeMap<String, Vec<&CanonicalTaskRecord>>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    previous_plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    graph_actionable: &BTreeSet<String>,
) -> BTreeMap<String, DerivedPlanStatus> {
    let mut next = BTreeMap::new();
    for node in graph.topological_order().iter().rev() {
        let Some(CanonicalNodeRecord::Plan(plan)) = graph.node(node) else {
            continue;
        };
        let status = derive_plan_status(
            plan,
            graph,
            descendant_tasks_by_plan,
            task_statuses,
            previous_plan_statuses,
            &next,
            graph_actionable,
        );
        next.insert(plan.id.0.to_string(), status);
    }
    for plan in &snapshot.plans {
        next.entry(plan.id.0.to_string()).or_insert_with(|| {
            if plan.operator_state == PlanOperatorState::Archived {
                DerivedPlanStatus::Archived
            } else if plan.operator_state == PlanOperatorState::Abandoned {
                DerivedPlanStatus::Abandoned
            } else {
                DerivedPlanStatus::Pending
            }
        });
    }
    next
}

fn derive_plan_status(
    plan: &crate::CanonicalPlanRecord,
    graph: &CanonicalCoordinationGraph<'_>,
    descendant_tasks_by_plan: &BTreeMap<String, Vec<&CanonicalTaskRecord>>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    previous_plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    next_plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    graph_actionable: &BTreeSet<String>,
) -> DerivedPlanStatus {
    match plan.operator_state {
        PlanOperatorState::Archived => return DerivedPlanStatus::Archived,
        PlanOperatorState::Abandoned => return DerivedPlanStatus::Abandoned,
        PlanOperatorState::None => {}
    }

    let plan_ref = NodeRef::plan(plan.id.clone());
    let direct_dependencies = graph.dependency_targets(&plan_ref);
    if direct_dependencies.iter().any(|target| {
        node_status(target, previous_plan_statuses, task_statuses) == NodeStatus::Abandoned
    }) {
        return DerivedPlanStatus::BrokenDependency;
    }

    let direct_children = graph.children_of_plan(&plan.id);
    if direct_children.iter().any(|child| match child.kind {
        prism_ir::NodeRefKind::Task => {
            task_statuses.get(child.id.as_str()) == Some(&EffectiveTaskStatus::BrokenDependency)
        }
        prism_ir::NodeRefKind::Plan => {
            next_plan_statuses.get(child.id.as_str()) == Some(&DerivedPlanStatus::BrokenDependency)
        }
    }) {
        return DerivedPlanStatus::BrokenDependency;
    }

    let descendant_tasks = descendant_tasks_by_plan
        .get(plan.id.0.as_str())
        .cloned()
        .unwrap_or_default();
    if descendant_tasks.is_empty() {
        return if direct_dependencies.iter().any(|target| {
            !node_status(target, previous_plan_statuses, task_statuses).is_completed()
        }) {
            DerivedPlanStatus::Blocked
        } else {
            DerivedPlanStatus::Pending
        };
    }
    if descendant_tasks
        .iter()
        .all(|task| task_statuses.get(task.id.0.as_str()) == Some(&EffectiveTaskStatus::Completed))
    {
        return DerivedPlanStatus::Completed;
    }
    if descendant_tasks.iter().all(|task| {
        task_statuses.get(task.id.0.as_str()).is_some_and(|status| {
            matches!(
                status,
                EffectiveTaskStatus::Completed
                    | EffectiveTaskStatus::Failed
                    | EffectiveTaskStatus::Abandoned
            )
        })
    }) && descendant_tasks.iter().any(|task| {
        matches!(
            task_statuses.get(task.id.0.as_str()),
            Some(EffectiveTaskStatus::Failed | EffectiveTaskStatus::Abandoned)
        )
    }) {
        return DerivedPlanStatus::Failed;
    }
    if direct_dependencies
        .iter()
        .any(|target| !node_status(target, previous_plan_statuses, task_statuses).is_completed())
    {
        return DerivedPlanStatus::Blocked;
    }
    if descendant_tasks
        .iter()
        .any(|task| task_statuses.get(task.id.0.as_str()) == Some(&EffectiveTaskStatus::Active))
    {
        return DerivedPlanStatus::Active;
    }
    if descendant_tasks
        .iter()
        .all(|task| task_statuses.get(task.id.0.as_str()) == Some(&EffectiveTaskStatus::Pending))
    {
        return DerivedPlanStatus::Pending;
    }
    if descendant_tasks
        .iter()
        .any(|task| graph_actionable.contains(task.id.0.as_str()))
    {
        return DerivedPlanStatus::Active;
    }
    DerivedPlanStatus::Blocked
}

fn compute_graph_actionable(
    snapshot: &CoordinationSnapshotV2,
    graph: &CanonicalCoordinationGraph<'_>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    ancestor_plans_by_task: &BTreeMap<String, Vec<PlanId>>,
) -> BTreeSet<String> {
    snapshot
        .tasks
        .iter()
        .filter(|task| task_statuses.get(task.id.0.as_str()) == Some(&EffectiveTaskStatus::Pending))
        .filter(|task| {
            graph
                .dependency_targets(&NodeRef::task(task.id.clone()))
                .iter()
                .all(|target| {
                    let status = node_status(target, plan_statuses, task_statuses);
                    status.is_completed() && status != NodeStatus::Abandoned
                })
        })
        .filter(|task| {
            ancestor_plans_by_task
                .get(task.id.0.as_str())
                .into_iter()
                .flatten()
                .all(|plan_id| {
                    matches!(
                        plan_statuses.get(plan_id.0.as_str()),
                        Some(DerivedPlanStatus::Pending | DerivedPlanStatus::Active)
                    )
                })
        })
        .map(|task| task.id.0.to_string())
        .collect()
}

fn task_blocker_causes(
    task: &CanonicalTaskRecord,
    graph: &CanonicalCoordinationGraph<'_>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
    plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    ancestor_plans_by_task: &BTreeMap<String, Vec<PlanId>>,
) -> Vec<BlockerCause> {
    let mut causes = Vec::new();
    for dependency in graph.dependency_targets(&NodeRef::task(task.id.clone())) {
        match node_status(&dependency, plan_statuses, task_statuses) {
            NodeStatus::Abandoned => causes.push(graph_cause("dependency_abandoned")),
            NodeStatus::Failed => causes.push(graph_cause("dependency_failed")),
            NodeStatus::Completed => {}
            NodeStatus::Pending
            | NodeStatus::Active
            | NodeStatus::Blocked
            | NodeStatus::BrokenDependency
            | NodeStatus::Archived => causes.push(graph_cause("dependency_incomplete")),
        }
    }
    for plan_id in ancestor_plans_by_task
        .get(task.id.0.as_str())
        .into_iter()
        .flatten()
    {
        match plan_statuses.get(plan_id.0.as_str()) {
            Some(DerivedPlanStatus::Blocked) => causes.push(graph_cause("ancestor_plan_blocked")),
            Some(DerivedPlanStatus::BrokenDependency) => {
                causes.push(graph_cause("ancestor_plan_broken_dependency"))
            }
            _ => {}
        }
    }
    dedupe_causes(causes)
}

fn dedupe_causes(causes: Vec<BlockerCause>) -> Vec<BlockerCause> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for cause in causes {
        let key = (
            format!("{:?}", cause.source),
            cause.code.clone(),
            cause.acceptance_label.clone(),
            cause.threshold_metric.clone(),
        );
        if seen.insert(key) {
            deduped.push(cause);
        }
    }
    deduped
}

fn graph_cause(code: &str) -> BlockerCause {
    BlockerCause {
        source: BlockerCauseSource::DependencyGraph,
        code: Some(code.to_string()),
        acceptance_label: None,
        threshold_metric: None,
        threshold_value: None,
        observed_value: None,
    }
}

fn base_task_status(task: &CanonicalTaskRecord) -> EffectiveTaskStatus {
    match task.lifecycle_status {
        TaskLifecycleStatus::Pending => EffectiveTaskStatus::Pending,
        TaskLifecycleStatus::Active => EffectiveTaskStatus::Active,
        TaskLifecycleStatus::Completed => EffectiveTaskStatus::Completed,
        TaskLifecycleStatus::Failed => EffectiveTaskStatus::Failed,
        TaskLifecycleStatus::Abandoned => EffectiveTaskStatus::Abandoned,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeStatus {
    Pending,
    Active,
    Blocked,
    BrokenDependency,
    Completed,
    Failed,
    Abandoned,
    Archived,
}

impl NodeStatus {
    fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }
}

fn node_status(
    node_ref: &NodeRef,
    plan_statuses: &BTreeMap<String, DerivedPlanStatus>,
    task_statuses: &BTreeMap<String, EffectiveTaskStatus>,
) -> NodeStatus {
    match node_ref.kind {
        prism_ir::NodeRefKind::Plan => plan_statuses
            .get(node_ref.id.as_str())
            .copied()
            .map(NodeStatus::from)
            .unwrap_or(NodeStatus::Pending),
        prism_ir::NodeRefKind::Task => task_statuses
            .get(node_ref.id.as_str())
            .copied()
            .map(NodeStatus::from)
            .unwrap_or(NodeStatus::Pending),
    }
}

impl From<DerivedPlanStatus> for NodeStatus {
    fn from(value: DerivedPlanStatus) -> Self {
        match value {
            DerivedPlanStatus::Pending => Self::Pending,
            DerivedPlanStatus::Active => Self::Active,
            DerivedPlanStatus::Blocked => Self::Blocked,
            DerivedPlanStatus::BrokenDependency => Self::BrokenDependency,
            DerivedPlanStatus::Completed => Self::Completed,
            DerivedPlanStatus::Failed => Self::Failed,
            DerivedPlanStatus::Abandoned => Self::Abandoned,
            DerivedPlanStatus::Archived => Self::Archived,
        }
    }
}

impl From<EffectiveTaskStatus> for NodeStatus {
    fn from(value: EffectiveTaskStatus) -> Self {
        match value {
            EffectiveTaskStatus::Pending => Self::Pending,
            EffectiveTaskStatus::Active => Self::Active,
            EffectiveTaskStatus::Blocked => Self::Blocked,
            EffectiveTaskStatus::BrokenDependency => Self::BrokenDependency,
            EffectiveTaskStatus::Completed => Self::Completed,
            EffectiveTaskStatus::Failed => Self::Failed,
            EffectiveTaskStatus::Abandoned => Self::Abandoned,
        }
    }
}

#[cfg(test)]
mod tests {
    use prism_ir::{
        ExecutorClass, PlanBinding, PlanKind, PlanScope, TaskExecutorPolicy, ValidationRef,
        WorkspaceRevision,
    };
    use serde_json::Value;

    use crate::{
        git_execution::TaskGitExecution, types::CoordinationPolicy, CanonicalPlanRecord,
        CanonicalTaskRecord, CoordinationDependencyRecord,
    };

    use super::*;

    fn plan(
        id: &str,
        parent_plan_id: Option<&str>,
        operator_state: PlanOperatorState,
    ) -> CanonicalPlanRecord {
        CanonicalPlanRecord {
            id: PlanId::new(id),
            parent_plan_id: parent_plan_id.map(PlanId::new),
            title: id.to_string(),
            goal: "goal".to_string(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            policy: CoordinationPolicy::default(),
            scheduling: crate::PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: Value::Null,
            operator_state,
        }
    }

    fn task(
        id: &str,
        parent_plan_id: &str,
        lifecycle_status: TaskLifecycleStatus,
        estimated_minutes: u32,
    ) -> CanonicalTaskRecord {
        CanonicalTaskRecord {
            id: TaskId::new(id),
            parent_plan_id: PlanId::new(parent_plan_id),
            title: id.to_string(),
            summary: None,
            kind: prism_ir::PlanNodeKind::Edit,
            status: match lifecycle_status {
                TaskLifecycleStatus::Pending => prism_ir::CoordinationTaskStatus::Proposed,
                TaskLifecycleStatus::Active => prism_ir::CoordinationTaskStatus::InProgress,
                TaskLifecycleStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
                TaskLifecycleStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
                TaskLifecycleStatus::Failed => prism_ir::CoordinationTaskStatus::Blocked,
            },
            lifecycle_status,
            estimated_minutes,
            executor: TaskExecutorPolicy {
                executor_class: ExecutorClass::WorktreeExecutor,
                target_label: None,
                allowed_principals: Vec::new(),
            },
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
            validation_refs: Vec::<ValidationRef>::new(),
            is_abstract: false,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: Value::Null,
            git_execution: TaskGitExecution::default(),
        }
    }

    #[test]
    fn derivations_mark_abandoned_dependencies_as_broken() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None, PlanOperatorState::None)],
            tasks: vec![
                task("task:dep", "plan:root", TaskLifecycleStatus::Abandoned, 5),
                task("task:main", "plan:root", TaskLifecycleStatus::Pending, 10),
            ],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::task(TaskId::new("task:main")),
                target: NodeRef::task(TaskId::new("task:dep")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let derivations = CoordinationDerivations::derive(&snapshot).unwrap();
        let task_state = derivations.task_state(&TaskId::new("task:main")).unwrap();
        assert_eq!(
            task_state.effective_status,
            EffectiveTaskStatus::BrokenDependency
        );
        assert_eq!(
            task_state.blocker_causes[0].code.as_deref(),
            Some("dependency_abandoned")
        );
        assert_eq!(
            derivations
                .plan_state(&PlanId::new("plan:root"))
                .unwrap()
                .derived_status,
            DerivedPlanStatus::BrokenDependency
        );
    }

    #[test]
    fn derivations_keep_failed_dependencies_blocked_not_broken() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None, PlanOperatorState::None)],
            tasks: vec![
                task("task:dep", "plan:root", TaskLifecycleStatus::Failed, 5),
                task("task:main", "plan:root", TaskLifecycleStatus::Pending, 10),
            ],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::task(TaskId::new("task:main")),
                target: NodeRef::task(TaskId::new("task:dep")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let derivations = CoordinationDerivations::derive(&snapshot).unwrap();
        let task_state = derivations.task_state(&TaskId::new("task:main")).unwrap();
        assert_eq!(task_state.effective_status, EffectiveTaskStatus::Blocked);
        assert!(task_state
            .blocker_causes
            .iter()
            .any(|cause| cause.code.as_deref() == Some("dependency_failed")));
    }

    #[test]
    fn derivations_propagate_blocked_ancestor_plans() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![
                plan("plan:parent", None, PlanOperatorState::None),
                plan("plan:child", Some("plan:parent"), PlanOperatorState::None),
                plan("plan:gate", None, PlanOperatorState::None),
            ],
            tasks: vec![
                task("task:gate", "plan:gate", TaskLifecycleStatus::Pending, 5),
                task("task:child", "plan:child", TaskLifecycleStatus::Pending, 10),
            ],
            dependencies: vec![CoordinationDependencyRecord {
                source: NodeRef::plan(PlanId::new("plan:parent")),
                target: NodeRef::task(TaskId::new("task:gate")),
            }],
            ..CoordinationSnapshotV2::default()
        };

        let derivations = CoordinationDerivations::derive(&snapshot).unwrap();
        assert_eq!(
            derivations
                .plan_state(&PlanId::new("plan:parent"))
                .unwrap()
                .derived_status,
            DerivedPlanStatus::Blocked
        );
        assert_eq!(
            derivations
                .plan_state(&PlanId::new("plan:child"))
                .unwrap()
                .derived_status,
            DerivedPlanStatus::Blocked
        );
        let task_state = derivations.task_state(&TaskId::new("task:child")).unwrap();
        assert_eq!(task_state.effective_status, EffectiveTaskStatus::Blocked);
        assert!(task_state
            .blocker_causes
            .iter()
            .any(|cause| cause.code.as_deref() == Some("ancestor_plan_blocked")));
    }

    #[test]
    fn derivations_compute_estimates_and_graph_actionability() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None, PlanOperatorState::None)],
            tasks: vec![
                task("task:done", "plan:root", TaskLifecycleStatus::Completed, 5),
                task("task:ready", "plan:root", TaskLifecycleStatus::Pending, 10),
            ],
            ..CoordinationSnapshotV2::default()
        };

        let derivations = CoordinationDerivations::derive(&snapshot).unwrap();
        let plan_state = derivations.plan_state(&PlanId::new("plan:root")).unwrap();
        assert_eq!(plan_state.estimated_minutes_total, 15);
        assert_eq!(plan_state.remaining_estimated_minutes, 10);
        assert_eq!(plan_state.derived_status, DerivedPlanStatus::Active);
        assert_eq!(
            derivations.graph_actionable_tasks(),
            &[TaskId::new("task:ready")]
        );
        assert!(
            derivations
                .task_state(&TaskId::new("task:ready"))
                .unwrap()
                .graph_actionable
        );
    }

    #[test]
    fn derivations_respect_archived_operator_state() {
        let snapshot = CoordinationSnapshotV2 {
            plans: vec![plan("plan:root", None, PlanOperatorState::Archived)],
            tasks: vec![task(
                "task:ready",
                "plan:root",
                TaskLifecycleStatus::Pending,
                10,
            )],
            ..CoordinationSnapshotV2::default()
        };

        let derivations = CoordinationDerivations::derive(&snapshot).unwrap();
        assert_eq!(
            derivations
                .plan_state(&PlanId::new("plan:root"))
                .unwrap()
                .derived_status,
            DerivedPlanStatus::Archived
        );
        assert_eq!(
            derivations
                .task_state(&TaskId::new("task:ready"))
                .unwrap()
                .effective_status,
            EffectiveTaskStatus::Pending
        );
        assert!(derivations.graph_actionable_tasks().is_empty());
    }
}
