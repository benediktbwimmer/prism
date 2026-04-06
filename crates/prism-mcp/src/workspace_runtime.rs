use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_agent::InferenceStore;
use prism_core::runtime_engine::{
    RuntimeDomain, RuntimeDomainState, RuntimeFreshnessState, RuntimeMaterializationDepth,
    WorkspaceFileDelta, WorkspaceFileSemanticFacts, WorkspaceRuntimeCoalescingKey,
    WorkspaceRuntimeCommand, WorkspaceRuntimeCommandKind, WorkspaceRuntimeEngine,
    WorkspaceRuntimePathRequest, WorkspaceRuntimeQueueClass, WorkspaceRuntimeQueueSnapshot,
};
use prism_core::{
    FsRefreshStatus, WorkspaceRefreshBreakdown, WorkspaceRefreshWork, WorkspaceSession,
    WorkspaceSnapshotRevisions,
};
use prism_ir::ObservedChangeSet;
use prism_memory::{EpisodicMemorySnapshot, SessionMemory};
use serde::Serialize;
use tracing::{debug, error};

use crate::{
    diagnostics_state::DiagnosticsState,
    log_refresh_workspace,
    mcp_call_log::McpCallLogStore,
    runtime_views::refresh_cached_runtime_status_for_config,
    workspace_host::{
        SharedWorkspaceReadSync, SharedWorkspaceReadSyncDecision, SharedWorkspaceRuntimeRevisions,
    },
    QueryHost, WorkspaceRefreshMetrics, WorkspaceRefreshReport,
};

const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const BACKGROUND_LANE_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const MUTATION_RUNTIME_SYNC_WAIT_TIMEOUT: Duration = Duration::from_millis(1500);
const MUTATION_RUNTIME_SYNC_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const READ_RUNTIME_SYNC_JOIN_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeConfig {
    pub(crate) workspace: Arc<WorkspaceSession>,
    pub(crate) notes: Arc<SessionMemory>,
    pub(crate) inferred_edges: Arc<InferenceStore>,
    pub(crate) diagnostics_state: Arc<DiagnosticsState>,
    pub(crate) mcp_call_log_store: Arc<McpCallLogStore>,
    pub(crate) runtime_diagnostics_auto_refresh: bool,
    pub(crate) sync_lock: Arc<RwLock<()>>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) loaded_episodic_revision: Arc<AtomicU64>,
    pub(crate) loaded_inference_revision: Arc<AtomicU64>,
    pub(crate) loaded_coordination_revision: Arc<AtomicU64>,
    pub(crate) current_revisions: Arc<SharedWorkspaceRuntimeRevisions>,
    pub(crate) read_sync: Arc<SharedWorkspaceReadSync>,
    pub(crate) runtime_engine: Arc<Mutex<WorkspaceRuntimeEngine>>,
    pub(crate) prepared_delta: Arc<Mutex<Option<PreparedWorkspaceRuntimeDelta>>>,
}

fn maybe_refresh_cached_runtime_status_for_config(config: &WorkspaceRuntimeConfig) {
    if !config.runtime_diagnostics_auto_refresh {
        return;
    }
    let _ = refresh_cached_runtime_status_for_config(
        &crate::workspace_diagnostics::WorkspaceDiagnosticsConfig {
            workspace: Arc::clone(&config.workspace),
            loaded_workspace_revision: Arc::clone(&config.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&config.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&config.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&config.loaded_coordination_revision),
            runtime_engine: Arc::clone(&config.runtime_engine),
            diagnostics_state: Arc::clone(&config.diagnostics_state),
            mcp_call_log_store: Arc::clone(&config.mcp_call_log_store),
        },
    );
}

fn sync_current_runtime_revisions(
    config: &WorkspaceRuntimeConfig,
    revisions: &WorkspaceSnapshotRevisions,
) {
    config
        .current_revisions
        .current_workspace_revision()
        .store(revisions.workspace, Ordering::Relaxed);
    config
        .current_revisions
        .current_episodic_revision()
        .store(revisions.episodic, Ordering::Relaxed);
    config
        .current_revisions
        .current_inference_revision()
        .store(revisions.inference, Ordering::Relaxed);
    config
        .current_revisions
        .current_coordination_revision()
        .store(revisions.coordination, Ordering::Relaxed);
}

fn runtime_read_is_current(config: &WorkspaceRuntimeConfig) -> bool {
    config.loaded_workspace_revision.load(Ordering::Relaxed)
        == config
            .current_revisions
            .current_workspace_revision()
            .load(Ordering::Relaxed)
        && config.loaded_episodic_revision.load(Ordering::Relaxed)
            == config
                .current_revisions
                .current_episodic_revision()
                .load(Ordering::Relaxed)
        && config.loaded_inference_revision.load(Ordering::Relaxed)
            == config
                .current_revisions
                .current_inference_revision()
                .load(Ordering::Relaxed)
        && config.loaded_coordination_revision.load(Ordering::Relaxed)
            == config
                .current_revisions
                .current_coordination_revision()
                .load(Ordering::Relaxed)
}

fn runtime_read_fast_path_available(config: &WorkspaceRuntimeConfig) -> bool {
    !config.workspace.needs_refresh()
        && !config.workspace.is_fallback_check_due_now()
        && runtime_read_is_current(config)
}

struct SharedReadSyncLeader<'a> {
    gate: &'a SharedWorkspaceReadSync,
}

impl<'a> SharedReadSyncLeader<'a> {
    fn new(gate: &'a SharedWorkspaceReadSync) -> Self {
        Self { gate }
    }
}

impl Drop for SharedReadSyncLeader<'_> {
    fn drop(&mut self) {
        self.gate.finish();
    }
}

pub(crate) struct WorkspaceRuntime {
    engine: Arc<Mutex<WorkspaceRuntimeEngine>>,
    wake: SyncSender<()>,
    stop: mpsc::Sender<()>,
    handle: Mutex<Option<JoinHandle<()>>>,
    lane_senders: HashMap<BackgroundRuntimeLane, SyncSender<WorkspaceRuntimeCommand>>,
    background_workers: Vec<BackgroundRuntimeLaneWorker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BackgroundRuntimeLane {
    Settle(RuntimeDomain),
    Checkpoint,
}

struct BackgroundRuntimeLaneWorker {
    stop: mpsc::Sender<()>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl WorkspaceRefreshReport {
    pub(crate) fn none() -> Self {
        Self {
            refresh_path: "none",
            runtime_sync_used: false,
            deferred: false,
            episodic_reloaded: false,
            inference_reloaded: false,
            coordination_reloaded: false,
            metrics: WorkspaceRefreshMetrics::default(),
        }
    }
}

fn refresh_work(metrics: WorkspaceRefreshMetrics) -> WorkspaceRefreshWork {
    WorkspaceRefreshWork {
        loaded_bytes: metrics.loaded_bytes,
        replay_volume: metrics.replay_volume,
        full_rebuild_count: metrics.full_rebuild_count,
        workspace_reloaded: metrics.workspace_reloaded,
    }
}

fn apply_refresh_breakdown(
    metrics: &mut WorkspaceRefreshMetrics,
    breakdown: WorkspaceRefreshBreakdown,
) {
    metrics.plan_refresh_ms = breakdown.plan_refresh_ms;
    metrics.build_indexer_ms = breakdown.build_indexer_ms;
    metrics.index_workspace_ms = breakdown.index_workspace_ms;
    metrics.publish_generation_ms = breakdown.publish_generation_ms;
    metrics.assisted_lease_ms = breakdown.assisted_lease_ms;
    metrics.curator_enqueue_ms = breakdown.curator_enqueue_ms;
    metrics.attach_cold_query_backends_ms = breakdown.attach_cold_query_backends_ms;
    metrics.finalize_refresh_state_ms = breakdown.finalize_refresh_state_ms;
}

fn dirty_workspace_deferred_report(
    config: &WorkspaceRuntimeConfig,
    runtime_sync_used: bool,
    metrics: WorkspaceRefreshMetrics,
) -> WorkspaceRefreshReport {
    config
        .workspace
        .record_runtime_refresh_observation_with_work(
            "deferred",
            metrics.lock_hold_ms,
            refresh_work(metrics),
        );
    maybe_refresh_cached_runtime_status_for_config(config);
    WorkspaceRefreshReport {
        refresh_path: "deferred",
        runtime_sync_used,
        deferred: true,
        episodic_reloaded: false,
        inference_reloaded: false,
        coordination_reloaded: false,
        metrics,
    }
}

#[derive(Clone, Copy, Default)]
struct ReloadMaterialization {
    loaded_bytes: u64,
    replay_volume: u64,
}

impl ReloadMaterialization {
    fn add(self, other: Self) -> Self {
        Self {
            loaded_bytes: self.loaded_bytes.saturating_add(other.loaded_bytes),
            replay_volume: self.replay_volume.saturating_add(other.replay_volume),
        }
    }
}

struct WorkspaceRuntimeCommandOutcome {
    report: WorkspaceRefreshReport,
    follow_up_commands: Vec<WorkspaceRuntimeCommand>,
    published_generation: bool,
}

impl WorkspaceRuntimeCommandOutcome {
    fn new(report: WorkspaceRefreshReport) -> Self {
        Self {
            report,
            follow_up_commands: Vec::new(),
            published_generation: true,
        }
    }

    fn with_follow_up_commands(
        report: WorkspaceRefreshReport,
        follow_up_commands: Vec<WorkspaceRuntimeCommand>,
    ) -> Self {
        Self {
            report,
            follow_up_commands,
            published_generation: true,
        }
    }

