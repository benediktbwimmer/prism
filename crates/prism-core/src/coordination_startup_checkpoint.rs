use std::path::Path;

use anyhow::Result;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority,
};

use crate::coordination_authority_api::coordination_startup_checkpoint_authority;
use crate::coordination_snapshot_sanitization::sanitize_persisted_coordination_snapshot;
use crate::published_plans::{
    merge_shared_coordination_into_snapshot, HydratedCoordinationPlanState,
};
use crate::util::current_timestamp;

pub(crate) fn load_persisted_coordination_plan_state<S>(
    store: &mut S,
) -> Result<Option<HydratedCoordinationPlanState>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    Ok(Some(hydrated_plan_state_from_checkpoint(checkpoint, None)?))
}

pub(crate) fn load_persisted_coordination_snapshot<S>(
    store: &mut S,
) -> Result<Option<CoordinationSnapshot>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    Ok(Some(snapshot_from_checkpoint(checkpoint, None)))
}

pub(crate) fn load_persisted_coordination_snapshot_v2<S>(
    store: &mut S,
) -> Result<Option<CoordinationSnapshotV2>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let Some(checkpoint) = store.load_coordination_startup_checkpoint()? else {
        return Ok(None);
    };
    Ok(Some(snapshot_v2_from_checkpoint(checkpoint, None)?))
}

pub(crate) fn save_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    _canonical_snapshot_v2: &CoordinationSnapshotV2,
    runtime_descriptors: Option<&[RuntimeDescriptor]>,
) -> Result<()>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let authority = resolve_coordination_startup_checkpoint_authority(root)?;
    let mut checkpoint_snapshot = sanitize_persisted_coordination_snapshot(snapshot.clone());
    checkpoint_snapshot.events.clear();
    store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
        version: CoordinationStartupCheckpoint::VERSION,
        materialized_at: current_timestamp(),
        coordination_revision: store.coordination_revision()?,
        authority,
        snapshot: checkpoint_snapshot.clone(),
        canonical_snapshot_v2: _canonical_snapshot_v2.clone(),
        runtime_descriptors: runtime_descriptors.unwrap_or_default().to_vec(),
    })
}

fn snapshot_from_checkpoint(
    checkpoint: CoordinationStartupCheckpoint,
    snapshot: Option<CoordinationSnapshot>,
) -> CoordinationSnapshot {
    match snapshot {
        Some(snapshot) => merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot),
        None => checkpoint.snapshot,
    }
}

fn snapshot_v2_from_checkpoint(
    checkpoint: CoordinationStartupCheckpoint,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<CoordinationSnapshotV2> {
    let canonical_snapshot_v2 = checkpoint.canonical_snapshot_v2;
    match snapshot {
        Some(snapshot) => {
            let _ = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Ok(canonical_snapshot_v2)
        }
        None => Ok(canonical_snapshot_v2),
    }
}

fn hydrated_plan_state_from_checkpoint(
    checkpoint: CoordinationStartupCheckpoint,
    snapshot: Option<CoordinationSnapshot>,
) -> Result<HydratedCoordinationPlanState> {
    let canonical_snapshot_v2 = checkpoint.canonical_snapshot_v2;
    Ok(match snapshot {
        Some(snapshot) => {
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            HydratedCoordinationPlanState {
                canonical_snapshot_v2,
                snapshot,
                runtime_descriptors: checkpoint.runtime_descriptors,
            }
        }
        None => {
            let snapshot = checkpoint.snapshot;
            HydratedCoordinationPlanState {
                canonical_snapshot_v2,
                snapshot,
                runtime_descriptors: checkpoint.runtime_descriptors,
            }
        }
    })
}

pub(crate) fn resolve_coordination_startup_checkpoint_authority(
    root: &Path,
) -> Result<CoordinationStartupCheckpointAuthority> {
    Ok(
        coordination_startup_checkpoint_authority(root)?.unwrap_or_else(|| {
            CoordinationStartupCheckpointAuthority {
                ref_name: "local-worktree".to_string(),
                head_commit: None,
                manifest_digest: None,
            }
        }),
    )
}
