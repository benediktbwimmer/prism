use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use crate::coordination_authority_store::{
    CoordinationAppendRequest, CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthoritySummary, CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope,
    CoordinationHistoryRequest, CoordinationReadEnvelope, CoordinationReplaceCurrentStateRequest,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::published_plans::HydratedCoordinationPlanState;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};

pub(crate) trait CoordinationAuthorityDb: Send + Sync {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities;

    fn read_plan_state(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<HydratedCoordinationPlanState>>;

    fn read_snapshot(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshot>>;

    fn read_snapshot_v2(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>>;

    fn read_summary(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthoritySummary>>;

    fn append_events(
        &self,
        request: CoordinationAppendRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn replace_current_state(
        &self,
        request: CoordinationReplaceCurrentStateRequest,
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

    fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult>;

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;

    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics>;
}
