use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use prism_agent::InferenceStore;
use prism_core::runtime_engine::{
    WorkspacePublishedGeneration, WorkspaceRuntimeContext, WorkspaceRuntimeEngine,
};
use prism_core::WorkspaceSession;
use prism_memory::SessionMemory;

use crate::dashboard_events::DashboardState;
use crate::diagnostics_state::DiagnosticsState;
use crate::mcp_call_log::McpCallLogStore;
use crate::runtime_views::refresh_cached_runtime_status_for_config;
use crate::workspace_diagnostics::{WorkspaceDiagnosticsConfig, WorkspaceDiagnosticsRuntime};
use crate::workspace_runtime::{
    hydrate_persisted_workspace_state, WorkspaceRuntime, WorkspaceRuntimeConfig,
};

static WORKSPACE_RUNTIME_SYNC_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<RwLock<()>>>>> =
    OnceLock::new();
static WORKSPACE_RUNTIME_CURRENT_REVISIONS: OnceLock<
    Mutex<HashMap<PathBuf, Weak<SharedWorkspaceRuntimeRevisions>>>,
> = OnceLock::new();

pub(crate) struct SharedWorkspaceRuntimeRevisions {
    current_workspace_revision: AtomicU64,
    current_episodic_revision: AtomicU64,
    current_inference_revision: AtomicU64,
    current_coordination_revision: AtomicU64,
}

impl SharedWorkspaceRuntimeRevisions {
    pub(crate) fn new(
        workspace_revision: u64,
        episodic_revision: u64,
        inference_revision: u64,
        coordination_revision: u64,
    ) -> Self {
        Self {
            current_workspace_revision: AtomicU64::new(workspace_revision),
            current_episodic_revision: AtomicU64::new(episodic_revision),
            current_inference_revision: AtomicU64::new(inference_revision),
            current_coordination_revision: AtomicU64::new(coordination_revision),
        }
    }

    pub(crate) fn current_workspace_revision(&self) -> &AtomicU64 {
        &self.current_workspace_revision
    }

    pub(crate) fn current_episodic_revision(&self) -> &AtomicU64 {
        &self.current_episodic_revision
    }

    pub(crate) fn current_inference_revision(&self) -> &AtomicU64 {
        &self.current_inference_revision
    }

    pub(crate) fn current_coordination_revision(&self) -> &AtomicU64 {
        &self.current_coordination_revision
    }
}

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeBinding {
    context: WorkspaceRuntimeContext,
    workspace: Arc<WorkspaceSession>,
    notes: Arc<SessionMemory>,
    inferred_edges: Arc<InferenceStore>,
    dashboard_state: Arc<DashboardState>,
    diagnostics_state: Arc<DiagnosticsState>,
    mcp_call_log_store: Arc<McpCallLogStore>,
    sync_lock: Arc<RwLock<()>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    loaded_episodic_revision: Arc<AtomicU64>,
    loaded_inference_revision: Arc<AtomicU64>,
    loaded_coordination_revision: Arc<AtomicU64>,
    current_revisions: Arc<SharedWorkspaceRuntimeRevisions>,
    engine: Arc<Mutex<WorkspaceRuntimeEngine>>,
    prepared_delta: Arc<Mutex<Option<crate::workspace_runtime::PreparedWorkspaceRuntimeDelta>>>,
    runtime: Arc<WorkspaceRuntime>,
    diagnostics: Arc<WorkspaceDiagnosticsRuntime>,
}

