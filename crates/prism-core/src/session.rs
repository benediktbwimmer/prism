use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use anyhow::{anyhow, Result};
use prism_agent::InferenceSnapshot;
use prism_coordination::CoordinationSnapshot;
use prism_coordination::CoordinationStore;
use prism_curator::{
    CuratorJobId, CuratorProposalDisposition, CuratorProposalState, CuratorSnapshot,
};
use prism_history::HistoryStore;
use prism_ir::{ChangeTrigger, EventId, ObservedChangeSet, TaskId};
use prism_memory::OutcomeMemory;
use prism_memory::{EpisodicMemorySnapshot, MemoryEvent, MemoryEventQuery, OutcomeEvent};
use prism_projections::validation_deltas_for_event;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, SqliteStore, Store};

pub use prism_store::SnapshotRevisions as WorkspaceSnapshotRevisions;

use crate::curator::{enqueue_curator_for_outcome_locked, CuratorHandle, CuratorHandleRef};
use crate::memory_events::{
    append_repo_memory_event, filter_memory_events, load_repo_memory_events,
};
use crate::util::{
    current_timestamp, current_timestamp_millis, workspace_fingerprint, WorkspaceFingerprint,
};
use crate::validation_feedback::{
    append_validation_feedback, load_validation_feedback, ValidationFeedbackEntry,
    ValidationFeedbackRecord,
};
use crate::watch::{refresh_prism_snapshot, try_refresh_prism_snapshot, WatchHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsRefreshStatus {
    Clean,
    Refreshed,
    DeferredBusy,
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
    pub(crate) refresh_lock: Arc<Mutex<()>>,
    pub(crate) refresh_state: Arc<WorkspaceRefreshState>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) fs_snapshot: Arc<Mutex<WorkspaceFingerprint>>,
    pub(crate) watch: Option<WatchHandle>,
    pub(crate) curator: Option<CuratorHandle>,
    pub(crate) coordination_enabled: bool,
}

impl WorkspaceSession {
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
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        self.sync_repo_memory_events_locked(&mut store)?;
        store.load_episodic_snapshot()
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
        self.sync_repo_memory_events_locked(&mut store)?;
        let workspace_revision = store.workspace_revision()?;
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
        let coordination = if self.coordination_enabled {
            store
                .load_coordination_snapshot()?
                .map(CoordinationStore::from_snapshot)
                .unwrap_or_else(CoordinationStore::new)
        } else {
            CoordinationStore::new()
        };
        let projections = store
            .load_projection_snapshot()?
            .map(ProjectionIndex::from_snapshot)
            .unwrap_or_else(|| ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot()));
        drop(store);

        let prism = Arc::new(Prism::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            coordination,
            projections,
        ));
        *self.prism.write().expect("workspace prism lock poisoned") = prism;
        self.loaded_workspace_revision
            .store(workspace_revision, Ordering::Relaxed);
        Ok(())
    }

    pub fn workspace_revision(&self) -> Result<u64> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .workspace_revision()
    }

    pub fn loaded_workspace_revision(&self) -> u64 {
        self.loaded_workspace_revision.load(Ordering::Relaxed)
    }

    pub fn loaded_workspace_revision_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.loaded_workspace_revision)
    }

    pub fn snapshot_revisions(&self) -> Result<WorkspaceSnapshotRevisions> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        self.sync_repo_memory_events_locked(&mut store)?;
        let mut revisions = store.snapshot_revisions()?;
        if !self.coordination_enabled {
            revisions.coordination = 0;
        }
        Ok(revisions)
    }

    pub fn episodic_revision(&self) -> Result<u64> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        self.sync_repo_memory_events_locked(&mut store)?;
        store.episodic_revision()
    }

    pub fn persist_episodic(&self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                episodic_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn append_memory_event(&self, event: MemoryEvent) -> Result<()> {
        if event.scope == prism_memory::MemoryScope::Repo {
            append_repo_memory_event(&self.root, &event)?;
        }
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .append_memory_events(&[event])?;
        Ok(())
    }

    pub fn memory_events(&self, query: &MemoryEventQuery) -> Result<Vec<MemoryEvent>> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        self.sync_repo_memory_events_locked(&mut store)?;
        let events = store.load_memory_events()?;
        Ok(filter_memory_events(events, query))
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
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .coordination_revision()
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
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_coordination_snapshot()
    }

    pub fn persist_coordination(&self, snapshot: &CoordinationSnapshot) -> Result<()> {
        if !self.coordination_enabled {
            return Ok(());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })
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
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(prism.coordination_snapshot()),
                ..AuxiliaryPersistBatch::default()
            })
    }

    pub fn mutate_coordination<T, F>(&self, mutate: F) -> Result<T>
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
        let prism = self.prism_arc();
        let result = mutate(prism.as_ref())?;
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                coordination_snapshot: Some(prism.coordination_snapshot()),
                ..AuxiliaryPersistBatch::default()
            })?;
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
            coordination_snapshot: None,
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
