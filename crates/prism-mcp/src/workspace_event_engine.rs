use std::sync::Arc;

use anyhow::Result;
use prism_coordination::EventExecutionRecord;
use prism_core::{
    CoordinationAuthorityStoreProvider, EventExecutionRecordAuthorityQuery,
    EventExecutionRecordWriteResult,
};

use crate::host_mutations::WorkspaceMutationBroker;
use crate::read_broker::WorkspaceReadBroker;

#[derive(Clone)]
pub(crate) struct WorkspaceEventEngine {
    workspace_root: std::path::PathBuf,
    authority_store_provider: CoordinationAuthorityStoreProvider,
    _read_broker: Arc<WorkspaceReadBroker>,
    _mutation_broker: Arc<WorkspaceMutationBroker>,
}

impl WorkspaceEventEngine {
    pub(crate) fn new(
        workspace_root: std::path::PathBuf,
        authority_store_provider: CoordinationAuthorityStoreProvider,
        read_broker: Arc<WorkspaceReadBroker>,
        mutation_broker: Arc<WorkspaceMutationBroker>,
    ) -> Self {
        Self {
            workspace_root,
            authority_store_provider,
            _read_broker: read_broker,
            _mutation_broker: mutation_broker,
        }
    }

    pub(crate) fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<Vec<EventExecutionRecord>> {
        Ok(self
            .authority_store_provider
            .open(&self.workspace_root)?
            .read_event_execution_records(request)?
            .value
            .unwrap_or_default())
    }

    pub(crate) fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult> {
        self.authority_store_provider
            .open(&self.workspace_root)?
            .upsert_event_execution_record(record)
    }
}
