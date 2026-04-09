use anyhow::Result;

use super::types::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedCapabilities,
    SpecMaterializedClearRequest, SpecMaterializedReadEnvelope, SpecMaterializedReplaceRequest,
    SpecMaterializedWriteResult, StoredSpecDependencyRecord,
};
use crate::SpecChecklistItem;

pub trait SpecMaterializedStore {
    fn capabilities(&self) -> SpecMaterializedCapabilities;

    fn read_specs(&self) -> Result<SpecMaterializedReadEnvelope<Vec<MaterializedSpecRecord>>>;

    fn read_checklist_items(&self) -> Result<SpecMaterializedReadEnvelope<Vec<SpecChecklistItem>>>;

    fn read_dependencies(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecDependencyRecord>>>;

    fn read_metadata(&self) -> Result<SpecMaterializationMetadata>;

    fn replace_materialization(
        &self,
        request: SpecMaterializedReplaceRequest,
    ) -> Result<SpecMaterializedWriteResult>;

    fn clear_materialization(
        &self,
        request: SpecMaterializedClearRequest,
    ) -> Result<SpecMaterializedWriteResult>;
}
