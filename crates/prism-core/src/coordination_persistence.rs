use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_seed, coordination_read_model_from_seed,
    coordination_snapshot_from_events, CoordinationEvent, CoordinationSnapshot,
};
use prism_ir::{PlanExecutionOverlay, PlanGraph, SessionId};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationPersistBatch,
    CoordinationPersistResult,
};
use serde_json::{json, Value};

use crate::published_plans::{
    load_hydrated_coordination_plan_state, load_hydrated_coordination_snapshot,
    sync_repo_published_plan_state, sync_repo_published_plan_state_observed,
    sync_repo_published_plans, HydratedCoordinationPlanState,
};
use crate::tracked_snapshot::publish_context_from_coordination_events;
use crate::workspace_identity::coordination_persist_context_for_root;

const COORDINATION_COMPACTION_SUFFIX_THRESHOLD: usize = 128;

fn observe_coordination_step<T, E, O, F, A>(
    observe_phase: &mut O,
    operation: &str,
    success_args: A,
    step: F,
) -> std::result::Result<T, E>
where
    E: ToString,
    O: FnMut(&str, Duration, Value, bool, Option<String>),
    F: FnOnce() -> std::result::Result<T, E>,
    A: FnOnce(&T) -> Value,
{
    let started = Instant::now();
    match step() {
        Ok(value) => {
            observe_phase(
                operation,
                started.elapsed(),
                success_args(&value),
                true,
                None,
            );
            Ok(value)
        }
        Err(error) => {
            observe_phase(
                operation,
                started.elapsed(),
                json!({}),
                false,
                Some(error.to_string()),
            );
            Err(error)
        }
    }
}

