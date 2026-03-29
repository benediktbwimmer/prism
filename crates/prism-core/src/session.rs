use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use anyhow::{anyhow, Result};
use prism_agent::InferenceSnapshot;
use prism_coordination::CoordinationSnapshot;
use prism_coordination::CoordinationReadModel;
use prism_curator::{
    CuratorJobId, CuratorProposalDisposition, CuratorProposalState, CuratorSnapshot,
};
use prism_history::HistoryStore;
use prism_ir::{
    ChangeTrigger, EventId, ObservedChangeSet, PlanExecutionOverlay, PlanGraph, SessionId, TaskId,
};
use prism_memory::OutcomeMemory;
use prism_memory::{EpisodicMemorySnapshot, MemoryEvent, MemoryEventQuery, OutcomeEvent};
use prism_projections::{
    concept_from_event, validation_deltas_for_event, ConceptEvent, ConceptRelationEvent,
    ConceptRelationEventAction,
};
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, SqliteStore, Store};

pub use prism_store::SnapshotRevisions as WorkspaceSnapshotRevisions;

use crate::concept_events::{append_repo_concept_event, load_repo_curated_concepts};
use crate::concept_relation_events::{
    append_repo_concept_relation_event, load_repo_concept_relations,
};
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator::{enqueue_curator_for_outcome_locked, CuratorHandle, CuratorHandleRef};
use crate::memory_events::{
    append_repo_memory_event, filter_memory_events, load_repo_memory_events,
};
use crate::published_knowledge::{
    validate_repo_concept_event, validate_repo_concept_relation_event, validate_repo_memory_event,
};
use crate::shared_runtime::{
    composite_workspace_revision, local_projection_snapshot_for_persist, merge_episodic_snapshots,
    merge_memory_events, merged_projection_index, shared_projection_snapshot_for_persist,
    split_episodic_snapshot_for_persist,
};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::util::{
    current_timestamp, current_timestamp_millis, workspace_fingerprint, WorkspaceFingerprint,
};
use crate::validation_feedback::{
    append_validation_feedback, load_validation_feedback, ValidationFeedbackEntry,
    ValidationFeedbackRecord,
};
use crate::watch::{refresh_prism_snapshot, try_refresh_prism_snapshot, WatchHandle};
use crate::workspace_identity::coordination_persist_context_for_root;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsRefreshStatus {
    Clean,
    Refreshed,
    DeferredBusy,
}

