use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_seed, coordination_read_model_from_seed,
    coordination_snapshot_from_events, snapshot_plan_graphs, CoordinationEvent,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2, TaskGitExecution,
};
use prism_ir::{PlanGraph, SessionId};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationPersistBatch,
    CoordinationPersistResult,
};
use serde_json::{json, Value};

use crate::coordination_snapshot_sanitization::sanitize_persisted_coordination_snapshot;
use crate::coordination_startup_checkpoint::{
    load_materialized_coordination_plan_state, load_materialized_coordination_snapshot,
    load_materialized_coordination_snapshot_v2, save_shared_coordination_startup_checkpoint,
};
use crate::published_plans::{
    execution_overlays_by_plan, load_hydrated_coordination_plan_state,
    load_hydrated_coordination_snapshot, load_hydrated_coordination_snapshot_v2,
    sync_repo_published_plans, HydratedCoordinationPlanState,
};
use crate::shared_coordination_ref::sync_shared_coordination_ref_state;
use crate::tracked_snapshot::{
    publish_context_from_coordination_events, sync_coordination_snapshot_state,
};
use crate::workspace_identity::coordination_persist_context_for_root;

const COORDINATION_COMPACTION_SUFFIX_THRESHOLD: usize = 128;
const TEST_SHARED_COORDINATION_REF_PUBLISH_OPT_IN: &str = "enable_shared_coordination_ref_publish";

fn shared_coordination_ref_publish_enabled(root: &Path) -> bool {
    let disabled = env::var_os("PRISM_TEST_DISABLE_SHARED_COORDINATION_REF_PUBLISH")
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false);
    if !disabled {
        return true;
    }
    root.join(".prism")
        .join("tests")
        .join(TEST_SHARED_COORDINATION_REF_PUBLISH_OPT_IN)
        .exists()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoordinationDerivedPersistenceMode {
    Inline,
    Deferred,
}

#[derive(Clone)]
struct CoordinationDerivedSyncInputs {
    plan_graphs: Vec<PlanGraph>,
}

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

fn sync_authoritative_shared_coordination_ref_observed<O>(
    root: &Path,
    snapshot: &CoordinationSnapshot,
    publish_context: Option<&crate::tracked_snapshot::TrackedSnapshotPublishContext>,
    observe_phase: &mut O,
) -> Result<()>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.publishedPlans.syncSharedCoordinationRef",
        |_| json!({}),
        || sync_shared_coordination_ref_state(root, snapshot, publish_context),
    )
}

fn sync_inline_coordination_projections_observed<S, O>(
    store: &mut S,
    root: &Path,
    authoritative_revision: u64,
    snapshot: &CoordinationSnapshot,
    derived: &CoordinationDerivedSyncInputs,
    publish_context: Option<&crate::tracked_snapshot::TrackedSnapshotPublishContext>,
    observe_phase: &mut O,
) -> Result<()>
where
    S: CoordinationJournal + CoordinationCheckpointStore + ?Sized,
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let repo_semantic_snapshot = repo_semantic_coordination_snapshot(snapshot.clone());
    let repo_semantic_execution_overlays =
        execution_overlays_by_plan(&repo_semantic_snapshot.tasks);
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.publishedPlans.syncTrackedSnapshot",
        |_| json!({}),
        || {
            sync_coordination_snapshot_state(
                root,
                &repo_semantic_snapshot,
                &derived.plan_graphs,
                &repo_semantic_execution_overlays,
                publish_context,
                Some(authoritative_revision),
            )
        },
    )?;
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.publishedPlans.saveStartupCheckpoint",
        |_| json!({}),
        || {
            save_shared_coordination_startup_checkpoint(
                root,
                store,
                &repo_semantic_snapshot,
                &[],
            )
        },
    )?;
    observe_phase(
        "mutation.coordination.syncPublishedPlans",
        Duration::ZERO,
        json!({ "mode": "state" }),
        true,
        None,
    );
    Ok(())
}

