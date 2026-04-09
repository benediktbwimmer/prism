use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_snapshot_v2, coordination_read_model_from_snapshot_v2,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
    CoordinationSnapshotV2,
};
use prism_store::{CoordinationCheckpointStore, CoordinationStartupCheckpoint, SqliteStore};

use super::traits::CoordinationMaterializedStore;
use super::types::{
    CoordinationCompactionWriteRequest, CoordinationMaterializationMetadata,
    CoordinationMaterializedBackendKind, CoordinationMaterializedCapabilities,
    CoordinationMaterializedClearRequest, CoordinationMaterializedReadEnvelope,
    CoordinationMaterializedState, CoordinationMaterializedWriteResult,
    CoordinationReadModelsWriteRequest, CoordinationStartupCheckpointWriteRequest,
};
use crate::coordination_startup_checkpoint::{
    load_persisted_coordination_plan_state, load_persisted_coordination_snapshot,
    load_persisted_coordination_snapshot_v2, save_coordination_startup_checkpoint,
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
        open_coordination_materialized_sqlite_store(&self.root)
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
            startup_checkpoint_coordination_revision: checkpoint
                .as_ref()
                .map(|value| value.coordination_revision),
            startup_checkpoint_version: checkpoint.as_ref().map(|value| value.version),
            startup_checkpoint_materialized_at: checkpoint
                .as_ref()
                .map(|value| value.materialized_at),
            startup_checkpoint_authority: checkpoint.as_ref().map(|value| value.authority.clone()),
            has_snapshot: checkpoint.is_some(),
            has_canonical_snapshot_v2: checkpoint.is_some(),
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

pub(crate) fn coordination_materialization_db_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.coordination_materialization_db_path()
}

pub(crate) fn open_coordination_materialized_sqlite_store(root: &Path) -> Result<SqliteStore> {
    let paths = PrismPaths::for_workspace_root(root)?;
    migrate_legacy_worktree_coordination_state(&paths)?;
    SqliteStore::open(paths.coordination_materialization_db_path()?)
}

fn migrate_legacy_worktree_coordination_state(paths: &PrismPaths) -> Result<()> {
    let target_db_path = paths.coordination_materialization_db_path()?;
    let legacy_db_path = paths.worktree_cache_db_path()?;

    if target_db_path == legacy_db_path || !legacy_db_path.exists() || target_db_path.exists() {
        return Ok(());
    }

    let mut target_store = SqliteStore::open(&target_db_path)?;
    let mut legacy_store = SqliteStore::open(&legacy_db_path)?;
    if let Some(checkpoint) =
        CoordinationCheckpointStore::load_coordination_startup_checkpoint(&mut legacy_store)?
    {
        CoordinationCheckpointStore::save_coordination_startup_checkpoint(
            &mut target_store,
            &checkpoint,
        )?;
    }
    if let Some(read_model) =
        CoordinationCheckpointStore::load_coordination_read_model(&mut legacy_store)?
    {
        CoordinationCheckpointStore::save_coordination_read_model(&mut target_store, &read_model)?;
    }
    if let Some(queue_read_model) =
        CoordinationCheckpointStore::load_coordination_queue_read_model(&mut legacy_store)?
    {
        CoordinationCheckpointStore::save_coordination_queue_read_model(
            &mut target_store,
            &queue_read_model,
        )?;
    }
    if let Some(snapshot) =
        prism_store::Store::load_coordination_event_stream(&mut legacy_store)?.fallback_snapshot
    {
        CoordinationCheckpointStore::save_coordination_compaction(&mut target_store, &snapshot)?;
    }

    Ok(())
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

    fn read_effective_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationReadModel>> {
        if let Some(read_model) = self.read_read_model()?.value {
            let metadata = self.read_metadata()?;
            return Ok(CoordinationMaterializedReadEnvelope::new(
                metadata,
                Some(read_model),
            ));
        }
        let snapshot = self.read_snapshot_v2()?;
        let metadata = snapshot.metadata.clone();
        let value = snapshot.value.map(|snapshot| {
            let mut model = coordination_read_model_from_snapshot_v2(&snapshot);
            model.revision = metadata.coordination_revision.unwrap_or_default();
            model
        });
        Ok(CoordinationMaterializedReadEnvelope::new(metadata, value))
    }

    fn read_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>> {
        self.load_envelope(|store| store.load_coordination_queue_read_model())
    }

    fn read_effective_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>> {
        if let Some(queue_read_model) = self.read_queue_read_model()?.value {
            let metadata = self.read_metadata()?;
            return Ok(CoordinationMaterializedReadEnvelope::new(
                metadata,
                Some(queue_read_model),
            ));
        }
        let snapshot = self.read_snapshot_v2()?;
        let metadata = snapshot.metadata.clone();
        let value = snapshot.value.map(|snapshot| {
            let mut model = coordination_queue_read_model_from_snapshot_v2(&snapshot);
            model.revision = metadata.coordination_revision.unwrap_or_default();
            model
        });
        Ok(CoordinationMaterializedReadEnvelope::new(metadata, value))
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

    fn write_startup_checkpoint(
        &self,
        request: CoordinationStartupCheckpointWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        let mut store = self.open_store()?;
        save_coordination_startup_checkpoint(
            &self.root,
            &mut store,
            &request.snapshot,
            &request.canonical_snapshot_v2,
            Some(&request.runtime_descriptors),
        )?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata_from_store(&mut store)?,
        })
    }

    fn write_read_models(
        &self,
        request: CoordinationReadModelsWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        let mut store = self.open_store()?;
        store.save_coordination_read_model(&request.read_model)?;
        store.save_coordination_queue_read_model(&request.queue_read_model)?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata_from_store(&mut store)?,
        })
    }

    fn write_compaction(
        &self,
        request: CoordinationCompactionWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        let mut store = self.open_store()?;
        store.save_coordination_compaction(&request.snapshot)?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata_from_store(&mut store)?,
        })
    }

    fn clear_materialization(
        &self,
        request: CoordinationMaterializedClearRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        let mut store = self.open_store()?;
        if request.clear_startup_checkpoint {
            CoordinationCheckpointStore::clear_coordination_startup_checkpoint(&mut store)?;
        }
        if request.clear_read_models {
            CoordinationCheckpointStore::clear_coordination_read_model(&mut store)?;
            CoordinationCheckpointStore::clear_coordination_queue_read_model(&mut store)?;
        }
        if request.clear_compaction {
            CoordinationCheckpointStore::clear_coordination_compaction(&mut store)?;
        }
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata_from_store(&mut store)?,
        })
    }
}
