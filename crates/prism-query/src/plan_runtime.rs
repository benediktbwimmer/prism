use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use prism_coordination::{
    coordination_snapshot_from_plan_graphs, execution_overlays_from_tasks, snapshot_plan_graphs,
    AcceptanceCriterion, CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan,
};
use prism_ir::{
    AgentId, AnchorRef, CoordinationTaskId, PlanAcceptanceCriterion, PlanEdge, PlanEdgeId,
    PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanId, PlanNode, PlanNodeId, PlanNodeKind,
    PlanNodeStatus, ValidationRef, WorkspaceRevision,
};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePlanRuntimeState {
    graphs: BTreeMap<String, PlanGraph>,
    execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    policies: BTreeMap<String, CoordinationPolicy>,
    next_plan: u64,
    next_task: u64,
}

impl NativePlanRuntimeState {
    pub(crate) fn from_coordination_snapshot(snapshot: &CoordinationSnapshot) -> Self {
        let graphs = snapshot_plan_graphs(snapshot);
        let execution_overlays = execution_overlays_by_plan(snapshot);
        Self::from_snapshot_with_graphs_and_overlays(snapshot, graphs, execution_overlays)
    }

    pub(crate) fn from_snapshot_with_graphs_and_overlays(
        snapshot: &CoordinationSnapshot,
        graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let policies = snapshot
            .plans
            .iter()
            .cloned()
            .map(|plan| (plan.id.0.to_string(), plan.policy))
            .collect::<BTreeMap<_, _>>();
        let mut state = Self::from_graphs_and_overlays(graphs, execution_overlays);
        state.policies = policies;
        state.next_plan = snapshot.next_plan;
        state.next_task = snapshot.next_task;
        state
    }

    pub(crate) fn from_graphs_and_overlays(
        graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let graphs = graphs
            .into_iter()
            .map(|graph| (graph.id.0.to_string(), graph))
            .collect::<BTreeMap<_, _>>();
        let execution_overlays = execution_overlays
            .into_iter()
            .map(|(plan_id, overlays)| (plan_id, sort_execution_overlays(overlays)))
            .collect::<BTreeMap<_, _>>();
        Self {
            graphs,
            execution_overlays,
            policies: BTreeMap::new(),
            next_plan: 0,
            next_task: 0,
        }
    }

    pub(crate) fn plan_graph(&self, plan_id: &PlanId) -> Option<PlanGraph> {
        self.graphs.get(plan_id.0.as_str()).cloned()
    }