    fn prepared(
        report: WorkspaceRefreshReport,
        follow_up_commands: Vec<WorkspaceRuntimeCommand>,
    ) -> Self {
        Self {
            report,
            follow_up_commands,
            published_generation: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedWorkspaceRuntimeDelta {
    report: WorkspaceRefreshReport,
    revisions: WorkspaceSnapshotRevisions,
    file_deltas: Vec<WorkspaceFileDelta>,
}

impl WorkspaceRuntime {
    pub(crate) fn spawn(config: WorkspaceRuntimeConfig) -> Self {
        let engine = Arc::clone(&config.runtime_engine);
        let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (background_senders, background_workers) =
            spawn_background_runtime_lanes(config.clone(), wake_tx.clone());
        let loop_background_senders = background_senders.clone();
        let handle = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            let timed_out = match wake_rx.recv_timeout(BACKGROUND_REFRESH_INTERVAL) {
                Ok(()) => false,
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if stop_rx.try_recv().is_ok() {
                break;
            }
            let command = {
                let mut engine = config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned");
                if let Some(command) = engine.start_next_command() {
                    Some(command)
                } else if timed_out
                    && engine.begin_ad_hoc_command(WorkspaceRuntimeCommand::new(
                        WorkspaceRuntimeCommandKind::MaterializeCheckpoint,
                        WorkspaceRuntimeQueueClass::CheckpointMaterialization,
                        WorkspaceRuntimeCoalescingKey::WorktreeContext,
                    ))
                {
                    Some(WorkspaceRuntimeCommand::new(
                        WorkspaceRuntimeCommandKind::MaterializeCheckpoint,
                        WorkspaceRuntimeQueueClass::CheckpointMaterialization,
                        WorkspaceRuntimeCoalescingKey::WorktreeContext,
                    ))
                } else {
                    None
                }
            };
            let Some(command) = command else {
                continue;
            };
            if let Some(lane) = background_lane_for_command(&command) {
                let sender = loop_background_senders
                    .get(&lane)
                    .expect("background runtime lane should exist");
                match sender.try_send(command.clone()) {
                    Ok(()) => {
                        config
                            .runtime_engine
                            .lock()
                            .expect("workspace runtime engine lock poisoned")
                            .finish_active_command();
                        continue;
                    }
                    Err(TrySendError::Full(_)) => {
                        config
                            .runtime_engine
                            .lock()
                            .expect("workspace runtime engine lock poisoned")
                            .retry_active_command();
                        thread::sleep(BACKGROUND_LANE_RETRY_INTERVAL);
                        continue;
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        error!(
                            root = %config.workspace.root().display(),
                            lane = ?lane,
                            "background runtime lane disconnected"
                        );
                    }
                }
            }
            if command_is_stale_against_generation(
                &command,
                &config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .published_generation_snapshot(),
            ) {
                config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .finish_active_command();
                continue;
            }
            let sync_result = run_workspace_runtime_command(&config, &command);
            if let Err(error) = sync_result {
                if is_transient_sqlite_lock(&error) {
                    config
                        .runtime_engine
                        .lock()
                        .expect("workspace runtime engine lock poisoned")
                        .retry_active_command();
                    config
                        .workspace
                        .record_runtime_refresh_observation_with_work(
                            "deferred",
                            0,
                            WorkspaceRefreshWork::default(),
                        );
                    debug!(
                        root = %config.workspace.root().display(),
                        error = %error,
                        "prism-mcp background workspace refresh deferred by sqlite lock contention"
                    );
                    continue;
                }
                config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .finish_active_command();
                error!(
                    root = %config.workspace.root().display(),
                    error = %error,
                    error_chain = %crate::logging::format_error_chain(&error),
                    "prism-mcp background workspace refresh failed"
                );
                continue;
            }
            let outcome = sync_result.expect("success handled above");
            if outcome.report.deferred {
                config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .retry_active_command();
                continue;
            }
            if outcome.published_generation {
                maybe_refresh_cached_runtime_status_for_config(&config);
            }
            config
                .runtime_engine
                .lock()
                .expect("workspace runtime engine lock poisoned")
                .complete_active_command(outcome.follow_up_commands);
        });
        Self {
            engine,
            wake: wake_tx,
            stop: stop_tx,
            handle: Mutex::new(Some(handle)),
            lane_senders: background_senders,
            background_workers,
        }
    }

    pub(crate) fn request_refresh_with_revisions(
        &self,
        path_requests: Vec<WorkspaceRuntimePathRequest>,
    ) {
        let _ = self
            .engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .enqueue_command(WorkspaceRuntimeCommand::with_path_requests(
                WorkspaceRuntimeCommandKind::PreparePaths,
                WorkspaceRuntimeQueueClass::FastPrepare,
                WorkspaceRuntimeCoalescingKey::WorktreeContext,
                path_requests,
            ));
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                debug!("workspace runtime wake channel disconnected");
            }
        }
    }

    pub(crate) fn queue_snapshot(&self) -> WorkspaceRuntimeQueueSnapshot {
        self.engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .queue_snapshot()
    }

    pub(crate) fn request_settle_domain(&self, domain: RuntimeDomain) {
        let Some(sender) = self
            .lane_senders
            .get(&BackgroundRuntimeLane::Settle(domain))
        else {
            return;
        };
        let command = WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::SettleDomain(domain),
            WorkspaceRuntimeQueueClass::Settle,
            WorkspaceRuntimeCoalescingKey::Domain(domain),
        );
        match sender.try_send(command) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                debug!("workspace runtime settle lane disconnected");
            }
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                debug!("workspace runtime wake channel disconnected");
            }
        }
    }
}

fn sync_workspace_runtime_materialization(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let guard = config
        .sync_lock
        .write()
        .expect("workspace runtime sync lock poisoned");
    sync_workspace_runtime_checkpoint_with_guard(config, guard, elapsed_ms(lock_wait_started))
}

impl Drop for WorkspaceRuntime {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                debug!("workspace runtime wake channel disconnected during drop");
            }
        }
        if let Some(handle) = self
            .handle
            .lock()
            .expect("workspace runtime handle lock poisoned")
            .take()
        {
            let _ = handle.join();
        }
        self.lane_senders.clear();
        for worker in &self.background_workers {
            let _ = worker.stop.send(());
            if let Some(handle) = worker
                .handle
                .lock()
                .expect("background runtime lane handle lock poisoned")
                .take()
            {
                let _ = handle.join();
            }
        }
    }
}

fn spawn_background_runtime_lanes(
    config: WorkspaceRuntimeConfig,
    wake: SyncSender<()>,
) -> (
    HashMap<BackgroundRuntimeLane, SyncSender<WorkspaceRuntimeCommand>>,
    Vec<BackgroundRuntimeLaneWorker>,
) {
    let mut senders = HashMap::new();
    let mut workers = Vec::new();
    for lane in [
        BackgroundRuntimeLane::Settle(RuntimeDomain::MemoryReanchor),
        BackgroundRuntimeLane::Settle(RuntimeDomain::Projections),
        BackgroundRuntimeLane::Settle(RuntimeDomain::Coordination),
        BackgroundRuntimeLane::Checkpoint,
    ] {
        let (tx, rx) = mpsc::sync_channel::<WorkspaceRuntimeCommand>(1);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let lane_config = config.clone();
        let lane_wake = wake.clone();
        let handle = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            let command = match rx.recv_timeout(BACKGROUND_REFRESH_INTERVAL) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if stop_rx.try_recv().is_ok() {
                break;
            }
            if command_is_stale_against_generation(
                &command,
                &lane_config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .published_generation_snapshot(),
            ) {
                continue;
            }
            let result = run_workspace_runtime_command(&lane_config, &command);
            match result {
                Ok(outcome) => {
                    if outcome.report.deferred {
                        let _ = lane_config
                            .runtime_engine
                            .lock()
                            .expect("workspace runtime engine lock poisoned")
                            .enqueue_command(command.clone());
                    } else {
                        if outcome.published_generation {
                            maybe_refresh_cached_runtime_status_for_config(&lane_config);
                        }
                        let mut engine = lane_config
                            .runtime_engine
                            .lock()
                            .expect("workspace runtime engine lock poisoned");
                        for follow_up in outcome.follow_up_commands {
                            let _ = engine.enqueue_command(follow_up);
                        }
                    }
                }
                Err(error) => {
                    if is_transient_sqlite_lock(&error) {
                        let _ = lane_config
                            .runtime_engine
                            .lock()
                            .expect("workspace runtime engine lock poisoned")
                            .enqueue_command(command.clone());
                        lane_config
                            .workspace
                            .record_runtime_refresh_observation_with_work(
                                "deferred",
                                0,
                                WorkspaceRefreshWork::default(),
                            );
                        debug!(
                            root = %lane_config.workspace.root().display(),
                            lane = ?lane,
                            error = %error,
                            "prism-mcp background runtime lane deferred by sqlite lock contention"
                        );
                    } else {
                        error!(
                            root = %lane_config.workspace.root().display(),
                            lane = ?lane,
                            error = %error,
                            error_chain = %crate::logging::format_error_chain(&error),
                            "prism-mcp background runtime lane failed"
                        );
                    }
                }
            }
            match lane_wake.try_send(()) {
                Ok(()) | Err(TrySendError::Full(())) => {}
                Err(TrySendError::Disconnected(())) => {
                    debug!("workspace runtime wake channel disconnected");
                }
            }
        });
        senders.insert(lane, tx);
        workers.push(BackgroundRuntimeLaneWorker {
            stop: stop_tx,
            handle: Mutex::new(Some(handle)),
        });
    }
    (senders, workers)
}

