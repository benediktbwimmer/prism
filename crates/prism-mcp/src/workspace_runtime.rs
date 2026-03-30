use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use prism_agent::InferenceStore;
use prism_core::{FsRefreshStatus, WorkspaceSession};
use prism_memory::{EpisodicMemorySnapshot, SessionMemory};
use serde::Serialize;
use serde_json::json;
use tracing::{debug, error};

use crate::{
    log_refresh_workspace, DashboardState, QueryHost, WorkspaceRefreshMetrics,
    WorkspaceRefreshReport,
};

const BACKGROUND_REFRESH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeConfig {
    pub(crate) workspace: Arc<WorkspaceSession>,
    pub(crate) notes: Arc<SessionMemory>,
    pub(crate) inferred_edges: Arc<InferenceStore>,
    pub(crate) dashboard_state: Arc<DashboardState>,
    pub(crate) sync_lock: Arc<Mutex<()>>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) loaded_episodic_revision: Arc<AtomicU64>,
    pub(crate) loaded_inference_revision: Arc<AtomicU64>,
    pub(crate) loaded_coordination_revision: Arc<AtomicU64>,
}

pub(crate) struct WorkspaceRuntime {
    wake: SyncSender<()>,
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

impl WorkspaceRuntime {
    pub(crate) fn spawn(config: WorkspaceRuntimeConfig) -> Self {
        let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let handle = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            match wake_rx.recv_timeout(BACKGROUND_REFRESH_INTERVAL) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if stop_rx.try_recv().is_ok() {
                break;
            }
            if let Err(error) = sync_workspace_runtime(&config) {
                if is_transient_sqlite_lock(&error) {
                    config
                        .workspace
                        .record_runtime_refresh_observation("deferred", 0);
                    debug!(
                        root = %config.workspace.root().display(),
                        error = %error,
                        "prism-mcp background workspace refresh deferred by sqlite lock contention"
                    );
                    continue;
                }
                error!(
                    root = %config.workspace.root().display(),
                    error = %error,
                    error_chain = %crate::logging::format_error_chain(&error),
                    "prism-mcp background workspace refresh failed"
                );
            }
        });
        Self {
            wake: wake_tx,
            stop: stop_tx,
            handle: Mutex::new(Some(handle)),
        }
    }

    pub(crate) fn request_refresh(&self) {
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                debug!("workspace runtime wake channel disconnected");
            }
        }
    }
}

impl Drop for WorkspaceRuntime {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self
            .handle
            .lock()
            .expect("workspace runtime handle lock poisoned")
            .take()
        {
            let _ = thread::Builder::new()
                .name("prism-workspace-runtime-join".to_string())
                .spawn(move || {
                    let _ = handle.join();
                });
        }
    }
}

pub(crate) fn sync_workspace_runtime(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let guard = config
        .sync_lock
        .lock()
        .expect("workspace runtime sync lock poisoned");
    sync_workspace_runtime_with_guard(config, guard, elapsed_ms(lock_wait_started))
}

