mod git_shared_refs;
mod traits;
mod types;

pub use git_shared_refs::GitSharedRefsCoordinationAuthorityStore;
pub use traits::CoordinationAuthorityStore;
pub use types::{
    CoordinationAuthorityBackendDetails, CoordinationAuthorityBackendKind,
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityProvenance, CoordinationAuthorityStamp, CoordinationConflictInfo,
    CoordinationCurrentState, CoordinationDerivedStateMode, CoordinationDiagnosticsRequest,
    CoordinationHistoryEnvelope, CoordinationHistoryEntry, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReadRequest, CoordinationStateView,
    CoordinationTransactionBase, CoordinationTransactionDiagnostic,
    CoordinationTransactionRequest, CoordinationTransactionResult,
    CoordinationTransactionStatus, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};