fn run_workspace_runtime_command(
    config: &WorkspaceRuntimeConfig,
    command: &WorkspaceRuntimeCommand,
) -> Result<WorkspaceRuntimeCommandOutcome> {
    match command.kind {
        WorkspaceRuntimeCommandKind::PreparePaths => {
            run_workspace_prepare_paths_command(config, command.path_requests.as_slice())
        }
        WorkspaceRuntimeCommandKind::ApplyPreparedDelta => {
            apply_prepared_workspace_delta_command(config)
        }
        WorkspaceRuntimeCommandKind::SettleDomain(domain) => {
            sync_workspace_settle_domain(config, domain).map(WorkspaceRuntimeCommandOutcome::new)
        }
        WorkspaceRuntimeCommandKind::MaterializeCheckpoint => {
            sync_workspace_runtime_materialization(config).map(WorkspaceRuntimeCommandOutcome::new)
        }
        _ => sync_workspace_runtime(config).map(WorkspaceRuntimeCommandOutcome::new),
    }
}

fn command_is_stale_against_generation(
    command: &WorkspaceRuntimeCommand,
    generation: &prism_core::runtime_engine::WorkspacePublishedGeneration,
) -> bool {
    matches!(command.kind, WorkspaceRuntimeCommandKind::SettleDomain(_))
        && command
            .target_generation
            .is_some_and(|target| target < generation.id)
}

fn background_lane_for_command(command: &WorkspaceRuntimeCommand) -> Option<BackgroundRuntimeLane> {
    match command.kind {
        WorkspaceRuntimeCommandKind::SettleDomain(domain) => {
            Some(BackgroundRuntimeLane::Settle(domain))
        }
        WorkspaceRuntimeCommandKind::MaterializeCheckpoint => {
            Some(BackgroundRuntimeLane::Checkpoint)
        }
        _ => None,
    }
}

pub(crate) fn sync_workspace_runtime(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let guard = config
        .sync_lock
        .write()
        .expect("workspace runtime sync lock poisoned");
    sync_workspace_runtime_with_guard(config, guard, elapsed_ms(lock_wait_started))
}

fn sync_workspace_runtime_with_guard(
    config: &WorkspaceRuntimeConfig,
    _guard: RwLockWriteGuard<'_, ()>,
    lock_wait_ms: u64,
) -> Result<WorkspaceRefreshReport> {
    let started = Instant::now();
    let fs_refresh_started = Instant::now();
    let refresh_path = match config.workspace.refresh_fs_nonblocking()? {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Incremental => "incremental",
        FsRefreshStatus::Rescan => "rescan",
        FsRefreshStatus::Full => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let fs_refresh_ms = elapsed_ms(fs_refresh_started);
    let deferred = refresh_path == "deferred";
    if deferred {
        let duration_ms = started.elapsed().as_millis();
        return Ok(dirty_workspace_deferred_report(
            config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms,
                lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
                fs_refresh_ms,
                ..WorkspaceRefreshMetrics::default()
            },
        ));
    }
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);
    let (
        episodic_reload,
        inference_reload,
        coordination_reload,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
    ) = if deferred {
        (None, None, None, 0, 0, 0)
    } else {
        config.loaded_workspace_revision.store(
            config.workspace.loaded_workspace_revision(),
            Ordering::Relaxed,
        );
        let episodic_started = Instant::now();
        let episodic_reload = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
        let load_episodic_ms = elapsed_ms(episodic_started);
        let inference_started = Instant::now();
        let inference_reload = reload_inference_snapshot_if_needed(config, revisions.inference)?;
        let load_inference_ms = elapsed_ms(inference_started);
        let coordination_started = Instant::now();
        let coordination_reload =
            reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
        let load_coordination_ms = elapsed_ms(coordination_started);
        (
            episodic_reload,
            inference_reload,
            coordination_reload,
            load_episodic_ms,
            load_inference_ms,
            load_coordination_ms,
        )
    };
    let episodic_reloaded = episodic_reload.is_some();
    let inference_reloaded = inference_reload.is_some();
    let coordination_reloaded = coordination_reload.is_some();
    let refresh_path = if refresh_path == "none"
        && (episodic_reloaded || inference_reloaded || coordination_reloaded)
    {
        "auxiliary"
    } else {
        refresh_path
    };
    let reload_materialization = episodic_reload
        .unwrap_or_default()
        .add(inference_reload.unwrap_or_default())
        .add(coordination_reload.unwrap_or_default());
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms,
        snapshot_revisions_ms,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
        loaded_bytes: reload_materialization.loaded_bytes,
        replay_volume: reload_materialization.replay_volume,
        full_rebuild_count: u64::from(refresh_path == "full"),
        workspace_reloaded: refresh_path == "full",
        ..WorkspaceRefreshMetrics::default()
    };
    if should_publish_runtime_generation(refresh_path, &[]) {
        publish_runtime_generation(
            config,
            &revisions,
            refresh_path,
            Vec::new(),
            if matches!(refresh_path, "incremental" | "rescan" | "full") {
                Some(true)
            } else {
                None
            },
        );
    }
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation_with_work(
                refresh_path,
                metrics.lock_hold_ms,
                refresh_work(metrics),
            );
    }
    log_refresh_workspace(
        refresh_path,
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
        metrics,
    );
    Ok(WorkspaceRefreshReport {
        refresh_path,
        runtime_sync_used: true,
        deferred,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        metrics,
    })
}

fn sync_workspace_runtime_for_read_with_guard(
    config: &WorkspaceRuntimeConfig,
    _guard: RwLockReadGuard<'_, ()>,
    lock_wait_ms: u64,
) -> Result<WorkspaceRefreshReport> {
    if config.workspace.needs_refresh() {
        return Ok(dirty_workspace_deferred_report(
            config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms,
                ..WorkspaceRefreshMetrics::default()
            },
        ));
    }
    let started = Instant::now();
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let episodic_started = Instant::now();
    let episodic_reload = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
    let load_episodic_ms = elapsed_ms(episodic_started);
    let inference_started = Instant::now();
    let inference_reload = reload_inference_snapshot_if_needed(config, revisions.inference)?;
    let load_inference_ms = elapsed_ms(inference_started);
    let coordination_started = Instant::now();
    let coordination_reload =
        reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
    let load_coordination_ms = elapsed_ms(coordination_started);
    let episodic_reloaded = episodic_reload.is_some();
    let inference_reloaded = inference_reload.is_some();
    let coordination_reloaded = coordination_reload.is_some();
    let deferred = config.workspace.needs_refresh();
    let refresh_path = if deferred {
        "deferred"
    } else if episodic_reloaded || inference_reloaded || coordination_reloaded {
        "auxiliary"
    } else {
        "none"
    };
    let reload_materialization = episodic_reload
        .unwrap_or_default()
        .add(inference_reload.unwrap_or_default())
        .add(coordination_reload.unwrap_or_default());
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms: 0,
        snapshot_revisions_ms,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
        loaded_bytes: reload_materialization.loaded_bytes,
        replay_volume: reload_materialization.replay_volume,
        full_rebuild_count: 0,
        workspace_reloaded: false,
        ..WorkspaceRefreshMetrics::default()
    };
    if should_publish_runtime_generation(refresh_path, &[]) {
        publish_runtime_generation(config, &revisions, refresh_path, Vec::new(), None);
    }
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation_with_work(
                refresh_path,
                metrics.lock_hold_ms,
                refresh_work(metrics),
            );
    }
    log_refresh_workspace(
        refresh_path,
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
        metrics,
    );
    Ok(WorkspaceRefreshReport {
        refresh_path,
        runtime_sync_used: true,
        deferred,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        metrics,
    })
}

pub(crate) fn hydrate_persisted_workspace_state(config: &WorkspaceRuntimeConfig) -> Result<()> {
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let revisions = config.workspace.snapshot_revisions()?;
    sync_current_runtime_revisions(config, &revisions);
    let _ = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
    let _ = reload_inference_snapshot_if_needed(config, revisions.inference)?;
    let _ = reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
    publish_runtime_generation(config, &revisions, "hydrate", Vec::new(), Some(false));
    Ok(())
}

fn try_sync_workspace_runtime_for_read(
    config: &WorkspaceRuntimeConfig,
) -> Result<Option<WorkspaceRefreshReport>> {
    match config.sync_lock.try_read() {
        Ok(guard) => sync_workspace_runtime_for_read_with_guard(config, guard, 0).map(Some),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Poisoned(_)) => {
            panic!("workspace runtime sync lock poisoned");
        }
    }
}

fn try_sync_workspace_runtime_for_mutation(
    config: &WorkspaceRuntimeConfig,
) -> Result<Option<WorkspaceRefreshReport>> {
    let wait_started = Instant::now();
    loop {
        match config.sync_lock.try_read() {
            Ok(guard) => {
                return sync_workspace_runtime_for_read_with_guard(
                    config,
                    guard,
                    elapsed_ms(wait_started),
                )
                .map(Some);
            }
            Err(TryLockError::WouldBlock) => {
                let elapsed = wait_started.elapsed();
                if elapsed >= MUTATION_RUNTIME_SYNC_WAIT_TIMEOUT {
                    return Ok(None);
                }
                let remaining = MUTATION_RUNTIME_SYNC_WAIT_TIMEOUT.saturating_sub(elapsed);
                thread::sleep(remaining.min(MUTATION_RUNTIME_SYNC_RETRY_INTERVAL));
            }
            Err(TryLockError::Poisoned(_)) => {
                panic!("workspace runtime sync lock poisoned");
            }
        }
    }
}

