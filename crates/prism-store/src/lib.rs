mod graph;
mod memory_projection;
mod memory_store;
mod outcome_projection;
mod patch_projection;
mod persistence;
mod sqlite;
mod store;

pub use graph::{
    DependencyInvalidationKeys, FileRecord, FileState, FileUpdate, Graph, GraphSnapshot,
};
pub use memory_store::MemoryStore;
pub use persistence::{
    ColdQueryStore, CoordinationCheckpointStore, CoordinationJournal, EventJournalStore,
    MaterializationStore,
};
pub use patch_projection::{
    PatchEventSummary, PatchEventSummaryQuery, PatchFileSummary, PatchFileSummaryQuery,
};
pub use sqlite::{migrate_worktree_cache_from_shared_runtime, SnapshotRevisions, SqliteStore};
pub use store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationPersistBatch,
    CoordinationPersistContext, CoordinationPersistResult, IndexPersistBatch,
    ProjectionMaterializationMetadata, Store, WorkspaceTreeDirectoryFingerprint,
    WorkspaceTreeFileFingerprint, WorkspaceTreeSnapshot,
};

#[cfg(test)]
mod tests;
