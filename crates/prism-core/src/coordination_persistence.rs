use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_seed, coordination_read_model_from_seed, CoordinationEvent,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2, TaskGitExecution,
};
use prism_ir::SessionId;
#[cfg(test)]
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationPersistBatch,
    CoordinationPersistResult,
};
#[cfg(not(test))]
use prism_store::{CoordinationCheckpointStore, CoordinationJournal, CoordinationPersistResult};
use serde_json::{json, Value};

use crate::coordination_authority_store::{
    configured_coordination_authority_store_provider,
    coordination_materialization_enabled_for_root, CoordinationAppendRequest,
    CoordinationCommitReceipt, CoordinationTransactionBase, CoordinationTransactionResult,
    CoordinationTransactionStatus,
};
use crate::coordination_materialized_store::{
    CoordinationMaterializedStore, SqliteCoordinationMaterializedStore,
};
use crate::coordination_materialized_store::{
    CoordinationReadModelsWriteRequest, CoordinationStartupCheckpointWriteRequest,
};
use crate::coordination_mutation_error::CoordinationAuthorityMutationError;
#[cfg(test)]
use crate::coordination_reads::{
    load_eventual_coordination_plan_state_for_root as load_eventual_plan_state_for_root,
    load_eventual_coordination_snapshot_for_root as load_eventual_snapshot_for_root,
    load_eventual_coordination_snapshot_v2_for_root as load_eventual_snapshot_v2_for_root,
};
#[cfg(test)]
use crate::published_plans::{sync_repo_published_plans, HydratedCoordinationPlanState};
use crate::tracked_snapshot::{
    publish_context_from_coordination_events, sync_coordination_snapshot_state,
};
#[cfg(test)]
use crate::workspace_identity::coordination_persist_context_for_root;
const COORDINATION_COMPACTION_SUFFIX_THRESHOLD: usize = 128;
const TEST_COORDINATION_AUTHORITY_PUBLICATION_OPT_IN: &str =
    "enable_coordination_authority_publication";
const LEGACY_TEST_SHARED_COORDINATION_REF_PUBLISH_OPT_IN: &str =
    "enable_shared_coordination_ref_publish";

fn coordination_authority_publication_enabled(root: &Path) -> bool {
    let disabled = env::var_os("PRISM_TEST_DISABLE_COORDINATION_AUTHORITY_PUBLICATION")
        .or_else(|| env::var_os("PRISM_TEST_DISABLE_SHARED_COORDINATION_REF_PUBLISH"))
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false);
    if !disabled {
        return true;
    }
    let tests_dir = root.join(".prism").join("tests");
    tests_dir
        .join(TEST_COORDINATION_AUTHORITY_PUBLICATION_OPT_IN)
        .exists()
        || tests_dir
            .join(LEGACY_TEST_SHARED_COORDINATION_REF_PUBLISH_OPT_IN)
            .exists()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoordinationDerivedPersistenceMode {
    Inline,
    Deferred,
}

#[derive(Clone)]
struct CoordinationDerivedSyncInputs {
    canonical_snapshot_v2: CoordinationSnapshotV2,
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

fn apply_coordination_authority_transaction_observed<O>(
    root: &Path,
    _snapshot: &CoordinationSnapshot,
    _derived: &CoordinationDerivedSyncInputs,
    appended_events: &[CoordinationEvent],
    session_id: Option<&SessionId>,
    _derived_persistence_mode: CoordinationDerivedPersistenceMode,
    observe_phase: &mut O,
) -> Result<()>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let authority_store =
        configured_coordination_authority_store_provider(root)?.open_mutation(root)?;
    let request = CoordinationAppendRequest {
        base: CoordinationTransactionBase::LatestStrong,
        session_id: session_id.cloned(),
        appended_events: appended_events.to_vec(),
    };
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.authority.applyTransaction",
        |result: &crate::coordination_authority_store::CoordinationTransactionResult| {
            json!({
                "committed": result.committed,
                "status": format!("{:?}", result.status),
                "appendedEventCount": request.appended_events.len(),
            })
        },
        || match authority_store.append_events(request.clone())? {
            result if matches!(result.status, CoordinationTransactionStatus::Committed) => {
                Ok::<_, anyhow::Error>(result)
            }
            result => Err(authority_transaction_error(
                &result,
                "coordination authority transaction did not commit successfully",
            )
            .into()),
        },
    )?;
    Ok(())
}