pub(crate) fn sync_persisted_workspace_state(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let _guard = config
        .sync_lock
        .write()
        .expect("workspace runtime sync lock poisoned");
    let lock_wait_ms = elapsed_ms(lock_wait_started);
    let started = Instant::now();
    let fs_refresh_started = Instant::now();
    let refresh_outcome = config.workspace.refresh_fs_with_status()?;
    let fs_refresh_ms = elapsed_ms(fs_refresh_started);
    let refresh_path = match refresh_outcome.status {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Incremental => "incremental",
        FsRefreshStatus::Rescan => "rescan",
        FsRefreshStatus::Full => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let workspace_reloaded = refresh_path == "full";
    if refresh_path == "deferred" {
        let duration_ms = started.elapsed().as_millis();
        return Ok(dirty_workspace_deferred_report(
            config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms,
                lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
                fs_refresh_ms,
                ..WorkspaceRefreshMetrics::default()
            },
        ));
    }
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);
    let episodic_started = Instant::now();
    let episodic_reload = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
    let load_episodic_ms = elapsed_ms(episodic_started);
    let inference_started = Instant::now();
    let inference_reload = reload_inference_snapshot_if_needed(config, revisions.inference)?;
    let load_inference_ms = elapsed_ms(inference_started);
    let coordination_started = Instant::now();
    let coordination_reload =
        reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
    let load_coordination_ms = elapsed_ms(coordination_started);
    let episodic_reloaded = episodic_reload.is_some();
    let inference_reloaded = inference_reload.is_some();
    let coordination_reloaded = coordination_reload.is_some();
    let deferred = refresh_path == "deferred";
    let refresh_path = if refresh_path == "none"
        && (episodic_reloaded || inference_reloaded || coordination_reloaded)
    {
        "auxiliary"
    } else {
        refresh_path
    };
    let reload_materialization = episodic_reload
        .unwrap_or_default()
        .add(inference_reload.unwrap_or_default())
        .add(coordination_reload.unwrap_or_default());
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms,
        snapshot_revisions_ms,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
        loaded_bytes: reload_materialization.loaded_bytes,
        replay_volume: reload_materialization.replay_volume,
        full_rebuild_count: u64::from(workspace_reloaded),
        workspace_reloaded,
        ..WorkspaceRefreshMetrics::default()
    };
    let mut metrics = metrics;
    apply_refresh_breakdown(&mut metrics, refresh_outcome.breakdown);
    let file_deltas =
        file_deltas_from_observed(config.workspace.as_ref(), &refresh_outcome.observed);
    if should_publish_runtime_generation(refresh_path, &file_deltas) {
        publish_runtime_generation(
            config,
            &revisions,
            refresh_path,
            file_deltas,
            if matches!(refresh_path, "incremental" | "rescan" | "full") {
                Some(true)
            } else {
                None
            },
        );
    }
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation_with_work(
                refresh_path,
                metrics.lock_hold_ms,
                refresh_work(metrics),
            );
    }
    log_refresh_workspace(
        refresh_path,
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
        metrics,
    );
    Ok(WorkspaceRefreshReport {
        refresh_path,
        runtime_sync_used: true,
        deferred,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        metrics,
    })
}

fn run_workspace_prepare_paths_command(
    config: &WorkspaceRuntimeConfig,
    path_requests: &[WorkspaceRuntimePathRequest],
) -> Result<WorkspaceRuntimeCommandOutcome> {
    let lock_wait_started = Instant::now();
    let _guard = config
        .sync_lock
        .write()
        .expect("workspace runtime sync lock poisoned");
    let lock_wait_ms = elapsed_ms(lock_wait_started);
    let dirty_paths = if path_requests.is_empty() {
        Vec::new()
    } else {
        config
            .workspace
            .scoped_refresh_paths_for_requests(path_requests)
    };
    if !path_requests.is_empty() && dirty_paths.is_empty() {
        return Ok(WorkspaceRuntimeCommandOutcome::new(
            WorkspaceRefreshReport::none(),
        ));
    }
    let started = Instant::now();
    let fs_refresh_started = Instant::now();
    let refresh_outcome = config.workspace.refresh_fs_with_paths(dirty_paths)?;
    let fs_refresh_ms = elapsed_ms(fs_refresh_started);
    let refresh_path = match refresh_outcome.status {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Incremental => "incremental",
        FsRefreshStatus::Rescan => "rescan",
        FsRefreshStatus::Full => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let workspace_reloaded = refresh_path == "full";
    if refresh_path == "deferred" {
        let duration_ms = started.elapsed().as_millis();
        return Ok(WorkspaceRuntimeCommandOutcome::prepared(
            dirty_workspace_deferred_report(
                config,
                true,
                WorkspaceRefreshMetrics {
                    lock_wait_ms,
                    lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
                    fs_refresh_ms,
                    ..WorkspaceRefreshMetrics::default()
                },
            ),
            Vec::new(),
        ));
    }
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms,
        snapshot_revisions_ms,
        load_episodic_ms: 0,
        load_inference_ms: 0,
        load_coordination_ms: 0,
        loaded_bytes: 0,
        replay_volume: 0,
        full_rebuild_count: u64::from(workspace_reloaded),
        workspace_reloaded,
        ..WorkspaceRefreshMetrics::default()
    };
    let mut metrics = metrics;
    apply_refresh_breakdown(&mut metrics, refresh_outcome.breakdown);
    let report = WorkspaceRefreshReport {
        refresh_path,
        runtime_sync_used: true,
        deferred: false,
        episodic_reloaded: false,
        inference_reloaded: false,
        coordination_reloaded: false,
        metrics,
    };
    *config
        .prepared_delta
        .lock()
        .expect("workspace runtime prepared delta lock poisoned") =
        Some(PreparedWorkspaceRuntimeDelta {
            report,
            revisions,
            file_deltas: file_deltas_from_observed(
                config.workspace.as_ref(),
                &refresh_outcome.observed,
            ),
        });
    Ok(WorkspaceRuntimeCommandOutcome::prepared(
        report,
        vec![WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::ApplyPreparedDelta,
            WorkspaceRuntimeQueueClass::FollowUpMutation,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
        )],
    ))
}

fn apply_prepared_workspace_delta_command(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRuntimeCommandOutcome> {
    let prepared = config
        .prepared_delta
        .lock()
        .expect("workspace runtime prepared delta lock poisoned")
        .take();
    let Some(prepared) = prepared else {
        return Ok(WorkspaceRuntimeCommandOutcome::new(
            WorkspaceRefreshReport::none(),
        ));
    };
    let checkpoint_pending = prepared.report.refresh_path != "none";
    let should_publish =
        if should_publish_runtime_generation(prepared.report.refresh_path, &prepared.file_deltas) {
            true
        } else {
            config
                .runtime_engine
                .lock()
                .expect("workspace runtime engine lock poisoned")
                .published_generation()
                .domain_states
                .is_empty()
        };
    let batch = if should_publish {
        Some(publish_runtime_generation(
            config,
            &prepared.revisions,
            prepared.report.refresh_path,
            prepared.file_deltas,
            Some(checkpoint_pending),
        ))
    } else {
        None
    };
    let follow_up_commands = follow_up_runtime_commands(
        config,
        &prepared.revisions,
        prepared.report.refresh_path,
        batch
            .map(|value| value.committed_generation)
            .unwrap_or_else(|| {
                config
                    .runtime_engine
                    .lock()
                    .expect("workspace runtime engine lock poisoned")
                    .published_generation()
                    .id
            }),
    );
    let duration_ms = u128::from(prepared.report.metrics.lock_hold_ms);
    log_refresh_workspace(
        prepared.report.refresh_path,
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        prepared.report.episodic_reloaded,
        prepared.report.inference_reloaded,
        prepared.report.coordination_reloaded,
        duration_ms,
        prepared.report.metrics,
    );
    Ok(WorkspaceRuntimeCommandOutcome::with_follow_up_commands(
        prepared.report,
        follow_up_commands,
    ))
}

fn follow_up_runtime_commands(
    config: &WorkspaceRuntimeConfig,
    revisions: &WorkspaceSnapshotRevisions,
    refresh_path: &str,
    target_generation: prism_core::runtime_engine::WorkspaceGenerationId,
) -> Vec<WorkspaceRuntimeCommand> {
    let mut commands = Vec::new();
    if config.loaded_episodic_revision.load(Ordering::Relaxed) != revisions.episodic {
        commands.push(
            WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::MemoryReanchor),
                WorkspaceRuntimeQueueClass::Settle,
                WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::MemoryReanchor),
            )
            .with_target_generation(target_generation),
        );
    }
    if config.loaded_inference_revision.load(Ordering::Relaxed) != revisions.inference {
        commands.push(
            WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::Projections),
                WorkspaceRuntimeQueueClass::Settle,
                WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::Projections),
            )
            .with_target_generation(target_generation),
        );
    }
    if config.loaded_coordination_revision.load(Ordering::Relaxed) != revisions.coordination {
        commands.push(
            WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::Coordination),
                WorkspaceRuntimeQueueClass::Settle,
                WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::Coordination),
            )
            .with_target_generation(target_generation),
        );
    }
    if refresh_path != "none" {
        commands.push(
            WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::MaterializeCheckpoint,
                WorkspaceRuntimeQueueClass::CheckpointMaterialization,
                WorkspaceRuntimeCoalescingKey::WorktreeContext,
            )
            .with_target_generation(target_generation),
        );
    }
    commands
}

