use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use prism_coordination::{
    assisted_heartbeat_window, claim_heartbeat_due_state, claim_lease_state,
    task_heartbeat_due_state, task_lease_state, CoordinationPolicy, CoordinationTask,
    LeaseHeartbeatDueState, LeaseState, WorkClaim,
};
use prism_history::HistoryStore;
use prism_ir::{
    new_prefixed_id, ChangeTrigger, ClaimStatus, EventActor, EventExecutionContext, EventId,
    EventMeta, LeaseRenewalMode, PrincipalActor, PrincipalAuthorityId, PrincipalId, SessionId,
    TaskId, WorkContextSnapshot,
};
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::CoordinationJournal;
use prism_store::{Graph, SqliteStore, WorkspaceTreeSnapshot};
use tracing::{error, info, warn};

use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator::{enqueue_curator_for_observed_async, CuratorHandleRef};
use crate::indexer::WorkspaceIndexer;
use crate::layout::discover_layout;
use crate::observed_change_tracker::SharedObservedChangeTracker;
use crate::protected_state::runtime_sync::{
    load_repo_protected_knowledge, load_repo_protected_plan_state,
    sync_selected_repo_protected_state, ProtectedStateImportSelection,
};
use crate::protected_state::streams::{classify_protected_repo_relative_path, ProtectedRepoStream};
use crate::session::{
    WorkspaceRefreshBreakdown, WorkspaceRefreshResult, WorkspaceRefreshState, WorkspaceSession,
};
use crate::shared_coordination_ref::{
    poll_shared_coordination_ref_live_sync, SharedCoordinationRefLiveSync,
};
use crate::shared_runtime::composite_workspace_revision;
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::shared_runtime_store::SharedRuntimeStore;
use crate::util::{cache_path, current_timestamp, is_generated_projection_relative_path};
use crate::workspace_identity::{
    coordination_persist_context_for_root, workspace_identity_for_root,
};
use crate::workspace_runtime_state::{WorkspacePublishedGeneration, WorkspaceRuntimeState};
use crate::workspace_tree::{
    diff_workspace_tree_snapshot, plan_full_refresh, plan_incremental_refresh,
    populate_package_regions, WorkspaceRefreshMode,
};
use crate::worktree_principal::BoundWorktreePrincipal;

const ASSISTED_LEASE_RENEWAL_ENV: &str = "PRISM_ASSISTED_LEASE_RENEWAL";
const SHARED_COORDINATION_REF_POLL_INTERVAL: Duration = Duration::from_millis(1500);

pub(crate) struct WatchHandle {
    pub(crate) stop: mpsc::Sender<WatchMessage>,
    pub(crate) handle: thread::JoinHandle<()>,
}

pub(crate) enum WatchMessage {
    Fs(notify::Result<Event>),
    Stop,
}

pub(crate) fn spawn_fs_watch(
    root: PathBuf,
    published_generation: Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    store: Arc<Mutex<SqliteStore>>,
    cold_query_store: Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: SharedRuntimeBackend,
    refresh_lock: Arc<Mutex<()>>,
    refresh_state: Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: Arc<AtomicU64>,
    fs_snapshot: Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<CuratorHandleRef>,
    observed_change_tracker: SharedObservedChangeTracker,
    worktree_principal_binding: Arc<Mutex<Option<BoundWorktreePrincipal>>>,
) -> Result<WatchHandle> {
    let (msg_tx, msg_rx) = mpsc::channel::<WatchMessage>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<bool>(1);
    let callback_tx = msg_tx.clone();

    let handle = thread::spawn(move || {
        let mut watcher = match recommended_watcher(move |event| {
            let _ = callback_tx.send(WatchMessage::Fs(event));
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                warn!(
                    root = %root.display(),
                    error = %error,
                    "failed to initialize prism fs watcher; continuing with fallback refresh checks"
                );
                let _ = ready_tx.send(false);
                return;
            }
        };

        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            warn!(
                root = %root.display(),
                error = %error,
                "failed to start prism fs watcher; continuing with fallback refresh checks"
            );
            let _ = ready_tx.send(false);
            return;
        }
        let _ = ready_tx.send(true);

        loop {
            let event = match msg_rx.recv() {
                Ok(WatchMessage::Fs(event)) => event,
                Ok(WatchMessage::Stop) | Err(mpsc::RecvError) => {
                    observed_change_tracker
                        .lock()
                        .expect("observed change tracker lock poisoned")
                        .flush(crate::ObservedChangeFlushTrigger::Disconnect);
                    break;
                }
            };

            let Ok(event) = event else {
                continue;
            };
            let mut dirty_paths = relevant_watch_paths(&root, &event);
            if dirty_paths.is_empty() {
                continue;
            }

            while let Ok(next) = msg_rx.recv_timeout(Duration::from_millis(75)) {
                match next {
                    WatchMessage::Fs(Ok(next)) => {
                        let next_paths = relevant_watch_paths(&root, &next);
                        if !next_paths.is_empty() {
                            dirty_paths.extend(next_paths);
                        }
                    }
                    WatchMessage::Fs(Err(_)) => continue,
                    WatchMessage::Stop => {
                        observed_change_tracker
                            .lock()
                            .expect("observed change tracker lock poisoned")
                            .flush(crate::ObservedChangeFlushTrigger::Disconnect);
                        return;
                    }
                };
            }

            if !dirty_paths.is_empty() {
                refresh_state.mark_fs_dirty_paths(dirty_paths.iter().cloned());

                if let Err(error) = refresh_prism_snapshot(
                    &root,
                    &published_generation,
                    &runtime_state,
                    &store,
                    &cold_query_store,
                    shared_runtime_store.as_ref(),
                    &shared_runtime,
                    &refresh_lock,
                    &refresh_state,
                    &loaded_workspace_revision,
                    &fs_snapshot,
                    checkpoint_materializer.clone(),
                    shared_runtime_materializer.clone(),
                    coordination_enabled,
                    curator.as_ref(),
                    &observed_change_tracker,
                    &worktree_principal_binding,
                    ChangeTrigger::FsWatch,
                    None,
                    Some(dirty_paths),
                ) {
                    error!(
                        root = %root.display(),
                        error = %error,
                        error_chain = %format_error_chain(&error),
                        "prism fs watch refresh failed"
                    );
                }
            }
        }
    });

    let _ = ready_rx.recv_timeout(Duration::from_millis(250));

    Ok(WatchHandle {
        stop: msg_tx,
        handle,
    })
}

