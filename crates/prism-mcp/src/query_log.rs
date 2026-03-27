use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prism_js::{
    QueryDiagnostic, QueryLogEntryView, QueryPhaseView, QueryResultSummaryView, QueryTraceView,
};
use serde_json::Value;

use crate::{current_timestamp, DashboardState, QueryHost, QueryLogArgs};

const QUERY_LOG_CAPACITY: usize = 200;
const DEFAULT_QUERY_LOG_LIMIT: usize = 20;
const DEFAULT_SLOW_QUERY_LIMIT: usize = 20;
const DEFAULT_SLOW_QUERY_MIN_DURATION_MS: u64 = 250;
const MAX_QUERY_TEXT_CHARS: usize = 4096;
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_SUMMARY_ITEMS: usize = 8;

#[derive(Debug)]
pub(crate) struct QueryLogStore {
    next_id: AtomicU64,
    entries: Mutex<VecDeque<QueryTraceRecord>>,
    capacity: usize,
}

impl Default for QueryLogStore {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(VecDeque::with_capacity(QUERY_LOG_CAPACITY)),
            capacity: QUERY_LOG_CAPACITY,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct QueryRun {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) query_text: String,
    pub(crate) query_summary: String,
    pub(crate) started_at: u64,
    pub(crate) started: Instant,
    pub(crate) session_id: String,
    pub(crate) task_id: Option<String>,
    dashboard: Arc<DashboardState>,
    phases: Arc<Mutex<Vec<QueryPhaseView>>>,
}

#[derive(Debug, Clone)]
struct QueryTraceRecord {
    entry: QueryLogEntryView,
    phases: Vec<QueryPhaseView>,
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
    pub(crate) fn begin_query_run(&self, kind: &str, query_text: impl Into<String>) -> QueryRun {
        let query_text = clamp_string(&query_text.into(), MAX_QUERY_TEXT_CHARS);
        let sequence = self.query_log_store.next_id.fetch_add(1, Ordering::Relaxed);
        let run = QueryRun {
            id: format!("query:{sequence}"),
            kind: kind.to_string(),
            query_summary: summarize_query(&query_text, kind),
            query_text,
            started_at: current_timestamp(),
            started: Instant::now(),
            session_id: self.session.session_id().0.to_string(),
            task_id: self.session.current_task().map(|task| task.0.to_string()),
            dashboard: Arc::clone(&self.dashboard_state),
            phases: Arc::new(Mutex::new(Vec::new())),
        };
        run.dashboard_start(self.dashboard_state.as_ref());
        run
    }

    pub(crate) fn query_log_entries(&self, args: QueryLogArgs) -> Vec<QueryLogEntryView> {
        let filter = QueryLogFilter::from_args(args, DEFAULT_QUERY_LOG_LIMIT);
        self.query_log_store.recent(filter)
    }

    pub(crate) fn slow_query_entries(&self, args: QueryLogArgs) -> Vec<QueryLogEntryView> {
        let filter = QueryLogFilter::from_args(args, DEFAULT_SLOW_QUERY_LIMIT).for_slow_queries();
        self.query_log_store.slow(filter)
    }

    pub(crate) fn query_trace_view(&self, id: &str) -> Option<QueryTraceView> {
        self.query_log_store.trace(id)
    }
}