pub(crate) trait CoordinationPersistenceBackend:
    CoordinationJournal + CoordinationCheckpointStore
{
    fn persist_coordination_authoritative_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
    ) -> Result<CoordinationPersistResult> {
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let result = self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, None),
            expected_revision: None,
            appended_events,
        })?;
        match (plan_graphs, execution_overlays) {
            (Some(plan_graphs), Some(execution_overlays)) => sync_repo_published_plan_state(
                root,
                snapshot,
                None,
                None,
                plan_graphs.to_vec(),
                execution_overlays.clone(),
                None,
            )?,
            _ => sync_repo_published_plans(root, snapshot, None)?,
        }
        Ok(result)
    }

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
        let existing_read_model = self.load_coordination_read_model()?;
        let existing_queue_read_model = self.load_coordination_queue_read_model()?;
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let _ = self.persist_coordination_authoritative_state_for_root(
            root,
            snapshot,
            plan_graphs,
            execution_overlays,
        )?;
        let read_model = coordination_read_model_from_seed(
            snapshot,
            existing_read_model.as_ref(),
            &appended_events,
        );
        let queue_read_model = coordination_queue_read_model_from_seed(
            snapshot,
            existing_queue_read_model.as_ref(),
            &appended_events,
        );
        self.save_coordination_read_model(&read_model)?;
        self.save_coordination_queue_read_model(&queue_read_model)?;
        self.maybe_compact_coordination_events(snapshot)?;
        Ok(())
    }

    fn persist_coordination_authoritative_mutation_state_for_root_with_session_observed<O>(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
        session_id: Option<&SessionId>,
        previous_snapshot: Option<&CoordinationSnapshot>,
        previous_plan_graphs: Option<&[PlanGraph]>,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
        mut observe_phase: O,
    ) -> Result<CoordinationPersistResult>
    where
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        let result = observe_coordination_step(
            &mut observe_phase,
            "mutation.coordination.commitPersistBatch",
            |result: &CoordinationPersistResult| {
                json!({
                    "applied": result.applied,
                    "appendedEventCount": appended_events.len(),
                })
            },
            || {
                self.commit_coordination_persist_batch(&CoordinationPersistBatch {
                    context: coordination_persist_context_for_root(root, session_id),
                    expected_revision: Some(expected_revision),
                    appended_events: appended_events.to_vec(),
                })
            },
        )?;
        let sync_started = Instant::now();
        let sync_mode = if plan_graphs.is_some() && execution_overlays.is_some() {
            "state"
        } else {
            "snapshot"
        };
        let publish_context = publish_context_from_coordination_events(appended_events);
        let sync_result = match (
            previous_snapshot,
            previous_plan_graphs,
            plan_graphs,
            execution_overlays,
        ) {
            (
                Some(previous_snapshot),
                Some(previous_plan_graphs),
                Some(plan_graphs),
                Some(execution_overlays),
            ) => sync_repo_published_plan_state_observed(
                root,
                snapshot,
                Some(previous_snapshot),
                Some(previous_plan_graphs),
                plan_graphs.to_vec(),
                execution_overlays.clone(),
                publish_context.as_ref(),
                &mut observe_phase,
            ),
            (_, _, Some(plan_graphs), Some(execution_overlays)) => {
                sync_repo_published_plan_state_observed(
                    root,
                    snapshot,
                    None,
                    None,
                    plan_graphs.to_vec(),
                    execution_overlays.clone(),
                    publish_context.as_ref(),
                    &mut observe_phase,
                )
            }
            _ => sync_repo_published_plans(root, snapshot, publish_context.as_ref()),
        };
        match sync_result {
            Ok(()) => observe_phase(
                "mutation.coordination.syncPublishedPlans",
                sync_started.elapsed(),
                json!({ "mode": sync_mode }),
                true,
                None,
            ),
            Err(error) => {
                observe_phase(
                    "mutation.coordination.syncPublishedPlans",
                    sync_started.elapsed(),
                    json!({ "mode": sync_mode }),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        }
        Ok(result)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn persist_coordination_mutation_state_for_root_with_session(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
        session_id: Option<&SessionId>,
        previous_snapshot: Option<&CoordinationSnapshot>,
        previous_plan_graphs: Option<&[PlanGraph]>,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
    ) -> Result<CoordinationPersistResult> {
        self.persist_coordination_mutation_state_for_root_with_session_observed(
            root,
            expected_revision,
            snapshot,
            appended_events,
            session_id,
            previous_snapshot,
            previous_plan_graphs,
            plan_graphs,
            execution_overlays,
            |_operation, _duration, _args, _success, _error| {},
        )
    }

    fn persist_coordination_mutation_state_for_root_with_session_observed<O>(
        &mut self,
        root: &Path,
        expected_revision: u64,
        snapshot: &CoordinationSnapshot,
        appended_events: &[CoordinationEvent],
        session_id: Option<&SessionId>,
        previous_snapshot: Option<&CoordinationSnapshot>,
        previous_plan_graphs: Option<&[PlanGraph]>,
        plan_graphs: Option<&[PlanGraph]>,
        execution_overlays: Option<&BTreeMap<String, Vec<PlanExecutionOverlay>>>,
        mut observe_phase: O,
    ) -> Result<CoordinationPersistResult>
    where
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        let existing_read_model = observe_coordination_step(
            &mut observe_phase,
            "mutation.coordination.loadReadModel",
            |model: &Option<_>| json!({ "hadReadModel": model.is_some() }),
            || self.load_coordination_read_model(),
        )?;
        let existing_queue_read_model = observe_coordination_step(
            &mut observe_phase,
            "mutation.coordination.loadQueueReadModel",
            |model: &Option<_>| json!({ "hadQueueReadModel": model.is_some() }),
            || self.load_coordination_queue_read_model(),
        )?;
        let result = self
            .persist_coordination_authoritative_mutation_state_for_root_with_session_observed(
                root,
                expected_revision,
                snapshot,
                appended_events,
                session_id,
                previous_snapshot,
                previous_plan_graphs,
                plan_graphs,
                execution_overlays,
                &mut observe_phase,
            )?;
        let read_model_started = Instant::now();
        let read_model = coordination_read_model_from_seed(
            snapshot,
            existing_read_model.as_ref(),
            appended_events,
        );
        observe_phase(
            "mutation.coordination.buildReadModel",
            read_model_started.elapsed(),
            json!({
                "appendedEventCount": appended_events.len(),
                "eventCount": snapshot.events.len(),
            }),
            true,
            None,
        );
        let queue_read_model_started = Instant::now();
        let queue_read_model = coordination_queue_read_model_from_seed(
            snapshot,
            existing_queue_read_model.as_ref(),
            appended_events,
        );
        observe_phase(
            "mutation.coordination.buildQueueReadModel",
            queue_read_model_started.elapsed(),
            json!({
                "appendedEventCount": appended_events.len(),
                "taskCount": snapshot.tasks.len(),
            }),
            true,
            None,
        );
        observe_coordination_step(
            &mut observe_phase,
            "mutation.coordination.saveReadModel",
            |_| json!({ "eventCount": snapshot.events.len() }),
            || self.save_coordination_read_model(&read_model),
        )?;
        observe_coordination_step(
            &mut observe_phase,
            "mutation.coordination.saveQueueReadModel",
            |_| json!({ "taskCount": snapshot.tasks.len() }),
            || self.save_coordination_queue_read_model(&queue_read_model),
        )?;
        if result.applied {
            observe_coordination_step(
                &mut observe_phase,
                "mutation.coordination.compactEvents",
                |_| json!({ "applied": true }),
                || self.maybe_compact_coordination_events(snapshot),
            )?;
        } else {
            observe_phase(
                "mutation.coordination.compactEvents",
                Duration::default(),
                json!({ "applied": false, "compaction": "skipped" }),
                true,
                None,
            );
        }
        Ok(result)
    }
}

impl<T: CoordinationJournal + CoordinationCheckpointStore + ?Sized> CoordinationPersistenceBackend
    for T
{
}

trait CoordinationCompactionBackend: CoordinationJournal + CoordinationCheckpointStore {
    fn maybe_compact_coordination_events(&mut self, snapshot: &CoordinationSnapshot) -> Result<()> {
        let stream = self.load_coordination_event_stream()?;
        if stream.suffix_events.len() < COORDINATION_COMPACTION_SUFFIX_THRESHOLD {
            return Ok(());
        }
        self.save_coordination_compaction(snapshot)
    }
}

impl<T: CoordinationJournal + CoordinationCheckpointStore + ?Sized> CoordinationCompactionBackend
    for T
{
}

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
