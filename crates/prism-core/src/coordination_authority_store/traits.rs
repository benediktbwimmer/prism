use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::types::{
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics, CoordinationCurrentState,
    CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReadRequest, CoordinationTransactionRequest,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, RuntimeDescriptorClearRequest,
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

    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>>;

    fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult>;

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;

    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics>;
}
