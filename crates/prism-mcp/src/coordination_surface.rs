use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, coordination_read_model_from_snapshot,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2,
};
use prism_core::WorkspaceSession;
use prism_query::Prism;
use std::sync::Arc;

use crate::{coordination_snapshot_has_data, coordination_snapshot_v2_has_data, QueryHost};

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

pub(crate) fn current_coordination_surface(host: &QueryHost) -> Result<CurrentCoordinationSurface> {
    let workspace = host.workspace_session_ref();
    current_coordination_surface_for_workspace(workspace, host.current_prism())
}

pub(crate) fn current_coordination_surface_for_workspace(
    workspace: Option<&WorkspaceSession>,
    prism: Arc<Prism>,
) -> Result<CurrentCoordinationSurface> {
    let mut snapshot = prism.coordination_snapshot();
    let mut snapshot_v2 = prism.coordination_snapshot_v2();
    let mut tracked_snapshot_revision = None;
    let mut startup_checkpoint_revision = None;
    let mut read_model_revision = None;
    let mut queue_read_model_revision = None;

    if let Some(workspace) = workspace {
        if let Some(state) = workspace.load_coordination_plan_state()? {
            if coordination_snapshot_has_data(&state.snapshot)
                || coordination_snapshot_v2_has_data(&state.canonical_snapshot_v2)
            {
                snapshot = state.snapshot;
                snapshot_v2 = state.canonical_snapshot_v2;
            }
        }
        tracked_snapshot_revision = workspace.load_tracked_coordination_snapshot_revision()?;
        startup_checkpoint_revision = workspace.load_coordination_startup_checkpoint_revision()?;
        read_model_revision = workspace.load_coordination_read_model()?.map(|model| model.revision);
        queue_read_model_revision = workspace
            .load_coordination_queue_read_model()?
            .map(|model| model.revision);
    }

    Ok(CurrentCoordinationSurface {
        read_model: coordination_read_model_from_snapshot(&snapshot),
        queue_read_model: coordination_queue_read_model_from_snapshot(&snapshot),
        snapshot,
        snapshot_v2,
        tracked_snapshot_revision,
        startup_checkpoint_revision,
        read_model_revision,
        queue_read_model_revision,
    })
}