#[derive(Debug, Clone)]
pub struct CoordinationPlanState {
    pub snapshot: CoordinationSnapshot,
    pub plan_graphs: Vec<PlanGraph>,
    pub execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

pub(crate) struct WorkspaceRefreshState {
    observed_fs_revision: AtomicU64,
    applied_fs_revision: AtomicU64,
    last_fallback_check_ms: AtomicU64,
    dirty_paths: Mutex<HashMap<PathBuf, u64>>,
}

const FALLBACK_FINGERPRINT_INTERVAL_MS: u64 = 250;

impl WorkspaceRefreshState {
    pub(crate) fn new() -> Self {
        Self {
            observed_fs_revision: AtomicU64::new(0),
            applied_fs_revision: AtomicU64::new(0),
            last_fallback_check_ms: AtomicU64::new(0),
            dirty_paths: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn mark_fs_dirty_paths<I>(&self, paths: I) -> u64
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let revision = self.observed_fs_revision.fetch_add(1, Ordering::Relaxed) + 1;
        let mut dirty_paths = self
            .dirty_paths
            .lock()
            .expect("workspace dirty paths lock poisoned");
        for path in paths {
            dirty_paths.insert(path, revision);
        }
        revision
    }

    pub(crate) fn dirty_paths_snapshot(&self) -> Vec<PathBuf> {
        self.dirty_paths
            .lock()
            .expect("workspace dirty paths lock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    pub(crate) fn mark_refreshed_revision(&self, revision: u64, consumed_paths: &[PathBuf]) {
        self.applied_fs_revision.store(revision, Ordering::Relaxed);
        if consumed_paths.is_empty() {
            return;
        }
        let mut dirty_paths = self
            .dirty_paths
            .lock()
            .expect("workspace dirty paths lock poisoned");
        for path in consumed_paths {
            let should_remove = dirty_paths
                .get(path)
                .is_some_and(|path_revision| *path_revision <= revision);
            if should_remove {
                dirty_paths.remove(path);
            }
        }
    }

    pub(crate) fn needs_refresh(&self) -> bool {
        self.observed_fs_revision.load(Ordering::Relaxed)
            != self.applied_fs_revision.load(Ordering::Relaxed)
    }

    pub(crate) fn observed_fs_revision(&self) -> u64 {
        self.observed_fs_revision.load(Ordering::Relaxed)
    }

    pub(crate) fn applied_fs_revision(&self) -> u64 {
        self.applied_fs_revision.load(Ordering::Relaxed)
    }

    pub(crate) fn should_run_fallback_check(&self, now_ms: u64) -> bool {
        loop {
            let last = self.last_fallback_check_ms.load(Ordering::Relaxed);
            if now_ms.saturating_sub(last) < FALLBACK_FINGERPRINT_INTERVAL_MS {
                return false;
            }
            if self
                .last_fallback_check_ms
                .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }
}

pub struct WorkspaceSession {
    pub(crate) root: PathBuf,
    pub(crate) prism: Arc<RwLock<Arc<Prism>>>,
    pub(crate) store: Arc<Mutex<SqliteStore>>,
    pub(crate) shared_runtime: SharedRuntimeBackend,
    pub(crate) shared_runtime_store: Option<Arc<Mutex<SqliteStore>>>,
    pub(crate) refresh_lock: Arc<Mutex<()>>,
    pub(crate) refresh_state: Arc<WorkspaceRefreshState>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) fs_snapshot: Arc<Mutex<WorkspaceFingerprint>>,
    pub(crate) watch: Option<WatchHandle>,
    pub(crate) curator: Option<CuratorHandle>,
    pub(crate) coordination_enabled: bool,
}

impl WorkspaceSession {
    fn shared_runtime_store(&self) -> Option<&Arc<Mutex<SqliteStore>>> {
        self.shared_runtime_store.as_ref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn prism(&self) -> Arc<Prism> {
        self.prism_arc()
    }

    pub fn prism_arc(&self) -> Arc<Prism> {
        self.prism
            .read()
            .expect("workspace prism lock poisoned")
            .clone()
    }

    pub fn refresh_fs(&self) -> Result<Vec<ObservedChangeSet>> {
        if !self.refresh_state.needs_refresh()
            && !self
                .refresh_state
                .should_run_fallback_check(current_timestamp_millis())
        {
            return Ok(Vec::new());
        }
        let known_snapshot = self
            .fs_snapshot
            .lock()
            .expect("workspace fingerprint lock poisoned")
            .clone();
        let current_fingerprint = workspace_fingerprint(&self.root, Some(&known_snapshot))?;
        if !self.refresh_state.needs_refresh() && current_fingerprint.value == known_snapshot.value
        {
            return Ok(Vec::new());
        }
        self.refresh_with_trigger(ChangeTrigger::FsWatch, Some(current_fingerprint))
    }

    pub fn refresh_fs_nonblocking(&self) -> Result<FsRefreshStatus> {
        if !self.refresh_state.needs_refresh()
            && !self
                .refresh_state
                .should_run_fallback_check(current_timestamp_millis())
        {
            return Ok(FsRefreshStatus::Clean);
        }
        let known_snapshot = self
            .fs_snapshot
            .lock()
            .expect("workspace fingerprint lock poisoned")
            .clone();
        let current_fingerprint = workspace_fingerprint(&self.root, Some(&known_snapshot))?;
        if !self.refresh_state.needs_refresh() && current_fingerprint.value == known_snapshot.value
        {
            return Ok(FsRefreshStatus::Clean);
        }
        let refreshed =
            self.try_refresh_with_trigger(ChangeTrigger::FsWatch, Some(current_fingerprint))?;
        if refreshed {
            Ok(FsRefreshStatus::Refreshed)
        } else {
            Ok(FsRefreshStatus::DeferredBusy)
        }
    }

    pub fn needs_refresh(&self) -> bool {
        self.refresh_state.needs_refresh()
    }

    pub fn observed_fs_revision(&self) -> u64 {
        self.refresh_state.observed_fs_revision()
    }

    pub fn applied_fs_revision(&self) -> u64 {
        self.refresh_state.applied_fs_revision()
    }

    pub fn persist_outcomes(&self) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(prism.outcome_snapshot()),
            ..AuxiliaryPersistBatch::default()
        })
    }

    pub fn persist_history(&self) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.save_history_snapshot(&prism.history_snapshot())
    }

