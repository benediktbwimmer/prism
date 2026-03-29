use std::time::Instant;

use prism_js::{QueryDiagnostic, QueryLogEntryView, QueryPhaseView, QueryTraceView};
use serde_json::{json, Value};
use tracing::info;

use crate::mcp_call_log::{
    new_log_entry, payload_summary, preview_value, query_result_summary, sanitize_query_text,
    summarize_query, unique_operations, unique_touches, McpCallLogStore, PersistedMcpCallRecord,
};
use crate::{current_timestamp, DashboardState, QueryHost, QueryLogArgs, SessionState};

const DEFAULT_QUERY_LOG_LIMIT: usize = 20;
const DEFAULT_SLOW_QUERY_LIMIT: usize = 20;
const DEFAULT_SLOW_QUERY_MIN_DURATION_MS: u64 = 100;
const COMPACT_QUERY_KINDS: &[&str] = &[
    "prism_locate",
    "prism_gather",
    "prism_open",
    "prism_workset",
    "prism_expand",
    "prism_task_brief",
];

#[derive(Debug, Clone)]
pub(crate) struct QueryRun {
    pub(crate) id: String,
    pub(crate) tool_name: String,
    pub(crate) kind: String,
    pub(crate) query_text: String,
    pub(crate) query_summary: String,
    pub(crate) started_at: u64,
    pub(crate) started: Instant,
    pub(crate) session_id: String,
    pub(crate) task_id: Option<String>,
    dashboard: std::sync::Arc<DashboardState>,
    phases: std::sync::Arc<std::sync::Mutex<Vec<QueryPhaseView>>>,
}

#[derive(Debug, Clone)]
struct QueryLogFilter {
    limit: usize,
    since: Option<u64>,
    target: Option<String>,
    operation: Option<String>,
    task_id: Option<String>,
    min_duration_ms: Option<u64>,
}

