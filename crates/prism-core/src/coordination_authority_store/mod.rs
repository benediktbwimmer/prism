mod db;
mod factory;
mod sqlite;
mod traits;
mod types;

pub use factory::{
    configured_coordination_authority_store_provider,
    coordination_materialization_enabled_by_default, coordination_materialization_enabled_for_root,
    default_coordination_authority_store_provider, open_coordination_authority_store,
    open_default_coordination_authority_store, resolve_coordination_authority_store_provider,
    CoordinationAuthorityBackendConfig, CoordinationAuthorityStoreProvider,
};
pub use sqlite::SqliteCoordinationAuthorityStore;
pub use traits::CoordinationAuthorityStore;
pub use types::{
    CoordinationAppendRequest, CoordinationAuthorityBackendDetails,
    CoordinationAuthorityBackendKind, CoordinationAuthorityCapabilities,
    CoordinationAuthorityDiagnostics, CoordinationAuthorityProvenance, CoordinationAuthorityStamp,
    CoordinationAuthoritySummary, CoordinationConflictInfo, CoordinationCurrentState,
    CoordinationDerivedStateMode, CoordinationDiagnosticsRequest, CoordinationHistoryEntry,
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
