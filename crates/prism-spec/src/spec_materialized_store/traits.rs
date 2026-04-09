use anyhow::Result;

use super::types::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedCapabilities,
    SpecMaterializedClearRequest, SpecMaterializedReadEnvelope, SpecMaterializedReplaceRequest,
    SpecMaterializedWriteResult, StoredSpecChecklistItemRecord, StoredSpecCoverageRecord,
    StoredSpecDependencyRecord, StoredSpecStatusRecord, StoredSpecSyncProvenanceRecord,
};

pub trait SpecMaterializedStore {
    fn capabilities(&self) -> SpecMaterializedCapabilities;

    fn read_specs(&self) -> Result<SpecMaterializedReadEnvelope<Vec<MaterializedSpecRecord>>>;

    fn read_checklist_items(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecChecklistItemRecord>>>;

    fn read_dependencies(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecDependencyRecord>>>;

    fn read_status_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecStatusRecord>>>;

    fn read_coverage_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecCoverageRecord>>>;

    fn read_sync_provenance_records(
        &self,
    ) -> Result<SpecMaterializedReadEnvelope<Vec<StoredSpecSyncProvenanceRecord>>>;

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