impl WorkspaceRuntimeBinding {
    pub(crate) fn new(
        workspace: Arc<WorkspaceSession>,
        notes: Arc<SessionMemory>,
        inferred_edges: Arc<InferenceStore>,
        dashboard_state: Arc<DashboardState>,
        diagnostics_state: Arc<DiagnosticsState>,
        mcp_call_log_store: Arc<McpCallLogStore>,
    ) -> Self {
        let context = WorkspaceRuntimeContext::from_root(workspace.root());
        let sync_lock = shared_workspace_runtime_sync_lock(context.root());
        let engine = Arc::new(Mutex::new(WorkspaceRuntimeEngine::new(context.clone())));
        let prepared_delta = Arc::new(Mutex::new(None));
        let loaded_workspace_revision = workspace.loaded_workspace_revision_handle();
        let loaded_episodic_revision = Arc::new(AtomicU64::new(0));
        let loaded_inference_revision = Arc::new(AtomicU64::new(0));
        let loaded_coordination_revision = Arc::new(AtomicU64::new(0));
        let current_revisions = shared_workspace_runtime_current_revisions(
            context.root(),
            workspace
                .workspace_revision()
                .unwrap_or_else(|_| workspace.loaded_workspace_revision()),
            workspace.episodic_revision().unwrap_or(0),
            workspace.inference_revision().unwrap_or(0),
            workspace.coordination_revision().unwrap_or(0),
        );
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::clone(&notes),
            inferred_edges: Arc::clone(&inferred_edges),
            dashboard_state: Arc::clone(&dashboard_state),
            diagnostics_state: Arc::clone(&diagnostics_state),
            mcp_call_log_store: Arc::clone(&mcp_call_log_store),
            sync_lock: Arc::clone(&sync_lock),
            loaded_workspace_revision: Arc::clone(&loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&loaded_coordination_revision),
            current_revisions: Arc::clone(&current_revisions),
            runtime_engine: Arc::clone(&engine),
            prepared_delta: Arc::clone(&prepared_delta),
        };
        let runtime = Arc::new(WorkspaceRuntime::spawn(config.clone()));
        let diagnostics = Arc::new(WorkspaceDiagnosticsRuntime::spawn(
            WorkspaceDiagnosticsConfig {
                workspace: Arc::clone(&workspace),
                loaded_workspace_revision: Arc::clone(&loaded_workspace_revision),
                loaded_episodic_revision: Arc::clone(&loaded_episodic_revision),
                loaded_inference_revision: Arc::clone(&loaded_inference_revision),
                loaded_coordination_revision: Arc::clone(&loaded_coordination_revision),
                runtime_engine: Arc::clone(&engine),
                diagnostics_state: Arc::clone(&diagnostics_state),
                mcp_call_log_store: Arc::clone(&mcp_call_log_store),
            },
        ));
        let _ = hydrate_persisted_workspace_state(&config);
        let diagnostics_config = WorkspaceDiagnosticsConfig {
            workspace: Arc::clone(&workspace),
            loaded_workspace_revision: Arc::clone(&loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&loaded_coordination_revision),
            runtime_engine: Arc::clone(&engine),
            diagnostics_state: Arc::clone(&diagnostics_state),
            mcp_call_log_store: Arc::clone(&mcp_call_log_store),
        };
        let _ = refresh_cached_runtime_status_for_config(&diagnostics_config);
        if workspace.needs_refresh() {
            runtime.request_refresh_with_revisions(workspace.pending_refresh_path_requests());
        }
        diagnostics.request_refresh();
        Self {
            context,
            workspace,
            notes,
            inferred_edges,
            dashboard_state,
            diagnostics_state,
            mcp_call_log_store,
            sync_lock,
            loaded_workspace_revision,
            loaded_episodic_revision,
            loaded_inference_revision,
            loaded_coordination_revision,
            current_revisions,
            engine,
            prepared_delta,
            runtime,
            diagnostics,
        }
    }

    pub(crate) fn context(&self) -> &WorkspaceRuntimeContext {
        &self.context
    }

    pub(crate) fn workspace(&self) -> &Arc<WorkspaceSession> {
        &self.workspace
    }

    pub(crate) fn runtime(&self) -> &Arc<WorkspaceRuntime> {
        &self.runtime
    }

    #[cfg(test)]
    pub(crate) fn sync_lock(&self) -> &Arc<RwLock<()>> {
        &self.sync_lock
    }

    pub(crate) fn loaded_workspace_revision(&self) -> &Arc<AtomicU64> {
        &self.loaded_workspace_revision
    }

    pub(crate) fn loaded_episodic_revision(&self) -> &Arc<AtomicU64> {
        &self.loaded_episodic_revision
    }

    pub(crate) fn loaded_inference_revision(&self) -> &Arc<AtomicU64> {
        &self.loaded_inference_revision
    }

    pub(crate) fn loaded_coordination_revision(&self) -> &Arc<AtomicU64> {
        &self.loaded_coordination_revision
    }

    pub(crate) fn current_revisions(&self) -> &Arc<SharedWorkspaceRuntimeRevisions> {
        &self.current_revisions
    }

    pub(crate) fn runtime_config(&self) -> WorkspaceRuntimeConfig {
        WorkspaceRuntimeConfig {
            workspace: Arc::clone(&self.workspace),
            notes: Arc::clone(&self.notes),
            inferred_edges: Arc::clone(&self.inferred_edges),
            dashboard_state: Arc::clone(&self.dashboard_state),
            diagnostics_state: Arc::clone(&self.diagnostics_state),
            mcp_call_log_store: Arc::clone(&self.mcp_call_log_store),
            sync_lock: Arc::clone(&self.sync_lock),
            loaded_workspace_revision: Arc::clone(&self.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&self.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&self.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&self.loaded_coordination_revision),
            current_revisions: Arc::clone(&self.current_revisions),
            runtime_engine: Arc::clone(&self.engine),
            prepared_delta: Arc::clone(&self.prepared_delta),
        }
    }

    pub(crate) fn diagnostics(&self) -> &Arc<WorkspaceDiagnosticsRuntime> {
        &self.diagnostics
    }

    pub(crate) fn published_generation_snapshot(&self) -> WorkspacePublishedGeneration {
        self.engine
            .lock()
            .expect("workspace runtime engine lock poisoned")
            .published_generation_snapshot()
    }
}

