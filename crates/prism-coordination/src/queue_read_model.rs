use prism_ir::{ArtifactStatus, ClaimStatus};
use serde::{Deserialize, Serialize};

use crate::types::{
    Artifact, CoordinationEvent, CoordinationSnapshot, CoordinationTask, WorkClaim,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordinationQueueReadModel {
    pub pending_handoff_tasks: Vec<CoordinationTask>,
    pub active_claims: Vec<WorkClaim>,
    pub pending_review_artifacts: Vec<Artifact>,
}

pub fn coordination_queue_read_model_from_snapshot(
    snapshot: &CoordinationSnapshot,
) -> CoordinationQueueReadModel {
    let mut pending_handoff_tasks = snapshot
        .tasks
        .iter()
        .filter(|task| task.pending_handoff_to.is_some())
        .cloned()
        .collect::<Vec<_>>();
    pending_handoff_tasks.sort_by(|left, right| left.id.0.cmp(&right.id.0));

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

    CoordinationQueueReadModel {
        pending_handoff_tasks,
        active_claims,
        pending_review_artifacts,
    }
}

pub fn coordination_queue_read_model_from_seed(
    snapshot: &CoordinationSnapshot,
    seed: Option<&CoordinationQueueReadModel>,
    appended_events: &[CoordinationEvent],
) -> CoordinationQueueReadModel {
    let Some(seed) = seed else {
        return coordination_queue_read_model_from_snapshot(snapshot);
    };
    if appended_events.is_empty() {
        return seed.clone();
    }

    let mut model = seed.clone();
    for event in appended_events {
        match event.kind {
            prism_ir::CoordinationEventKind::TaskCreated
            | prism_ir::CoordinationEventKind::TaskAssigned
            | prism_ir::CoordinationEventKind::TaskBlocked
            | prism_ir::CoordinationEventKind::TaskUnblocked
            | prism_ir::CoordinationEventKind::TaskStatusChanged
            | prism_ir::CoordinationEventKind::TaskHeartbeated
            | prism_ir::CoordinationEventKind::TaskResumed
            | prism_ir::CoordinationEventKind::TaskReclaimed
            | prism_ir::CoordinationEventKind::HandoffRequested
            | prism_ir::CoordinationEventKind::HandoffAccepted => {
                refresh_pending_handoff_task(
                    &mut model.pending_handoff_tasks,
                    snapshot,
                    event.task.as_ref(),
                );
            }
            prism_ir::CoordinationEventKind::ClaimAcquired
            | prism_ir::CoordinationEventKind::ClaimRenewed
            | prism_ir::CoordinationEventKind::ClaimReleased
            | prism_ir::CoordinationEventKind::ClaimContended => {
                refresh_active_claim(&mut model.active_claims, snapshot, event.claim.as_ref());
            }
            prism_ir::CoordinationEventKind::ArtifactProposed
            | prism_ir::CoordinationEventKind::ArtifactReviewed
            | prism_ir::CoordinationEventKind::ArtifactSuperseded => {
                refresh_pending_review_artifact(
                    &mut model.pending_review_artifacts,
                    snapshot,
                    event.artifact.as_ref(),
                );
            }
            prism_ir::CoordinationEventKind::PlanCreated
            | prism_ir::CoordinationEventKind::PlanUpdated
            | prism_ir::CoordinationEventKind::MutationRejected => {}
        }
    }

    model
        .pending_handoff_tasks
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    model
        .active_claims
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    model
        .pending_review_artifacts
        .sort_by(|left, right| left.id.0.cmp(&right.id.0));
    model
}

fn refresh_pending_handoff_task(
    pending_handoff_tasks: &mut Vec<CoordinationTask>,
    snapshot: &CoordinationSnapshot,
    task_id: Option<&prism_ir::CoordinationTaskId>,
) {
    let Some(task_id) = task_id else {
        return;
    };
    upsert_or_remove_task(
        pending_handoff_tasks,
        task_id,
        snapshot
            .tasks
            .iter()
            .find(|task| &task.id == task_id && task.pending_handoff_to.is_some())
            .cloned(),
    );
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

fn upsert_or_remove_task(
    tasks: &mut Vec<CoordinationTask>,
    task_id: &prism_ir::CoordinationTaskId,
    next: Option<CoordinationTask>,
) {
    if let Some(next) = next {
        if let Some(existing) = tasks.iter_mut().find(|task| task.id == *task_id) {
            *existing = next;
        } else {
            tasks.push(next);
        }
    } else {
        tasks.retain(|task| task.id != *task_id);
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
