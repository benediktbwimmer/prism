mod sqlite;
mod traits;
mod types;

pub use sqlite::SqliteSpecMaterializedStore;
pub use traits::SpecMaterializedStore;
pub use types::{
    MaterializedSpecRecord, SpecMaterializationMetadata, SpecMaterializedBackendKind,
    SpecMaterializedCapabilities, SpecMaterializedClearRequest, SpecMaterializedReadEnvelope,
    SpecMaterializedReplaceRequest, SpecMaterializedWriteResult, StoredSpecChecklistItemRecord,
    StoredSpecChecklistPosture, StoredSpecCoverageRecord, StoredSpecDependencyPosture,
    StoredSpecDependencyRecord, StoredSpecStatusRecord, StoredSpecSyncProvenanceRecord,
};
