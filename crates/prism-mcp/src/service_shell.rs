use std::sync::Arc;

use prism_agent::InferenceStore;
use prism_core::{
    sync_live_runtime_descriptor_with_provider, CoordinationAuthorityStoreProvider,
    WorkspaceSession,
};
use prism_memory::SessionMemory;
use tracing::debug;

use crate::diagnostics_state::DiagnosticsState;
use crate::features::PrismMcpFeatures;
use crate::host_mutations::WorkspaceMutationBroker;
use crate::mcp_call_log::McpCallLogStore;
use crate::read_broker::WorkspaceReadBroker;
use crate::session_seed::{load_session_seed, PersistedSessionSeed};
use crate::workspace_host::{WorkspaceRuntimeBinding, WorkspaceRuntimeHost};
use crate::workspace_runtime::WorkspaceAuthoritySyncOwner;

#[derive(Clone)]
pub(crate) struct WorkspaceServiceShell {
    workspace_runtime_binding: Arc<WorkspaceRuntimeBinding>,
    authority_sync_owner: Arc<WorkspaceAuthoritySyncOwner>,
    read_broker: Arc<WorkspaceReadBroker>,
    mutation_broker: Arc<WorkspaceMutationBroker>,
    authority_store_provider: CoordinationAuthorityStoreProvider,
    restored_session_seed: Option<PersistedSessionSeed>,
}

impl WorkspaceServiceShell {
    pub(crate) fn bind_workspace(
        workspace: Arc<WorkspaceSession>,
        notes: Arc<SessionMemory>,
        inferred_edges: Arc<InferenceStore>,
        diagnostics_state: Arc<DiagnosticsState>,
        mcp_call_log_store: Arc<McpCallLogStore>,
        authority_store_provider: CoordinationAuthorityStoreProvider,
        features: &PrismMcpFeatures,
    ) -> Self {
        let workspace_runtime_host = Arc::new(WorkspaceRuntimeHost::new());
        let workspace_runtime_binding = workspace_runtime_host.bind_workspace(
            Arc::clone(&workspace),
            notes,
            inferred_edges,
            diagnostics_state,
            mcp_call_log_store,
            features.runtime_diagnostics_auto_refresh,
        );
        let authority_sync_owner = Arc::new(WorkspaceAuthoritySyncOwner::new(Arc::clone(
            &workspace_runtime_binding,
        )));
        let read_broker = Arc::new(WorkspaceReadBroker::new(Arc::clone(
            &workspace_runtime_binding,
        )));
        let mutation_broker = Arc::new(WorkspaceMutationBroker);
        if features.coordination_layer_enabled() {
            if let Err(error) = sync_live_runtime_descriptor_with_provider(
                workspace.root(),
                &authority_store_provider,
            ) {
                debug!(
                    error = %error,
                    root = %workspace.root().display(),
                    "failed to publish shared coordination runtime descriptor for workspace service shell"
                );
            }
        }
        let restored_session_seed = match load_session_seed(workspace.root()) {
            Ok(seed) => seed,
            Err(error) => {
                debug!(error = %error, "failed to load persisted session seed");
                None
            }
        };
        Self {
            workspace_runtime_binding,
            authority_sync_owner,
            read_broker,
            mutation_broker,
            authority_store_provider,
            restored_session_seed,
        }
    }

    pub(crate) fn workspace_runtime_binding(&self) -> &Arc<WorkspaceRuntimeBinding> {
        &self.workspace_runtime_binding
    }

    pub(crate) fn authority_sync_owner(&self) -> &Arc<WorkspaceAuthoritySyncOwner> {
        &self.authority_sync_owner
    }

    pub(crate) fn read_broker(&self) -> &Arc<WorkspaceReadBroker> {
        &self.read_broker
    }

    pub(crate) fn mutation_broker(&self) -> &Arc<WorkspaceMutationBroker> {
        &self.mutation_broker
    }

    pub(crate) fn authority_store_provider(&self) -> &CoordinationAuthorityStoreProvider {
        &self.authority_store_provider
    }

    pub(crate) fn restored_session_seed(&self) -> Option<&PersistedSessionSeed> {
        self.restored_session_seed.as_ref()
    }
}
