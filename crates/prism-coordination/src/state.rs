use std::collections::HashMap;
use std::sync::RwLock;

use crate::helpers::sorted_values;
use crate::types::{
    Artifact, ArtifactReview, CoordinationEvent, CoordinationSnapshot, CoordinationTask, Plan,
    WorkClaim,
};
use prism_ir::{ArtifactId, ClaimId, CoordinationTaskId, PlanId, ReviewId};

#[derive(Default)]
pub struct CoordinationStore {
    pub(crate) state: RwLock<CoordinationState>,
}

#[derive(Default)]
pub(crate) struct CoordinationState {
    pub(crate) plans: HashMap<PlanId, Plan>,
    pub(crate) tasks: HashMap<CoordinationTaskId, CoordinationTask>,
    pub(crate) claims: HashMap<ClaimId, WorkClaim>,
    pub(crate) artifacts: HashMap<ArtifactId, Artifact>,
    pub(crate) reviews: HashMap<ReviewId, ArtifactReview>,
    pub(crate) events: Vec<CoordinationEvent>,
    pub(crate) next_plan: u64,
    pub(crate) next_task: u64,
    pub(crate) next_claim: u64,
    pub(crate) next_artifact: u64,
    pub(crate) next_review: u64,
}

impl CoordinationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: CoordinationSnapshot) -> Self {
        let store = Self::new();
        store.replace_from_snapshot(snapshot);
        store
    }

    pub fn replace_from_snapshot(&self, snapshot: CoordinationSnapshot) {
        let mut state = self
            .state
            .write()
            .expect("coordination store lock poisoned");
        *state = CoordinationState {
            plans: snapshot
                .plans
                .into_iter()
                .map(|plan| (plan.id.clone(), plan))
                .collect(),
            tasks: snapshot
                .tasks
                .into_iter()
                .map(|task| (task.id.clone(), task))
                .collect(),
            claims: snapshot
                .claims
                .into_iter()
                .map(|claim| (claim.id.clone(), claim))
                .collect(),
            artifacts: snapshot
                .artifacts
                .into_iter()
                .map(|artifact| (artifact.id.clone(), artifact))
                .collect(),
            reviews: snapshot
                .reviews
                .into_iter()
                .map(|review| (review.id.clone(), review))
                .collect(),
            events: snapshot.events,
            next_plan: snapshot.next_plan,
            next_task: snapshot.next_task,
            next_claim: snapshot.next_claim,
            next_artifact: snapshot.next_artifact,
            next_review: snapshot.next_review,
        };
    }

    pub fn snapshot(&self) -> CoordinationSnapshot {
        let state = self.state.read().expect("coordination store lock poisoned");
        CoordinationSnapshot {
            plans: sorted_values(&state.plans, |plan| plan.id.0.to_string()),
            tasks: sorted_values(&state.tasks, |task| task.id.0.to_string()),
            claims: sorted_values(&state.claims, |claim| claim.id.0.to_string()),
            artifacts: sorted_values(&state.artifacts, |artifact| artifact.id.0.to_string()),
            reviews: sorted_values(&state.reviews, |review| review.id.0.to_string()),
            events: state.events.clone(),
            next_plan: state.next_plan,
            next_task: state.next_task,
            next_claim: state.next_claim,
            next_artifact: state.next_artifact,
            next_review: state.next_review,
        }
    }
}