impl QueryHost {
    pub(crate) fn begin_query_run(
        &self,
        session: &SessionState,
        tool_name: &str,
        kind: &str,
        query_text: impl Into<String>,
    ) -> QueryRun {
        let query_text = sanitize_query_text(&query_text.into());
        let run = QueryRun {
            id: prism_ir::new_prefixed_id("query").to_string(),
            tool_name: tool_name.to_string(),
            kind: kind.to_string(),
            query_summary: summarize_query(&query_text, kind),
            query_text,
            started_at: current_timestamp(),
            started: Instant::now(),
            session_id: session.session_id().0.to_string(),
            task_id: session.current_task().map(|task| task.0.to_string()),
            dashboard: std::sync::Arc::clone(&self.dashboard_state),
            phases: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        run.dashboard_start(self.dashboard_state.as_ref());
        run
    }

    pub(crate) fn query_log_entries(&self, args: QueryLogArgs) -> Vec<QueryLogEntryView> {
        let filter = QueryLogFilter::from_args(args, DEFAULT_QUERY_LOG_LIMIT);
        let mut matches = self
            .mcp_call_log_store
            .records()
            .into_iter()
            .filter(|record| filter.matches(record))
            .filter_map(|record| record.query_compat.map(|trace| trace.entry))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        matches.truncate(filter.limit);
        matches
    }

    pub(crate) fn slow_query_entries(&self, args: QueryLogArgs) -> Vec<QueryLogEntryView> {
        let filter = QueryLogFilter::from_args(args, DEFAULT_SLOW_QUERY_LIMIT).for_slow_queries();
        let mut matches = self
            .mcp_call_log_store
            .records()
            .into_iter()
            .filter(|record| filter.matches(record))
            .filter_map(|record| record.query_compat.map(|trace| trace.entry))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .duration_ms
                .cmp(&left.duration_ms)
                .then_with(|| right.started_at.cmp(&left.started_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        matches.truncate(filter.limit);
        matches
    }

    pub(crate) fn query_trace_view(&self, id: &str) -> Option<QueryTraceView> {
        self.mcp_call_log_store
            .records()
            .into_iter()
            .find(|record| record.entry.id == id)
            .and_then(|record| record.query_compat)
    }
}

impl QueryRun {
    pub(crate) fn record_phase(
        &self,
        operation: &str,
        args: &Value,
        duration: std::time::Duration,
        success: bool,
        error: Option<String>,
    ) {
        let phase = QueryPhaseView {
            operation: operation.to_string(),
            started_at: current_timestamp(),
            duration_ms: crate::mcp_call_log::duration_to_ms(duration),
            args_summary: Some(crate::mcp_call_log::summarize_value(args)),
            touched: crate::mcp_call_log::touches_for_value(args),
            success,
            error,
        };
        self.phases
            .lock()
            .expect("query log phases lock poisoned")
            .push(phase.clone());
        self.dashboard_phase(self.dashboard.as_ref(), &phase);
    }

    pub(crate) fn finish_success(
        &self,
        store: &McpCallLogStore,
        result: &Value,
        diagnostics: Vec<QueryDiagnostic>,
        json_bytes: usize,
        output_cap_hit: bool,
    ) {
        let phases = self
            .phases
            .lock()
            .expect("query log phases lock poisoned")
            .clone();
        let query_entry = QueryLogEntryView {
            id: self.id.clone(),
            kind: self.kind.clone(),
            query_summary: self.query_summary.clone(),
            query_text: self.query_text.clone(),
            started_at: self.started_at,
            duration_ms: crate::mcp_call_log::duration_to_ms(self.started.elapsed()),
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            success: true,
            error: None,
            operations: unique_operations(&phases),
            touched: unique_touches(&phases),
            diagnostics: diagnostics.clone(),
            result: query_result_summary(Some(result), json_bytes, output_cap_hit, &diagnostics),
        };
        let request_value = json!({
            "tool": self.tool_name,
            "queryKind": self.kind,
            "queryText": self.query_text,
        });
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                store.runtime(),
                "tool",
                &self.tool_name,
                self.query_summary.clone(),
                self.started_at,
                query_entry.duration_ms,
                Some(self.session_id.clone()),
                self.task_id.clone(),
                true,
                None,
                query_entry.operations.clone(),
                query_entry.touched.clone(),
                diagnostics,
                payload_summary(Some(&request_value)),
                payload_summary(Some(result)),
            ),
            phases: phases.clone(),
            request_preview: preview_value(&request_value),
            response_preview: preview_value(result),
            metadata: json!({
                "tool": self.tool_name,
                "queryKind": self.kind,
                "queryText": self.query_text,
            }),
            query_compat: Some(QueryTraceView {
                entry: query_entry.clone(),
                phases: phases.clone(),
            }),
        };
        let _ = store.push(record);
        emit_compact_query_timing(&query_entry, &phases);
        self.dashboard_finish(self.dashboard.as_ref(), &query_entry);
    }

    pub(crate) fn finish_error(
        &self,
        store: &McpCallLogStore,
        diagnostics: Vec<QueryDiagnostic>,
        error: impl Into<String>,
    ) {
        let error = error.into();
        let phases = self
            .phases
            .lock()
            .expect("query log phases lock poisoned")
            .clone();
        let query_entry = QueryLogEntryView {
            id: self.id.clone(),
            kind: self.kind.clone(),
            query_summary: self.query_summary.clone(),
            query_text: self.query_text.clone(),
            started_at: self.started_at,
            duration_ms: crate::mcp_call_log::duration_to_ms(self.started.elapsed()),
            session_id: self.session_id.clone(),
            task_id: self.task_id.clone(),
            success: false,
            error: Some(error.clone()),
            operations: unique_operations(&phases),
            touched: unique_touches(&phases),
            diagnostics: diagnostics.clone(),
            result: query_result_summary(None, 0, false, &diagnostics),
        };
        let request_value = json!({
            "tool": self.tool_name,
            "queryKind": self.kind,
            "queryText": self.query_text,
        });
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                store.runtime(),
                "tool",
                &self.tool_name,
                self.query_summary.clone(),
                self.started_at,
                query_entry.duration_ms,
                Some(self.session_id.clone()),
                self.task_id.clone(),
                false,
                Some(error.clone()),
                query_entry.operations.clone(),
                query_entry.touched.clone(),
                diagnostics,
                payload_summary(Some(&request_value)),
                payload_summary(None),
            ),
            phases: phases.clone(),
            request_preview: preview_value(&request_value),
            response_preview: None,
            metadata: json!({
                "tool": self.tool_name,
                "queryKind": self.kind,
                "queryText": self.query_text,
            }),
            query_compat: Some(QueryTraceView {
                entry: query_entry.clone(),
                phases: phases.clone(),
            }),
        };
        let _ = store.push(record);
        emit_compact_query_timing(&query_entry, &phases);
        self.dashboard_finish(self.dashboard.as_ref(), &query_entry);
    }
}

