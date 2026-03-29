use prism_ir::{
    ArtifactStatus, ClaimStatus, CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus,
    PlanId, PlanStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{
    Artifact, CoordinationSnapshot, Plan, PolicyViolation, PolicyViolationRecord, WorkClaim,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordinationReadModel {
    pub active_plans: Vec<Plan>,
    pub task_count: usize,
    pub in_review_task_ids: Vec<CoordinationTaskId>,
    pub active_claims: Vec<WorkClaim>,
    pub pending_review_artifacts: Vec<Artifact>,
    pub proposed_artifact_count: usize,
    pub recent_violations: Vec<PolicyViolationRecord>,
}

pub fn coordination_read_model_from_snapshot(
    snapshot: &CoordinationSnapshot,
) -> CoordinationReadModel {
    let mut active_plans = snapshot
        .plans
        .iter()
        .filter(|plan| plan.status == PlanStatus::Active)
        .cloned()
        .collect::<Vec<_>>();
    active_plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    let mut in_review_task_ids = snapshot
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                CoordinationTaskStatus::InReview | CoordinationTaskStatus::Validating
            )
        })
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    in_review_task_ids.sort_by(|left, right| left.0.cmp(&right.0));

    let mut active_claims = snapshot
        .claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Active)
        .cloned()
        .collect::<Vec<_>>();
    active_claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    let mut pending_review_artifacts = snapshot
        .artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.status,
                ArtifactStatus::Proposed | ArtifactStatus::InReview
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    pending_review_artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));

    let proposed_artifact_count = snapshot
        .artifacts
        .iter()
        .filter(|artifact| artifact.status == ArtifactStatus::Proposed)
        .count();

    let mut recent_violations = snapshot
        .events
        .iter()
        .filter(|event| event.kind == CoordinationEventKind::MutationRejected)
        .filter_map(|event| {
            let violations = event
                .metadata
                .get("violations")
                .and_then(|value| {
                    serde_json::from_value::<Vec<PolicyViolation>>(value.clone()).ok()
                })
                .unwrap_or_default();
            if violations.is_empty() && event.metadata == Value::Null {
                return None;
            }
            Some(PolicyViolationRecord {
                event_id: event.meta.id.clone(),
                ts: event.meta.ts,
                summary: event.summary.clone(),
                plan_id: event.plan.clone(),
                task_id: event.task.clone(),
                claim_id: event.claim.clone(),
                artifact_id: event.artifact.clone(),
                violations,
            })
        })
        .collect::<Vec<_>>();
    recent_violations.sort_by(|left, right| {
        right
            .ts
            .cmp(&left.ts)
            .then_with(|| left.event_id.0.cmp(&right.event_id.0))
    });

    CoordinationReadModel {
        active_plans,
        task_count: snapshot.tasks.len(),
        in_review_task_ids,
        active_claims,
        pending_review_artifacts,
        proposed_artifact_count,
        recent_violations,
    }
}

pub fn ready_task_count_for_active_plans(
    active_plans: &[Plan],
    ready_tasks_for_plan: impl Fn(&PlanId) -> usize,
) -> usize {
    active_plans
        .iter()
        .map(|plan| ready_tasks_for_plan(&plan.id))
        .sum()
}