#[derive(Default)]
pub(crate) struct WorkspaceRuntimeHost {
    bindings: Mutex<HashMap<PathBuf, Arc<WorkspaceRuntimeBinding>>>,
}

impl WorkspaceRuntimeHost {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn bind_workspace(
        &self,
        workspace: Arc<WorkspaceSession>,
        notes: Arc<SessionMemory>,
        inferred_edges: Arc<InferenceStore>,
        dashboard_state: Arc<DashboardState>,
        diagnostics_state: Arc<DiagnosticsState>,
        mcp_call_log_store: Arc<McpCallLogStore>,
    ) -> Arc<WorkspaceRuntimeBinding> {
        let mut bindings = self
            .bindings
            .lock()
            .expect("workspace runtime host registry poisoned");
        let lookup_root = workspace
            .root()
            .canonicalize()
            .unwrap_or_else(|_| workspace.root().to_path_buf());
        if let Some(existing) = bindings.get(&lookup_root) {
            return Arc::clone(existing);
        }
        let binding = Arc::new(WorkspaceRuntimeBinding::new(
            workspace,
            notes,
            inferred_edges,
            dashboard_state,
            diagnostics_state,
            mcp_call_log_store,
        ));
        bindings.insert(binding.context().root().to_path_buf(), Arc::clone(&binding));
        binding
    }
}

fn shared_workspace_runtime_sync_lock(root: &Path) -> Arc<RwLock<()>> {
    let locks = WORKSPACE_RUNTIME_SYNC_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("workspace runtime sync-lock registry poisoned");
    if let Some(existing) = locks.get(root).and_then(Weak::upgrade) {
        return existing;
    }
    let lock = Arc::new(RwLock::new(()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn shared_workspace_runtime_current_revisions(
    root: &Path,
    workspace_revision: u64,
    episodic_revision: u64,
    inference_revision: u64,
    coordination_revision: u64,
) -> Arc<SharedWorkspaceRuntimeRevisions> {
    let revisions = WORKSPACE_RUNTIME_CURRENT_REVISIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut revisions = revisions
        .lock()
        .expect("workspace runtime current-revisions registry poisoned");
    if let Some(existing) = revisions.get(root).and_then(Weak::upgrade) {
        return existing;
    }
    let current = Arc::new(SharedWorkspaceRuntimeRevisions::new(
        workspace_revision,
        episodic_revision,
        inference_revision,
        coordination_revision,
    ));
    revisions.insert(root.to_path_buf(), Arc::downgrade(&current));
    current
}
