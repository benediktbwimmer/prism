use std::path::Path;

use anyhow::Result;
use prism_coordination::CoordinationSnapshot;
use prism_store::{AuxiliaryPersistBatch, Store};

use crate::published_plans::{
    load_hydrated_coordination_plan_state, load_hydrated_coordination_snapshot,
    sync_repo_published_plans, HydratedCoordinationPlanState,
};

pub(crate) trait CoordinationPersistenceBackend: Store {
    fn load_hydrated_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<CoordinationSnapshot>> {
        load_hydrated_coordination_snapshot(root, self.load_coordination_snapshot()?)
    }

    fn load_hydrated_coordination_plan_state_for_root(
        &mut self,
        root: &Path,
    ) -> Result<Option<HydratedCoordinationPlanState>> {
        load_hydrated_coordination_plan_state(root, self.load_coordination_snapshot()?)
    }

    fn persist_coordination_snapshot_for_root(
        &mut self,
        root: &Path,
        snapshot: &CoordinationSnapshot,
    ) -> Result<()> {
        self.commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            coordination_snapshot: Some(snapshot.clone()),
            ..AuxiliaryPersistBatch::default()
        })?;
        sync_repo_published_plans(root, snapshot)
    }
}

impl<T: Store + ?Sized> CoordinationPersistenceBackend for T {}
