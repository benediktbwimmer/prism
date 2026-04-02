use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_stream::stream;
use axum::response::sse::Event;
use prism_js::{QueryLogEntryView, QueryPhaseView};
use serde_json::json;
use tokio::sync::broadcast;

use crate::dashboard_types::{
    ActiveOperationView, DashboardEventEnvelope, DashboardOperationDetailView,
    DashboardOperationsView, MutationLogEntryView, MutationTraceView,
};
use crate::{
    current_timestamp,
    mcp_call_log::{
        duration_to_ms, new_log_entry, payload_summary, preview_value, summarize_value,
        touches_for_value, unique_operations, unique_touches, McpCallLogStore,
        PersistedMcpCallRecord,
    },
    QueryHost, QueryRun, SessionState,
};

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

#[derive(Clone)]
pub(crate) struct MutationRun {
    dashboard: Arc<DashboardState>,
    mcp_call_log_store: Arc<McpCallLogStore>,
    workspace: Option<Arc<prism_core::WorkspaceSession>>,
    id: String,
    tool_name: String,
    action: String,
    started_at: u64,
    started: Instant,
    session_id: String,
    task_id: Option<String>,
    phases: Arc<Mutex<Vec<QueryPhaseView>>>,
    finalized: bool,
}

#[derive(Debug, Clone)]
struct MutationTraceRecord {
    entry: MutationLogEntryView,
    phases: Vec<QueryPhaseView>,
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
                phases: record.phases,
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
    pub(crate) fn begin_mutation_run(&self, session: &SessionState, action: &str) -> MutationRun {
        let run = MutationRun {
            dashboard: Arc::clone(&self.dashboard_state),
            mcp_call_log_store: Arc::clone(&self.mcp_call_log_store),
            workspace: self.workspace_session_arc(),
            id: format!(
                "mutation:{}",
                self.dashboard_state
                    .next_mutation_id
                    .fetch_add(1, Ordering::Relaxed)
            ),
            tool_name: tool_name_for_action(action).to_string(),
            action: action.to_string(),
            started_at: current_timestamp(),
            started: Instant::now(),
            session_id: session.session_id().0.to_string(),
            task_id: session.current_task().map(|task| task.0.to_string()),
            phases: Arc::new(Mutex::new(Vec::new())),
            finalized: false,
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
            recent_queries: self.diagnostics_state.recent_queries(limit),
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
    pub(crate) fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub(crate) fn record_phase(
        &self,
        operation: &str,
        args: &serde_json::Value,
        duration: Duration,
        success: bool,
        error: Option<String>,
    ) {
        let phase = QueryPhaseView {
            operation: operation.to_string(),
            started_at: current_timestamp(),
            duration_ms: duration_to_ms(duration),
            args_summary: Some(summarize_value(args)),
            touched: touches_for_value(args),
            success,
            error,
        };
        self.phases
            .lock()
            .expect("mutation log phases lock poisoned")
            .push(phase.clone());
        self.dashboard_phase(&phase);
    }

    pub(crate) fn finish_success(
        mut self,
        task_id: Option<String>,
        result_ids: Vec<String>,
        violation_count: usize,
        result: serde_json::Value,
    ) {
        self.finalized = true;
        let mut phases = self
            .phases
            .lock()
            .expect("mutation log phases lock poisoned")
            .clone();
        let mut started_at = self.started_at;
        let mut duration_ms = self.started.elapsed().as_millis() as u64;
        let mut metadata = json!({
            "tool": self.tool_name,
            "action": self.action,
        });
        crate::request_envelope::apply_current_request_envelope(
            &mut phases,
            &mut started_at,
            &mut duration_ms,
            &mut metadata,
        );
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.dashboard.as_ref(),
            self.workspace.as_deref(),
        );
        let entry = MutationLogEntryView {
            id: self.id.clone(),
            action: self.action.clone(),
            started_at,
            duration_ms,
            session_id: self.session_id.clone(),
            task_id: task_id.or(self.task_id.clone()),
            success: true,
            error: None,
            result_ids,
            violation_count,
        };
        let request_payload = crate::request_envelope::current_specialized_request_payload()
            .unwrap_or_else(|| {
                json!({
                    "tool": self.tool_name,
                    "action": self.action,
                })
            });
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                self.mcp_call_log_store.runtime(),
                "tool",
                &self.tool_name,
                None,
                self.action.clone(),
                started_at,
                entry.duration_ms,
                Some(self.session_id.clone()),
                entry.task_id.clone(),
                true,
                None,
                unique_operations(&phases),
                unique_touches(&phases),
                Vec::new(),
                payload_summary(Some(&request_payload)),
                payload_summary(Some(&result)),
            ),
            phases: phases.clone(),
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: preview_value(&result),
            metadata: {
                metadata["resultIds"] = json!(entry.result_ids.clone());
                metadata["violationCount"] = json!(entry.violation_count);
                metadata
            },
            query_compat: None,
        };
        let _ = self.mcp_call_log_store.push(record);
        self.dashboard.remove_active(&self.id);
        self.dashboard.push_mutation(MutationTraceRecord {
            entry: entry.clone(),
            phases,
            result,
        });
        self.dashboard
            .publish_value("mutation.finished", json!(entry));
    }

    pub(crate) fn finish_error(mut self, error: impl Into<String>) {
        self.finalized = true;
        let mut phases = self
            .phases
            .lock()
            .expect("mutation log phases lock poisoned")
            .clone();
        let mut started_at = self.started_at;
        let mut duration_ms = self.started.elapsed().as_millis() as u64;
        let mut metadata = json!({
            "tool": self.tool_name,
            "action": self.action,
        });
        crate::request_envelope::apply_current_request_envelope(
            &mut phases,
            &mut started_at,
            &mut duration_ms,
            &mut metadata,
        );
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.dashboard.as_ref(),
            self.workspace.as_deref(),
        );
        let entry = MutationLogEntryView {
            id: self.id.clone(),
            action: self.action.clone(),
            started_at,
            duration_ms,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            success: false,
            error: Some(error.into()),
            result_ids: Vec::new(),
            violation_count: 0,
        };
        let request_payload = crate::request_envelope::current_specialized_request_payload()
            .unwrap_or_else(|| {
                json!({
                    "tool": self.tool_name,
                    "action": self.action,
                })
            });
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                self.mcp_call_log_store.runtime(),
                "tool",
                &self.tool_name,
                None,
                self.action.clone(),
                started_at,
                entry.duration_ms,
                Some(self.session_id.clone()),
                entry.task_id.clone(),
                false,
                entry.error.clone(),
                unique_operations(&phases),
                unique_touches(&phases),
                Vec::new(),
                payload_summary(Some(&request_payload)),
                payload_summary(None),
            ),
            phases: phases.clone(),
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: None,
            metadata,
            query_compat: None,
        };
        let _ = self.mcp_call_log_store.push(record);
        self.dashboard.remove_active(&self.id);
        self.dashboard.push_mutation(MutationTraceRecord {
            entry: entry.clone(),
            phases,
            result: serde_json::Value::Null,
        });
        self.dashboard
            .publish_value("mutation.finished", json!(entry));
    }

    fn dashboard_phase(&self, phase: &QueryPhaseView) {
        let active = ActiveOperationView {
            id: self.id.clone(),
            kind: "mutation".to_string(),
            label: self.action.clone(),
            started_at: self.started_at,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            status: "running".to_string(),
            phase: Some(phase.operation.clone()),
            touched: phase.touched.clone(),
            error: phase.error.clone(),
        };
        self.dashboard.upsert_active(active.clone());
        self.dashboard
            .publish_value("mutation.phase", json!(active));
    }
}

