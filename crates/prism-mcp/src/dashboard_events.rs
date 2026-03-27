use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_stream::stream;
use axum::response::sse::Event;
use prism_js::{QueryLogEntryView, QueryPhaseView};
use serde_json::json;
use tokio::sync::broadcast;

use crate::dashboard_types::{
    ActiveOperationView, DashboardEventEnvelope, DashboardOperationDetailView,
    DashboardOperationsView, MutationLogEntryView, MutationTraceView,
};
use crate::{current_timestamp, QueryHost, QueryLogArgs, QueryRun};

const DASHBOARD_EVENT_CAPACITY: usize = 512;
const ACTIVE_OPERATION_LIMIT: usize = 256;
const MUTATION_LOG_CAPACITY: usize = 200;
const DEFAULT_DASHBOARD_OPERATION_LIMIT: usize = 20;

#[derive(Debug)]
pub(crate) struct DashboardState {
    next_event_id: AtomicU64,
    next_mutation_id: AtomicU64,
    replay: Mutex<VecDeque<DashboardEventEnvelope>>,
    sender: broadcast::Sender<DashboardEventEnvelope>,
    active_operations: Mutex<BTreeMap<String, ActiveOperationView>>,
    mutation_log: Mutex<VecDeque<MutationTraceRecord>>,
}

