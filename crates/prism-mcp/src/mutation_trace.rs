use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prism_js::QueryPhaseView;
use serde::Serialize;
use serde_json::{Value, json};

use crate::mcp_call_log::{
    McpCallLogStore, PersistedMcpCallRecord, duration_to_ms, new_log_entry, payload_summary,
    preview_value, summarize_value, touches_for_value, unique_operations, unique_touches,
};
use crate::{QueryHost, SessionState, current_timestamp};

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationLogEntryView {
    pub(crate) id: String,
    pub(crate) action: String,
    pub(crate) started_at: u64,
    pub(crate) duration_ms: u64,
    pub(crate) session_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) success: bool,
    pub(crate) error: Option<String>,
    pub(crate) result_ids: Vec<String>,
    pub(crate) violation_count: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationTraceView {
    pub(crate) entry: MutationLogEntryView,
    pub(crate) phases: Vec<QueryPhaseView>,
    pub(crate) result: Value,
}

#[derive(Clone)]
pub(crate) struct MutationRun {
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
    finalized: Arc<AtomicBool>,
}

impl QueryHost {
    pub(crate) fn begin_mutation_run(&self, session: &SessionState, action: &str) -> MutationRun {
        let id = self
            .next_mutation_trace_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string();
        MutationRun {
            mcp_call_log_store: Arc::clone(&self.mcp_call_log_store),
            workspace: self.workspace_session_arc(),
            id: format!("mutation:{id}"),
            tool_name: "prism_mutate".to_string(),
            action: action.to_string(),
            started_at: current_timestamp(),
            started: Instant::now(),
            session_id: session.session_id().0.to_string(),
            task_id: session
                .effective_current_task()
                .map(|task| task.0.to_string()),
            phases: Arc::new(Mutex::new(Vec::new())),
            finalized: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn mutation_trace_view(&self, id: &str) -> Option<MutationTraceView> {
        self.mcp_call_log_store
            .records()
            .into_iter()
            .find(|record| {
                record.entry.id == id
                    || record
                        .mutation_compat
                        .as_ref()
                        .is_some_and(|trace| trace.entry.id == id)
            })
            .and_then(|record| record.mutation_compat)
    }
}

impl MutationRun {
    pub(crate) fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub(crate) fn record_phase(
        &self,
        operation: &str,
        args: &Value,
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
            .expect("mutation trace phases lock poisoned")
            .push(phase);
    }

    pub(crate) fn finish_success(
        &self,
        task_id: Option<String>,
        result_ids: Vec<String>,
        violation_count: usize,
        result: Value,
    ) {
        if self.finalized.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut phases = self
            .phases
            .lock()
            .expect("mutation trace phases lock poisoned")
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
        let trace = MutationTraceView {
            entry: entry.clone(),
            phases: phases.clone(),
            result: result.clone(),
        };
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
            phases,
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: preview_value(&result),
            metadata,
            query_compat: None,
            mutation_compat: Some(trace),
        };
        let _ = self.mcp_call_log_store.push(record);
    }

    pub(crate) fn finish_error(&self, error: impl Into<String>) {
        if self.finalized.swap(true, Ordering::SeqCst) {
            return;
        }
        let error = error.into();
        let mut phases = self
            .phases
            .lock()
            .expect("mutation trace phases lock poisoned")
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
        let trace = MutationTraceView {
            entry: entry.clone(),
            phases: phases.clone(),
            result: Value::Null,
        };
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
            phases,
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: None,
            metadata,
            query_compat: None,
            mutation_compat: Some(trace),
        };
        let _ = self.mcp_call_log_store.push(record);
    }
}

impl Drop for MutationRun {
    fn drop(&mut self) {
        if Arc::strong_count(&self.phases) != 1 {
            return;
        }
        if self.finalized.swap(true, Ordering::SeqCst) {
            return;
        }

        let mut phases = self
            .phases
            .lock()
            .expect("mutation trace phases lock poisoned")
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
        let trace = MutationTraceView {
            entry: entry.clone(),
            phases: phases.clone(),
            result: Value::Null,
        };
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
            phases,
            request_payload: Some(request_payload.clone()),
            request_preview: preview_value(&request_payload),
            response_preview: None,
            metadata,
            query_compat: None,
            mutation_compat: Some(trace),
        };
        let _ = self.mcp_call_log_store.push(record);
    }
}
