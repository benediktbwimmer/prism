use std::collections::{BTreeMap, BTreeSet};

use prism_coordination::{
    AcceptanceCriterion, CanonicalPlanRecord, CanonicalTaskRecord, CoordinationPolicy,
    CoordinationSnapshot, CoordinationSnapshotV2, PlanScheduling,
};
use prism_ir::{
    AgentId, AnchorRef, BlockerCause, BlockerCauseSource, GitExecutionStatus,
    DerivedPlanStatus, GitIntegrationStatus, PlanAcceptanceCriterion, PlanBinding, PlanEdge,
    PlanEdgeId, PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanId, PlanNode,
    PlanNodeBlocker, PlanNodeBlockerKind, PlanNodeId, PlanNodeStatus, ValidationRef,
};
use serde_json::Value;

use crate::types::PlanProjection;

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePlanRuntimeState {
    snapshot_v2: CoordinationSnapshotV2,
    plan_revisions: BTreeMap<String, u64>,
}

impl NativePlanRuntimeState {
    pub(crate) fn from_coordination_snapshot(snapshot: &CoordinationSnapshot) -> Self {
        let plan_revisions = snapshot
            .plans
            .iter()
            .map(|plan| (plan.id.0.to_string(), plan.revision))
            .collect::<BTreeMap<_, _>>();
        Self::from_canonical_snapshot(snapshot.to_canonical_snapshot_v2(), plan_revisions)
    }

