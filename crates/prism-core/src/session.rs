use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use prism_agent::{InferenceSnapshot, InferredEdgeRecord};
use prism_coordination::{
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_curator::{
    CuratorJobId, CuratorProposalDisposition, CuratorProposalState, CuratorSnapshot,
};
use prism_history::HistoryStore;
use prism_ir::{
    new_prefixed_id, AnchorRef, ChangeTrigger, CredentialId, EventActor, EventExecutionContext,
    EventId, EventMeta, LineageEvent, LineageId, ObservedChangeCheckpoint,
    ObservedChangeCheckpointEntry, ObservedChangeCheckpointTrigger, ObservedChangeSet,
    PrincipalActor, PrincipalAuthorityId, PrincipalId, PrincipalRegistrySnapshot, SessionId,
    TaskId, WorkContextSnapshot,
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

use crate::PrismPaths;
pub use prism_store::SnapshotRevisions as WorkspaceSnapshotRevisions;

pub(crate) const HOT_OUTCOME_HYDRATION_LIMIT: usize = 256;
const MUTATION_REFRESH_WAIT_TIMEOUT: Duration = Duration::from_millis(1500);
const MUTATION_REFRESH_RETRY_INTERVAL: Duration = Duration::from_millis(10);

use crate::admission::AdmissionBusyError;
use crate::checkpoint_materializer::{
    persist_coordination_materialization, CheckpointMaterializerHandle,
    CoordinationMaterialization,
};
use crate::concept_events::append_repo_concept_event;
use crate::concept_relation_events::append_repo_concept_relation_event;
use crate::contract_events::append_repo_contract_event;
use crate::coordination_authority_store::{
    configured_coordination_authority_store_provider,
    coordination_materialization_enabled_by_default, CoordinationAuthorityBackendKind,
    CoordinationAuthorityStamp,
};
use crate::coordination_authority_sync::{
    apply_service_backed_coordination_current_state, sync_coordination_authority_update,
};
use crate::coordination_materialized_store::{
    CoordinationMaterializedStore, SqliteCoordinationMaterializedStore,
};
use crate::coordination_persistence::{
    coordination_event_delta, CoordinationDerivedPersistenceMode,
    CoordinationPersistenceBackend,
};
use crate::coordination_reads::{CoordinationReadConsistency, CoordinationReadResult};
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
use crate::prism_doc::{
    bundle_prism_doc_export, export_repo_prism_doc_with_plan_state, PrismDocBundleFormat,
    PrismDocExportResult,
};
use crate::projection_hydration::persisted_projection_load_plan;
use crate::protected_state::runtime_sync::{
    build_runtime_state_with_materialized_coordination_state,
    load_repo_protected_knowledge_for_runtime, load_repo_protected_plan_state,
    sync_repo_protected_state,
};
use crate::published_knowledge::{
    validate_repo_concept_event, validate_repo_concept_relation_event,
    validate_repo_contract_event, validate_repo_memory_event, validate_repo_patch_event,
};
use crate::published_plans::HydratedCoordinationPlanState;
use crate::repo_patch_events::{
    append_repo_patch_event, load_repo_patch_events, merge_repo_patch_events_into_memory,
};
use crate::runtime_engine::WorkspaceRuntimePathRequest;
use crate::shared_runtime::{merge_memory_events, merged_projection_index};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::tracked_snapshot::tracked_snapshot_authority_active;
use crate::tracked_snapshot::{
    load_tracked_coordination_materialization_status, publish_context_from_coordination_events,
};
use crate::util::{current_timestamp, current_timestamp_millis};
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
use crate::worktree_mutator_slot::WorktreeMutatorSlotRecord;
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistedObservedChangeCheckpointResult {
    pub event_ids: Vec<EventId>,
    pub flushed_set_count: usize,
    pub changed_path_count: usize,
    pub entry_count: usize,
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
    pub canonical_snapshot_v2: CoordinationSnapshotV2,
    pub runtime_descriptors: Vec<prism_coordination::RuntimeDescriptor>,
}

impl From<HydratedCoordinationPlanState> for CoordinationPlanState {
    fn from(value: HydratedCoordinationPlanState) -> Self {
        Self {
            snapshot: value.snapshot,
            canonical_snapshot_v2: value.canonical_snapshot_v2,
            runtime_descriptors: value.runtime_descriptors,
        }
    }
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

pub(crate) struct WorkspaceSessionFullRuntime {
    pub(crate) repo_projection_sync_pending: Arc<AtomicBool>,
    pub(crate) repo_patch_provenance_sync_pending: Arc<AtomicBool>,
    pub(crate) refresh_lock: Arc<Mutex<()>>,
    pub(crate) refresh_state: Arc<WorkspaceRefreshState>,
    pub(crate) fs_snapshot: Arc<Mutex<WorkspaceTreeSnapshot>>,
    pub(crate) watch: Option<WatchHandle>,
    pub(crate) protected_state_watch: Option<WatchHandle>,
    pub(crate) coordination_authority_watch: Option<WatchHandle>,
    pub(crate) curator: Option<CuratorHandle>,
    pub(crate) checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    pub(crate) observed_change_tracker: SharedObservedChangeTracker,
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
    pub(crate) principal_registry: Arc<RwLock<PrincipalRegistrySnapshot>>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) coordination_runtime_revision: Arc<AtomicU64>,
    pub(crate) full_runtime: Option<WorkspaceSessionFullRuntime>,
    pub(crate) coordination_enabled: bool,
    pub(crate) worktree_mutator_slot: Arc<Mutex<Option<WorktreeMutatorSlotRecord>>>,
    pub(crate) worktree_principal_binding: Arc<Mutex<Option<BoundWorktreePrincipal>>>,
}

impl WorkspaceSession {
    pub(crate) fn full_runtime_state(&self) -> Option<&WorkspaceSessionFullRuntime> {
        self.full_runtime.as_ref()
    }

    fn full_runtime_state_mut(&mut self) -> Option<&mut WorkspaceSessionFullRuntime> {
        self.full_runtime.as_mut()
    }

    #[cfg(test)]
    pub(crate) fn refresh_lock_handle(&self) -> Option<&Arc<Mutex<()>>> {
        self.full_runtime_state()
            .map(|full_runtime| &full_runtime.refresh_lock)
    }

    #[cfg(test)]
    pub(crate) fn repo_projection_sync_pending_handle(&self) -> Option<&Arc<AtomicBool>> {
        self.full_runtime_state()
            .map(|full_runtime| &full_runtime.repo_projection_sync_pending)
    }

    pub fn coordination_only_runtime(&self) -> bool {
        matches!(
            prism_ir::PrismRuntimeMode::from_capabilities(self.prism_arc().runtime_capabilities()),
            Some(prism_ir::PrismRuntimeMode::CoordinationOnly)
        )
    }

    pub fn bind_active_work_context(&self, work: ActiveWorkContextBinding) {
        let Some(full_runtime) = self.full_runtime_state() else {
            return;
        };
        full_runtime
            .observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .set_active_work(work);
    }

    pub fn clear_active_work_context(&self) {
        let Some(full_runtime) = self.full_runtime_state() else {
            return;
        };
        full_runtime
            .observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .clear_active_work();
    }

    pub fn active_work_context(&self) -> Option<ActiveWorkContextBinding> {
        self.full_runtime_state()?
            .observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .active_work()
    }

    pub fn flush_observed_changes(&self, trigger: ObservedChangeFlushTrigger) -> usize {
        let Some(full_runtime) = self.full_runtime_state() else {
            return 0;
        };
        full_runtime
            .observed_change_tracker
            .lock()
            .expect("observed change tracker lock poisoned")
            .flush(trigger)
    }

    pub fn take_flushed_observed_changes(&self) -> Vec<FlushedObservedChangeSet> {
        let Some(full_runtime) = self.full_runtime_state() else {
            return Vec::new();
        };
        full_runtime
            .observed_change_tracker
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
        Ok(self
            .persist_flushed_observed_change_checkpoints_detailed(
                session_id,
                request_id,
                credential_id,
                summary,
            )?
            .event_ids)
    }

    pub fn persist_flushed_observed_change_checkpoints_detailed(
        &self,
        session_id: Option<&SessionId>,
        request_id: Option<String>,
        credential_id: Option<&CredentialId>,
        summary: Option<&str>,
    ) -> Result<PersistedObservedChangeCheckpointResult> {
        let flushed = self.take_flushed_observed_changes();
        if flushed.is_empty() {
            return Ok(PersistedObservedChangeCheckpointResult::default());
        }
        let normalized_summary = summary
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let flushed_set_count = flushed.len();
        let changed_path_count = flushed
            .iter()
            .map(|change_set| change_set.changed_paths.len())
            .sum();
        let entry_count = flushed
            .iter()
            .map(|change_set| change_set.entries.len())
            .sum();
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
                summary: change_set.work.summary.clone(),
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
        Ok(PersistedObservedChangeCheckpointResult {
            event_ids,
            flushed_set_count,
            changed_path_count,
            entry_count,
        })
    }

    fn lock_refresh_for_mutation(&self, reason: &'static str) -> MutexGuard<'_, ()> {
        let wait_started = Instant::now();
        let guard = self
            .full_runtime_state()
            .expect("workspace refresh lock unavailable in coordination-only mode")
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
        let refresh_lock = &self
            .full_runtime_state()
            .expect("workspace refresh lock unavailable in coordination-only mode")
            .refresh_lock;
        match refresh_lock.try_lock() {
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
        let refresh_lock = &self
            .full_runtime_state()
            .expect("workspace refresh lock unavailable in coordination-only mode")
            .refresh_lock;
        loop {
            match refresh_lock.try_lock() {
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
        Ok(local)
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
        Ok(local)
    }

    pub fn prism(&self) -> Arc<Prism> {
        self.prism_arc()
    }

    pub fn prism_arc(&self) -> Arc<Prism> {
        let prism = self
            .published_generation
            .read()
            .expect("workspace published generation lock poisoned")
            .prism_arc();
        Self::attach_cold_query_backends(prism.as_ref(), &self.cold_query_store);
        prism
    }

    pub fn publish_pending_repo_patch_provenance_for_active_work(&self) -> Result<Vec<EventId>> {
        if self.coordination_only_runtime() {
            return Ok(Vec::new());
        }
        let Some(bound_principal) = self.bound_worktree_principal() else {
            return Ok(Vec::new());
        };
        let Some(active_work) = self.active_work_context() else {
            return Ok(Vec::new());
        };
        publish_pending_repo_patch_provenance(
            &self.root,
            self.prism_arc(),
            Arc::clone(&self.runtime_state),
            bound_principal,
            active_work,
        )
    }

    pub fn schedule_pending_repo_patch_provenance_for_active_work(&self) {
        if self.coordination_only_runtime() {
            return;
        }
        let Some(bound_principal) = self.bound_worktree_principal() else {
            return;
        };
        let Some(active_work) = self.active_work_context() else {
            return;
        };
        let pending_sync = &self
            .full_runtime_state()
            .expect("repo patch provenance state unavailable in coordination-only mode")
            .repo_patch_provenance_sync_pending;
        if pending_sync
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let root = self.root.clone();
        let prism = self.prism_arc();
        let runtime_state = Arc::clone(&self.runtime_state);
        let pending = Arc::clone(pending_sync);
        std::thread::Builder::new()
            .name("repo-patch-provenance-sync".to_string())
            .spawn(move || {
                if let Err(error) = publish_pending_repo_patch_provenance(
                    &root,
                    prism,
                    runtime_state,
                    bound_principal,
                    active_work,
                ) {
                    warn!(
                        root = %root.display(),
                        error = %error,
                        "failed to publish pending repo patch provenance in background"
                    );
                }
                pending.store(false, Ordering::Release);
            })
            .expect("failed to spawn repo patch provenance sync worker");
    }

    pub(crate) fn attach_cold_query_backends(prism: &Prism, store: &Arc<Mutex<SqliteStore>>) {
        if matches!(
            prism_ir::PrismRuntimeMode::from_capabilities(prism.runtime_capabilities()),
            Some(prism_ir::PrismRuntimeMode::CoordinationOnly)
        ) {
            return;
        }
        prism.set_history_backend(Some(Arc::new(StoreHistoryReadBackend::new(Arc::clone(
            store,
        )))));
        prism.set_outcome_backend(Some(Arc::new(StoreOutcomeReadBackend::new(Arc::clone(
            store,
        )))));
    }

    fn publish_runtime_state(
        &self,
        runtime_state: WorkspaceRuntimeState,
        local_workspace_revision: u64,
        workspace_revision: u64,
        coordination_context: Option<prism_store::CoordinationPersistContext>,
        intent_override: Option<prism_projections::IntentIndex>,
    ) {
        let next = runtime_state.publish_generation_with_intent(
            prism_ir::WorkspaceRevision {
                graph_version: local_workspace_revision,
                git_commit: None,
            },
            coordination_context,
            intent_override,
        );
        Self::attach_cold_query_backends(next.prism_arc().as_ref(), &self.cold_query_store);
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

    pub fn export_prism_docs(
        &self,
        output_root: &Path,
        bundle: Option<PrismDocBundleFormat>,
    ) -> Result<PrismDocExportResult> {
        let prism = self.prism_arc();
        let concepts = prism.curated_concepts_snapshot();
        let relations = prism.concept_relations_snapshot();
        let contracts = prism.curated_contracts();
        let plan_state = if self.coordination_enabled {
            self.load_coordination_plan_state()?
                .map(|value| HydratedCoordinationPlanState {
                    snapshot: value.snapshot,
                    canonical_snapshot_v2: value.canonical_snapshot_v2,
                    runtime_descriptors: value.runtime_descriptors,
                })
        } else {
            None
        };
        let sync = export_repo_prism_doc_with_plan_state(
            &self.root,
            output_root,
            &concepts,
            &relations,
            &contracts,
            plan_state,
        )?;
        let bundle = bundle
            .map(|format| bundle_prism_doc_export(output_root, &sync.files, format))
            .transpose()?;
        Ok(PrismDocExportResult { sync, bundle })
    }

    pub fn refresh_fs(&self) -> Result<Vec<ObservedChangeSet>> {
        Ok(self.refresh_fs_with_status()?.observed)
    }

    pub fn refresh_fs_with_status(&self) -> Result<WorkspaceFsRefreshOutcome> {
        let outcome = self.refresh_fs_with_scoped_paths(None)?;
        if outcome.status != FsRefreshStatus::Clean {
            self.schedule_pending_repo_patch_provenance_for_active_work();
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
            self.schedule_pending_repo_patch_provenance_for_active_work();
        }
        Ok(outcome)
    }

    fn refresh_fs_with_scoped_paths(
        &self,
        dirty_paths_override: Option<Vec<PathBuf>>,
    ) -> Result<WorkspaceFsRefreshOutcome> {
        let Some(full_runtime) = self.full_runtime_state() else {
            return Ok(WorkspaceFsRefreshOutcome {
                status: FsRefreshStatus::Clean,
                observed: Vec::new(),
                breakdown: WorkspaceRefreshBreakdown::default(),
            });
        };
        let now_ms = current_timestamp_millis();
        let fs_fallback_due = full_runtime.refresh_state.should_run_fallback_check(now_ms);
        let has_scoped_override = dirty_paths_override
            .as_ref()
            .is_some_and(|dirty_paths| !dirty_paths.is_empty());
        if !full_runtime.refresh_state.needs_refresh() && !fs_fallback_due && !has_scoped_override {
            return Ok(WorkspaceFsRefreshOutcome {
                status: FsRefreshStatus::Clean,
                observed: Vec::new(),
                breakdown: WorkspaceRefreshBreakdown::default(),
            });
        }
        let dirty_paths = dirty_paths_override
            .clone()
            .unwrap_or_else(|| full_runtime.refresh_state.dirty_paths_snapshot());
        let refreshed = if (full_runtime.refresh_state.needs_refresh() || has_scoped_override)
            && !dirty_paths.is_empty()
        {
            self.refresh_with_trigger(ChangeTrigger::FsWatch, None, Some(dirty_paths))?
        } else {
            let known_snapshot = self
                .full_runtime_state()
                .expect("workspace snapshot unavailable in coordination-only mode")
                .fs_snapshot
                .lock()
                .expect("workspace tree snapshot lock poisoned")
                .clone();
            let plan = plan_full_refresh(&self.root, &known_snapshot)?;
            if !full_runtime.refresh_state.needs_refresh() && plan.delta.is_empty() {
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
        let Some(full_runtime) = self.full_runtime_state() else {
            return Ok(FsRefreshStatus::Clean);
        };
        let needs_refresh = full_runtime.refresh_state.needs_refresh();
        let now_ms = current_timestamp_millis();
        let fs_fallback_due = full_runtime.refresh_state.should_run_fallback_check(now_ms);
        if !needs_refresh && !fs_fallback_due {
            return Ok(FsRefreshStatus::Clean);
        }
        let dirty_paths = full_runtime.refresh_state.dirty_paths_snapshot();
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
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.needs_refresh())
            .unwrap_or(false)
    }

    pub fn pending_refresh_paths(&self) -> Vec<PathBuf> {
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.dirty_paths_snapshot())
            .unwrap_or_default()
    }

    pub fn mark_fs_dirty_paths<I>(&self, paths: I) -> u64
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let Some(full_runtime) = self.full_runtime_state() else {
            let _ = paths.into_iter().count();
            return 0;
        };
        full_runtime.refresh_state.mark_fs_dirty_paths(paths)
    }

    pub fn pending_refresh_path_requests(&self) -> Vec<WorkspaceRuntimePathRequest> {
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.dirty_path_requests_snapshot())
            .unwrap_or_default()
    }

    pub fn scoped_refresh_paths_for_requests(
        &self,
        requests: &[WorkspaceRuntimePathRequest],
    ) -> Vec<PathBuf> {
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.scoped_dirty_paths_for_requests(requests))
            .unwrap_or_else(|| {
                let _ = requests;
                Vec::new()
            })
    }

    pub fn observed_fs_revision(&self) -> u64 {
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.observed_fs_revision())
            .unwrap_or(0)
    }

    pub fn applied_fs_revision(&self) -> u64 {
        self.full_runtime_state()
            .map(|full_runtime| full_runtime.refresh_state.applied_fs_revision())
            .unwrap_or(0)
    }

    pub fn last_refresh(&self) -> Option<WorkspaceLastRefresh> {
        self.full_runtime_state()
            .and_then(|full_runtime| full_runtime.refresh_state.last_refresh())
    }

    pub fn is_fallback_check_due_now(&self) -> bool {
        self.full_runtime_state()
            .map(|full_runtime| {
                full_runtime
                    .refresh_state
                    .fallback_check_due(current_timestamp_millis())
            })
            .unwrap_or(false)
    }

    pub fn workspace_materialization_summary(&self) -> WorkspaceMaterializationSummary {
        if self.coordination_only_runtime() {
            return WorkspaceMaterializationSummary {
                known_files: 0,
                known_directories: 0,
                materialized_files: 0,
                materialized_nodes: 0,
                materialized_edges: 0,
                boundaries: Vec::new(),
            };
        }
        let snapshot = self
            .full_runtime_state()
            .expect("workspace snapshot unavailable in coordination-only mode")
            .fs_snapshot
            .lock()
            .expect("workspace fs snapshot lock poisoned")
            .clone();
        let prism = self.prism_arc();
        summarize_workspace_materialization(self.root(), &snapshot, prism.graph())
    }

    pub fn workspace_materialization_coverage(&self) -> WorkspaceMaterializationCoverage {
        if self.coordination_only_runtime() {
            return WorkspaceMaterializationCoverage {
                known_files: 0,
                known_directories: 0,
                materialized_files: 0,
                materialized_nodes: 0,
                materialized_edges: 0,
            };
        }
        let snapshot = self
            .full_runtime_state()
            .expect("workspace snapshot unavailable in coordination-only mode")
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
            .full_runtime_state()
            .expect("workspace snapshot unavailable in coordination-only mode")
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
            let placeholder = WorkspaceRuntimeState::placeholder_with_layout_and_capabilities(
                state.layout(),
                current_prism.runtime_capabilities(),
            );
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
        let mut indexer = WorkspaceIndexer::with_runtime_state_stores_and_options(
            &self.root,
            reopened_store,
            runtime_state,
            next_layout.clone(),
            layout_refresh_required
                || next_layout.workspace_manifest != current_layout.workspace_manifest
                || next_layout.packages.len() != current_layout.packages.len(),
            Some(cached_snapshot.clone()),
            self.full_runtime_state()
                .expect("checkpoint materializer state unavailable in coordination-only mode")
                .checkpoint_materializer
                .clone(),
            crate::WorkspaceSessionOptions {
                runtime_mode: prism_ir::PrismRuntimeMode::from_capabilities(
                    current_prism.runtime_capabilities(),
                )
                .unwrap_or(prism_ir::PrismRuntimeMode::Full),
                shared_runtime: self.shared_runtime.clone(),
                hydrate_persisted_projections: false,
                hydrate_persisted_co_change: true,
            },
        )?;
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
            let fallback_state = if self.coordination_enabled {
                let mut store = self.store.lock().expect("workspace store lock poisoned");
                build_runtime_state_with_materialized_coordination_state(
                    &self.root,
                    &mut *store,
                    current_prism.as_ref(),
                    next_layout,
                )?
            } else {
                let mut fallback_graph = Graph::from_snapshot(current_prism.graph().snapshot());
                fallback_graph.bind_workspace_root(&self.root);
                WorkspaceRuntimeState::new(
                    next_layout,
                    fallback_graph,
                    HistoryStore::from_snapshot(current_prism.history_snapshot()),
                    OutcomeMemory::from_snapshot(current_prism.outcome_snapshot()),
                    Default::default(),
                    Vec::new(),
                    ProjectionIndex::from_snapshot(current_prism.projection_snapshot()),
                    current_prism.runtime_capabilities(),
                )
            };
            *self
                .runtime_state
                .lock()
                .expect("workspace runtime state lock poisoned") = fallback_state;
            return Err(error);
        }
        let observed_changes = index_result.expect("deepening index result checked above");

        let local_workspace_revision = indexer.store.workspace_revision()?;
        let workspace_revision = local_workspace_revision;
        let next_state = indexer.into_runtime_state();
        let next_intent = current_prism
            .updated_intent_for_observed_changes(Arc::as_ref(&next_state.graph), &observed_changes);
        self.publish_runtime_state(
            next_state,
            local_workspace_revision,
            workspace_revision,
            coordination_context,
            Some(next_intent),
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
        if self.coordination_only_runtime() {
            let _ = paths.into_iter().count();
            return Ok(false);
        }
        let started = Instant::now();
        let guard = self.lock_refresh_for_mutation("ensurePathsDeep");
        self.ensure_paths_deep_with_guard(guard, paths, started)
    }

    pub fn try_ensure_paths_deep<I>(&self, paths: I) -> Result<Option<bool>>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        if self.coordination_only_runtime() {
            let _ = paths.into_iter().count();
            return Ok(Some(false));
        }
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
        let result = self
            .full_runtime_state()
            .expect("checkpoint materializer state unavailable in coordination-only mode")
            .checkpoint_materializer
            .as_ref()
            .map(|materializer| materializer.enqueue_outcome_snapshot(snapshot.clone()))
            .unwrap_or_else(|| {
                let mut store = Self::lock_store_for_mutation(
                    &self.store,
                    "mutation.waitWorkspaceStoreLock",
                    "persistOutcomes",
                );
                store.save_outcome_snapshot(&snapshot)
            });
        mutation_trace::record_phase(
            "mutation.persistOutcomesSchedule",
            json!({ "target": "workspace" }),
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
        store.append_outcome_events(std::slice::from_ref(event), &[])?;
        Ok(())
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
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        prism_store::MaterializationStore::load_episodic_snapshot(&mut *store)
    }

    pub fn load_episodic_snapshot_for_runtime(&self) -> Result<Option<EpisodicMemorySnapshot>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_episodic_snapshot()
    }

    #[allow(dead_code)]
    pub(crate) fn try_recover_runtime_from_persisted_state(&self) -> Result<bool> {
        let Ok(guard) = self
            .full_runtime_state()
            .expect("workspace refresh lock unavailable in coordination-only mode")
            .refresh_lock
            .try_lock()
        else {
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
        let runtime_capabilities = self.prism_arc().runtime_capabilities();
        let knowledge_storage = runtime_capabilities.knowledge_storage_enabled();
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        let local_workspace_revision = store.workspace_revision()?;
        sync_repo_protected_state(&self.root, &mut *store, runtime_capabilities)?;
        let workspace_revision = local_workspace_revision;
        let layout = discover_layout(&self.root)?;
        let mut graph = if knowledge_storage {
            store.load_graph()?.unwrap_or_default()
        } else {
            Graph::default()
        };
        if knowledge_storage {
            graph.bind_workspace_root(&self.root);
            sync_root_nodes(&mut graph, &layout);
            resolve_graph_edges(&mut graph, None);
        }
        let projection_metadata = if knowledge_storage {
            store.load_projection_materialization_metadata()?
        } else {
            Default::default()
        };
        let local_projection_snapshot = if knowledge_storage {
            if self.hydrate_persisted_projections {
                store.load_projection_snapshot()?
            } else if self.hydrate_persisted_co_change {
                store.load_projection_snapshot()?
            } else {
                store.load_projection_snapshot_without_co_change()?
            }
        } else {
            None
        };
        let load_plan = persisted_projection_load_plan(
            projection_metadata,
            self.hydrate_persisted_projections && knowledge_storage,
            self.hydrate_persisted_co_change && knowledge_storage,
        );
        let mut history = if knowledge_storage {
            store
                .load_history_snapshot_with_options(load_plan.load_history_events)?
                .map(HistoryStore::from_snapshot)
                .unwrap_or_else(HistoryStore::new)
        } else {
            HistoryStore::new()
        };
        if knowledge_storage {
            history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        }
        let outcomes = if knowledge_storage {
            let outcomes = if load_plan.load_full_outcomes {
                store.load_outcome_snapshot()?
            } else {
                store.load_recent_outcome_snapshot(HOT_OUTCOME_HYDRATION_LIMIT)?
            }
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
            merge_repo_patch_events_into_memory(&self.root, &outcomes)?;
            outcomes
        } else {
            OutcomeMemory::new()
        };
        let plan_state = if self.coordination_enabled {
            load_repo_protected_plan_state(&self.root, &mut *store)?
        } else {
            None
        };
        let coordination_snapshot = plan_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let repo_knowledge =
            load_repo_protected_knowledge_for_runtime(&self.root, runtime_capabilities)?;
        let protected_knowledge_work = if knowledge_storage {
            protected_knowledge_recovery_work(&repo_knowledge)?
        } else {
            WorkspaceRefreshWork::default()
        };
        let projections = if knowledge_storage {
            merged_projection_index(
                local_projection_snapshot,
                None,
                repo_knowledge.curated_concepts,
                repo_knowledge.curated_contracts,
                repo_knowledge.concept_relations,
                &history.snapshot(),
                &outcomes.snapshot(),
            )
        } else {
            ProjectionIndex::default()
        };
        let recovery_work = workspace_recovery_work(
            &graph,
            &history,
            &outcomes,
            protected_knowledge_work,
            &coordination_snapshot,
        )?;
        drop(store);

        let runtime_state = WorkspaceRuntimeState::new_with_coordination_state(
            discover_layout(&self.root)?,
            graph,
            history,
            outcomes,
            coordination_snapshot,
            plan_state
                .as_ref()
                .map(|state| state.canonical_snapshot_v2.clone())
                .unwrap_or_default(),
            plan_state
                .as_ref()
                .map(|state| state.runtime_descriptors.clone())
                .unwrap_or_default(),
            projections,
            self.prism_arc().runtime_capabilities(),
        );
        self.publish_runtime_state(
            runtime_state,
            local_workspace_revision,
            workspace_revision,
            Some(coordination_persist_context_for_root(&self.root, None)),
            None,
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

    pub fn record_runtime_refresh_observation(&self, path: &str, duration_ms: u64) {
        if self.coordination_only_runtime() {
            let _ = (path, duration_ms);
            return;
        }
        self.full_runtime_state()
            .expect("workspace refresh state unavailable in coordination-only mode")
            .refresh_state
            .record_runtime_refresh_observation(
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
        if self.coordination_only_runtime() {
            let _ = (path, duration_ms, work);
            return;
        }
        self.full_runtime_state()
            .expect("workspace refresh state unavailable in coordination-only mode")
            .refresh_state
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
        revisions.coordination = self.coordination_runtime_revision_value(revisions.coordination);
        Ok(revisions)
    }

    pub fn snapshot_revisions_for_runtime(&self) -> Result<WorkspaceSnapshotRevisions> {
        let mut revisions = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .snapshot_revisions()?;
        // Runtime sync freshness must track the worktree-local runtime cache and the
        // live in-memory coordination revision, not the shared runtime sqlite backend.
        revisions.coordination = self.coordination_runtime_revision_value(revisions.coordination);
        Ok(revisions)
    }

    pub fn episodic_revision(&self) -> Result<u64> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .episodic_revision()
    }

    pub fn persist_episodic(&self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        let persist_started = Instant::now();
        let result = self
            .full_runtime_state()
            .expect("checkpoint materializer state unavailable in coordination-only mode")
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
        Ok(())
    }

    pub fn append_memory_event(&self, event: MemoryEvent) -> Result<()> {
        if event.scope == prism_memory::MemoryScope::Repo {
            validate_repo_memory_event(&event)?;
            append_repo_memory_event(&self.root, &event)?;
        }
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
        Ok(())
    }

    pub fn memory_events(&self, query: &MemoryEventQuery) -> Result<Vec<MemoryEvent>> {
        let local_events = {
            let mut store = self.store.lock().expect("workspace store lock poisoned");
            store.load_memory_events()?
        };
        Ok(filter_memory_events(
            merge_memory_events(local_events, Vec::new()),
            query,
        ))
    }

    pub fn load_principal_registry(&self) -> Result<Option<PrincipalRegistrySnapshot>> {
        let snapshot = self
            .principal_registry
            .read()
            .expect("principal registry lock poisoned")
            .clone();
        if snapshot.principals.is_empty() && snapshot.credentials.is_empty() {
            Ok(None)
        } else {
            Ok(Some(snapshot))
        }
    }

    pub fn persist_principal_registry(&self, snapshot: &PrincipalRegistrySnapshot) -> Result<()> {
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        prism_store::MaterializationStore::save_principal_registry_snapshot(&mut *store, snapshot)?;
        *self
            .principal_registry
            .write()
            .expect("principal registry lock poisoned") = snapshot.clone();
        Ok(())
    }

    pub(crate) fn authenticate_principal_credential_cached(
        &self,
        credential_id: &CredentialId,
        principal_token: &str,
    ) -> Result<crate::AuthenticatedPrincipal> {
        let mut repair_attempted = false;
        loop {
            let mut snapshot = self
                .principal_registry
                .write()
                .expect("principal registry lock poisoned");
            if snapshot.principals.is_empty() && snapshot.credentials.is_empty() {
                drop(snapshot);
                if repair_attempted {
                    return Err(anyhow!("principal registry is not initialized"));
                }
                repair_attempted = true;
                let mut store = self.store.lock().expect("workspace store lock poisoned");
                if let Some(snapshot) =
                    crate::ensure_local_principal_registry_snapshot(&self.root, &mut *store)?
                {
                    *self
                        .principal_registry
                        .write()
                        .expect("principal registry lock poisoned") = snapshot;
                    continue;
                }
                return Err(anyhow!("principal registry is not initialized"));
            }
            return crate::principal_registry::authenticate_principal_credential_without_persist(
                &mut snapshot,
                credential_id,
                principal_token,
            );
        }
    }

    pub fn event_execution_context(
        &self,
        session_id: Option<&SessionId>,
        request_id: Option<String>,
        credential_id: Option<&CredentialId>,
    ) -> EventExecutionContext {
        let context = crate::PrismPaths::for_workspace_root(&self.root)
            .map(|paths| prism_store::CoordinationPersistContext {
                repo_id: paths.identity().repo_id.clone(),
                worktree_id: paths.identity().worktree_id.clone(),
                branch_ref: paths.identity().branch_ref.clone(),
                session_id: session_id.map(|session_id| session_id.0.to_string()),
                instance_id: Some(paths.identity().instance_id.clone()),
            })
            .unwrap_or_else(|_| coordination_persist_context_for_root(&self.root, session_id));
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
        let guard = self.lock_refresh_for_mutation("appendConceptEvent");
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
        if event.concept.scope == prism_projections::ConceptScope::Repo {
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
            self.store
                .lock()
                .expect("projection target store lock poisoned")
                .upsert_projection_concept(&concept)?;
        }
        Ok(())
    }

    pub fn append_contract_event(&self, event: ContractEvent) -> Result<()> {
        let guard = self.lock_refresh_for_mutation("appendContractEvent");
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
        if event.contract.scope == prism_projections::ContractScope::Repo {
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
        Ok(())
    }

    pub fn append_concept_relation_event(&self, event: ConceptRelationEvent) -> Result<()> {
        let guard = self.lock_refresh_for_mutation("appendConceptRelationEvent");
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
        if event.relation.scope == prism_projections::ConceptScope::Repo {
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
            self.store
                .lock()
                .expect("projection target store lock poisoned")
                .upsert_projection_concept_relation(&relation)?;
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
        let persisted_revision = {
            let store = self.store.lock().expect("workspace store lock poisoned");
            Store::coordination_revision(&store)?
        };
        Ok(self
            .read_coordination_authority_stamp()?
            .and_then(|authority| authority_revision_from_stamp(&authority))
            .unwrap_or(persisted_revision))
    }

    pub fn coordination_runtime_revision(&self) -> Result<u64> {
        Ok(self.coordination_runtime_revision_value(self.coordination_revision()?))
    }

    fn coordination_runtime_revision_value(&self, persisted_revision: u64) -> u64 {
        if !self.coordination_enabled {
            return 0;
        }
        persisted_revision.max(self.coordination_runtime_revision.load(Ordering::Relaxed))
    }

    pub fn persist_inference(&self, snapshot: &InferenceSnapshot) -> Result<()> {
        let persist_started = Instant::now();
        let result = self
            .full_runtime_state()
            .expect("checkpoint materializer state unavailable in coordination-only mode")
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

    pub fn load_coordination_snapshot_v2(&self) -> Result<Option<CoordinationSnapshotV2>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        Ok(self
            .read_coordination_snapshot_v2_with_consistency(CoordinationReadConsistency::Eventual)?
            .into_value())
    }

    pub(crate) fn read_legacy_coordination_snapshot_with_consistency(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadResult<CoordinationSnapshot>> {
        let materialization_enabled = self.coordination_materialization_enabled()?;
        self.read_coordination_with_consistency(
            consistency,
            || {
                if materialization_enabled {
                    if let Some(snapshot) = SqliteCoordinationMaterializedStore::new(&self.root)
                        .read_legacy_snapshot()?
                        .value
                    {
                        return Ok(Some(snapshot));
                    }
                }
                Ok(self
                    .read_coordination_current_state_from_authority()?
                    .map(|state| state.snapshot))
            },
            || crate::published_plans::load_authoritative_coordination_snapshot(&self.root),
        )
    }

    pub fn read_coordination_snapshot_v2_with_consistency(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadResult<CoordinationSnapshotV2>> {
        let materialization_enabled = self.coordination_materialization_enabled()?;
        self.read_coordination_with_consistency(
            consistency,
            || {
                if materialization_enabled {
                    if let Some(snapshot_v2) = SqliteCoordinationMaterializedStore::new(&self.root)
                        .read_snapshot_v2()?
                        .value
                    {
                        return Ok(Some(snapshot_v2));
                    }
                }
                Ok(self
                    .read_coordination_current_state_from_authority()?
                    .map(|state| state.canonical_snapshot_v2))
            },
            || crate::published_plans::load_authoritative_coordination_snapshot_v2(&self.root),
        )
    }

    pub fn load_coordination_read_model(&self) -> Result<Option<CoordinationReadModel>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        let authoritative_revision = self
            .read_coordination_authority_stamp()?
            .and_then(|authority| authority_revision_from_stamp(&authority));
        if self.coordination_materialization_enabled()? {
            let materialized_store = SqliteCoordinationMaterializedStore::new(&self.root);
            if let Some(model) = materialized_store.read_effective_read_model()?.value {
                return Ok(Some(model));
            }
            return Ok(self
                .read_coordination_current_state_from_authority()?
                .map(|state| {
                    let mut model =
                        prism_coordination::coordination_read_model_from_snapshot(&state.snapshot);
                    if let Some(revision) = authoritative_revision {
                        model.revision = revision;
                    }
                    model
                }));
        }
        Ok(self
            .read_coordination_current_state_from_authority()?
            .map(|state| {
                let mut model =
                    prism_coordination::coordination_read_model_from_snapshot(&state.snapshot);
                if let Some(revision) = authoritative_revision {
                    model.revision = revision;
                }
                model
            }))
    }

    pub fn load_coordination_queue_read_model(&self) -> Result<Option<CoordinationQueueReadModel>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        let authoritative_revision = self
            .read_coordination_authority_stamp()?
            .and_then(|authority| authority_revision_from_stamp(&authority));
        if self.coordination_materialization_enabled()? {
            let materialized_store = SqliteCoordinationMaterializedStore::new(&self.root);
            if let Some(model) = materialized_store.read_effective_queue_read_model()?.value {
                return Ok(Some(model));
            }
            return Ok(self
                .read_coordination_current_state_from_authority()?
                .map(|state| {
                    let mut model = prism_coordination::coordination_queue_read_model_from_snapshot(
                        &state.snapshot,
                    );
                    if let Some(revision) = authoritative_revision {
                        model.revision = revision;
                    }
                    model
                }));
        }
        Ok(self
            .read_coordination_current_state_from_authority()?
            .map(|state| {
                let mut model =
                    prism_coordination::coordination_queue_read_model_from_snapshot(&state.snapshot);
                if let Some(revision) = authoritative_revision {
                    model.revision = revision;
                }
                model
            }))
    }

    pub fn load_coordination_startup_checkpoint_revision(&self) -> Result<Option<u64>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        if !self.coordination_materialization_enabled()? {
            let provider = configured_coordination_authority_store_provider(&self.root)?;
            if matches!(
                provider.config(),
                crate::CoordinationAuthorityBackendConfig::Sqlite { .. }
            ) {
                let mut store = prism_store::SqliteStore::open(
                    PrismPaths::for_workspace_root(&self.root)?.coordination_authority_db_path()?,
                )?;
                return prism_store::CoordinationCheckpointStore::load_coordination_startup_checkpoint_revision(&mut store);
            }
            return Ok(self
                .read_coordination_authority_stamp()?
                .and_then(|authority| authority_revision_from_stamp(&authority)));
        }
        let store = SqliteCoordinationMaterializedStore::new(&self.root);
        let metadata_revision = store
            .read_metadata()?
            .startup_checkpoint_coordination_revision;
        if metadata_revision.is_some() {
            return Ok(metadata_revision);
        }
        Ok(store
            .read_startup_checkpoint()?
            .value
            .map(|checkpoint| checkpoint.coordination_revision))
    }

    pub fn load_tracked_coordination_snapshot_revision(&self) -> Result<Option<u64>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        Ok(
            load_tracked_coordination_materialization_status(&self.root)?
                .map(|status| status.coordination_revision),
        )
    }

    pub fn load_coordination_plan_state(&self) -> Result<Option<CoordinationPlanState>> {
        if !self.coordination_enabled {
            return Ok(None);
        }
        Ok(self
            .read_coordination_plan_state_with_consistency(CoordinationReadConsistency::Eventual)?
            .into_value())
    }

    pub fn read_coordination_plan_state_with_consistency(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadResult<CoordinationPlanState>> {
        let materialization_enabled = self.coordination_materialization_enabled()?;
        self.read_coordination_with_consistency(
            consistency,
            || {
                if materialization_enabled {
                    if let Some(value) = SqliteCoordinationMaterializedStore::new(&self.root)
                        .read_plan_state()?
                        .value
                    {
                        return Ok(Some(CoordinationPlanState::from(
                            HydratedCoordinationPlanState {
                                snapshot: value.legacy_snapshot,
                                canonical_snapshot_v2: value.canonical_snapshot_v2,
                                runtime_descriptors: value.runtime_descriptors,
                            },
                        )));
                    }
                }
                Ok(self
                    .read_coordination_current_state_from_authority()?
                    .map(|state| CoordinationPlanState {
                        snapshot: state.snapshot,
                        canonical_snapshot_v2: state.canonical_snapshot_v2,
                        runtime_descriptors: state.runtime_descriptors,
                    }))
            },
            || {
                Ok(
                    crate::published_plans::load_authoritative_coordination_plan_state(&self.root)?
                        .map(CoordinationPlanState::from),
                )
            },
        )
    }

    fn read_coordination_with_consistency<T, E, S>(
        &self,
        consistency: CoordinationReadConsistency,
        eventual_load: E,
        strong_load: S,
    ) -> Result<CoordinationReadResult<T>>
    where
        E: Fn() -> Result<Option<T>>,
        S: Fn() -> Result<Option<T>>,
    {
        if !self.coordination_enabled {
            return Ok(CoordinationReadResult::unavailable(consistency, None));
        }
        match consistency {
            CoordinationReadConsistency::Eventual => {
                let value = eventual_load()?;
                Ok(match value {
                    Some(value) => CoordinationReadResult::verified_current(consistency, value),
                    None => CoordinationReadResult::unavailable(consistency, None),
                })
            }
            CoordinationReadConsistency::Strong => {
                match self.refresh_coordination_authority_for_strong_read() {
                    Ok(()) => match strong_load() {
                        Ok(Some(value)) => {
                            Ok(CoordinationReadResult::verified_current(consistency, value))
                        }
                        Ok(None) => Ok(CoordinationReadResult::unavailable(consistency, None)),
                        Err(error) => {
                            self.stale_coordination_read_fallback(consistency, eventual_load, error)
                        }
                    },
                    Err(error) => {
                        self.stale_coordination_read_fallback(consistency, eventual_load, error)
                    }
                }
            }
        }
    }

    fn coordination_materialization_enabled(&self) -> Result<bool> {
        Ok(coordination_materialization_enabled_by_default(
            configured_coordination_authority_store_provider(&self.root)?.config(),
        ))
    }

    fn read_coordination_current_state_from_authority_with_consistency(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<Option<crate::CoordinationCurrentState>> {
        crate::published_plans::load_authoritative_coordination_current_state_with_consistency(
            &self.root,
            consistency,
        )
    }

    fn read_coordination_current_state_from_authority(
        &self,
    ) -> Result<Option<crate::CoordinationCurrentState>> {
        self.read_coordination_current_state_from_authority_with_consistency(
            CoordinationReadConsistency::Eventual,
        )
    }

    fn read_coordination_authority_stamp(&self) -> Result<Option<CoordinationAuthorityStamp>> {
        let provider = configured_coordination_authority_store_provider(&self.root)?;
        Ok(provider
            .open_stamp_reads(&self.root)?
            .read_authority_stamp(CoordinationReadConsistency::Eventual)?
            .value)
    }

    fn stale_coordination_read_fallback<T, E>(
        &self,
        consistency: CoordinationReadConsistency,
        eventual_load: E,
        error: anyhow::Error,
    ) -> Result<CoordinationReadResult<T>>
    where
        E: Fn() -> Result<Option<T>>,
    {
        let refresh_error = Some(error.to_string());
        let value = eventual_load()?;
        Ok(match value {
            Some(value) => {
                CoordinationReadResult::verified_stale(consistency, value, refresh_error)
            }
            None => CoordinationReadResult::unavailable(consistency, refresh_error),
        })
    }

    fn refresh_coordination_authority_for_strong_read(&self) -> Result<()> {
        sync_coordination_authority_update(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            self.full_runtime_state().map(|full_runtime| &full_runtime.refresh_lock),
            &self.loaded_workspace_revision,
            &self.coordination_runtime_revision,
            self.coordination_enabled,
        )
    }

    pub fn hydrate_coordination_runtime(&self) -> Result<Option<CoordinationPlanState>> {
        let state = self
            .read_coordination_plan_state_with_consistency(CoordinationReadConsistency::Strong)?
            .into_value();
        let Some(state) = state else {
            return Ok(None);
        };
        let current_state = crate::CoordinationCurrentState::from(HydratedCoordinationPlanState {
            snapshot: state.snapshot.clone(),
            canonical_snapshot_v2: state.canonical_snapshot_v2.clone(),
            runtime_descriptors: state.runtime_descriptors.clone(),
        });
        let local_workspace_revision = self
            .store
            .lock()
            .expect("workspace store lock poisoned")
            .workspace_revision()?;
        let workspace_revision = self.loaded_workspace_revision.load(Ordering::Relaxed);
        apply_service_backed_coordination_current_state(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            &self.loaded_workspace_revision,
            None,
            None,
            local_workspace_revision,
            workspace_revision,
            Some(coordination_persist_context_for_root(&self.root, None)),
            &current_state,
            self.coordination_runtime_revision_value(self.coordination_revision()?),
            None,
        )?;
        Ok(Some(state))
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

    fn coordination_mutation_requires_refresh_lock(&self) -> bool {
        let runtime_capabilities = self
            .runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .runtime_capabilities;
        !matches!(
            prism_ir::PrismRuntimeMode::from_capabilities(runtime_capabilities),
            Some(prism_ir::PrismRuntimeMode::CoordinationOnly)
        )
    }

    fn mutate_coordination_with_session_guarded<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        observe_phase: O,
        _guard: Option<MutexGuard<'_, ()>>,
    ) -> Result<T>
    where
        F: FnOnce(&Prism) -> Result<T>,
        O: FnMut(&str, Duration, Value, bool, Option<String>),
    {
        self.mutate_coordination_with_session_guarded_with_options(
            session_id,
            mutate,
            observe_phase,
            _guard,
            true,
        )
    }

    fn mutate_coordination_with_session_guarded_with_options<T, F, O>(
        &self,
        session_id: Option<&SessionId>,
        mutate: F,
        mut observe_phase: O,
        _guard: Option<MutexGuard<'_, ()>>,
        schedule_materialization: bool,
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
        let before = prism.legacy_coordination_snapshot();
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
        let snapshot = prism.legacy_coordination_snapshot();
        let appended_events = coordination_event_delta(&before.events, &snapshot.events);
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
        let canonical_snapshot_v2 = prism.coordination_snapshot_v2();
        let runtime_descriptors = prism.runtime_descriptors();
        let current_state = crate::CoordinationCurrentState {
            snapshot: snapshot.clone(),
            canonical_snapshot_v2: canonical_snapshot_v2.clone(),
            runtime_descriptors: runtime_descriptors.clone(),
        };
        let publish_context = publish_context_from_coordination_events(&appended_events);
        let should_persist = !appended_events.is_empty() || snapshot != before;
        let local_workspace_revision = prism.workspace_revision().graph_version;
        let workspace_revision = self.loaded_workspace_revision.load(Ordering::Relaxed);
        if should_persist {
            let persist_started = Instant::now();
            let persist_result = {
                let mut store = self.store.lock().expect("coordination store lock poisoned");
                store.persist_coordination_authoritative_mutation_state_for_root_with_session_observed(
                    &self.root,
                    expected_revision,
                    &snapshot,
                    &appended_events,
                    session_id,
                    Some(&before),
                    &canonical_snapshot_v2,
                    CoordinationDerivedPersistenceMode::Deferred,
                    &mut observe_phase,
                )
            };
            observe_phase(
                "mutation.coordination.persistState",
                persist_started.elapsed(),
                json!({
                    "appendedEventCount": appended_events.len(),
                    "planCount": snapshot.plans.len(),
                }),
                persist_result.is_ok(),
                persist_result.as_ref().err().map(|error| error.to_string()),
            );
            let authoritative_revision = persist_result?.revision;
            let apply_started = Instant::now();
            let apply_result = apply_service_backed_coordination_current_state(
                &self.root,
                &self.published_generation,
                &self.runtime_state,
                &self.store,
                &self.cold_query_store,
                &self.loaded_workspace_revision,
                Some(&self.coordination_runtime_revision),
                if schedule_materialization {
                    self.full_runtime_state()
                        .and_then(|full_runtime| full_runtime.checkpoint_materializer.as_ref())
                } else {
                    None
                },
                local_workspace_revision,
                workspace_revision,
                Some(coordination_persist_context_for_root(
                    &self.root, session_id,
                )),
                &current_state,
                authoritative_revision,
                publish_context.clone(),
            );
            observe_phase(
                "mutation.coordination.applyCurrentState",
                apply_started.elapsed(),
                json!({
                    "eventCount": current_state.snapshot.events.len(),
                    "materializationScheduled": schedule_materialization,
                }),
                apply_result.is_ok(),
                apply_result.as_ref().err().map(|error| error.to_string()),
            );
            apply_result?;
            if !schedule_materialization {
                observe_phase(
                    "mutation.coordination.scheduleMaterialization",
                    Duration::ZERO,
                    json!({ "eventCount": snapshot.events.len() }),
                    true,
                    None,
                );
            }
        } else {
            let runtime_state = self
                .runtime_state
                .lock()
                .expect("workspace runtime state lock poisoned")
                .clone();
            self.publish_runtime_state(
                runtime_state,
                local_workspace_revision,
                workspace_revision,
                Some(coordination_persist_context_for_root(
                    &self.root, session_id,
                )),
                None,
            );
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
        let guard = if self.coordination_mutation_requires_refresh_lock() {
            let lock_wait_started = Instant::now();
            let guard = self
                .full_runtime_state()
                .expect("workspace refresh lock unavailable in coordination-only mode")
                .refresh_lock
                .lock()
                .expect("workspace refresh lock poisoned");
            observe_phase(
                "mutation.coordination.waitRefreshLock",
                lock_wait_started.elapsed(),
                json!({ "bypassed": false }),
                true,
                None,
            );
            Some(guard)
        } else {
            observe_phase(
                "mutation.coordination.waitRefreshLock",
                Duration::ZERO,
                json!({ "bypassed": true }),
                true,
                None,
            );
            None
        };
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
        let guard = if self.coordination_mutation_requires_refresh_lock() {
            let Some(guard) = self.try_lock_refresh_for_phase(
                "mutation.coordination.waitRefreshLock",
                "mutateCoordination",
            )?
            else {
                return Ok(None);
            };
            Some(guard)
        } else {
            None
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
        let guard = if self.coordination_mutation_requires_refresh_lock() {
            let Some(guard) = self.wait_lock_refresh_for_phase(
                "mutation.coordination.waitRefreshLock",
                "mutateCoordination",
                MUTATION_REFRESH_WAIT_TIMEOUT,
            )?
            else {
                return Ok(None);
            };
            Some(guard)
        } else {
            None
        };
        self.mutate_coordination_with_session_guarded(session_id, mutate, observe_phase, guard)
            .map(Some)
    }

    pub fn mutate_coordination_with_session_wait_observed_no_materialization<T, F, O>(
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
        let guard = if self.coordination_mutation_requires_refresh_lock() {
            let Some(guard) = self.wait_lock_refresh_for_phase(
                "mutation.coordination.waitRefreshLock",
                "mutateCoordination",
                MUTATION_REFRESH_WAIT_TIMEOUT,
            )?
            else {
                return Ok(None);
            };
            Some(guard)
        } else {
            None
        };
        self.mutate_coordination_with_session_guarded_with_options(
            session_id,
            mutate,
            observe_phase,
            guard,
            false,
        )
        .map(Some)
    }

    pub fn curator_snapshot(&self) -> Result<CuratorSnapshot> {
        self.full_runtime_state()
            .and_then(|full_runtime| full_runtime.curator.as_ref())
            .map(|curator| curator.snapshot())
            .transpose()
            .map(|snapshot| snapshot.unwrap_or_default())
    }

    #[cfg(test)]
    pub(crate) fn is_curator_snapshot_loaded(&self) -> bool {
        self.full_runtime_state()
            .and_then(|full_runtime| full_runtime.curator.as_ref())
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
        let guard = self.lock_refresh_for_mutation("setCuratorProposalState");
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
        let Some(curator) = self
            .full_runtime_state()
            .and_then(|full_runtime| full_runtime.curator.as_ref())
        else {
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
        if let Some(materializer) = self
            .full_runtime_state()
            .and_then(|full_runtime| full_runtime.checkpoint_materializer.as_ref())
        {
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
                .full_runtime_state()
                .expect("checkpoint materializer state unavailable in coordination-only mode")
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
                .full_runtime_state()
                .expect("checkpoint materializer state unavailable in coordination-only mode")
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
                .full_runtime_state()
                .expect("checkpoint materializer state unavailable in coordination-only mode")
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
        if let Some(curator) = self
            .full_runtime_state()
            .and_then(|full_runtime| full_runtime.curator.as_ref())
        {
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
        let Some(full_runtime) = self.full_runtime_state() else {
            return Ok(());
        };
        if let Some(materializer) = &full_runtime.checkpoint_materializer {
            materializer.flush()?;
        }
        self.ensure_coordination_materialization_state()?;
        let wait_started = Instant::now();
        while full_runtime
            .repo_projection_sync_pending
            .load(Ordering::Acquire)
        {
            if wait_started.elapsed() > Duration::from_secs(5) {
                return Err(anyhow!(
                    "timed out waiting for background PRISM doc sync to finish"
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn ensure_coordination_materialization_state(&self) -> Result<()> {
        if !self.coordination_enabled || !self.coordination_materialization_enabled()? {
            return Ok(());
        }

        let materialized_store = SqliteCoordinationMaterializedStore::new(&self.root);
        let authoritative_revision = self
            .read_coordination_authority_stamp()?
            .and_then(|authority| authority_revision_from_stamp(&authority))
            .unwrap_or_else(|| self.coordination_runtime_revision.load(Ordering::Relaxed));
        let startup_checkpoint = materialized_store.read_startup_checkpoint()?;
        let read_model = materialized_store.read_read_model()?;
        let queue_read_model = materialized_store.read_queue_read_model()?;
        let startup_revision = startup_checkpoint
            .value
            .as_ref()
            .map(|checkpoint| checkpoint.coordination_revision);
        let read_model_revision = read_model.value.as_ref().map(|value| value.revision);
        let queue_read_model_revision = queue_read_model.value.as_ref().map(|value| value.revision);
        if startup_revision == Some(authoritative_revision)
            && read_model_revision == Some(authoritative_revision)
            && queue_read_model_revision == Some(authoritative_revision)
        {
            return Ok(());
        }

        let Some(current_state) = self.read_coordination_current_state_from_authority_with_consistency(
            CoordinationReadConsistency::Strong,
        )? else {
            return Ok(());
        };
        let mut store = self.store.lock().expect("workspace store lock poisoned");
        persist_coordination_materialization(
            &self.root,
            &mut *store,
            &CoordinationMaterialization {
                authoritative_revision,
                snapshot: current_state.snapshot,
                canonical_snapshot_v2: current_state.canonical_snapshot_v2,
                runtime_descriptors: Some(current_state.runtime_descriptors),
                publish_context: None,
            },
        )
    }

    pub fn persist_runtime_startup_checkpoint(&self) -> Result<()> {
        if self.coordination_only_runtime() {
            return Ok(());
        }
        crate::workspace_startup_checkpoint::persist_workspace_runtime_startup_checkpoint(self)
    }

    pub(crate) fn workspace_tree_snapshot(&self) -> prism_store::WorkspaceTreeSnapshot {
        self.full_runtime_state()
            .map(|full_runtime| {
                full_runtime
                    .fs_snapshot
                    .lock()
                    .expect("workspace tree snapshot lock poisoned")
                    .clone()
            })
            .unwrap_or_default()
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
                .full_runtime_state()
                .expect("checkpoint materializer state unavailable in coordination-only mode")
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
        if let Some(curator) = self
            .full_runtime_state()
            .and_then(|full_runtime| full_runtime.curator.as_ref())
        {
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
        let full_runtime = self
            .full_runtime_state()
            .expect("workspace refresh state unavailable in coordination-only mode");
        let curator = full_runtime.curator.as_ref().map(CuratorHandleRef::from);
        refresh_prism_snapshot(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            &full_runtime.refresh_lock,
            &full_runtime.refresh_state,
            &self.loaded_workspace_revision,
            &full_runtime.fs_snapshot,
            full_runtime.checkpoint_materializer.clone(),
            self.coordination_enabled,
            curator.as_ref(),
            &full_runtime.observed_change_tracker,
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
        let full_runtime = self
            .full_runtime_state()
            .expect("workspace refresh state unavailable in coordination-only mode");
        try_refresh_prism_snapshot(
            &self.root,
            &self.published_generation,
            &self.runtime_state,
            &self.store,
            &self.cold_query_store,
            &full_runtime.refresh_lock,
            &full_runtime.refresh_state,
            &self.loaded_workspace_revision,
            &full_runtime.fs_snapshot,
            full_runtime.checkpoint_materializer.clone(),
            self.coordination_enabled,
            full_runtime.curator.as_ref().map(CuratorHandleRef::from).as_ref(),
            &full_runtime.observed_change_tracker,
            &self.worktree_principal_binding,
            trigger,
            known_fingerprint,
            dirty_paths_override,
        )
    }
}

fn authority_revision_from_stamp(authority: &CoordinationAuthorityStamp) -> Option<u64> {
    match authority.backend_kind {
        CoordinationAuthorityBackendKind::Sqlite => {
            parse_authority_revision_token(&authority.snapshot_id, "sqlite-revision:")
        }
        CoordinationAuthorityBackendKind::Postgres => {
            parse_authority_revision_token(&authority.snapshot_id, "postgres-revision:")
        }
    }
}

fn parse_authority_revision_token(snapshot_id: &str, prefix: &str) -> Option<u64> {
    snapshot_id
        .strip_prefix(prefix)?
        .split(':')
        .next()?
        .parse()
        .ok()
}

fn publish_pending_repo_patch_provenance(
    root: &Path,
    prism: Arc<Prism>,
    runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    bound_principal: BoundWorktreePrincipal,
    active_work: ActiveWorkContextBinding,
) -> Result<Vec<EventId>> {
    if tracked_snapshot_authority_active(root)? {
        return Ok(Vec::new());
    }
    let existing_repo_event_ids = load_repo_patch_events(root)?
        .into_iter()
        .map(|event| event.meta.id)
        .collect::<HashSet<_>>();
    let patch_events = prism.query_outcomes(&OutcomeRecallQuery {
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
        append_repo_patch_event(root, &repo_event)?;
        published.push(repo_event);
    }

    if published.is_empty() {
        return Ok(Vec::new());
    }

    {
        let mut runtime_state = runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        for event in &published {
            prism.apply_outcome_event_to_projections(event);
            let _ = prism.outcome_memory().store_event(event.clone())?;
            runtime_state.apply_outcome_event(event);
        }
    }

    Ok(published.into_iter().map(|event| event.meta.id).collect())
}

impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        let coordination_only_runtime = self.coordination_only_runtime();
        let principal_registry = (!coordination_only_runtime).then(|| {
            self.principal_registry
                .read()
                .expect("principal registry lock poisoned")
                .clone()
        });
        if let Some(full_runtime) = self.full_runtime_state_mut() {
            if let Some(watch) = full_runtime.watch.take() {
                let _ = watch.stop.send(WatchMessage::Stop);
                let _ = watch.handle.join();
            }
            if let Some(watch) = full_runtime.protected_state_watch.take() {
                let _ = watch.stop.send(WatchMessage::Stop);
                let _ = watch.handle.join();
            }
            if let Some(watch) = full_runtime.coordination_authority_watch.take() {
                let _ = watch.stop.send(WatchMessage::Stop);
                let _ = watch.handle.join();
            }
        }
        if !coordination_only_runtime {
            if let Err(error) =
                self.persist_flushed_observed_change_checkpoints(None, None, None, None)
            {
                warn!(error = %error, "failed to persist observed change checkpoints during workspace session shutdown");
            }
            if let Err(error) = self.flush_materializations() {
                warn!(error = %error, "failed to flush workspace materializations during workspace session shutdown");
            } else if let Err(error) = self.persist_runtime_startup_checkpoint() {
                warn!(error = %error, "failed to persist workspace startup checkpoint during workspace session shutdown");
            }
        }
        if let Some(full_runtime) = self.full_runtime_state_mut() {
            if let Some(mut curator) = full_runtime.curator.take() {
                curator.stop();
            }
            if let Some(mut materializer) = full_runtime.checkpoint_materializer.take() {
                materializer.stop();
            }
        }
        if let Some(principal_registry) = principal_registry.as_ref() {
            if !principal_registry.principals.is_empty() || !principal_registry.credentials.is_empty()
            {
                let persist_result =
                    prism_store::MaterializationStore::save_principal_registry_snapshot(
                        &mut *self.store.lock().expect("workspace store lock poisoned"),
                        principal_registry,
                    );
                if let Err(error) = persist_result {
                    warn!(error = %error, "failed to persist principal registry during workspace session shutdown");
                }
            }
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
