use anyhow::Result;
use prism_ir::{
    ArtifactId, ClaimId, EventMeta, ReviewId, SessionId, Timestamp, WorkspaceRevision,
};

use crate::mutations::{
    acquire_claim_mutation, propose_artifact_mutation, release_claim_mutation,
    renew_claim_mutation, review_artifact_mutation, supersede_artifact_mutation,
};
use crate::state::CoordinationState;
use crate::types::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationConflict, CoordinationSnapshot, WorkClaim,
};

pub struct CoordinationRuntimeState {
    state: CoordinationState,
}

impl CoordinationRuntimeState {
    pub fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        Self {
            state: CoordinationState::from_snapshot(snapshot),
        }
    }

    pub fn snapshot(&self) -> CoordinationSnapshot {
        self.state.snapshot()
    }

    pub fn acquire_claim(
        &mut self,
        meta: EventMeta,
        session_id: SessionId,
        input: ClaimAcquireInput,
    ) -> Result<(Option<ClaimId>, Vec<CoordinationConflict>, Option<WorkClaim>)> {
        acquire_claim_mutation(&mut self.state, meta, session_id, input)
    }

    pub fn renew_claim(
        &mut self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
    ) -> Result<WorkClaim> {
        renew_claim_mutation(&mut self.state, meta, session_id, claim_id, ttl_seconds)
    }

    pub fn release_claim(
        &mut self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
    ) -> Result<WorkClaim> {
        release_claim_mutation(&mut self.state, meta, session_id, claim_id)
    }

    pub fn propose_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactProposeInput,
    ) -> Result<(ArtifactId, Artifact)> {
        propose_artifact_mutation(&mut self.state, meta, input)
    }

    pub fn supersede_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        supersede_artifact_mutation(&mut self.state, meta, input)
    }

    pub fn review_artifact(
        &mut self,
        meta: EventMeta,
        input: ArtifactReviewInput,
        current_revision: WorkspaceRevision,
    ) -> Result<(ReviewId, ArtifactReview, Artifact)> {
        review_artifact_mutation(&mut self.state, meta, input, current_revision)
    }

    pub fn claims_for_anchor(
        &mut self,
        anchors: &[prism_ir::AnchorRef],
        now: Timestamp,
    ) -> Vec<WorkClaim> {
        crate::helpers::expire_claims_locked(&mut self.state, now);
        self.state
            .claims
            .values()
            .filter(|claim| crate::helpers::claim_is_active(claim, now))
            .filter(|claim| crate::helpers::anchors_overlap(&claim.anchors, anchors))
            .cloned()
            .collect()
    }
}
