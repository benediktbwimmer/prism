use std::collections::{BTreeMap, BTreeSet};

use prism_ir::{
    AcceptanceEvidencePolicy, PlanAcceptanceCriterion, PlanBinding, PlanEdge, PlanEdgeId,
    PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanNode, PlanNodeId, PlanNodeStatus,
    ValidationRef,
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
        scope: plan.scope,
        kind: plan.kind,
        title: authored_plan_title(&plan),
        goal: plan.goal,
        status: plan.status,
        revision: plan.revision,
        root_nodes: plan
            .root_tasks
            .into_iter()
            .map(plan_node_id_from_task_id)
            .collect(),
        tags: plan.tags,
        created_from: plan.created_from,
        metadata: plan.metadata,
        nodes,
        edges,
    }
}

pub fn execution_overlays_from_tasks(tasks: &[CoordinationTask]) -> Vec<PlanExecutionOverlay> {
    let mut overlays = tasks
        .iter()
        .filter(|task| {
            task.pending_handoff_to.is_some()
                || task.session.is_some()
                || task.worktree_id.is_some()
                || task.branch_ref.is_some()
        })
        .map(|task| PlanExecutionOverlay {
            node_id: plan_node_id_from_task_id(task.id.clone()),
            pending_handoff_to: task.pending_handoff_to.clone(),
            session: task.session.clone(),
            worktree_id: task.worktree_id.clone(),
            branch_ref: task.branch_ref.clone(),
            effective_assignee: None,
            awaiting_handoff_from: None,
        })
        .collect::<Vec<_>>();
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

pub fn coordination_snapshot_from_plan_graphs(
    graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> CoordinationSnapshot {
    let mut snapshot = CoordinationSnapshot::default();
    for graph in graphs {
        snapshot.plans.push(plan_from_graph(graph));
        let overlays = execution_overlays
            .get(graph.id.0.as_str())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|overlay| (overlay.node_id.0.to_string(), overlay))
            .collect::<BTreeMap<_, _>>();
        let mut dependencies = BTreeMap::<String, Vec<prism_ir::CoordinationTaskId>>::new();
        for edge in &graph.edges {
            if edge.kind != PlanEdgeKind::DependsOn {
                continue;
            }
            dependencies
                .entry(edge.from.0.to_string())
                .or_default()
                .push(coordination_task_id_from_plan_node_id(edge.to.clone()));
        }
        for node in &graph.nodes {
            snapshot.tasks.push(task_from_plan_node(
                graph.id.clone(),
                node.clone(),
                dependencies.remove(node.id.0.as_str()).unwrap_or_default(),
                overlays.get(node.id.0.as_str()).cloned(),
            ));
        }
    }
    snapshot
        .plans
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
        .tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    snapshot
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
    let bindings = task_bindings(&task);
    PlanNode {
        id: plan_node_id_from_task_id(task.id),
        plan_id: task.plan,
        kind: task.kind,
        title: task.title,
        summary: task.summary,
        status: map_task_status(task.status),
        bindings,
        acceptance: task
            .acceptance
            .into_iter()
            .map(map_acceptance)
            .collect::<Vec<_>>(),
        validation_refs: task.validation_refs,
        is_abstract: task.is_abstract,
        assignee: task.assignee,
        base_revision: task.base_revision,
        priority: task.priority,
        tags: task.tags,
        metadata: task.metadata,
    }
}

fn plan_from_graph(graph: &PlanGraph) -> Plan {
    Plan {
        id: graph.id.clone(),
        goal: graph.goal.clone(),
        title: graph.title.clone(),
        status: graph.status,
        policy: crate::types::CoordinationPolicy::default(),
        scope: graph.scope,
        kind: graph.kind,
        revision: graph.revision,
        scheduling: crate::types::PlanScheduling::default(),
        tags: graph.tags.clone(),
        created_from: graph.created_from.clone(),
        metadata: graph.metadata.clone(),
        root_tasks: graph
            .root_nodes
            .iter()
            .cloned()
            .map(coordination_task_id_from_plan_node_id)
            .collect(),
    }
}

fn task_from_plan_node(
    plan_id: prism_ir::PlanId,
    node: PlanNode,
    depends_on: Vec<prism_ir::CoordinationTaskId>,
    execution: Option<PlanExecutionOverlay>,
) -> CoordinationTask {
    let anchors = node.bindings.anchors.clone();
    CoordinationTask {
        id: coordination_task_id_from_plan_node_id(node.id),
        plan: plan_id,
        kind: node.kind,
        title: node.title,
        summary: node.summary,
        status: map_plan_node_status(node.status),
        assignee: node.assignee,
        pending_handoff_to: execution
            .as_ref()
            .and_then(|overlay| overlay.pending_handoff_to.clone()),
        session: execution
            .as_ref()
            .and_then(|overlay| overlay.session.clone()),
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id: execution
            .as_ref()
            .and_then(|overlay| overlay.worktree_id.clone()),
        branch_ref: execution.and_then(|overlay| overlay.branch_ref),
        anchors,
        bindings: node.bindings,
        depends_on,
        acceptance: node
            .acceptance
            .into_iter()
            .map(map_plan_acceptance)
            .collect(),
        validation_refs: node.validation_refs,
        is_abstract: node.is_abstract,
        base_revision: node.base_revision,
        priority: node.priority,
        tags: node.tags,
        metadata: node.metadata,
    }
}

fn authored_plan_title(plan: &Plan) -> String {
    plan.title.clone()
}

fn task_bindings(task: &CoordinationTask) -> PlanBinding {
    let mut bindings = task.bindings.clone();
    if bindings.anchors.is_empty() {
        bindings.anchors = task.anchors.clone();
    }
    bindings
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

fn map_plan_acceptance(criterion: PlanAcceptanceCriterion) -> AcceptanceCriterion {
    AcceptanceCriterion {
        label: criterion.label,
        anchors: criterion.anchors,
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

fn map_plan_node_status(status: PlanNodeStatus) -> prism_ir::CoordinationTaskStatus {
    match status {
        PlanNodeStatus::Proposed => prism_ir::CoordinationTaskStatus::Proposed,
        PlanNodeStatus::Ready => prism_ir::CoordinationTaskStatus::Ready,
        PlanNodeStatus::InProgress => prism_ir::CoordinationTaskStatus::InProgress,
        PlanNodeStatus::Blocked | PlanNodeStatus::Waiting => {
            prism_ir::CoordinationTaskStatus::Blocked
        }
        PlanNodeStatus::InReview => prism_ir::CoordinationTaskStatus::InReview,
        PlanNodeStatus::Validating => prism_ir::CoordinationTaskStatus::Validating,
        PlanNodeStatus::Completed => prism_ir::CoordinationTaskStatus::Completed,
        PlanNodeStatus::Abandoned => prism_ir::CoordinationTaskStatus::Abandoned,
    }
}

fn plan_node_id_from_task_id(task_id: prism_ir::CoordinationTaskId) -> PlanNodeId {
    PlanNodeId::new(task_id.0)
}

fn coordination_task_id_from_plan_node_id(node_id: PlanNodeId) -> prism_ir::CoordinationTaskId {
    prism_ir::CoordinationTaskId::new(node_id.0)
}
