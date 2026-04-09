mod db;
mod factory;
mod sqlite;
mod traits;
mod types;

pub use factory::{
    configured_coordination_authority_store_provider,
    coordination_materialization_enabled_by_default, coordination_materialization_enabled_for_root,
    default_coordination_authority_store_provider,
    open_coordination_authority_coordination_surface_read_port,
    open_coordination_authority_diagnostics_store,
    open_coordination_authority_event_execution_store, open_coordination_authority_history_store,
    open_coordination_authority_mutation_store, open_coordination_authority_runtime_store,
    open_coordination_authority_snapshot_store, open_coordination_authority_stamp_read_port,
    open_default_coordination_authority_coordination_surface_read_port,
    open_default_coordination_authority_diagnostics_store,
    open_default_coordination_authority_event_execution_store,
    open_default_coordination_authority_history_store,
    open_default_coordination_authority_mutation_store,
    open_default_coordination_authority_runtime_store,
    open_default_coordination_authority_snapshot_store,
    open_default_coordination_authority_stamp_read_port,
    resolve_coordination_authority_store_provider, CoordinationAuthorityBackendConfig,
    CoordinationAuthorityStoreProvider,
};
pub use sqlite::SqliteCoordinationAuthorityStore;
pub use traits::{
    CoordinationAuthorityCoordinationSurfaceReadPort, CoordinationAuthorityDiagnosticsStore,
    CoordinationAuthorityEventExecutionStore, CoordinationAuthorityHistoryStore,
    CoordinationAuthorityMutationStore, CoordinationAuthorityRuntimeStore,
    CoordinationAuthoritySnapshotStore, CoordinationAuthorityStampReadPort,
};
pub use types::{
    CoordinationAppendRequest, CoordinationAuthorityBackendDetails,
    CoordinationAuthorityBackendKind, CoordinationAuthorityCoordinationSurface,
    CoordinationAuthorityDiagnostics, CoordinationAuthorityProvenance, CoordinationAuthorityStamp,
    CoordinationCommitReceipt, CoordinationConflictInfo, CoordinationCurrentState,
    CoordinationDiagnosticsRequest, CoordinationHistoryEntry, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReplaceCurrentStateRequest,
    CoordinationTransactionBase, CoordinationTransactionDiagnostic, CoordinationTransactionResult,
    CoordinationTransactionStatus, EventExecutionOwnerExpectation,
    EventExecutionRecordAuthorityQuery, EventExecutionRecordWriteResult,
    EventExecutionTransitionKind, EventExecutionTransitionPreconditions,
    EventExecutionTransitionRequest, EventExecutionTransitionResult,
    EventExecutionTransitionStatus, PostgresCoordinationAuthorityBackendDetails,
    RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
    SqliteCoordinationAuthorityBackendDetails,
};