fn sync_workspace_settle_domain(
    config: &WorkspaceRuntimeConfig,
    domain: RuntimeDomain,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let _guard = config
        .sync_lock
        .read()
        .expect("workspace runtime sync lock poisoned");
    let lock_wait_ms = elapsed_ms(lock_wait_started);
    if config.workspace.needs_refresh() {
        return Ok(dirty_workspace_deferred_report(
            config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms,
                ..WorkspaceRefreshMetrics::default()
            },
        ));
    }

    let started = Instant::now();
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);

    let (
        episodic_reload,
        inference_reload,
        coordination_reload,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
    ) = match domain {
        RuntimeDomain::MemoryReanchor => {
            let reload_started = Instant::now();
            let reload = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
            (reload, None, None, elapsed_ms(reload_started), 0, 0)
        }
        RuntimeDomain::Projections => {
            let reload_started = Instant::now();
            let reload = reload_inference_snapshot_if_needed(config, revisions.inference)?;
            (None, reload, None, 0, elapsed_ms(reload_started), 0)
        }
        RuntimeDomain::Coordination => {
            let reload_started = Instant::now();
            let reload = reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
            (None, None, reload, 0, 0, elapsed_ms(reload_started))
        }
        _ => (None, None, None, 0, 0, 0),
    };

    let episodic_reloaded = episodic_reload.is_some();
    let inference_reloaded = inference_reload.is_some();
    let coordination_reloaded = coordination_reload.is_some();
    let reload_materialization = episodic_reload
        .unwrap_or_default()
        .add(inference_reload.unwrap_or_default())
        .add(coordination_reload.unwrap_or_default());
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms: 0,
        snapshot_revisions_ms,
        load_episodic_ms,
        load_inference_ms,
        load_coordination_ms,
        loaded_bytes: reload_materialization.loaded_bytes,
        replay_volume: reload_materialization.replay_volume,
        full_rebuild_count: 0,
        workspace_reloaded: false,
        ..WorkspaceRefreshMetrics::default()
    };
    let refresh_path = if episodic_reloaded || inference_reloaded || coordination_reloaded {
        "settle"
    } else {
        "none"
    };
    if refresh_path != "none" {
        publish_runtime_generation(config, &revisions, refresh_path, Vec::new(), None);
    }
    log_refresh_workspace(
        refresh_path,
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
        metrics,
    );
    Ok(WorkspaceRefreshReport {
        refresh_path,
        runtime_sync_used: true,
        deferred: false,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        metrics,
    })
}

fn reload_episodic_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<Option<ReloadMaterialization>> {
    let loaded = config.loaded_episodic_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(None);
    }

    let snapshot = config
        .workspace
        .load_episodic_snapshot_for_runtime()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        });
    let materialization = ReloadMaterialization {
        loaded_bytes: serialized_size(&snapshot)?,
        replay_volume: u64::try_from(snapshot.entries.len()).unwrap_or(u64::MAX),
    };
    config.notes.replace_from_snapshot(snapshot);
    config
        .loaded_episodic_revision
        .store(revision, Ordering::Relaxed);
    Ok(Some(materialization))
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn reload_inference_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<Option<ReloadMaterialization>> {
    let loaded = config.loaded_inference_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(None);
    }

    let snapshot = config
        .workspace
        .load_inference_snapshot()?
        .unwrap_or_default();
    let materialization = ReloadMaterialization {
        loaded_bytes: serialized_size(&snapshot)?,
        replay_volume: u64::try_from(snapshot.records.len()).unwrap_or(u64::MAX),
    };
    config.inferred_edges.replace_from_snapshot(snapshot);
    config
        .loaded_inference_revision
        .store(revision, Ordering::Relaxed);
    Ok(Some(materialization))
}

fn reload_coordination_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<Option<ReloadMaterialization>> {
    let loaded = config.loaded_coordination_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(None);
    }

    let state = config.workspace.hydrate_coordination_runtime()?;
    let materialization = state
        .as_ref()
        .map(coordination_reload_materialization)
        .transpose()?
        .unwrap_or_default();
    config
        .loaded_coordination_revision
        .store(revision, Ordering::Relaxed);
    Ok(Some(materialization))
}

fn serialized_size<T: Serialize>(value: &T) -> Result<u64> {
    Ok(u64::try_from(serde_json::to_vec(value)?.len()).unwrap_or(u64::MAX))
}

fn coordination_reload_materialization(
    state: &prism_core::CoordinationPlanState,
) -> Result<ReloadMaterialization> {
    let overlay_count = state
        .execution_overlays
        .values()
        .map(|overlays| overlays.len())
        .sum::<usize>();
    let plan_graph_node_count = state
        .plan_graphs
        .iter()
        .map(|graph| graph.nodes.len().saturating_add(graph.edges.len()))
        .sum::<usize>();
    let snapshot = &state.snapshot;
    Ok(ReloadMaterialization {
        loaded_bytes: serialized_size(snapshot)?
            .saturating_add(serialized_size(&state.plan_graphs)?)
            .saturating_add(serialized_size(&state.execution_overlays)?),
        replay_volume: u64::try_from(
            snapshot
                .plans
                .len()
                .saturating_add(snapshot.tasks.len())
                .saturating_add(snapshot.claims.len())
                .saturating_add(snapshot.artifacts.len())
                .saturating_add(snapshot.reviews.len())
                .saturating_add(snapshot.events.len())
                .saturating_add(plan_graph_node_count)
                .saturating_add(overlay_count),
        )
        .unwrap_or(u64::MAX),
    })
}

fn materialization_depth_for_coverage(
    coverage: &prism_core::WorkspaceMaterializationCoverage,
) -> RuntimeMaterializationDepth {
    match coverage.depth() {
        "shallow" => RuntimeMaterializationDepth::Shallow,
        "medium" => RuntimeMaterializationDepth::Medium,
        _ => RuntimeMaterializationDepth::Deep,
    }
}

fn domain_state_for_revision(
    loaded_revision: u64,
    current_revision: u64,
    current_depth: RuntimeMaterializationDepth,
) -> RuntimeDomainState {
    RuntimeDomainState::new(
        if loaded_revision == current_revision {
            RuntimeFreshnessState::Current
        } else {
            RuntimeFreshnessState::Pending
        },
        if loaded_revision == current_revision {
            current_depth
        } else {
            RuntimeMaterializationDepth::KnownUnmaterialized
        },
    )
}

fn runtime_domain_states(
    config: &WorkspaceRuntimeConfig,
    revisions: &WorkspaceSnapshotRevisions,
    refresh_path: &str,
    checkpoint_pending: bool,
) -> BTreeMap<RuntimeDomain, RuntimeDomainState> {
    let mut states = BTreeMap::new();
    let workspace_coverage = config.workspace.workspace_materialization_coverage();
    let workspace_freshness = if refresh_path == "deferred" || config.workspace.needs_refresh() {
        RuntimeFreshnessState::Pending
    } else {
        RuntimeFreshnessState::Current
    };
    let workspace_depth = materialization_depth_for_coverage(&workspace_coverage);
    states.insert(
        RuntimeDomain::FileFacts,
        RuntimeDomainState::new(workspace_freshness, workspace_depth),
    );
    states.insert(
        RuntimeDomain::CrossFileEdges,
        RuntimeDomainState::new(
            workspace_freshness,
            if workspace_freshness == RuntimeFreshnessState::Current
                && workspace_coverage.materialized_edges > 0
            {
                RuntimeMaterializationDepth::Deep
            } else {
                RuntimeMaterializationDepth::KnownUnmaterialized
            },
        ),
    );
    states.insert(
        RuntimeDomain::Projections,
        domain_state_for_revision(
            config.loaded_inference_revision.load(Ordering::Relaxed),
            revisions.inference,
            workspace_depth,
        ),
    );
    states.insert(
        RuntimeDomain::MemoryReanchor,
        domain_state_for_revision(
            config.loaded_episodic_revision.load(Ordering::Relaxed),
            revisions.episodic,
            RuntimeMaterializationDepth::Deep,
        ),
    );
    states.insert(
        RuntimeDomain::Checkpoint,
        RuntimeDomainState::new(
            if checkpoint_pending {
                RuntimeFreshnessState::Pending
            } else {
                RuntimeFreshnessState::Current
            },
            if checkpoint_pending {
                RuntimeMaterializationDepth::KnownUnmaterialized
            } else {
                workspace_depth
            },
        ),
    );
    states.insert(
        RuntimeDomain::Coordination,
        domain_state_for_revision(
            config.loaded_coordination_revision.load(Ordering::Relaxed),
            revisions.coordination,
            RuntimeMaterializationDepth::Deep,
        ),
    );
    states
}

fn file_deltas_from_observed(
    workspace: &WorkspaceSession,
    observed: &[ObservedChangeSet],
) -> Vec<WorkspaceFileDelta> {
    let prism = workspace.prism();
    let graph = prism.graph();
    observed
        .iter()
        .map(|change| WorkspaceFileDelta {
            previous_path: change
                .previous_path
                .as_ref()
                .map(|path| PathBuf::from(path.as_str())),
            current_path: change
                .current_path
                .as_ref()
                .map(|path| PathBuf::from(path.as_str())),
            file_count: change.files.len(),
            added_nodes: change.added.len(),
            removed_nodes: change.removed.len(),
            updated_nodes: change.updated.len(),
            current_facts: change.current_path.as_ref().and_then(|path| {
                graph
                    .file_record(std::path::Path::new(path.as_str()))
                    .map(|record| WorkspaceFileSemanticFacts {
                        path: PathBuf::from(path.as_str()),
                        file_id: record.file_id,
                        source_hash: record.hash,
                        parse_depth: record.parse_depth,
                        node_count: record.nodes.len(),
                        edge_count: record.edges.len(),
                        fingerprint_count: record.fingerprints.len(),
                        unresolved_call_count: record.unresolved_calls.len(),
                        unresolved_import_count: record.unresolved_imports.len(),
                        unresolved_impl_count: record.unresolved_impls.len(),
                        unresolved_intent_count: record.unresolved_intents.len(),
                    })
            }),
            edge_added: change.edge_added.len(),
            edge_removed: change.edge_removed.len(),
        })
        .collect()
}

fn should_publish_runtime_generation(
    refresh_path: &str,
    file_deltas: &[WorkspaceFileDelta],
) -> bool {
    refresh_path != "none" || !file_deltas.is_empty()
}

