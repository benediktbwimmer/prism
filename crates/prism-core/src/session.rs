use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, TryLockError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use prism_agent::{InferenceSnapshot, InferredEdgeRecord};
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, coordination_read_model_from_snapshot,
    snapshot_plan_graphs, CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
};
use prism_curator::{
    CuratorJobId, CuratorProposalDisposition, CuratorProposalState, CuratorSnapshot,
};
use prism_history::HistoryStore;
use prism_ir::{
    new_prefixed_id, AnchorRef, ChangeTrigger, CoordinationEventKind, CredentialId, EventActor,
    EventExecutionContext, EventId, EventMeta, LineageEvent, LineageId, ObservedChangeCheckpoint,
    ObservedChangeCheckpointEntry, ObservedChangeCheckpointTrigger, ObservedChangeSet,
    PlanExecutionOverlay, PlanGraph, PrincipalActor, PrincipalAuthorityId, PrincipalId,
    PrincipalRegistrySnapshot, SessionId, TaskId, WorkContextSnapshot,
};
use prism_memory::OutcomeMemory;
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEvent, MemoryEventQuery, OutcomeEvent, OutcomeKind,
    OutcomeRecallQuery, OutcomeResult, TaskReplay,
};
use prism_parser::ParseDepth;
use prism_projections::{
    concept_from_event, contract_from_event, validation_deltas_for_event, ConceptEvent,
    ConceptRelationEvent, ConceptRelationEventAction, ConceptScope, ContractEvent, ProjectionIndex,
};
use prism_query::Prism;
use prism_store::{AuxiliaryPersistBatch, Graph, SqliteStore, Store, WorkspaceTreeSnapshot};
use prism_store::{PatchEventSummary, PatchFileSummary};
use serde_json::{json, Value};
use tracing::{info, warn};

pub use prism_store::SnapshotRevisions as WorkspaceSnapshotRevisions;

pub(crate) const HOT_OUTCOME_HYDRATION_LIMIT: usize = 256;
const MUTATION_REFRESH_WAIT_TIMEOUT: Duration = Duration::from_millis(1500);
const MUTATION_REFRESH_RETRY_INTERVAL: Duration = Duration::from_millis(10);

