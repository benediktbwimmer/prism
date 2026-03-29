use std::path::Path;

use anyhow::Result;
use prism_coordination::{CoordinationEvent, CoordinationSnapshot};
use prism_ir::SessionId;
use prism_store::{CoordinationPersistBatch, CoordinationPersistResult, Store};

use crate::published_plans::{
    load_hydrated_coordination_plan_state, load_hydrated_coordination_snapshot,
    sync_repo_published_plans, HydratedCoordinationPlanState,
};
use crate::workspace_identity::coordination_persist_context_for_root;

pub(crate) trait CoordinationPersistenceBackend: Store {
    fn load_hydrated_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshot>> {
        load_hydrated_coordination_snapshot(root, self.load_coordination_snapshot()?)
    }

    fn load_hydrated_coordination_plan_state_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<HydratedCoordinationPlanState>> {
        load_hydrated_coordination_plan_state(root, self.load_coordination_snapshot()?)
    }

    fn persist_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, None),
            expected_revision: None,
            snapshot: snapshot.clone(),
            appended_events,
        })?;
        sync_repo_published_plans(root, snapshot)
    }

    fn persist_coordination_mutation_for_root(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
    ) -> Result<CoordinationPersistResult> {
        self.persist_coordination_mutation_for_root_with_session(
            root,
            expected_revision,
            snapshot,
            appended_events,
            None,
        )
    }

    fn persist_coordination_mutation_for_root_with_session(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
        session_id: Option<&SessionId>,
    ) -> Result<CoordinationPersistResult> {
        let result = self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, session_id),
            expected_revision: Some(expected_revision),
            snapshot: snapshot.clone(),
            appended_events: appended_events.to_vec(),
        })?;
        sync_repo_published_plans(root, snapshot)?;
        Ok(result)
    }
}

impl<T: Store + ?Sized> CoordinationPersistenceBackend for T {}

fn coordination_event_delta(
    existing_events: &[CoordinationEvent],
    next_events: &[CoordinationEvent],
) -> Vec<CoordinationEvent> {
    next_events
        .iter()
        .filter(|event| {
            !existing_events
                .iter()
                .any(|stored| stored.meta.id == event.meta.id)
        })
        .cloned()
        .collect()
}
