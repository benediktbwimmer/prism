#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::coordination_authority_store::{
    configured_coordination_authority_store_provider,
};
#[cfg(test)]
use crate::coordination_materialized_store::{
    CoordinationMaterializedStore, SqliteCoordinationMaterializedStore,
};
#[cfg(test)]
use crate::coordination_persistence::repo_semantic_coordination_snapshot;
#[cfg(test)]
use crate::published_plans::HydratedCoordinationPlanState;
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationReadConsistency {
    Eventual,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationReadFreshness {
    VerifiedCurrent,
    VerifiedStale,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct CoordinationReadResult<T> {
    pub consistency: CoordinationReadConsistency,
    pub freshness: CoordinationReadFreshness,
    pub value: Option<T>,
    pub refresh_error: Option<String>,
}

impl<T> CoordinationReadResult<T> {
    pub(crate) fn verified_current(consistency: CoordinationReadConsistency, value: T) -> Self {
        Self {
            consistency,
            freshness: CoordinationReadFreshness::VerifiedCurrent,
            value: Some(value),
            refresh_error: None,
        }
    }

    pub(crate) fn verified_stale(
        consistency: CoordinationReadConsistency,
        value: T,
        refresh_error: Option<String>,
    ) -> Self {
        Self {
            consistency,
            freshness: CoordinationReadFreshness::VerifiedStale,
            value: Some(value),
            refresh_error,
        }
    }

    pub(crate) fn unavailable(
        consistency: CoordinationReadConsistency,
        refresh_error: Option<String>,
    ) -> Self {
        Self {
            consistency,
            freshness: CoordinationReadFreshness::Unavailable,
            value: None,
            refresh_error,
        }
    }

    pub fn into_value(self) -> Option<T> {
        self.value
    }
}

#[cfg(test)]
pub(crate) fn load_eventual_coordination_snapshot_for_root(
    root: &Path,
) -> Result<Option<CoordinationSnapshot>> {
    if let Some(snapshot) = SqliteCoordinationMaterializedStore::new(root)
        .read_legacy_snapshot()?
        .value
    {
        return Ok(Some(snapshot));
    }
    Ok(configured_coordination_authority_store_provider(root)?
        .open_snapshot(root)?
        .read_snapshot(CoordinationReadConsistency::Eventual)?
        .value
    )
}

#[cfg(test)]
pub(crate) fn load_eventual_coordination_snapshot_v2_for_root(
    root: &Path,
) -> Result<Option<CoordinationSnapshotV2>> {
    if let Some(snapshot_v2) = SqliteCoordinationMaterializedStore::new(root)
        .read_snapshot_v2()?
        .value
    {
        return Ok(Some(snapshot_v2));
    }
    Ok(configured_coordination_authority_store_provider(root)?
        .open_snapshot(root)?
        .read_snapshot_v2(CoordinationReadConsistency::Eventual)?
        .value
    )
}

#[cfg(test)]
pub(crate) fn load_eventual_coordination_plan_state_for_root(
    root: &Path,
) -> Result<Option<HydratedCoordinationPlanState>> {
    if let Some(value) = SqliteCoordinationMaterializedStore::new(root).read_plan_state()?.value {
        return Ok(Some(HydratedCoordinationPlanState {
            snapshot: value.legacy_snapshot,
            canonical_snapshot_v2: value.canonical_snapshot_v2,
            runtime_descriptors: value.runtime_descriptors,
        }));
    }
    Ok(
        crate::published_plans::load_authoritative_coordination_current_state_with_consistency(
            root,
            CoordinationReadConsistency::Eventual,
        )?
        .map(|state| {
            let snapshot = repo_semantic_coordination_snapshot(state.snapshot);
            HydratedCoordinationPlanState {
                canonical_snapshot_v2: snapshot.to_canonical_snapshot_v2(),
                snapshot,
                runtime_descriptors: state.runtime_descriptors,
            }
        }),
    )
}