pub(crate) fn spawn_protected_state_watch(
    root: PathBuf,
    published_generation: Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    store: Arc<Mutex<SqliteStore>>,
    cold_query_store: Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: SharedRuntimeBackend,
    refresh_lock: Arc<Mutex<()>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<WatchHandle> {
    let (msg_tx, msg_rx) = mpsc::channel::<WatchMessage>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<bool>(1);
    let callback_tx = msg_tx.clone();

    let handle = thread::spawn(move || {
        let mut watcher = match recommended_watcher(move |event| {
            let _ = callback_tx.send(WatchMessage::Fs(event));
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                warn!(
                    root = %root.display(),
                    error = %error,
                    "failed to initialize prism protected-state watcher; continuing without live .prism sync"
                );
                let _ = ready_tx.send(false);
                return;
            }
        };

        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            warn!(
                root = %root.display(),
                error = %error,
                "failed to start prism protected-state watcher; continuing without live .prism sync"
            );
            let _ = ready_tx.send(false);
            return;
        }
        let _ = ready_tx.send(true);

        loop {
            let event = match msg_rx.recv() {
                Ok(WatchMessage::Fs(event)) => event,
                Ok(WatchMessage::Stop) | Err(mpsc::RecvError) => return,
            };

            let Ok(event) = event else {
                continue;
            };
            let mut protected_streams = relevant_protected_state_streams(&root, &event);
            if protected_streams.is_empty() {
                continue;
            }

            while let Ok(next) = msg_rx.recv_timeout(Duration::from_millis(75)) {
                match next {
                    WatchMessage::Fs(Ok(next)) => {
                        protected_streams.extend(relevant_protected_state_streams(&root, &next));
                    }
                    WatchMessage::Fs(Err(_)) => continue,
                    WatchMessage::Stop => return,
                };
            }

            if protected_streams.is_empty() {
                continue;
            }

            if let Err(error) = sync_protected_state_watch_update(
                &root,
                &published_generation,
                &runtime_state,
                &store,
                &cold_query_store,
                shared_runtime_store.as_ref(),
                &shared_runtime,
                &refresh_lock,
                &loaded_workspace_revision,
                coordination_enabled,
                &protected_streams,
            ) {
                error!(
                    root = %root.display(),
                    error = %error,
                    error_chain = %format_error_chain(&error),
                    "prism protected-state watch sync failed"
                );
            }
        }
    });

    let _ = ready_rx.recv_timeout(Duration::from_millis(250));

    Ok(WatchHandle {
        stop: msg_tx,
        handle,
    })
}