fn sync_workspace_runtime_with_guard(
    config: &WorkspaceRuntimeConfig,
    _guard: MutexGuard<'_, ()>,
    lock_wait_ms: u64,
) -> Result<WorkspaceRefreshReport> {
    let started = Instant::now();
    let fs_refresh_started = Instant::now();
    let refresh_path = match config.workspace.refresh_fs_nonblocking()? {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Incremental => "incremental",
        FsRefreshStatus::Full => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let fs_refresh_ms = elapsed_ms(fs_refresh_started);
    let deferred = refresh_path == "deferred";
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
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
    };
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation(refresh_path, metrics.lock_hold_ms);
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
    config.dashboard_state.publish_value(
        "runtime.refreshed",
        json!({
            "refreshPath": refresh_path,
            "durationMs": duration_ms,
            "coordinationReloaded": coordination_reloaded,
            "deferred": deferred,
            "episodicReloaded": episodic_reloaded,
            "fsAppliedRevision": config.workspace.applied_fs_revision(),
            "fsDirty": config.workspace.observed_fs_revision() != config.workspace.applied_fs_revision(),
            "fsObservedRevision": config.workspace.observed_fs_revision(),
            "lockWaitMs": metrics.lock_wait_ms,
            "lockHoldMs": metrics.lock_hold_ms,
            "fsRefreshMs": metrics.fs_refresh_ms,
            "snapshotRevisionsMs": metrics.snapshot_revisions_ms,
            "loadEpisodicMs": metrics.load_episodic_ms,
            "loadInferenceMs": metrics.load_inference_ms,
            "loadCoordinationMs": metrics.load_coordination_ms,
            "loadedBytes": metrics.loaded_bytes,
            "replayVolume": metrics.replay_volume,
            "fullRebuildCount": metrics.full_rebuild_count,
            "inferenceReloaded": inference_reloaded,
            "loadedCoordinationRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
            "loadedEpisodicRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
            "loadedInferenceRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
            "loadedWorkspaceRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
            "materialization": {
                "workspace": {
                    "currentRevision": revisions.workspace,
                    "loadedRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_workspace_revision.load(Ordering::Relaxed), revisions.workspace),
                },
                "episodic": {
                    "currentRevision": revisions.episodic,
                    "loadedRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_episodic_revision.load(Ordering::Relaxed), revisions.episodic),
                },
                "inference": {
                    "currentRevision": revisions.inference,
                    "loadedRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_inference_revision.load(Ordering::Relaxed), revisions.inference),
                },
                "coordination": {
                    "currentRevision": revisions.coordination,
                    "loadedRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_coordination_revision.load(Ordering::Relaxed), revisions.coordination),
                }
            },
        }),
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
    _guard: MutexGuard<'_, ()>,
    lock_wait_ms: u64,
) -> Result<WorkspaceRefreshReport> {
    let started = Instant::now();
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
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
    };
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation(refresh_path, metrics.lock_hold_ms);
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
    config.dashboard_state.publish_value(
        "runtime.refreshed",
        json!({
            "refreshPath": refresh_path,
            "durationMs": duration_ms,
            "coordinationReloaded": coordination_reloaded,
            "deferred": deferred,
            "episodicReloaded": episodic_reloaded,
            "fsAppliedRevision": config.workspace.applied_fs_revision(),
            "fsDirty": config.workspace.observed_fs_revision() != config.workspace.applied_fs_revision(),
            "fsObservedRevision": config.workspace.observed_fs_revision(),
            "lockWaitMs": metrics.lock_wait_ms,
            "lockHoldMs": metrics.lock_hold_ms,
            "fsRefreshMs": metrics.fs_refresh_ms,
            "snapshotRevisionsMs": metrics.snapshot_revisions_ms,
            "loadEpisodicMs": metrics.load_episodic_ms,
            "loadInferenceMs": metrics.load_inference_ms,
            "loadCoordinationMs": metrics.load_coordination_ms,
            "loadedBytes": metrics.loaded_bytes,
            "replayVolume": metrics.replay_volume,
            "fullRebuildCount": metrics.full_rebuild_count,
            "inferenceReloaded": inference_reloaded,
            "loadedCoordinationRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
            "loadedEpisodicRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
            "loadedInferenceRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
            "loadedWorkspaceRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
            "materialization": {
                "workspace": {
                    "currentRevision": revisions.workspace,
                    "loadedRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_workspace_revision.load(Ordering::Relaxed), revisions.workspace),
                },
                "episodic": {
                    "currentRevision": revisions.episodic,
                    "loadedRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_episodic_revision.load(Ordering::Relaxed), revisions.episodic),
                },
                "inference": {
                    "currentRevision": revisions.inference,
                    "loadedRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_inference_revision.load(Ordering::Relaxed), revisions.inference),
                },
                "coordination": {
                    "currentRevision": revisions.coordination,
                    "loadedRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_coordination_revision.load(Ordering::Relaxed), revisions.coordination),
                }
            },
            "workspaceReloaded": false,
        }),
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
    let _ = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
    let _ = reload_inference_snapshot_if_needed(config, revisions.inference)?;
    let _ = reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
    Ok(())
}

fn try_sync_workspace_runtime_for_read(
    config: &WorkspaceRuntimeConfig,
) -> Result<Option<WorkspaceRefreshReport>> {
    match config.sync_lock.try_lock() {
        Ok(guard) => sync_workspace_runtime_for_read_with_guard(config, guard, 0).map(Some),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Poisoned(_)) => {
            panic!("workspace runtime sync lock poisoned");
        }
    }
}

