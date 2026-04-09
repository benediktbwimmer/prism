use anyhow::Result;
use prism_coordination::{
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_store::CoordinationStartupCheckpoint;

use super::types::{
    CoordinationCompactionWriteRequest, CoordinationMaterializationMetadata,
    CoordinationMaterializedCapabilities, CoordinationMaterializedClearRequest,
    CoordinationMaterializedReadEnvelope, CoordinationMaterializedState,
    CoordinationMaterializedWriteResult, CoordinationReadModelsWriteRequest,
    CoordinationStartupCheckpointWriteRequest,
};

pub trait CoordinationMaterializedStore {
    fn capabilities(&self) -> CoordinationMaterializedCapabilities;

    fn read_legacy_snapshot(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshot>>;

    fn read_snapshot_v2(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshotV2>>;

    fn read_plan_state(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationMaterializedState>>;

    fn read_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationReadModel>>;

    fn read_effective_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationReadModel>>;

    fn read_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>>;

    fn read_effective_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>>;

    fn read_startup_checkpoint(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationStartupCheckpoint>>;

    fn read_metadata(&self) -> Result<CoordinationMaterializationMetadata>;

    fn write_startup_checkpoint(
        &self,
        request: CoordinationStartupCheckpointWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult>;

    fn write_read_models(
        &self,
        request: CoordinationReadModelsWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult>;

    fn write_compaction(
        &self,
        request: CoordinationCompactionWriteRequest,
    ) -> Result<CoordinationMaterializedWriteResult>;

    fn clear_materialization(
        &self,
        request: CoordinationMaterializedClearRequest,
    ) -> Result<CoordinationMaterializedWriteResult>;
}
