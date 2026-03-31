use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use prism_agent::InferenceStore;
use prism_core::runtime_engine::WorkspaceRuntimeContext;
use prism_core::WorkspaceSession;
use prism_memory::SessionMemory;

use crate::dashboard_events::DashboardState;
use crate::workspace_runtime::{
    hydrate_persisted_workspace_state, WorkspaceRuntime, WorkspaceRuntimeConfig,
};

static WORKSPACE_RUNTIME_SYNC_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> =
    OnceLock::new();

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeBinding {
    context: WorkspaceRuntimeContext,
    workspace: Arc<WorkspaceSession>,
    notes: Arc<SessionMemory>,
    inferred_edges: Arc<InferenceStore>,
    dashboard_state: Arc<DashboardState>,
    sync_lock: Arc<Mutex<()>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    loaded_episodic_revision: Arc<AtomicU64>,
    loaded_inference_revision: Arc<AtomicU64>,
    loaded_coordination_revision: Arc<AtomicU64>,
    runtime: Arc<WorkspaceRuntime>,
}

impl WorkspaceRuntimeBinding {
    pub(crate) fn new(
        workspace: Arc<WorkspaceSession>,
        notes: Arc<SessionMemory>,
        inferred_edges: Arc<InferenceStore>,
        dashboard_state: Arc<DashboardState>,
    ) -> Self {
        let context = WorkspaceRuntimeContext::from_root(workspace.root());
        let sync_lock = shared_workspace_runtime_sync_lock(context.root());
        let loaded_workspace_revision = workspace.loaded_workspace_revision_handle();
        let loaded_episodic_revision = Arc::new(AtomicU64::new(0));
        let loaded_inference_revision = Arc::new(AtomicU64::new(0));
        let loaded_coordination_revision = Arc::new(AtomicU64::new(0));
        let config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::clone(&notes),
            inferred_edges: Arc::clone(&inferred_edges),
            dashboard_state: Arc::clone(&dashboard_state),
            sync_lock: Arc::clone(&sync_lock),
            loaded_workspace_revision: Arc::clone(&loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&loaded_coordination_revision),
        };
        let runtime = Arc::new(WorkspaceRuntime::spawn(config.clone()));
        let _ = hydrate_persisted_workspace_state(&config);
        if workspace.needs_refresh() {
            runtime.request_refresh();
        }
        Self {
            context,
            workspace,
            notes,
            inferred_edges,
            dashboard_state,
            sync_lock,
            loaded_workspace_revision,
            loaded_episodic_revision,
            loaded_inference_revision,
            loaded_coordination_revision,
            runtime,
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
    pub(crate) fn sync_lock(&self) -> &Arc<Mutex<()>> {
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

    pub(crate) fn runtime_config(&self) -> WorkspaceRuntimeConfig {
        WorkspaceRuntimeConfig {
            workspace: Arc::clone(&self.workspace),
            notes: Arc::clone(&self.notes),
            inferred_edges: Arc::clone(&self.inferred_edges),
            dashboard_state: Arc::clone(&self.dashboard_state),
            sync_lock: Arc::clone(&self.sync_lock),
            loaded_workspace_revision: Arc::clone(&self.loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&self.loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&self.loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&self.loaded_coordination_revision),
        }
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
        ));
        bindings.insert(binding.context().root().to_path_buf(), Arc::clone(&binding));
        binding
    }

    pub(crate) fn binding_for_root(&self, root: &Path) -> Option<Arc<WorkspaceRuntimeBinding>> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.bindings
            .lock()
            .expect("workspace runtime host registry poisoned")
            .get(&root)
            .cloned()
    }
}

fn shared_workspace_runtime_sync_lock(root: &Path) -> Arc<Mutex<()>> {
    let locks = WORKSPACE_RUNTIME_SYNC_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("workspace runtime sync-lock registry poisoned");
    if let Some(existing) = locks.get(root).and_then(Weak::upgrade) {
        return existing;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}