    pub(crate) fn plan_execution(&self, plan_id: &PlanId) -> Vec<PlanExecutionOverlay> {
        self.execution_overlays
            .get(plan_id.0.as_str())
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn apply_to_coordination_snapshot(
        &self,
        mut snapshot: CoordinationSnapshot,
    ) -> CoordinationSnapshot {
        let graphs = self.graphs.values().cloned().collect::<Vec<_>>();
        let mut plan_snapshot =
            coordination_snapshot_from_plan_graphs(&graphs, &self.execution_overlays);
        for plan in &mut plan_snapshot.plans {
            if let Some(policy) = self.policies.get(plan.id.0.as_str()) {
                plan.policy = policy.clone();
            }
        }
        snapshot.plans = plan_snapshot.plans;
        snapshot.tasks = plan_snapshot.tasks;
        snapshot.next_plan = snapshot.next_plan.max(self.next_plan);
        snapshot.next_task = snapshot.next_task.max(self.next_task);
        snapshot
    }

    pub(crate) fn create_plan_from_coordination(&mut self, plan: &Plan) -> Result<PlanId> {
        if self.graphs.contains_key(plan.id.0.as_str()) {
            return Err(anyhow!("plan `{}` already exists", plan.id.0));
        }
        self.next_plan = self
            .next_plan
            .max(counter_suffix(&plan.id.0, "plan:").unwrap_or(0));
        self.graphs.insert(
            plan.id.0.to_string(),
            PlanGraph {
                id: plan.id.clone(),
                scope: prism_ir::PlanScope::Repo,
                kind: prism_ir::PlanKind::TaskExecution,
                title: plan.goal.clone(),
                goal: plan.goal.clone(),
                status: plan.status,
                revision: 0,
                root_nodes: plan
                    .root_tasks
                    .iter()
                    .cloned()
                    .map(plan_node_id_from_task_id)
                    .collect(),
                tags: Vec::new(),
                created_from: None,
                metadata: Value::Null,
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        );
        self.execution_overlays
            .entry(plan.id.0.to_string())
            .or_default();
        self.policies
            .insert(plan.id.0.to_string(), plan.policy.clone());
        Ok(plan.id.clone())
    }

    pub(crate) fn update_plan_from_coordination(&mut self, plan: &Plan) -> Result<()> {
        let Some(graph) = self.graphs.get_mut(plan.id.0.as_str()) else {
            return Err(anyhow!("unknown plan `{}`", plan.id.0));
        };
        graph.title = plan.goal.clone();
        graph.goal = plan.goal.clone();
        graph.status = plan.status;
        graph.root_nodes = plan
            .root_tasks
            .iter()
            .cloned()
            .map(plan_node_id_from_task_id)
            .collect();
        self.policies
            .insert(plan.id.0.to_string(), plan.policy.clone());
        Ok(())
    }

    pub(crate) fn create_node(
        &mut self,
        plan_id: &PlanId,
        title: String,
        status: Option<PlanNodeStatus>,
        assignee: Option<AgentId>,
        anchors: Vec<AnchorRef>,
        depends_on: Vec<String>,
        acceptance: Vec<AcceptanceCriterion>,
        base_revision: WorkspaceRevision,
    ) -> Result<PlanNodeId> {
        let depends_on = dedupe_string_ids(depends_on);
        self.validate_dependency_targets(plan_id, &depends_on)?;
        self.next_task += 1;
        let node_id = PlanNodeId::new(format!("coord-task:{}", self.next_task));
        let graph = self
            .graphs
            .get_mut(plan_id.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
        graph.nodes.push(PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind: PlanNodeKind::Edit,
            title,
            summary: None,
            status: status.unwrap_or(PlanNodeStatus::Ready),
            bindings: prism_ir::PlanBinding {
                anchors: dedupe_anchors(anchors),
                concept_handles: Vec::new(),
                artifact_refs: Vec::new(),
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            },
            acceptance: acceptance
                .into_iter()
                .map(plan_acceptance_from_coordination)
                .collect(),
            is_abstract: false,
            assignee,
            base_revision,
            priority: None,
            tags: Vec::new(),
            metadata: Value::Null,
        });
        for dependency_id in depends_on {
            graph.edges.push(PlanEdge {
                id: dependency_edge_id(&node_id, dependency_id.as_str()),
                plan_id: plan_id.clone(),
                from: node_id.clone(),
                to: PlanNodeId::new(dependency_id),
                kind: PlanEdgeKind::DependsOn,
                summary: None,
                metadata: Value::Null,
            });
        }
        recompute_root_nodes(graph);
        Ok(node_id)
    }

    pub(crate) fn update_node(
        &mut self,
        node_id: &PlanNodeId,
        status: Option<PlanNodeStatus>,
        assignee: Option<Option<AgentId>>,
        title: Option<String>,
        anchors: Option<Vec<AnchorRef>>,
        depends_on: Option<Vec<String>>,
        acceptance: Option<Vec<AcceptanceCriterion>>,
        base_revision: Option<WorkspaceRevision>,
    ) -> Result<PlanId> {
        let (plan_key, node_index) = self
            .find_node(node_id)
            .ok_or_else(|| anyhow!("unknown plan node `{}`", node_id.0))?;
        if let Some(depends_on) = depends_on.as_ref() {
            self.validate_dependency_targets(&PlanId::new(plan_key.clone()), depends_on)?;
        }
        let graph = self
            .graphs
            .get_mut(plan_key.as_str())
            .expect("plan graph validated above");
        let node = graph
            .nodes
            .get_mut(node_index)
            .expect("node index validated above");
        if let Some(status) = status {
            node.status = status;
        }
        if let Some(assignee) = assignee {
            node.assignee = assignee;
        }
        if let Some(title) = title {
            node.title = title;
        }
        if let Some(anchors) = anchors {
            node.bindings.anchors = dedupe_anchors(anchors);
        }
        if let Some(acceptance) = acceptance {
            node.acceptance = acceptance
                .into_iter()
                .map(plan_acceptance_from_coordination)
                .collect();
        }
        if let Some(base_revision) = base_revision {
            node.base_revision = base_revision;
        }
        if let Some(depends_on) = depends_on {
            let dependency_targets = dedupe_string_ids(depends_on);
            graph
                .edges
                .retain(|edge| !(edge.kind == PlanEdgeKind::DependsOn && edge.from == *node_id));
            for dependency_id in dependency_targets {
                graph.edges.push(PlanEdge {
                    id: dependency_edge_id(node_id, dependency_id.as_str()),
                    plan_id: graph.id.clone(),
                    from: node_id.clone(),
                    to: PlanNodeId::new(dependency_id),
                    kind: PlanEdgeKind::DependsOn,
                    summary: None,
                    metadata: Value::Null,
                });
            }
            recompute_root_nodes(graph);
        }
        Ok(graph.id.clone())
    }

    pub(crate) fn create_edge(
        &mut self,
        plan_id: &PlanId,
        from_node_id: &PlanNodeId,
        to_node_id: &PlanNodeId,
        kind: PlanEdgeKind,
    ) -> Result<()> {
        let graph = self
            .graphs
            .get_mut(plan_id.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
        ensure_node_in_graph(graph, from_node_id)?;
        ensure_node_in_graph(graph, to_node_id)?;
        validate_edge_insertion(graph, from_node_id, to_node_id, kind)?;
        let edge_id = edge_id_for_kind(from_node_id, to_node_id, kind);
        if graph.edges.iter().any(|edge| edge.id == edge_id) {
            return Ok(());
        }
        graph.edges.push(PlanEdge {
            id: edge_id,
            plan_id: plan_id.clone(),
            from: from_node_id.clone(),
            to: to_node_id.clone(),
            kind,
            summary: None,
            metadata: Value::Null,
        });
        if kind == PlanEdgeKind::DependsOn {
            recompute_root_nodes(graph);
        }
        Ok(())
    }

    pub(crate) fn delete_edge(
        &mut self,
        plan_id: &PlanId,
        from_node_id: &PlanNodeId,
        to_node_id: &PlanNodeId,
        kind: PlanEdgeKind,
    ) -> Result<()> {
        let graph = self
            .graphs
            .get_mut(plan_id.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
        let previous_len = graph.edges.len();
        graph.edges.retain(|edge| {
            !(edge.from == *from_node_id && edge.to == *to_node_id && edge.kind == kind)
        });
        if graph.edges.len() == previous_len {
            return Err(anyhow!(
                "unknown plan edge `{}` -> `{}` ({:?})",
                from_node_id.0,
                to_node_id.0,
                kind
            ));
        }
        if kind == PlanEdgeKind::DependsOn {
            recompute_root_nodes(graph);
        }
        Ok(())
    }

    pub(crate) fn create_task_from_coordination(
        &mut self,
        task: &CoordinationTask,
    ) -> Result<PlanNodeId> {
        self.next_task = self
            .next_task
            .max(counter_suffix(&task.id.0, "coord-task:").unwrap_or(0));
        let node_id = plan_node_id_from_task_id(task.id.clone());
        let graph = self
            .graphs
            .get_mut(task.plan.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
        if graph.nodes.iter().any(|node| node.id == node_id) {
            return Err(anyhow!("plan node `{}` already exists", node_id.0));
        }
        graph.nodes.push(plan_node_from_coordination_task(task));
        sync_dependency_edges(graph, &node_id, &task.depends_on);
        recompute_root_nodes(graph);
        self.sync_execution_overlay(task);
        Ok(node_id)
    }

    pub(crate) fn update_task_from_coordination(
        &mut self,
        task: &CoordinationTask,
    ) -> Result<PlanId> {
        self.next_task = self
            .next_task
            .max(counter_suffix(&task.id.0, "coord-task:").unwrap_or(0));
        let node_id = plan_node_id_from_task_id(task.id.clone());
        let plan_id = {
            let graph = self
                .graphs
                .get_mut(task.plan.0.as_str())
                .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == node_id)
                .ok_or_else(|| anyhow!("unknown plan node `{}`", node_id.0))?;
            populate_plan_node_from_coordination_task(node, task);
            sync_dependency_edges(graph, &node_id, &task.depends_on);
            recompute_root_nodes(graph);
            graph.id.clone()
        };
        self.sync_execution_overlay(task);
        Ok(plan_id)
    }

    fn find_node(&self, node_id: &PlanNodeId) -> Option<(String, usize)> {
        self.graphs.iter().find_map(|(plan_id, graph)| {
            graph
                .nodes
                .iter()
                .position(|node| node.id == *node_id)
                .map(|index| (plan_id.clone(), index))
        })
    }

    fn validate_dependency_targets(&self, plan_id: &PlanId, depends_on: &[String]) -> Result<()> {
        let graph = self
            .graphs
            .get(plan_id.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
        let known_nodes = graph
            .nodes
            .iter()
            .map(|node| node.id.0.as_str())
            .collect::<BTreeSet<_>>();
        for dependency_id in depends_on {
            if !known_nodes.contains(dependency_id.as_str()) {
                return Err(anyhow!("unknown dependency task `{dependency_id}`"));
            }
        }
        Ok(())
    }

    fn sync_execution_overlay(&mut self, task: &CoordinationTask) {
        let plan_key = task.plan.0.to_string();
        let node_id = plan_node_id_from_task_id(task.id.clone());
        let overlays = self.execution_overlays.entry(plan_key).or_default();
        overlays.retain(|overlay| overlay.node_id != node_id);
        if task.pending_handoff_to.is_some() || task.session.is_some() {
            overlays.push(PlanExecutionOverlay {
                node_id,
                pending_handoff_to: task.pending_handoff_to.clone(),
                session: task.session.clone(),
            });
            *overlays = sort_execution_overlays(std::mem::take(overlays));
        }
    }
}

fn sort_execution_overlays(mut overlays: Vec<PlanExecutionOverlay>) -> Vec<PlanExecutionOverlay> {
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

fn execution_overlays_by_plan(
    snapshot: &CoordinationSnapshot,
) -> BTreeMap<String, Vec<PlanExecutionOverlay>> {
    snapshot
        .tasks
        .iter()
        .cloned()
        .fold(BTreeMap::new(), |mut map, task| {
            map.entry(task.plan.0.to_string())
                .or_insert_with(Vec::new)
                .push(task);
            map
        })
        .into_iter()
        .map(|(plan_id, tasks)| {
            (
                plan_id,
                sort_execution_overlays(execution_overlays_from_tasks(&tasks)),
            )
        })
        .collect()
}

fn plan_acceptance_from_coordination(criterion: AcceptanceCriterion) -> PlanAcceptanceCriterion {
    PlanAcceptanceCriterion {
        label: criterion.label,
        anchors: criterion.anchors,
        required_checks: Vec::<ValidationRef>::new(),
        evidence_policy: prism_ir::AcceptanceEvidencePolicy::Any,
    }
}

fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    let mut anchors = anchors;
    anchors.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    anchors.dedup();
    anchors
}

fn dedupe_string_ids(ids: Vec<String>) -> Vec<String> {
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    ids
}

fn plan_node_from_coordination_task(task: &CoordinationTask) -> PlanNode {
    let mut node = PlanNode {
        id: plan_node_id_from_task_id(task.id.clone()),
        plan_id: task.plan.clone(),
        kind: PlanNodeKind::Edit,
        title: String::new(),
        summary: None,
        status: map_coordination_task_status(task.status),
        bindings: prism_ir::PlanBinding::default(),
        acceptance: Vec::new(),
        is_abstract: false,
        assignee: None,
        base_revision: task.base_revision.clone(),
        priority: None,
        tags: Vec::new(),
        metadata: Value::Null,
    };
    populate_plan_node_from_coordination_task(&mut node, task);
    node
}

fn populate_plan_node_from_coordination_task(node: &mut PlanNode, task: &CoordinationTask) {
    node.title = task.title.clone();
    node.status = map_coordination_task_status(task.status);
    node.bindings.anchors = dedupe_anchors(task.anchors.clone());
    node.acceptance = task
        .acceptance
        .clone()
        .into_iter()
        .map(plan_acceptance_from_coordination)
        .collect();
    node.assignee = task.assignee.clone();
    node.base_revision = task.base_revision.clone();
}

fn sync_dependency_edges(
    graph: &mut PlanGraph,
    node_id: &PlanNodeId,
    depends_on: &[CoordinationTaskId],
) {
    graph
        .edges
        .retain(|edge| !(edge.kind == PlanEdgeKind::DependsOn && edge.from == *node_id));
    for dependency in depends_on {
        graph.edges.push(PlanEdge {
            id: dependency_edge_id(node_id, dependency.0.as_str()),
            plan_id: graph.id.clone(),
            from: node_id.clone(),
            to: plan_node_id_from_task_id(dependency.clone()),
            kind: PlanEdgeKind::DependsOn,
            summary: None,
            metadata: Value::Null,
        });
    }
}

fn plan_node_id_from_task_id(task_id: CoordinationTaskId) -> PlanNodeId {
    PlanNodeId::new(task_id.0)
}

fn map_coordination_task_status(status: prism_ir::CoordinationTaskStatus) -> PlanNodeStatus {
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

fn ensure_node_in_graph(graph: &PlanGraph, node_id: &PlanNodeId) -> Result<()> {
    if graph.nodes.iter().any(|node| node.id == *node_id) {
        Ok(())
    } else {
        Err(anyhow!("unknown plan node `{}`", node_id.0))
    }
}

fn recompute_root_nodes(graph: &mut PlanGraph) {
    let dependency_sources = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == PlanEdgeKind::DependsOn)
        .map(|edge| edge.from.0.to_string())
        .collect::<BTreeSet<_>>();
    graph.root_nodes = graph
        .nodes
        .iter()
        .filter(|node| !dependency_sources.contains(node.id.0.as_str()))
        .map(|node| node.id.clone())
        .collect();
}

fn dependency_edge_id(from: &PlanNodeId, to: &str) -> PlanEdgeId {
    PlanEdgeId::new(format!("plan-edge:{}:depends-on:{}", from.0, to))
}

fn edge_id_for_kind(from: &PlanNodeId, to: &PlanNodeId, kind: PlanEdgeKind) -> PlanEdgeId {
    PlanEdgeId::new(format!(
        "plan-edge:{}:{}:{}",
        from.0,
        plan_edge_kind_slug(kind),
        to.0
    ))
}

fn plan_edge_kind_slug(kind: PlanEdgeKind) -> &'static str {
    match kind {
        PlanEdgeKind::DependsOn => "depends-on",
        PlanEdgeKind::Blocks => "blocks",
        PlanEdgeKind::Informs => "informs",
        PlanEdgeKind::Validates => "validates",
        PlanEdgeKind::HandoffTo => "handoff-to",
        PlanEdgeKind::ChildOf => "child-of",
        PlanEdgeKind::RelatedTo => "related-to",
    }
}

fn counter_suffix(id: &str, prefix: &str) -> Option<u64> {
    id.strip_prefix(prefix)?.parse().ok()
}

fn validate_edge_insertion(
    graph: &PlanGraph,
    from_node_id: &PlanNodeId,
    to_node_id: &PlanNodeId,
    kind: PlanEdgeKind,
) -> Result<()> {
    if from_node_id == to_node_id {
        return Err(anyhow!(
            "plan edge `{}` -> `{}` ({:?}) cannot target itself",
            from_node_id.0,
            to_node_id.0,
            kind
        ));
    }
    if kind == PlanEdgeKind::ChildOf
        && graph.edges.iter().any(|edge| {
            edge.kind == PlanEdgeKind::ChildOf
                && edge.from == *from_node_id
                && edge.to != *to_node_id
        })
    {
        return Err(anyhow!(
            "plan node `{}` already has an authored parent",
            from_node_id.0
        ));
    }
    if edge_kind_requires_acyclic_graph(kind)
        && constrained_path_exists(graph, to_node_id, from_node_id)
    {
        return Err(anyhow!(
            "plan edge `{}` -> `{}` ({:?}) would introduce a cycle",
            from_node_id.0,
            to_node_id.0,
            kind
        ));
    }
    Ok(())
}

fn edge_kind_requires_acyclic_graph(kind: PlanEdgeKind) -> bool {
    matches!(
        kind,
        PlanEdgeKind::DependsOn
            | PlanEdgeKind::Blocks
            | PlanEdgeKind::Validates
            | PlanEdgeKind::HandoffTo
            | PlanEdgeKind::ChildOf
    )
}

fn constrained_path_exists(graph: &PlanGraph, start: &PlanNodeId, target: &PlanNodeId) -> bool {
    let mut pending = vec![start.clone()];
    let mut visited = BTreeSet::new();
    while let Some(node_id) = pending.pop() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        if node_id == *target {
            return true;
        }
        pending.extend(
            graph
                .edges
                .iter()
                .filter(|edge| edge_kind_requires_acyclic_graph(edge.kind) && edge.from == node_id)
                .map(|edge| edge.to.clone()),
        );
    }
    false
}