impl Drop for MutationRun {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }

        let mut phases = self
            .phases
            .lock()
            .expect("mutation log phases lock poisoned")
            .clone();
        let mut started_at = self.started_at;
        let mut duration_ms = self.started.elapsed().as_millis() as u64;
        let error = "request dropped before mutation completed".to_string();
        let mut metadata = json!({
            "tool": self.tool_name,
            "action": self.action,
            "lifecycle": {
                "state": "dropped",
                "finalized": false,
            },
        });
        crate::request_envelope::apply_current_request_envelope(
            &mut phases,
            &mut started_at,
            &mut duration_ms,
            &mut metadata,
        );
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.dashboard.as_ref(),
            self.workspace.as_deref(),
        );
        let entry = MutationLogEntryView {
            id: self.id.clone(),
            action: self.action.clone(),
            started_at,
            duration_ms,
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            success: false,
            error: Some(error.clone()),
            result_ids: Vec::new(),
            violation_count: 0,
        };
        let request_payload = crate::request_envelope::current_specialized_request_payload()
            .unwrap_or_else(|| {
                json!({
                    "tool": self.tool_name,
                    "action": self.action,
                })
            });
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                self.mcp_call_log_store.runtime(),
                "tool",
                &self.tool_name,
                None,
                self.action.clone(),
                started_at,
                entry.duration_ms,
                Some(self.session_id.clone()),
                entry.task_id.clone(),
                false,
                entry.error.clone(),
                unique_operations(&phases),
                unique_touches(&phases),
                Vec::new(),
                payload_summary(Some(&request_payload)),
                payload_summary(None),
            ),
            phases: phases.clone(),
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: None,
            metadata,
            query_compat: None,
        };
        let _ = self.mcp_call_log_store.push(record);
        self.dashboard.remove_active(&self.id);
        self.dashboard.push_mutation(MutationTraceRecord {
            entry: entry.clone(),
            phases,
            result: serde_json::Value::Null,
        });
        self.dashboard
            .publish_value("mutation.finished", json!(entry));
    }
}

fn tool_name_for_action(action: &str) -> &str {
    let _ = action;
    "prism_mutate"
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