impl QueryRun {
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
            .expect("query log phases lock poisoned")
            .push(phase.clone());
        self.dashboard_phase(self.dashboard.as_ref(), &phase);
    }

    pub(crate) fn finish_success(
        &self,
        store: &QueryLogStore,
        result: &Value,
        diagnostics: Vec<QueryDiagnostic>,
        json_bytes: usize,
        output_cap_hit: bool,
    ) {
        let record = self.build_record(
            Some(result),
            diagnostics,
            json_bytes,
            output_cap_hit,
            true,
            None,
        );
        store.push(record.clone());
        self.dashboard_finish(self.dashboard.as_ref(), &record.entry);
    }

    pub(crate) fn finish_error(
        &self,
        store: &QueryLogStore,
        diagnostics: Vec<QueryDiagnostic>,
        error: impl Into<String>,
    ) {
        let record = self.build_record(None, diagnostics, 0, false, false, Some(error.into()));
        store.push(record.clone());
        self.dashboard_finish(self.dashboard.as_ref(), &record.entry);
    }

    fn build_record(
        &self,
        result: Option<&Value>,
        diagnostics: Vec<QueryDiagnostic>,
        json_bytes: usize,
        output_cap_hit: bool,
        success: bool,
        error: Option<String>,
    ) -> QueryTraceRecord {
        let phases = self
            .phases
            .lock()
            .expect("query log phases lock poisoned")
            .clone();
        let operations = unique_operations(&phases);
        let touched = unique_touches(&phases);
        let result_cap_hit = diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "result_truncated" | "depth_limited"
            )
        });
        let result_summary = match result {
            Some(value) => QueryResultSummaryView {
                kind: value_kind(value),
                json_bytes,
                item_count: item_count(value),
                truncated: output_cap_hit || result_cap_hit,
                output_cap_hit,
                result_cap_hit,
            },
            None => QueryResultSummaryView {
                kind: "error".to_string(),
                json_bytes,
                item_count: None,
                truncated: false,
                output_cap_hit: false,
                result_cap_hit: false,
            },
        };
        QueryTraceRecord {
            entry: QueryLogEntryView {
                id: self.id.clone(),
                kind: self.kind.clone(),
                query_summary: self.query_summary.clone(),
                query_text: self.query_text.clone(),
                started_at: self.started_at,
                duration_ms: duration_to_ms(self.started.elapsed()),
                session_id: self.session_id.clone(),
                task_id: self.task_id.clone(),
                success,
                error,
                operations,
                touched,
                diagnostics,
                result: result_summary,
            },
            phases,
        }
    }
}

impl QueryLogStore {
    fn recent(&self, filter: QueryLogFilter) -> Vec<QueryLogEntryView> {
        let entries = self.entries.lock().expect("query log lock poisoned");
        let mut matches = entries
            .iter()
            .rev()
            .filter(|record| filter.matches(record))
            .take(filter.limit)
            .map(|record| record.entry.clone())
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        matches
    }

