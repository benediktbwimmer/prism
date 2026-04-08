use prism_coordination::BlockerKind;
use prism_ir::{
    CoordinationTaskId, DerivedPlanStatus, EffectiveTaskStatus, NodeRefKind, PlanId, PlanStatus,
    TaskId,
};

use crate::common::current_timestamp;
use crate::{PlanSummary, Prism};

impl Prism {
    pub fn plan_summary(&self, plan_id: &PlanId) -> Option<PlanSummary> {
        let snapshot = self.coordination_snapshot_v2();
        let derivations = snapshot.derive_statuses().ok()?;
        let graph = snapshot.graph().ok()?;
        let derived_plan = derivations.plan_state(plan_id)?;
        let task_records = snapshot
            .tasks
            .iter()
            .map(|task| (task.id.0.to_string(), task))
            .collect::<std::collections::BTreeMap<_, _>>();
        let descendant_plan_count = canonical_descendant_plan_ids(&graph, plan_id).len();
        let descendant_task_ids = canonical_descendant_task_ids(&graph, plan_id);
        let now = current_timestamp();

        let mut summary = PlanSummary {
            plan_id: plan_id.clone(),
            status: compatibility_plan_status(derived_plan.derived_status),
            total_nodes: descendant_plan_count + descendant_task_ids.len(),
            completed_nodes: 0,
            abandoned_nodes: 0,
            in_progress_nodes: 0,
            actionable_nodes: 0,
            execution_blocked_nodes: 0,
            completion_gated_nodes: 0,
            review_gated_nodes: 0,
            validation_gated_nodes: 0,
            stale_nodes: 0,
            claim_conflicted_nodes: 0,
        };

        for task_id in descendant_task_ids {
            let task = task_records.get(task_id.0.as_str())?;
            let task_state = derivations.task_state(&task.id)?;
            match task_state.effective_status {
                EffectiveTaskStatus::Completed => {
                    summary.completed_nodes += 1;
                    continue;
                }
                EffectiveTaskStatus::Abandoned => {
                    summary.abandoned_nodes += 1;
                    continue;
                }
                EffectiveTaskStatus::Active => summary.in_progress_nodes += 1,
                _ => {}
            }

            let blockers = self.blockers(&CoordinationTaskId::new(task.id.0.clone()), now);
            let actionable = task_state.graph_actionable && blockers.is_empty();
            if actionable {
                summary.actionable_nodes += 1;
            } else {
                summary.execution_blocked_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_completion_gate(blocker.kind))
            {
                summary.completion_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_review_gate(blocker.kind))
            {
                summary.review_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_validation_gate(blocker.kind))
            {
                summary.validation_gated_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| is_task_stale_gate(blocker.kind))
            {
                summary.stale_nodes += 1;
            }
            if blockers
                .iter()
                .any(|blocker| blocker.kind == BlockerKind::ClaimConflict)
            {
                summary.claim_conflicted_nodes += 1;
            }
        }

        Some(summary)
    }
}

fn canonical_descendant_plan_ids(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<PlanId> {
    let mut plans = Vec::new();
    let mut tasks = Vec::new();
    collect_canonical_descendants(graph, plan_id, &mut plans, &mut tasks);
    plans
}

fn canonical_descendant_task_ids(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<TaskId> {
    let mut plans = Vec::new();
    let mut tasks = Vec::new();
    collect_canonical_descendants(graph, plan_id, &mut plans, &mut tasks);
    tasks
}

fn collect_canonical_descendants(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
    plans: &mut Vec<PlanId>,
    tasks: &mut Vec<TaskId>,
) {
    for child in graph.children_of_plan(plan_id) {
        match child.kind {
            NodeRefKind::Plan => {
                let child_plan = PlanId::new(child.id.clone());
                plans.push(child_plan.clone());
                collect_canonical_descendants(graph, &child_plan, plans, tasks);
            }
            NodeRefKind::Task => tasks.push(TaskId::new(child.id)),
        }
    }
}

fn is_task_completion_gate(kind: BlockerKind) -> bool {
    !matches!(kind, BlockerKind::Dependency | BlockerKind::ClaimConflict)
}

fn is_task_review_gate(kind: BlockerKind) -> bool {
    matches!(
        kind,
        BlockerKind::ReviewRequired | BlockerKind::RiskReviewRequired
    )
}

fn is_task_validation_gate(kind: BlockerKind) -> bool {
    kind == BlockerKind::ValidationRequired
}

fn is_task_stale_gate(kind: BlockerKind) -> bool {
    matches!(
        kind,
        BlockerKind::StaleRevision | BlockerKind::ArtifactStale
    )
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending | DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked
        | DerivedPlanStatus::BrokenDependency
        | DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
    }
}
