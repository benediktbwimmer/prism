use anyhow::Result;
use prism_coordination::RuntimeDescriptor;

use super::types::{
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationCurrentState, CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReadRequest,
    CoordinationTransactionRequest, CoordinationTransactionResult, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};

pub trait CoordinationAuthorityStore: Send + Sync {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities;

    fn read_current(
        &self,
        request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>>;

    fn apply_transaction(
        &self,
        request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>>;

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;

    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics>;
}