fn persist_coordination_read_models_and_compaction_observed<S, O>(
    store: &mut S,
    authoritative_revision: u64,
    snapshot: &CoordinationSnapshot,
    appended_events: &[CoordinationEvent],
    existing_read_model: Option<CoordinationReadModel>,
    existing_queue_read_model: Option<CoordinationQueueReadModel>,
    observe_phase: &mut O,
    applied: bool,
) -> Result<()>
where
    S: CoordinationJournal + CoordinationCheckpointStore + ?Sized,
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let read_model_started = Instant::now();
    let mut read_model =
        coordination_read_model_from_seed(snapshot, existing_read_model.as_ref(), appended_events);
    read_model.revision = authoritative_revision;
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
    let mut queue_read_model = coordination_queue_read_model_from_seed(
        snapshot,
        existing_queue_read_model.as_ref(),
        appended_events,
    );
    queue_read_model.revision = authoritative_revision;
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
        observe_phase,
        "mutation.coordination.saveReadModel",
        |_| json!({ "eventCount": snapshot.events.len() }),
        || store.save_coordination_read_model(&read_model),
    )?;
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.saveQueueReadModel",
        |_| json!({ "taskCount": snapshot.tasks.len() }),
        || store.save_coordination_queue_read_model(&queue_read_model),
    )?;
    if applied {
        observe_coordination_step(
            observe_phase,
            "mutation.coordination.compactEvents",
            |_| json!({ "applied": true }),
            || store.maybe_compact_coordination_events(snapshot),
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
    Ok(())
}

pub(crate) trait CoordinationPersistenceBackend:
    CoordinationJournal + CoordinationCheckpointStore
{
    fn persist_coordination_authoritative_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<CoordinationPersistResult> {
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let result = self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, None),
            expected_revision: None,
            appended_events,
        })?;
        sync_repo_published_plans(root, snapshot, None)?;
        save_shared_coordination_startup_checkpoint(root, self, snapshot, &[])?;
        Ok(result)
    }

    fn load_hydrated_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshot>> {
        let stream = self.load_coordination_event_stream()?;
        let snapshot =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot)
                .map(repo_semantic_coordination_snapshot);
        if let Some(snapshot) =
            load_materialized_coordination_snapshot(root, self, snapshot.clone())?
        {
            return Ok(Some(snapshot));
        }
        load_hydrated_coordination_snapshot(root, snapshot)
    }

    fn load_hydrated_coordination_snapshot_v2_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshotV2>> {
        let stream = self.load_coordination_event_stream()?;
        let snapshot =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot)
                .map(repo_semantic_coordination_snapshot);
        if let Some(snapshot_v2) =
            load_materialized_coordination_snapshot_v2(root, self, snapshot.clone())?
        {
            return Ok(Some(snapshot_v2));
        }
        load_hydrated_coordination_snapshot_v2(root, snapshot)
    }

    fn load_hydrated_coordination_plan_state_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<HydratedCoordinationPlanState>> {
        let stream = self.load_coordination_event_stream()?;
        let snapshot =
            coordination_snapshot_from_events(&stream.suffix_events, stream.fallback_snapshot)
                .map(repo_semantic_coordination_snapshot);
        if let Some(plan_state) =
            load_materialized_coordination_plan_state(root, self, snapshot.clone())?
        {
            return Ok(Some(plan_state));
        }
        load_hydrated_coordination_plan_state(root, snapshot)
    }

    fn persist_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        self.persist_coordination_state_for_root(root, snapshot)
    }

    fn persist_coordination_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        let existing_read_model = self.load_coordination_read_model()?;
        let existing_queue_read_model = self.load_coordination_queue_read_model()?;
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let _ = self.persist_coordination_authoritative_state_for_root(root, snapshot)?;
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
        _previous_snapshot: Option<&CoordinationSnapshot>,
        derived_persistence_mode: CoordinationDerivedPersistenceMode,
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
        if !result.applied {
            observe_phase(
                "mutation.coordination.publishedPlans.syncSharedCoordinationRef",
                Duration::ZERO,
                json!({ "skipped": true, "applied": false }),
                true,
                None,
            );
            if matches!(
                derived_persistence_mode,
                CoordinationDerivedPersistenceMode::Inline
            ) {
                observe_phase(
                    "mutation.coordination.publishedPlans.syncTrackedSnapshot",
                    Duration::ZERO,
                    json!({ "skipped": true, "applied": false }),
                    true,
                    None,
                );
                observe_phase(
                    "mutation.coordination.publishedPlans.saveStartupCheckpoint",
                    Duration::ZERO,
                    json!({ "skipped": true, "applied": false }),
                    true,
                    None,
                );
                observe_phase(
                    "mutation.coordination.syncPublishedPlans",
                    Duration::ZERO,
                    json!({ "mode": "noop" }),
                    true,
                    None,
                );
            } else {
                observe_phase(
                    "mutation.coordination.publishedPlans.syncTrackedSnapshot",
                    Duration::ZERO,
                    json!({ "deferred": true, "applied": false }),
                    true,
                    None,
                );
                observe_phase(
                    "mutation.coordination.publishedPlans.saveStartupCheckpoint",
                    Duration::ZERO,
                    json!({ "deferred": true, "applied": false }),
                    true,
                    None,
                );
                observe_phase(
                    "mutation.coordination.syncPublishedPlans",
                    Duration::ZERO,
                    json!({ "mode": "noop" }),
                    true,
                    None,
                );
            }
            return Ok(result);
        }
        let publish_context = publish_context_from_coordination_events(appended_events);
        let derived = CoordinationDerivedSyncInputs {
            plan_graphs: snapshot_plan_graphs(snapshot),
        };
        if shared_coordination_ref_publish_enabled(root) {
            sync_authoritative_shared_coordination_ref_observed(
                root,
                snapshot,
                publish_context.as_ref(),
                &mut observe_phase,
            )?;
        } else {
            observe_phase(
                "mutation.coordination.publishedPlans.syncSharedCoordinationRef",
                Duration::ZERO,
                json!({
                    "skipped": true,
                    "reason": "test_default_disabled",
                }),
                true,
                None,
            );
        }
        if matches!(
            derived_persistence_mode,
            CoordinationDerivedPersistenceMode::Inline
        ) {
            sync_inline_coordination_projections_observed(
                self,
                root,
                result.revision,
                snapshot,
                &derived,
                publish_context.as_ref(),
                &mut observe_phase,
            )?;
        } else {
            observe_phase(
                "mutation.coordination.publishedPlans.syncTrackedSnapshot",
                Duration::ZERO,
                json!({ "deferred": true }),
                true,
                None,
            );
            observe_phase(
                "mutation.coordination.publishedPlans.saveStartupCheckpoint",
                Duration::ZERO,
                json!({ "deferred": true }),
                true,
                None,
            );
            observe_phase(
                "mutation.coordination.syncPublishedPlans",
                Duration::ZERO,
                json!({ "mode": "shared_ref_only" }),
                true,
                None,
            );
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
    ) -> Result<CoordinationPersistResult> {
        self.persist_coordination_mutation_state_for_root_with_session_observed(
            root,
            expected_revision,
            snapshot,
            appended_events,
            session_id,
            previous_snapshot,
            CoordinationDerivedPersistenceMode::Inline,
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
        derived_persistence_mode: CoordinationDerivedPersistenceMode,
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
                derived_persistence_mode,
                &mut observe_phase,
            )?;
        persist_coordination_read_models_and_compaction_observed(
            self,
            result.revision,
            snapshot,
            appended_events,
            existing_read_model,
            existing_queue_read_model,
            &mut observe_phase,
            result.applied,
        )?;
        Ok(result)
    }
}

pub(crate) fn repo_semantic_coordination_snapshot(
    mut snapshot: CoordinationSnapshot,
) -> CoordinationSnapshot {
    for task in &mut snapshot.tasks {
        task.pending_handoff_to = None;
        task.session = None;
        task.worktree_id = None;
        task.branch_ref = None;
        task.git_execution = TaskGitExecution::default();
    }
    sanitize_persisted_coordination_snapshot(snapshot)
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

pub(crate) fn coordination_event_delta(
    existing_events: &[CoordinationEvent],
    next_events: &[CoordinationEvent],
) -> Vec<CoordinationEvent> {
    if existing_events.is_empty() {
        return next_events.to_vec();
    }
    if existing_events.len() <= next_events.len()
        && existing_events
            .iter()
            .zip(next_events.iter())
            .all(|(stored, next)| stored.meta.id == next.meta.id)
    {
        return next_events[existing_events.len()..].to_vec();
    }

    let existing_ids = existing_events
        .iter()
        .map(|event| event.meta.id.0.as_str())
        .collect::<HashSet<_>>();
    next_events
        .iter()
        .filter(|event| !existing_ids.contains(event.meta.id.0.as_str()))
        .cloned()
        .collect()
}
