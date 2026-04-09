use anyhow::Result;
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::traits::CoordinationAuthorityDb;
use crate::coordination_authority_store::{
    CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthorityStore, CoordinationCurrentState, CoordinationDiagnosticsRequest,
    CoordinationHistoryEnvelope, CoordinationHistoryRequest, CoordinationReadEnvelope,
    CoordinationReadRequest, CoordinationTransactionRequest, CoordinationTransactionResult,
    EventExecutionRecordAuthorityQuery, EventExecutionRecordWriteResult,
    EventExecutionTransitionRequest, EventExecutionTransitionResult, RuntimeDescriptorClearRequest,
    RuntimeDescriptorPublishRequest, RuntimeDescriptorQuery,
};

pub(crate) struct DbCoordinationAuthorityStore<Db> {
    db: Db,
}

impl<Db> DbCoordinationAuthorityStore<Db> {
    pub(crate) fn new(db: Db) -> Self {
        Self { db }
    }
}

impl<Db> CoordinationAuthorityStore for DbCoordinationAuthorityStore<Db>
where
    Db: CoordinationAuthorityDb,
{
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        self.db.capabilities()
    }

    fn read_current(
        &self,
        request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>> {
        self.db.read_current(request)
    }

    fn apply_transaction(
        &self,
        request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult> {
        self.db.apply_transaction(request)
    }

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

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        self.db.read_history(request)
    }

    fn diagnostics(
        &self,
        request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        self.db.diagnostics(request)
    }
}
