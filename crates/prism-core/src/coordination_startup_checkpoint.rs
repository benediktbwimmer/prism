use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use prism_coordination::{
    migrate_legacy_hybrid_snapshot_to_canonical_v2, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_ir::{PlanExecutionOverlay, PlanGraph};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority,
};

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
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => {
            let mut plan_graphs = checkpoint.plan_graphs;
            let mut execution_overlays = checkpoint.execution_overlays;
            let runtime_descriptors = checkpoint.runtime_descriptors;
            merge_snapshot_bootstrap_into_plan_state(
                &snapshot,
                &mut plan_graphs,
                &mut execution_overlays,
            );
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: migrate_legacy_hybrid_snapshot_to_canonical_v2(
                    &snapshot,
                    &plan_graphs,
                    &execution_overlays,
                )?,
                snapshot,
                plan_graphs,
                execution_overlays,
                runtime_descriptors,
            })
        }
        None => {
            let snapshot = checkpoint.snapshot;
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: checkpoint.canonical_snapshot_v2.unwrap_or(
                    migrate_legacy_hybrid_snapshot_to_canonical_v2(
                        &snapshot,
                        &checkpoint.plan_graphs,
                        &checkpoint.execution_overlays,
                    )?,
                ),
                snapshot,
                plan_graphs: checkpoint.plan_graphs,
                execution_overlays: checkpoint.execution_overlays,
                runtime_descriptors: checkpoint.runtime_descriptors,
            })
        }
    })
}

pub(crate) fn load_materialized_coordination_snapshot<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshot>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
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

pub(crate) fn load_materialized_coordination_snapshot_v2<S>(
    root: &Path,
    store: &mut S,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<Option<CoordinationSnapshotV2>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = load_matching_coordination_startup_checkpoint(root, store)? else {
        return Ok(None);
    };
    Ok(match snapshot {
        Some(snapshot) => {
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(migrate_legacy_hybrid_snapshot_to_canonical_v2(
                &snapshot,
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
            )?)
        }
        None => Some(checkpoint.canonical_snapshot_v2.unwrap_or(
            migrate_legacy_hybrid_snapshot_to_canonical_v2(
                &checkpoint.snapshot,
                &checkpoint.plan_graphs,
                &checkpoint.execution_overlays,
            )?,
        )),
    })
}

pub(crate) fn save_shared_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    plan_graphs: &[PlanGraph],
    execution_overlays: &BTreeMap<String, Vec<PlanExecutionOverlay>>,
    runtime_descriptors: &[prism_coordination::RuntimeDescriptor],
) -> Result<()>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let authority = coordination_startup_authority(root)?;
    let mut checkpoint_snapshot = snapshot.clone();
    checkpoint_snapshot.events.clear();
    store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
        version: CoordinationStartupCheckpoint::VERSION,
        materialized_at: current_timestamp(),
        coordination_revision: store.coordination_revision()?,
        authority,
        snapshot: checkpoint_snapshot.clone(),
        canonical_snapshot_v2: Some(migrate_legacy_hybrid_snapshot_to_canonical_v2(
            &checkpoint_snapshot,
            plan_graphs,
            execution_overlays,
        )?),
        plan_graphs: plan_graphs.to_vec(),
        execution_overlays: execution_overlays.clone(),
        runtime_descriptors: runtime_descriptors.to_vec(),
    })
}

fn load_matching_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<CoordinationStartupCheckpoint>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    if store.coordination_revision()? > checkpoint.coordination_revision {
        return Ok(None);
    }
    let authority = coordination_startup_authority(root)?;
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

pub(crate) fn coordination_startup_authority(
    root: &Path,
) -> Result<CoordinationStartupCheckpointAuthority> {
    Ok(
        shared_coordination_startup_authority(root)?.unwrap_or_else(|| {
            CoordinationStartupCheckpointAuthority {
                ref_name: "local-worktree".to_string(),
                head_commit: None,
                manifest_digest: None,
            }
        }),
    )
}