fn persist_authority_transaction_observed<O>(
    root: &Path,
    _snapshot: &CoordinationSnapshot,
    _derived: &CoordinationDerivedSyncInputs,
    appended_events: &[CoordinationEvent],
    session_id: Option<&SessionId>,
    _derived_persistence_mode: CoordinationDerivedPersistenceMode,
    observe_phase: &mut O,
) -> Result<CoordinationTransactionResult>
where
    O: FnMut(&str, Duration, Value, bool, Option<String>),
{
    let authority_store =
        configured_coordination_authority_store_provider(root)?.open_mutation(root)?;
    let request = CoordinationAppendRequest {
        base: CoordinationTransactionBase::LatestStrong,
        session_id: session_id.cloned(),
        appended_events: appended_events.to_vec(),
    };
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.authority.applyTransaction",
        |result: &crate::coordination_authority_store::CoordinationTransactionResult| {
            json!({
                "committed": result.committed,
                "status": format!("{:?}", result.status),
                "appendedEventCount": request.appended_events.len(),
            })
        },
        || authority_store.append_events(request.clone()),
    )
}

fn authority_transaction_error(
    result: &CoordinationTransactionResult,
    fallback_message: &str,
) -> CoordinationAuthorityMutationError {
    let diagnostic = result.diagnostics.first();
    let diagnostic_code = diagnostic
        .map(|value| value.code.clone())
        .unwrap_or_else(|| "authority_transaction_failed".to_string());
    let diagnostic_message = diagnostic
        .map(|value| value.message.clone())
        .unwrap_or_else(|| fallback_message.to_string());
    match result.status {
        CoordinationTransactionStatus::Conflict => CoordinationAuthorityMutationError::conflict(
            "authority_transaction_conflict",
            result
                .conflict
                .as_ref()
                .map(|value| value.reason.clone())
                .unwrap_or(diagnostic_message.clone()),
            result.authority.clone(),
        ),
        CoordinationTransactionStatus::Rejected => CoordinationAuthorityMutationError::rejected(
            diagnostic_code,
            diagnostic_message,
            result.authority.clone(),
        ),
        CoordinationTransactionStatus::Indeterminate => {
            CoordinationAuthorityMutationError::indeterminate(
                diagnostic_code,
                diagnostic_message,
                result.authority.clone(),
            )
        }
        CoordinationTransactionStatus::Committed => CoordinationAuthorityMutationError::rejected(
            "authority_transaction_failed",
            fallback_message,
            result.authority.clone(),
        ),
    }
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
    observe_coordination_step(
        observe_phase,
        "mutation.coordination.publishedPlans.syncTrackedSnapshot",
        |_| json!({}),
        || {
            sync_coordination_snapshot_state(
                root,
                &repo_semantic_snapshot,
                publish_context,
                Some(authoritative_revision),
            )
        },
    )?;
    if coordination_materialization_enabled_for_root(root)? {
        observe_coordination_step(
            observe_phase,
            "mutation.coordination.publishedPlans.saveStartupCheckpoint",
            |_| json!({}),
            || {
                let _ = store;
                SqliteCoordinationMaterializedStore::new(root).write_startup_checkpoint(
                    CoordinationStartupCheckpointWriteRequest {
                        snapshot: repo_semantic_snapshot.clone(),
                        canonical_snapshot_v2: derived.canonical_snapshot_v2.clone(),
                        runtime_descriptors: Vec::new(),
                    },
                )?;
                Ok::<(), anyhow::Error>(())
            },
        )?;
    } else {
        observe_phase(
            "mutation.coordination.publishedPlans.saveStartupCheckpoint",
            Duration::ZERO,
            json!({ "skipped": true, "materialization": "disabled" }),
            true,
            None,
        );
    }
    observe_phase(
        "mutation.coordination.syncDerivedState",
        Duration::ZERO,
        json!({ "mode": "state" }),
        true,
        None,
    );
    Ok(())
}

