use prism_ir::{
    ArtifactStatus, ClaimStatus, CoordinationEventKind, CoordinationTaskId, CoordinationTaskStatus,
    PlanId, PlanOperatorState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    types::{Artifact, CoordinationSnapshot, PolicyViolation, PolicyViolationRecord, WorkClaim},
    CoordinationDerivations, CoordinationSnapshotV2,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordinationReadModel {
    #[serde(default)]
    pub revision: u64,
    pub active_plan_ids: Vec<PlanId>,
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
    coordination_read_model_from_snapshot_v2(&snapshot.to_canonical_snapshot_v2())
}

pub fn coordination_read_model_from_snapshot_v2(
    snapshot: &CoordinationSnapshotV2,
) -> CoordinationReadModel {
    let derivations = CoordinationDerivations::derive(snapshot)
        .expect("canonical coordination snapshot should validate before deriving read models");

    let mut active_plan_ids = snapshot
        .plans
        .iter()
        .filter(|plan| plan.operator_state == PlanOperatorState::None)
        .map(|plan| plan.id.clone())
        .collect::<Vec<_>>();
    active_plan_ids.sort_by(|left, right| left.0.cmp(&right.0));

    let mut in_review_task_ids = snapshot
        .tasks
        .iter()
        .filter(|task| canonical_task_is_in_review(task))
        .map(|task| CoordinationTaskId::new(task.id.0.clone()))
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
        .filter_map(policy_violation_record_from_event)
        .collect::<Vec<_>>();
    sort_recent_violations(&mut recent_violations);

    let _ = derivations;
    CoordinationReadModel {
        revision: 0,
        active_plan_ids,
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
                refresh_active_plan(&mut model.active_plan_ids, snapshot, event.plan.as_ref());
            }
            CoordinationEventKind::TaskCreated
            | CoordinationEventKind::TaskAssigned
            | CoordinationEventKind::TaskBlocked
            | CoordinationEventKind::TaskUnblocked
            | CoordinationEventKind::TaskStatusChanged
            | CoordinationEventKind::TaskHeartbeated
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
    sort_active_plans(&mut model.active_plan_ids);
    sort_task_ids(&mut model.in_review_task_ids);
    sort_active_claims(&mut model.active_claims);
    sort_pending_review_artifacts(&mut model.pending_review_artifacts);
    sort_recent_violations(&mut model.recent_violations);
    model
}

pub fn ready_task_count_for_active_plans(
    active_plan_ids: &[PlanId],
    ready_tasks_for_plan: impl Fn(&PlanId) -> usize,
) -> usize {
    active_plan_ids.iter().map(ready_tasks_for_plan).sum()
}

fn refresh_active_plan(
    active_plan_ids: &mut Vec<PlanId>,
    snapshot: &CoordinationSnapshot,
    plan_id: Option<&PlanId>,
) {
    let Some(plan_id) = plan_id else {
        return;
    };
    upsert_or_remove_by_plan_id(
        active_plan_ids,
        plan_id,
        snapshot
            .plans
            .iter()
            .find(|plan| &plan.id == plan_id && plan.status == prism_ir::PlanStatus::Active)
            .map(|plan| plan.id.clone()),
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
    let Some(next) = policy_violation_record_from_event(event) else {
        return;
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

fn upsert_or_remove_by_plan_id(
    active_plan_ids: &mut Vec<PlanId>,
    plan_id: &PlanId,
    next: Option<PlanId>,
) {
    if let Some(next) = next {
        if let Some(existing) = active_plan_ids
            .iter_mut()
            .find(|existing| **existing == *plan_id)
        {
            *existing = next;
        } else {
            active_plan_ids.push(next);
        }
    } else {
        active_plan_ids.retain(|existing| *existing != *plan_id);
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

fn sort_active_plans(active_plan_ids: &mut [PlanId]) {
    active_plan_ids.sort_by(|left, right| left.0.cmp(&right.0));
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

fn policy_violation_record_from_event(
    event: &crate::types::CoordinationEvent,
) -> Option<PolicyViolationRecord> {
    let violations = event
        .metadata
        .get("violations")
        .and_then(|value| serde_json::from_value::<Vec<PolicyViolation>>(value.clone()).ok())
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
}

fn canonical_task_is_in_review(task: &crate::CanonicalTaskRecord) -> bool {
    matches!(
        task.metadata.get("legacy_phase").and_then(Value::as_str),
        Some("in_review" | "validating")
    )
}
