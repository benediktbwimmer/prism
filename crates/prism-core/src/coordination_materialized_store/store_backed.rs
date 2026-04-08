use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_store::{CoordinationCheckpointStore, CoordinationJournal};

use super::types::{
    CoordinationCompactionWriteRequest, CoordinationMaterializationMetadata,
    CoordinationMaterializedBackendKind, CoordinationMaterializedWriteResult,
    CoordinationReadModelsWriteRequest, CoordinationStartupCheckpointWriteRequest,
};
use crate::coordination_startup_checkpoint::save_shared_coordination_startup_checkpoint;

pub(crate) struct StoreBackedCoordinationMaterializedStore<'a, S: ?Sized> {
    root: PathBuf,
    store: &'a mut S,
}

impl<'a, S: ?Sized> StoreBackedCoordinationMaterializedStore<'a, S> {
    pub(crate) fn new(root: &Path, store: &'a mut S) -> Self {
        Self {
            root: root.to_path_buf(),
            store,
        }
    }
}

impl<S> StoreBackedCoordinationMaterializedStore<'_, S>
where
    S: CoordinationJournal + CoordinationCheckpointStore + ?Sized,
{
    fn load_metadata(&mut self) -> Result<CoordinationMaterializationMetadata> {
        let checkpoint = self.store.load_coordination_startup_checkpoint()?;
        let read_model = self.store.load_coordination_read_model()?;
        let queue_read_model = self.store.load_coordination_queue_read_model()?;
        let coordination_revision = Some(self.store.coordination_revision()?);

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

    pub(crate) fn write_startup_checkpoint_mut(
        &mut self,
        request: CoordinationStartupCheckpointWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        save_shared_coordination_startup_checkpoint(
            &self.root,
            self.store,
            &request.snapshot,
            &request.canonical_snapshot_v2,
            Some(&request.runtime_descriptors),
        )?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata()?,
        })
    }

    pub(crate) fn write_read_models_mut(
        &mut self,
        request: CoordinationReadModelsWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        self.store
            .save_coordination_read_model(&request.read_model)?;
        self.store
            .save_coordination_queue_read_model(&request.queue_read_model)?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata()?,
        })
    }

    pub(crate) fn write_compaction_mut(
        &mut self,
        request: CoordinationCompactionWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult> {
        self.store.save_coordination_compaction(&request.snapshot)?;
        Ok(CoordinationMaterializedWriteResult {
            metadata: self.load_metadata()?,
        })
    }
}