pub(crate) fn sync_persisted_workspace_state(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let lock_wait_started = Instant::now();
    let _guard = config
        .sync_lock
        .lock()
        .expect("workspace runtime sync lock poisoned");
    let lock_wait_ms = elapsed_ms(lock_wait_started);
    let started = Instant::now();
    let fs_refresh_started = Instant::now();
    let refresh_outcome = config.workspace.refresh_fs_with_status()?;
    let fs_refresh_ms = elapsed_ms(fs_refresh_started);
    let refresh_path = match refresh_outcome.status {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Incremental => "incremental",
        FsRefreshStatus::Full => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let workspace_reloaded = refresh_path == "full";
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let revisions_started = Instant::now();
    let revisions = config.workspace.snapshot_revisions_for_runtime()?;
    let snapshot_revisions_ms = elapsed_ms(revisions_started);
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
    };
    if deferred {
        config
            .workspace
            .record_runtime_refresh_observation(refresh_path, metrics.lock_hold_ms);
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
    config.dashboard_state.publish_value(
        "runtime.refreshed",
        json!({
            "refreshPath": refresh_path,
            "durationMs": duration_ms,
            "coordinationReloaded": coordination_reloaded,
            "deferred": deferred,
            "episodicReloaded": episodic_reloaded,
            "fsAppliedRevision": config.workspace.applied_fs_revision(),
            "fsDirty": config.workspace.observed_fs_revision() != config.workspace.applied_fs_revision(),
            "fsObservedRevision": config.workspace.observed_fs_revision(),
            "lockWaitMs": metrics.lock_wait_ms,
            "lockHoldMs": metrics.lock_hold_ms,
            "fsRefreshMs": metrics.fs_refresh_ms,
            "snapshotRevisionsMs": metrics.snapshot_revisions_ms,
            "loadEpisodicMs": metrics.load_episodic_ms,
            "loadInferenceMs": metrics.load_inference_ms,
            "loadCoordinationMs": metrics.load_coordination_ms,
            "loadedBytes": metrics.loaded_bytes,
            "replayVolume": metrics.replay_volume,
            "fullRebuildCount": metrics.full_rebuild_count,
            "inferenceReloaded": inference_reloaded,
            "loadedCoordinationRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
            "loadedEpisodicRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
            "loadedInferenceRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
            "loadedWorkspaceRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
            "materialization": {
                "workspace": {
                    "currentRevision": revisions.workspace,
                    "loadedRevision": config.loaded_workspace_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_workspace_revision.load(Ordering::Relaxed), revisions.workspace),
                },
                "episodic": {
                    "currentRevision": revisions.episodic,
                    "loadedRevision": config.loaded_episodic_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_episodic_revision.load(Ordering::Relaxed), revisions.episodic),
                },
                "inference": {
                    "currentRevision": revisions.inference,
                    "loadedRevision": config.loaded_inference_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_inference_revision.load(Ordering::Relaxed), revisions.inference),
                },
                "coordination": {
                    "currentRevision": revisions.coordination,
                    "loadedRevision": config.loaded_coordination_revision.load(Ordering::Relaxed),
                    "status": revision_status(config.loaded_coordination_revision.load(Ordering::Relaxed), revisions.coordination),
                }
            },
            "workspaceReloaded": workspace_reloaded,
        }),
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

