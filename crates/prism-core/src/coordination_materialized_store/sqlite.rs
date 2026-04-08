use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::{
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_store::{CoordinationCheckpointStore, CoordinationStartupCheckpoint, SqliteStore};

use super::traits::CoordinationMaterializedStore;
use super::types::{
    CoordinationMaterializationMetadata, CoordinationMaterializedBackendKind,
    CoordinationMaterializedCapabilities, CoordinationMaterializedReadEnvelope,
    CoordinationMaterializedState,
};
use crate::coordination_startup_checkpoint::{
    load_persisted_coordination_plan_state, load_persisted_coordination_snapshot,
    load_persisted_coordination_snapshot_v2,
};
use crate::prism_paths::PrismPaths;

pub struct SqliteCoordinationMaterializedStore {
    root: PathBuf,
}

impl SqliteCoordinationMaterializedStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn open_store(&self) -> Result<SqliteStore> {
        let paths = PrismPaths::for_workspace_root(&self.root)?;
        SqliteStore::open(paths.worktree_cache_db_path()?)
    }

    fn load_metadata_from_store(
        &self,
        store: &mut SqliteStore,
    ) -> Result<CoordinationMaterializationMetadata> {
        let checkpoint = store.load_coordination_startup_checkpoint()?;
        let read_model = store.load_coordination_read_model()?;
        let queue_read_model = store.load_coordination_queue_read_model()?;
        let coordination_revision = Some(store.coordination_revision()?);

        Ok(CoordinationMaterializationMetadata {
            backend_kind: CoordinationMaterializedBackendKind::Sqlite,
            coordination_revision,
            startup_checkpoint_version: checkpoint.as_ref().map(|value| value.version),
            startup_checkpoint_materialized_at: checkpoint
                .as_ref()
                .map(|value| value.materialized_at),
            startup_checkpoint_authority: checkpoint.as_ref().map(|value| value.authority.clone()),
            has_snapshot: checkpoint.is_some(),
            has_canonical_snapshot_v2: checkpoint
                .as_ref()
                .and_then(|value| value.canonical_snapshot_v2.as_ref())
                .is_some(),
            runtime_descriptor_count: checkpoint
                .as_ref()
                .map(|value| value.runtime_descriptors.len())
                .unwrap_or_default(),
            has_read_model: read_model.is_some(),
            has_queue_read_model: queue_read_model.is_some(),
        })
    }

    fn load_envelope<T, F>(&self, load: F) -> Result<CoordinationMaterializedReadEnvelope<T>>
    where
        F: FnOnce(&mut SqliteStore) -> Result<Option<T>>,
    {
        let mut store = self.open_store()?;
        let metadata = self.load_metadata_from_store(&mut store)?;
        let value = load(&mut store)?;
        Ok(CoordinationMaterializedReadEnvelope::new(metadata, value))
    }
}

impl CoordinationMaterializedStore for SqliteCoordinationMaterializedStore {
    fn capabilities(&self) -> CoordinationMaterializedCapabilities {
        CoordinationMaterializedCapabilities {
            supports_eventual_snapshots: true,
            supports_read_models: true,
            supports_startup_checkpoints: true,
            supports_metadata: true,
        }
    }

    fn read_snapshot(&self) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshot>> {
        self.load_envelope(load_persisted_coordination_snapshot)
    }

    fn read_snapshot_v2(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshotV2>> {
        self.load_envelope(load_persisted_coordination_snapshot_v2)
    }

    fn read_plan_state(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationMaterializedState>> {
        self.load_envelope(|store| {
            Ok(load_persisted_coordination_plan_state(store)?.map(Into::into))
        })
    }

    fn read_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationReadModel>> {
        self.load_envelope(|store| store.load_coordination_read_model())
    }

    fn read_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>> {
        self.load_envelope(|store| store.load_coordination_queue_read_model())
    }

    fn read_startup_checkpoint(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationStartupCheckpoint>> {
        self.load_envelope(|store| store.load_coordination_startup_checkpoint())
    }

    fn read_metadata(&self) -> Result<CoordinationMaterializationMetadata> {
        let mut store = self.open_store()?;
        self.load_metadata_from_store(&mut store)
    }
}