fn persist_coordination_read_models_and_compaction_observed<S, O>(
    store: &mut S,
    root: &Path,
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
    if !coordination_materialization_enabled_for_root(root)? {
        observe_phase(
            "mutation.coordination.buildReadModel",
            Duration::ZERO,
            json!({ "skipped": true, "materialization": "disabled" }),
            true,
            None,
        );
        observe_phase(
            "mutation.coordination.buildQueueReadModel",
            Duration::ZERO,
            json!({ "skipped": true, "materialization": "disabled" }),
            true,
            None,
        );
        observe_phase(
            "mutation.coordination.saveReadModels",
            Duration::ZERO,
            json!({ "skipped": true, "materialization": "disabled" }),
            true,
            None,
        );
        observe_phase(
            "mutation.coordination.compactEvents",
            Duration::ZERO,
            json!({ "skipped": true, "materialization": "disabled" }),
            true,
            None,
        );
        return Ok(());
    }
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
        "mutation.coordination.saveReadModels",
        |_| {
            json!({
                "eventCount": snapshot.events.len(),
                "taskCount": snapshot.tasks.len(),
            })
        },
        || {
            let _ = store;
            SqliteCoordinationMaterializedStore::new(root).write_read_models(
                CoordinationReadModelsWriteRequest {
                    read_model: read_model.clone(),
                    queue_read_model: queue_read_model.clone(),
                },
            )?;
            Ok::<(), anyhow::Error>(())
        },
    )?;
    if applied {
        observe_coordination_step(
            observe_phase,
            "mutation.coordination.compactEvents",
            |_| json!({ "applied": true }),
            || {
                let _ = store;
                SqliteCoordinationMaterializedStore::new(root).write_compaction(
                    crate::CoordinationCompactionWriteRequest {
                        snapshot: snapshot.clone(),
                    },
                )?;
                Ok::<(), anyhow::Error>(())
            },
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
    #[cfg(test)]
    fn persist_coordination_authoritative_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
        canonical_snapshot_v2: &CoordinationSnapshotV2,
    ) -> Result<CoordinationPersistResult> {
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let result = self.commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_persist_context_for_root(root, None),
            expected_revision: None,
            appended_events,
        })?;
        sync_repo_published_plans(root, snapshot, canonical_snapshot_v2, None)?;
        SqliteCoordinationMaterializedStore::new(root).write_startup_checkpoint(
            CoordinationStartupCheckpointWriteRequest {
                snapshot: snapshot.clone(),
                canonical_snapshot_v2: canonical_snapshot_v2.clone(),
                runtime_descriptors: Vec::new(),
            },
        )?;
        Ok(result)
    }

    #[cfg(test)]
    fn load_eventual_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshot>> {
        let _ = self;
        load_eventual_snapshot_for_root(root)
    }

    #[cfg(test)]
    fn load_eventual_coordination_snapshot_v2_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshotV2>> {
        let _ = self;
        load_eventual_snapshot_v2_for_root(root)
    }

    #[cfg(test)]
    fn load_eventual_coordination_plan_state_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<HydratedCoordinationPlanState>> {
        let _ = self;
        load_eventual_plan_state_for_root(root)
    }

    #[cfg(test)]
    fn persist_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        self.persist_coordination_state_for_root(
            root,
            snapshot,
            &snapshot.to_canonical_snapshot_v2(),
        )
    }

    #[cfg(test)]
    fn persist_coordination_state_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
        canonical_snapshot_v2: &CoordinationSnapshotV2,
    ) -> Result<()> {
        let existing_read_model = self.load_coordination_read_model()?;
        let existing_queue_read_model = self.load_coordination_queue_read_model()?;
        let existing_events = self.load_coordination_events()?;
        let appended_events = coordination_event_delta(&existing_events, &snapshot.events);
        let _ = self.persist_coordination_authoritative_state_for_root(
            root,
            snapshot,
            canonical_snapshot_v2,
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
        SqliteCoordinationMaterializedStore::new(root).write_read_models(
            CoordinationReadModelsWriteRequest {
                read_model,
                queue_read_model,
            },
        )?;
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
        canonical_snapshot_v2: &CoordinationSnapshotV2,
        derived_persistence_mode: CoordinationDerivedPersistenceMode,
        mut observe_phase: O,
    ) -> Result<CoordinationPersistResult>
    where
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        let publish_context = publish_context_from_coordination_events(appended_events);
        let derived = CoordinationDerivedSyncInputs {
            canonical_snapshot_v2: canonical_snapshot_v2.clone(),
        };
        let _ = coordination_authority_publication_enabled(root);
        let transaction = persist_authority_transaction_observed(
            root,
            snapshot,
            &derived,
            appended_events,
            session_id,
            derived_persistence_mode,
            &mut observe_phase,
        )?;
        let result = match transaction.status {
            CoordinationTransactionStatus::Committed => {
                transaction.commit.unwrap_or(CoordinationCommitReceipt {
                    revision: expected_revision.saturating_add(1),
                    inserted_events: appended_events.len(),
                    applied: true,
                })
            }
            _ => {
                return Err(authority_transaction_error(
                    &transaction,
                    "coordination authority transaction did not commit successfully",
                )
                .into())
            }
        };
        let result = CoordinationPersistResult {
            revision: result.revision,
            inserted_events: result.inserted_events,
            applied: result.applied,
        };
        if !result.applied {
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
                    "mutation.coordination.syncDerivedState",
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
                    "mutation.coordination.syncDerivedState",
                    Duration::ZERO,
                    json!({ "mode": "noop" }),
                    true,
                    None,
                );
            }
            return Ok(result);
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
                "mutation.coordination.syncDerivedState",
                Duration::ZERO,
                json!({ "mode": "authority_only" }),
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
        canonical_snapshot_v2: &CoordinationSnapshotV2,
    ) -> Result<CoordinationPersistResult> {
        self.persist_coordination_mutation_state_for_root_with_session_observed(
            root,
            expected_revision,
            snapshot,
            appended_events,
            session_id,
            previous_snapshot,
            canonical_snapshot_v2,
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
        canonical_snapshot_v2: &CoordinationSnapshotV2,
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
                canonical_snapshot_v2,
                derived_persistence_mode,
                &mut observe_phase,
            )?;
        persist_coordination_read_models_and_compaction_observed(
            self,
            root,
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
    snapshot
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
