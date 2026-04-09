use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use crate::coordination_authority_store::CoordinationReplaceCurrentStateRequest;
use crate::coordination_authority_store::{
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

pub(crate) trait CoordinationAuthorityStampReadDb: Send + Sync {
    fn read_authority_stamp(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthorityStamp>>;
}

pub(crate) trait CoordinationAuthorityCoordinationSurfaceReadDb: Send + Sync {
    fn read_coordination_surface(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthorityCoordinationSurface>>;
}

pub(crate) trait CoordinationAuthorityMutationDb: Send + Sync {
    fn append_events(
        &self,
        request: CoordinationAppendRequest,
    ) -> Result<CoordinationTransactionResult>;
}

pub(crate) trait CoordinationAuthorityRuntimeDb: Send + Sync {
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

pub(crate) trait CoordinationAuthorityEventExecutionDb: Send + Sync {
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

pub(crate) trait CoordinationAuthorityHistoryDb: Send + Sync {
    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;
}

pub(crate) trait CoordinationAuthorityDiagnosticsDb: Send + Sync {
    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics>;
}

pub(crate) trait CoordinationAuthoritySnapshotDb: Send + Sync {
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
