use std::path::Path;

use anyhow::Result;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};
use prism_store::{CoordinationCheckpointStore, CoordinationJournal};

use crate::coordination_startup_checkpoint::{
    load_persisted_coordination_plan_state, load_persisted_coordination_snapshot,
    load_persisted_coordination_snapshot_v2,
};
use crate::published_plans::HydratedCoordinationPlanState;

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

pub(crate) fn load_eventual_coordination_snapshot_for_root<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<CoordinationSnapshot>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let _ = root;
    load_persisted_coordination_snapshot(store)
}

pub(crate) fn load_eventual_coordination_snapshot_v2_for_root<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<CoordinationSnapshotV2>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let _ = root;
    load_persisted_coordination_snapshot_v2(store)
}

pub(crate) fn load_eventual_coordination_plan_state_for_root<S>(
    root: &Path,
    store: &mut S,
) -> Result<Option<HydratedCoordinationPlanState>>
where
    S: CoordinationCheckpointStore + CoordinationJournal + ?Sized,
{
    let _ = root;
    load_persisted_coordination_plan_state(store)
}
