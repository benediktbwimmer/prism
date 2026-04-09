use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::traits::{
    CoordinationAuthorityDiagnosticsDb, CoordinationAuthorityEventExecutionDb,
    CoordinationAuthorityHistoryDb, CoordinationAuthorityMutationDb,
    CoordinationAuthorityProjectionDb, CoordinationAuthorityRuntimeDb,
    CoordinationAuthoritySnapshotDb,
};
use crate::coordination_authority_store::{
    CoordinationAppendRequest, CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityDiagnosticsStore, CoordinationAuthorityEventExecutionStore,
    CoordinationAuthorityHistoryStore, CoordinationAuthorityMutationStore,
    CoordinationAuthorityProjectionStore, CoordinationAuthorityRuntimeStore,
    CoordinationAuthoritySnapshotStore, CoordinationAuthoritySummary,
    CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReplaceCurrentStateRequest,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2};

pub(crate) struct DbCoordinationAuthorityStore<Db> {
    db: Db,
}

impl<Db> DbCoordinationAuthorityStore<Db> {
    pub(crate) fn new(db: Db) -> Self {
        Self { db }
    }
}

impl<Db> CoordinationAuthorityProjectionStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityProjectionDb,
{
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        self.db.capabilities()
    }

    fn read_summary(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthoritySummary>> {
        self.db.read_summary(consistency)
    }

    fn read_canonical_snapshot_v2(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>> {
        self.db.read_canonical_snapshot_v2(consistency)
    }
}

impl<Db> CoordinationAuthorityMutationStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityMutationDb,
{
    fn append_events(
        &self,
        request: CoordinationAppendRequest,
    ) -> Result<CoordinationTransactionResult> {
        self.db.append_events(request)
    }
}

impl<Db> CoordinationAuthorityRuntimeStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityRuntimeDb,
{
    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult> {
        self.db.publish_runtime_descriptor(request)
    }

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult> {
        self.db.clear_runtime_descriptor(request)
    }

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>> {
        self.db.list_runtime_descriptors(request)
    }
}

impl<Db> CoordinationAuthorityEventExecutionStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityEventExecutionDb,
{
    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>> {
        self.db.read_event_execution_records(request)
    }

    fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult> {
        self.db.upsert_event_execution_record(record)
    }

    fn apply_event_execution_transition(
        &self,
        request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult> {
        self.db.apply_event_execution_transition(request)
    }
}

impl<Db> CoordinationAuthorityHistoryStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityHistoryDb,
{
    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        self.db.read_history(request)
    }
}

impl<Db> CoordinationAuthorityDiagnosticsStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityDiagnosticsDb,
{
    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        self.db.diagnostics(request)
    }
}

impl<Db> CoordinationAuthoritySnapshotStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthoritySnapshotDb,
{
    fn read_snapshot(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshot>> {
        self.db.read_snapshot(consistency)
    }

    fn read_snapshot_v2(
        &self,
        consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>> {
        self.db.read_snapshot_v2(consistency)
    }

    fn replace_current_state(
        &self,
        request: CoordinationReplaceCurrentStateRequest,
    ) -> Result<CoordinationTransactionResult> {
        self.db.replace_current_state(request)
    }
}
