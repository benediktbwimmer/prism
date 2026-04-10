use std::path::Path;

use anyhow::Result;

use crate::coordination_authority_store::{
    CoordinationAuthorityEventExecutionStore, CoordinationAuthorityStoreProvider,
};
use crate::{CoordinationReadEnvelope, EventExecutionRecordAuthorityQuery};

use super::types::{
    shared_execution_records_from_event_envelope, SharedExecutionRecord,
    SharedExecutionRecordQuery, SharedExecutionRecordWriteResult, SharedExecutionTransitionRequest,
    SharedExecutionTransitionResult,
};

pub trait SharedExecutionSubstrateStore: Send + Sync {
    fn read_execution_records(
        &self,
        request: SharedExecutionRecordQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<SharedExecutionRecord>>>;

    fn upsert_execution_record(
        &self,
        record: SharedExecutionRecord,
    ) -> Result<SharedExecutionRecordWriteResult>;

    fn apply_execution_transition(
        &self,
        request: SharedExecutionTransitionRequest,
    ) -> Result<SharedExecutionTransitionResult>;
}

pub struct AuthorityBackedSharedExecutionSubstrateStore {
    event_execution_store: Box<dyn CoordinationAuthorityEventExecutionStore>,
}

impl AuthorityBackedSharedExecutionSubstrateStore {
    pub fn new(event_execution_store: Box<dyn CoordinationAuthorityEventExecutionStore>) -> Self {
        Self {
            event_execution_store,
        }
    }
}

impl SharedExecutionSubstrateStore for AuthorityBackedSharedExecutionSubstrateStore {
    fn read_execution_records(
        &self,
        request: SharedExecutionRecordQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<SharedExecutionRecord>>> {
        let envelope = self.event_execution_store.read_event_execution_records(
            EventExecutionRecordAuthorityQuery {
                consistency: request.consistency,
                event_execution_id: request.execution_id,
                limit: request.limit,
            },
        )?;
        Ok(shared_execution_records_from_event_envelope(
            envelope,
            request.family,
        ))
    }

    fn upsert_execution_record(
        &self,
        record: SharedExecutionRecord,
    ) -> Result<SharedExecutionRecordWriteResult> {
        let write = self
            .event_execution_store
            .upsert_event_execution_record(record.into_event_execution_record()?)?;
        Ok(SharedExecutionRecordWriteResult::from_event_execution_record_write_result(write))
    }

    fn apply_execution_transition(
        &self,
        request: SharedExecutionTransitionRequest,
    ) -> Result<SharedExecutionTransitionResult> {
        let result = self
            .event_execution_store
            .apply_event_execution_transition(request.into_event_execution_transition_request()?)?;
        Ok(SharedExecutionTransitionResult::from_event_execution_transition_result(result))
    }
}

pub fn open_shared_execution_substrate_store(
    root: &Path,
    provider: &CoordinationAuthorityStoreProvider,
) -> Result<Box<dyn SharedExecutionSubstrateStore>> {
    Ok(Box::new(AuthorityBackedSharedExecutionSubstrateStore::new(
        provider.open_event_execution(root)?,
    )))
}
