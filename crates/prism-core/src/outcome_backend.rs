use std::sync::{Arc, Mutex};

use anyhow::Result;
use prism_ir::{EventId, TaskId};
use prism_memory::{OutcomeEvent, OutcomeRecallQuery, TaskReplay};
use prism_query::OutcomeReadBackend;
use prism_store::{ColdQueryStore, SqliteStore};

pub(crate) struct StoreOutcomeReadBackend {
    local_store: Arc<Mutex<SqliteStore>>,
}

impl StoreOutcomeReadBackend {
    pub(crate) fn new(local_store: Arc<Mutex<SqliteStore>>) -> Self {
        Self { local_store }
    }
}

impl OutcomeReadBackend for StoreOutcomeReadBackend {
    fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        if query.anchors.is_empty() && query.task.is_some() {
            return self.load_projected_task_outcomes(query);
        }
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcomes(query)
    }

    fn load_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcome_event(event_id)
    }

    fn load_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_task_replay(task_id)
    }
}

impl StoreOutcomeReadBackend {
    fn load_projected_task_outcomes(
        &self,
        query: &OutcomeRecallQuery,
    ) -> Result<Vec<OutcomeEvent>> {
        let local_ids = self
            .local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_projection_outcome_event_ids(query)?;
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcome_events_by_ids(&local_ids)
    }
}