pub(crate) fn spawn_shared_coordination_ref_watch(
    root: PathBuf,
    published_generation: Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: Arc<Mutex<WorkspaceRuntimeState>>,
    store: Arc<Mutex<SqliteStore>>,
    cold_query_store: Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    refresh_lock: Arc<Mutex<()>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    coordination_runtime_revision: Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<WatchHandle> {
    let (msg_tx, msg_rx) = mpsc::channel::<WatchMessage>();
    let handle = thread::spawn(move || loop {
        match msg_rx.recv_timeout(SHARED_COORDINATION_REF_POLL_INTERVAL) {
            Ok(WatchMessage::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Ok(WatchMessage::Fs(_)) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        if let Err(error) = sync_shared_coordination_ref_watch_update(
            &root,
            &published_generation,
            &runtime_state,
            &store,
            &cold_query_store,
            shared_runtime_store.as_ref(),
            &refresh_lock,
            &loaded_workspace_revision,
            &coordination_runtime_revision,
            coordination_enabled,
        ) {
            error!(
                root = %root.display(),
                error = %error,
                error_chain = %format_error_chain(&error),
                "prism shared coordination ref live sync failed"
            );
        }
    });

    Ok(WatchHandle {
        stop: msg_tx,
        handle,
    })
}

pub(crate) fn refresh_prism_snapshot(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: &SharedRuntimeBackend,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    observed_change_tracker: &SharedObservedChangeTracker,
    worktree_principal_binding: &Arc<Mutex<Option<BoundWorktreePrincipal>>>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
) -> Result<WorkspaceRefreshResult> {
    let guard = refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");
    refresh_prism_snapshot_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        shared_runtime,
        refresh_state,
        loaded_workspace_revision,
        fs_snapshot,
        checkpoint_materializer,
        shared_runtime_materializer,
        coordination_enabled,
        curator,
        observed_change_tracker,
        worktree_principal_binding,
        trigger,
        known_fingerprint,
        dirty_paths_override,
        guard,
    )
}

pub(crate) fn try_refresh_prism_snapshot(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: &SharedRuntimeBackend,
    refresh_lock: &Arc<Mutex<()>>,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    observed_change_tracker: &SharedObservedChangeTracker,
    worktree_principal_binding: &Arc<Mutex<Option<BoundWorktreePrincipal>>>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
) -> Result<Option<WorkspaceRefreshResult>> {
    let Ok(guard) = refresh_lock.try_lock() else {
        return Ok(None);
    };
    let observed = refresh_prism_snapshot_with_guard(
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime_store,
        shared_runtime,
        refresh_state,
        loaded_workspace_revision,
        fs_snapshot,
        checkpoint_materializer,
        shared_runtime_materializer,
        coordination_enabled,
        curator,
        observed_change_tracker,
        worktree_principal_binding,
        trigger,
        known_fingerprint,
        dirty_paths_override,
        guard,
    )?;
    Ok(Some(observed))
}

fn refresh_prism_snapshot_with_guard(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: &SharedRuntimeBackend,
    refresh_state: &Arc<WorkspaceRefreshState>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    fs_snapshot: &Arc<Mutex<WorkspaceTreeSnapshot>>,
    checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    shared_runtime_materializer: Option<CheckpointMaterializerHandle>,
    coordination_enabled: bool,
    curator: Option<&CuratorHandleRef>,
    observed_change_tracker: &SharedObservedChangeTracker,
    worktree_principal_binding: &Arc<Mutex<Option<BoundWorktreePrincipal>>>,
    trigger: ChangeTrigger,
    known_fingerprint: Option<WorkspaceTreeSnapshot>,
    dirty_paths_override: Option<Vec<PathBuf>>,
    _guard: MutexGuard<'_, ()>,
) -> Result<WorkspaceRefreshResult> {
    let started = Instant::now();
    let plan_started = Instant::now();
    let observed_revision = refresh_state.observed_fs_revision();
    let dirty_paths = if trigger == ChangeTrigger::FsWatch {
        dirty_paths_override.unwrap_or_else(|| refresh_state.dirty_paths_snapshot())
    } else {
        Vec::new()
    };
    let scoped_watch_refresh = trigger == ChangeTrigger::FsWatch
        && !dirty_paths.is_empty()
        && can_scope_watch_refresh(root, &dirty_paths);
    let cached_snapshot = fs_snapshot
        .lock()
        .expect("workspace tree snapshot lock poisoned")
        .clone();
    let mut plan = if scoped_watch_refresh {
        plan_incremental_refresh(root, &cached_snapshot, &dirty_paths)?
    } else if let Some(next_snapshot) = known_fingerprint {
        crate::workspace_tree::WorkspaceRefreshPlan {
            mode: if cached_snapshot.files.is_empty() && cached_snapshot.directories.is_empty() {
                WorkspaceRefreshMode::Full
            } else {
                WorkspaceRefreshMode::Rescan
            },
            delta: diff_workspace_tree_snapshot(root, &cached_snapshot, &next_snapshot),
            next_snapshot,
        }
    } else {
        plan_full_refresh(root, &cached_snapshot)?
    };
    let plan_refresh_ms = u64::try_from(plan_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if plan.delta.is_empty() {
        *fs_snapshot
            .lock()
            .expect("workspace tree snapshot lock poisoned") = plan.next_snapshot;
        refresh_state.mark_refreshed_revision(observed_revision, &dirty_paths);
        return Ok(WorkspaceRefreshResult {
            mode: None,
            observed: Vec::new(),
            breakdown: WorkspaceRefreshBreakdown {
                plan_refresh_ms,
                ..WorkspaceRefreshBreakdown::default()
            },
        });
    }
    let build_indexer_started = Instant::now();
    let current_prism = published_generation
        .read()
        .expect("workspace published generation lock poisoned")
        .prism_arc();
    let coordination_context = current_prism.coordination_context();
    let runtime_state_value = {
        let mut state = runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned");
        let placeholder = WorkspaceRuntimeState::placeholder_with_layout(state.layout());
        std::mem::replace(&mut *state, placeholder)
    };
    let mut runtime_state_value = runtime_state_value;
    runtime_state_value.overlay_live_projection_knowledge(current_prism.as_ref());
    let cached_layout = runtime_state_value.layout();
    let layout_refresh_required = plan.mode != WorkspaceRefreshMode::Incremental
        || cached_layout.refresh_required_for_paths(plan.delta.changed_files.iter());
    let next_layout = if layout_refresh_required {
        discover_layout(root)?
    } else {
        cached_layout.clone()
    };
    let refresh_runtime_roots = layout_refresh_required
        || next_layout.workspace_manifest != cached_layout.workspace_manifest
        || next_layout.packages.len() != cached_layout.packages.len();
    let reopened_store = store
        .lock()
        .expect("workspace store lock poisoned")
        .reopen_runtime_writer()?;
    let reopened_shared_runtime_store: Option<SharedRuntimeStore> = shared_runtime_store
        .map(|store| {
            store
                .lock()
                .expect("shared runtime store lock poisoned")
                .reopen_runtime_writer()
        })
        .transpose()?;
    let mut indexer = WorkspaceIndexer::with_runtime_state_stores_and_options(
        root,
        reopened_store,
        reopened_shared_runtime_store,
        runtime_state_value,
        next_layout.clone(),
        refresh_runtime_roots,
        Some(cached_snapshot),
        checkpoint_materializer,
        crate::workspace_session_defaults::runtime_rebuild_session_options(
            coordination_enabled,
            shared_runtime,
        ),
    )?;
    indexer.shared_runtime_materializer = shared_runtime_materializer;
    populate_package_regions(&mut plan.delta, &indexer.layout);
    let build_indexer_ms =
        u64::try_from(build_indexer_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let index_workspace_started = Instant::now();
    let observed_meta =
        observed_change_event_meta(root, observed_change_tracker, worktree_principal_binding);
    let observed =
        match indexer.index_with_refresh_plan_and_meta(trigger.clone(), &plan, observed_meta) {
            Ok(observed) => observed,
            Err(error) => {
                let mut fallback_graph = Graph::from_snapshot(current_prism.graph().snapshot());
                fallback_graph.bind_workspace_root(root);
                *runtime_state
                    .lock()
                    .expect("workspace runtime state lock poisoned") = WorkspaceRuntimeState::new(
                    next_layout,
                    fallback_graph,
                    HistoryStore::from_snapshot(current_prism.history_snapshot()),
                    OutcomeMemory::from_snapshot(current_prism.outcome_snapshot()),
                    current_prism.coordination_snapshot(),
                    current_prism.authored_plan_graphs(),
                    current_prism.plan_execution_overlays_by_plan(),
                    ProjectionIndex::from_snapshot(current_prism.projection_snapshot()),
                );
                return Err(error);
            }
        };
    let index_workspace_ms =
        u64::try_from(index_workspace_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    observed_change_tracker
        .lock()
        .expect("observed change tracker lock poisoned")
        .record(
            worktree_principal_binding
                .lock()
                .expect("worktree principal binding lock poisoned")
                .clone(),
            &observed,
        );
    let local_workspace_revision = indexer.store.workspace_revision()?;
    let workspace_revision = composite_workspace_revision(
        local_workspace_revision,
        indexer
            .shared_runtime_store
            .as_ref()
            .map(SharedRuntimeStore::workspace_revision)
            .transpose()?,
    );
    let mut next_state = indexer.into_runtime_state();
    let published_workspace_revision = prism_ir::WorkspaceRevision {
        graph_version: local_workspace_revision,
        git_commit: None,
    };
    let publish_generation_started = Instant::now();
    let mut next = next_state.publish_generation(
        published_workspace_revision.clone(),
        coordination_context.clone(),
    );
    let mut publish_generation_ms =
        u64::try_from(publish_generation_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut assisted_lease_ms = 0u64;
    if trigger == ChangeTrigger::FsWatch && coordination_enabled {
        let assisted_lease_started = Instant::now();
        match maybe_auto_heartbeat_assisted_leases(
            root,
            next.prism_arc().as_ref(),
            store,
            shared_runtime_store,
        ) {
            Ok(true) => {
                let prism = next.prism_arc();
                next_state.replace_coordination_runtime(
                    prism.coordination_snapshot(),
                    prism.authored_plan_graphs(),
                    prism.plan_execution_overlays_by_plan(),
                );
                let republish_started = Instant::now();
                next = next_state.publish_generation(
                    published_workspace_revision.clone(),
                    coordination_context.clone(),
                );
                publish_generation_ms = publish_generation_ms.saturating_add(
                    u64::try_from(republish_started.elapsed().as_millis()).unwrap_or(u64::MAX),
                );
            }
            Ok(false) => {}
            Err(error) => {
                warn!(
                    root = %root.display(),
                    error = %error,
                    error_chain = %format_error_chain(&error),
                    "assisted lease heartbeat skipped after fs refresh"
                );
            }
        }
        assisted_lease_ms =
            u64::try_from(assisted_lease_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    }
    *fs_snapshot
        .lock()
        .expect("workspace tree snapshot lock poisoned") = plan.next_snapshot;
    let curator_started = Instant::now();
    if let Some(curator) = curator {
        enqueue_curator_for_observed_async(
            root,
            curator.clone(),
            next.prism_arc(),
            Arc::clone(store),
            observed.clone(),
        );
    }
    let curator_enqueue_ms =
        u64::try_from(curator_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let attach_cold_query_backends_started = Instant::now();
    WorkspaceSession::attach_cold_query_backends(
        next.prism_arc().as_ref(),
        cold_query_store,
        shared_runtime_store,
    );
    let attach_cold_query_backends_ms =
        u64::try_from(attach_cold_query_backends_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let finalize_refresh_state_started = Instant::now();
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(workspace_revision, Ordering::Relaxed);
    refresh_state.mark_refreshed_revision(observed_revision, &dirty_paths);
    refresh_state.record_refresh(
        plan.mode.as_str(),
        started.elapsed().as_millis() as u64,
        workspace_revision,
        &plan.delta,
    );
    let finalize_refresh_state_ms =
        u64::try_from(finalize_refresh_state_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let breakdown = WorkspaceRefreshBreakdown {
        plan_refresh_ms,
        build_indexer_ms,
        index_workspace_ms,
        publish_generation_ms,
        assisted_lease_ms,
        curator_enqueue_ms,
        attach_cold_query_backends_ms,
        finalize_refresh_state_ms,
    };
    info!(
        root = %root.display(),
        trigger = ?trigger,
        refresh_mode = %plan.mode.as_str(),
        observed_change_sets = observed.len(),
        plan_refresh_ms,
        build_indexer_ms,
        index_workspace_ms,
        publish_generation_ms,
        assisted_lease_ms,
        curator_enqueue_ms,
        attach_cold_query_backends_ms,
        finalize_refresh_state_ms,
        total_ms = started.elapsed().as_millis(),
        "completed prism workspace refresh pipeline"
    );
    Ok(WorkspaceRefreshResult {
        mode: Some(plan.mode),
        observed,
        breakdown,
    })
}

#[derive(Debug, Clone)]
enum AssistedLeaseTarget {
    Task {
        task: CoordinationTask,
        principal: PrincipalActor,
        session_id: Option<SessionId>,
        policy: CoordinationPolicy,
    },
    Claim {
        claim: WorkClaim,
        principal: PrincipalActor,
        session_id: SessionId,
        policy: CoordinationPolicy,
    },
}

impl AssistedLeaseTarget {
    fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::Task { session_id, .. } => session_id.as_ref(),
            Self::Claim { session_id, .. } => Some(session_id),
        }
    }

    fn correlation(&self) -> Option<TaskId> {
        match self {
            Self::Task { task, .. } => Some(TaskId::new(task.id.0.clone())),
            Self::Claim { claim, .. } => claim
                .task
                .as_ref()
                .map(|task_id| TaskId::new(task_id.0.clone())),
        }
    }

    fn principal(&self) -> &PrincipalActor {
        match self {
            Self::Task { principal, .. } | Self::Claim { principal, .. } => principal,
        }
    }

    fn due_state(&self, now: u64) -> LeaseHeartbeatDueState {
        match self {
            Self::Task { task, policy, .. } => task_heartbeat_due_state(task, policy, now),
            Self::Claim { claim, policy, .. } => claim_heartbeat_due_state(claim, policy, now),
        }
    }

    fn assisted_window(&self) -> u64 {
        match self {
            Self::Task { policy, .. } | Self::Claim { policy, .. } => {
                assisted_heartbeat_window(policy)
            }
        }
    }

    fn matches_event(&self, event: &prism_coordination::CoordinationEvent) -> bool {
        match self {
            Self::Task { task, .. } => event
                .task
                .as_ref()
                .is_some_and(|task_id| task_id == &task.id),
            Self::Claim { claim, .. } => event
                .claim
                .as_ref()
                .is_some_and(|claim_id| claim_id == &claim.id),
        }
    }
}

fn assisted_lease_renewal_enabled() -> bool {
    env::var(ASSISTED_LEASE_RENEWAL_ENV)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn maybe_auto_heartbeat_assisted_leases(
    root: &Path,
    prism: &Prism,
    store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
) -> Result<bool> {
    if !assisted_lease_renewal_enabled() {
        return Ok(false);
    }
    if let Some(shared_runtime_store) = shared_runtime_store {
        let mut store = shared_runtime_store
            .lock()
            .expect("shared runtime store lock poisoned");
        maybe_auto_heartbeat_assisted_leases_in_store(root, prism, &mut *store)
    } else {
        let mut store = store.lock().expect("workspace store lock poisoned");
        maybe_auto_heartbeat_assisted_leases_in_store(root, prism, &mut *store)
    }
}

fn maybe_auto_heartbeat_assisted_leases_in_store<T>(
    root: &Path,
    prism: &Prism,
    store: &mut T,
) -> Result<bool>
where
    T: CoordinationPersistenceBackend,
{
    let worktree_id = workspace_identity_for_root(root).worktree_id;
    let now = current_timestamp();
    let Some(target) = select_assisted_lease_target(prism, &worktree_id, now) else {
        return Ok(false);
    };
    if !matches!(
        target.due_state(now),
        LeaseHeartbeatDueState::DueSoon | LeaseHeartbeatDueState::DueNow
    ) {
        return Ok(false);
    }
    let Some(last_explicit_ts) = last_explicit_authenticated_target_event_ts(prism, &target) else {
        return Ok(false);
    };
    if now > last_explicit_ts.saturating_add(target.assisted_window()) {
        return Ok(false);
    }

    let before_snapshot = prism.coordination_snapshot();
    let before_plan_graphs = prism.authored_plan_graphs();
    let before_execution_overlays = prism.plan_execution_overlays_by_plan();
    let event_meta = assisted_lease_event_meta(root, &target, now);

    let heartbeat_result = match &target {
        AssistedLeaseTarget::Task { task, .. } => prism
            .heartbeat_native_task(event_meta, &task.id, "watcher_auto")
            .map(|_| ()),
        AssistedLeaseTarget::Claim {
            claim, session_id, ..
        } => prism
            .renew_native_claim(event_meta, session_id, &claim.id, None, "watcher_auto")
            .map(|_| ()),
    };
    if let Err(error) = heartbeat_result {
        prism.replace_coordination_snapshot_and_plan_graphs(
            before_snapshot,
            before_plan_graphs,
            before_execution_overlays,
        );
        return Err(error);
    }

    let snapshot = prism.coordination_snapshot();
    let appended_events = snapshot
        .events
        .iter()
        .filter(|event| {
            !before_snapshot
                .events
                .iter()
                .any(|stored| stored.meta.id == event.meta.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let plan_graphs = prism.authored_plan_graphs();
    let execution_overlays = prism.plan_execution_overlays_by_plan();
    let expected_revision = store.coordination_revision()?;

    if let Err(error) = store
        .persist_coordination_authoritative_mutation_state_for_root_with_session_observed(
            root,
            expected_revision,
            &snapshot,
            &appended_events,
            target.session_id(),
            Some(&before_snapshot),
            Some(&before_plan_graphs),
            Some(&plan_graphs),
            Some(&execution_overlays),
            |_operation, _duration, _args, _success, _error| {},
        )
    {
        prism.replace_coordination_snapshot_and_plan_graphs(
            before_snapshot,
            before_plan_graphs,
            before_execution_overlays,
        );
        return Err(error);
    }

    Ok(true)
}

fn select_assisted_lease_target(
    prism: &Prism,
    worktree_id: &str,
    now: u64,
) -> Option<AssistedLeaseTarget> {
    let task_targets = prism
        .coordination_snapshot()
        .tasks
        .into_iter()
        .filter_map(|task| assisted_task_target(prism, worktree_id, task, now));
    let claim_targets = prism
        .coordination_snapshot()
        .claims
        .into_iter()
        .filter_map(|claim| assisted_claim_target(prism, worktree_id, claim, now));
    let mut targets = task_targets.chain(claim_targets).collect::<Vec<_>>();
    if targets.len() != 1 {
        return None;
    }
    targets.pop()
}

fn assisted_task_target(
    prism: &Prism,
    worktree_id: &str,
    task: CoordinationTask,
    now: u64,
) -> Option<AssistedLeaseTarget> {
    if task.worktree_id.as_deref() != Some(worktree_id) || task.pending_handoff_to.is_some() {
        return None;
    }
    if !matches!(task_lease_state(&task, now), LeaseState::Active) {
        return None;
    }
    let holder = task.lease_holder.clone()?;
    let principal = holder.principal.clone()?;
    let plan = prism.coordination_plan(&task.plan)?;
    if plan.policy.lease_renewal_mode != LeaseRenewalMode::Assisted {
        return None;
    }
    Some(AssistedLeaseTarget::Task {
        task,
        principal,
        session_id: holder.session_id.clone(),
        policy: plan.policy,
    })
}

fn assisted_claim_target(
    prism: &Prism,
    worktree_id: &str,
    claim: WorkClaim,
    now: u64,
) -> Option<AssistedLeaseTarget> {
    if claim.worktree_id.as_deref() != Some(worktree_id) || claim.status != ClaimStatus::Active {
        return None;
    }
    if !matches!(claim_lease_state(&claim, now), LeaseState::Active) {
        return None;
    }
    let holder = claim.lease_holder.as_ref()?;
    let principal = holder.principal.clone()?;
    let task_id = claim.task.as_ref()?;
    let task = prism.coordination_task(task_id)?;
    if task.pending_handoff_to.is_some() {
        return None;
    }
    let plan = prism.coordination_plan(&task.plan)?;
    if plan.policy.lease_renewal_mode != LeaseRenewalMode::Assisted {
        return None;
    }
    let session_id = claim.holder.clone();
    Some(AssistedLeaseTarget::Claim {
        claim,
        principal,
        session_id,
        policy: plan.policy,
    })
}

fn last_explicit_authenticated_target_event_ts(
    prism: &Prism,
    target: &AssistedLeaseTarget,
) -> Option<u64> {
    prism
        .coordination_events()
        .into_iter()
        .rev()
        .find_map(|event| {
            if !target.matches_event(&event) {
                return None;
            }
            let EventActor::Principal(principal) = &event.meta.actor else {
                return None;
            };
            if principal != target.principal() {
                return None;
            }
            event
                .meta
                .execution_context
                .as_ref()
                .and_then(|context| context.credential_id.as_ref().map(|_| event.meta.ts))
        })
}

fn observed_change_event_meta(
    root: &Path,
    observed_change_tracker: &SharedObservedChangeTracker,
    worktree_principal_binding: &Arc<Mutex<Option<BoundWorktreePrincipal>>>,
) -> EventMeta {
    let work = observed_change_tracker
        .lock()
        .expect("observed change tracker lock poisoned")
        .active_work();
    let actor = worktree_principal_binding
        .lock()
        .expect("worktree principal binding lock poisoned")
        .clone()
        .map(|principal| {
            EventActor::Principal(PrincipalActor {
                authority_id: PrincipalAuthorityId::new(principal.authority_id),
                principal_id: PrincipalId::new(principal.principal_id),
                kind: None,
                name: Some(principal.principal_name),
            })
        })
        .unwrap_or(EventActor::System);
    let context = coordination_persist_context_for_root(root, None);
    EventMeta {
        id: EventId::new(new_prefixed_id("observed")),
        ts: current_timestamp(),
        actor,
        correlation: work
            .as_ref()
            .map(|active| {
                active
                    .coordination_task_id
                    .clone()
                    .unwrap_or_else(|| active.work_id.clone())
            })
            .map(TaskId::new),
        causation: None,
        execution_context: Some(EventExecutionContext {
            repo_id: Some(context.repo_id),
            worktree_id: Some(context.worktree_id),
            branch_ref: context.branch_ref,
            session_id: context.session_id,
            instance_id: context.instance_id,
            request_id: None,
            credential_id: None,
            work_context: work.map(|active| WorkContextSnapshot {
                work_id: active.work_id,
                kind: active.kind,
                title: active.title,
                summary: active.summary,
                parent_work_id: active.parent_work_id,
                coordination_task_id: active.coordination_task_id,
                plan_id: active.plan_id,
                plan_title: active.plan_title,
            }),
        }),
    }
}

fn assisted_lease_event_meta(root: &Path, target: &AssistedLeaseTarget, now: u64) -> EventMeta {
    let context = coordination_persist_context_for_root(root, target.session_id());
    EventMeta {
        id: EventId::new(new_prefixed_id("coordination")),
        ts: now,
        actor: EventActor::Principal(target.principal().clone()),
        correlation: target.correlation(),
        causation: None,
        execution_context: Some(EventExecutionContext {
            repo_id: Some(context.repo_id),
            worktree_id: Some(context.worktree_id),
            branch_ref: context.branch_ref,
            session_id: context.session_id,
            instance_id: context.instance_id,
            request_id: None,
            credential_id: None,
            work_context: None,
        }),
    }
}

fn can_scope_watch_refresh(root: &Path, dirty_paths: &[PathBuf]) -> bool {
    dirty_paths.iter().all(|path| path.starts_with(root))
}

fn is_authoritative_protected_state_fallback_path(relative: &Path) -> bool {
    let segments = relative
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    matches!(segments.as_slice(), [prism] if prism == ".prism")
        || matches!(
            segments.as_slice(),
            [prism, second]
                if prism == ".prism"
                    && matches!(
                        second.as_str(),
                        "memory" | "changes" | "concepts" | "contracts" | "plans"
                    )
        )
        || matches!(
            segments.as_slice(),
            [prism, plans, streams]
                if prism == ".prism" && plans == "plans" && streams == "streams"
        )
}

fn relevant_protected_state_streams(root: &Path, event: &Event) -> Vec<ProtectedRepoStream> {
    let mut streams = BTreeMap::<String, ProtectedRepoStream>::new();
    let mut saw_prism_path = false;
    for path in &event.paths {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == ".prism")
        {
            saw_prism_path |= is_authoritative_protected_state_fallback_path(relative);
        }
        let Some(stream) = classify_protected_repo_relative_path(relative) else {
            continue;
        };
        streams.insert(stream.stream_id().to_string(), stream);
    }
    if streams.is_empty() && saw_prism_path {
        streams.insert(
            "memory:events".to_string(),
            ProtectedRepoStream::memory_stream("events.jsonl")
                .expect("well-formed default memory stream"),
        );
        streams.insert(
            "changes:events".to_string(),
            ProtectedRepoStream::patch_events(),
        );
        streams.insert(
            "concepts:events".to_string(),
            ProtectedRepoStream::concept_events(),
        );
        streams.insert(
            "concepts:relations".to_string(),
            ProtectedRepoStream::concept_relations(),
        );
        streams.insert(
            "contracts:events".to_string(),
            ProtectedRepoStream::contract_events(),
        );
        streams.insert(
            "plan:protected-watch".to_string(),
            ProtectedRepoStream::plan_stream(&prism_ir::PlanId::new("plan:protected-watch")),
        );
    }
    streams.into_values().collect()
}

fn relevant_watch_paths(root: &Path, event: &Event) -> Vec<PathBuf> {
    event
        .paths
        .iter()
        .filter_map(|path| {
            let Ok(relative) = path.strip_prefix(root) else {
                return Some(path.clone());
            };
            (!is_ignored_watch_relative_path(relative)).then(|| path.clone())
        })
        .collect()
}

pub(crate) fn sync_protected_state_watch_update(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    shared_runtime: &SharedRuntimeBackend,
    refresh_lock: &Arc<Mutex<()>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
    streams: &[ProtectedRepoStream],
) -> Result<()> {
    let selection = ProtectedStateImportSelection::from_streams(streams.iter());
    if selection.is_empty() {
        return Ok(());
    }
    let _guard = refresh_lock
        .lock()
        .expect("protected-state refresh lock poisoned");
    let workspace_cache_path = cache_path(root)?;
    let shared_runtime_aliases_workspace_store =
        shared_runtime.aliases_sqlite_path(workspace_cache_path.as_path());

    let (report, local_workspace_revision, shared_workspace_revision, plan_state) =
        if let Some(shared_runtime_store) = shared_runtime_store {
            let local_store = store.lock().expect("workspace store lock poisoned");
            let local_workspace_revision = local_store.workspace_revision()?;
            drop(local_store);

            let mut shared_store = shared_runtime_store
                .lock()
                .expect("shared runtime store lock poisoned");
            let report = sync_selected_repo_protected_state(root, &mut *shared_store, selection)?;
            let plan_state = if coordination_enabled && selection.reloads_coordination() {
                load_repo_protected_plan_state(root, &mut *shared_store)?
            } else {
                None
            };
            let shared_workspace_revision = if shared_runtime_aliases_workspace_store {
                None
            } else {
                Some(shared_store.workspace_revision()?)
            };
            (
                report,
                local_workspace_revision,
                shared_workspace_revision,
                plan_state,
            )
        } else {
            let mut local_store = store.lock().expect("workspace store lock poisoned");
            let report = sync_selected_repo_protected_state(root, &mut *local_store, selection)?;
            let plan_state = if coordination_enabled && selection.reloads_coordination() {
                load_repo_protected_plan_state(root, &mut *local_store)?
            } else {
                None
            };
            let local_workspace_revision = local_store.workspace_revision()?;
            (report, local_workspace_revision, None, plan_state)
        };

    let workspace_revision =
        composite_workspace_revision(local_workspace_revision, shared_workspace_revision);
    let mut next_state = runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned")
        .clone();
    if selection.reloads_projection_knowledge() {
        let repo_knowledge = load_repo_protected_knowledge(root)?;
        next_state
            .projections
            .replace_curated_concepts(repo_knowledge.curated_concepts);
        next_state
            .projections
            .replace_curated_contracts(repo_knowledge.curated_contracts);
        next_state
            .projections
            .replace_concept_relations(repo_knowledge.concept_relations);
    }
    if coordination_enabled && selection.reloads_coordination() {
        next_state.replace_coordination_runtime(
            plan_state
                .as_ref()
                .map(|state| state.snapshot.clone())
                .unwrap_or_default(),
            plan_state
                .as_ref()
                .map(|state| state.plan_graphs.clone())
                .unwrap_or_default(),
            plan_state
                .as_ref()
                .map(|state| state.execution_overlays.clone())
                .unwrap_or_default(),
        );
    }
    let stream_ids = streams
        .iter()
        .map(|stream| stream.stream_id().to_string())
        .collect::<Vec<_>>();
    let next = next_state.publish_generation(
        prism_ir::WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        },
        Some(coordination_persist_context_for_root(root, None)),
    );
    WorkspaceSession::attach_cold_query_backends(
        next.prism_arc().as_ref(),
        cold_query_store,
        shared_runtime_store,
    );
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(workspace_revision, Ordering::Relaxed);
    info!(
        root = %root.display(),
        stream_ids = ?stream_ids,
        imported_memory_events = report.imported_memory_events,
        imported_patch_events = report.imported_patch_events,
        reload_projection_knowledge = selection.reloads_projection_knowledge(),
        reload_coordination = selection.reloads_coordination(),
        "applied prism protected-state watch sync"
    );
    Ok(())
}

pub(crate) fn sync_shared_coordination_ref_watch_update(
    root: &Path,
    published_generation: &Arc<RwLock<WorkspacePublishedGeneration>>,
    runtime_state: &Arc<Mutex<WorkspaceRuntimeState>>,
    store: &Arc<Mutex<SqliteStore>>,
    cold_query_store: &Arc<Mutex<SqliteStore>>,
    shared_runtime_store: Option<&Arc<Mutex<SharedRuntimeStore>>>,
    refresh_lock: &Arc<Mutex<()>>,
    loaded_workspace_revision: &Arc<AtomicU64>,
    coordination_runtime_revision: &Arc<AtomicU64>,
    coordination_enabled: bool,
) -> Result<()> {
    if !coordination_enabled {
        return Ok(());
    }
    let SharedCoordinationRefLiveSync::Changed(shared) =
        poll_shared_coordination_ref_live_sync(root)?
    else {
        return Ok(());
    };

    let _guard = refresh_lock
        .lock()
        .expect("shared coordination ref refresh lock poisoned");

    let local_workspace_revision = store
        .lock()
        .expect("workspace store lock poisoned")
        .workspace_revision()?;
    let persisted_coordination_revision = if let Some(shared_store) = shared_runtime_store {
        shared_store
            .lock()
            .expect("shared runtime store lock poisoned")
            .coordination_revision()?
    } else {
        store
            .lock()
            .expect("workspace store lock poisoned")
            .coordination_revision()?
    };
    let shared_workspace_revision = shared_runtime_store
        .map(|store| {
            store
                .lock()
                .expect("shared runtime store lock poisoned")
                .workspace_revision()
        })
        .transpose()?;
    let workspace_revision =
        composite_workspace_revision(local_workspace_revision, shared_workspace_revision);

    let mut next_state = runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned")
        .clone();
    next_state.replace_coordination_runtime(
        shared.snapshot.clone(),
        shared.plan_graphs.clone(),
        shared.execution_overlays.clone(),
    );
    let next = next_state.publish_generation(
        prism_ir::WorkspaceRevision {
            graph_version: local_workspace_revision,
            git_commit: None,
        },
        Some(coordination_persist_context_for_root(root, None)),
    );
    WorkspaceSession::attach_cold_query_backends(
        next.prism_arc().as_ref(),
        cold_query_store,
        shared_runtime_store,
    );
    *runtime_state
        .lock()
        .expect("workspace runtime state lock poisoned") = next_state;
    *published_generation
        .write()
        .expect("workspace published generation lock poisoned") = next;
    loaded_workspace_revision.store(workspace_revision, Ordering::Relaxed);
    let next_coordination_revision = coordination_runtime_revision
        .load(Ordering::Relaxed)
        .max(persisted_coordination_revision)
        .saturating_add(1);
    coordination_runtime_revision.store(next_coordination_revision, Ordering::Relaxed);
    info!(
        root = %root.display(),
        plan_count = shared.snapshot.plans.len(),
        task_count = shared.snapshot.tasks.len(),
        claim_count = shared.snapshot.claims.len(),
        artifact_count = shared.snapshot.artifacts.len(),
        review_count = shared.snapshot.reviews.len(),
        "applied shared coordination ref live sync"
    );
    Ok(())
}

fn is_ignored_watch_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return false;
    }

    if is_generated_projection_relative_path(relative) {
        return true;
    }

    if components.iter().any(|component| {
        matches!(
            component.as_str(),
            ".git" | ".prism" | "target" | "node_modules"
        )
    }) {
        return true;
    }

    matches!(
        components.as_slice(),
        [first, second, ..]
            if first == "benchmarks"
                && matches!(second.as_str(), "external" | "results")
    )
}

fn format_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::{
        can_scope_watch_refresh, is_ignored_watch_relative_path,
        maybe_auto_heartbeat_assisted_leases_in_store, relevant_protected_state_streams,
        relevant_watch_paths,
    };
    use notify::{
        event::{EventAttributes, ModifyKind},
        Event, EventKind,
    };
    use prism_coordination::{
        CoordinationPolicy, CoordinationStore, PlanCreateInput, TaskCreateInput,
    };
    use prism_ir::{AgentId, EventActor, EventExecutionContext, EventId, EventMeta, SessionId};
    use prism_query::Prism;
    use prism_store::{CoordinationJournal, Graph, MemoryStore};
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::util::current_timestamp;
    use crate::workspace_identity::coordination_persist_context_for_root;

    static ASSISTED_LEASE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(prism_ir::new_prefixed_id(name).to_string());
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn modify_event(path: PathBuf) -> Event {
        Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![path],
            attrs: EventAttributes::new(),
        }
    }

    #[test]
    fn ignored_watch_paths_skip_generated_benchmark_results() {
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("PRISM.md").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("docs/prism/plans/index.md").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("benchmarks/results/local/prism/workspaces/demo/repo/src/lib.rs")
                .as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("benchmarks/external/demo/src/lib.rs").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from("node_modules/pkg/index.json").as_path()
        ));
        assert!(is_ignored_watch_relative_path(
            PathBuf::from(".prism/plans/streams/plan:1.jsonl").as_path()
        ));
        assert!(!is_ignored_watch_relative_path(
            PathBuf::from("crates/prism-core/src/watch.rs").as_path()
        ));
    }

    #[test]
    fn relevant_watch_paths_drop_ignored_generated_paths() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![
                root.join("PRISM.md"),
                root.join("docs/prism/plans/index.md"),
                root.join(".prism/plans/streams/plan:1.jsonl"),
                root.join("benchmarks/results/local/prism/workspaces/demo/repo/src/lib.rs"),
                root.join("crates/prism-core/src/watch.rs"),
            ],
            attrs: EventAttributes::new(),
        };

        let paths = relevant_watch_paths(&root, &event);
        assert_eq!(paths, vec![root.join("crates/prism-core/src/watch.rs")]);
    }

    #[test]
    fn relevant_protected_state_streams_detect_authoritative_prism_paths() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![
                root.join(".prism/memory/events.jsonl"),
                root.join(".prism/concepts/events.jsonl"),
            ],
            attrs: EventAttributes::new(),
        };

        let streams = relevant_protected_state_streams(&root, &event);
        assert!(streams
            .iter()
            .any(|stream| stream.stream_id() == "memory:events"));
        assert!(streams
            .iter()
            .any(|stream| stream.stream_id() == "concepts:events"));
    }

    #[test]
    fn relevant_protected_state_streams_fallback_for_prism_directory_events() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![root.join(".prism/memory")],
            attrs: EventAttributes::new(),
        };

        let streams = relevant_protected_state_streams(&root, &event);
        assert!(streams
            .iter()
            .any(|stream| stream.stream_id() == "memory:events"));
        assert!(streams
            .iter()
            .any(|stream| stream.stream_id() == "concepts:events"));
        assert!(streams
            .iter()
            .any(|stream| stream.stream().starts_with("repo_plan")));
    }

    #[test]
    fn relevant_protected_state_streams_ignore_snapshot_outputs() {
        let root = PathBuf::from("/workspace/prism");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![
                root.join(".prism/state/manifest.json"),
                root.join(".prism/state/plans/plan-demo.json"),
                root.join(".prism/plans/active/plan:demo.jsonl"),
            ],
            attrs: EventAttributes::new(),
        };

        let streams = relevant_protected_state_streams(&root, &event);
        assert!(streams.is_empty());
    }

    #[test]
    fn relevant_watch_paths_keep_out_of_root_events() {
        let root = PathBuf::from("/workspace/prism");
        let outside = PathBuf::from("/tmp/external.rs");
        let event = modify_event(outside.clone());
        let paths = relevant_watch_paths(&root, &event);
        assert_eq!(paths, vec![outside]);
    }

    #[test]
    fn scoped_watch_refresh_requires_in_root_paths() {
        let root = PathBuf::from("/workspace/prism");
        assert!(can_scope_watch_refresh(
            &root,
            &[root.join("docs/guide.md"), root.join("src/lib.rs")]
        ));
        assert!(!can_scope_watch_refresh(
            &root,
            &[
                root.join("docs/guide.md"),
                PathBuf::from("/tmp/editor-copy.md")
            ]
        ));
    }

    fn principal_meta(id: &str, ts: u64, credential_id: Option<&str>) -> EventMeta {
        EventMeta {
            id: EventId::new(id),
            ts,
            actor: EventActor::Principal(prism_ir::PrincipalActor {
                authority_id: prism_ir::PrincipalAuthorityId::new("local"),
                principal_id: prism_ir::PrincipalId::new("agent:a"),
                kind: Some(prism_ir::PrincipalKind::Agent),
                name: Some("agent:a".to_string()),
            }),
            correlation: None,
            causation: None,
            execution_context: Some(EventExecutionContext {
                repo_id: None,
                worktree_id: None,
                branch_ref: None,
                session_id: Some("session:a".to_string()),
                instance_id: None,
                request_id: None,
                credential_id: credential_id.map(prism_ir::CredentialId::new),
                work_context: None,
            }),
        }
    }

    #[test]
    fn assisted_watcher_heartbeat_renews_single_due_task() {
        let _guard = ASSISTED_LEASE_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(super::ASSISTED_LEASE_RENEWAL_ENV, "1") };
        let root = temp_root("watch-heartbeat");
        let prism = Prism::new(Graph::default());
        let coordination_context = coordination_persist_context_for_root(root.as_path(), None);
        prism.set_coordination_context(Some(coordination_context.clone()));
        let now = current_timestamp();
        let coordination = CoordinationStore::new();
        let (plan_id, _) = coordination
            .create_plan(
                principal_meta(
                    "coord:plan:watcher-auto",
                    now.saturating_sub(260),
                    Some("credential:explicit"),
                ),
                PlanCreateInput {
                    title: "Watcher auto heartbeat".to_string(),
                    goal: "Watcher auto heartbeat".to_string(),
                    status: Some(prism_ir::PlanStatus::Active),
                    policy: Some(CoordinationPolicy {
                        lease_stale_after_seconds: 300,
                        lease_expires_after_seconds: 900,
                        lease_renewal_mode: prism_ir::LeaseRenewalMode::Assisted,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        let (task_id, _) = coordination
            .create_task(
                principal_meta(
                    "coord:task:watcher-auto",
                    now.saturating_sub(250),
                    Some("credential:explicit"),
                ),
                TaskCreateInput {
                    plan_id,
                    title: "Edit alpha".to_string(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: Some(AgentId::new("agent:a")),
                    session: Some(SessionId::new("session:a")),
                    worktree_id: Some(coordination_context.worktree_id),
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: prism_ir::WorkspaceRevision::default(),
                },
            )
            .unwrap();
        prism.replace_coordination_snapshot(coordination.snapshot());

        let mut store = MemoryStore::default();
        let worktree_id = super::workspace_identity_for_root(root.as_path()).worktree_id;
        let target = super::select_assisted_lease_target(&prism, &worktree_id, current_timestamp())
            .unwrap_or_else(|| {
                panic!(
                    "missing assisted target: {:?}",
                    prism.coordination_snapshot().tasks
                )
            });
        assert!(matches!(
            target.due_state(current_timestamp()),
            prism_coordination::LeaseHeartbeatDueState::DueSoon
                | prism_coordination::LeaseHeartbeatDueState::DueNow
        ));
        assert!(super::last_explicit_authenticated_target_event_ts(&prism, &target).is_some());
        let changed =
            maybe_auto_heartbeat_assisted_leases_in_store(root.as_path(), &prism, &mut store)
                .expect("assisted heartbeat should succeed");

        assert!(changed);
        let event = prism
            .coordination_events()
            .last()
            .expect("heartbeat event should exist")
            .clone();
        assert_eq!(event.kind, prism_ir::CoordinationEventKind::TaskHeartbeated);
        assert_eq!(event.task.as_ref(), Some(&task_id));
        assert_eq!(event.metadata["renewalProvenance"], "watcher_auto");
        let context = event
            .meta
            .execution_context
            .expect("watcher auto heartbeat should record execution context");
        assert!(context.credential_id.is_none());
        assert_eq!(store.coordination_revision().unwrap(), 1);
        unsafe { std::env::remove_var(super::ASSISTED_LEASE_RENEWAL_ENV) };
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn assisted_watcher_heartbeat_skips_when_multiple_active_leases_exist() {
        let _guard = ASSISTED_LEASE_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(super::ASSISTED_LEASE_RENEWAL_ENV, "1") };
        let root = temp_root("watch-heartbeat-skip");
        let prism = Prism::new(Graph::default());
        let coordination_context = coordination_persist_context_for_root(root.as_path(), None);
        prism.set_coordination_context(Some(coordination_context.clone()));
        let now = current_timestamp();
        let coordination = CoordinationStore::new();
        let (plan_id, _) = coordination
            .create_plan(
                principal_meta(
                    "coord:plan:watcher-skip",
                    now.saturating_sub(260),
                    Some("credential:explicit"),
                ),
                PlanCreateInput {
                    title: "Watcher skip on ambiguity".to_string(),
                    goal: "Watcher skip on ambiguity".to_string(),
                    status: Some(prism_ir::PlanStatus::Active),
                    policy: Some(CoordinationPolicy {
                        lease_stale_after_seconds: 300,
                        lease_expires_after_seconds: 900,
                        lease_renewal_mode: prism_ir::LeaseRenewalMode::Assisted,
                        ..CoordinationPolicy::default()
                    }),
                },
            )
            .unwrap();
        for (index, title) in ["Edit alpha", "Edit beta"].into_iter().enumerate() {
            coordination
                .create_task(
                    principal_meta(
                        &format!("coord:task:watcher-skip:{index}"),
                        now.saturating_sub(250),
                        Some("credential:explicit"),
                    ),
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: title.to_string(),
                        status: Some(prism_ir::CoordinationTaskStatus::Ready),
                        assignee: Some(AgentId::new("agent:a")),
                        session: Some(SessionId::new("session:a")),
                        worktree_id: Some(coordination_context.worktree_id.clone()),
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism_ir::WorkspaceRevision::default(),
                    },
                )
                .unwrap();
        }
        prism.replace_coordination_snapshot(coordination.snapshot());

        let event_count = prism.coordination_events().len();
        let mut store = MemoryStore::default();
        let changed =
            maybe_auto_heartbeat_assisted_leases_in_store(root.as_path(), &prism, &mut store)
                .expect("ambiguous assisted heartbeat should be skipped cleanly");

        assert!(!changed);
        assert_eq!(prism.coordination_events().len(), event_count);
        assert_eq!(store.coordination_revision().unwrap(), 0);
        unsafe { std::env::remove_var(super::ASSISTED_LEASE_RENEWAL_ENV) };
        let _ = std::fs::remove_dir_all(root);
    }
}
