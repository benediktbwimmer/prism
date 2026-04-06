use std::collections::VecDeque;
use std::sync::Mutex;

use prism_js::{
    QueryLogEntryView, RuntimeLogEventView, RuntimeSharedCoordinationRefView, RuntimeStatusView,
};

const RECENT_QUERY_CAPACITY: usize = 128;

#[derive(Debug, Default)]
pub(crate) struct DiagnosticsState {
    runtime_status: Mutex<Option<CachedRuntimeStatus>>,
    last_runtime_event: Mutex<Option<RuntimeLogEventView>>,
    shared_coordination_ref: Mutex<Option<CachedSharedCoordinationRef>>,
    recent_queries: Mutex<VecDeque<QueryLogEntryView>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeStatusRevisionKey {
    pub(crate) workspace_revision: u64,
    pub(crate) episodic_revision: u64,
    pub(crate) inference_revision: u64,
    pub(crate) coordination_revision: u64,
}

#[derive(Debug, Clone)]
struct CachedRuntimeStatus {
    revisions: RuntimeStatusRevisionKey,
    value: RuntimeStatusView,
}

#[derive(Debug, Clone)]
struct CachedSharedCoordinationRef {
    coordination_revision: u64,
    value: Option<RuntimeSharedCoordinationRefView>,
}

impl DiagnosticsState {
    pub(crate) fn update_runtime_status(
        &self,
        runtime_status: RuntimeStatusView,
        last_runtime_event: Option<RuntimeLogEventView>,
        revisions: RuntimeStatusRevisionKey,
    ) {
        *self
            .shared_coordination_ref
            .lock()
            .expect("diagnostics shared coordination ref lock poisoned") =
            Some(CachedSharedCoordinationRef {
                coordination_revision: revisions.coordination_revision,
                value: runtime_status.shared_coordination_ref.clone(),
            });
        *self
            .runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned") = Some(CachedRuntimeStatus {
            revisions,
            value: runtime_status,
        });
        if let Some(last_runtime_event) = last_runtime_event {
            *self
                .last_runtime_event
                .lock()
                .expect("diagnostics last runtime event lock poisoned") = Some(last_runtime_event);
        }
    }

    pub(crate) fn runtime_status(&self) -> Option<RuntimeStatusView> {
        self.runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned")
            .as_ref()
            .map(|cached| cached.value.clone())
    }

    pub(crate) fn runtime_status_for_revisions(
        &self,
        revisions: RuntimeStatusRevisionKey,
    ) -> Option<RuntimeStatusView> {
        self.runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned")
            .as_ref()
            .filter(|cached| cached.revisions == revisions)
            .map(|cached| cached.value.clone())
    }

    pub(crate) fn invalidate_runtime_status(&self) {
        *self
            .runtime_status
            .lock()
            .expect("diagnostics runtime status lock poisoned") = None;
        *self
            .shared_coordination_ref
            .lock()
            .expect("diagnostics shared coordination ref lock poisoned") = None;
    }

    pub(crate) fn shared_coordination_ref_for_revision(
        &self,
        coordination_revision: u64,
    ) -> Option<Option<RuntimeSharedCoordinationRefView>> {
        self.shared_coordination_ref
            .lock()
            .expect("diagnostics shared coordination ref lock poisoned")
            .as_ref()
            .filter(|cached| cached.coordination_revision == coordination_revision)
            .map(|cached| cached.value.clone())
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