fn changed_paths_from_file_deltas(file_deltas: &[WorkspaceFileDelta]) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for delta in file_deltas {
        if let Some(path) = &delta.previous_path {
            paths.insert(path.clone());
        }
        if let Some(path) = &delta.current_path {
            paths.insert(path.clone());
        }
    }
    paths.into_iter().collect()
}

fn file_facts_from_workspace(
    workspace: &WorkspaceSession,
) -> BTreeMap<PathBuf, WorkspaceFileSemanticFacts> {
    let prism = workspace.prism();
    prism
        .graph()
        .file_records()
        .map(|(path, record)| {
            (
                path.clone(),
                WorkspaceFileSemanticFacts {
                    path: path.clone(),
                    file_id: record.file_id,
                    source_hash: record.hash,
                    parse_depth: record.parse_depth,
                    node_count: record.nodes.len(),
                    edge_count: record.edges.len(),
                    fingerprint_count: record.fingerprints.len(),
                    unresolved_call_count: record.unresolved_calls.len(),
                    unresolved_import_count: record.unresolved_imports.len(),
                    unresolved_impl_count: record.unresolved_impls.len(),
                    unresolved_intent_count: record.unresolved_intents.len(),
                },
            )
        })
        .collect()
}

fn publish_runtime_generation(
    config: &WorkspaceRuntimeConfig,
    revisions: &WorkspaceSnapshotRevisions,
    refresh_path: &str,
    file_deltas: Vec<WorkspaceFileDelta>,
    checkpoint_pending_override: Option<bool>,
) -> prism_core::runtime_engine::WorkspaceRuntimeDeltaBatch {
    let changed_paths = changed_paths_from_file_deltas(&file_deltas);
    let mut engine = config
        .runtime_engine
        .lock()
        .expect("workspace runtime engine lock poisoned");
    if let Some(checkpoint_pending) = checkpoint_pending_override {
        engine.set_checkpoint_pending(checkpoint_pending);
    }
    let domain_states =
        runtime_domain_states(config, revisions, refresh_path, engine.checkpoint_pending());
    if file_deltas.is_empty()
        && (refresh_path == "hydrate" || engine.current_file_facts().is_empty())
    {
        engine.replace_current_file_facts(file_facts_from_workspace(config.workspace.as_ref()));
    }
    engine.record_commit(changed_paths, file_deltas, domain_states)
}

fn sync_workspace_runtime_checkpoint_with_guard(
    config: &WorkspaceRuntimeConfig,
    _guard: RwLockWriteGuard<'_, ()>,
    lock_wait_ms: u64,
) -> Result<WorkspaceRefreshReport> {
    if config.workspace.needs_refresh() {
        return Ok(dirty_workspace_deferred_report(
            config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms,
                ..WorkspaceRefreshMetrics::default()
            },
        ));
    }
    let checkpoint_pending = config
        .runtime_engine
        .lock()
        .expect("workspace runtime engine lock poisoned")
        .checkpoint_pending();
    if !checkpoint_pending {
        return Ok(WorkspaceRefreshReport::none());
    }

    let started = Instant::now();
    config.workspace.flush_materializations()?;
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
    sync_current_runtime_revisions(config, &revisions);
    let duration_ms = started.elapsed().as_millis();
    let metrics = WorkspaceRefreshMetrics {
        lock_wait_ms,
        lock_hold_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
        fs_refresh_ms: 0,
        snapshot_revisions_ms,
        load_episodic_ms: 0,
        load_inference_ms: 0,
        load_coordination_ms: 0,
        loaded_bytes: 0,
        replay_volume: 0,
        full_rebuild_count: 0,
        workspace_reloaded: false,
        ..WorkspaceRefreshMetrics::default()
    };
    publish_runtime_generation(config, &revisions, "checkpoint", Vec::new(), Some(false));
    log_refresh_workspace(
        "checkpoint",
        config.loaded_workspace_revision.load(Ordering::Relaxed),
        config.loaded_episodic_revision.load(Ordering::Relaxed),
        config.loaded_inference_revision.load(Ordering::Relaxed),
        config.loaded_coordination_revision.load(Ordering::Relaxed),
        config.workspace.as_ref(),
        false,
        false,
        false,
        duration_ms,
        metrics,
    );
    Ok(WorkspaceRefreshReport {
        refresh_path: "checkpoint",
        runtime_sync_used: true,
        deferred: false,
        episodic_reloaded: false,
        inference_reloaded: false,
        coordination_reloaded: false,
        metrics,
    })
}

impl QueryHost {
    pub(crate) fn refresh_workspace(&self) -> Result<()> {
        let Some(binding) = self.workspace_runtime_binding() else {
            return Ok(());
        };
        let workspace = binding.workspace();
        let runtime = binding.runtime();
        let diagnostics = binding.diagnostics();
        let config = binding.runtime_config();
        let _report = sync_persisted_workspace_state(&config)?;
        runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
        diagnostics.request_refresh();
        Ok(())
    }

    pub(crate) fn observe_workspace_for_read(&self) -> Result<WorkspaceRefreshReport> {
        let Some(binding) = self.workspace_runtime_binding() else {
            return Ok(WorkspaceRefreshReport::none());
        };
        let workspace = binding.workspace();
        let runtime = binding.runtime();
        let diagnostics = binding.diagnostics();
        let config = binding.runtime_config();
        if runtime_read_fast_path_available(&config) {
            diagnostics.request_refresh();
            return Ok(WorkspaceRefreshReport::none());
        }
        match config.read_sync.try_begin_or_join(
            || runtime_read_fast_path_available(&config),
            READ_RUNTIME_SYNC_JOIN_TIMEOUT,
        ) {
            SharedWorkspaceReadSyncDecision::Current => {
                diagnostics.request_refresh();
                return Ok(WorkspaceRefreshReport::none());
            }
            SharedWorkspaceReadSyncDecision::Busy => {
                runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
                diagnostics.request_refresh();
                workspace.record_runtime_refresh_observation_with_work(
                    "deferred",
                    0,
                    WorkspaceRefreshWork::default(),
                );
                return Ok(WorkspaceRefreshReport {
                    refresh_path: "deferred",
                    runtime_sync_used: false,
                    deferred: true,
                    episodic_reloaded: false,
                    inference_reloaded: false,
                    coordination_reloaded: false,
                    metrics: WorkspaceRefreshMetrics::default(),
                });
            }
            SharedWorkspaceReadSyncDecision::Leader => {}
        }
        let _leader = SharedReadSyncLeader::new(&config.read_sync);
        let Some(report) = try_sync_workspace_runtime_for_read(&config)? else {
            runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
            diagnostics.request_refresh();
            workspace.record_runtime_refresh_observation_with_work(
                "deferred",
                0,
                WorkspaceRefreshWork::default(),
            );
            return Ok(WorkspaceRefreshReport {
                refresh_path: "deferred",
                runtime_sync_used: false,
                deferred: true,
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
                metrics: WorkspaceRefreshMetrics::default(),
            });
        };
        if report.deferred || (!workspace.needs_refresh() && workspace.is_fallback_check_due_now())
        {
            runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
        }
        diagnostics.request_refresh();
        Ok(report)
    }

    pub(crate) fn refresh_workspace_for_mutation(&self) -> Result<WorkspaceRefreshReport> {
        let Some(binding) = self.workspace_runtime_binding() else {
            return Ok(WorkspaceRefreshReport::none());
        };
        let workspace = binding.workspace();
        let runtime = binding.runtime();
        let diagnostics = binding.diagnostics();
        let config = binding.runtime_config();
        let Some(report) = try_sync_workspace_runtime_for_mutation(&config)? else {
            runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
            diagnostics.request_refresh();
            return Ok(dirty_workspace_deferred_report(
                &config,
                true,
                WorkspaceRefreshMetrics::default(),
            ));
        };
        if report.deferred {
            runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
        }
        diagnostics.request_refresh();
        Ok(report)
    }
}