    pub fn load_episodic_snapshot(&self) -> Result<Option<EpisodicMemorySnapshot>> {
        let local_snapshot = {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            if self.shared_runtime_store().is_none() {
                self.sync_repo_memory_events_locked(&mut store)?;
            }
            store.load_episodic_snapshot()?
        };
        let shared_snapshot = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            self.sync_repo_memory_events_locked(&mut store)?;
            store.load_episodic_snapshot()?
        } else {
            None
        };
        Ok(merge_episodic_snapshots(local_snapshot, shared_snapshot))
    }

    pub fn reload_persisted_prism(&self) -> Result<()> {
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.reload_persisted_prism_with_guard(guard)
    }

    pub fn try_reload_persisted_prism(&self) -> Result<bool> {
        let Ok(guard) = self.refresh_lock.try_lock() else {
            return Ok(false);
        };
        self.reload_persisted_prism_with_guard(guard)?;
        Ok(true)
    }

    fn reload_persisted_prism_with_guard(&self, _guard: MutexGuard<'_, ()>) -> Result<()> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let local_workspace_revision = store.workspace_revision()?;
        let shared_workspace_revision = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            self.sync_repo_memory_events_locked(&mut shared_store)?;
            Some(shared_store.workspace_revision()?)
        } else {
            None
        };
        let workspace_revision =
            composite_workspace_revision(local_workspace_revision, shared_workspace_revision);
        let graph = store.load_graph()?.unwrap_or_default();
        let mut history = store
            .load_history_snapshot()?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let outcomes = store
            .load_outcome_snapshot()?
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
        let plan_state = if self.coordination_enabled {
            if let Some(shared_runtime_store) = self.shared_runtime_store() {
                shared_runtime_store
                    .lock()
                    .expect("shared runtime store lock poisoned")
                    .load_hydrated_coordination_plan_state_for_root(&self.root)?
            } else {
                store.load_hydrated_coordination_plan_state_for_root(&self.root)?
            }
        } else {
            None
        };
        let coordination_snapshot = plan_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let projections = merged_projection_index(
            store.load_projection_snapshot()?,
            if let Some(shared_runtime_store) = self.shared_runtime_store() {
                shared_runtime_store
                    .lock()
                    .expect("shared runtime store lock poisoned")
                    .load_projection_snapshot()?
            } else {
                None
            },
            load_repo_curated_concepts(&self.root)?,
            load_repo_concept_relations(&self.root)?,
            &history.snapshot(),
            &outcomes.snapshot(),
        );
        drop(store);

        let prism = Arc::new(
            Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
                graph,
                history,
                outcomes,
                coordination_snapshot,
                projections,
                plan_state
                    .as_ref()
                    .map(|state| state.plan_graphs.clone())
                    .unwrap_or_default(),
                plan_state
                    .map(|state| state.execution_overlays)
                    .unwrap_or_default(),
                ),
        );
        prism.set_workspace_revision(prism_ir::WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        });
        prism.set_coordination_context(Some(coordination_persist_context_for_root(
            &self.root, None,
        )));
        *self.prism.write().expect("workspace prism lock poisoned") = prism;
        self.loaded_workspace_revision
            .store(workspace_revision, Ordering::Relaxed);
        Ok(())
    }

    pub fn workspace_revision(&self) -> Result<u64> {
        let local_revision = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .workspace_revision()?;
        let shared_revision = self
            .shared_runtime_store()
            .map(|store| {
                store
                    .lock()
                    .expect("shared runtime store lock poisoned")
                    .workspace_revision()
            })
            .transpose()?;
        Ok(composite_workspace_revision(local_revision, shared_revision))
    }

    pub fn loaded_workspace_revision(&self) -> u64 {
        self.loaded_workspace_revision.load(Ordering::Relaxed)
    }

    pub fn loaded_workspace_revision_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.loaded_workspace_revision)
    }

    pub fn snapshot_revisions(&self) -> Result<WorkspaceSnapshotRevisions> {
        let mut revisions = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .snapshot_revisions()?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            self.sync_repo_memory_events_locked(&mut shared_store)?;
            let shared_revisions = shared_store.snapshot_revisions()?;
            revisions.workspace =
                composite_workspace_revision(revisions.workspace, Some(shared_revisions.workspace));
            revisions.episodic = revisions.episodic.max(shared_revisions.episodic);
            revisions.coordination = shared_revisions.coordination;
        } else {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            self.sync_repo_memory_events_locked(&mut store)?;
            revisions = store.snapshot_revisions()?;
        }
        if !self.coordination_enabled {
            revisions.coordination = 0;
        }
        Ok(revisions)
    }

    pub fn episodic_revision(&self) -> Result<u64> {
        let local_revision = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .episodic_revision()?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            self.sync_repo_memory_events_locked(&mut store)?;
            Ok(local_revision.max(store.episodic_revision()?))
        } else {
            Ok(local_revision)
        }
    }

    pub fn persist_episodic(&self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let (local_snapshot, shared_snapshot) = split_episodic_snapshot_for_persist(snapshot);
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                    episodic_snapshot: Some(local_snapshot),
                    ..AuxiliaryPersistBatch::default()
                })?;
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                    episodic_snapshot: Some(shared_snapshot),
                    ..AuxiliaryPersistBatch::default()
                })?;
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                    episodic_snapshot: Some(snapshot.clone()),
                    ..AuxiliaryPersistBatch::default()
                })?;
        }
        Ok(())
    }

    pub fn append_memory_event(&self, event: MemoryEvent) -> Result<()> {
        if event.scope == prism_memory::MemoryScope::Repo {
            validate_repo_memory_event(&event)?;
            append_repo_memory_event(&self.root, &event)?;
        }
        match (event.scope, self.shared_runtime_store()) {
            (prism_memory::MemoryScope::Local, _) => {
                self.store
                    .lock()
                    .expect("workspace store lock poisoned")
                    .append_memory_events(&[event])?;
            }
            (_, Some(shared_runtime_store)) => {
                shared_runtime_store
                    .lock()
                    .expect("shared runtime store lock poisoned")
                    .append_memory_events(&[event])?;
            }
            (_, None) => {
                self.store
                    .lock()
                    .expect("workspace store lock poisoned")
                    .append_memory_events(&[event])?;
            }
        }
        Ok(())
    }

    pub fn memory_events(&self, query: &MemoryEventQuery) -> Result<Vec<MemoryEvent>> {
        let local_events = {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            if self.shared_runtime_store().is_none() {
                self.sync_repo_memory_events_locked(&mut store)?;
            }
            store.load_memory_events()?
        };
        let shared_events = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            self.sync_repo_memory_events_locked(&mut store)?;
            store.load_memory_events()?
        } else {
            Vec::new()
        };
        Ok(filter_memory_events(
            merge_memory_events(local_events, shared_events),
            query,
        ))
    }

    pub fn append_concept_event(&self, event: ConceptEvent) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        if event.concept.scope == prism_projections::ConceptScope::Repo {
            validate_repo_concept_event(&event)?;
            append_repo_concept_event(&self.root, &event)?;
        }
        let prism = self.prism_arc();
        let previous = prism.concept_by_handle(&event.concept.handle);
        let concept = concept_from_event(previous.as_ref(), &event);
        prism.upsert_curated_concept(concept);
        let snapshot = prism.projection_snapshot();
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .save_projection_snapshot(&local_projection_snapshot_for_persist(&snapshot))?;
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .save_projection_snapshot(&shared_projection_snapshot_for_persist(&snapshot))?;
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .save_projection_snapshot(&snapshot)?;
        }
        Ok(())
    }

    pub fn append_concept_relation_event(&self, event: ConceptRelationEvent) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        if event.relation.scope == prism_projections::ConceptScope::Repo {
            validate_repo_concept_relation_event(&event)?;
            append_repo_concept_relation_event(&self.root, &event)?;
        }
        let prism = self.prism_arc();
        match event.action {
            ConceptRelationEventAction::Upsert => prism.upsert_concept_relation(event.relation),
            ConceptRelationEventAction::Retire => prism.remove_concept_relation(
                &event.relation.source_handle,
                &event.relation.target_handle,
                event.relation.kind,
            ),
        }
        let snapshot = prism.projection_snapshot();
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .save_projection_snapshot(&local_projection_snapshot_for_persist(&snapshot))?;
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .save_projection_snapshot(&shared_projection_snapshot_for_persist(&snapshot))?;
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .save_projection_snapshot(&snapshot)?;
        }
        Ok(())
    }

    pub fn load_inference_snapshot(&self) -> Result<Option<InferenceSnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_inference_snapshot()
    }

    pub fn inference_revision(&self) -> Result<u64> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .inference_revision()
    }

    pub fn coordination_revision(&self) -> Result<u64> {
        if !self.coordination_enabled {
            return Ok(0);
        }
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .coordination_revision()
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .coordination_revision()
        }
    }

    pub fn persist_inference(&self, snapshot: &InferenceSnapshot) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                inference_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn load_coordination_snapshot(&self) -> Result<Option<CoordinationSnapshot>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_hydrated_coordination_snapshot_for_root(&self.root)
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .load_hydrated_coordination_snapshot_for_root(&self.root)
        }
    }

    pub fn load_coordination_read_model(&self) -> Result<Option<CoordinationReadModel>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_coordination_read_model()
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .load_coordination_read_model()
        }
    }

    pub fn load_coordination_plan_state(&self) -> Result<Option<CoordinationPlanState>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        let state = if let Some(store) = self.shared_runtime_store() {
            store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_hydrated_coordination_plan_state_for_root(&self.root)?
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .load_hydrated_coordination_plan_state_for_root(&self.root)?
        };
        Ok(state.map(|state| CoordinationPlanState {
            snapshot: state.snapshot,
            plan_graphs: state.plan_graphs,
            execution_overlays: state.execution_overlays,
        }))
    }

    pub fn persist_coordination(&self, snapshot: &CoordinationSnapshot) -> Result<()> {
        if !self.coordination_enabled {
            return Ok(());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .persist_coordination_snapshot_for_root(&self.root, snapshot)
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .persist_coordination_snapshot_for_root(&self.root, snapshot)
        }
    }

    pub fn persist_current_coordination(&self) -> Result<()> {
        if !self.coordination_enabled {
            return Ok(());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let prism = self.prism_arc();
        let snapshot = prism.coordination_snapshot();
        let plan_graphs = prism.plan_graphs();
        let execution_overlays = prism.plan_execution_overlays_by_plan();
        let target = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            Arc::clone(shared_runtime_store)
        } else {
            Arc::clone(&self.store)
        };
        let result = target
            .lock()
            .expect("coordination store lock poisoned")
            .persist_coordination_state_for_root(
                &self.root,
                &snapshot,
                Some(&plan_graphs),
                Some(&execution_overlays),
            );
        result
    }

    pub fn mutate_coordination<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
    {
        self.mutate_coordination_with_session(None, mutate)
    }

    pub fn mutate_coordination_with_session<T, F>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
    ) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
    {
        if !self.coordination_enabled {
            return Err(anyhow!(
                "coordination is disabled for this workspace session"
            ));
        }
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let expected_revision = self.coordination_revision()?;
        let prism = self.prism_arc();
        let before = prism.coordination_snapshot();
        let result = mutate(prism.as_ref())?;
        let snapshot = prism.coordination_snapshot();
        let appended_events = snapshot
            .events
            .iter()
            .filter(|event| {
                !before
                    .events
                    .iter()
                    .any(|stored| stored.meta.id == event.meta.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let plan_graphs = prism.plan_graphs();
        let execution_overlays = prism.plan_execution_overlays_by_plan();
        let target = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            Arc::clone(shared_runtime_store)
        } else {
            Arc::clone(&self.store)
        };
        let mut store = target.lock().expect("coordination store lock poisoned");
        if let Some(session_id) = session_id {
            store.persist_coordination_mutation_state_for_root_with_session(
                &self.root,
                expected_revision,
                &snapshot,
                &appended_events,
                Some(session_id),
                Some(&plan_graphs),
                Some(&execution_overlays),
            )?;
        } else {
            store.persist_coordination_mutation_state_for_root_with_session(
                &self.root,
                expected_revision,
                &snapshot,
                &appended_events,
                None,
                Some(&plan_graphs),
                Some(&execution_overlays),
            )?;
        }
        Ok(result)
    }

    pub fn curator_snapshot(&self) -> CuratorSnapshot {
        self.curator
            .as_ref()
            .map(CuratorHandle::snapshot)
            .unwrap_or_default()
    }

    pub fn set_curator_proposal_state(
        &self,
        job_id: &CuratorJobId,
        proposal_index: usize,
        disposition: CuratorProposalDisposition,
        task: Option<TaskId>,
        note: Option<String>,
        output: Option<String>,
    ) -> Result<()> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let Some(curator) = &self.curator else {
            return Ok(());
        };
        let mut state = curator.state.lock().expect("curator state lock poisoned");
        let record = state
            .snapshot
            .records
            .iter_mut()
            .find(|record| &record.id == job_id)
            .ok_or_else(|| anyhow::anyhow!("unknown curator job `{}`", job_id.0))?;
        if record.proposal_states.len() <= proposal_index {
            if let Some(run) = &record.run {
                record
                    .proposal_states
                    .resize(run.proposals.len(), CuratorProposalState::default());
            }
        }
        let proposal_state = record
            .proposal_states
            .get_mut(proposal_index)
            .ok_or_else(|| anyhow::anyhow!("unknown curator proposal index {proposal_index}"))?;
        proposal_state.disposition = disposition;
        proposal_state.decided_at = Some(current_timestamp());
        proposal_state.task = task;
        proposal_state.note = note;
        proposal_state.output = output;
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            curator_snapshot: Some(state.snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        })?;
        Ok(())
    }

    pub fn append_outcome(&self, event: OutcomeEvent) -> Result<EventId> {
        self.append_outcome_with_auxiliary(event, None, None)
    }

    pub fn try_append_outcome(&self, event: OutcomeEvent) -> Result<Option<EventId>> {
        self.try_append_outcome_with_auxiliary(event, None, None)
    }

    pub fn append_validation_feedback(
        &self,
        record: ValidationFeedbackRecord,
    ) -> Result<ValidationFeedbackEntry> {
        append_validation_feedback(&self.root, record)
    }

    pub fn validation_feedback(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ValidationFeedbackEntry>> {
        let mut entries = load_validation_feedback(&self.root)?;
        entries.reverse();
        if let Some(limit) = limit {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    pub fn append_outcome_with_auxiliary(
        &self,
        event: OutcomeEvent,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<EventId> {
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.append_outcome_with_auxiliary_guarded(event, episodic_snapshot, inference_snapshot)
    }

    pub fn try_append_outcome_with_auxiliary(
        &self,
        event: OutcomeEvent,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<Option<EventId>> {
        let Ok(_guard) = self.refresh_lock.try_lock() else {
            return Ok(None);
        };
        self.append_outcome_with_auxiliary_guarded(event, episodic_snapshot, inference_snapshot)
            .map(Some)
    }

    fn append_outcome_with_auxiliary_guarded(
        &self,
        event: OutcomeEvent,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<EventId> {
        let prism = self.prism_arc();
        let deltas = validation_deltas_for_event(&event, |node| prism.lineage_of(node));
        prism.apply_outcome_event_to_projections(&event);
        let id = prism.outcome_memory().store_event(event)?;
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(prism.outcome_snapshot()),
            validation_deltas: deltas,
            episodic_snapshot,
            inference_snapshot,
            curator_snapshot: None,
        })?;
        if let Some(curator) = &self.curator {
            enqueue_curator_for_outcome_locked(curator, prism.as_ref(), &mut store, id.clone())?;
        }
        Ok(id)
    }

    fn refresh_with_trigger(
        &self,
        trigger: ChangeTrigger,
        known_fingerprint: Option<WorkspaceFingerprint>,
    ) -> Result<Vec<ObservedChangeSet>> {
        let curator = self.curator.as_ref().map(CuratorHandleRef::from);
        let observed = refresh_prism_snapshot(
            &self.root,
            &self.prism,
            &self.store,
            self.shared_runtime.sqlite_path(),
            &self.refresh_lock,
            &self.refresh_state,
            &self.loaded_workspace_revision,
            &self.fs_snapshot,
            self.coordination_enabled,
            curator.as_ref(),
            trigger,
            known_fingerprint,
        )?;
        Ok(observed)
    }

    fn try_refresh_with_trigger(
        &self,
        trigger: ChangeTrigger,
        known_fingerprint: Option<WorkspaceFingerprint>,
    ) -> Result<bool> {
        let observed = try_refresh_prism_snapshot(
            &self.root,
            &self.prism,
            &self.store,
            self.shared_runtime.sqlite_path(),
            &self.refresh_lock,
            &self.refresh_state,
            &self.loaded_workspace_revision,
            &self.fs_snapshot,
            self.coordination_enabled,
            self.curator.as_ref().map(CuratorHandleRef::from).as_ref(),
            trigger,
            known_fingerprint,
        )?;
        Ok(observed.is_some())
    }

    fn sync_repo_memory_events_locked(&self, store: &mut SqliteStore) -> Result<bool> {
        let events = load_repo_memory_events(&self.root)?;
        if events.is_empty() {
            return Ok(false);
        }
        Ok(store.append_memory_events(&events)? > 0)
    }
}

impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            let _ = watch.stop.send(());
            let _ = watch.handle.join();
        }
        if let Some(mut curator) = self.curator.take() {
            curator.stop();
        }
    }
}
