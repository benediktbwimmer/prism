use std::collections::{BTreeMap, BTreeSet};

use prism_ir::{
    AcceptanceEvidencePolicy, CoordinationTaskStatus, DerivedPlanStatus, NodeRefKind,
    PlanAcceptanceCriterion, PlanBinding, PlanEdge, PlanEdgeId, PlanEdgeKind,
    PlanExecutionOverlay, PlanGraph, PlanNode, PlanNodeId, PlanNodeStatus, ValidationRef,
};
use serde_json::Value;

use crate::state::CoordinationStore;
use crate::types::{AcceptanceCriterion, CoordinationSnapshot, CoordinationTask, Plan};
use crate::{
    CanonicalPlanRecord, CanonicalTaskRecord, CoordinationDerivations, CoordinationSnapshotV2,
};

pub fn snapshot_plan_graphs(snapshot: &CoordinationSnapshot) -> Vec<PlanGraph> {
    canonical_snapshot_plan_projections(snapshot)
        .into_iter()
        .map(|(graph, _)| graph)
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
    dedupe_and_sort_edges(&mut edges);
    let root_nodes = derive_root_nodes(&nodes, &edges);

    PlanGraph {
        id: plan.id.clone(),
        scope: plan.scope,
        kind: plan.kind,
        title: authored_plan_title(&plan),
        goal: plan.goal,
        status: plan.status,
        revision: plan.revision,
        root_nodes,
        tags: plan.tags,
        created_from: plan.created_from,
        metadata: plan.metadata,
        nodes,
        edges,
    }
}

