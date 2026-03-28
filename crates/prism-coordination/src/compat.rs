use std::collections::BTreeSet;

use prism_ir::{
    AcceptanceEvidencePolicy, PlanAcceptanceCriterion, PlanBinding, PlanEdge, PlanEdgeId,
    PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanKind, PlanNode, PlanNodeId, PlanNodeKind,
    PlanNodeStatus, PlanScope, ValidationRef,
};
use serde_json::Value;

use crate::state::CoordinationStore;
use crate::types::{AcceptanceCriterion, CoordinationSnapshot, CoordinationTask, Plan};

pub fn snapshot_plan_graphs(snapshot: &CoordinationSnapshot) -> Vec<PlanGraph> {
    snapshot
        .plans
        .iter()
        .cloned()
        .map(|plan| {
            let tasks = snapshot
                .tasks
                .iter()
                .filter(|task| task.plan == plan.id)
                .cloned()
                .collect::<Vec<_>>();
            plan_graph_from_coordination(plan, tasks)
        })
        .collect()
}

pub fn plan_graph_from_coordination(plan: Plan, mut tasks: Vec<CoordinationTask>) -> PlanGraph {
    tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    let nodes = tasks
        .iter()
        .cloned()
        .map(plan_node_from_task)
        .collect::<Vec<_>>();
    let mut edges = tasks
        .iter()
        .flat_map(|task| dependency_edges_for_task(task))
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    PlanGraph {
        id: plan.id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: plan.goal.clone(),
        goal: plan.goal,
        status: plan.status,
        revision: 0,
        root_nodes: plan
            .root_tasks
            .into_iter()
            .map(plan_node_id_from_task_id)
            .collect(),
        tags: Vec::new(),
        created_from: None,
        metadata: Value::Null,
        nodes,
        edges,
    }
}

pub fn execution_overlays_from_tasks(tasks: &[CoordinationTask]) -> Vec<PlanExecutionOverlay> {
    let mut overlays = tasks
        .iter()
        .filter(|task| task.pending_handoff_to.is_some() || task.session.is_some())
        .map(|task| PlanExecutionOverlay {
            node_id: plan_node_id_from_task_id(task.id.clone()),
            pending_handoff_to: task.pending_handoff_to.clone(),
            session: task.session.clone(),
        })
        .collect::<Vec<_>>();
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

impl CoordinationStore {
    pub fn plan_graph(&self, plan_id: &prism_ir::PlanId) -> Option<PlanGraph> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let plan = state.plans.get(plan_id)?.clone();
        let tasks = state
            .tasks
            .values()
            .filter(|task| &task.plan == plan_id)
            .cloned()
            .collect::<Vec<_>>();
        Some(plan_graph_from_coordination(plan, tasks))
    }

    pub fn plan_execution_overlays(&self, plan_id: &prism_ir::PlanId) -> Vec<PlanExecutionOverlay> {
        let state = self.state.read().expect("coordination store lock poisoned");
        let tasks = state
            .tasks
            .values()
            .filter(|task| &task.plan == plan_id)
            .cloned()
            .collect::<Vec<_>>();
        execution_overlays_from_tasks(&tasks)
    }
}

fn plan_node_from_task(task: CoordinationTask) -> PlanNode {
    PlanNode {
        id: plan_node_id_from_task_id(task.id),
        plan_id: task.plan,
        kind: PlanNodeKind::Edit,
        title: task.title,
        summary: None,
        status: map_task_status(task.status),
        bindings: PlanBinding {
            anchors: task.anchors,
            concept_handles: Vec::new(),
            artifact_refs: Vec::new(),
            memory_refs: Vec::new(),
            outcome_refs: Vec::new(),
        },
        acceptance: task
            .acceptance
            .into_iter()
            .map(map_acceptance)
            .collect::<Vec<_>>(),
        is_abstract: false,
        assignee: task.assignee,
        base_revision: task.base_revision,
        priority: None,
        tags: Vec::new(),
        metadata: Value::Null,
    }
}

fn dependency_edges_for_task(task: &CoordinationTask) -> Vec<PlanEdge> {
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for dependency in &task.depends_on {
        if !seen.insert(dependency.0.to_string()) {
            continue;
        }
        edges.push(PlanEdge {
            id: PlanEdgeId::new(format!(
                "plan-edge:{}:depends-on:{}",
                task.id.0, dependency.0
            )),
            plan_id: task.plan.clone(),
            from: plan_node_id_from_task_id(task.id.clone()),
            to: plan_node_id_from_task_id(dependency.clone()),
            kind: PlanEdgeKind::DependsOn,
            summary: None,
            metadata: Value::Null,
        });
    }
    edges
}

fn map_acceptance(criterion: AcceptanceCriterion) -> PlanAcceptanceCriterion {
    PlanAcceptanceCriterion {
        label: criterion.label,
        anchors: criterion.anchors,
        required_checks: Vec::<ValidationRef>::new(),
        evidence_policy: AcceptanceEvidencePolicy::Any,
    }
}

fn map_task_status(status: prism_ir::CoordinationTaskStatus) -> PlanNodeStatus {
    match status {
        prism_ir::CoordinationTaskStatus::Proposed => PlanNodeStatus::Proposed,
        prism_ir::CoordinationTaskStatus::Ready => PlanNodeStatus::Ready,
        prism_ir::CoordinationTaskStatus::InProgress => PlanNodeStatus::InProgress,
        prism_ir::CoordinationTaskStatus::Blocked => PlanNodeStatus::Blocked,
        prism_ir::CoordinationTaskStatus::InReview => PlanNodeStatus::InReview,
        prism_ir::CoordinationTaskStatus::Validating => PlanNodeStatus::Validating,
        prism_ir::CoordinationTaskStatus::Completed => PlanNodeStatus::Completed,
        prism_ir::CoordinationTaskStatus::Abandoned => PlanNodeStatus::Abandoned,
    }
}

fn plan_node_id_from_task_id(task_id: prism_ir::CoordinationTaskId) -> PlanNodeId {
    PlanNodeId::new(task_id.0)
}