impl QueryLogFilter {
    fn from_args(args: QueryLogArgs, default_limit: usize) -> Self {
        Self {
            limit: args.limit.unwrap_or(default_limit).min(500),
            since: args.since,
            target: args.target,
            operation: args.operation,
            task_id: args.task_id,
            min_duration_ms: args.min_duration_ms,
        }
    }

    fn for_slow_queries(mut self) -> Self {
        if self.min_duration_ms.is_none() {
            self.min_duration_ms = Some(DEFAULT_SLOW_QUERY_MIN_DURATION_MS);
        }
        self
    }

    fn matches(&self, record: &PersistedMcpCallRecord) -> bool {
        let Some(trace) = record.query_compat.as_ref() else {
            return false;
        };
        if let Some(since) = self.since {
            if trace.entry.started_at < since {
                return false;
            }
        }
        if let Some(min_duration_ms) = self.min_duration_ms {
            if trace.entry.duration_ms < min_duration_ms {
                return false;
            }
        }
        if let Some(task_id) = &self.task_id {
            if trace.entry.task_id.as_deref() != Some(task_id.as_str()) {
                return false;
            }
        }
        if let Some(operation) = &self.operation {
            if !trace
                .entry
                .operations
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(operation))
            {
                return false;
            }
        }
        if let Some(target) = &self.target {
            if !matches_target(&trace.entry, target) {
                return false;
            }
        }
        true
    }
}

fn emit_compact_query_timing(entry: &QueryLogEntryView, phases: &[QueryPhaseView]) {
    if !COMPACT_QUERY_KINDS
        .iter()
        .any(|kind| kind == &entry.kind.as_str())
    {
        return;
    }
    let refresh_ms = phase_duration_ms(phases, "compact.refreshWorkspace");
    let handler_ms = phase_duration_ms(phases, "compact.handler");
    let other_ms = entry
        .duration_ms
        .saturating_sub(refresh_ms.saturating_add(handler_ms));
    info!(
        target: "prism_mcp::benchmark_telemetry",
        query_id = %entry.id,
        tool = %entry.kind,
        success = entry.success,
        total_ms = entry.duration_ms,
        refresh_ms,
        handler_ms,
        other_ms,
        session_id = %entry.session_id,
        task_id = %entry.task_id.as_deref().unwrap_or_default(),
        "compact query timing"
    );
}

fn phase_duration_ms(phases: &[QueryPhaseView], operation: &str) -> u64 {
    phases
        .iter()
        .find(|phase| phase.operation == operation)
        .map(|phase| phase.duration_ms)
        .unwrap_or(0)
}

fn matches_target(entry: &QueryLogEntryView, target: &str) -> bool {
    contains_case_insensitive(&entry.query_text, target)
        || contains_case_insensitive(&entry.query_summary, target)
        || entry
            .touched
            .iter()
            .any(|item| contains_case_insensitive(item, target))
        || entry
            .operations
            .iter()
            .any(|item| contains_case_insensitive(item, target))
        || entry
            .task_id
            .as_ref()
            .map(|task_id| contains_case_insensitive(task_id, target))
            .unwrap_or(false)
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