pub fn execution_overlays_from_tasks(tasks: &[CoordinationTask]) -> Vec<PlanExecutionOverlay> {
    let mut overlays =
        tasks
            .iter()
            .filter_map(|task| {
                let git_execution = (task.git_execution != crate::TaskGitExecution::default())
                    .then(|| prism_ir::GitExecutionOverlay {
                        status: task.git_execution.status,
                        pending_task_status: task.git_execution.pending_task_status,
                        source_ref: task.git_execution.source_ref.clone(),
                        target_ref: task.git_execution.target_ref.clone(),
                        publish_ref: task.git_execution.publish_ref.clone(),
                        target_branch: task.git_execution.target_branch.clone(),
                        source_commit: task.git_execution.source_commit.clone(),
                        publish_commit: task.git_execution.publish_commit.clone(),
                        target_commit_at_publish: task
                            .git_execution
                            .target_commit_at_publish
                            .clone(),
                        review_artifact_ref: task.git_execution.review_artifact_ref.clone(),
                        integration_commit: task.git_execution.integration_commit.clone(),
                        integration_evidence: task.git_execution.integration_evidence.clone(),
                        integration_mode: task.git_execution.integration_mode,
                        integration_status: task.git_execution.integration_status,
                    });
                if task.pending_handoff_to.is_none()
                    && task.session.is_none()
                    && task.worktree_id.is_none()
                    && task.branch_ref.is_none()
                    && git_execution.is_none()
                {
                    return None;
                }
                Some(PlanExecutionOverlay {
                    node_id: plan_node_id_from_task_id(task.id.clone()),
                    pending_handoff_to: task.pending_handoff_to.clone(),
                    session: task.session.clone(),
                    worktree_id: task.worktree_id.clone(),
                    branch_ref: task.branch_ref.clone(),
                    effective_assignee: None,
                    awaiting_handoff_from: None,
                    git_execution,
                })
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
        let mut dependencies = BTreeMap::<String, TaskDependencyBuckets>::new();
        for edge in &graph.edges {
            if edge.kind != PlanEdgeKind::DependsOn
                || !is_task_backed_plan_node_id(edge.from.0.as_str())
                || !is_task_backed_plan_node_id(edge.to.0.as_str())
            {
                continue;
            }
            let buckets = dependencies.entry(edge.from.0.to_string()).or_default();
            match dependency_lifecycle_from_edge(edge) {
                DependencyLifecycle::Completed => buckets
                    .depends_on
                    .push(coordination_task_id_from_plan_node_id(edge.to.clone())),
                DependencyLifecycle::CoordinationPublished => buckets
                    .coordination_depends_on
                    .push(coordination_task_id_from_plan_node_id(edge.to.clone())),
                DependencyLifecycle::IntegratedToTarget => buckets
                    .integrated_depends_on
                    .push(coordination_task_id_from_plan_node_id(edge.to.clone())),
            }
        }
        for node in &graph.nodes {
            if !is_task_backed_plan_node_id(node.id.0.as_str()) {
                continue;
            }
            let buckets = dependencies.remove(node.id.0.as_str()).unwrap_or_default();
            snapshot.tasks.push(task_from_plan_node(
                graph.id.clone(),
                node.clone(),
                buckets.depends_on,
                buckets.coordination_depends_on,
                buckets.integrated_depends_on,
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
        canonical_snapshot_plan_projections(&state.snapshot())
            .into_iter()
            .find_map(|(graph, _)| (graph.id == *plan_id).then_some(graph))
    }

    pub fn plan_execution_overlays(&self, plan_id: &prism_ir::PlanId) -> Vec<PlanExecutionOverlay> {
        let state = self.state.read().expect("coordination store lock poisoned");
        canonical_snapshot_plan_projections(&state.snapshot())
            .into_iter()
            .find_map(|(graph, overlays)| (graph.id == *plan_id).then_some(overlays))
            .unwrap_or_default()
    }
}

fn canonical_snapshot_plan_projections(
    snapshot: &CoordinationSnapshot,
) -> Vec<(PlanGraph, Vec<PlanExecutionOverlay>)> {
    let snapshot_v2 = snapshot.to_canonical_snapshot_v2();
    let Ok(derivations) = snapshot_v2.derive_statuses() else {
        return Vec::new();
    };
    let revisions = snapshot
        .plans
        .iter()
        .map(|plan| (plan.id.0.to_string(), plan.revision))
        .collect::<BTreeMap<_, _>>();
    snapshot_v2
        .plans
        .iter()
        .map(|plan| canonical_plan_projection(plan, &snapshot_v2, &derivations, &revisions))
        .collect()
}

fn canonical_plan_projection(
    plan: &CanonicalPlanRecord,
    snapshot_v2: &CoordinationSnapshotV2,
    derivations: &CoordinationDerivations,
    revisions: &BTreeMap<String, u64>,
) -> (PlanGraph, Vec<PlanExecutionOverlay>) {
    let tasks = snapshot_v2
        .tasks
        .iter()
        .filter(|task| task.parent_plan_id == plan.id)
        .collect::<Vec<_>>();
    let task_ids = tasks
        .iter()
        .map(|task| task.id.0.as_str())
        .collect::<BTreeSet<_>>();
    let nodes = tasks
        .iter()
        .map(|task| canonical_plan_node_from_task(task))
        .collect::<Vec<_>>();
    let mut edges = snapshot_v2
        .dependencies
        .iter()
        .filter_map(|dependency| {
            if dependency.source.kind != NodeRefKind::Task
                || dependency.target.kind != NodeRefKind::Task
                || !task_ids.contains(dependency.source.id.as_str())
                || !task_ids.contains(dependency.target.id.as_str())
            {
                return None;
            }
            Some(PlanEdge {
                id: PlanEdgeId::new(format!(
                    "plan-edge:{}:depends-on:{}",
                    dependency.source.id, dependency.target.id
                )),
                plan_id: plan.id.clone(),
                from: PlanNodeId::new(dependency.source.id.clone()),
                to: PlanNodeId::new(dependency.target.id.clone()),
                kind: PlanEdgeKind::DependsOn,
                summary: None,
                metadata: Value::Null,
            })
        })
        .collect::<Vec<_>>();
    dedupe_and_sort_edges(&mut edges);
    let root_nodes = derive_root_nodes(&nodes, &edges);
    let graph = PlanGraph {
        id: plan.id.clone(),
        scope: plan.scope,
        kind: plan.kind,
        title: plan.title.clone(),
        goal: plan.goal.clone(),
        status: canonical_plan_status(
            derivations
                .plan_state(&plan.id)
                .map(|state| state.derived_status)
                .unwrap_or(DerivedPlanStatus::Pending),
        ),
        revision: revisions.get(plan.id.0.as_str()).copied().unwrap_or_default(),
        root_nodes,
        tags: plan.tags.clone(),
        created_from: plan.created_from.clone(),
        metadata: plan.metadata.clone(),
        nodes,
        edges,
    };
    let overlays = canonical_execution_overlays(&tasks);
    (graph, overlays)
}

fn canonical_plan_node_from_task(task: &CanonicalTaskRecord) -> PlanNode {
    PlanNode {
        id: PlanNodeId::new(task.id.0.clone()),
        plan_id: task.parent_plan_id.clone(),
        kind: task.kind,
        title: task.title.clone(),
        summary: task.summary.clone(),
        status: canonical_task_node_status(task),
        bindings: canonical_task_bindings(task),
        acceptance: task
            .acceptance
            .iter()
            .cloned()
            .map(map_acceptance)
            .collect::<Vec<_>>(),
        validation_refs: task.validation_refs.clone(),
        is_abstract: task.is_abstract,
        assignee: task.assignee.clone(),
        base_revision: task.base_revision.clone(),
        priority: task.priority,
        tags: task.tags.clone(),
        metadata: task.metadata.clone(),
    }
}

fn canonical_task_bindings(task: &CanonicalTaskRecord) -> PlanBinding {
    let mut bindings = task.bindings.clone();
    if bindings.anchors.is_empty() {
        bindings.anchors = task.anchors.clone();
    }
    bindings
}

fn canonical_task_node_status(task: &CanonicalTaskRecord) -> PlanNodeStatus {
    match task
        .git_execution
        .pending_task_status
        .unwrap_or_else(|| if task.pending_handoff_to.is_some() { CoordinationTaskStatus::Blocked } else { task.status })
    {
        CoordinationTaskStatus::Proposed => PlanNodeStatus::Proposed,
        CoordinationTaskStatus::Ready => PlanNodeStatus::Ready,
        CoordinationTaskStatus::InProgress => PlanNodeStatus::InProgress,
        CoordinationTaskStatus::Blocked => PlanNodeStatus::Blocked,
        CoordinationTaskStatus::InReview => PlanNodeStatus::InReview,
        CoordinationTaskStatus::Validating => PlanNodeStatus::Validating,
        CoordinationTaskStatus::Completed => PlanNodeStatus::Completed,
        CoordinationTaskStatus::Abandoned => PlanNodeStatus::Abandoned,
    }
}

fn canonical_execution_overlays(tasks: &[&CanonicalTaskRecord]) -> Vec<PlanExecutionOverlay> {
    let mut overlays = tasks
        .iter()
        .filter_map(|task| {
            let git_execution = (task.git_execution != crate::TaskGitExecution::default())
                .then(|| prism_ir::GitExecutionOverlay {
                    status: task.git_execution.status,
                    pending_task_status: task.git_execution.pending_task_status,
                    source_ref: task.git_execution.source_ref.clone(),
                    target_ref: task.git_execution.target_ref.clone(),
                    publish_ref: task.git_execution.publish_ref.clone(),
                    target_branch: task.git_execution.target_branch.clone(),
                    source_commit: task.git_execution.source_commit.clone(),
                    publish_commit: task.git_execution.publish_commit.clone(),
                    target_commit_at_publish: task
                        .git_execution
                        .target_commit_at_publish
                        .clone(),
                    review_artifact_ref: task.git_execution.review_artifact_ref.clone(),
                    integration_commit: task.git_execution.integration_commit.clone(),
                    integration_evidence: task.git_execution.integration_evidence.clone(),
                    integration_mode: task.git_execution.integration_mode,
                    integration_status: task.git_execution.integration_status,
                });
            if task.pending_handoff_to.is_none()
                && task.session.is_none()
                && task.worktree_id.is_none()
                && task.branch_ref.is_none()
                && git_execution.is_none()
            {
                return None;
            }
            Some(PlanExecutionOverlay {
                node_id: PlanNodeId::new(task.id.0.clone()),
                pending_handoff_to: task.pending_handoff_to.clone(),
                session: task.session.clone(),
                worktree_id: task.worktree_id.clone(),
                branch_ref: task.branch_ref.clone(),
                effective_assignee: task.pending_handoff_to.clone().or_else(|| task.assignee.clone()),
                awaiting_handoff_from: None,
                git_execution,
            })
        })
        .collect::<Vec<_>>();
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

fn canonical_plan_status(status: DerivedPlanStatus) -> prism_ir::PlanStatus {
    match status {
        DerivedPlanStatus::Pending | DerivedPlanStatus::Active => prism_ir::PlanStatus::Active,
        DerivedPlanStatus::Blocked
        | DerivedPlanStatus::BrokenDependency
        | DerivedPlanStatus::Failed => prism_ir::PlanStatus::Blocked,
        DerivedPlanStatus::Completed => prism_ir::PlanStatus::Completed,
        DerivedPlanStatus::Abandoned => prism_ir::PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => prism_ir::PlanStatus::Archived,
    }
}

fn plan_node_from_task(task: CoordinationTask) -> PlanNode {
    let bindings = task_bindings(&task);
    let effective_status = effective_task_status(&task);
    PlanNode {
        id: plan_node_id_from_task_id(task.id),
        plan_id: task.plan,
        kind: task.kind,
        title: task.title,
        summary: task.summary,
        status: map_task_status(effective_status),
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
    }
}

fn derive_root_nodes(nodes: &[PlanNode], edges: &[PlanEdge]) -> Vec<PlanNodeId> {
    let hidden_from_roots = edges
        .iter()
        .filter(|edge| matches!(edge.kind, PlanEdgeKind::DependsOn | PlanEdgeKind::ChildOf))
        .map(|edge| edge.from.0.as_str())
        .collect::<BTreeSet<_>>();
    nodes.iter()
        .filter(|node| !hidden_from_roots.contains(node.id.0.as_str()))
        .map(|node| node.id.clone())
        .collect()
}

fn task_from_plan_node(
    plan_id: prism_ir::PlanId,
    node: PlanNode,
    depends_on: Vec<prism_ir::CoordinationTaskId>,
    coordination_depends_on: Vec<prism_ir::CoordinationTaskId>,
    integrated_depends_on: Vec<prism_ir::CoordinationTaskId>,
    execution: Option<PlanExecutionOverlay>,
) -> CoordinationTask {
    let anchors = node.bindings.anchors.clone();
    let pending_handoff_to = execution
        .as_ref()
        .and_then(|overlay| overlay.pending_handoff_to.clone());
    let session = execution
        .as_ref()
        .and_then(|overlay| overlay.session.clone());
    let worktree_id = execution
        .as_ref()
        .and_then(|overlay| overlay.worktree_id.clone());
    let branch_ref = execution
        .as_ref()
        .and_then(|overlay| overlay.branch_ref.clone());
    let git_execution = execution
        .as_ref()
        .and_then(|overlay| overlay.git_execution.clone())
        .map(|overlay| crate::TaskGitExecution {
            status: overlay.status,
            pending_task_status: overlay.pending_task_status,
            source_ref: overlay.source_ref,
            target_ref: overlay.target_ref,
            publish_ref: overlay.publish_ref,
            target_branch: overlay.target_branch,
            source_commit: overlay.source_commit,
            publish_commit: overlay.publish_commit,
            target_commit_at_publish: overlay.target_commit_at_publish,
            review_artifact_ref: overlay.review_artifact_ref,
            integration_commit: overlay.integration_commit,
            integration_evidence: overlay.integration_evidence,
            integration_mode: overlay.integration_mode,
            integration_status: overlay.integration_status,
            last_preflight: None,
            last_publish: None,
        })
        .unwrap_or_default();
    CoordinationTask {
        id: coordination_task_id_from_plan_node_id(node.id),
        plan: plan_id,
        kind: node.kind,
        title: node.title,
        summary: node.summary,
        status: map_plan_node_status(node.status),
        assignee: node.assignee,
        pending_handoff_to,
        session,
        lease_holder: None,
        lease_started_at: None,
        lease_refreshed_at: None,
        lease_stale_at: None,
        lease_expires_at: None,
        worktree_id,
        branch_ref,
        anchors,
        bindings: node.bindings,
        depends_on,
        coordination_depends_on,
        integrated_depends_on,
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
        git_execution,
    }
}

fn authored_plan_title(plan: &Plan) -> String {
    plan.title.clone()
}

#[derive(Default)]
struct TaskDependencyBuckets {
    depends_on: Vec<prism_ir::CoordinationTaskId>,
    coordination_depends_on: Vec<prism_ir::CoordinationTaskId>,
    integrated_depends_on: Vec<prism_ir::CoordinationTaskId>,
}

#[derive(Clone, Copy)]
enum DependencyLifecycle {
    Completed,
    CoordinationPublished,
    IntegratedToTarget,
}

impl DependencyLifecycle {
    fn metadata_value(self) -> Option<&'static str> {
        match self {
            Self::Completed => None,
            Self::CoordinationPublished => Some("coordination_published"),
            Self::IntegratedToTarget => Some("integrated_to_target"),
        }
    }

    fn summary(self) -> Option<String> {
        match self {
            Self::Completed => None,
            Self::CoordinationPublished => Some("Requires coordination publication".to_string()),
            Self::IntegratedToTarget => Some("Requires target integration".to_string()),
        }
    }
}

fn dependency_lifecycle_from_edge(edge: &PlanEdge) -> DependencyLifecycle {
    match edge
        .metadata
        .get("dependencyLifecycle")
        .and_then(Value::as_str)
    {
        Some("coordination_published") => DependencyLifecycle::CoordinationPublished,
        Some("integrated_to_target") => DependencyLifecycle::IntegratedToTarget,
        _ => DependencyLifecycle::Completed,
    }
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
    for (dependencies, lifecycle) in [
        (&task.depends_on, DependencyLifecycle::Completed),
        (
            &task.coordination_depends_on,
            DependencyLifecycle::CoordinationPublished,
        ),
        (
            &task.integrated_depends_on,
            DependencyLifecycle::IntegratedToTarget,
        ),
    ] {
        for dependency in dependencies {
            if !seen.insert(format!(
                "{}:{}",
                dependency.0,
                lifecycle.metadata_value().unwrap_or("completed")
            )) {
                continue;
            }
            let metadata = lifecycle
                .metadata_value()
                .map(|value| serde_json::json!({ "dependencyLifecycle": value }))
                .unwrap_or(Value::Null);
            edges.push(PlanEdge {
                id: PlanEdgeId::new(format!(
                    "plan-edge:{}:depends-on:{}",
                    task.id.0, dependency.0
                )),
                plan_id: task.plan.clone(),
                from: plan_node_id_from_task_id(task.id.clone()),
                to: plan_node_id_from_task_id(dependency.clone()),
                kind: PlanEdgeKind::DependsOn,
                summary: lifecycle.summary(),
                metadata,
            });
        }
    }
    edges
}

fn dedupe_and_sort_edges(edges: &mut Vec<PlanEdge>) {
    edges.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    edges.dedup_by(|left, right| left.id == right.id);
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

fn effective_task_status(task: &CoordinationTask) -> prism_ir::CoordinationTaskStatus {
    task.git_execution.pending_task_status.unwrap_or(task.status)
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

fn is_task_backed_plan_node_id(id: &str) -> bool {
    id.starts_with("coord-task:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoordinationPolicy, PlanScheduling, TaskGitExecution};
    use prism_ir::{
        CoordinationTaskStatus, PlanKind, PlanNodeKind, PlanScope, PlanStatus,
        WorkspaceRevision,
    };
    use serde_json::Value;

    #[test]
    fn plan_graph_derives_root_nodes_without_root_tasks() {
        let plan_id = prism_ir::PlanId::new("plan:compat");
        let task_a = prism_ir::CoordinationTaskId::new("coord-task:a");
        let task_b = prism_ir::CoordinationTaskId::new("coord-task:b");
        let plan = Plan {
            id: plan_id.clone(),
            goal: "goal".into(),
            title: "Compat".into(),
            status: PlanStatus::Active,
            policy: CoordinationPolicy::default(),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            revision: 1,
            scheduling: PlanScheduling::default(),
            tags: Vec::new(),
            created_from: None,
            metadata: Value::Null,
        };
        let tasks = vec![
            CoordinationTask {
                id: task_a.clone(),
                plan: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "A".into(),
                summary: None,
                status: CoordinationTaskStatus::Ready,
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
                depends_on: vec![task_b.clone()],
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: Value::Null,
                git_execution: TaskGitExecution::default(),
            },
            CoordinationTask {
                id: task_b.clone(),
                plan: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "B".into(),
                summary: None,
                status: CoordinationTaskStatus::Ready,
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
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: Value::Null,
                git_execution: TaskGitExecution::default(),
            },
        ];

        let graph = plan_graph_from_coordination(plan, tasks);

        assert_eq!(graph.root_nodes, vec![PlanNodeId::new(task_b.0)]);
    }
}
