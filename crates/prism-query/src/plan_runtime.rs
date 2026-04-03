use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use prism_coordination::{
    coordination_snapshot_from_plan_graphs, execution_overlays_from_tasks, snapshot_plan_graphs,
    AcceptanceCriterion, CoordinationPolicy, CoordinationSnapshot, CoordinationTask, Plan,
    PlanScheduling,
};
use prism_ir::{
    new_prefixed_id, AgentId, AnchorRef, BlockerCause, BlockerCauseSource, CoordinationTaskId,
    PlanAcceptanceCriterion, PlanBinding, PlanEdge, PlanEdgeId, PlanEdgeKind, PlanExecutionOverlay,
    PlanGraph, PlanId, PlanKind, PlanNode, PlanNodeBlocker, PlanNodeBlockerKind, PlanNodeId,
    PlanNodeKind, PlanNodeStatus, ValidationRef, WorkspaceRevision,
};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePlanRuntimeState {
    graphs: BTreeMap<String, PlanGraph>,
    execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    policies: BTreeMap<String, CoordinationPolicy>,
    schedules: BTreeMap<String, PlanScheduling>,
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
        let schedules = snapshot
            .plans
            .iter()
            .cloned()
            .map(|plan| (plan.id.0.to_string(), plan.scheduling))
            .collect::<BTreeMap<_, _>>();
        let mut state = Self::from_graphs_and_overlays(graphs, execution_overlays);
        state.policies = policies;
        state.schedules = schedules;
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
            schedules: BTreeMap::new(),
            next_plan: 0,
            next_task: 0,
        }
    }

    pub(crate) fn plan_graph(&self, plan_id: &PlanId) -> Option<PlanGraph> {
        self.graphs.get(plan_id.0.as_str()).cloned()
    }

    pub(crate) fn plan_graphs(&self) -> Vec<PlanGraph> {
        self.graphs.values().cloned().collect()
    }

    pub(crate) fn execution_overlays_by_plan(&self) -> BTreeMap<String, Vec<PlanExecutionOverlay>> {
        self.execution_overlays.clone()
    }

    pub(crate) fn plan_execution(&self, plan_id: &PlanId) -> Vec<PlanExecutionOverlay> {
        let Some(graph) = self.graphs.get(plan_id.0.as_str()) else {
            return Vec::new();
        };
        let overlays = self
            .execution_overlays
            .get(plan_id.0.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        derive_execution_overlays(graph, overlays)
    }

    pub(crate) fn policy(&self, plan_id: &PlanId) -> Option<CoordinationPolicy> {
        self.policies.get(plan_id.0.as_str()).cloned()
    }

    pub(crate) fn scheduling(&self, plan_id: &PlanId) -> Option<PlanScheduling> {
        self.schedules.get(plan_id.0.as_str()).cloned()
    }

    pub(crate) fn apply_to_coordination_snapshot(
        &self,
        mut snapshot: CoordinationSnapshot,
    ) -> CoordinationSnapshot {
        let task_runtime_scope = snapshot
            .tasks
            .iter()
            .map(|task| {
                (
                    task.id.clone(),
                    (
                        task.pending_handoff_to.clone(),
                        task.session.clone(),
                        task.worktree_id.clone(),
                        task.branch_ref.clone(),
                        task.git_execution.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let graphs = self.graphs.values().cloned().collect::<Vec<_>>();
        let mut plan_snapshot =
            coordination_snapshot_from_plan_graphs(&graphs, &self.execution_overlays);
        for plan in &mut plan_snapshot.plans {
            if let Some(policy) = self.policies.get(plan.id.0.as_str()) {
                plan.policy = policy.clone();
            }
            if let Some(scheduling) = self.schedules.get(plan.id.0.as_str()) {
                plan.scheduling = scheduling.clone();
            }
        }
        for task in &mut plan_snapshot.tasks {
            if let Some((pending_handoff_to, session, worktree_id, branch_ref, git_execution)) =
                task_runtime_scope.get(&task.id)
            {
                task.pending_handoff_to = pending_handoff_to.clone();
                task.session = session.clone();
                task.worktree_id = worktree_id.clone();
                task.branch_ref = branch_ref.clone();
                task.git_execution = git_execution.clone();
            }
        }
        snapshot.plans = plan_snapshot.plans;
        snapshot.tasks = plan_snapshot.tasks;
        snapshot.next_plan = snapshot.next_plan.max(self.next_plan);
        snapshot.next_task = snapshot.next_task.max(self.next_task);
        snapshot
    }

    pub(crate) fn apply_task_execution_authored_fields_to_coordination_snapshot(
        &self,
        mut snapshot: CoordinationSnapshot,
    ) -> CoordinationSnapshot {
        let task_runtime_scope = snapshot
            .tasks
            .iter()
            .map(|task| {
                (
                    task.id.clone(),
                    (
                        task.pending_handoff_to.clone(),
                        task.session.clone(),
                        task.worktree_id.clone(),
                        task.branch_ref.clone(),
                        task.git_execution.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let task_execution_graphs = self
            .graphs
            .values()
            .filter(|graph| graph.kind == PlanKind::TaskExecution)
            .cloned()
            .map(|mut graph| {
                let task_backed_node_ids = graph
                    .nodes
                    .iter()
                    .filter(|node| is_task_backed_plan_node_id(node.id.0.as_str()))
                    .map(|node| node.id.clone())
                    .collect::<BTreeSet<_>>();
                graph
                    .nodes
                    .retain(|node| task_backed_node_ids.contains(&node.id));
                graph.edges.retain(|edge| {
                    task_backed_node_ids.contains(&edge.from)
                        && task_backed_node_ids.contains(&edge.to)
                });
                graph
                    .root_nodes
                    .retain(|node_id| task_backed_node_ids.contains(node_id));
                graph
            })
            .filter(|graph| !graph.nodes.is_empty())
            .collect::<Vec<_>>();
        if task_execution_graphs.is_empty() {
            return snapshot;
        }
        let task_execution_plan_ids = task_execution_graphs
            .iter()
            .map(|graph| graph.id.0.to_string())
            .collect::<BTreeSet<_>>();
        let task_execution_overlays = self
            .execution_overlays
            .iter()
            .filter(|(plan_id, _)| task_execution_plan_ids.contains(plan_id.as_str()))
            .map(|(plan_id, overlays)| (plan_id.clone(), overlays.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut plan_snapshot = coordination_snapshot_from_plan_graphs(
            &task_execution_graphs,
            &task_execution_overlays,
        );
        for plan in &mut plan_snapshot.plans {
            if let Some(policy) = self.policies.get(plan.id.0.as_str()) {
                plan.policy = policy.clone();
            }
            if let Some(scheduling) = self.schedules.get(plan.id.0.as_str()) {
                plan.scheduling = scheduling.clone();
            }
        }
        snapshot
            .plans
            .retain(|plan| !task_execution_plan_ids.contains(plan.id.0.as_str()));
        snapshot
            .tasks
            .retain(|task| !task_execution_plan_ids.contains(task.plan.0.as_str()));
        snapshot.plans.extend(plan_snapshot.plans);
        snapshot
            .tasks
            .extend(plan_snapshot.tasks.into_iter().map(|mut task| {
                if let Some((pending_handoff_to, session, worktree_id, branch_ref, git_execution)) =
                    task_runtime_scope.get(&task.id)
                {
                    task.pending_handoff_to = pending_handoff_to.clone();
                    task.session = session.clone();
                    task.worktree_id = worktree_id.clone();
                    task.branch_ref = branch_ref.clone();
                    task.git_execution = git_execution.clone();
                }
                task
            }));
        snapshot
            .plans
            .sort_by(|left, right| left.id.0.cmp(&right.id.0));
        snapshot
            .tasks
            .sort_by(|left, right| left.id.0.cmp(&right.id.0));
        snapshot.next_plan = snapshot.next_plan.max(self.next_plan);
        snapshot.next_task = snapshot.next_task.max(self.next_task);
        snapshot
    }

    pub(crate) fn sync_task_execution_plan_statuses_from_coordination_snapshot(
        &mut self,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        for plan in snapshot
            .plans
            .iter()
            .filter(|plan| plan.kind == PlanKind::TaskExecution)
        {
            if let Some(graph) = self.graphs.get_mut(plan.id.0.as_str()) {
                graph.status = plan.status;
                graph.scope = plan.scope;
            } else {
                self.create_plan_from_coordination(plan)?;
                for task in snapshot.tasks.iter().filter(|task| task.plan == plan.id) {
                    self.create_task_from_coordination(task)?;
                }
            }
            self.policies
                .insert(plan.id.0.to_string(), plan.policy.clone());
            self.schedules
                .insert(plan.id.0.to_string(), plan.scheduling.clone());
        }
        self.next_plan = self.next_plan.max(snapshot.next_plan);
        self.next_task = self.next_task.max(snapshot.next_task);
        Ok(())
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
                scope: plan.scope,
                kind: plan.kind,
                title: authored_plan_title(plan),
                goal: plan.goal.clone(),
                status: plan.status,
                revision: plan.revision,
                root_nodes: plan
                    .root_tasks
                    .iter()
                    .cloned()
                    .map(plan_node_id_from_task_id)
                    .collect(),
                tags: plan.tags.clone(),
                created_from: plan.created_from.clone(),
                metadata: plan.metadata.clone(),
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        );
        self.execution_overlays
            .entry(plan.id.0.to_string())
            .or_default();
        self.policies
            .insert(plan.id.0.to_string(), plan.policy.clone());
        self.schedules
            .insert(plan.id.0.to_string(), plan.scheduling.clone());
        Ok(plan.id.clone())
    }

    pub(crate) fn update_plan_from_coordination(&mut self, plan: &Plan) -> Result<()> {
        let Some(graph) = self.graphs.get_mut(plan.id.0.as_str()) else {
            return Err(anyhow!("unknown plan `{}`", plan.id.0));
        };
        graph.scope = plan.scope;
        graph.kind = plan.kind;
        graph.title = authored_plan_title(plan);
        graph.goal = plan.goal.clone();
        graph.status = plan.status;
        graph.revision = plan.revision;
        graph.root_nodes = plan
            .root_tasks
            .iter()
            .cloned()
            .map(plan_node_id_from_task_id)
            .collect();
        graph.tags = plan.tags.clone();
        graph.created_from = plan.created_from.clone();
        graph.metadata = plan.metadata.clone();
        self.policies
            .insert(plan.id.0.to_string(), plan.policy.clone());
        self.schedules
            .insert(plan.id.0.to_string(), plan.scheduling.clone());
        Ok(())
    }

    pub(crate) fn update_task_and_plan_from_coordination(
        &mut self,
        task: &CoordinationTask,
        plan: &Plan,
    ) -> Result<PlanId> {
        let plan_id = self.update_task_from_coordination(task)?;
        self.update_plan_from_coordination(plan)?;
        Ok(plan_id)
    }

    pub(crate) fn create_node(
        &mut self,
        plan_id: &PlanId,
        kind: PlanNodeKind,
        title: String,
        summary: Option<String>,
        status: Option<PlanNodeStatus>,
        assignee: Option<AgentId>,
        is_abstract: bool,
        bindings: PlanBinding,
        depends_on: Vec<String>,
        acceptance: Vec<PlanAcceptanceCriterion>,
        validation_refs: Vec<ValidationRef>,
        base_revision: WorkspaceRevision,
        priority: Option<u8>,
        tags: Vec<String>,
    ) -> Result<PlanNodeId> {
        let depends_on = dedupe_string_ids(depends_on);
        self.validate_dependency_targets(plan_id, &depends_on)?;
        self.next_task += 1;
        let node_id = PlanNodeId::new(new_prefixed_id("coord-task"));
        let graph = self
            .graphs
            .get_mut(plan_id.0.as_str())
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
        graph.nodes.push(PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind,
            title,
            summary,
            status: status.unwrap_or(PlanNodeStatus::Ready),
            bindings: normalize_plan_binding(bindings),
            acceptance: normalize_plan_acceptance(acceptance),
            validation_refs: normalize_validation_refs(validation_refs),
            is_abstract,
            assignee,
            base_revision,
            priority,
            tags: normalize_string_refs(tags),
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
        kind: Option<PlanNodeKind>,
        status: Option<PlanNodeStatus>,
        assignee: Option<Option<AgentId>>,
        is_abstract: Option<bool>,
        title: Option<String>,
        summary: Option<String>,
        clear_summary: bool,
        bindings: Option<PlanBinding>,
        depends_on: Option<Vec<String>>,
        acceptance: Option<Vec<PlanAcceptanceCriterion>>,
        validation_refs: Option<Vec<ValidationRef>>,
        base_revision: Option<WorkspaceRevision>,
        priority: Option<u8>,
        clear_priority: bool,
        tags: Option<Vec<String>>,
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
        if let Some(kind) = kind {
            node.kind = kind;
        }
        if let Some(status) = status {
            node.status = status;
        }
        if let Some(assignee) = assignee {
            node.assignee = assignee;
        }
        if let Some(is_abstract) = is_abstract {
            node.is_abstract = is_abstract;
        }
        if let Some(title) = title {
            node.title = title;
        }
        if let Some(summary) = summary {
            node.summary = Some(summary);
        } else if clear_summary {
            node.summary = None;
        }
        if let Some(bindings) = bindings {
            node.bindings = normalize_plan_binding(bindings);
        }
        if let Some(acceptance) = acceptance {
            node.acceptance = normalize_plan_acceptance(acceptance);
        }
        if let Some(validation_refs) = validation_refs {
            node.validation_refs = normalize_validation_refs(validation_refs);
        }
        if let Some(base_revision) = base_revision {
            node.base_revision = base_revision;
        }
        if let Some(priority) = priority {
            node.priority = Some(priority);
        } else if clear_priority {
            node.priority = None;
        }
        if let Some(tags) = tags {
            node.tags = normalize_string_refs(tags);
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
        if edge_kind_affects_root_nodes(kind) {
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
        if edge_kind_affects_root_nodes(kind) {
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
            if graph.kind == PlanKind::TaskExecution {
                if let Some(index) = graph.nodes.iter().position(|node| node.id == node_id) {
                    graph.nodes[index] = plan_node_from_coordination_task(task);
                } else {
                    graph.nodes.push(plan_node_from_coordination_task(task));
                    graph
                        .nodes
                        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
                }
            } else {
                let node = graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == node_id)
                    .ok_or_else(|| anyhow!("unknown plan node `{}`", node_id.0))?;
                populate_plan_node_from_coordination_task(node, task);
            }
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
        let git_execution = (task.git_execution != prism_coordination::TaskGitExecution::default())
            .then(|| prism_ir::GitExecutionOverlay {
                status: task.git_execution.status,
                pending_task_status: task.git_execution.pending_task_status,
                source_ref: task.git_execution.source_ref.clone(),
                target_ref: task.git_execution.target_ref.clone(),
                publish_ref: task.git_execution.publish_ref.clone(),
                target_branch: task.git_execution.target_branch.clone(),
            });
        if task.pending_handoff_to.is_some()
            || task.session.is_some()
            || task.worktree_id.is_some()
            || task.branch_ref.is_some()
            || git_execution.is_some()
        {
            overlays.push(PlanExecutionOverlay {
                node_id,
                pending_handoff_to: task.pending_handoff_to.clone(),
                session: task.session.clone(),
                worktree_id: task.worktree_id.clone(),
                branch_ref: task.branch_ref.clone(),
                effective_assignee: None,
                awaiting_handoff_from: None,
                git_execution,
            });
            *overlays = sort_execution_overlays(std::mem::take(overlays));
        }
    }
}

fn sort_execution_overlays(mut overlays: Vec<PlanExecutionOverlay>) -> Vec<PlanExecutionOverlay> {
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

fn derive_execution_overlays(
    graph: &PlanGraph,
    overlays: &[PlanExecutionOverlay],
) -> Vec<PlanExecutionOverlay> {
    let mut derived = Vec::new();
    for node in &graph.nodes {
        let stored = overlay_for_node(overlays, &node.id);
        let pending_handoff_to = stored.and_then(|overlay| overlay.pending_handoff_to.clone());
        let session = stored.and_then(|overlay| overlay.session.clone());
        let worktree_id = stored.and_then(|overlay| overlay.worktree_id.clone());
        let branch_ref = stored.and_then(|overlay| overlay.branch_ref.clone());
        let git_execution = stored.and_then(|overlay| overlay.git_execution.clone());
        let effective_assignee = effective_assignee_for_node(graph, overlays, node);
        let awaiting_handoff_from = awaiting_handoff_from_node(graph, node);
        if pending_handoff_to.is_some()
            || session.is_some()
            || worktree_id.is_some()
            || branch_ref.is_some()
            || git_execution.is_some()
            || effective_assignee.is_some()
            || awaiting_handoff_from.is_some()
        {
            derived.push(PlanExecutionOverlay {
                node_id: node.id.clone(),
                pending_handoff_to,
                session,
                worktree_id,
                branch_ref,
                effective_assignee,
                awaiting_handoff_from,
                git_execution,
            });
        }
    }
    sort_execution_overlays(derived)
}

fn sort_and_dedupe_plan_node_blockers(blockers: &mut Vec<PlanNodeBlocker>) {
    blockers.sort_by(|left, right| {
        plan_node_blocker_kind_key(left.kind)
            .cmp(&plan_node_blocker_kind_key(right.kind))
            .then_with(|| left.summary.cmp(&right.summary))
            .then_with(|| {
                left.related_node_id
                    .as_ref()
                    .map(|id| id.0.as_str())
                    .cmp(&right.related_node_id.as_ref().map(|id| id.0.as_str()))
            })
    });
    blockers.dedup_by(|left, right| {
        left.kind == right.kind
            && left.summary == right.summary
            && left.related_node_id == right.related_node_id
            && left.related_artifact_id == right.related_artifact_id
            && left.validation_checks == right.validation_checks
    });
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
        anchors: dedupe_anchors(criterion.anchors),
        required_checks: Vec::<ValidationRef>::new(),
        evidence_policy: prism_ir::AcceptanceEvidencePolicy::Any,
    }
}

fn merge_acceptance_from_coordination(
    existing: Vec<PlanAcceptanceCriterion>,
    incoming: Vec<AcceptanceCriterion>,
) -> Vec<PlanAcceptanceCriterion> {
    if incoming.is_empty() {
        return existing;
    }
    let existing = existing
        .into_iter()
        .map(|criterion| (criterion.label.clone(), criterion))
        .collect::<BTreeMap<_, _>>();
    incoming
        .into_iter()
        .map(|criterion| {
            let mut mapped = plan_acceptance_from_coordination(criterion);
            if let Some(existing) = existing.get(&mapped.label) {
                mapped.required_checks = existing.required_checks.clone();
                mapped.evidence_policy = existing.evidence_policy;
            }
            mapped
        })
        .collect()
}

fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    let mut anchors = anchors;
    anchors.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    anchors.dedup();
    anchors
}

fn normalize_string_refs(ids: Vec<String>) -> Vec<String> {
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    ids
}

fn dedupe_string_ids(ids: Vec<String>) -> Vec<String> {
    normalize_string_refs(ids)
}

fn dedupe_validation_refs(mut refs: Vec<ValidationRef>) -> Vec<ValidationRef> {
    refs.sort_by(|left, right| left.id.cmp(&right.id));
    refs.dedup_by(|left, right| left.id == right.id);
    refs
}

fn normalize_plan_binding(mut binding: PlanBinding) -> PlanBinding {
    binding.anchors = dedupe_anchors(binding.anchors);
    binding.concept_handles = normalize_string_refs(binding.concept_handles);
    binding.artifact_refs = normalize_string_refs(binding.artifact_refs);
    binding.memory_refs = normalize_string_refs(binding.memory_refs);
    binding.outcome_refs = normalize_string_refs(binding.outcome_refs);
    binding
}

fn plan_node_blocker_kind_key(kind: PlanNodeBlockerKind) -> u8 {
    match kind {
        PlanNodeBlockerKind::Dependency => 0,
        PlanNodeBlockerKind::BlockingNode => 1,
        PlanNodeBlockerKind::ChildIncomplete => 2,
        PlanNodeBlockerKind::ValidationGate => 3,
        PlanNodeBlockerKind::Handoff => 4,
        PlanNodeBlockerKind::ClaimConflict => 5,
        PlanNodeBlockerKind::ReviewRequired => 6,
        PlanNodeBlockerKind::RiskReviewRequired => 7,
        PlanNodeBlockerKind::ValidationRequired => 8,
        PlanNodeBlockerKind::StaleRevision => 9,
        PlanNodeBlockerKind::ArtifactStale => 10,
    }
}

fn is_completed_status(status: PlanNodeStatus) -> bool {
    matches!(status, PlanNodeStatus::Completed)
}

fn dependency_blocker_cause(code: &str) -> BlockerCause {
    BlockerCause {
        source: BlockerCauseSource::DependencyGraph,
        code: Some(code.to_owned()),
        acceptance_label: None,
        threshold_metric: None,
        threshold_value: None,
        observed_value: None,
    }
}

fn runtime_blocker_cause(code: &str) -> BlockerCause {
    BlockerCause {
        source: BlockerCauseSource::RuntimeState,
        code: Some(code.to_owned()),
        acceptance_label: None,
        threshold_metric: None,
        threshold_value: None,
        observed_value: None,
    }
}

fn overlay_for_node<'a>(
    overlays: &'a [PlanExecutionOverlay],
    node_id: &PlanNodeId,
) -> Option<&'a PlanExecutionOverlay> {
    overlays.iter().find(|overlay| overlay.node_id == *node_id)
}

fn graph_node_by_id<'a>(graph: &'a PlanGraph, node_id: &PlanNodeId) -> Option<&'a PlanNode> {
    graph.nodes.iter().find(|node| node.id == *node_id)
}

fn readiness_blockers_for_node(
    graph: &PlanGraph,
    overlays: &[PlanExecutionOverlay],
    node: &PlanNode,
) -> Vec<PlanNodeBlocker> {
    let mut blockers = Vec::new();
    if let Some(overlay) = overlay_for_node(overlays, &node.id) {
        if let Some(target) = overlay.pending_handoff_to.as_ref() {
            blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::Handoff,
                summary: format!(
                    "pending handoff to `{}` must be resolved before execution can continue",
                    target.0
                ),
                related_node_id: None,
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![runtime_blocker_cause("pending_handoff")],
            });
        }
    }
    for edge in graph.edges.iter().filter(|edge| edge.from == node.id) {
        let Some(target) = graph_node_by_id(graph, &edge.to) else {
            continue;
        };
        if is_completed_status(target.status) {
            continue;
        }
        match edge.kind {
            PlanEdgeKind::DependsOn => blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::Dependency,
                summary: format!(
                    "depends on `{}` completing before this node can proceed",
                    target.title
                ),
                related_node_id: Some(target.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![dependency_blocker_cause("depends_on_edge")],
            }),
            PlanEdgeKind::Blocks => blockers.push(PlanNodeBlocker {
                kind: PlanNodeBlockerKind::BlockingNode,
                summary: format!("authored blocking node `{}` is not completed", target.title),
                related_node_id: Some(target.id.clone()),
                related_artifact_id: None,
                risk_score: None,
                validation_checks: Vec::new(),
                causes: vec![dependency_blocker_cause("authored_blocking_edge")],
            }),
            _ => {}
        }
    }
    for edge in graph.edges.iter().filter(|edge| edge.to == node.id) {
        if edge.kind != PlanEdgeKind::HandoffTo {
            continue;
        }
        let Some(source) = graph_node_by_id(graph, &edge.from) else {
            continue;
        };
        if is_completed_status(source.status) {
            continue;
        }
        blockers.push(PlanNodeBlocker {
            kind: PlanNodeBlockerKind::Handoff,
            summary: format!(
                "awaiting handoff from `{}` before this node should proceed",
                source.title
            ),
            related_node_id: Some(source.id.clone()),
            related_artifact_id: None,
            risk_score: None,
            validation_checks: Vec::new(),
            causes: vec![dependency_blocker_cause("handoff_edge")],
        });
    }
    sort_and_dedupe_plan_node_blockers(&mut blockers);
    blockers
}

fn completion_blockers_for_node(graph: &PlanGraph, node: &PlanNode) -> Vec<PlanNodeBlocker> {
    let mut blockers = Vec::new();
    for edge in graph.edges.iter().filter(|edge| edge.to == node.id) {
        if edge.kind != PlanEdgeKind::ChildOf {
            continue;
        }
        let Some(child) = graph_node_by_id(graph, &edge.from) else {
            continue;
        };
        if matches!(
            child.status,
            PlanNodeStatus::Completed | PlanNodeStatus::Abandoned
        ) {
            continue;
        }
        blockers.push(PlanNodeBlocker {
            kind: PlanNodeBlockerKind::ChildIncomplete,
            summary: format!(
                "child node `{}` must reach a terminal state before this parent can complete",
                child.title
            ),
            related_node_id: Some(child.id.clone()),
            related_artifact_id: None,
            risk_score: None,
            validation_checks: Vec::new(),
            causes: vec![dependency_blocker_cause("child_incomplete")],
        });
    }
    for edge in graph.edges.iter().filter(|edge| edge.from == node.id) {
        if edge.kind != PlanEdgeKind::Validates {
            continue;
        }
        let Some(target) = graph_node_by_id(graph, &edge.to) else {
            continue;
        };
        if is_completed_status(target.status) {
            continue;
        }
        blockers.push(PlanNodeBlocker {
            kind: PlanNodeBlockerKind::ValidationGate,
            summary: if declared_validation_checks(target).is_empty() {
                format!("validation gate `{}` is not completed", target.title)
            } else {
                format!(
                    "validation gate `{}` is not completed for checks: {}",
                    target.title,
                    declared_validation_checks(target).join(", ")
                )
            },
            related_node_id: Some(target.id.clone()),
            related_artifact_id: None,
            risk_score: None,
            validation_checks: declared_validation_checks(target),
            causes: vec![dependency_blocker_cause("validation_gate_incomplete")],
        });
    }
    sort_and_dedupe_plan_node_blockers(&mut blockers);
    blockers
}

pub(crate) fn node_blockers_for_graph(
    graph: &PlanGraph,
    overlays: &[PlanExecutionOverlay],
    node_id: &PlanNodeId,
) -> Vec<PlanNodeBlocker> {
    let Some(node) = graph.nodes.iter().find(|node| node.id == *node_id) else {
        return Vec::new();
    };
    let mut blockers = readiness_blockers_for_node(graph, overlays, node);
    blockers.extend(completion_blockers_for_node(graph, node));
    sort_and_dedupe_plan_node_blockers(&mut blockers);
    blockers
}

fn normalize_plan_acceptance(
    acceptance: Vec<PlanAcceptanceCriterion>,
) -> Vec<PlanAcceptanceCriterion> {
    acceptance
        .into_iter()
        .map(|mut criterion| {
            criterion.anchors = dedupe_anchors(criterion.anchors);
            criterion.required_checks = dedupe_validation_refs(criterion.required_checks);
            criterion
        })
        .collect()
}

fn normalize_validation_refs(validation_refs: Vec<ValidationRef>) -> Vec<ValidationRef> {
    let mut normalized = validation_refs;
    normalized.sort_by(|left, right| left.id.cmp(&right.id));
    normalized.dedup_by(|left, right| left.id == right.id);
    normalized
}

pub(crate) fn declared_validation_checks(node: &PlanNode) -> Vec<String> {
    let mut checks = node
        .validation_refs
        .iter()
        .map(|check| check.id.clone())
        .collect::<Vec<_>>();
    checks.extend(node.acceptance.iter().flat_map(|criterion| {
        criterion
            .required_checks
            .iter()
            .map(|check| check.id.clone())
            .collect::<Vec<_>>()
    }));
    dedupe_string_ids(checks)
}

pub(crate) fn required_validation_checks_for_node(
    graph: &PlanGraph,
    node: &PlanNode,
) -> Vec<String> {
    let mut checks = Vec::new();
    for edge in graph.edges.iter().filter(|edge| edge.from == node.id) {
        if edge.kind != PlanEdgeKind::Validates {
            continue;
        }
        let Some(target) = graph_node_by_id(graph, &edge.to) else {
            continue;
        };
        checks.extend(declared_validation_checks(target));
    }
    dedupe_string_ids(checks)
}

fn plan_node_from_coordination_task(task: &CoordinationTask) -> PlanNode {
    let mut node = PlanNode {
        id: plan_node_id_from_task_id(task.id.clone()),
        plan_id: task.plan.clone(),
        kind: task.kind,
        title: String::new(),
        summary: task.summary.clone(),
        status: map_coordination_task_status(effective_coordination_task_status(task)),
        bindings: task_bindings(task),
        acceptance: task
            .acceptance
            .iter()
            .cloned()
            .map(plan_acceptance_from_coordination)
            .collect(),
        validation_refs: dedupe_validation_refs(task.validation_refs.clone()),
        is_abstract: task.is_abstract,
        assignee: task.assignee.clone(),
        base_revision: task.base_revision.clone(),
        priority: task.priority,
        tags: normalize_string_refs(task.tags.clone()),
        metadata: task.metadata.clone(),
    };
    populate_plan_node_from_coordination_task(&mut node, task);
    node
}

fn populate_plan_node_from_coordination_task(node: &mut PlanNode, task: &CoordinationTask) {
    if task.kind != PlanNodeKind::Edit || node.kind == PlanNodeKind::Edit {
        node.kind = task.kind;
    }
    node.title = task.title.clone();
    if task.summary.is_some() {
        node.summary = task.summary.clone();
    }
    node.status = map_coordination_task_status(effective_coordination_task_status(task));
    let mut bindings = if task_has_authored_binding_metadata(task) {
        normalize_plan_binding(task.bindings.clone())
    } else {
        normalize_plan_binding(node.bindings.clone())
    };
    bindings.anchors = dedupe_anchors(task.anchors.clone());
    node.bindings = bindings;
    node.acceptance =
        merge_acceptance_from_coordination(node.acceptance.clone(), task.acceptance.clone());
    if !task.validation_refs.is_empty() || node.validation_refs.is_empty() {
        node.validation_refs = dedupe_validation_refs(task.validation_refs.clone());
    }
    if task.is_abstract {
        node.is_abstract = true;
    }
    node.assignee = task.assignee.clone();
    node.base_revision = task.base_revision.clone();
    if task.priority.is_some() {
        node.priority = task.priority;
    }
    if !task.tags.is_empty() {
        node.tags = normalize_string_refs(task.tags.clone());
    }
    if !task.metadata.is_null() {
        node.metadata = task.metadata.clone();
    }
}

fn authored_plan_title(plan: &Plan) -> String {
    plan.title.clone()
}

fn task_bindings(task: &CoordinationTask) -> PlanBinding {
    let mut bindings = normalize_plan_binding(task.bindings.clone());
    if bindings.anchors.is_empty() {
        bindings.anchors = dedupe_anchors(task.anchors.clone());
    }
    bindings
}

fn task_has_authored_binding_metadata(task: &CoordinationTask) -> bool {
    !task.bindings.concept_handles.is_empty()
        || !task.bindings.artifact_refs.is_empty()
        || !task.bindings.memory_refs.is_empty()
        || !task.bindings.outcome_refs.is_empty()
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

fn is_task_backed_plan_node_id(id: &str) -> bool {
    id.starts_with("coord-task:")
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

fn effective_coordination_task_status(task: &CoordinationTask) -> prism_ir::CoordinationTaskStatus {
    task.published_task_status.unwrap_or(task.status)
}

fn ensure_node_in_graph(graph: &PlanGraph, node_id: &PlanNodeId) -> Result<()> {
    if graph.nodes.iter().any(|node| node.id == *node_id) {
        Ok(())
    } else {
        Err(anyhow!("unknown plan node `{}`", node_id.0))
    }
}

fn recompute_root_nodes(graph: &mut PlanGraph) {
    let hidden_from_roots = graph
        .edges
        .iter()
        .filter(|edge| edge_kind_affects_root_nodes(edge.kind))
        .map(|edge| edge.from.0.to_string())
        .collect::<BTreeSet<_>>();
    graph.root_nodes = graph
        .nodes
        .iter()
        .filter(|node| !hidden_from_roots.contains(node.id.0.as_str()))
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

fn edge_kind_affects_root_nodes(kind: PlanEdgeKind) -> bool {
    matches!(kind, PlanEdgeKind::DependsOn | PlanEdgeKind::ChildOf)
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
    let from_node = graph_node_by_id(graph, from_node_id)
        .ok_or_else(|| anyhow!("unknown plan node `{}`", from_node_id.0))?;
    let to_node = graph_node_by_id(graph, to_node_id)
        .ok_or_else(|| anyhow!("unknown plan node `{}`", to_node_id.0))?;
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
    match kind {
        PlanEdgeKind::Validates => {
            if to_node.kind != PlanNodeKind::Validate {
                return Err(anyhow!(
                    "plan edge `{}` -> `{}` (Validates) must target a Validate node",
                    from_node_id.0,
                    to_node_id.0
                ));
            }
            if declared_validation_checks(to_node).is_empty() {
                return Err(anyhow!(
                    "plan edge `{}` -> `{}` (Validates) must target a validation node with stable validation refs",
                    from_node_id.0,
                    to_node_id.0
                ));
            }
        }
        PlanEdgeKind::HandoffTo if from_node.is_abstract || to_node.is_abstract => {
            return Err(anyhow!(
                "plan edge `{}` -> `{}` (HandoffTo) must connect executable nodes, not abstract structure",
                from_node_id.0,
                to_node_id.0
            ));
        }
        _ => {}
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

fn effective_assignee_for_node(
    graph: &PlanGraph,
    overlays: &[PlanExecutionOverlay],
    node: &PlanNode,
) -> Option<AgentId> {
    if let Some(overlay) = overlay_for_node(overlays, &node.id) {
        if let Some(agent) = overlay.pending_handoff_to.clone() {
            return Some(agent);
        }
    }
    if let Some(agent) = node.assignee.clone() {
        return Some(agent);
    }
    let mut handoff_sources = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == PlanEdgeKind::HandoffTo && edge.to == node.id)
        .filter_map(|edge| graph_node_by_id(graph, &edge.from))
        .collect::<Vec<_>>();
    handoff_sources.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    for source in handoff_sources {
        if !is_completed_status(source.status) {
            continue;
        }
        if let Some(overlay) = overlay_for_node(overlays, &source.id) {
            if let Some(agent) = overlay.pending_handoff_to.clone() {
                return Some(agent);
            }
        }
        if let Some(agent) = source.assignee.clone() {
            return Some(agent);
        }
    }
    None
}

fn awaiting_handoff_from_node(graph: &PlanGraph, node: &PlanNode) -> Option<PlanNodeId> {
    let mut sources = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == PlanEdgeKind::HandoffTo && edge.to == node.id)
        .filter_map(|edge| {
            graph_node_by_id(graph, &edge.from)
                .filter(|source| !is_completed_status(source.status))
                .map(|_| edge.from.clone())
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    sources.into_iter().next()
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
