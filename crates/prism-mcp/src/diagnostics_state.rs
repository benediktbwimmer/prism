use std::collections::VecDeque;
use std::sync::Mutex;

use prism_js::{QueryLogEntryView, RuntimeLogEventView, RuntimeStatusView};

const RECENT_QUERY_CAPACITY: usize = 128;

#[derive(Debug, Default)]
pub(crate) struct DiagnosticsState {
    runtime_status: Mutex<Option<RuntimeStatusView>>,
    last_runtime_event: Mutex<Option<RuntimeLogEventView>>,
    recent_queries: Mutex<VecDeque<QueryLogEntryView>>,
}

impl DiagnosticsState {
    pub(crate) fn update_runtime_status(
        &self,
        runtime_status: RuntimeStatusView,
        last_runtime_event: Option<RuntimeLogEventView>,
    ) {
        *self
            .runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned") = Some(runtime_status);
        *self
            .last_runtime_event
            .lock()
            .expect("diagnostics last runtime event lock poisoned") = last_runtime_event;
    }

    pub(crate) fn runtime_status(&self) -> Option<RuntimeStatusView> {
        self.runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned")
            .clone()
    }

    pub(crate) fn last_runtime_event(&self) -> Option<RuntimeLogEventView> {
        self.last_runtime_event
            .lock()
            .expect("diagnostics last runtime event lock poisoned")
            .clone()
    }

    pub(crate) fn push_recent_query(&self, entry: QueryLogEntryView) {
        let mut recent = self
            .recent_queries
            .lock()
            .expect("diagnostics recent query lock poisoned");
        if recent.len() == RECENT_QUERY_CAPACITY {
            recent.pop_front();
        }
        recent.push_back(entry);
    }

    pub(crate) fn recent_queries(&self, limit: Option<usize>) -> Vec<QueryLogEntryView> {
        let limit = limit.unwrap_or(RECENT_QUERY_CAPACITY);
        let mut entries = self
            .recent_queries
            .lock()
            .expect("diagnostics recent query lock poisoned")
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        entries
    }

    pub(crate) fn recent_query_error_count(&self, limit: Option<usize>) -> usize {
        self.recent_queries(limit)
            .into_iter()
            .filter(|entry| !entry.success)
            .count()
    }
}