impl Default for DashboardState {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(DASHBOARD_EVENT_CAPACITY);
        Self {
            next_event_id: AtomicU64::new(1),
            next_mutation_id: AtomicU64::new(1),
            replay: Mutex::new(VecDeque::with_capacity(DASHBOARD_EVENT_CAPACITY)),
            sender,
            active_operations: Mutex::new(BTreeMap::new()),
            mutation_log: Mutex::new(VecDeque::with_capacity(MUTATION_LOG_CAPACITY)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MutationRun {
    dashboard: Arc<DashboardState>,
    id: String,
    action: String,
    started_at: u64,
    started: Instant,
    session_id: String,
    task_id: Option<String>,
}

#[derive(Debug, Clone)]
struct MutationTraceRecord {
    entry: MutationLogEntryView,
    result: serde_json::Value,
}

impl DashboardState {
    pub(crate) fn publish_value(&self, event: &str, data: serde_json::Value) {
        let id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        let envelope = DashboardEventEnvelope {
            id,
            event: event.to_string(),
            data,
        };
        {
            let mut replay = self.replay.lock().expect("dashboard replay lock poisoned");
            if replay.len() == DASHBOARD_EVENT_CAPACITY {
                replay.pop_front();
            }
            replay.push_back(envelope.clone());
        }
        let _ = self.sender.send(envelope);
    }

    fn upsert_active(&self, operation: ActiveOperationView) {
        let mut active = self
            .active_operations
            .lock()
            .expect("dashboard active lock poisoned");
        if active.len() >= ACTIVE_OPERATION_LIMIT && !active.contains_key(&operation.id) {
            if let Some(first) = active.keys().next().cloned() {
                active.remove(&first);
            }
        }
        active.insert(operation.id.clone(), operation);
    }

    fn remove_active(&self, id: &str) {
        self.active_operations
            .lock()
            .expect("dashboard active lock poisoned")
            .remove(id);
    }

    fn push_mutation(&self, record: MutationTraceRecord) {
        let mut log = self
            .mutation_log
            .lock()
            .expect("dashboard mutation log lock poisoned");
        if log.len() == MUTATION_LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(record);
    }

    pub(crate) fn active_operations(&self) -> Vec<ActiveOperationView> {
        let mut operations = self
            .active_operations
            .lock()
            .expect("dashboard active lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        operations.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        operations
    }

    pub(crate) fn mutation_entries(&self, limit: Option<usize>) -> Vec<MutationLogEntryView> {
        let limit = limit.unwrap_or(DEFAULT_DASHBOARD_OPERATION_LIMIT);
        let mut entries = self
            .mutation_log
            .lock()
            .expect("dashboard mutation log lock poisoned")
            .iter()
            .rev()
            .take(limit)
            .map(|record| record.entry.clone())
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        entries
    }

    pub(crate) fn mutation_detail(&self, id: &str) -> Option<MutationTraceView> {
        self.mutation_log
            .lock()
            .expect("dashboard mutation log lock poisoned")
            .iter()
            .find(|record| record.entry.id == id)
            .cloned()
            .map(|record| MutationTraceView {
                entry: record.entry,
                result: record.result,
            })
    }

    pub(crate) fn sse_stream(
        self: Arc<Self>,
        last_event_id: Option<u64>,
    ) -> impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>> {
        let replay = {
            let replay = self.replay.lock().expect("dashboard replay lock poisoned");
            replay
                .iter()
                .filter(|event| last_event_id.is_none_or(|last| event.id > last))
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut receiver = self.sender.subscribe();
        stream! {
            for event in replay {
                yield Ok(event_to_sse(&event));
            }
            loop {
                match receiver.recv().await {
                    Ok(event) => yield Ok(event_to_sse(&event)),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

impl QueryHost {
    pub(crate) fn begin_mutation_run(&self, action: &str) -> MutationRun {
        let sequence = self
            .dashboard_state
            .next_mutation_id
            .fetch_add(1, Ordering::Relaxed);
        let run = MutationRun {
            dashboard: Arc::clone(&self.dashboard_state),
            id: format!("mutation:{sequence}"),
            action: action.to_string(),
            started_at: current_timestamp(),
            started: Instant::now(),
            session_id: self.session.session_id().0.to_string(),
            task_id: self.session.current_task().map(|task| task.0.to_string()),
        };
        let active = ActiveOperationView {
            id: run.id.clone(),
            kind: "mutation".to_string(),
            label: run.action.clone(),
            started_at: run.started_at,
            session_id: run.session_id.clone(),
            task_id: run.task_id.clone(),
            status: "running".to_string(),
            phase: None,
            touched: Vec::new(),
            error: None,
        };
        self.dashboard_state.upsert_active(active.clone());
        self.dashboard_state
            .publish_value("mutation.started", json!(active));
        run
    }

    pub(crate) fn dashboard_operations_view(
        &self,
        limit: Option<usize>,
    ) -> DashboardOperationsView {
        DashboardOperationsView {
            active: self.dashboard_state.active_operations(),
            recent_queries: self.query_log_entries(QueryLogArgs {
                limit,
                since: None,
                target: None,
                operation: None,
                task_id: None,
                min_duration_ms: None,
            }),
            recent_mutations: self.dashboard_state.mutation_entries(limit),
        }
    }

    pub(crate) fn dashboard_operation_detail(
        &self,
        id: &str,
    ) -> Option<DashboardOperationDetailView> {
        if let Some(operation) = self
            .dashboard_state
            .active_operations()
            .into_iter()
            .find(|operation| operation.id == id)
        {
            return Some(DashboardOperationDetailView::Active { operation });
        }
        if id.starts_with("query:") {
            return self
                .query_trace_view(id)
                .map(|trace| DashboardOperationDetailView::Query { trace });
        }
        if id.starts_with("mutation:") {
            return self
                .dashboard_state
                .mutation_detail(id)
                .map(|trace| DashboardOperationDetailView::Mutation { trace });
        }
        None
    }

    pub(crate) fn dashboard_state(&self) -> Arc<DashboardState> {
        Arc::clone(&self.dashboard_state)
    }
}

impl MutationRun {
    pub(crate) fn finish_success(
        self,
        task_id: Option<String>,
        result_ids: Vec<String>,
        violation_count: usize,
        result: serde_json::Value,
    ) {
        let entry = MutationLogEntryView {
            id: self.id.clone(),
            action: self.action.clone(),
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_millis() as u64,
            session_id: self.session_id.clone(),
            task_id: task_id.or(self.task_id.clone()),
            success: true,
            error: None,
            result_ids,
            violation_count,
        };
        self.dashboard.remove_active(&self.id);
        self.dashboard.push_mutation(MutationTraceRecord {
            entry: entry.clone(),
            result,
        });
        self.dashboard
            .publish_value("mutation.finished", json!(entry));
    }

    pub(crate) fn finish_error(self, error: impl Into<String>) {
        let entry = MutationLogEntryView {
            id: self.id.clone(),
            action: self.action.clone(),
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_millis() as u64,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            success: false,
            error: Some(error.into()),
            result_ids: Vec::new(),
            violation_count: 0,
        };
        self.dashboard.remove_active(&self.id);
        self.dashboard.push_mutation(MutationTraceRecord {
            entry: entry.clone(),
            result: serde_json::Value::Null,
        });
        self.dashboard
            .publish_value("mutation.finished", json!(entry));
    }
}

impl QueryRun {
    pub(crate) fn dashboard_start(&self, dashboard: &DashboardState) {
        let active = ActiveOperationView {
            id: self.id.clone(),
            kind: "query".to_string(),
            label: self.query_summary.clone(),
            started_at: self.started_at,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            status: "running".to_string(),
            phase: None,
            touched: Vec::new(),
            error: None,
        };
        dashboard.upsert_active(active.clone());
        dashboard.publish_value("query.started", json!(active));
    }

    pub(crate) fn dashboard_phase(&self, dashboard: &DashboardState, phase: &QueryPhaseView) {
        let active = ActiveOperationView {
            id: self.id.clone(),
            kind: "query".to_string(),
            label: self.query_summary.clone(),
            started_at: self.started_at,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            status: "running".to_string(),
            phase: Some(phase.operation.clone()),
            touched: phase.touched.clone(),
            error: phase.error.clone(),
        };
        dashboard.upsert_active(active.clone());
        dashboard.publish_value("query.phase", json!(active));
    }

    pub(crate) fn dashboard_finish(&self, dashboard: &DashboardState, entry: &QueryLogEntryView) {
        dashboard.remove_active(&self.id);
        dashboard.publish_value("query.finished", json!(entry));
    }
}

fn event_to_sse(event: &DashboardEventEnvelope) -> Event {
    Event::default()
        .id(event.id.to_string())
        .event(event.event.clone())
        .json_data(&event.data)
        .unwrap_or_else(|_| {
            Event::default()
                .id(event.id.to_string())
                .event("dashboard.error")
        })
}