    pub(crate) fn from_canonical_snapshot(
        snapshot_v2: CoordinationSnapshotV2,
        plan_revisions: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            snapshot_v2,
            plan_revisions,
        }
    }

    pub(crate) fn plan_projection(&self, plan_id: &PlanId) -> Option<PlanProjection> {
        let derivations = self.snapshot_v2.derive_statuses().ok()?;
        let plan = self.plan_record(plan_id)?.clone();
        Some(self.project_plan(&plan, &derivations))
    }

    pub(crate) fn plan_projections(&self) -> Vec<PlanProjection> {
        let Some(derivations) = self.snapshot_v2.derive_statuses().ok() else {
            return Vec::new();
        };
        self.snapshot_v2
            .plans
            .iter()
            .map(|plan| self.project_plan(plan, &derivations))
            .collect()
    }

    pub(crate) fn policy(&self, plan_id: &PlanId) -> Option<CoordinationPolicy> {
        self.plan_record(plan_id).map(|plan| plan.policy.clone())
    }

    pub(crate) fn scheduling(&self, plan_id: &PlanId) -> Option<PlanScheduling> {
        self.plan_record(plan_id).map(|plan| plan.scheduling.clone())
    }

    fn plan_record(&self, plan_id: &PlanId) -> Option<&CanonicalPlanRecord> {
        self.snapshot_v2
            .plans
            .iter()
            .find(|plan| plan.id == *plan_id)
    }

    fn project_plan(
        &self,
        plan: &CanonicalPlanRecord,
        derivations: &prism_coordination::CoordinationDerivations,
    ) -> PlanProjection {
        let tasks = self
            .snapshot_v2
            .tasks
            .iter()
            .filter(|task| task.parent_plan_id == plan.id)
            .collect::<Vec<_>>();
        let task_ids = tasks
            .iter()
            .map(|task| task.id.0.as_str())
            .collect::<BTreeSet<_>>();
        let mut nodes = tasks
            .iter()
            .map(|task| plan_node_from_task_record(task))
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        let mut edges = self
            .snapshot_v2
            .dependencies
            .iter()
            .filter_map(|dependency| {
                if dependency.source.kind != prism_ir::NodeRefKind::Task
                    || dependency.target.kind != prism_ir::NodeRefKind::Task
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
        let mut graph = PlanGraph {
            id: plan.id.clone(),
            scope: plan.scope,
            kind: plan.kind,
            title: plan.title.clone(),
            goal: plan.goal.clone(),
            status: derived_plan_status_to_plan_status(
                derivations
                    .plan_state(&plan.id)
                    .map(|state| state.derived_status)
                    .unwrap_or(DerivedPlanStatus::Pending),
            ),
            revision: self
                .plan_revisions
                .get(plan.id.0.as_str())
                .copied()
                .unwrap_or_default(),
            root_nodes,
            tags: plan.tags.clone(),
            created_from: plan.created_from.clone(),
            metadata: plan.metadata.clone(),
            nodes,
            edges,
        };
        recompute_root_nodes(&mut graph);
        let base_overlays = stored_execution_overlays_from_tasks(&tasks);
        let execution_overlays = derive_execution_overlays(&graph, &base_overlays);
        PlanProjection {
            graph,
            execution_overlays,
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

fn plan_acceptance_from_coordination(criterion: AcceptanceCriterion) -> PlanAcceptanceCriterion {
    PlanAcceptanceCriterion {
        label: criterion.label,
        anchors: dedupe_anchors(criterion.anchors),
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
        match edge.kind {
            PlanEdgeKind::DependsOn => {
                let dependency_requirement = dependency_requirement_from_edge(edge);
                if dependency_requirement_satisfied(
                    target,
                    overlay_for_node(overlays, &target.id),
                    dependency_requirement,
                ) {
                    continue;
                }
                blockers.push(PlanNodeBlocker {
                    kind: PlanNodeBlockerKind::Dependency,
                    summary: dependency_requirement_blocker_summary(target, dependency_requirement),
                    related_node_id: Some(target.id.clone()),
                    related_artifact_id: None,
                    risk_score: None,
                    validation_checks: Vec::new(),
                    causes: vec![dependency_blocker_cause(
                        dependency_requirement.blocker_cause(),
                    )],
                });
            }
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
        if constrained_path_exists(graph, &target.id, &node.id) {
            continue;
        }
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

fn plan_node_from_task_record(task: &CanonicalTaskRecord) -> PlanNode {
    PlanNode {
        id: PlanNodeId::new(task.id.0.clone()),
        plan_id: task.parent_plan_id.clone(),
        kind: task.kind,
        title: task.title.clone(),
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
    }
}

fn task_bindings(task: &CanonicalTaskRecord) -> PlanBinding {
    let mut bindings = normalize_plan_binding(task.bindings.clone());
    if bindings.anchors.is_empty() {
        bindings.anchors = dedupe_anchors(task.anchors.clone());
    }
    bindings
}

fn stored_execution_overlays_from_tasks(tasks: &[&CanonicalTaskRecord]) -> Vec<PlanExecutionOverlay> {
    let mut overlays = tasks
        .iter()
        .filter_map(|task| {
            let git_execution = (task.git_execution != prism_coordination::TaskGitExecution::default())
                .then(|| prism_ir::GitExecutionOverlay {
                    status: task.git_execution.status,
                    pending_task_status: task.git_execution.pending_task_status,
                    source_ref: task.git_execution.source_ref.clone(),
                    target_ref: task.git_execution.target_ref.clone(),
                    publish_ref: task.git_execution.publish_ref.clone(),
                    target_branch: task.git_execution.target_branch.clone(),
                    source_commit: task.git_execution.source_commit.clone(),
                    publish_commit: task.git_execution.publish_commit.clone(),
                    target_commit_at_publish: task.git_execution.target_commit_at_publish.clone(),
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
                effective_assignee: None,
                awaiting_handoff_from: None,
                git_execution,
            })
        })
        .collect::<Vec<_>>();
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyRequirement {
    Completed,
    CoordinationPublished,
    IntegratedToTarget,
}

impl DependencyRequirement {
    fn blocker_cause(self) -> &'static str {
        match self {
            Self::Completed => "depends_on_edge",
            Self::CoordinationPublished => "coordination_published_dependency_edge",
            Self::IntegratedToTarget => "integrated_to_target_dependency_edge",
        }
    }
}

fn dependency_requirement_from_edge(edge: &PlanEdge) -> DependencyRequirement {
    match edge
        .metadata
        .get("dependencyLifecycle")
        .and_then(Value::as_str)
    {
        Some("coordination_published") => DependencyRequirement::CoordinationPublished,
        Some("integrated_to_target") => DependencyRequirement::IntegratedToTarget,
        _ => DependencyRequirement::Completed,
    }
}

fn dependency_requirement_satisfied(
    target: &PlanNode,
    overlay: Option<&PlanExecutionOverlay>,
    requirement: DependencyRequirement,
) -> bool {
    match requirement {
        DependencyRequirement::Completed => is_completed_status(target.status),
        DependencyRequirement::CoordinationPublished => overlay
            .and_then(|overlay| overlay.git_execution.as_ref())
            .map(|git_execution| git_execution.status == GitExecutionStatus::CoordinationPublished)
            .unwrap_or(false),
        DependencyRequirement::IntegratedToTarget => overlay
            .and_then(|overlay| overlay.git_execution.as_ref())
            .map(|git_execution| {
                git_execution.integration_status == GitIntegrationStatus::IntegratedToTarget
            })
            .unwrap_or(false),
    }
}

fn dependency_requirement_blocker_summary(
    target: &PlanNode,
    requirement: DependencyRequirement,
) -> String {
    match requirement {
        DependencyRequirement::Completed => format!(
            "depends on `{}` completing before this node can proceed",
            target.title
        ),
        DependencyRequirement::CoordinationPublished => format!(
            "depends on `{}` publishing coordination state before this node can proceed",
            target.title
        ),
        DependencyRequirement::IntegratedToTarget => format!(
            "depends on `{}` integrating to the target branch before this node can proceed",
            target.title
        ),
    }
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

fn effective_coordination_task_status(
    task: &CanonicalTaskRecord,
) -> prism_ir::CoordinationTaskStatus {
    if task.pending_handoff_to.is_some() {
        prism_ir::CoordinationTaskStatus::Blocked
    } else if let Some(status) = task.git_execution.pending_task_status {
        status
    } else {
        task.status
    }
}

fn derived_plan_status_to_plan_status(status: DerivedPlanStatus) -> prism_ir::PlanStatus {
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

fn dedupe_and_sort_edges(edges: &mut Vec<PlanEdge>) {
    edges.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    edges.dedup_by(|left, right| left.id == right.id);
}

fn derive_root_nodes(nodes: &[PlanNode], edges: &[PlanEdge]) -> Vec<PlanNodeId> {
    let hidden_from_roots = edges
        .iter()
        .filter(|edge| edge_kind_affects_root_nodes(edge.kind))
        .map(|edge| edge.from.0.as_str())
        .collect::<BTreeSet<_>>();
    nodes.iter()
        .filter(|node| !hidden_from_roots.contains(node.id.0.as_str()))
        .map(|node| node.id.clone())
        .collect()
}

fn edge_kind_affects_root_nodes(kind: PlanEdgeKind) -> bool {
    matches!(kind, PlanEdgeKind::DependsOn | PlanEdgeKind::ChildOf)
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
