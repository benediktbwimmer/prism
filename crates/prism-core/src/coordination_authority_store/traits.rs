use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::types::CoordinationReplaceCurrentStateRequest;
use super::types::{
    CoordinationAppendRequest, CoordinationAuthorityCoordinationSurface,
    CoordinationAuthorityDiagnostics, CoordinationAuthorityStamp, CoordinationDiagnosticsRequest,
    CoordinationHistoryEnvelope, CoordinationHistoryRequest, CoordinationReadEnvelope,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};

pub trait CoordinationAuthorityStampReadPort: Send + Sync {
    fn read_authority_stamp(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthorityStamp>>;
}

pub trait CoordinationAuthorityCoordinationSurfaceReadPort: Send + Sync {
    fn read_coordination_surface(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthorityCoordinationSurface>>;
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
