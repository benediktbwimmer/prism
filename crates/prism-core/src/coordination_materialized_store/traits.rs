use anyhow::Result;
use prism_coordination::{
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot, CoordinationSnapshotV2,
};
use prism_store::CoordinationStartupCheckpoint;

use super::types::{
    CoordinationMaterializationMetadata, CoordinationMaterializedCapabilities,
    CoordinationMaterializedReadEnvelope, CoordinationMaterializedState,
};

pub trait CoordinationMaterializedStore: Send + Sync {
    fn capabilities(&self) -> CoordinationMaterializedCapabilities;

    fn read_snapshot(&self) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshot>>;

    fn read_snapshot_v2(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationSnapshotV2>>;

    fn read_plan_state(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationMaterializedState>>;

    fn read_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationReadModel>>;

    fn read_queue_read_model(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationQueueReadModel>>;

    fn read_startup_checkpoint(
        &self,
    ) -> Result<CoordinationMaterializedReadEnvelope<CoordinationStartupCheckpoint>>;

    fn read_metadata(&self) -> Result<CoordinationMaterializationMetadata>;
}
