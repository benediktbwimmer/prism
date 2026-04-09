mod coordination_checkpoint;
mod graph;
mod memory_projection;
mod memory_store;
mod outcome_projection;
mod patch_projection;
mod persistence;
mod sqlite;
mod store;

pub use coordination_checkpoint::{
    CoordinationStartupCheckpoint, CoordinationStartupCheckpointAuthority,
};
pub use graph::{
    DependencyInvalidationKeys, FileRecord, FileState, FileUpdate, Graph, GraphSnapshot,
};
pub use memory_store::MemoryStore;
pub use patch_projection::{
    PatchEventSummary, PatchEventSummaryQuery, PatchFileSummary, PatchFileSummaryQuery,
};
pub use persistence::{
    ColdQueryStore, CoordinationCheckpointStore, CoordinationEventExecutionStore,
    CoordinationJournal, EventJournalStore, MaterializationStore,
};
pub use sqlite::{
    migrate_worktree_cache_from_shared_runtime, PatchPathIdentityRepairReport, SnapshotRevisions,
    SqliteStore,
};
pub use store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationMutationLogEntry,
    CoordinationPersistBatch, CoordinationPersistContext, CoordinationPersistResult,
    EventExecutionRecordQuery, IndexPersistBatch, ProjectionMaterializationMetadata, Store,
    WorkspaceTreeDirectoryFingerprint, WorkspaceTreeFileFingerprint, WorkspaceTreeSnapshot,
};

#[cfg(test)]
mod tests;
