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

pub fn coordination_read_model_from_seed(
    snapshot: &CoordinationSnapshot,
    seed: Option<&CoordinationReadModel>,
    appended_events: &[crate::types::CoordinationEvent],
) -> CoordinationReadModel {
    let Some(seed) = seed else {
        return coordination_read_model_from_snapshot(snapshot);
    };
    if appended_events.is_empty() {
        return seed.clone();
    }

    let mut model = seed.clone();
    for event in appended_events {
        match event.kind {
            CoordinationEventKind::PlanCreated | CoordinationEventKind::PlanUpdated => {
                refresh_active_plan(&mut model.active_plans, snapshot, event.plan.as_ref());
            }
            CoordinationEventKind::TaskCreated
            | CoordinationEventKind::TaskAssigned
            | CoordinationEventKind::TaskBlocked
            | CoordinationEventKind::TaskUnblocked
            | CoordinationEventKind::TaskStatusChanged
            | CoordinationEventKind::TaskResumed
            | CoordinationEventKind::TaskReclaimed
            | CoordinationEventKind::HandoffRequested
            | CoordinationEventKind::HandoffAccepted => {
                refresh_in_review_task(
                    &mut model.in_review_task_ids,
                    snapshot,
                    event.task.as_ref(),
                );
            }
            CoordinationEventKind::ClaimAcquired
            | CoordinationEventKind::ClaimRenewed
            | CoordinationEventKind::ClaimReleased
            | CoordinationEventKind::ClaimContended => {
                refresh_active_claim(&mut model.active_claims, snapshot, event.claim.as_ref());
            }
            CoordinationEventKind::ArtifactProposed
            | CoordinationEventKind::ArtifactReviewed
            | CoordinationEventKind::ArtifactSuperseded => {
                refresh_pending_review_artifact(
                    &mut model.pending_review_artifacts,
                    snapshot,
                    event.artifact.as_ref(),
                );
            }
            CoordinationEventKind::MutationRejected => {
                upsert_recent_violation(&mut model.recent_violations, event);
            }
        }
    }

    model.task_count = snapshot.tasks.len();
    model.proposed_artifact_count = model
        .pending_review_artifacts
        .iter()
        .filter(|artifact| artifact.status == ArtifactStatus::Proposed)
        .count();
    sort_active_plans(&mut model.active_plans);
    sort_task_ids(&mut model.in_review_task_ids);
    sort_active_claims(&mut model.active_claims);
    sort_pending_review_artifacts(&mut model.pending_review_artifacts);
    sort_recent_violations(&mut model.recent_violations);
    model
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

fn refresh_active_plan(
    active_plans: &mut Vec<Plan>,
    snapshot: &CoordinationSnapshot,
    plan_id: Option<&PlanId>,
) {
    let Some(plan_id) = plan_id else {
        return;
    };
    upsert_or_remove_by_plan_id(
        active_plans,
        plan_id,
        snapshot
            .plans
            .iter()
            .find(|plan| &plan.id == plan_id && plan.status == PlanStatus::Active)
            .cloned(),
    );
}

fn refresh_in_review_task(
    in_review_task_ids: &mut Vec<CoordinationTaskId>,
    snapshot: &CoordinationSnapshot,
    task_id: Option<&CoordinationTaskId>,
) {
    let Some(task_id) = task_id else {
        return;
    };
    let next = snapshot
        .tasks
        .iter()
        .find(|task| &task.id == task_id)
        .filter(|task| {
            matches!(
                task.status,
                CoordinationTaskStatus::InReview | CoordinationTaskStatus::Validating
            )
        })
        .map(|task| task.id.clone());
    upsert_or_remove_task_id(in_review_task_ids, task_id, next);
}

fn refresh_active_claim(
    active_claims: &mut Vec<WorkClaim>,
    snapshot: &CoordinationSnapshot,
    claim_id: Option<&prism_ir::ClaimId>,
) {
    let Some(claim_id) = claim_id else {
        return;
    };
    upsert_or_remove_claim(
        active_claims,
        claim_id,
        snapshot
            .claims
            .iter()
            .find(|claim| &claim.id == claim_id && claim.status == ClaimStatus::Active)
            .cloned(),
    );
}

fn refresh_pending_review_artifact(
    pending_review_artifacts: &mut Vec<Artifact>,
    snapshot: &CoordinationSnapshot,
    artifact_id: Option<&prism_ir::ArtifactId>,
) {
    let Some(artifact_id) = artifact_id else {
        return;
    };
    upsert_or_remove_artifact(
        pending_review_artifacts,
        artifact_id,
        snapshot
            .artifacts
            .iter()
            .find(|artifact| {
                &artifact.id == artifact_id
                    && matches!(
                        artifact.status,
                        ArtifactStatus::Proposed | ArtifactStatus::InReview
                    )
            })
            .cloned(),
    );
}

fn upsert_recent_violation(
    recent_violations: &mut Vec<PolicyViolationRecord>,
    event: &crate::types::CoordinationEvent,
) {
    let violations = event
        .metadata
        .get("violations")
        .and_then(|value| serde_json::from_value::<Vec<PolicyViolation>>(value.clone()).ok())
        .unwrap_or_default();
    if violations.is_empty() && event.metadata == Value::Null {
        return;
    }
    let next = PolicyViolationRecord {
        event_id: event.meta.id.clone(),
        ts: event.meta.ts,
        summary: event.summary.clone(),
        plan_id: event.plan.clone(),
        task_id: event.task.clone(),
        claim_id: event.claim.clone(),
        artifact_id: event.artifact.clone(),
        violations,
    };
    if let Some(existing) = recent_violations
        .iter_mut()
        .find(|record| record.event_id == next.event_id)
    {
        *existing = next;
    } else {
        recent_violations.push(next);
    }
}

fn upsert_or_remove_by_plan_id(active_plans: &mut Vec<Plan>, plan_id: &PlanId, next: Option<Plan>) {
    if let Some(next) = next {
        if let Some(existing) = active_plans.iter_mut().find(|plan| plan.id == *plan_id) {
            *existing = next;
        } else {
            active_plans.push(next);
        }
    } else {
        active_plans.retain(|plan| plan.id != *plan_id);
    }
}

fn upsert_or_remove_task_id(
    task_ids: &mut Vec<CoordinationTaskId>,
    task_id: &CoordinationTaskId,
    next: Option<CoordinationTaskId>,
) {
    task_ids.retain(|existing| existing != task_id);
    if let Some(next) = next {
        task_ids.push(next);
    }
}

fn upsert_or_remove_claim(
    claims: &mut Vec<WorkClaim>,
    claim_id: &prism_ir::ClaimId,
    next: Option<WorkClaim>,
) {
    if let Some(next) = next {
        if let Some(existing) = claims.iter_mut().find(|claim| claim.id == *claim_id) {
            *existing = next;
        } else {
            claims.push(next);
        }
    } else {
        claims.retain(|claim| claim.id != *claim_id);
    }
}

fn upsert_or_remove_artifact(
    artifacts: &mut Vec<Artifact>,
    artifact_id: &prism_ir::ArtifactId,
    next: Option<Artifact>,
) {
    if let Some(next) = next {
        if let Some(existing) = artifacts
            .iter_mut()
            .find(|artifact| artifact.id == *artifact_id)
        {
            *existing = next;
        } else {
            artifacts.push(next);
        }
    } else {
        artifacts.retain(|artifact| artifact.id != *artifact_id);
    }
}

fn sort_active_plans(active_plans: &mut [Plan]) {
    active_plans.sort_by(|left, right| left.id.0.cmp(&right.id.0));
}

fn sort_task_ids(task_ids: &mut [CoordinationTaskId]) {
    task_ids.sort_by(|left, right| left.0.cmp(&right.0));
}

fn sort_active_claims(active_claims: &mut [WorkClaim]) {
    active_claims.sort_by(|left, right| left.id.0.cmp(&right.id.0));
}

fn sort_pending_review_artifacts(pending_review_artifacts: &mut [Artifact]) {
    pending_review_artifacts.sort_by(|left, right| left.id.0.cmp(&right.id.0));
}

fn sort_recent_violations(recent_violations: &mut [PolicyViolationRecord]) {
    recent_violations.sort_by(|left, right| {
        right
            .ts
            .cmp(&left.ts)
            .then_with(|| left.event_id.0.cmp(&right.event_id.0))
    });
}
