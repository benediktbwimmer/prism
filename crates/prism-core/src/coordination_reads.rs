#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::coordination_materialized_store::{
    CoordinationMaterializedStore, SqliteCoordinationMaterializedStore,
};
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
    Ok(SqliteCoordinationMaterializedStore::new(root)
        .read_legacy_snapshot()?
        .value)
}

#[cfg(test)]
pub(crate) fn load_eventual_coordination_snapshot_v2_for_root(
    root: &Path,
) -> Result<Option<CoordinationSnapshotV2>> {
    Ok(SqliteCoordinationMaterializedStore::new(root)
        .read_snapshot_v2()?
        .value)
}

#[cfg(test)]
pub(crate) fn load_eventual_coordination_plan_state_for_root(
    root: &Path,
) -> Result<Option<HydratedCoordinationPlanState>> {
    Ok(SqliteCoordinationMaterializedStore::new(root)
        .read_plan_state()?
        .value
        .map(|value| HydratedCoordinationPlanState {
            snapshot: value.legacy_snapshot,
            canonical_snapshot_v2: value.canonical_snapshot_v2,
            runtime_descriptors: value.runtime_descriptors,
        }))
}
