use prism_ir::{ArtifactStatus, ClaimStatus};
use serde::{Deserialize, Serialize};

use crate::types::{Artifact, CoordinationSnapshot, CoordinationTask, WorkClaim};

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
