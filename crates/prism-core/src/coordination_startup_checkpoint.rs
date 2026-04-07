use std::path::Path;

use anyhow::Result;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};
use prism_store::{
    CoordinationCheckpointStore, CoordinationJournal, CoordinationStartupCheckpoint,
    CoordinationStartupCheckpointAuthority,
};

use crate::coordination_snapshot_sanitization::sanitize_persisted_coordination_snapshot;
use crate::published_plans::{merge_shared_coordination_into_snapshot, HydratedCoordinationPlanState};
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
            let snapshot = merge_shared_coordination_into_snapshot(checkpoint.snapshot, snapshot);
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: checkpoint
                    .canonical_snapshot_v2
                    .unwrap_or_else(|| {
                        canonical_snapshot_v2_from_snapshot(&snapshot)
                            .expect("checkpoint snapshot should project into canonical v2")
                    }),
                snapshot,
                runtime_descriptors: checkpoint.runtime_descriptors,
            })
        }
        None => {
            let snapshot = checkpoint.snapshot;
            Some(HydratedCoordinationPlanState {
                canonical_snapshot_v2: checkpoint
                    .canonical_snapshot_v2
                    .unwrap_or_else(|| canonical_snapshot_v2_from_snapshot(&snapshot).expect("checkpoint snapshot should project into canonical v2")),
                snapshot,
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
            Some(
                checkpoint
                    .canonical_snapshot_v2
                    .unwrap_or_else(|| {
                        canonical_snapshot_v2_from_snapshot(&snapshot)
                            .expect("checkpoint snapshot should project into canonical v2")
                    }),
            )
        }
        None => Some(
            checkpoint
                .canonical_snapshot_v2
                .unwrap_or_else(|| canonical_snapshot_v2_from_snapshot(&checkpoint.snapshot).expect("checkpoint snapshot should project into canonical v2")),
        ),
    })
}

pub(crate) fn save_shared_coordination_startup_checkpoint<S>(
    root: &Path,
    store: &mut S,
    snapshot: &CoordinationSnapshot,
    runtime_descriptors: &[prism_coordination::RuntimeDescriptor],
) -> Result<()>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let authority = coordination_startup_authority(root)?;
    let mut checkpoint_snapshot = sanitize_persisted_coordination_snapshot(snapshot.clone());
    checkpoint_snapshot.events.clear();
    store.save_coordination_startup_checkpoint(&CoordinationStartupCheckpoint {
        version: CoordinationStartupCheckpoint::VERSION,
        materialized_at: current_timestamp(),
        coordination_revision: store.coordination_revision()?,
        authority,
        snapshot: checkpoint_snapshot.clone(),
        canonical_snapshot_v2: Some(canonical_snapshot_v2_from_snapshot(&checkpoint_snapshot)?),
        runtime_descriptors: runtime_descriptors.to_vec(),
    })
}

fn canonical_snapshot_v2_from_snapshot(
    snapshot: &CoordinationSnapshot,
) -> Result<CoordinationSnapshotV2> {
    Ok(snapshot.to_canonical_snapshot_v2())
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