fn revision_status(loaded_revision: u64, current_revision: u64) -> &'static str {
    if loaded_revision == current_revision {
        "current"
    } else {
        "stale"
    }
}

impl QueryHost {
    pub(crate) fn refresh_workspace(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        let Some(runtime) = &self.workspace_runtime else {
            let _ = workspace.refresh_fs()?;
            self.sync_workspace_revision(workspace)?;
            self.sync_episodic_revision(workspace)?;
            self.sync_inference_revision(workspace)?;
            self.sync_coordination_revision(workspace)?;
            return Ok(());
        };

        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(workspace),
            notes: Arc::clone(&self.notes),
            inferred_edges: Arc::clone(&self.inferred_edges),
            dashboard_state: Arc::clone(&self.dashboard_state),
            sync_lock: Arc::clone(&self.workspace_runtime_sync_lock),
            loaded_workspace_revision: Arc::clone(&self.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&self.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&self.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&self.loaded_coordination_revision),
        };
        let report = sync_persisted_workspace_state(&config)?;
        if report.coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        runtime.request_refresh();
        Ok(())
    }

    pub(crate) fn observe_workspace_for_read(&self) -> Result<WorkspaceRefreshReport> {
        let Some(workspace) = &self.workspace else {
            return Ok(WorkspaceRefreshReport::none());
        };
        let Some(runtime) = &self.workspace_runtime else {
            let refresh_path = if workspace.needs_refresh() {
                "deferred"
            } else {
                "none"
            };
            if refresh_path == "deferred" {
                workspace.record_runtime_refresh_observation(refresh_path, 0);
            }
            return Ok(WorkspaceRefreshReport {
                refresh_path,
                runtime_sync_used: false,
                deferred: refresh_path == "deferred",
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
                metrics: WorkspaceRefreshMetrics::default(),
            });
        };

        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(workspace),
            notes: Arc::clone(&self.notes),
            inferred_edges: Arc::clone(&self.inferred_edges),
            dashboard_state: Arc::clone(&self.dashboard_state),
            sync_lock: Arc::clone(&self.workspace_runtime_sync_lock),
            loaded_workspace_revision: Arc::clone(&self.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&self.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&self.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&self.loaded_coordination_revision),
        };
        let Some(report) = try_sync_workspace_runtime_for_read(&config)? else {
            runtime.request_refresh();
            workspace.record_runtime_refresh_observation("deferred", 0);
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
        if report.coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        if report.deferred {
            runtime.request_refresh();
        }
        Ok(report)
    }

    pub(crate) fn refresh_workspace_for_mutation(&self) -> Result<WorkspaceRefreshReport> {
        let Some(workspace) = &self.workspace else {
            return Ok(WorkspaceRefreshReport::none());
        };
        let Some(runtime) = &self.workspace_runtime else {
            let refresh_path = if workspace.needs_refresh() {
                "deferred"
            } else {
                "none"
            };
            if refresh_path == "deferred" {
                workspace.record_runtime_refresh_observation(refresh_path, 0);
            }
            return Ok(WorkspaceRefreshReport {
                refresh_path,
                runtime_sync_used: false,
                deferred: refresh_path == "deferred",
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
                metrics: WorkspaceRefreshMetrics::default(),
            });
        };

        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(workspace),
            notes: Arc::clone(&self.notes),
            inferred_edges: Arc::clone(&self.inferred_edges),
            dashboard_state: Arc::clone(&self.dashboard_state),
            sync_lock: Arc::clone(&self.workspace_runtime_sync_lock),
            loaded_workspace_revision: Arc::clone(&self.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&self.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&self.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&self.loaded_coordination_revision),
        };
        let lock_wait_started = Instant::now();
        let guard = config
            .sync_lock
            .lock()
            .expect("workspace runtime sync lock poisoned");
        let report = sync_workspace_runtime_for_read_with_guard(
            &config,
            guard,
            elapsed_ms(lock_wait_started),
        )?;
        if report.coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        if report.deferred {
            runtime.request_refresh();
        }
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
