use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use prism_ir::{EventId, TaskId};
use prism_memory::{OutcomeEvent, OutcomeRecallQuery, TaskReplay};
use prism_query::OutcomeReadBackend;
use prism_store::{ColdQueryStore, SqliteStore};

use crate::shared_runtime_store::SharedRuntimeStore;

pub(crate) struct StoreOutcomeReadBackend {
    local_store: Arc<Mutex<SqliteStore>>,
    shared_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
}

impl StoreOutcomeReadBackend {
    pub(crate) fn new(
        local_store: Arc<Mutex<SqliteStore>>,
        shared_store: Option<Arc<Mutex<SharedRuntimeStore>>>,
    ) -> Self {
        Self {
            local_store,
            shared_store,
        }
    }
}

impl OutcomeReadBackend for StoreOutcomeReadBackend {
    fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        if query.anchors.is_empty() && query.task.is_some() {
            return self.load_projected_task_outcomes(query);
        }

        let Some(shared_store) = &self.shared_store else {
            return self
                .local_store
                .lock()
                .expect("workspace store lock poisoned")
                .load_outcomes(query);
        };

        if query.anchors.is_empty() {
            let mut events = shared_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_outcomes(query)?;
            let local = self
                .local_store
                .lock()
                .expect("workspace store lock poisoned")
                .load_outcomes(query)?;
            merge_events(&mut events, local, query.limit);
            return Ok(events);
        }

        let local_ids = self
            .local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_projection_outcome_event_ids(query)?;
        let mut events = shared_store
            .lock()
            .expect("shared runtime store lock poisoned")
            .load_outcome_events_by_ids(&local_ids)?;
        let mut seen = events
            .iter()
            .map(|event| event.meta.id.clone())
            .collect::<HashSet<_>>();
        if query.limit == 0 || events.len() < query.limit {
            let fallback = shared_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_outcomes_by_payload_scan(query, &seen)?;
            for event in fallback {
                if seen.insert(event.meta.id.clone()) {
                    events.push(event);
                }
            }
        }
        let local = self
            .local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcomes(query)?;
        merge_events(&mut events, local, query.limit);
        Ok(events)
    }

    fn load_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        if let Some(shared_store) = &self.shared_store {
            let shared = shared_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_outcome_event(event_id)?;
            if shared.is_some() {
                return Ok(shared);
            }
        }
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_outcome_event(event_id)
    }

    fn load_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay> {
        if let Some(shared_store) = &self.shared_store {
            let mut replay = shared_store
                .lock()
                .expect("shared runtime store lock poisoned")
                .load_task_replay(task_id)?;
            let local = self
                .local_store
                .lock()
                .expect("workspace store lock poisoned")
                .load_task_replay(task_id)?;
            merge_events(&mut replay.events, local.events, 0);
            return Ok(replay);
        }
        self.local_store
            .lock()
            .expect("workspace store lock poisoned")
            .load_task_replay(task_id)
    }
}

impl StoreOutcomeReadBackend {
    fn load_projected_task_outcomes(&self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        let local = {
            let local_store = self
                .local_store
                .lock()
                .expect("workspace store lock poisoned");
            let local_ids = local_store.load_projection_outcome_event_ids(query)?;
            local_store.load_outcome_events_by_ids(&local_ids)?
        };

        let Some(shared_store) = &self.shared_store else {
            return Ok(local);
        };

        let mut shared = {
            let mut shared_store = shared_store
                .lock()
                .expect("shared runtime store lock poisoned");
            let shared_ids = shared_store.load_projection_outcome_event_ids(query)?;
            shared_store.load_outcome_events_by_ids(&shared_ids)?
        };
        merge_events(&mut shared, local, query.limit);
        Ok(shared)
    }
}

fn sort_and_limit_events(events: &mut Vec<OutcomeEvent>, limit: usize) {
    events.sort_by(|left, right| {
        right
            .meta
            .ts
            .cmp(&left.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });
    if limit > 0 {
        events.truncate(limit);
    }
}

fn merge_events(existing: &mut Vec<OutcomeEvent>, incoming: Vec<OutcomeEvent>, limit: usize) {
    let mut seen = existing
        .iter()
        .map(|event| event.meta.id.clone())
        .collect::<HashSet<_>>();
    for event in incoming {
        if seen.insert(event.meta.id.clone()) {
            existing.push(event);
        }
    }
    sort_and_limit_events(existing, limit);
}
