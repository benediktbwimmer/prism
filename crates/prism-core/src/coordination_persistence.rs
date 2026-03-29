use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use prism_coordination::{coordination_snapshot_from_events, CoordinationEvent, CoordinationSnapshot};
use prism_ir::{PlanExecutionOverlay, PlanGraph, SessionId};
use prism_store::{CoordinationPersistBatch, CoordinationPersistResult, Store};

use crate::published_plans::{
    load_hydrated_coordination_plan_state, load_hydrated_coordination_snapshot,
    sync_repo_published_plan_state, sync_repo_published_plans, HydratedCoordinationPlanState,
};
use crate::workspace_identity::coordination_persist_context_for_root;

const COORDINATION_COMPACTION_SUFFIX_THRESHOLD: usize = 128;

pub(crate) trait CoordinationPersistenceBackend: Store {
    fn load_hydrated_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshot>> {
        let stream = self.load_coordination_event_stream()?;
        let snapshot =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot);
        load_hydrated_coordination_snapshot(root, snapshot)
    }

    fn load_hydrated_coordination_plan_state_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<HydratedCoordinationPlanState>> {
        let stream = self.load_coordination_event_stream()?;
        let snapshot =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot);
        load_hydrated_coordination_plan_state(root, snapshot)
    }

    fn persist_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        self.persist_coordination_state_for_root(root, snapshot, None, None)
    }

    fn persist_coordination_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
    ) -> Result<()> {
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, None),
            expected_revision: None,
            appended_events,
        })?;
        self.maybe_compact_coordination_events(snapshot)?;
        match (plan_graphs, execution_overlays) {
            (Some(plan_graphs), Some(execution_overlays)) => sync_repo_published_plan_state(
                root,
                snapshot,
                plan_graphs.to_vec(),
                execution_overlays.clone(),
            ),
            _ => sync_repo_published_plans(root, snapshot),
        }
    }

    fn persist_coordination_mutation_state_for_root_with_session(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
        session_id: Option<&SessionId>,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
    ) -> Result<CoordinationPersistResult> {
        let result = self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, session_id),
            expected_revision: Some(expected_revision),
            appended_events: appended_events.to_vec(),
        })?;
        if result.applied {
            self.maybe_compact_coordination_events(snapshot)?;
        }
        match (plan_graphs, execution_overlays) {
            (Some(plan_graphs), Some(execution_overlays)) => sync_repo_published_plan_state(
                root,
                snapshot,
                plan_graphs.to_vec(),
                execution_overlays.clone(),
            )?,
            _ => sync_repo_published_plans(root, snapshot)?,
        }
        Ok(result)
    }
}

impl<T: Store + ?Sized> CoordinationPersistenceBackend for T {}

trait CoordinationCompactionBackend: Store {
    fn maybe_compact_coordination_events(
        &mut self,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        let stream = self.load_coordination_event_stream()?;
        if stream.suffix_events.len() < COORDINATION_COMPACTION_SUFFIX_THRESHOLD {
            return Ok(());
        }
        self.save_coordination_compaction(snapshot)
    }
}

impl<T: Store + ?Sized> CoordinationCompactionBackend for T {}

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
