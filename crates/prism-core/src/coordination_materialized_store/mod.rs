mod sqlite;
mod store_backed;
mod traits;
mod types;

pub use sqlite::SqliteCoordinationMaterializedStore;
pub(crate) use store_backed::StoreBackedCoordinationMaterializedStore;
pub use traits::CoordinationMaterializedStore;
pub use types::{
    CoordinationCompactionWriteRequest, CoordinationMaterializationMetadata,
    CoordinationMaterializedBackendKind, CoordinationMaterializedCapabilities,
    CoordinationMaterializedClearRequest, CoordinationMaterializedReadEnvelope,
    CoordinationMaterializedState, CoordinationMaterializedWriteResult,
    CoordinationReadModelsWriteRequest, CoordinationStartupCheckpointWriteRequest,
};
