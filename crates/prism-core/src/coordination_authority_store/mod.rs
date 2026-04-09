mod db;
mod factory;
mod sqlite;
mod traits;
mod types;

pub use factory::{
    configured_coordination_authority_store_provider,
    coordination_materialization_enabled_by_default, coordination_materialization_enabled_for_root,
    default_coordination_authority_store_provider, open_coordination_authority_snapshot_store,
    open_coordination_authority_store, open_default_coordination_authority_snapshot_store,
    open_default_coordination_authority_store, resolve_coordination_authority_store_provider,
    CoordinationAuthorityBackendConfig, CoordinationAuthorityStoreProvider,
};
pub use sqlite::SqliteCoordinationAuthorityStore;
pub use traits::{
    CoordinationAuthorityCurrentStateStore, CoordinationAuthorityDiagnosticsStore,
    CoordinationAuthorityEventExecutionStore, CoordinationAuthorityHistoryStore,
    CoordinationAuthorityMutationStore, CoordinationAuthorityRuntimeStore,
    CoordinationAuthoritySnapshotStore, CoordinationAuthorityStore,
};
pub use types::{
    CoordinationAppendRequest, CoordinationAuthorityBackendDetails,
    CoordinationAuthorityBackendKind, CoordinationAuthorityCapabilities,
    CoordinationAuthorityDiagnostics, CoordinationAuthorityProvenance, CoordinationAuthorityStamp,
    CoordinationAuthoritySummary, CoordinationCommitReceipt, CoordinationConflictInfo,
    CoordinationCurrentState, CoordinationDiagnosticsRequest, CoordinationHistoryEntry,
    CoordinationHistoryEnvelope, CoordinationHistoryRequest, CoordinationReadEnvelope,
    CoordinationReplaceCurrentStateRequest, CoordinationTransactionBase,
    CoordinationTransactionDiagnostic, CoordinationTransactionResult,
    CoordinationTransactionStatus, EventExecutionOwnerExpectation,
    EventExecutionRecordAuthorityQuery, EventExecutionRecordWriteResult,
    EventExecutionTransitionKind, EventExecutionTransitionPreconditions,
    EventExecutionTransitionRequest, EventExecutionTransitionResult,
    EventExecutionTransitionStatus, PostgresCoordinationAuthorityBackendDetails,
    RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
    SqliteCoordinationAuthorityBackendDetails,
};