use crate::admission::AdmissionBusyError;
use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::concept_events::append_repo_concept_event;
use crate::concept_relation_events::append_repo_concept_relation_event;
use crate::contract_events::append_repo_contract_event;
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator::{enqueue_curator_for_outcome_locked, CuratorHandle, CuratorHandleRef};
use crate::history_backend::StoreHistoryReadBackend;
use crate::indexer::{
    protected_knowledge_recovery_work, workspace_recovery_work, WorkspaceIndexer,
};
use crate::indexer_support::resolve_graph_edges;
use crate::layout::{discover_layout, sync_root_nodes};
use crate::materialization::{
    summarize_workspace_materialization, summarize_workspace_materialization_coverage,
    WorkspaceMaterializationCoverage, WorkspaceMaterializationSummary,
};
use crate::memory_events::{append_repo_memory_event, filter_memory_events};
use crate::mutation_trace;
use crate::observed_change_tracker::SharedObservedChangeTracker;
use crate::outcome_backend::StoreOutcomeReadBackend;
use crate::prism_doc::{sync_repo_prism_doc, PrismDocSyncResult};
use crate::projection_hydration::persisted_projection_load_plan;
use crate::protected_state::runtime_sync::{
    load_repo_protected_knowledge, load_repo_protected_plan_state, sync_repo_protected_state,
};
use crate::published_knowledge::{
    validate_repo_concept_event, validate_repo_concept_relation_event,
    validate_repo_contract_event, validate_repo_memory_event, validate_repo_patch_event,
};
use crate::repo_patch_events::{
    append_repo_patch_event, load_repo_patch_events, merge_repo_patch_events_into_memory,
};
use crate::runtime_engine::WorkspaceRuntimePathRequest;
use crate::shared_runtime::{
    composite_workspace_revision, merge_episodic_snapshots, merge_memory_events,
    merged_projection_index, split_episodic_snapshot_for_persist,
};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::shared_runtime_store::SharedRuntimeStore;
use crate::util::{cache_path, current_timestamp, current_timestamp_millis};
use crate::validation_feedback::{
    append_validation_feedback, load_validation_feedback, ValidationFeedbackEntry,
    ValidationFeedbackRecord,
};
use crate::watch::{refresh_prism_snapshot, try_refresh_prism_snapshot, WatchHandle, WatchMessage};
use crate::workspace_identity::coordination_persist_context_for_root;
use crate::workspace_runtime_state::{WorkspacePublishedGeneration, WorkspaceRuntimeState};
use crate::workspace_tree::{
    plan_full_refresh, populate_package_regions, WorkspaceRefreshDelta, WorkspaceRefreshMode,
    WorkspaceRefreshPlan,
};
use crate::worktree_principal::BoundWorktreePrincipal;
use crate::{ActiveWorkContextBinding, FlushedObservedChangeSet, ObservedChangeFlushTrigger};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsRefreshStatus {
    Clean,
    Incremental,
    Rescan,
    Full,
    DeferredBusy,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFsRefreshOutcome {
    pub status: FsRefreshStatus,
    pub observed: Vec<ObservedChangeSet>,
    pub breakdown: WorkspaceRefreshBreakdown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspaceRefreshBreakdown {
    pub plan_refresh_ms: u64,
    pub build_indexer_ms: u64,
    pub index_workspace_ms: u64,
    pub publish_generation_ms: u64,
    pub assisted_lease_ms: u64,
    pub curator_enqueue_ms: u64,
    pub attach_cold_query_backends_ms: u64,
    pub finalize_refresh_state_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspaceRefreshWork {
    pub loaded_bytes: u64,
    pub replay_volume: u64,
    pub full_rebuild_count: u64,
    pub workspace_reloaded: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceRefreshResult {
    pub(crate) mode: Option<WorkspaceRefreshMode>,
    pub(crate) observed: Vec<ObservedChangeSet>,
    pub(crate) breakdown: WorkspaceRefreshBreakdown,
}

#[derive(Debug, Clone)]
pub struct WorkspaceLastRefresh {
    pub path: String,
    pub timestamp: String,
    pub duration_ms: u64,
    pub fs_observed_revision: u64,
    pub fs_applied_revision: u64,
    pub workspace_revision: u64,
    pub loaded_bytes: u64,
    pub replay_volume: u64,
    pub full_rebuild_count: u64,
    pub workspace_reloaded: bool,
    pub changed_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub changed_directories: Vec<String>,
    pub changed_packages: Vec<String>,
    pub unaffected_directories: Vec<String>,
    pub unaffected_packages: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkspaceRefreshSeed {
    pub(crate) path: &'static str,
    pub(crate) duration_ms: u64,
    pub(crate) work: WorkspaceRefreshWork,
}

#[derive(Debug, Clone)]
pub struct CoordinationPlanState {
    pub snapshot: CoordinationSnapshot,
    pub plan_graphs: Vec<PlanGraph>,
    pub execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

fn coordination_delta_affects_repo_plan_projection(
    appended_events: &[prism_coordination::CoordinationEvent],
) -> bool {
    appended_events.iter().any(|event| {
        matches!(
            event.kind,
            CoordinationEventKind::PlanCreated
                | CoordinationEventKind::PlanUpdated
                | CoordinationEventKind::TaskCreated
                | CoordinationEventKind::TaskAssigned
                | CoordinationEventKind::TaskStatusChanged
                | CoordinationEventKind::TaskBlocked
                | CoordinationEventKind::TaskUnblocked
                | CoordinationEventKind::TaskResumed
                | CoordinationEventKind::TaskReclaimed
                | CoordinationEventKind::ClaimAcquired
                | CoordinationEventKind::ClaimReleased
                | CoordinationEventKind::HandoffRequested
                | CoordinationEventKind::HandoffAccepted
        )
    })
}

pub(crate) struct WorkspaceRefreshState {
    observed_fs_revision: AtomicU64,
    applied_fs_revision: AtomicU64,
    last_fallback_check_ms: AtomicU64,
    dirty_paths: Mutex<HashMap<PathBuf, u64>>,
    last_refresh: Mutex<Option<WorkspaceLastRefresh>>,
}

const FALLBACK_FINGERPRINT_INTERVAL_MS: u64 = 250;

impl WorkspaceRefreshState {
    pub(crate) fn new() -> Self {
        Self {
            observed_fs_revision: AtomicU64::new(0),
            applied_fs_revision: AtomicU64::new(0),
            last_fallback_check_ms: AtomicU64::new(0),
            dirty_paths: Mutex::new(HashMap::new()),
            last_refresh: Mutex::new(None),
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

    pub(crate) fn dirty_path_requests_snapshot(&self) -> Vec<WorkspaceRuntimePathRequest> {
        let mut requests = self
            .dirty_paths
            .lock()
            .expect("workspace dirty paths lock poisoned")
            .iter()
            .map(|(path, revision)| WorkspaceRuntimePathRequest {
                path: path.clone(),
                revision: *revision,
            })
            .collect::<Vec<_>>();
        requests.sort_by(|left, right| left.path.cmp(&right.path));
        requests
    }

    pub(crate) fn scoped_dirty_paths_for_requests(
        &self,
        requests: &[WorkspaceRuntimePathRequest],
    ) -> Vec<PathBuf> {
        let dirty_paths = self
            .dirty_paths
            .lock()
            .expect("workspace dirty paths lock poisoned");
        requests
            .iter()
            .filter(|request| {
                if request.revision == 0 {
                    return true;
                }
                dirty_paths
                    .get(&request.path)
                    .is_some_and(|revision| *revision == request.revision)
            })
            .map(|request| request.path.clone())
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

    pub(crate) fn record_refresh(
        &self,
        path: &str,
        duration_ms: u64,
        workspace_revision: u64,
        delta: &WorkspaceRefreshDelta,
    ) {
        let record = WorkspaceLastRefresh {
            path: path.to_string(),
            timestamp: current_timestamp().to_string(),
            duration_ms,
            fs_observed_revision: self.observed_fs_revision(),
            fs_applied_revision: self.applied_fs_revision(),
            workspace_revision,
            loaded_bytes: 0,
            replay_volume: 0,
            full_rebuild_count: 0,
            workspace_reloaded: false,
            changed_files: delta
                .changed_files
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            removed_files: delta
                .removed_files
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            changed_directories: delta
                .changed_directories
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            changed_packages: delta
                .changed_packages
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            unaffected_directories: delta
                .unaffected_directories
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            unaffected_packages: delta
                .unaffected_packages
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        };
        *self
            .last_refresh
            .lock()
            .expect("workspace last refresh lock poisoned") = Some(record);
    }

    pub(crate) fn record_runtime_refresh_observation_with_work(
        &self,
        path: &str,
        duration_ms: u64,
        workspace_revision: u64,
        work: WorkspaceRefreshWork,
    ) {
        let record = WorkspaceLastRefresh {
            path: path.to_string(),
            timestamp: current_timestamp().to_string(),
            duration_ms,
            fs_observed_revision: self.observed_fs_revision(),
            fs_applied_revision: self.applied_fs_revision(),
            workspace_revision,
            loaded_bytes: work.loaded_bytes,
            replay_volume: work.replay_volume,
            full_rebuild_count: work.full_rebuild_count,
            workspace_reloaded: work.workspace_reloaded,
            changed_files: Vec::new(),
            removed_files: Vec::new(),
            changed_directories: Vec::new(),
            changed_packages: Vec::new(),
            unaffected_directories: Vec::new(),
            unaffected_packages: Vec::new(),
        };
        *self
            .last_refresh
            .lock()
            .expect("workspace last refresh lock poisoned") = Some(record);
    }

    pub(crate) fn record_runtime_refresh_observation(
        &self,
        path: &str,
        duration_ms: u64,
        workspace_revision: u64,
    ) {
        self.record_runtime_refresh_observation_with_work(
            path,
            duration_ms,
            workspace_revision,
            WorkspaceRefreshWork::default(),
        );
    }

    pub(crate) fn last_refresh(&self) -> Option<WorkspaceLastRefresh> {
        self.last_refresh
            .lock()
            .expect("workspace last refresh lock poisoned")
            .clone()
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

    pub(crate) fn fallback_check_due(&self, now_ms: u64) -> bool {
        let last = self.last_fallback_check_ms.load(Ordering::Relaxed);
        now_ms.saturating_sub(last) >= FALLBACK_FINGERPRINT_INTERVAL_MS
    }
}

pub struct WorkspaceSession {
    pub(crate) root: PathBuf,
    pub(crate) published_generation: Arc<RwLock<WorkspacePublishedGeneration>>,
    pub(crate) runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    pub(crate) store: Arc<Mutex<SqliteStore>>,
    pub(crate) cold_query_store: Arc<Mutex<SqliteStore>>,
    pub(crate) shared_runtime: SharedRuntimeBackend,
    pub(crate) hydrate_persisted_projections: bool,
    pub(crate) hydrate_persisted_co_change: bool,
    pub(crate) shared_runtime_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    pub(crate) refresh_lock: Arc<Mutex<()>>,
    pub(crate) refresh_state: Arc<WorkspaceRefreshState>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) fs_snapshot: Arc<Mutex<WorkspaceTreeSnapshot>>,
    pub(crate) watch: Option<WatchHandle>,
    pub(crate) protected_state_watch: Option<WatchHandle>,
    pub(crate) curator: Option<CuratorHandle>,
    pub(crate) checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    pub(crate) shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    pub(crate) coordination_enabled: bool,
    pub(crate) worktree_principal_binding: Arc<Mutex<Option<BoundWorktreePrincipal>>>,
    pub(crate) observed_change_tracker: SharedObservedChangeTracker,
}

impl WorkspaceSession {
    pub fn bind_active_work_context(&self, work: ActiveWorkContextBinding) {
        self.observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .set_active_work(work);
    }

    pub fn clear_active_work_context(&self) {
        self.observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .clear_active_work();
    }

    pub fn active_work_context(&self) -> Option<ActiveWorkContextBinding> {
        self.observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .active_work()
    }

    pub fn flush_observed_changes(&self, trigger: ObservedChangeFlushTrigger) {
        self.observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .flush(trigger);
    }

    pub fn take_flushed_observed_changes(&self) -> Vec<FlushedObservedChangeSet> {
        self.observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .take_flushed()
    }

    pub fn persist_flushed_observed_change_checkpoints(
        &self,
        session_id: Option<&SessionId>,
        request_id: Option<String>,
        credential_id: Option<&CredentialId>,
        summary: Option<&str>,
    ) -> Result<Vec<EventId>> {
        let flushed = self.take_flushed_observed_changes();
        if flushed.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_summary = summary
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let mut event_ids = Vec::with_capacity(flushed.len());
        for change_set in flushed {
            let checkpoint = ObservedChangeCheckpoint {
                flush_trigger: observed_change_checkpoint_trigger(change_set.trigger),
                changed_paths: change_set.changed_paths.clone(),
                entries: change_set
                    .entries
                    .iter()
                    .map(|entry| ObservedChangeCheckpointEntry {
                        trigger: format!("{:?}", entry.trigger).to_ascii_lowercase(),
                        previous_path: entry.previous_path.clone(),
                        current_path: entry.current_path.clone(),
                        file_count: entry.file_count,
                        added_nodes: entry.added_nodes,
                        removed_nodes: entry.removed_nodes,
                        updated_nodes: entry.updated_nodes,
                        observed_at: entry.observed_at,
                    })
                    .collect(),
                window_started_at: change_set.window_started_at,
                window_ended_at: change_set.window_ended_at,
                summary: normalized_summary.clone(),
            };
            let summary = normalized_summary
                .clone()
                .unwrap_or_else(|| default_observed_change_checkpoint_summary(&change_set));
            let mut execution_context =
                self.event_execution_context(session_id, request_id.clone(), credential_id);
            execution_context.work_context = Some(WorkContextSnapshot {
                work_id: change_set.work.work_id.clone(),
                kind: change_set.work.kind,
                title: change_set.work.title.clone(),
                parent_work_id: change_set.work.parent_work_id.clone(),
                coordination_task_id: change_set.work.coordination_task_id.clone(),
                plan_id: change_set.work.plan_id.clone(),
                plan_title: change_set.work.plan_title.clone(),
            });
            let event = OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(new_prefixed_id("checkpoint")),
                    ts: change_set.window_ended_at,
                    actor: EventActor::Principal(PrincipalActor {
                        authority_id: PrincipalAuthorityId::new(
                            change_set.principal.authority_id.clone(),
                        ),
                        principal_id: PrincipalId::new(change_set.principal.principal_id.clone()),
                        kind: None,
                        name: Some(change_set.principal.principal_name.clone()),
                    }),
                    correlation: Some(TaskId::new(change_set.work.work_id.clone())),
                    causation: None,
                    execution_context: Some(execution_context),
                },
                anchors: Vec::new(),
                kind: prism_memory::OutcomeKind::NoteAdded,
                result: prism_memory::OutcomeResult::Success,
                summary,
                evidence: Vec::new(),
                metadata: json!({
                    "observedChangeCheckpoint": checkpoint,
                }),
            };
            event_ids.push(self.append_outcome(event)?);
        }
        Ok(event_ids)
    }

    fn shared_runtime_store(&self) -> Option<&Arc<Mutex<SharedRuntimeStore>>> {
        self.shared_runtime_store.as_ref()
    }

    fn lock_refresh_for_mutation(&self, reason: &'static str) -> MutexGuard<'_, ()> {
        let wait_started = Instant::now();
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        mutation_trace::record_phase(
            "mutation.waitRefreshLock",
            json!({ "reason": reason }),
            wait_started.elapsed(),
            true,
            None,
        );
        guard
    }

    fn try_lock_refresh_for_phase(
        &self,
        phase: &'static str,
        reason: &'static str,
    ) -> Result<Option<MutexGuard<'_, ()>>> {
        match self.refresh_lock.try_lock() {
            Ok(guard) => {
                mutation_trace::record_phase(
                    phase,
                    json!({ "reason": reason }),
                    Duration::ZERO,
                    true,
                    None,
                );
                Ok(Some(guard))
            }
            Err(TryLockError::WouldBlock) => {
                let error = AdmissionBusyError::refresh_lock(reason);
                mutation_trace::record_phase(
                    phase,
                    json!({ "reason": reason, "admission": "busy" }),
                    Duration::ZERO,
                    false,
                    Some(error.to_string()),
                );
                Ok(None)
            }
            Err(TryLockError::Poisoned(_)) => {
                panic!("workspace refresh lock poisoned");
            }
        }
    }

    fn try_lock_refresh_for_mutation(
        &self,
        reason: &'static str,
    ) -> Result<Option<MutexGuard<'_, ()>>> {
        self.try_lock_refresh_for_phase("mutation.waitRefreshLock", reason)
    }

    fn wait_lock_refresh_for_phase(
        &self,
        phase: &'static str,
        reason: &'static str,
        timeout: Duration,
    ) -> Result<Option<MutexGuard<'_, ()>>> {
        let wait_started = Instant::now();
        loop {
            match self.refresh_lock.try_lock() {
                Ok(guard) => {
                    mutation_trace::record_phase(
                        phase,
                        json!({ "reason": reason }),
                        wait_started.elapsed(),
                        true,
                        None,
                    );
                    return Ok(Some(guard));
                }
                Err(TryLockError::WouldBlock) => {
                    let elapsed = wait_started.elapsed();
                    if elapsed >= timeout {
                        let error = AdmissionBusyError::refresh_lock(reason);
                        mutation_trace::record_phase(
                            phase,
                            json!({ "reason": reason, "admission": "busy" }),
                            elapsed,
                            false,
                            Some(error.to_string()),
                        );
                        return Ok(None);
                    }
                    let remaining = timeout.saturating_sub(elapsed);
                    std::thread::sleep(remaining.min(MUTATION_REFRESH_RETRY_INTERVAL));
                }
                Err(TryLockError::Poisoned(_)) => {
                    panic!("workspace refresh lock poisoned");
                }
            }
        }
    }

    fn lock_store_for_mutation<'a, T>(
        store: &'a Arc<Mutex<T>>,
        operation: &'static str,
        reason: &'static str,
    ) -> MutexGuard<'a, T> {
        let wait_started = Instant::now();
        let guard = store.lock().expect("workspace store lock poisoned");
        mutation_trace::record_phase(
            operation,
            json!({ "reason": reason }),
            wait_started.elapsed(),
            true,
            None,
        );
        guard
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load_hot_lineage_history(&self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        Ok(self.prism_arc().hot_lineage_history(lineage))
    }

    pub fn load_cold_lineage_history(&self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        Ok(self.prism_arc().cold_lineage_history(lineage))
    }

    pub fn load_lineage_history(&self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        Ok(self.prism_arc().lineage_history(lineage))
    }

    pub fn load_hot_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        Ok(self.prism_arc().hot_task_replay(task_id))
    }

    pub fn load_cold_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        Ok(self.prism_arc().cold_task_replay(task_id))
    }

    pub fn load_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        Ok(self.prism_arc().resume_task(task_id))
    }

    pub fn load_hot_outcomes(
        &self,
        query: &prism_memory::OutcomeRecallQuery,
    ) -> Result<Vec<OutcomeEvent>> {
        Ok(self.prism_arc().query_hot_outcomes(query))
    }

    pub fn load_cold_outcomes(
        &self,
        query: &prism_memory::OutcomeRecallQuery,
    ) -> Result<Vec<OutcomeEvent>> {
        Ok(self.prism_arc().query_cold_outcomes(query))
    }

    pub fn load_outcomes(
        &self,
        query: &prism_memory::OutcomeRecallQuery,
    ) -> Result<Vec<OutcomeEvent>> {
        Ok(self.prism_arc().query_outcomes(query))
    }

    pub fn load_hot_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Ok(self.prism_arc().hot_outcome_event(event_id))
    }

    pub fn load_cold_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Ok(self.prism_arc().cold_outcome_event(event_id))
    }

    pub fn load_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Ok(self.prism_arc().outcome_event(event_id))
    }

    pub fn load_patch_event_summaries(
        &self,
        target: Option<&prism_ir::NodeId>,
        task_id: Option<&TaskId>,
        since: Option<u64>,
        path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PatchEventSummary>> {
        let target_anchor = target.cloned().map(AnchorRef::Node);
        let local = {
            let store = self.store.lock().expect("workspace store lock poisoned");
            store.load_patch_event_summaries(&prism_store::PatchEventSummaryQuery {
                target: target_anchor.clone(),
                task_id: task_id.cloned(),
                since,
                path: path.map(ToOwned::to_owned),
                limit,
            })?
        };
        let shared = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            store.load_patch_event_summaries(target_anchor.as_ref(), task_id, since, path, limit)?
        } else {
            Vec::new()
        };
        Ok(merge_patch_event_summaries(shared, local, limit))
    }

    pub fn load_patch_file_summaries(
        &self,
        task_id: Option<&TaskId>,
        since: Option<u64>,
        path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PatchFileSummary>> {
        let local = {
            let store = self.store.lock().expect("workspace store lock poisoned");
            store.load_patch_file_summaries(&prism_store::PatchFileSummaryQuery {
                task_id: task_id.cloned(),
                since,
                path: path.map(ToOwned::to_owned),
                limit,
            })?
        };
        let shared = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            store.load_patch_file_summaries(task_id, since, path, limit)?
        } else {
            Vec::new()
        };
        Ok(merge_patch_file_summaries(shared, local, limit))
    }

    pub fn prism(&self) -> Arc<Prism> {
        self.prism_arc()
    }

    pub fn prism_arc(&self) -> Arc<Prism> {
        self.published_generation
            .read()
            .expect("workspace published generation lock poisoned")
            .prism_arc()
    }

    pub fn publish_pending_repo_patch_provenance_for_active_work(&self) -> Result<Vec<EventId>> {
        let Some(bound_principal) = self.bound_worktree_principal() else {
            return Ok(Vec::new());
        };
        let Some(active_work) = self.active_work_context() else {
            return Ok(Vec::new());
        };

        let existing_repo_event_ids = load_repo_patch_events(&self.root)?
            .into_iter()
            .map(|event| event.meta.id)
            .collect::<HashSet<_>>();
        let patch_events = self.prism_arc().query_outcomes(&OutcomeRecallQuery {
            kinds: Some(vec![OutcomeKind::PatchApplied]),
            result: Some(OutcomeResult::Success),
            limit: 0,
            ..OutcomeRecallQuery::default()
        });
        let actor = EventActor::Principal(PrincipalActor {
            authority_id: PrincipalAuthorityId::new(bound_principal.authority_id),
            principal_id: PrincipalId::new(bound_principal.principal_id),
            kind: None,
            name: Some(bound_principal.principal_name),
        });

        let mut published = Vec::new();
        for patch_event in patch_events {
            if existing_repo_event_ids.contains(&patch_event.meta.id) {
                continue;
            }
            if !matches!(patch_event.meta.actor, EventActor::System) {
                continue;
            }
            let Some(work_context) = patch_event
                .meta
                .execution_context
                .as_ref()
                .and_then(|context| context.work_context.as_ref())
            else {
                continue;
            };
            if work_context.work_id != active_work.work_id {
                continue;
            }

            let mut repo_event = patch_event.clone();
            repo_event.meta.actor = actor.clone();
            validate_repo_patch_event(&repo_event)?;
            append_repo_patch_event(&self.root, &repo_event)?;
            published.push(repo_event);
        }

        if published.is_empty() {
            return Ok(Vec::new());
        }

        let prism = self.prism_arc();
        let mut runtime_state = self
            .runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        let mut event_ids = Vec::with_capacity(published.len());
        for event in published {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event.clone())?;
            runtime_state.apply_outcome_event(&event);
            event_ids.push(id);
        }
        drop(runtime_state);
        self.sync_prism_doc()?;
        Ok(event_ids)
    }

    pub(crate) fn attach_cold_query_backends(
        prism: &Prism,
        store: &Arc<Mutex<SqliteStore>>,
        shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    ) {
        prism.set_history_backend(Some(Arc::new(StoreHistoryReadBackend::new(Arc::clone(
            store,
        )))));
        prism.set_outcome_backend(Some(Arc::new(StoreOutcomeReadBackend::new(
            Arc::clone(store),
            shared_runtime_store.map(Arc::clone),
        ))));
    }

    fn publish_runtime_state(
        &self,
        runtime_state: WorkspaceRuntimeState,
        local_workspace_revision: u64,
        workspace_revision: u64,
        coordination_context: Option<prism_store::CoordinationPersistContext>,
    ) {
        let next = runtime_state.publish_generation(
            prism_ir::WorkspaceRevision {
                graph_version: local_workspace_revision,
                git_commit: None,
            },
            coordination_context,
        );
        Self::attach_cold_query_backends(
            next.prism_arc().as_ref(),
            &self.cold_query_store,
            self.shared_runtime_store.as_ref(),
        );
        *self
            .runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned") = runtime_state;
        *self
            .published_generation
            .write()
            .expect("workspace published generation lock poisoned") = next;
        self.loaded_workspace_revision
            .store(workspace_revision, Ordering::Relaxed);
    }

    pub fn sync_prism_doc(&self) -> Result<PrismDocSyncResult> {
        let prism = self.prism_arc();
        let concepts = prism.curated_concepts_snapshot();
        let relations = prism.concept_relations_snapshot();
        let contracts = prism.curated_contracts();
        sync_repo_prism_doc(&self.root, &concepts, &relations, &contracts)
    }

    pub fn refresh_fs(&self) -> Result<Vec<ObservedChangeSet>> {
        Ok(self.refresh_fs_with_status()?.observed)
    }

    pub fn refresh_fs_with_status(&self) -> Result<WorkspaceFsRefreshOutcome> {
        let outcome = self.refresh_fs_with_scoped_paths(None)?;
        if outcome.status != FsRefreshStatus::Clean {
            self.sync_prism_doc()?;
        }
        Ok(outcome)
    }

    pub fn refresh_fs_with_paths(
        &self,
        dirty_paths: Vec<PathBuf>,
    ) -> Result<WorkspaceFsRefreshOutcome> {
        let outcome =
            self.refresh_fs_with_scoped_paths((!dirty_paths.is_empty()).then_some(dirty_paths))?;
        if outcome.status != FsRefreshStatus::Clean {
            self.sync_prism_doc()?;
        }
        Ok(outcome)
    }

    fn refresh_fs_with_scoped_paths(
        &self,
        dirty_paths_override: Option<Vec<PathBuf>>,
    ) -> Result<WorkspaceFsRefreshOutcome> {
        let now_ms = current_timestamp_millis();
        let fs_fallback_due = self.refresh_state.should_run_fallback_check(now_ms);
        let has_scoped_override = dirty_paths_override
            .as_ref()
            .is_some_and(|dirty_paths| !dirty_paths.is_empty());
        if !self.refresh_state.needs_refresh() && !fs_fallback_due && !has_scoped_override {
            return Ok(WorkspaceFsRefreshOutcome {
                status: FsRefreshStatus::Clean,
                observed: Vec::new(),
                breakdown: WorkspaceRefreshBreakdown::default(),
            });
        }
        let dirty_paths = dirty_paths_override
            .clone()
            .unwrap_or_else(|| self.refresh_state.dirty_paths_snapshot());
        let refreshed = if (self.refresh_state.needs_refresh() || has_scoped_override)
            && !dirty_paths.is_empty()
        {
            self.refresh_with_trigger(ChangeTrigger::FsWatch, None, Some(dirty_paths))?
        } else {
            let known_snapshot = self
                .fs_snapshot
                .lock()
                .expect("workspace tree snapshot lock poisoned")
                .clone();
            let plan = plan_full_refresh(&self.root, &known_snapshot)?;
            if !self.refresh_state.needs_refresh() && plan.delta.is_empty() {
                return Ok(WorkspaceFsRefreshOutcome {
                    status: FsRefreshStatus::Clean,
                    observed: Vec::new(),
                    breakdown: WorkspaceRefreshBreakdown::default(),
                });
            }
            self.refresh_with_trigger(ChangeTrigger::FsWatch, Some(plan.next_snapshot), None)?
        };
        let status = match refreshed.mode {
            None => FsRefreshStatus::Clean,
            Some(WorkspaceRefreshMode::Incremental) => FsRefreshStatus::Incremental,
            Some(WorkspaceRefreshMode::Rescan) => FsRefreshStatus::Rescan,
            Some(WorkspaceRefreshMode::Full) => FsRefreshStatus::Full,
        };
        Ok(WorkspaceFsRefreshOutcome {
            status,
            observed: refreshed.observed,
            breakdown: refreshed.breakdown,
        })
    }

    pub fn refresh_fs_nonblocking(&self) -> Result<FsRefreshStatus> {
        let needs_refresh = self.refresh_state.needs_refresh();
        let now_ms = current_timestamp_millis();
        let fs_fallback_due = self.refresh_state.should_run_fallback_check(now_ms);
        if !needs_refresh && !fs_fallback_due {
            return Ok(FsRefreshStatus::Clean);
        }
        let dirty_paths = self.refresh_state.dirty_paths_snapshot();
        let refreshed = self.try_refresh_with_trigger(
            ChangeTrigger::FsWatch,
            None,
            (!dirty_paths.is_empty()).then_some(dirty_paths.clone()),
        )?;
        match refreshed {
            Some(result) => Ok(match result.mode {
                None => FsRefreshStatus::Clean,
                Some(WorkspaceRefreshMode::Incremental) => FsRefreshStatus::Incremental,
                Some(WorkspaceRefreshMode::Rescan) => FsRefreshStatus::Rescan,
                Some(WorkspaceRefreshMode::Full) => FsRefreshStatus::Full,
            }),
            None if needs_refresh || !dirty_paths.is_empty() => Ok(FsRefreshStatus::DeferredBusy),
            None => Ok(FsRefreshStatus::Clean),
        }
    }

    pub fn needs_refresh(&self) -> bool {
        self.refresh_state.needs_refresh()
    }

    pub fn pending_refresh_paths(&self) -> Vec<PathBuf> {
        self.refresh_state.dirty_paths_snapshot()
    }

    pub fn pending_refresh_path_requests(&self) -> Vec<WorkspaceRuntimePathRequest> {
        self.refresh_state.dirty_path_requests_snapshot()
    }

    pub fn scoped_refresh_paths_for_requests(
        &self,
        requests: &[WorkspaceRuntimePathRequest],
    ) -> Vec<PathBuf> {
        self.refresh_state.scoped_dirty_paths_for_requests(requests)
    }

    pub fn observed_fs_revision(&self) -> u64 {
        self.refresh_state.observed_fs_revision()
    }

    pub fn applied_fs_revision(&self) -> u64 {
        self.refresh_state.applied_fs_revision()
    }

    pub fn last_refresh(&self) -> Option<WorkspaceLastRefresh> {
        self.refresh_state.last_refresh()
    }

    pub fn is_fallback_check_due_now(&self) -> bool {
        self.refresh_state
            .fallback_check_due(current_timestamp_millis())
    }

    pub fn workspace_materialization_summary(&self) -> WorkspaceMaterializationSummary {
        let snapshot = self
            .fs_snapshot
            .lock()
            .expect("workspace fs snapshot lock poisoned")
            .clone();
        let prism = self.prism_arc();
        summarize_workspace_materialization(self.root(), &snapshot, prism.graph())
    }

    pub fn workspace_materialization_coverage(&self) -> WorkspaceMaterializationCoverage {
        let snapshot = self
            .fs_snapshot
            .lock()
            .expect("workspace fs snapshot lock poisoned")
            .clone();
        let prism = self.prism_arc();
        summarize_workspace_materialization_coverage(&snapshot, prism.graph())
    }

    fn ensure_paths_deep_with_guard<I>(
        &self,
        _guard: MutexGuard<'_, ()>,
        paths: I,
        started: Instant,
    ) -> Result<bool>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let current_prism = self.prism_arc();
        let deep_paths = paths
            .into_iter()
            .filter(|path| {
                current_prism
                    .graph()
                    .file_record(path)
                    .is_some_and(|record| record.parse_depth != ParseDepth::Deep)
            })
            .collect::<HashSet<_>>();
        if deep_paths.is_empty() {
            return Ok(false);
        }

        let cached_snapshot = self
            .fs_snapshot
            .lock()
            .expect("workspace tree snapshot lock poisoned")
            .clone();
        let coordination_context = current_prism.coordination_context();
        let runtime_state = {
            let mut state = self
                .runtime_state
                .lock()
                .expect("workspace runtime state lock poisoned");
            let placeholder = WorkspaceRuntimeState::placeholder_with_layout(state.layout());
            std::mem::replace(&mut *state, placeholder)
        };
        let mut runtime_state = runtime_state;
        runtime_state.overlay_live_projection_knowledge(current_prism.as_ref());
        let current_layout = runtime_state.layout();
        let layout_refresh_required = current_layout.refresh_required_for_paths(deep_paths.iter());
        let next_layout = if layout_refresh_required {
            discover_layout(&self.root)?
        } else {
            current_layout.clone()
        };
        let reopened_store = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .reopen_runtime_writer()?;
        let reopened_shared_runtime_store: Option<SharedRuntimeStore> = self
            .shared_runtime_store
            .as_ref()
            .map(|store| {
                store
                    .lock()
                    .expect("shared runtime store lock poisoned")
                    .reopen_runtime_writer()
            })
            .transpose()?;
        let mut indexer = WorkspaceIndexer::with_runtime_state_stores_and_options(
            &self.root,
            reopened_store,
            reopened_shared_runtime_store,
            runtime_state,
            next_layout.clone(),
            layout_refresh_required
                || next_layout.workspace_manifest != current_layout.workspace_manifest
                || next_layout.packages.len() != current_layout.packages.len(),
            Some(cached_snapshot.clone()),
            self.checkpoint_materializer.clone(),
            crate::WorkspaceSessionOptions {
                coordination: self.coordination_enabled,
                shared_runtime: self.shared_runtime.sqlite_path().map_or(
                    SharedRuntimeBackend::Disabled,
                    |path| SharedRuntimeBackend::Sqlite {
                        path: path.to_path_buf(),
                    },
                ),
                hydrate_persisted_projections: false,
                hydrate_persisted_co_change: true,
            },
        )?;
        indexer.shared_runtime_materializer = self.shared_runtime_materializer.clone();
        let mut plan = WorkspaceRefreshPlan {
            mode: WorkspaceRefreshMode::Incremental,
            delta: WorkspaceRefreshDelta {
                changed_files: deep_paths.iter().cloned().collect(),
                ..WorkspaceRefreshDelta::default()
            },
            next_snapshot: cached_snapshot,
        };
        populate_package_regions(&mut plan.delta, &indexer.layout);
        let index_result = indexer.index_with_refresh_plan_and_deep_paths(
            ChangeTrigger::ManualReindex,
            &plan,
            &deep_paths,
        );
        if let Err(error) = index_result {
            let fallback_state = WorkspaceRuntimeState::new(
                next_layout,
                Graph::from_snapshot(current_prism.graph().snapshot()),
                HistoryStore::from_snapshot(current_prism.history_snapshot()),
                OutcomeMemory::from_snapshot(current_prism.outcome_snapshot()),
                current_prism.coordination_snapshot(),
                current_prism.authored_plan_graphs(),
                current_prism.plan_execution_overlays_by_plan(),
                ProjectionIndex::from_snapshot(current_prism.projection_snapshot()),
            );
            *self
                .runtime_state
                .lock()
                .expect("workspace runtime state lock poisoned") = fallback_state;
            return Err(error);
        }

        let local_workspace_revision = indexer.store.workspace_revision()?;
        let workspace_revision = composite_workspace_revision(
            local_workspace_revision,
            indexer
                .shared_runtime_store
                .as_ref()
                .map(SharedRuntimeStore::workspace_revision)
                .transpose()?,
        );
        let next_state = indexer.into_runtime_state();
        self.publish_runtime_state(
            next_state,
            local_workspace_revision,
            workspace_revision,
            coordination_context,
        );
        info!(
            root = %self.root.display(),
            deepened_path_count = deep_paths.len(),
            workspace_revision,
            duration_ms = started.elapsed().as_millis(),
            "deepened prism workspace files on demand"
        );
        Ok(true)
    }

    pub fn ensure_paths_deep<I>(&self, paths: I) -> Result<bool>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let started = Instant::now();
        let guard = self.lock_refresh_for_mutation("ensurePathsDeep");
        self.ensure_paths_deep_with_guard(guard, paths, started)
    }

    pub fn try_ensure_paths_deep<I>(&self, paths: I) -> Result<Option<bool>>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let started = Instant::now();
        let Some(guard) = self.try_lock_refresh_for_mutation("ensurePathsDeep")? else {
            return Ok(None);
        };
        self.ensure_paths_deep_with_guard(guard, paths, started)
            .map(Some)
    }

    pub fn persist_outcomes(&self) -> Result<()> {
        let _guard = self.lock_refresh_for_mutation("persistOutcomes");
        let prism = self.prism_arc();
        let persist_started = Instant::now();
        let snapshot = prism.outcome_snapshot();
        let result = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            self.shared_runtime_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_outcome_snapshot(snapshot.clone()))
                .unwrap_or_else(|| {
                    let mut store = shared_runtime_store
                        .lock()
                        .expect("shared runtime store lock poisoned");
                    prism_store::MaterializationStore::save_outcome_snapshot(&mut *store, &snapshot)
                })
        } else {
            self.checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_outcome_snapshot(snapshot.clone()))
                .unwrap_or_else(|| {
                    let mut store = Self::lock_store_for_mutation(
                        &self.store,
                        "mutation.waitWorkspaceStoreLock",
                        "persistOutcomes",
                    );
                    store.save_outcome_snapshot(&snapshot)
                })
        };
        mutation_trace::record_phase(
            "mutation.persistOutcomesSchedule",
            json!({}),
            persist_started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
        result
    }

    fn append_outcome_event_to_persistent_stores(
        &self,
        store: &mut SqliteStore,
        event: &OutcomeEvent,
    ) -> Result<()> {
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::EventJournalStore::append_outcome_events(
                &mut *shared_store,
                std::slice::from_ref(event),
                &[],
            )?;
            store.append_local_outcome_projection(std::slice::from_ref(event))?;
            Ok(())
        } else {
            store.append_outcome_events(std::slice::from_ref(event), &[])?;
            Ok(())
        }
    }

    pub fn persist_history(&self) -> Result<()> {
        let _guard = self.lock_refresh_for_mutation("persistHistory");
        let prism = self.prism_arc();
        let mut store = Self::lock_store_for_mutation(
            &self.store,
            "mutation.waitWorkspaceStoreLock",
            "persistHistory",
        );
        let persist_started = Instant::now();
        let result = store.save_history_snapshot(&prism.history_snapshot());
        mutation_trace::record_phase(
            "mutation.persistHistory",
            json!({}),
            persist_started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
        result
    }

    pub fn load_episodic_snapshot(&self) -> Result<Option<EpisodicMemorySnapshot>> {
        let local_snapshot = {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            prism_store::MaterializationStore::load_episodic_snapshot(&mut *store)?
        };
        let shared_snapshot = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::MaterializationStore::load_episodic_snapshot(&mut *store)?
        } else {
            None
        };
        Ok(merge_episodic_snapshots(local_snapshot, shared_snapshot))
    }

    pub fn load_episodic_snapshot_for_runtime(&self) -> Result<Option<EpisodicMemorySnapshot>> {
        let local_snapshot = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .load_episodic_snapshot()?;
        let shared_snapshot = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::MaterializationStore::load_episodic_snapshot(&mut *store)?
        } else {
            None
        };
        Ok(merge_episodic_snapshots(local_snapshot, shared_snapshot))
    }

    #[allow(dead_code)]
    pub(crate) fn try_recover_runtime_from_persisted_state(&self) -> Result<bool> {
        let Ok(guard) = self.refresh_lock.try_lock() else {
            return Ok(false);
        };
        self.recover_runtime_from_persisted_state_with_guard(guard)?;
        Ok(true)
    }

    #[allow(dead_code)]
    fn recover_runtime_from_persisted_state_with_guard(
        &self,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<()> {
        self.recover_runtime_from_persisted_state_locked()
    }

    #[allow(dead_code)]
    fn recover_runtime_from_persisted_state_locked(&self) -> Result<()> {
        let started = Instant::now();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let local_workspace_revision = store.workspace_revision()?;
        let shared_workspace_revision =
            if let Some(shared_runtime_store) = self.shared_runtime_store() {
                let mut shared_store = shared_runtime_store
                    .lock()
                    .expect("shared runtime store lock poisoned");
                sync_repo_protected_state(&self.root, &mut *shared_store)?;
                Some(shared_store.workspace_revision()?)
            } else {
                sync_repo_protected_state(&self.root, &mut *store)?;
                None
            };
        let workspace_revision =
            composite_workspace_revision(local_workspace_revision, shared_workspace_revision);
        let mut graph = store.load_graph()?.unwrap_or_default();
        let layout = discover_layout(&self.root)?;
        sync_root_nodes(&mut graph, &layout);
        resolve_graph_edges(&mut graph, None);
        let projection_metadata = store.load_projection_materialization_metadata()?;
        let local_projection_snapshot = if self.hydrate_persisted_projections {
            store.load_projection_snapshot()?
        } else if self.hydrate_persisted_co_change {
            store.load_projection_snapshot()?
        } else {
            store.load_projection_snapshot_without_co_change()?
        };
        let load_plan = persisted_projection_load_plan(
            projection_metadata,
            self.hydrate_persisted_projections,
            self.hydrate_persisted_co_change,
        );
        let shared_runtime_aliases_workspace_store = self
            .shared_runtime
            .aliases_sqlite_path(&cache_path(&self.root)?);
        let shared_projection_snapshot = if shared_runtime_aliases_workspace_store {
            None
        } else if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            shared_store.load_projection_knowledge_snapshot()?
        } else {
            None
        };
        let mut history = store
            .load_history_snapshot_with_options(load_plan.load_history_events)?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let outcomes = if shared_runtime_aliases_workspace_store {
            if load_plan.load_full_outcomes {
                store.load_outcome_snapshot()?
            } else {
                store.load_recent_outcome_snapshot(HOT_OUTCOME_HYDRATION_LIMIT)?
            }
        } else if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            if load_plan.load_full_outcomes {
                prism_store::ColdQueryStore::load_outcome_snapshot(&mut *shared_store)?
            } else {
                prism_store::ColdQueryStore::load_recent_outcome_snapshot(
                    &mut *shared_store,
                    HOT_OUTCOME_HYDRATION_LIMIT,
                )?
            }
        } else {
            if load_plan.load_full_outcomes {
                store.load_outcome_snapshot()?
            } else {
                store.load_recent_outcome_snapshot(HOT_OUTCOME_HYDRATION_LIMIT)?
            }
        }
        .map(OutcomeMemory::from_snapshot)
        .unwrap_or_else(OutcomeMemory::new);
        merge_repo_patch_events_into_memory(&self.root, &outcomes)?;
        let plan_state = if self.coordination_enabled {
            if let Some(shared_runtime_store) = self.shared_runtime_store() {
                let mut shared_store = shared_runtime_store
                    .lock()
                    .expect("shared runtime store lock poisoned");
                load_repo_protected_plan_state(&self.root, &mut *shared_store)?
            } else {
                load_repo_protected_plan_state(&self.root, &mut *store)?
            }
        } else {
            None
        };
        let coordination_snapshot = plan_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let repo_knowledge = load_repo_protected_knowledge(&self.root)?;
        let protected_knowledge_work = protected_knowledge_recovery_work(&repo_knowledge)?;
        let projections = merged_projection_index(
            local_projection_snapshot,
            shared_projection_snapshot,
            repo_knowledge.curated_concepts,
            repo_knowledge.curated_contracts,
            repo_knowledge.concept_relations,
            &history.snapshot(),
            &outcomes.snapshot(),
        );
        let recovery_work = workspace_recovery_work(
            &graph,
            &history,
            &outcomes,
            protected_knowledge_work,
            &coordination_snapshot,
            &plan_state
                .as_ref()
                .map(|state| state.plan_graphs.clone())
                .unwrap_or_default(),
            &plan_state
                .as_ref()
                .map(|state| state.execution_overlays.clone())
                .unwrap_or_default(),
        )?;
        drop(store);

        let runtime_state = WorkspaceRuntimeState::new(
            discover_layout(&self.root)?,
            graph,
            history,
            outcomes,
            coordination_snapshot,
            plan_state
                .as_ref()
                .map(|state| state.plan_graphs.clone())
                .unwrap_or_default(),
            plan_state
                .as_ref()
                .map(|state| state.execution_overlays.clone())
                .unwrap_or_default(),
            projections,
        );
        self.publish_runtime_state(
            runtime_state,
            local_workspace_revision,
            workspace_revision,
            Some(coordination_persist_context_for_root(&self.root, None)),
        );
        self.record_runtime_refresh_observation_with_work(
            "recovery",
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            WorkspaceRefreshWork {
                workspace_reloaded: true,
                ..recovery_work
            },
        );
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
        Ok(composite_workspace_revision(
            local_revision,
            shared_revision,
        ))
    }

    pub fn loaded_workspace_revision(&self) -> u64 {
        self.loaded_workspace_revision.load(Ordering::Relaxed)
    }

    pub fn loaded_workspace_revision_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.loaded_workspace_revision)
    }

    pub fn record_runtime_refresh_observation(&self, path: &str, duration_ms: u64) {
        self.refresh_state.record_runtime_refresh_observation(
            path,
            duration_ms,
            self.loaded_workspace_revision(),
        );
    }

    pub fn record_runtime_refresh_observation_with_work(
        &self,
        path: &str,
        duration_ms: u64,
        work: WorkspaceRefreshWork,
    ) {
        self.refresh_state
            .record_runtime_refresh_observation_with_work(
                path,
                duration_ms,
                self.loaded_workspace_revision(),
                work,
            );
    }

    pub fn snapshot_revisions(&self) -> Result<WorkspaceSnapshotRevisions> {
        let mut revisions = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .snapshot_revisions()?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            let shared_revisions = shared_store.snapshot_revisions()?;
            revisions.workspace =
                composite_workspace_revision(revisions.workspace, Some(shared_revisions.workspace));
            revisions.episodic = revisions.episodic.max(shared_revisions.episodic);
            revisions.coordination = shared_revisions.coordination;
        }
        if !self.coordination_enabled {
            revisions.coordination = 0;
        }
        Ok(revisions)
    }

    pub fn snapshot_revisions_for_runtime(&self) -> Result<WorkspaceSnapshotRevisions> {
        let mut revisions = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .snapshot_revisions()?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let shared_revisions = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .snapshot_revisions()?;
            revisions.workspace =
                composite_workspace_revision(revisions.workspace, Some(shared_revisions.workspace));
            revisions.episodic = revisions.episodic.max(shared_revisions.episodic);
            revisions.coordination = shared_revisions.coordination;
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
            let store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            Ok(local_revision.max(store.episodic_revision()?))
        } else {
            Ok(local_revision)
        }
    }

    pub fn persist_episodic(&self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let (local_snapshot, shared_snapshot) = split_episodic_snapshot_for_persist(snapshot);
            let local_started = Instant::now();
            let local_result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_episodic_snapshot(local_snapshot.clone()))
                .unwrap_or_else(|| {
                    let mut store = Self::lock_store_for_mutation(
                        &self.store,
                        "mutation.waitWorkspaceStoreLock",
                        "persistEpisodicLocal",
                    );
                    store.save_episodic_snapshot(&local_snapshot)
                });
            mutation_trace::record_phase(
                "mutation.persistEpisodicSchedule",
                json!({ "target": "workspace" }),
                local_started.elapsed(),
                local_result.is_ok(),
                local_result.as_ref().err().map(ToString::to_string),
            );
            local_result?;
            let shared_started = Instant::now();
            let shared_result = self
                .shared_runtime_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_episodic_snapshot(shared_snapshot.clone()))
                .unwrap_or_else(|| {
                    let mut shared_store = Self::lock_store_for_mutation(
                        shared_runtime_store,
                        "mutation.waitSharedRuntimeStoreLock",
                        "persistEpisodicShared",
                    );
                    prism_store::MaterializationStore::save_episodic_snapshot(
                        &mut *shared_store,
                        &shared_snapshot,
                    )
                });
            mutation_trace::record_phase(
                "mutation.persistEpisodicSchedule",
                json!({ "target": "sharedRuntime" }),
                shared_started.elapsed(),
                shared_result.is_ok(),
                shared_result.as_ref().err().map(ToString::to_string),
            );
            shared_result?;
        } else {
            let persist_started = Instant::now();
            let result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_episodic_snapshot(snapshot.clone()))
                .unwrap_or_else(|| {
                    let mut store = Self::lock_store_for_mutation(
                        &self.store,
                        "mutation.waitWorkspaceStoreLock",
                        "persistEpisodic",
                    );
                    store.save_episodic_snapshot(snapshot)
                });
            mutation_trace::record_phase(
                "mutation.persistEpisodicSchedule",
                json!({ "target": "workspace" }),
                persist_started.elapsed(),
                result.is_ok(),
                result.as_ref().err().map(ToString::to_string),
            );
            result?;
        }
        Ok(())
    }

    pub fn append_memory_event(&self, event: MemoryEvent) -> Result<()> {
        let should_sync_prism_doc = event.scope == prism_memory::MemoryScope::Repo;
        if event.scope == prism_memory::MemoryScope::Repo {
            validate_repo_memory_event(&event)?;
            append_repo_memory_event(&self.root, &event)?;
        }
        match (event.scope, self.shared_runtime_store()) {
            (prism_memory::MemoryScope::Local, _) => {
                let mut store = Self::lock_store_for_mutation(
                    &self.store,
                    "mutation.waitWorkspaceStoreLock",
                    "appendMemoryEventLocal",
                );
                let persist_started = Instant::now();
                let result = store.append_memory_events(&[event]);
                mutation_trace::record_phase(
                    "mutation.appendMemoryEvent",
                    json!({ "target": "workspace", "scope": "local" }),
                    persist_started.elapsed(),
                    result.is_ok(),
                    result.as_ref().err().map(ToString::to_string),
                );
                result?;
            }
            (_, Some(shared_runtime_store)) => {
                let scope = match event.scope {
                    prism_memory::MemoryScope::Local => "local",
                    prism_memory::MemoryScope::Session => "session",
                    prism_memory::MemoryScope::Repo => "repo",
                };
                let mut store = Self::lock_store_for_mutation(
                    shared_runtime_store,
                    "mutation.waitSharedRuntimeStoreLock",
                    "appendMemoryEventShared",
                );
                let persist_started = Instant::now();
                let result =
                    prism_store::EventJournalStore::append_memory_events(&mut *store, &[event]);
                mutation_trace::record_phase(
                    "mutation.appendMemoryEvent",
                    json!({ "target": "sharedRuntime", "scope": scope }),
                    persist_started.elapsed(),
                    result.is_ok(),
                    result.as_ref().err().map(ToString::to_string),
                );
                result?;
            }
            (_, None) => {
                let scope = match event.scope {
                    prism_memory::MemoryScope::Local => "local",
                    prism_memory::MemoryScope::Session => "session",
                    prism_memory::MemoryScope::Repo => "repo",
                };
                let mut store = Self::lock_store_for_mutation(
                    &self.store,
                    "mutation.waitWorkspaceStoreLock",
                    "appendMemoryEvent",
                );
                let persist_started = Instant::now();
                let result = store.append_memory_events(&[event]);
                mutation_trace::record_phase(
                    "mutation.appendMemoryEvent",
                    json!({ "target": "workspace", "scope": scope }),
                    persist_started.elapsed(),
                    result.is_ok(),
                    result.as_ref().err().map(ToString::to_string),
                );
                result?;
            }
        }
        if should_sync_prism_doc {
            self.sync_prism_doc()?;
        }
        Ok(())
    }

    pub fn append_inference_records(&self, records: &[InferredEdgeRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .append_inference_records(records)?;
        Ok(())
    }

    fn delete_persisted_projection_concept_everywhere(&self, handle: &str) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .delete_projection_concept(handle)?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .delete_projection_concept(handle)?;
        }
        Ok(())
    }

    fn delete_persisted_projection_relation_everywhere(
        &self,
        source_handle: &str,
        target_handle: &str,
        kind: prism_projections::ConceptRelationKind,
    ) -> Result<()> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .delete_projection_concept_relation(source_handle, target_handle, kind)?;
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .delete_projection_concept_relation(source_handle, target_handle, kind)?;
        }
        Ok(())
    }

    pub fn memory_events(&self, query: &MemoryEventQuery) -> Result<Vec<MemoryEvent>> {
        let local_events = {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            store.load_memory_events()?
        };
        let shared_events = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::ColdQueryStore::load_memory_events(&mut *store)?
        } else {
            Vec::new()
        };
        Ok(filter_memory_events(
            merge_memory_events(local_events, shared_events),
            query,
        ))
    }

    pub fn load_principal_registry(&self) -> Result<Option<PrincipalRegistrySnapshot>> {
        let Some(shared_runtime_store) = self.shared_runtime_store() else {
            return Ok(None);
        };
        let mut store = shared_runtime_store
            .lock()
            .expect("shared runtime store lock poisoned");
        prism_store::MaterializationStore::load_principal_registry_snapshot(&mut *store)
    }

    pub fn persist_principal_registry(&self, snapshot: &PrincipalRegistrySnapshot) -> Result<()> {
        let Some(shared_runtime_store) = self.shared_runtime_store() else {
            return Err(anyhow!(
                "principal registry persistence requires a shared runtime backend"
            ));
        };
        let mut store = shared_runtime_store
            .lock()
            .expect("shared runtime store lock poisoned");
        prism_store::MaterializationStore::save_principal_registry_snapshot(&mut *store, snapshot)
    }

    pub fn event_execution_context(
        &self,
        session_id: Option<&SessionId>,
        request_id: Option<String>,
        credential_id: Option<&CredentialId>,
    ) -> EventExecutionContext {
        let context = coordination_persist_context_for_root(&self.root, session_id);
        EventExecutionContext {
            repo_id: Some(context.repo_id),
            worktree_id: Some(context.worktree_id),
            branch_ref: context.branch_ref,
            session_id: context.session_id,
            instance_id: context.instance_id,
            request_id,
            credential_id: credential_id.cloned(),
            work_context: None,
        }
    }

    pub fn append_concept_event(&self, event: ConceptEvent) -> Result<()> {
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.append_concept_event_guarded(event, guard)
    }

    pub fn try_append_concept_event(&self, event: ConceptEvent) -> Result<bool> {
        let Some(guard) = self.try_lock_refresh_for_mutation("appendConceptEvent")? else {
            return Ok(false);
        };
        self.append_concept_event_guarded(event, guard)?;
        Ok(true)
    }

    fn append_concept_event_guarded(
        &self,
        event: ConceptEvent,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<()> {
        let should_sync_prism_doc = event.concept.scope == prism_projections::ConceptScope::Repo;
        if should_sync_prism_doc {
            validate_repo_concept_event(&event)?;
            append_repo_concept_event(&self.root, &event)?;
        }
        let prism = self.prism_arc();
        let previous = prism.concept_by_handle(&event.concept.handle);
        let concept = concept_from_event(previous.as_ref(), &event);
        prism.upsert_curated_concept(concept.clone());
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .projections
            .upsert_curated_concept(concept.clone());
        self.delete_persisted_projection_concept_everywhere(&event.concept.handle)?;
        if concept.scope == ConceptScope::Session {
            if let Some(target) = self.shared_runtime_store() {
                target
                    .lock()
                    .expect("projection target store lock poisoned")
                    .upsert_projection_concept(&concept)?;
            } else {
                self.store
                    .lock()
                    .expect("projection target store lock poisoned")
                    .upsert_projection_concept(&concept)?;
            }
        }
        if should_sync_prism_doc {
            self.sync_prism_doc()?;
        }
        Ok(())
    }

    pub fn append_contract_event(&self, event: ContractEvent) -> Result<()> {
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.append_contract_event_guarded(event, guard)
    }

    pub fn try_append_contract_event(&self, event: ContractEvent) -> Result<bool> {
        let Some(guard) = self.try_lock_refresh_for_mutation("appendContractEvent")? else {
            return Ok(false);
        };
        self.append_contract_event_guarded(event, guard)?;
        Ok(true)
    }

    fn append_contract_event_guarded(
        &self,
        event: ContractEvent,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<()> {
        let should_sync_prism_doc = event.contract.scope == prism_projections::ContractScope::Repo;
        if should_sync_prism_doc {
            validate_repo_contract_event(&event)?;
            append_repo_contract_event(&self.root, &event)?;
        }
        let prism = self.prism_arc();
        let previous = prism.contract_by_handle(&event.contract.handle);
        let contract = contract_from_event(previous.as_ref(), &event);
        prism.upsert_curated_contract(contract.clone());
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .projections
            .upsert_curated_contract(contract);
        if should_sync_prism_doc {
            self.sync_prism_doc()?;
        }
        Ok(())
    }

    pub fn append_concept_relation_event(&self, event: ConceptRelationEvent) -> Result<()> {
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.append_concept_relation_event_guarded(event, guard)
    }

    pub fn try_append_concept_relation_event(&self, event: ConceptRelationEvent) -> Result<bool> {
        let Some(guard) = self.try_lock_refresh_for_mutation("appendConceptRelationEvent")? else {
            return Ok(false);
        };
        self.append_concept_relation_event_guarded(event, guard)?;
        Ok(true)
    }

    fn append_concept_relation_event_guarded(
        &self,
        event: ConceptRelationEvent,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<()> {
        let should_sync_prism_doc = event.relation.scope == prism_projections::ConceptScope::Repo;
        if should_sync_prism_doc {
            validate_repo_concept_relation_event(&event)?;
            append_repo_concept_relation_event(&self.root, &event)?;
        }
        let prism = self.prism_arc();
        let relation = event.relation.clone();
        match event.action {
            ConceptRelationEventAction::Upsert => {
                prism.upsert_concept_relation(relation.clone());
                self.runtime_state
                    .lock()
                    .expect("workspace runtime state lock poisoned")
                    .projections
                    .upsert_concept_relation(relation.clone());
            }
            ConceptRelationEventAction::Retire => {
                prism.remove_concept_relation(
                    &relation.source_handle,
                    &relation.target_handle,
                    relation.kind,
                );
                self.runtime_state
                    .lock()
                    .expect("workspace runtime state lock poisoned")
                    .projections
                    .remove_concept_relation(
                        &relation.source_handle,
                        &relation.target_handle,
                        relation.kind,
                    );
            }
        }
        self.delete_persisted_projection_relation_everywhere(
            &relation.source_handle,
            &relation.target_handle,
            relation.kind,
        )?;
        if matches!(event.action, ConceptRelationEventAction::Upsert)
            && relation.scope == ConceptScope::Session
        {
            if let Some(target) = self.shared_runtime_store() {
                target
                    .lock()
                    .expect("projection target store lock poisoned")
                    .upsert_projection_concept_relation(&relation)?;
            } else {
                self.store
                    .lock()
                    .expect("projection target store lock poisoned")
                    .upsert_projection_concept_relation(&relation)?;
            }
        }
        if should_sync_prism_doc {
            self.sync_prism_doc()?;
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
            let store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::CoordinationJournal::coordination_revision(&*store)
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .coordination_revision()
        }
    }

    pub fn persist_inference(&self, snapshot: &InferenceSnapshot) -> Result<()> {
        let persist_started = Instant::now();
        let result = self
            .checkpoint_materializer
            .as_ref()
            .map(|materializer| materializer.enqueue_inference_snapshot(snapshot.clone()))
            .unwrap_or_else(|| {
                self.store
                    .lock()
                    .expect("workspace store lock poisoned")
                    .save_inference_snapshot(snapshot)
            });
        mutation_trace::record_phase(
            "mutation.persistInferenceSchedule",
            json!({ "target": "workspace" }),
            persist_started.elapsed(),
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        );
        result
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
        let persisted = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::CoordinationCheckpointStore::load_coordination_read_model(&mut *store)
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .load_coordination_read_model()
        }?;
        Ok(Some(persisted.unwrap_or_else(|| {
            coordination_read_model_from_snapshot(&self.prism_arc().coordination_snapshot())
        })))
    }

    pub fn load_coordination_queue_read_model(&self) -> Result<Option<CoordinationQueueReadModel>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        let persisted = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            prism_store::CoordinationCheckpointStore::load_coordination_queue_read_model(
                &mut *store,
            )
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .load_coordination_queue_read_model()
        }?;
        Ok(Some(persisted.unwrap_or_else(|| {
            coordination_queue_read_model_from_snapshot(&self.prism_arc().coordination_snapshot())
        })))
    }

    pub fn load_coordination_plan_state(&self) -> Result<Option<CoordinationPlanState>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        let state = if let Some(store) = self.shared_runtime_store() {
            let mut store = store.lock().expect("shared runtime store lock poisoned");
            load_repo_protected_plan_state(&self.root, &mut *store)?
        } else {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            load_repo_protected_plan_state(&self.root, &mut *store)?
        };
        Ok(state.map(|state| CoordinationPlanState {
            snapshot: state.snapshot,
            plan_graphs: state.plan_graphs,
            execution_overlays: state.execution_overlays,
        }))
    }

    pub fn hydrate_coordination_runtime(&self) -> Result<Option<CoordinationPlanState>> {
        let state = self.load_coordination_plan_state()?;
        let snapshot = state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let plan_graphs = state
            .as_ref()
            .map(|state| state.plan_graphs.clone())
            .unwrap_or_default();
        let execution_overlays = state
            .as_ref()
            .map(|state| state.execution_overlays.clone())
            .unwrap_or_default();
        self.prism_arc()
            .replace_coordination_snapshot_and_plan_graphs(
                snapshot,
                plan_graphs,
                execution_overlays,
            );
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .replace_coordination_runtime(
                state
                    .as_ref()
                    .map(|state| state.snapshot.clone())
                    .unwrap_or_default(),
                state
                    .as_ref()
                    .map(|state| state.plan_graphs.clone())
                    .unwrap_or_default(),
                state
                    .as_ref()
                    .map(|state| state.execution_overlays.clone())
                    .unwrap_or_default(),
            );
        Ok(state)
    }

    pub fn persist_coordination(&self, snapshot: &CoordinationSnapshot) -> Result<()> {
        if !self.coordination_enabled {
            return Ok(());
        }
        let _guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        let result = if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .persist_coordination_snapshot_for_root(&self.root, snapshot)
        } else {
            self.store
                .lock()
                .expect("workspace store lock poisoned")
                .persist_coordination_snapshot_for_root(&self.root, snapshot)
        };
        result
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
        let plan_graphs = prism.authored_plan_graphs();
        let execution_overlays = prism.plan_execution_overlays_by_plan();
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .replace_coordination_runtime(
                snapshot.clone(),
                plan_graphs.clone(),
                execution_overlays.clone(),
            );
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            shared_runtime_store
                .lock()
                .expect("coordination store lock poisoned")
                .persist_coordination_authoritative_state_for_root(
                    &self.root,
                    &snapshot,
                    Some(&plan_graphs),
                    Some(&execution_overlays),
                )?;
        } else {
            self.store
                .lock()
                .expect("coordination store lock poisoned")
                .persist_coordination_authoritative_state_for_root(
                    &self.root,
                    &snapshot,
                    Some(&plan_graphs),
                    Some(&execution_overlays),
                )?;
        }
        let materialize_started = Instant::now();
        let enqueue_result = self
            .shared_runtime_materializer
            .as_ref()
            .or(self.checkpoint_materializer.as_ref())
            .map(|materializer| materializer.enqueue_coordination_snapshot(snapshot.clone()))
            .unwrap_or_else(|| {
                let read_model = coordination_read_model_from_snapshot(&snapshot);
                let queue_model = coordination_queue_read_model_from_snapshot(&snapshot);
                if let Some(shared_runtime_store) = self.shared_runtime_store() {
                    let mut store = shared_runtime_store
                        .lock()
                        .expect("coordination store lock poisoned");
                    prism_store::CoordinationCheckpointStore::save_coordination_read_model(
                        &mut *store,
                        &read_model,
                    )?;
                    prism_store::CoordinationCheckpointStore::save_coordination_queue_read_model(
                        &mut *store,
                        &queue_model,
                    )?;
                    if prism_store::CoordinationJournal::load_coordination_event_stream(
                        &mut *store,
                    )?
                    .suffix_events
                    .len()
                        >= 128
                    {
                        prism_store::CoordinationCheckpointStore::save_coordination_compaction(
                            &mut *store,
                            &snapshot,
                        )?;
                    }
                } else {
                    let mut store = self.store.lock().expect("coordination store lock poisoned");
                    store.save_coordination_read_model(&read_model)?;
                    store.save_coordination_queue_read_model(&queue_model)?;
                    if store.load_coordination_event_stream()?.suffix_events.len() >= 128 {
                        store.save_coordination_compaction(&snapshot)?;
                    }
                }
                Ok(())
            });
        mutation_trace::record_phase(
            "mutation.coordination.scheduleMaterialization",
            json!({}),
            materialize_started.elapsed(),
            enqueue_result.is_ok(),
            enqueue_result.as_ref().err().map(ToString::to_string),
        );
        enqueue_result
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
        self.mutate_coordination_with_session_observed(
            session_id,
            mutate,
            |_operation, _duration, _args, _success, _error| {},
        )
    }

    fn mutate_coordination_with_session_guarded<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        mut observe_phase: O,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        let revision_started = Instant::now();
        let expected_revision = match self.coordination_revision() {
            Ok(revision) => {
                observe_phase(
                    "mutation.coordination.readRevision",
                    revision_started.elapsed(),
                    json!({ "revision": revision }),
                    true,
                    None,
                );
                revision
            }
            Err(error) => {
                observe_phase(
                    "mutation.coordination.readRevision",
                    revision_started.elapsed(),
                    json!({}),
                    false,
                    Some(error.to_string()),
                );
                return Err(error);
            }
        };
        let prism = self.prism_arc();
        let before = prism.coordination_snapshot();
        let before_plan_graphs = snapshot_plan_graphs(&before);
        let mutate_started = Instant::now();
        let result = mutate(prism.as_ref());
        observe_phase(
            "mutation.coordination.applyMutation",
            mutate_started.elapsed(),
            json!({ "workspaceRevision": prism.workspace_revision() }),
            result.is_ok(),
            result.as_ref().err().map(|error| error.to_string()),
        );
        let delta_started = Instant::now();
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
        observe_phase(
            "mutation.coordination.captureDelta",
            delta_started.elapsed(),
            json!({
                "appendedEventCount": appended_events.len(),
                "snapshotChanged": snapshot != before,
            }),
            result.is_ok(),
            result.as_ref().err().map(|error| error.to_string()),
        );
        let plan_graphs = prism.authored_plan_graphs();
        let execution_overlays = prism.plan_execution_overlays_by_plan();
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .replace_coordination_runtime(
                snapshot.clone(),
                plan_graphs.clone(),
                execution_overlays.clone(),
            );
        if let Some(shared_runtime_store) = self.shared_runtime_store() {
            let mut store = shared_runtime_store
                .lock()
                .expect("coordination store lock poisoned");
            let should_persist = !appended_events.is_empty() || snapshot != before;
            if should_persist {
                let persist_started = Instant::now();
                let persist_result = store
                    .persist_coordination_authoritative_mutation_state_for_root_with_session_observed(
                        &self.root,
                        expected_revision,
                        &snapshot,
                        &appended_events,
                        session_id,
                        Some(&before),
                        Some(&before_plan_graphs),
                        Some(&plan_graphs),
                        Some(&execution_overlays),
                        &mut observe_phase,
                    );
                observe_phase(
                    "mutation.coordination.persistState",
                    persist_started.elapsed(),
                    json!({
                        "appendedEventCount": appended_events.len(),
                        "planCount": plan_graphs.len(),
                    }),
                    persist_result.is_ok(),
                    persist_result.as_ref().err().map(|error| error.to_string()),
                );
                persist_result?;
                let materialize_started = Instant::now();
                let enqueue_result = if let Some(materializer) = self
                    .shared_runtime_materializer
                    .as_ref()
                    .or(self.checkpoint_materializer.as_ref())
                {
                    materializer.enqueue_coordination_snapshot(snapshot.clone())
                } else {
                    let read_model = coordination_read_model_from_snapshot(&snapshot);
                    let queue_model = coordination_queue_read_model_from_snapshot(&snapshot);
                    prism_store::CoordinationCheckpointStore::save_coordination_read_model(
                        &mut *store,
                        &read_model,
                    )?;
                    prism_store::CoordinationCheckpointStore::save_coordination_queue_read_model(
                        &mut *store,
                        &queue_model,
                    )?;
                    if prism_store::CoordinationJournal::load_coordination_event_stream(
                        &mut *store,
                    )?
                    .suffix_events
                    .len()
                        >= 128
                    {
                        prism_store::CoordinationCheckpointStore::save_coordination_compaction(
                            &mut *store,
                            &snapshot,
                        )?;
                    }
                    Ok(())
                };
                observe_phase(
                    "mutation.coordination.scheduleMaterialization",
                    materialize_started.elapsed(),
                    json!({ "eventCount": snapshot.events.len() }),
                    enqueue_result.is_ok(),
                    enqueue_result.as_ref().err().map(|error| error.to_string()),
                );
                enqueue_result?;
            }
        } else {
            let mut store = self.store.lock().expect("coordination store lock poisoned");
            let should_persist = !appended_events.is_empty() || snapshot != before;
            if should_persist {
                let persist_started = Instant::now();
                let persist_result = store
                    .persist_coordination_authoritative_mutation_state_for_root_with_session_observed(
                        &self.root,
                        expected_revision,
                        &snapshot,
                        &appended_events,
                        session_id,
                        Some(&before),
                        Some(&before_plan_graphs),
                        Some(&plan_graphs),
                        Some(&execution_overlays),
                        &mut observe_phase,
                    );
                observe_phase(
                    "mutation.coordination.persistState",
                    persist_started.elapsed(),
                    json!({
                        "appendedEventCount": appended_events.len(),
                        "planCount": plan_graphs.len(),
                    }),
                    persist_result.is_ok(),
                    persist_result.as_ref().err().map(|error| error.to_string()),
                );
                persist_result?;
                let materialize_started = Instant::now();
                let enqueue_result = if let Some(materializer) = self
                    .shared_runtime_materializer
                    .as_ref()
                    .or(self.checkpoint_materializer.as_ref())
                {
                    materializer.enqueue_coordination_snapshot(snapshot.clone())
                } else {
                    let read_model = coordination_read_model_from_snapshot(&snapshot);
                    let queue_model = coordination_queue_read_model_from_snapshot(&snapshot);
                    store.save_coordination_read_model(&read_model)?;
                    store.save_coordination_queue_read_model(&queue_model)?;
                    if store.load_coordination_event_stream()?.suffix_events.len() >= 128 {
                        store.save_coordination_compaction(&snapshot)?;
                    }
                    Ok(())
                };
                observe_phase(
                    "mutation.coordination.scheduleMaterialization",
                    materialize_started.elapsed(),
                    json!({ "eventCount": snapshot.events.len() }),
                    enqueue_result.is_ok(),
                    enqueue_result.as_ref().err().map(|error| error.to_string()),
                );
                enqueue_result?;
            }
        }
        if coordination_delta_affects_repo_plan_projection(&appended_events) {
            self.sync_prism_doc()?;
        }
        result
    }

    pub fn mutate_coordination_with_session_observed<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        mut observe_phase: O,
    ) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        if !self.coordination_enabled {
            return Err(anyhow!(
                "coordination is disabled for this workspace session"
            ));
        }
        let lock_wait_started = Instant::now();
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        observe_phase(
            "mutation.coordination.waitRefreshLock",
            lock_wait_started.elapsed(),
            json!({}),
            true,
            None,
        );
        self.mutate_coordination_with_session_guarded(session_id, mutate, observe_phase, guard)
    }

    pub fn try_mutate_coordination_with_session<T, F>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
    ) -> Result<Option<T>>
    where
        F: FnOnce(&Prism) -> Result<T>,
    {
        self.try_mutate_coordination_with_session_observed(
            session_id,
            mutate,
            |_operation, _duration, _args, _success, _error| {},
        )
    }

    pub fn try_mutate_coordination_with_session_observed<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        observe_phase: O,
    ) -> Result<Option<T>>
    where
        F: FnOnce(&Prism) -> Result<T>,
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        if !self.coordination_enabled {
            return Err(anyhow!(
                "coordination is disabled for this workspace session"
            ));
        }
        let Some(guard) = self.try_lock_refresh_for_phase(
            "mutation.coordination.waitRefreshLock",
            "mutateCoordination",
        )?
        else {
            return Ok(None);
        };
        self.mutate_coordination_with_session_guarded(session_id, mutate, observe_phase, guard)
            .map(Some)
    }

    pub fn mutate_coordination_with_session_wait_observed<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        observe_phase: O,
    ) -> Result<Option<T>>
    where
        F: FnOnce(&Prism) -> Result<T>,
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        if !self.coordination_enabled {
            return Err(anyhow!(
                "coordination is disabled for this workspace session"
            ));
        }
        let Some(guard) = self.wait_lock_refresh_for_phase(
            "mutation.coordination.waitRefreshLock",
            "mutateCoordination",
            MUTATION_REFRESH_WAIT_TIMEOUT,
        )?
        else {
            return Ok(None);
        };
        self.mutate_coordination_with_session_guarded(session_id, mutate, observe_phase, guard)
            .map(Some)
    }

    pub fn curator_snapshot(&self) -> Result<CuratorSnapshot> {
        self.curator
            .as_ref()
            .map(CuratorHandle::snapshot)
            .transpose()
            .map(|snapshot| snapshot.unwrap_or_default())
    }

    #[cfg(test)]
    pub(crate) fn is_curator_snapshot_loaded(&self) -> bool {
        self.curator
            .as_ref()
            .map(|curator| {
                curator
                    .state
                    .lock()
                    .expect("curator state lock poisoned")
                    .loaded
            })
            .unwrap_or(false)
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
        let guard = self
            .refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        self.set_curator_proposal_state_guarded(
            job_id,
            proposal_index,
            disposition,
            task,
            note,
            output,
            guard,
        )
    }

    pub fn try_set_curator_proposal_state(
        &self,
        job_id: &CuratorJobId,
        proposal_index: usize,
        disposition: CuratorProposalDisposition,
        task: Option<TaskId>,
        note: Option<String>,
        output: Option<String>,
    ) -> Result<bool> {
        let Some(guard) = self.try_lock_refresh_for_mutation("setCuratorProposalState")? else {
            return Ok(false);
        };
        self.set_curator_proposal_state_guarded(
            job_id,
            proposal_index,
            disposition,
            task,
            note,
            output,
            guard,
        )?;
        Ok(true)
    }

    fn set_curator_proposal_state_guarded(
        &self,
        job_id: &CuratorJobId,
        proposal_index: usize,
        disposition: CuratorProposalDisposition,
        task: Option<TaskId>,
        note: Option<String>,
        output: Option<String>,
        _guard: MutexGuard<'_, ()>,
    ) -> Result<()> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let Some(curator) = &self.curator else {
            return Ok(());
        };
        let mut state = curator.state.lock().expect("curator state lock poisoned");
        state.ensure_loaded(&mut *store)?;
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
        if let Some(materializer) = self.checkpoint_materializer.as_ref() {
            materializer.enqueue_curator_snapshot(state.snapshot.clone())?;
        } else {
            store.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
                curator_snapshot: Some(state.snapshot.clone()),
                ..AuxiliaryPersistBatch::default()
            })?;
        }
        Ok(())
    }

    pub fn append_outcome(&self, event: OutcomeEvent) -> Result<EventId> {
        let _guard = self.lock_refresh_for_mutation("appendOutcome");
        self.append_outcome_guarded(event)
    }

    pub fn try_append_outcome(&self, event: OutcomeEvent) -> Result<Option<EventId>> {
        let Some(_guard) = self.try_lock_refresh_for_mutation("appendOutcome")? else {
            return Ok(None);
        };
        self.append_outcome_guarded(event).map(Some)
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
        memory_events: Vec<MemoryEvent>,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<EventId> {
        let _guard = self.lock_refresh_for_mutation("appendOutcomeWithAuxiliary");
        self.append_outcome_with_auxiliary_guarded(
            event,
            memory_events,
            episodic_snapshot,
            inference_snapshot,
        )
    }

    pub fn try_append_outcome_with_auxiliary(
        &self,
        event: OutcomeEvent,
        memory_events: Vec<MemoryEvent>,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<Option<EventId>> {
        let Some(_guard) = self.try_lock_refresh_for_mutation("appendOutcomeWithAuxiliary")? else {
            return Ok(None);
        };
        self.append_outcome_with_auxiliary_guarded(
            event,
            memory_events,
            episodic_snapshot,
            inference_snapshot,
        )
        .map(Some)
    }

    fn append_outcome_with_auxiliary_guarded(
        &self,
        event: OutcomeEvent,
        memory_events: Vec<MemoryEvent>,
        episodic_snapshot: Option<EpisodicMemorySnapshot>,
        inference_snapshot: Option<InferenceSnapshot>,
    ) -> Result<EventId> {
        let prism = self.prism_arc();
        let deltas = validation_deltas_for_event(&event, |node| prism.lineage_of(node));
        prism.apply_outcome_event_to_projections(&event);
        let persisted_event = event.clone();
        let id = prism.outcome_memory().store_event(event)?;
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .apply_outcome_event(&persisted_event);
        let mut store = Self::lock_store_for_mutation(
            &self.store,
            "mutation.waitWorkspaceStoreLock",
            "appendOutcomeWithAuxiliary",
        );
        let persist_started = Instant::now();
        let event_result =
            self.append_outcome_event_to_persistent_stores(&mut store, &persisted_event);
        let memory_result = if event_result.is_ok() && !memory_events.is_empty() {
            store.append_memory_events(&memory_events).map(|_| ())
        } else {
            Ok(())
        };
        let persist_result = event_result.and(memory_result);
        mutation_trace::record_phase(
            "mutation.appendOutcomePersist",
            json!({}),
            persist_started.elapsed(),
            persist_result.is_ok(),
            persist_result.as_ref().err().map(ToString::to_string),
        );
        persist_result?;
        if !deltas.is_empty() {
            let materialize_started = Instant::now();
            let enqueue_result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_validation_deltas(deltas.clone()))
                .unwrap_or_else(|| store.apply_validation_deltas(&deltas));
            mutation_trace::record_phase(
                "mutation.appendOutcomeScheduleMaterialization",
                json!({ "validationDeltaCount": deltas.len() }),
                materialize_started.elapsed(),
                enqueue_result.is_ok(),
                enqueue_result.as_ref().err().map(ToString::to_string),
            );
            if let Err(error) = enqueue_result {
                let fallback_started = Instant::now();
                let fallback_result = store.apply_validation_deltas(&deltas);
                mutation_trace::record_phase(
                    "mutation.appendOutcomeMaterializationFallback",
                    json!({ "validationDeltaCount": deltas.len() }),
                    fallback_started.elapsed(),
                    fallback_result.is_ok(),
                    fallback_result.as_ref().err().map(ToString::to_string),
                );
                if fallback_result.is_err() {
                    return Err(error);
                }
            }
        }
        if let Some(snapshot) = episodic_snapshot {
            let materialize_started = Instant::now();
            let enqueue_result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_episodic_snapshot(snapshot.clone()))
                .unwrap_or_else(|| store.save_episodic_snapshot(&snapshot));
            mutation_trace::record_phase(
                "mutation.appendOutcomeScheduleMaterialization",
                json!({ "kind": "episodicSnapshot" }),
                materialize_started.elapsed(),
                enqueue_result.is_ok(),
                enqueue_result.as_ref().err().map(ToString::to_string),
            );
            enqueue_result?;
        }
        if let Some(snapshot) = inference_snapshot {
            let materialize_started = Instant::now();
            let enqueue_result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_inference_snapshot(snapshot.clone()))
                .unwrap_or_else(|| store.save_inference_snapshot(&snapshot));
            mutation_trace::record_phase(
                "mutation.appendOutcomeScheduleMaterialization",
                json!({ "kind": "inferenceSnapshot" }),
                materialize_started.elapsed(),
                enqueue_result.is_ok(),
                enqueue_result.as_ref().err().map(ToString::to_string),
            );
            enqueue_result?;
        }
        if let Some(curator) = &self.curator {
            let curator_started = Instant::now();
            enqueue_curator_for_outcome_locked(curator, prism.as_ref(), &mut store, id.clone())?;
            mutation_trace::record_phase(
                "mutation.enqueueCurator",
                json!({}),
                curator_started.elapsed(),
                true,
                None,
            );
        }
        Ok(id)
    }

    pub fn flush_materializations(&self) -> Result<()> {
        if let Some(materializer) = &self.checkpoint_materializer {
            materializer.flush()?;
        }
        if let Some(materializer) = &self.shared_runtime_materializer {
            materializer.flush()?;
        }
        Ok(())
    }

    fn append_outcome_guarded(&self, event: OutcomeEvent) -> Result<EventId> {
        let prism = self.prism_arc();
        let deltas = validation_deltas_for_event(&event, |node| prism.lineage_of(node));
        prism.apply_outcome_event_to_projections(&event);
        let persisted_event = event.clone();
        let id = prism.outcome_memory().store_event(event)?;
        self.runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .apply_outcome_event(&persisted_event);
        let mut store = Self::lock_store_for_mutation(
            &self.store,
            "mutation.waitWorkspaceStoreLock",
            "appendOutcome",
        );
        let persist_started = Instant::now();
        let persist_result =
            self.append_outcome_event_to_persistent_stores(&mut store, &persisted_event);
        mutation_trace::record_phase(
            "mutation.appendOutcomePersist",
            json!({}),
            persist_started.elapsed(),
            persist_result.is_ok(),
            persist_result.as_ref().err().map(ToString::to_string),
        );
        persist_result?;
        if !deltas.is_empty() {
            let materialize_started = Instant::now();
            let enqueue_result = self
                .checkpoint_materializer
                .as_ref()
                .map(|materializer| materializer.enqueue_validation_deltas(deltas.clone()))
                .unwrap_or_else(|| store.apply_validation_deltas(&deltas));
            mutation_trace::record_phase(
                "mutation.appendOutcomeScheduleMaterialization",
                json!({ "validationDeltaCount": deltas.len() }),
                materialize_started.elapsed(),
                enqueue_result.is_ok(),
                enqueue_result.as_ref().err().map(ToString::to_string),
            );
            if let Err(error) = enqueue_result {
                let fallback_started = Instant::now();
                let fallback_result = store.apply_validation_deltas(&deltas);
                mutation_trace::record_phase(
                    "mutation.appendOutcomeMaterializationFallback",
                    json!({ "validationDeltaCount": deltas.len() }),
                    fallback_started.elapsed(),
                    fallback_result.is_ok(),
                    fallback_result.as_ref().err().map(ToString::to_string),
                );
                if fallback_result.is_err() {
                    return Err(error);
                }
            }
        }
        if let Some(curator) = &self.curator {
            let curator_started = Instant::now();
            enqueue_curator_for_outcome_locked(curator, prism.as_ref(), &mut store, id.clone())?;
            mutation_trace::record_phase(
                "mutation.enqueueCurator",
                json!({}),
                curator_started.elapsed(),
                true,
                None,
            );
        }
        Ok(id)
    }

    fn refresh_with_trigger(
        &self,
        trigger: ChangeTrigger,
        known_fingerprint: Option<WorkspaceTreeSnapshot>,
        dirty_paths_override: Option<Vec<PathBuf>>,
    ) -> Result<WorkspaceRefreshResult> {
        let curator = self.curator.as_ref().map(CuratorHandleRef::from);
        refresh_prism_snapshot(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            self.shared_runtime_store.as_ref(),
            self.shared_runtime.sqlite_path(),
            &self.refresh_lock,
            &self.refresh_state,
            &self.loaded_workspace_revision,
            &self.fs_snapshot,
            self.checkpoint_materializer.clone(),
            self.shared_runtime_materializer.clone(),
            self.coordination_enabled,
            curator.as_ref(),
            &self.observed_change_tracker,
            &self.worktree_principal_binding,
            trigger,
            known_fingerprint,
            dirty_paths_override,
        )
    }

    fn try_refresh_with_trigger(
        &self,
        trigger: ChangeTrigger,
        known_fingerprint: Option<WorkspaceTreeSnapshot>,
        dirty_paths_override: Option<Vec<PathBuf>>,
    ) -> Result<Option<WorkspaceRefreshResult>> {
        try_refresh_prism_snapshot(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            self.shared_runtime_store.as_ref(),
            self.shared_runtime.sqlite_path(),
            &self.refresh_lock,
            &self.refresh_state,
            &self.loaded_workspace_revision,
            &self.fs_snapshot,
            self.checkpoint_materializer.clone(),
            self.shared_runtime_materializer.clone(),
            self.coordination_enabled,
            self.curator.as_ref().map(CuratorHandleRef::from).as_ref(),
            &self.observed_change_tracker,
            &self.worktree_principal_binding,
            trigger,
            known_fingerprint,
            dirty_paths_override,
        )
    }
}

