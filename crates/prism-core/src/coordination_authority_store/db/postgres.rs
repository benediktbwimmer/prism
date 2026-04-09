use std::path::Path;

use anyhow::{anyhow, Result};
use prism_coordination::{EventExecutionRecord, RuntimeDescriptor};

use super::store::DbCoordinationAuthorityStore;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostgresCoordinationAuthorityDb {
    connection_url: String,
}

impl PostgresCoordinationAuthorityDb {
    pub(crate) fn open(_root: &Path, connection_url: &str) -> Result<Self> {
        Err(anyhow!(
            "postgres-backed coordination authority is not implemented yet (configured connection: {connection_url})"
        ))
    }

    fn not_implemented(&self) -> anyhow::Error {
        anyhow!(
            "postgres-backed coordination authority is not implemented yet (configured connection: {})",
            self.connection_url
        )
    }
}

impl CoordinationAuthorityDb for PostgresCoordinationAuthorityDb {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities {
        CoordinationAuthorityCapabilities {
            supports_eventual_reads: false,
            supports_transactions: false,
            supports_runtime_descriptors: false,
            supports_event_execution_records: false,
            supports_retained_history: false,
            supports_diagnostics: false,
        }
    }

    fn read_current(
        &self,
        _request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>> {
        Err(self.not_implemented())
    }

    fn apply_transaction(
        &self,
        _request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult> {
        Err(self.not_implemented())
    }

    fn publish_runtime_descriptor(
        &self,
        _request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult> {
        Err(self.not_implemented())
    }

    fn clear_runtime_descriptor(
        &self,
        _request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult> {
        Err(self.not_implemented())
    }

    fn list_runtime_descriptors(
        &self,
        _request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>> {
        Err(self.not_implemented())
    }

    fn read_event_execution_records(
        &self,
        _request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>> {
        Err(self.not_implemented())
    }

    fn upsert_event_execution_record(
        &self,
        _record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult> {
        Err(self.not_implemented())
    }

    fn apply_event_execution_transition(
        &self,
        _request: EventExecutionTransitionRequest,
    ) -> Result<EventExecutionTransitionResult> {
        Err(self.not_implemented())
    }

    fn read_history(
        &self,
        _request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope> {
        Err(self.not_implemented())
    }

    fn diagnostics(
        &self,
        _request: CoordinationDiagnosticsRequest,
    ) -> Result<CoordinationAuthorityDiagnostics> {
        Err(self.not_implemented())
    }
}

pub(crate) fn open_postgres_coordination_authority_store(
    root: &Path,
    connection_url: &str,
) -> Result<Box<dyn CoordinationAuthorityStore>> {
    let db = PostgresCoordinationAuthorityDb::open(root, connection_url)?;
    Ok(Box::new(DbCoordinationAuthorityStore::new(db)))
}
