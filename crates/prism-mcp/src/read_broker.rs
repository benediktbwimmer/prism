use std::sync::Arc;

use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, coordination_read_model_from_snapshot,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2,
};
use prism_core::{
    configured_coordination_authority_store_provider, CoordinationAuthorityBackendConfig,
    CoordinationAuthorityBackendKind, CoordinationReadConsistency, CoordinationReadRequest,
    CoordinationAuthorityStamp, CoordinationStateView, WorkspaceSession,
};
use prism_query::Prism;

use crate::workspace_host::WorkspaceRuntimeBinding;

#[derive(Debug, Clone)]
pub(crate) struct CurrentCoordinationSurface {
    pub(crate) snapshot: CoordinationSnapshot,
    pub(crate) snapshot_v2: CoordinationSnapshotV2,
    pub(crate) read_model: CoordinationReadModel,
    pub(crate) queue_read_model: CoordinationQueueReadModel,
    pub(crate) tracked_snapshot_revision: Option<u64>,
    pub(crate) startup_checkpoint_revision: Option<u64>,
    pub(crate) read_model_revision: Option<u64>,
    pub(crate) queue_read_model_revision: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceReadBroker {
    workspace_runtime_binding: Arc<WorkspaceRuntimeBinding>,
}

impl WorkspaceReadBroker {
    pub(crate) fn new(workspace_runtime_binding: Arc<WorkspaceRuntimeBinding>) -> Self {
        Self {
            workspace_runtime_binding,
        }
    }

    pub(crate) fn workspace_session(&self) -> &WorkspaceSession {
        self.workspace_runtime_binding.workspace().as_ref()
    }

    pub(crate) fn current_prism(&self) -> Arc<Prism> {
        self.workspace_runtime_binding.workspace().prism_arc()
    }

    pub(crate) fn current_coordination_surface(&self) -> Result<CurrentCoordinationSurface> {
        current_coordination_surface_for_workspace(
            Some(self.workspace_session()),
            self.current_prism(),
        )
    }

    pub(crate) fn current_coordination_snapshot(&self) -> Result<CoordinationSnapshot> {
        Ok(self.current_coordination_surface()?.snapshot)
    }

    pub(crate) fn current_coordination_snapshot_v2(&self) -> Result<CoordinationSnapshotV2> {
        Ok(self.current_coordination_surface()?.snapshot_v2)
    }

    pub(crate) fn current_coordination_read_model(&self) -> Result<CoordinationReadModel> {
        Ok(self.current_coordination_surface()?.read_model)
    }

    pub(crate) fn current_coordination_queue_read_model(
        &self,
    ) -> Result<CoordinationQueueReadModel> {
        Ok(self.current_coordination_surface()?.queue_read_model)
    }
}

pub(crate) fn current_coordination_surface_for_workspace(
    workspace: Option<&WorkspaceSession>,
    prism: Arc<Prism>,
) -> Result<CurrentCoordinationSurface> {
    if let Some(workspace) = workspace {
        let provider = configured_coordination_authority_store_provider(workspace.root())?;
        if db_authority_reads_collapse_to_current(provider.config()) {
            return current_coordination_surface_from_authority(workspace, &provider);
        }
    }

    let mut snapshot = CoordinationSnapshot::default();
    let mut snapshot_v2 = CoordinationSnapshotV2::default();
    let mut read_model = CoordinationReadModel::default();
    let mut queue_read_model = CoordinationQueueReadModel::default();
    let mut loaded_read_model = false;
    let mut loaded_queue_read_model = false;
    let mut tracked_snapshot_revision = None;
    let mut startup_checkpoint_revision = None;
    let mut read_model_revision = None;
    let mut queue_read_model_revision = None;

    if let Some(workspace) = workspace {
        if let Some(state) = workspace.load_coordination_plan_state()? {
            snapshot = state.snapshot;
            snapshot_v2 = state.canonical_snapshot_v2;
        }
        tracked_snapshot_revision = workspace.load_tracked_coordination_snapshot_revision()?;
        startup_checkpoint_revision = workspace.load_coordination_startup_checkpoint_revision()?;
        if let Some(model) = workspace.load_coordination_read_model()? {
            read_model_revision = Some(model.revision);
            read_model = model;
            loaded_read_model = true;
        }
        if let Some(model) = workspace.load_coordination_queue_read_model()? {
            queue_read_model_revision = Some(model.revision);
            queue_read_model = model;
            loaded_queue_read_model = true;
        }
    } else {
        snapshot = prism.coordination_snapshot();
        snapshot_v2 = prism.coordination_snapshot_v2();
    }

    if !loaded_read_model {
        read_model = coordination_read_model_from_snapshot(&snapshot);
    }
    if !loaded_queue_read_model {
        queue_read_model = coordination_queue_read_model_from_snapshot(&snapshot);
    }

    Ok(CurrentCoordinationSurface {
        snapshot,
        snapshot_v2,
        read_model,
        queue_read_model,
        tracked_snapshot_revision,
        startup_checkpoint_revision,
        read_model_revision,
        queue_read_model_revision,
    })
}

fn db_authority_reads_collapse_to_current(config: &CoordinationAuthorityBackendConfig) -> bool {
    matches!(
        config,
        CoordinationAuthorityBackendConfig::Sqlite { .. }
            | CoordinationAuthorityBackendConfig::Postgres { .. }
    )
}

fn current_coordination_surface_from_authority(
    workspace: &WorkspaceSession,
    provider: &prism_core::CoordinationAuthorityStoreProvider,
) -> Result<CurrentCoordinationSurface> {
    let store = provider.open(workspace.root())?;
    let envelope = store.read_current(CoordinationReadRequest {
        consistency: CoordinationReadConsistency::Eventual,
        view: CoordinationStateView::PlanState,
    })?;
    let authority_revision = authority_revision_from_stamp(envelope.authority.as_ref());
    let current_state = envelope.value.unwrap_or_else(|| prism_core::CoordinationCurrentState {
        snapshot: CoordinationSnapshot::default(),
        canonical_snapshot_v2: CoordinationSnapshotV2::default(),
        runtime_descriptors: Vec::new(),
    });
    let snapshot = current_state.snapshot;
    let snapshot_v2 = current_state.canonical_snapshot_v2;
    let read_model = coordination_read_model_from_snapshot(&snapshot);
    let queue_read_model = coordination_queue_read_model_from_snapshot(&snapshot);
    Ok(CurrentCoordinationSurface {
        snapshot,
        snapshot_v2,
        read_model,
        queue_read_model,
        tracked_snapshot_revision: authority_revision,
        startup_checkpoint_revision: authority_revision,
        read_model_revision: authority_revision,
        queue_read_model_revision: authority_revision,
    })
}

fn authority_revision_from_stamp(authority: Option<&CoordinationAuthorityStamp>) -> Option<u64> {
    let authority = authority?;
    match authority.backend_kind {
        CoordinationAuthorityBackendKind::Sqlite => {
            parse_authority_revision_token(&authority.snapshot_id, "sqlite-revision:")
        }
        CoordinationAuthorityBackendKind::Postgres => {
            parse_authority_revision_token(&authority.snapshot_id, "postgres-revision:")
        }
        CoordinationAuthorityBackendKind::GitSharedRefs => None,
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
