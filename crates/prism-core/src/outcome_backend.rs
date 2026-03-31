use std::sync::{Arc, Mutex};

use anyhow::Result;
use prism_ir::{EventId, TaskId};
use prism_memory::{OutcomeEvent, OutcomeRecallQuery, TaskReplay};
use prism_query::OutcomeReadBackend;
use prism_store::SqliteStore;

pub(crate) struct StoreOutcomeReadBackend {
    store: Arc<Mutex<SqliteStore>>,
}

impl StoreOutcomeReadBackend {
    pub(crate) fn new(store: Arc<Mutex<SqliteStore>>) -> Self {
        Self { store }
    }
}

impl OutcomeReadBackend for StoreOutcomeReadBackend {
    fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcomes(query)
    }

    fn load_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcome_event(event_id)
    }

    fn load_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        self.store
            .lock()
            .expect("workspace store lock poisoned")
            .load_task_replay(task_id)
    }
}
