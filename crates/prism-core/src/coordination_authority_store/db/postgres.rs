use std::path::Path;

use anyhow::{anyhow, Result};
use prism_coordination::{
    CoordinationSnapshot, CoordinationSnapshotV2, EventExecutionRecord, RuntimeDescriptor,
};

use super::store::DbCoordinationAuthorityStore;
use super::traits::{CoordinationAuthorityDb, CoordinationAuthoritySnapshotDb};
use crate::coordination_authority_store::{
    CoordinationAppendRequest, CoordinationAuthorityCapabilities, CoordinationAuthorityDiagnostics,
    CoordinationAuthoritySnapshotStore, CoordinationAuthorityStore, CoordinationAuthoritySummary,
    CoordinationDiagnosticsRequest, CoordinationHistoryEnvelope, CoordinationHistoryRequest,
    CoordinationReadEnvelope, CoordinationReplaceCurrentStateRequest,
    CoordinationTransactionResult, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult, EventExecutionTransitionRequest,
    EventExecutionTransitionResult, RuntimeDescriptorClearRequest, RuntimeDescriptorPublishRequest,
    RuntimeDescriptorQuery,
};
use crate::coordination_reads::CoordinationReadConsistency;
use crate::published_plans::HydratedCoordinationPlanState;

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

    fn read_plan_state(
        &self,
        _consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<HydratedCoordinationPlanState>> {
        Err(self.not_implemented())
    }

    fn read_summary(
        &self,
        _consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationAuthoritySummary>> {
        Err(self.not_implemented())
    }

    fn append_events(
        &self,
        _request: CoordinationAppendRequest,
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

impl CoordinationAuthoritySnapshotDb for PostgresCoordinationAuthorityDb {
    fn read_snapshot(
        &self,
        _consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshot>> {
        Err(self.not_implemented())
    }

    fn read_snapshot_v2(
        &self,
        _consistency: CoordinationReadConsistency,
    ) -> Result<CoordinationReadEnvelope<CoordinationSnapshotV2>> {
        Err(self.not_implemented())
    }

    fn replace_current_state(
        &self,
        _request: CoordinationReplaceCurrentStateRequest,
    ) -> Result<CoordinationTransactionResult> {
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

pub(crate) fn open_postgres_coordination_authority_snapshot_store(
    root: &Path,
    connection_url: &str,
) -> Result<Box<dyn CoordinationAuthoritySnapshotStore>> {
    let db = PostgresCoordinationAuthorityDb::open(root, connection_url)?;
    Ok(Box::new(DbCoordinationAuthorityStore::new(db)))
}