    fn slow(&self, filter: QueryLogFilter) -> Vec<QueryLogEntryView> {
        let entries = self.entries.lock().expect("query log lock poisoned");
        let mut matches = entries
            .iter()
            .filter(|record| filter.matches(record))
            .map(|record| record.entry.clone())
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

    fn trace(&self, id: &str) -> Option<QueryTraceView> {
        self.entries
            .lock()
            .expect("query log lock poisoned")
            .iter()
            .find(|record| record.entry.id == id)
            .cloned()
            .map(|record| QueryTraceView {
                entry: record.entry,
                phases: record.phases,
            })
    }

    fn push(&self, record: QueryTraceRecord) {
        let mut entries = self.entries.lock().expect("query log lock poisoned");
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(record);
    }
}

impl QueryLogFilter {
    fn from_args(args: QueryLogArgs, default_limit: usize) -> Self {
        Self {
            limit: args.limit.unwrap_or(default_limit).min(QUERY_LOG_CAPACITY),
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

    fn matches(&self, record: &QueryTraceRecord) -> bool {
        if let Some(since) = self.since {
            if record.entry.started_at < since {
                return false;
            }
        }
        if let Some(min_duration_ms) = self.min_duration_ms {
            if record.entry.duration_ms < min_duration_ms {
                return false;
            }
        }
        if let Some(task_id) = &self.task_id {
            if record.entry.task_id.as_deref() != Some(task_id.as_str()) {
                return false;
            }
        }
        if let Some(operation) = &self.operation {
            if !record
                .entry
                .operations
                .iter()
                .any(|candidate| equals_case_insensitive(candidate, operation))
            {
                return false;
            }
        }
        if let Some(target) = &self.target {
            if !matches_target(record, target) {
                return false;
            }
        }
        true
    }
}

fn summarize_query(query_text: &str, kind: &str) -> String {
    let summary = query_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if summary.is_empty() {
        kind.to_string()
    } else {
        clamp_string(&summary, MAX_SUMMARY_CHARS)
    }
}

fn summarize_value(value: &Value) -> Value {
    match value {
        Value::String(string) => Value::String(clamp_string(string, MAX_SUMMARY_CHARS)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_SUMMARY_ITEMS)
                .map(summarize_value)
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .take(MAX_SUMMARY_ITEMS)
                .map(|(key, value)| (key.clone(), summarize_value(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn touches_for_value(value: &Value) -> Vec<String> {
    let mut touched = BTreeSet::new();
    collect_touch_values(value, &mut Vec::new(), &mut touched);
    touched.into_iter().collect()
}

fn collect_touch_values(value: &Value, key_path: &mut Vec<String>, touched: &mut BTreeSet<String>) {
    match value {
        Value::String(string) => {
            let key = key_path.last().map(String::as_str);
            if should_record_touch(key, string) {
                touched.insert(clamp_string(string, MAX_SUMMARY_CHARS));
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(MAX_SUMMARY_ITEMS) {
                collect_touch_values(item, key_path, touched);
            }
        }
        Value::Object(map) => {
            for (key, value) in map.iter().take(MAX_SUMMARY_ITEMS) {
                key_path.push(key.clone());
                collect_touch_values(value, key_path, touched);
                key_path.pop();
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn should_record_touch(key: Option<&str>, value: &str) -> bool {
    !value.is_empty() && (key.is_some_and(is_semantic_touch_key) || looks_like_touch_value(value))
}

fn is_semantic_touch_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("path")
        || key.contains("file")
        || key.contains("target")
        || key.contains("task")
        || key.contains("lineage")
        || key.contains("artifact")
        || key.contains("claim")
        || key.contains("plan")
        || key.contains("job")
        || key.contains("anchor")
        || key.contains("operation")
        || key.contains("uri")
}

fn looks_like_touch_value(value: &str) -> bool {
    value.contains('/')
        || value.contains("::")
        || value.starts_with("task:")
        || value.starts_with("coord-task:")
        || value.starts_with("plan:")
        || value.starts_with("claim:")
        || value.starts_with("artifact:")
        || value.starts_with("lineage:")
        || value.starts_with("http://")
        || value.starts_with("https://")
        || matches_file_name(value)
}

fn matches_file_name(value: &str) -> bool {
    value.ends_with(".rs")
        || value.ends_with(".toml")
        || value.ends_with(".json")
        || value.ends_with(".yaml")
        || value.ends_with(".yml")
        || value.ends_with(".md")
        || value.ends_with(".ts")
        || value.ends_with(".js")
}

fn unique_operations(phases: &[QueryPhaseView]) -> Vec<String> {
    let mut operations = Vec::new();
    for phase in phases {
        if !operations.iter().any(|seen| seen == &phase.operation) {
            operations.push(phase.operation.clone());
        }
    }
    operations
}

fn unique_touches(phases: &[QueryPhaseView]) -> Vec<String> {
    let mut touched = BTreeSet::new();
    for phase in phases {
        for item in &phase.touched {
            touched.insert(item.clone());
        }
    }
    touched.into_iter().collect()
}

fn matches_target(record: &QueryTraceRecord, target: &str) -> bool {
    contains_case_insensitive(&record.entry.query_text, target)
        || contains_case_insensitive(&record.entry.query_summary, target)
        || record
            .entry
            .touched
            .iter()
            .any(|item| contains_case_insensitive(item, target))
        || record
            .entry
            .operations
            .iter()
            .any(|item| contains_case_insensitive(item, target))
        || record
            .entry
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

fn equals_case_insensitive(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn clamp_string(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let head = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn value_kind(value: &Value) -> String {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
    .to_string()
}

fn item_count(value: &Value) -> Option<usize> {
    match value {
        Value::Array(items) => Some(items.len()),
        Value::Object(map) => Some(map.len()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn duration_to_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