fn is_transient_sqlite_lock(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let text = cause.to_string().to_ascii_lowercase();
        text.contains("database is locked")
            || text.contains("database table is locked")
            || text.contains("database schema is locked")
            || text.contains("locked database")
            || text.contains("sql busy")
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use prism_agent::InferenceStore;
    use prism_core::index_workspace_session;
    use prism_core::runtime_engine::{WorkspaceRuntimeContext, WorkspaceRuntimeEngine};
    use prism_memory::{MemoryEntry, MemoryKind, SessionMemory};

    use super::*;
    use crate::diagnostics_state::DiagnosticsState;
    use crate::mcp_call_log::McpCallLogStore;
    use crate::tests_support::temp_workspace;

    fn test_current_revisions(
        workspace: &Arc<WorkspaceSession>,
    ) -> Arc<crate::workspace_host::SharedWorkspaceRuntimeRevisions> {
        Arc::new(crate::workspace_host::SharedWorkspaceRuntimeRevisions::new(
            workspace
                .workspace_revision()
                .unwrap_or_else(|_| workspace.loaded_workspace_revision()),
            workspace.episodic_revision().unwrap_or(0),
            workspace.inference_revision().unwrap_or(0),
            workspace.coordination_revision().unwrap_or(0),
        ))
    }

    fn test_read_sync() -> Arc<crate::workspace_host::SharedWorkspaceReadSync> {
        Arc::new(crate::workspace_host::SharedWorkspaceReadSync::default())
    }

    #[test]
    fn dirty_workspace_deferred_report_skips_reload_metrics() {
        let root = temp_workspace();
        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        let report = dirty_workspace_deferred_report(
            &config,
            true,
            WorkspaceRefreshMetrics {
                lock_wait_ms: 7,
                ..WorkspaceRefreshMetrics::default()
            },
        );

        assert_eq!(report.refresh_path, "deferred");
        assert!(report.runtime_sync_used);
        assert!(report.deferred);
        assert_eq!(report.metrics.lock_wait_ms, 7);
        assert_eq!(report.metrics.lock_hold_ms, 0);
        assert_eq!(report.metrics.fs_refresh_ms, 0);
        assert_eq!(report.metrics.snapshot_revisions_ms, 0);
        assert_eq!(report.metrics.load_episodic_ms, 0);
        assert_eq!(report.metrics.load_inference_ms, 0);
        assert_eq!(report.metrics.load_coordination_ms, 0);
        assert_eq!(
            workspace
                .last_refresh()
                .as_ref()
                .map(|refresh| refresh.path.as_str()),
            Some("deferred")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sync_persisted_workspace_state_reports_rescan_for_fallback_scan() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/created.md"),
            "# Watcher Created Doc\n\nThis document was added after startup.\n",
        )
        .unwrap();

        let report = sync_persisted_workspace_state(&config).unwrap();

        assert_eq!(report.refresh_path, "rescan");
        assert!(!report.deferred);
        assert_eq!(report.metrics.full_rebuild_count, 0);
        assert!(!report.metrics.workspace_reloaded);
        assert_eq!(
            workspace
                .last_refresh()
                .as_ref()
                .map(|refresh| refresh.path.as_str()),
            Some("rescan")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scoped_prepare_paths_skip_auxiliary_snapshot_reload() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let initial_episodic_revision = workspace.episodic_revision().unwrap_or(0);
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(initial_episodic_revision)),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        workspace
            .persist_episodic(&EpisodicMemorySnapshot {
                entries: vec![MemoryEntry::new(
                    MemoryKind::Structural,
                    "prepare path should not reload this episodic snapshot",
                )],
            })
            .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() { let _x = 1; }\n").unwrap();
        let scoped_path = root.join("src/lib.rs");
        thread::sleep(Duration::from_millis(1_100));

        let outcome = run_workspace_prepare_paths_command(
            &config,
            &[WorkspaceRuntimePathRequest {
                path: scoped_path,
                revision: 0,
            }],
        )
        .unwrap();
        let report = outcome.report;

        assert!(matches!(
            report.refresh_path,
            "none" | "incremental" | "rescan" | "full"
        ));
        assert!(!report.episodic_reloaded);
        assert_eq!(report.metrics.load_episodic_ms, 0);
        assert_eq!(
            config.loaded_episodic_revision.load(Ordering::Relaxed),
            initial_episodic_revision
        );
        assert!(
            workspace.episodic_revision().unwrap() > initial_episodic_revision,
            "persisted episodic revision should advance independently of scoped prepare"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scoped_prepare_paths_return_follow_up_settle_and_checkpoint_work() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        workspace
            .persist_episodic(&EpisodicMemorySnapshot {
                entries: vec![MemoryEntry::new(
                    MemoryKind::Structural,
                    "queued settle should reload this episodic snapshot",
                )],
            })
            .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() { let _x = 1; }\n").unwrap();
        thread::sleep(Duration::from_millis(1_100));

        let scoped_path = root.join("src/lib.rs");
        let outcome = run_workspace_prepare_paths_command(
            &config,
            &[WorkspaceRuntimePathRequest {
                path: scoped_path,
                revision: 0,
            }],
        )
        .unwrap();
        let report = outcome.report;

        assert!(matches!(
            report.refresh_path,
            "none" | "incremental" | "rescan" | "full"
        ));
        assert!(
            outcome
                .follow_up_commands
                .iter()
                .any(|command| command.kind == WorkspaceRuntimeCommandKind::ApplyPreparedDelta),
            "scoped prepare should enqueue an explicit apply step"
        );
        let apply = run_workspace_runtime_command(
            &config,
            &WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::ApplyPreparedDelta,
                WorkspaceRuntimeQueueClass::FollowUpMutation,
                WorkspaceRuntimeCoalescingKey::WorktreeContext,
            ),
        )
        .unwrap();
        assert!(
            apply
                .follow_up_commands
                .iter()
                .any(|command| command.queue_class == WorkspaceRuntimeQueueClass::Settle),
            "apply should enqueue settle work for stale auxiliary domains"
        );
        let has_checkpoint_materialization = apply.follow_up_commands.iter().any(|command| {
            command.queue_class == WorkspaceRuntimeQueueClass::CheckpointMaterialization
        });
        assert_eq!(
            has_checkpoint_materialization,
            report.refresh_path != "none",
            "checkpoint materialization should only follow a scoped prepare that actually refreshed workspace state"
        );
        let published_generation = config
            .runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .published_generation_snapshot();
        let file_facts = published_generation
            .domain_states
            .get(&RuntimeDomain::FileFacts)
            .expect("file facts domain state should be published");
        assert_eq!(file_facts.freshness, RuntimeFreshnessState::Current);
        let memory_reanchor = published_generation
            .domain_states
            .get(&RuntimeDomain::MemoryReanchor)
            .expect("memory reanchor domain state should be published");
        assert_eq!(memory_reanchor.freshness, RuntimeFreshnessState::Pending);
        let checkpoint = published_generation
            .domain_states
            .get(&RuntimeDomain::Checkpoint)
            .expect("checkpoint domain state should be published");
        assert_eq!(
            checkpoint.freshness,
            if report.refresh_path == "none" {
                RuntimeFreshnessState::Current
            } else {
                RuntimeFreshnessState::Pending
            }
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settle_domain_reload_advances_loaded_episodic_revision() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let initial_episodic_revision = workspace.episodic_revision().unwrap_or(0);
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(initial_episodic_revision)),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        workspace
            .persist_episodic(&EpisodicMemorySnapshot {
                entries: vec![MemoryEntry::new(
                    MemoryKind::Structural,
                    "domain settle should advance episodic revision",
                )],
            })
            .unwrap();
        let persisted_revision = workspace.episodic_revision().unwrap();
        config
            .loaded_episodic_revision
            .store(persisted_revision.saturating_add(1), Ordering::Relaxed);

        let report = sync_workspace_settle_domain(&config, RuntimeDomain::MemoryReanchor).unwrap();

        assert_eq!(report.refresh_path, "settle");
        assert!(report.episodic_reloaded);
        assert_eq!(
            config.loaded_episodic_revision.load(Ordering::Relaxed),
            persisted_revision
        );
        let published_generation = config
            .runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .published_generation_snapshot();
        let memory_reanchor = published_generation
            .domain_states
            .get(&RuntimeDomain::MemoryReanchor)
            .expect("memory reanchor domain state should be published");
        assert_eq!(memory_reanchor.freshness, RuntimeFreshnessState::Current);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settle_domains_do_not_block_on_other_shared_runtime_readers() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let initial_episodic_revision = workspace.episodic_revision().unwrap_or(0);
        let sync_lock = Arc::new(RwLock::new(()));
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::clone(&sync_lock),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(initial_episodic_revision)),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
                WorkspaceRuntimeContext::from_root(&root),
            ))),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        workspace
            .persist_episodic(&EpisodicMemorySnapshot {
                entries: vec![MemoryEntry::new(
                    MemoryKind::Structural,
                    "shared settle readers should overlap",
                )],
            })
            .unwrap();
        let persisted_revision = workspace.episodic_revision().unwrap();
        config
            .loaded_episodic_revision
            .store(persisted_revision.saturating_add(1), Ordering::Relaxed);

        let (ready_tx, ready_rx) = mpsc::channel();
        let lock_clone = Arc::clone(&sync_lock);
        let reader = thread::spawn(move || {
            let _guard = lock_clone
                .read()
                .expect("shared runtime read lock should be available");
            ready_tx.send(()).expect("reader should report readiness");
            thread::sleep(Duration::from_millis(150));
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reader should acquire shared lock");

        let started = Instant::now();
        let report = sync_workspace_settle_domain(&config, RuntimeDomain::MemoryReanchor).unwrap();
        let elapsed = started.elapsed();
        reader.join().expect("reader thread should finish");

        assert_eq!(report.refresh_path, "settle");
        assert!(
            elapsed < Duration::from_millis(100),
            "settle domain should share the runtime read gate with other settle readers"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoint_materialization_command_clears_pending_checkpoint_domain() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let runtime_engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
            WorkspaceRuntimeContext::from_root(&root),
        )));
        runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .set_checkpoint_pending(true);
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::clone(&runtime_engine),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        let report = sync_workspace_runtime_materialization(&config).unwrap();
        assert_eq!(report.refresh_path, "checkpoint");

        let engine = runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned");
        assert!(!engine.checkpoint_pending());
        let checkpoint = engine
            .published_generation()
            .domain_states
            .get(&RuntimeDomain::Checkpoint)
            .expect("checkpoint domain state should be published");
        assert_eq!(checkpoint.freshness, RuntimeFreshnessState::Current);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoint_materialization_waits_for_shared_runtime_readers() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let sync_lock = Arc::new(RwLock::new(()));
        let runtime_engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
            WorkspaceRuntimeContext::from_root(&root),
        )));
        runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .set_checkpoint_pending(true);
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::clone(&sync_lock),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::clone(&runtime_engine),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        let (ready_tx, ready_rx) = mpsc::channel();
        let lock_clone = Arc::clone(&sync_lock);
        let reader = thread::spawn(move || {
            let _guard = lock_clone
                .read()
                .expect("shared runtime read lock should be available");
            ready_tx.send(()).expect("reader should report readiness");
            thread::sleep(Duration::from_millis(150));
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reader should acquire shared lock");

        let started = Instant::now();
        let report = sync_workspace_runtime_materialization(&config).unwrap();
        let elapsed = started.elapsed();
        reader.join().expect("reader thread should finish");

        assert_eq!(report.refresh_path, "checkpoint");
        assert!(
            elapsed >= Duration::from_millis(100),
            "checkpoint materialization should keep exclusive writer admission"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_settle_commands_are_skipped_once_newer_generation_exists() {
        let root = temp_workspace();
        let context = WorkspaceRuntimeContext::from_root(&root);
        let mut engine = WorkspaceRuntimeEngine::new(context.clone());
        let older_generation = engine
            .record_commit(Vec::new(), Vec::new(), BTreeMap::new())
            .committed_generation;
        let newer_generation = engine
            .record_commit(Vec::new(), Vec::new(), BTreeMap::new())
            .committed_generation;

        let stale = WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::Projections),
            WorkspaceRuntimeQueueClass::Settle,
            WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::Projections),
        )
        .with_target_generation(older_generation);
        let current = WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::SettleDomain(RuntimeDomain::Projections),
            WorkspaceRuntimeQueueClass::Settle,
            WorkspaceRuntimeCoalescingKey::Domain(RuntimeDomain::Projections),
        )
        .with_target_generation(newer_generation);
        let checkpoint = WorkspaceRuntimeCommand::new(
            WorkspaceRuntimeCommandKind::MaterializeCheckpoint,
            WorkspaceRuntimeQueueClass::CheckpointMaterialization,
            WorkspaceRuntimeCoalescingKey::WorktreeContext,
        )
        .with_target_generation(older_generation);

        assert!(command_is_stale_against_generation(
            &stale,
            engine.published_generation()
        ));
        assert!(!command_is_stale_against_generation(
            &current,
            engine.published_generation()
        ));
        assert!(!command_is_stale_against_generation(
            &checkpoint,
            engine.published_generation()
        ));
    }

    #[test]
    fn scoped_prepare_paths_publish_file_local_semantic_facts() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let runtime_engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
            WorkspaceRuntimeContext::from_root(&root),
        )));
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::clone(&runtime_engine),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { let _value = 1; }\n",
        )
        .unwrap();
        thread::sleep(Duration::from_millis(1_100));

        let scoped_path = root.join("src/lib.rs");
        let outcome = run_workspace_prepare_paths_command(
            &config,
            &[WorkspaceRuntimePathRequest {
                path: scoped_path,
                revision: 0,
            }],
        )
        .unwrap();
        assert!(matches!(
            outcome.report.refresh_path,
            "none" | "incremental" | "rescan" | "full"
        ));
        assert!(
            runtime_engine
                .lock()
                .expect("workspace runtime engine lock poisoned")
                .recent_deltas()
                .is_empty(),
            "prepare phase should not publish the generation before apply"
        );
        let apply = run_workspace_runtime_command(
            &config,
            &WorkspaceRuntimeCommand::new(
                WorkspaceRuntimeCommandKind::ApplyPreparedDelta,
                WorkspaceRuntimeQueueClass::FollowUpMutation,
                WorkspaceRuntimeCoalescingKey::WorktreeContext,
            ),
        )
        .unwrap();
        assert!(!apply.report.deferred);

        let recent_deltas = runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .recent_deltas();
        if outcome.report.refresh_path == "none" {
            assert!(
                recent_deltas
                    .last()
                    .and_then(|delta| delta.file_deltas.first())
                    .and_then(|delta| delta.current_facts.as_ref())
                    .is_none(),
                "no-op scoped prepares should not synthesize file-fact deltas"
            );
        } else {
            let facts = recent_deltas
                .last()
                .and_then(|delta| delta.file_deltas.first())
                .and_then(|delta| delta.current_facts.as_ref())
                .expect("committed delta should carry current file facts");
            assert_eq!(
                facts.path.file_name().and_then(|name| name.to_str()),
                Some("lib.rs")
            );
            assert!(facts.source_hash > 0);
            assert!(facts.node_count > 0);
            assert!(facts.fingerprint_count > 0);
            let engine = runtime_engine
                .lock()
                .expect("workspace runtime engine lock poisoned");
            let live_facts = engine
                .file_facts(facts.path.as_path())
                .expect("engine should retain current file facts");
            assert_eq!(live_facts.source_hash, facts.source_hash);
            assert_eq!(live_facts.node_count, facts.node_count);
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hydrate_persisted_workspace_state_seeds_engine_file_facts() {
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let workspace = Arc::new(index_workspace_session(&root).unwrap());
        let runtime_engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
            WorkspaceRuntimeContext::from_root(&root),
        )));
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(
                workspace.loaded_workspace_revision(),
            )),
            loaded_episodic_revision: Arc::new(AtomicU64::new(
                workspace.episodic_revision().unwrap_or(0),
            )),
            loaded_inference_revision: Arc::new(AtomicU64::new(
                workspace.inference_revision().unwrap_or(0),
            )),
            loaded_coordination_revision: Arc::new(AtomicU64::new(
                workspace.coordination_revision().unwrap_or(0),
            )),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine: Arc::clone(&runtime_engine),
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        hydrate_persisted_workspace_state(&config).unwrap();

        let engine = runtime_engine
            .lock()
            .expect("workspace runtime engine lock poisoned");
        let facts = engine
            .current_file_facts()
            .values()
            .find(|facts| facts.path.file_name().and_then(|name| name.to_str()) == Some("lib.rs"))
            .expect("hydrate should seed current file facts");
        assert!(facts.source_hash > 0);
        assert!(facts.node_count > 0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hydrate_persisted_workspace_state_replays_shared_runtime_memory_without_checkpoint_flush() {
        let shared_runtime_root = temp_workspace();
        let shared_runtime_sqlite = shared_runtime_root.join("shared-runtime.db");
        let root = temp_workspace();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

        let options = prism_core::WorkspaceSessionOptions {
            runtime_mode: prism_core::PrismRuntimeMode::Full,
            shared_runtime: prism_core::SharedRuntimeBackend::Sqlite {
                path: shared_runtime_sqlite,
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        };
        let session = Arc::new(
            prism_core::index_workspace_session_with_options(&root, options.clone()).unwrap(),
        );
        let alpha = session
            .prism()
            .symbol("alpha")
            .into_iter()
            .find(|symbol| symbol.id().path == "demo::alpha")
            .expect("alpha should be indexed")
            .id()
            .clone();
        let mut entry = prism_memory::MemoryEntry::new(
            prism_memory::MemoryKind::Structural,
            "hydrate replay memory",
        );
        entry.id = prism_memory::MemoryId("memory:hydrate-replay".to_string());
        entry.anchors = vec![prism_ir::AnchorRef::Node(alpha)];
        entry.scope = prism_memory::MemoryScope::Session;
        entry.source = prism_memory::MemorySource::User;
        entry.trust = 0.9;
        session
            .append_memory_event(prism_memory::MemoryEvent::from_entry(
                prism_memory::MemoryEventKind::Stored,
                entry,
                Some("task:hydrate-replay".to_string()),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        drop(session);

        let workspace =
            Arc::new(prism_core::index_workspace_session_with_options(&root, options).unwrap());
        let runtime_engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(
            WorkspaceRuntimeContext::from_root(&root),
        )));
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            diagnostics_state: Arc::new(DiagnosticsState::default()),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(Some(&root))),
            runtime_diagnostics_auto_refresh: false,
            sync_lock: Arc::new(RwLock::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(0)),
            loaded_episodic_revision: Arc::new(AtomicU64::new(0)),
            loaded_inference_revision: Arc::new(AtomicU64::new(0)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(0)),
            current_revisions: test_current_revisions(&workspace),
            read_sync: test_read_sync(),
            runtime_engine,
            prepared_delta: Arc::new(Mutex::new(None)),
        };

        hydrate_persisted_workspace_state(&config).unwrap();

        let notes = config.notes.snapshot();
        assert!(notes
            .entries
            .iter()
            .any(|candidate| candidate.content == "hydrate replay memory"));
        assert!(config.loaded_episodic_revision.load(Ordering::Relaxed) > 0);
        assert_eq!(
            config.loaded_coordination_revision.load(Ordering::Relaxed),
            workspace.coordination_revision().unwrap()
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(shared_runtime_root);
    }

    #[test]
    fn shared_read_sync_waiter_observes_current_after_leader_finishes() {
        let gate = Arc::new(crate::workspace_host::SharedWorkspaceReadSync::default());
        let current = Arc::new(AtomicBool::new(false));

        assert!(matches!(
            gate.try_begin_or_join(|| current.load(Ordering::Relaxed), Duration::from_millis(5)),
            crate::workspace_host::SharedWorkspaceReadSyncDecision::Leader
        ));

        let waiter_gate = Arc::clone(&gate);
        let waiter_current = Arc::clone(&current);
        let waiter = thread::spawn(move || {
            waiter_gate.try_begin_or_join(
                || waiter_current.load(Ordering::Relaxed),
                Duration::from_millis(200),
            )
        });

        thread::sleep(Duration::from_millis(20));
        current.store(true, Ordering::Relaxed);
        gate.finish();

        assert!(matches!(
            waiter.join().expect("waiter should finish"),
            crate::workspace_host::SharedWorkspaceReadSyncDecision::Current
        ));
    }

    #[test]
    fn shared_read_sync_waiter_returns_busy_after_timeout() {
        let gate = Arc::new(crate::workspace_host::SharedWorkspaceReadSync::default());

        assert!(matches!(
            gate.try_begin_or_join(|| false, Duration::from_millis(5)),
            crate::workspace_host::SharedWorkspaceReadSyncDecision::Leader
        ));

        let started = Instant::now();
        let outcome = gate.try_begin_or_join(|| false, Duration::from_millis(40));

        assert!(matches!(
            outcome,
            crate::workspace_host::SharedWorkspaceReadSyncDecision::Busy
        ));
        assert!(
            started.elapsed() >= Duration::from_millis(30),
            "busy follower should wait briefly before giving up"
        );

        gate.finish();
    }
}
