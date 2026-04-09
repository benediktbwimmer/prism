mod factory;
mod git_shared_refs;
mod sqlite;
mod traits;
mod types;

pub use factory::{
    configured_coordination_authority_store_provider,
    default_coordination_authority_store_provider, open_coordination_authority_store,
    open_default_coordination_authority_store, resolve_coordination_authority_store_provider,
    CoordinationAuthorityBackendConfig, CoordinationAuthorityStoreProvider,
};
pub use git_shared_refs::GitSharedRefsCoordinationAuthorityStore;
pub use sqlite::SqliteCoordinationAuthorityStore;
pub use traits::CoordinationAuthorityStore;
pub use types::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationConflictInfo,
    CoordinationCurrentState, CoordinationDerivedStateMode, CoordinationDiagnosticsRequest,
    CoordinationHistoryEntry, CoordinationHistoryEnvelope, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReadRequest, CoordinationStateView,
    CoordinationTransactionBase, CoordinationTransactionDiagnostic, CoordinationTransactionRequest,
    CoordinationTransactionResult, CoordinationTransactionStatus,
    EventExecutionRecordAuthorityQuery, EventExecutionRecordWriteResult,
    EventExecutionOwnerExpectation, EventExecutionTransitionKind,
    EventExecutionTransitionPreconditions, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, EventExecutionTransitionStatus,
    RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};
