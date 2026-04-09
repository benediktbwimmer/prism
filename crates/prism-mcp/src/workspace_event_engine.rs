use std::sync::Arc;

use crate::host_mutations::WorkspaceMutationBroker;
use crate::read_broker::WorkspaceReadBroker;

#[derive(Clone)]
pub(crate) struct WorkspaceEventEngine {
    _read_broker: Arc<WorkspaceReadBroker>,
    _mutation_broker: Arc<WorkspaceMutationBroker>,
}

impl WorkspaceEventEngine {
    pub(crate) fn new(
        read_broker: Arc<WorkspaceReadBroker>,
        mutation_broker: Arc<WorkspaceMutationBroker>,
    ) -> Self {
        Self {
            _read_broker: read_broker,
            _mutation_broker: mutation_broker,
        }
    }
}