fn merge_patch_event_summaries(
    mut shared: Vec<PatchEventSummary>,
    local: Vec<PatchEventSummary>,
    limit: usize,
) -> Vec<PatchEventSummary> {
    let mut seen = shared
        .iter()
        .map(|summary| summary.event_id.clone())
        .collect::<HashSet<_>>();
    for summary in local {
        if seen.insert(summary.event_id.clone()) {
            shared.push(summary);
        }
    }
    shared.sort_by(|left, right| {
        right
            .ts
            .cmp(&left.ts)
            .then_with(|| left.event_id.0.cmp(&right.event_id.0))
    });
    if limit > 0 {
        shared.truncate(limit);
    }
    shared
}

fn merge_patch_file_summaries(
    shared: Vec<PatchFileSummary>,
    local: Vec<PatchFileSummary>,
    limit: usize,
) -> Vec<PatchFileSummary> {
    let mut merged = HashMap::<String, PatchFileSummary>::new();
    for summary in shared.into_iter().chain(local) {
        match merged.get_mut(&summary.path) {
            Some(existing)
                if (summary.ts, summary.event_id.0.as_str())
                    > (existing.ts, existing.event_id.0.as_str()) =>
            {
                *existing = summary;
            }
            None => {
                merged.insert(summary.path.clone(), summary);
            }
            _ => {}
        }
    }
    let mut rows = merged.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .ts
            .cmp(&left.ts)
            .then_with(|| left.event_id.0.cmp(&right.event_id.0))
    });
    if limit > 0 {
        rows.truncate(limit);
    }
    rows
}

impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            let _ = watch.stop.send(WatchMessage::Stop);
            let _ = watch.handle.join();
        }
        if let Some(watch) = self.protected_state_watch.take() {
            let _ = watch.stop.send(WatchMessage::Stop);
            let _ = watch.handle.join();
        }
        if let Err(error) = self.persist_flushed_observed_change_checkpoints(None, None, None, None)
        {
            warn!(error = %error, "failed to persist observed change checkpoints during workspace session shutdown");
        }
        if let Some(mut curator) = self.curator.take() {
            curator.stop();
        }
        if let Some(mut materializer) = self.checkpoint_materializer.take() {
            materializer.stop();
        }
        if let Some(mut materializer) = self.shared_runtime_materializer.take() {
            materializer.stop();
        }
    }
}

fn observed_change_checkpoint_trigger(
    trigger: ObservedChangeFlushTrigger,
) -> ObservedChangeCheckpointTrigger {
    match trigger {
        ObservedChangeFlushTrigger::MutationBoundary => {
            ObservedChangeCheckpointTrigger::MutationBoundary
        }
        ObservedChangeFlushTrigger::WorkTransition => {
            ObservedChangeCheckpointTrigger::WorkTransition
        }
        ObservedChangeFlushTrigger::Disconnect => ObservedChangeCheckpointTrigger::Disconnect,
        ObservedChangeFlushTrigger::ExplicitCheckpoint => {
            ObservedChangeCheckpointTrigger::ExplicitCheckpoint
        }
    }
}

fn default_observed_change_checkpoint_summary(change_set: &FlushedObservedChangeSet) -> String {
    let path_count = change_set.changed_paths.len();
    let noun = if path_count == 1 { "path" } else { "paths" };
    format!(
        "Checkpointed {path_count} changed {noun} for work {}",
        change_set.work.title
    )
}
