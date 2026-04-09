use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::types::CoordinationReplaceCurrentStateRequest;
use super::types::{
    CoordinationAppendRequest, CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthoritySummary, CoordinationCurrentState, CoordinationDiagnosticsRequest,
    CoordinationHistoryEnvelope, CoordinationHistoryRequest, CoordinationReadEnvelope,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};

pub trait CoordinationAuthorityCurrentStateStore: Send + Sync {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities;

    fn read_current_state(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>>;

    fn read_summary(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthoritySummary>>;
}

pub trait CoordinationAuthorityMutationStore: Send + Sync {
    fn append_events(
        &self,
        request: CoordinationAppendRequest,
    ) -> Result<CoordinationTransactionResult>;
}

pub trait CoordinationAuthorityRuntimeStore: Send + Sync {
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
}

pub trait CoordinationAuthorityEventExecutionStore: Send + Sync {
    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>>;

    fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult>;

    fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult>;
}

pub trait CoordinationAuthorityHistoryStore: Send + Sync {
    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;
}

pub trait CoordinationAuthorityDiagnosticsStore: Send + Sync {
    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics>;
}

pub trait CoordinationAuthorityStore:
    CoordinationAuthorityCurrentStateStore
    + CoordinationAuthorityMutationStore
    + CoordinationAuthorityRuntimeStore
    + CoordinationAuthorityEventExecutionStore
    + CoordinationAuthorityHistoryStore
    + CoordinationAuthorityDiagnosticsStore
{
}

impl<T> CoordinationAuthorityStore for T where
    T: CoordinationAuthorityCurrentStateStore
        + CoordinationAuthorityMutationStore
        + CoordinationAuthorityRuntimeStore
        + CoordinationAuthorityEventExecutionStore
        + CoordinationAuthorityHistoryStore
        + CoordinationAuthorityDiagnosticsStore
{
}

pub trait CoordinationAuthoritySnapshotStore: Send + Sync {
    fn read_snapshot(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshot>>;

    fn read_snapshot_v2(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>>;

    fn replace_current_state(
        &self,
        request: CoordinationReplaceCurrentStateRequest,
    ) -> Result<CoordinationTransactionResult>;
}
