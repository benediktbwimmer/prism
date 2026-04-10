use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use tracing::{debug, error};

use crate::diagnostics_state::DiagnosticsState;
use crate::mcp_call_log::McpCallLogStore;
use crate::runtime_views::refresh_cached_runtime_status_for_config;
use prism_core::WorkspaceSession;
use prism_core::runtime_engine::WorkspaceRuntimeEngine;

const DIAGNOSTICS_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct WorkspaceDiagnosticsConfig {
    pub(crate) workspace: Arc<WorkspaceSession>,
    pub(crate) loaded_workspace_revision: Arc<AtomicU64>,
    pub(crate) loaded_episodic_revision: Arc<AtomicU64>,
    pub(crate) loaded_inference_revision: Arc<AtomicU64>,
    pub(crate) loaded_coordination_revision: Arc<AtomicU64>,
    pub(crate) runtime_engine: Arc<Mutex<WorkspaceRuntimeEngine>>,
    pub(crate) diagnostics_state: Arc<DiagnosticsState>,
    pub(crate) mcp_call_log_store: Arc<McpCallLogStore>,
}

pub(crate) struct WorkspaceDiagnosticsRuntime {
    enabled: bool,
    wake: SyncSender<()>,
    stop: mpsc::Sender<()>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl WorkspaceDiagnosticsRuntime {
    pub(crate) fn spawn(config: WorkspaceDiagnosticsConfig, enabled: bool) -> Self {
        let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let handle = enabled.then(|| {
            thread::spawn(move || {
                loop {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    match wake_rx.recv_timeout(DIAGNOSTICS_REFRESH_INTERVAL) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    if let Err(error) = refresh_cached_runtime_status_for_config(&config) {
                        error!(
                            root = %config.workspace.root().display(),
                            error = %error,
                            error_chain = %crate::logging::format_error_chain(&error),
                            "prism-mcp diagnostics refresh failed"
                        );
                    }
                }
            })
        });
        Self {
            enabled,
            wake: wake_tx,
            stop: stop_tx,
            handle: Mutex::new(handle),
        }
    }

    pub(crate) fn request_refresh(&self) {
        if !self.enabled {
            return;
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                debug!("workspace diagnostics wake channel disconnected");
            }
        }
    }
}

impl Drop for WorkspaceDiagnosticsRuntime {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = self.wake.try_send(());
        if let Some(handle) = self
            .handle
            .lock()
            .expect("workspace diagnostics handle lock poisoned")
            .take()
        {
            let _ = handle.join();
        }
    }
}
