use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use prism_coordination::CoordinationSnapshot;
use prism_ir::{PlanExecutionOverlay, PlanGraph};
use prism_store::{CoordinationCheckpointStore, CoordinationStartupCheckpoint};

use crate::published_plans::{
    merge_shared_coordination_into_snapshot, merge_snapshot_bootstrap_into_plan_state,
    HydratedCoordinationPlanState,
};
use crate::shared_coordination_ref::shared_coordination_startup_authority;
use crate::util::current_timestamp;

pub(crate) fn load_materialized_coordination_plan_state<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<HydratedCoordinationPlanState>>
where
    S: CoordinationCheckpointStore + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => {
            let mut plan_graphs = checkpoint.plan_graphs;
            let mut execution_overlays = checkpoint.execution_overlays;
            merge_snapshot_bootstrap_into_plan_state(
                &snapshot,
                &mut plan_graphs,
                &mut execution_overlays,
            );
            Some(HydratedCoordinationPlanState {
                snapshot: merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot),
                plan_graphs,
                execution_overlays,
            })
        }
        None => Some(HydratedCoordinationPlanState {
            snapshot: checkpoint.snapshot,
            plan_graphs: checkpoint.plan_graphs,
            execution_overlays: checkpoint.execution_overlays,
        }),
    })
}

pub(crate) fn load_materialized_coordination_snapshot<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>>
where
    S: CoordinationCheckpointStore + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => Some(merge_shared_coordination_into_snapshot(
            checkpoint.snapshot,
            snapshot,
        )),
        None => Some(checkpoint.snapshot),
    })
}

pub(crate) fn save_shared_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
) -> Result<()>
where
    S: CoordinationCheckpointStore + ?Sized,
{
    let Some(authority) = shared_coordination_startup_authority(root)? else {
        return Ok(());
    };
    store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
        version: CoordinationStartupCheckpoint::VERSION,
        materialized_at: current_timestamp(),
        authority,
        snapshot: snapshot.clone(),
        plan_graphs: plan_graphs.to_vec(),
        execution_overlays: execution_overlays.clone(),
    })
}

fn load_matching_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<CoordinationStartupCheckpoint>>
where
    S: CoordinationCheckpointStore + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    let Some(authority) = shared_coordination_startup_authority(root)? else {
        return Ok(None);
    };
    if checkpoint.authority.ref_name != authority.ref_name {
        return Ok(None);
    }
    if checkpoint.authority.head_commit.is_some()
        && checkpoint.authority.head_commit != authority.head_commit
    {
        return Ok(None);
    }
    if checkpoint.authority.manifest_digest.is_some()
        && authority.manifest_digest.is_some()
        && checkpoint.authority.manifest_digest != authority.manifest_digest
    {
        return Ok(None);
    }
    Ok(Some(checkpoint))
}
