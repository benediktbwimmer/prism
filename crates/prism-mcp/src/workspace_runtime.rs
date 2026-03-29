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
use serde_json::json;
use tracing::{debug, error};

use crate::{log_refresh_workspace, DashboardState, QueryHost, WorkspaceRefreshReport};

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
            deferred: false,
            episodic_reloaded: false,
            inference_reloaded: false,
            coordination_reloaded: false,
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
    let guard = config
        .sync_lock
        .lock()
        .expect("workspace runtime sync lock poisoned");
    sync_workspace_runtime_with_guard(config, guard)
}

fn sync_workspace_runtime_with_guard(
    config: &WorkspaceRuntimeConfig,
    _guard: MutexGuard<'_, ()>,
) -> Result<WorkspaceRefreshReport> {
    let started = Instant::now();
    let refresh_path = match config.workspace.refresh_fs_nonblocking()? {
        FsRefreshStatus::Clean => "none",
        FsRefreshStatus::Refreshed => "full",
        FsRefreshStatus::DeferredBusy => "deferred",
    };
    let deferred = refresh_path == "deferred";
    let revisions = config.workspace.snapshot_revisions()?;
    let (episodic_reloaded, inference_reloaded, coordination_reloaded) = if deferred {
        (false, false, false)
    } else {
        config.loaded_workspace_revision.store(
            config.workspace.loaded_workspace_revision(),
            Ordering::Relaxed,
        );
        (
            reload_episodic_snapshot_if_needed(config, revisions.episodic)?,
            reload_inference_snapshot_if_needed(config, revisions.inference)?,
            reload_coordination_snapshot_if_needed(config, revisions.coordination)?,
        )
    };
    let refresh_path = if refresh_path == "none"
        && (episodic_reloaded || inference_reloaded || coordination_reloaded)
    {
        "auxiliary"
    } else {
        refresh_path
    };
    let duration_ms = started.elapsed().as_millis();
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
        deferred,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
    })
}

fn has_stale_coordination_revision(config: &WorkspaceRuntimeConfig) -> Result<bool> {
    let revisions = config.workspace.snapshot_revisions()?;
    Ok(
        revisions.coordination
            > config.loaded_coordination_revision.load(Ordering::Relaxed),
    )
}

fn try_sync_workspace_runtime(
    config: &WorkspaceRuntimeConfig,
) -> Result<Option<WorkspaceRefreshReport>> {
    match config.sync_lock.try_lock() {
        Ok(guard) => sync_workspace_runtime_with_guard(config, guard).map(Some),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Poisoned(_)) => {
            panic!("workspace runtime sync lock poisoned");
        }
    }
}

pub(crate) fn sync_persisted_workspace_state(
    config: &WorkspaceRuntimeConfig,
) -> Result<WorkspaceRefreshReport> {
    let _guard = config
        .sync_lock
        .lock()
        .expect("workspace runtime sync lock poisoned");
    let started = Instant::now();
    let workspace_reloaded = !config.workspace.refresh_fs()?.is_empty();
    config.loaded_workspace_revision.store(
        config.workspace.loaded_workspace_revision(),
        Ordering::Relaxed,
    );
    let revisions = config.workspace.snapshot_revisions()?;
    let episodic_reloaded = reload_episodic_snapshot_if_needed(config, revisions.episodic)?;
    let inference_reloaded = reload_inference_snapshot_if_needed(config, revisions.inference)?;
    let coordination_reloaded =
        reload_coordination_snapshot_if_needed(config, revisions.coordination)?;
    let deferred = false;
    let refresh_path = if workspace_reloaded {
        "full"
    } else if episodic_reloaded || inference_reloaded || coordination_reloaded {
        "auxiliary"
    } else {
        "none"
    };
    let duration_ms = started.elapsed().as_millis();
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
        deferred,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
    })
}

fn reload_episodic_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<bool> {
    let loaded = config.loaded_episodic_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(false);
    }

    let snapshot = config
        .workspace
        .load_episodic_snapshot()?
        .unwrap_or(EpisodicMemorySnapshot {
            entries: Vec::new(),
        });
    config.notes.replace_from_snapshot(snapshot);
    config
        .loaded_episodic_revision
        .store(revision, Ordering::Relaxed);
    Ok(true)
}

fn reload_inference_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<bool> {
    let loaded = config.loaded_inference_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(false);
    }

    let snapshot = config
        .workspace
        .load_inference_snapshot()?
        .unwrap_or_default();
    config.inferred_edges.replace_from_snapshot(snapshot);
    config
        .loaded_inference_revision
        .store(revision, Ordering::Relaxed);
    Ok(true)
}

fn reload_coordination_snapshot_if_needed(
    config: &WorkspaceRuntimeConfig,
    revision: u64,
) -> Result<bool> {
    let loaded = config.loaded_coordination_revision.load(Ordering::Relaxed);
    if revision == loaded {
        return Ok(false);
    }

    let plan_state = config.workspace.load_coordination_plan_state()?;
    let snapshot = plan_state
        .as_ref()
        .map(|state| state.snapshot.clone())
        .unwrap_or_default();
    config
        .workspace
        .prism_arc()
        .replace_coordination_snapshot_and_plan_graphs(
            snapshot,
            plan_state
                .as_ref()
                .map(|state| state.plan_graphs.clone())
                .unwrap_or_default(),
            plan_state
                .map(|state| state.execution_overlays)
                .unwrap_or_default(),
        );
    config
        .loaded_coordination_revision
        .store(revision, Ordering::Relaxed);
    Ok(true)
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
            let refresh_path = match workspace.refresh_fs_nonblocking()? {
                FsRefreshStatus::Clean => "none",
                FsRefreshStatus::Refreshed => "full",
                FsRefreshStatus::DeferredBusy => "deferred",
            };
            return Ok(WorkspaceRefreshReport {
                refresh_path,
                deferred: refresh_path == "deferred",
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
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
        let Some(report) = try_sync_workspace_runtime(&config)? else {
            if has_stale_coordination_revision(&config)? {
                let report = sync_workspace_runtime(&config)?;
                if report.coordination_reloaded {
                    let _ = self.publish_dashboard_coordination_update();
                }
                return Ok(report);
            }
            runtime.request_refresh();
            return Ok(WorkspaceRefreshReport {
                refresh_path: "deferred",
                deferred: true,
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
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
            let _ = workspace.refresh_fs()?;
            self.sync_workspace_revision(workspace)?;
            self.sync_episodic_revision(workspace)?;
            self.sync_inference_revision(workspace)?;
            self.sync_coordination_revision(workspace)?;
            return Ok(WorkspaceRefreshReport::none());
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
        if report.deferred {
            runtime.request_refresh();
        }
        Ok(report)
    }
}
