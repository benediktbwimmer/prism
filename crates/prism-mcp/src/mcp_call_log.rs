use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use prism_ir::{new_prefixed_id, new_sortable_token};
use prism_js::{
    McpCallLogEntryView, McpCallPayloadSummaryView, McpCallStatsBucketView, McpCallStatsView,
    McpCallTraceView, QueryDiagnostic, QueryPhaseView, QueryResultSummaryView, QueryTraceView,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{McpLogArgs, QueryHost};

const DEFAULT_MCP_LOG_LIMIT: usize = 20;
const DEFAULT_SLOW_MCP_LIMIT: usize = 20;
const DEFAULT_SLOW_MCP_MIN_DURATION_MS: u64 = 100;
const DEFAULT_MCP_CALL_LOG_MAX_BYTES: u64 = 64 * 1024 * 1024;
const MCP_CALL_LOG_MAX_BYTES_ENV: &str = "PRISM_MCP_CALL_LOG_MAX_BYTES";
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_SUMMARY_ITEMS: usize = 8;
const MAX_QUERY_TEXT_CHARS: usize = 4096;
const MAX_PREVIEW_CHARS: usize = 2048;

#[derive(Debug, Clone)]
pub(crate) struct McpCallLogRuntime {
    pub(crate) instance_id: String,
    pub(crate) process_id: u32,
    pub(crate) workspace_root: Option<String>,
}

#[derive(Debug)]
pub(crate) struct McpCallLogStore {
    path: Option<PathBuf>,
    max_bytes: u64,
    io_lock: Mutex<()>,
    fallback_records: Mutex<VecDeque<PersistedMcpCallRecord>>,
    runtime: McpCallLogRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedMcpCallRecord {
    pub(crate) entry: McpCallLogEntryView,
    pub(crate) phases: Vec<QueryPhaseView>,
    pub(crate) request_preview: Option<Value>,
    pub(crate) response_preview: Option<Value>,
    pub(crate) metadata: Value,
    pub(crate) query_compat: Option<QueryTraceView>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpCallFilter {
    pub(crate) limit: usize,
    pub(crate) since: Option<u64>,
    pub(crate) call_type: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) success: Option<bool>,
    pub(crate) min_duration_ms: Option<u64>,
    pub(crate) contains: Option<String>,
}

impl McpCallLogStore {
    pub(crate) fn for_root(root: Option<&Path>) -> Self {
        let runtime = McpCallLogRuntime {
            instance_id: format!("mcp-instance:{}", new_sortable_token()),
            process_id: std::process::id(),
            workspace_root: root.map(|path| path.display().to_string()),
        };
        let path = root.map(|path| default_mcp_call_log_path(path, &runtime.instance_id));
        Self {
            path,
            max_bytes: configured_max_bytes(),
            io_lock: Mutex::new(()),
            fallback_records: Mutex::new(VecDeque::new()),
            runtime,
        }
    }

    pub(crate) fn runtime(&self) -> &McpCallLogRuntime {
        &self.runtime
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn file_len(&self) -> Option<u64> {
        self.path
            .as_ref()
            .and_then(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len())
    }

    pub(crate) fn push(&self, record: PersistedMcpCallRecord) -> Result<()> {
        if let Some(path) = &self.path {
            let _guard = self.io_lock.lock().expect("mcp call log lock poisoned");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let line = serde_json::to_string(&record)?;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            writeln!(file, "{line}")
                .with_context(|| format!("failed to append {}", path.display()))?;
            trim_file_to_max_bytes(path, self.max_bytes)?;
            Ok(())
        } else {
            let mut records = self
                .fallback_records
                .lock()
                .expect("fallback mcp call log lock poisoned");
            records.push_back(record);
            while records.len() > 512 {
                records.pop_front();
            }
            Ok(())
        }
    }

    pub(crate) fn records(&self) -> Vec<PersistedMcpCallRecord> {
        if let Some(path) = &self.path {
            read_records_from_path(path).unwrap_or_default()
        } else {
            self.fallback_records
                .lock()
                .expect("fallback mcp call log lock poisoned")
                .iter()
                .cloned()
                .collect()
        }
    }

    pub(crate) fn recent(&self, filter: McpCallFilter) -> Vec<McpCallLogEntryView> {
        let mut matches = self
            .records()
            .into_iter()
            .filter(|record| filter.matches(&record.entry))
            .map(|record| record.entry)
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

    pub(crate) fn slow(&self, mut filter: McpCallFilter) -> Vec<McpCallLogEntryView> {
        if filter.min_duration_ms.is_none() {
            filter.min_duration_ms = Some(DEFAULT_SLOW_MCP_MIN_DURATION_MS);
        }
        let mut matches = self
            .records()
            .into_iter()
            .filter(|record| filter.matches(&record.entry))
            .map(|record| record.entry)
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

    pub(crate) fn trace(&self, id: &str) -> Option<McpCallTraceView> {
        self.records()
            .into_iter()
            .find(|record| record.entry.id == id)
            .map(|record| McpCallTraceView {
                entry: record.entry,
                phases: record.phases,
                request_preview: record.request_preview,
                response_preview: record.response_preview,
                metadata: record.metadata,
            })
    }

    pub(crate) fn stats(&self, filter: McpCallFilter) -> McpCallStatsView {
        let records = self
            .records()
            .into_iter()
            .filter(|record| filter.matches(&record.entry))
            .collect::<Vec<_>>();
        let total_calls = records.len();
        let success_count = records.iter().filter(|record| record.entry.success).count();
        let error_count = total_calls.saturating_sub(success_count);
        let total_duration = records.iter().fold(0u128, |acc, record| {
            acc + u128::from(record.entry.duration_ms)
        });
        let average_duration_ms = if total_calls == 0 {
            0
        } else {
            (total_duration / total_calls as u128) as u64
        };
        let max_duration_ms = records
            .iter()
            .map(|record| record.entry.duration_ms)
            .max()
            .unwrap_or(0);
        McpCallStatsView {
            total_calls,
            success_count,
            error_count,
            average_duration_ms,
            max_duration_ms,
            by_call_type: aggregate_buckets(&records, |record| {
                Some(record.entry.call_type.clone())
            }),
            by_name: aggregate_buckets(&records, |record| Some(record.entry.name.clone())),
            by_view_name: aggregate_buckets(&records, |record| record.entry.view_name.clone()),
        }
    }
}

impl McpCallFilter {
    pub(crate) fn from_args(
        limit: Option<usize>,
        since: Option<u64>,
        call_type: Option<String>,
        name: Option<String>,
        task_id: Option<String>,
        session_id: Option<String>,
        success: Option<bool>,
        min_duration_ms: Option<u64>,
        contains: Option<String>,
        default_limit: usize,
    ) -> Self {
        Self {
            limit: limit.unwrap_or(default_limit).min(500),
            since,
            call_type,
            name,
            task_id,
            session_id,
            success,
            min_duration_ms,
            contains,
        }
    }

    fn matches(&self, entry: &McpCallLogEntryView) -> bool {
        if let Some(since) = self.since {
            if entry.started_at < since {
                return false;
            }
        }
        if let Some(call_type) = &self.call_type {
            if !equals_case_insensitive(&entry.call_type, call_type) {
                return false;
            }
        }
        if let Some(name) = &self.name {
            if !equals_case_insensitive(&entry.name, name)
                && !contains_case_insensitive(&entry.name, name)
            {
                return false;
            }
        }
        if let Some(task_id) = &self.task_id {
            if entry.task_id.as_deref() != Some(task_id.as_str()) {
                return false;
            }
        }
        if let Some(session_id) = &self.session_id {
            if entry.session_id.as_deref() != Some(session_id.as_str()) {
                return false;
            }
        }
        if let Some(success) = self.success {
            if entry.success != success {
                return false;
            }
        }
        if let Some(min_duration_ms) = self.min_duration_ms {
            if entry.duration_ms < min_duration_ms {
                return false;
            }
        }
        if let Some(contains) = &self.contains {
            if !entry_contains(entry, contains) {
                return false;
            }
        }
        true
    }
}

pub(crate) fn default_mcp_call_log_path(root: &Path, instance_id: &str) -> PathBuf {
    let file_suffix = instance_id.replace(':', "-");
    root.join(".prism")
        .join(format!("prism-mcp-call-log-{file_suffix}.jsonl"))
}

pub(crate) fn summarize_query(query_text: &str, kind: &str) -> String {
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

pub(crate) fn sanitize_query_text(query_text: &str) -> String {
    clamp_string(query_text, MAX_QUERY_TEXT_CHARS)
}

pub(crate) fn summarize_value(value: &Value) -> Value {
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

pub(crate) fn touches_for_value(value: &Value) -> Vec<String> {
    let mut touched = BTreeSet::new();
    collect_touch_values(value, &mut Vec::new(), &mut touched);
    touched.into_iter().collect()
}

pub(crate) fn duration_to_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

pub(crate) fn unique_operations(phases: &[QueryPhaseView]) -> Vec<String> {
    let mut operations = Vec::new();
    for phase in phases {
        if !operations.iter().any(|seen| seen == &phase.operation) {
            operations.push(phase.operation.clone());
        }
    }
    operations
}

pub(crate) fn unique_touches(phases: &[QueryPhaseView]) -> Vec<String> {
    let mut touched = BTreeSet::new();
    for phase in phases {
        for item in &phase.touched {
            touched.insert(item.clone());
        }
    }
    touched.into_iter().collect()
}

pub(crate) fn payload_summary(value: Option<&Value>) -> McpCallPayloadSummaryView {
    match value {
        Some(value) => McpCallPayloadSummaryView {
            kind: value_kind(value),
            json_bytes: serde_json::to_vec(value)
                .map(|bytes| bytes.len())
                .unwrap_or(0),
            item_count: item_count(value),
            truncated: is_value_truncated(value),
            excerpt: Some(summarize_value(value)),
        },
        None => McpCallPayloadSummaryView {
            kind: "none".to_string(),
            json_bytes: 0,
            item_count: None,
            truncated: false,
            excerpt: None,
        },
    }
}

pub(crate) fn query_result_summary(
    value: Option<&Value>,
    json_bytes: usize,
    output_cap_hit: bool,
    diagnostics: &[QueryDiagnostic],
) -> QueryResultSummaryView {
    let result_cap_hit = diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "result_truncated" | "depth_limited"
        )
    });
    match value {
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
    }
}

pub(crate) fn new_log_entry(
    runtime: &McpCallLogRuntime,
    call_type: &str,
    name: &str,
    view_name: Option<String>,
    summary: String,
    started_at: u64,
    duration_ms: u64,
    session_id: Option<String>,
    task_id: Option<String>,
    success: bool,
    error: Option<String>,
    operations: Vec<String>,
    touched: Vec<String>,
    diagnostics: Vec<QueryDiagnostic>,
    request: McpCallPayloadSummaryView,
    response: McpCallPayloadSummaryView,
) -> McpCallLogEntryView {
    McpCallLogEntryView {
        id: new_prefixed_id("mcp-call").to_string(),
        call_type: call_type.to_string(),
        name: name.to_string(),
        view_name,
        summary,
        started_at,
        duration_ms,
        session_id,
        task_id,
        success,
        error,
        operations,
        touched,
        diagnostics,
        request,
        response,
        server_instance_id: runtime.instance_id.clone(),
        process_id: runtime.process_id,
        workspace_root: runtime.workspace_root.clone(),
        trace_available: true,
    }
}

impl QueryHost {
    pub(crate) fn mcp_call_entries(&self, args: McpLogArgs) -> Vec<McpCallLogEntryView> {
        self.mcp_call_log_store.recent(McpCallFilter::from_args(
            args.limit,
            args.since,
            args.call_type,
            args.name,
            args.task_id,
            args.session_id,
            args.success,
            args.min_duration_ms,
            args.contains,
            DEFAULT_MCP_LOG_LIMIT,
        ))
    }

    pub(crate) fn slow_mcp_call_entries(&self, args: McpLogArgs) -> Vec<McpCallLogEntryView> {
        let mut filter = McpCallFilter::from_args(
            args.limit,
            args.since,
            args.call_type,
            args.name,
            args.task_id,
            args.session_id,
            args.success,
            args.min_duration_ms,
            args.contains,
            DEFAULT_SLOW_MCP_LIMIT,
        );
        if filter.min_duration_ms.is_none() {
            filter.min_duration_ms = Some(DEFAULT_SLOW_MCP_MIN_DURATION_MS);
        }
        self.mcp_call_log_store.slow(filter)
    }

    pub(crate) fn mcp_call_trace_view(&self, id: &str) -> Option<McpCallTraceView> {
        self.mcp_call_log_store.trace(id)
    }

    pub(crate) fn mcp_call_stats(&self, args: McpLogArgs) -> McpCallStatsView {
        self.mcp_call_log_store.stats(McpCallFilter::from_args(
            None,
            args.since,
            args.call_type,
            args.name,
            args.task_id,
            args.session_id,
            args.success,
            args.min_duration_ms,
            args.contains,
            DEFAULT_MCP_LOG_LIMIT,
        ))
    }
}

fn configured_max_bytes() -> u64 {
    env::var(MCP_CALL_LOG_MAX_BYTES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MCP_CALL_LOG_MAX_BYTES)
}

fn trim_file_to_max_bytes(path: &Path, max_bytes: u64) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() <= max_bytes {
        return Ok(());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader
        .lines()
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read {}", path.display()))?;
    while serialized_lines_len(&lines) > max_bytes as usize && !lines.is_empty() {
        lines.remove(0);
    }
    let mut file = File::create(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
}

fn read_records_from_path(path: &Path) -> Result<Vec<PersistedMcpCallRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<PersistedMcpCallRecord>(&line) {
            records.push(record);
        }
    }
    Ok(records)
}

fn aggregate_buckets(
    records: &[PersistedMcpCallRecord],
    key_fn: impl Fn(&PersistedMcpCallRecord) -> Option<String>,
) -> Vec<McpCallStatsBucketView> {
    let mut buckets: BTreeMap<String, Vec<&PersistedMcpCallRecord>> = BTreeMap::new();
    for record in records {
        let Some(key) = key_fn(record) else {
            continue;
        };
        buckets.entry(key).or_default().push(record);
    }
    let mut views = buckets
        .into_iter()
        .map(|(key, bucket)| {
            let count = bucket.len();
            let error_count = bucket.iter().filter(|record| !record.entry.success).count();
            let unique_task_count = bucket
                .iter()
                .filter_map(|record| record.entry.task_id.as_deref())
                .collect::<BTreeSet<_>>()
                .len();
            let total_duration = bucket.iter().fold(0u128, |acc, record| {
                acc + u128::from(record.entry.duration_ms)
            });
            let average_duration_ms = if count == 0 {
                0
            } else {
                (total_duration / count as u128) as u64
            };
            let max_duration_ms = bucket
                .iter()
                .map(|record| record.entry.duration_ms)
                .max()
                .unwrap_or(0);
            let total_result_json_bytes = bucket.iter().fold(0u128, |acc, record| {
                acc + record.entry.response.json_bytes as u128
            });
            let average_result_json_bytes = if count == 0 {
                0
            } else {
                (total_result_json_bytes / count as u128) as u64
            };
            let max_result_json_bytes = bucket
                .iter()
                .map(|record| record.entry.response.json_bytes)
                .max()
                .unwrap_or(0) as u64;
            McpCallStatsBucketView {
                key,
                count,
                error_count,
                unique_task_count,
                average_duration_ms,
                max_duration_ms,
                average_result_json_bytes,
                max_result_json_bytes,
            }
        })
        .collect::<Vec<_>>();
    views.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| right.max_duration_ms.cmp(&left.max_duration_ms))
            .then_with(|| left.key.cmp(&right.key))
    });
    views.truncate(10);
    views
}

fn serialized_lines_len(lines: &[String]) -> usize {
    lines.iter().map(|line| line.len() + 1).sum()
}

fn entry_contains(entry: &McpCallLogEntryView, needle: &str) -> bool {
    contains_case_insensitive(&entry.name, needle)
        || entry
            .view_name
            .as_ref()
            .is_some_and(|view_name| contains_case_insensitive(view_name, needle))
        || contains_case_insensitive(&entry.summary, needle)
        || contains_case_insensitive(&entry.call_type, needle)
        || entry
            .task_id
            .as_ref()
            .is_some_and(|task_id| contains_case_insensitive(task_id, needle))
        || entry
            .session_id
            .as_ref()
            .is_some_and(|session_id| contains_case_insensitive(session_id, needle))
        || entry
            .operations
            .iter()
            .any(|value| contains_case_insensitive(value, needle))
        || entry
            .touched
            .iter()
            .any(|value| contains_case_insensitive(value, needle))
        || entry
            .error
            .as_ref()
            .is_some_and(|value| contains_case_insensitive(value, needle))
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
        || value.ends_with(".py")
        || value.ends_with(".toml")
        || value.ends_with(".json")
        || value.ends_with(".yaml")
        || value.ends_with(".yml")
        || value.ends_with(".md")
        || value.ends_with(".ts")
        || value.ends_with(".js")
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

fn is_value_truncated(value: &Value) -> bool {
    match value {
        Value::String(string) => string.chars().count() > MAX_SUMMARY_CHARS,
        Value::Array(items) => items.len() > MAX_SUMMARY_ITEMS,
        Value::Object(map) => map.len() > MAX_SUMMARY_ITEMS,
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(crate) fn preview_value(value: &Value) -> Option<Value> {
    let summarized = summarize_value(value);
    let bytes = serde_json::to_vec(&summarized).ok()?;
    if bytes.len() <= MAX_PREVIEW_CHARS {
        Some(summarized)
    } else {
        Some(json!(clamp_string(
            &String::from_utf8_lossy(&bytes),
            MAX_PREVIEW_CHARS
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn test_runtime() -> McpCallLogRuntime {
        McpCallLogRuntime {
            instance_id: "mcp-instance:test".to_string(),
            process_id: 4242,
            workspace_root: Some("/tmp/prism-mcp-log-tests".to_string()),
        }
    }

    fn temp_test_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "prism-mcp-call-log-test-{}-{suffix}",
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_store(max_bytes: u64) -> McpCallLogStore {
        let root = temp_test_dir();
        McpCallLogStore {
            path: Some(root.join("mcp-log.jsonl")),
            max_bytes,
            io_lock: Mutex::new(()),
            fallback_records: Mutex::new(VecDeque::new()),
            runtime: test_runtime(),
        }
    }

    fn test_record(index: usize) -> PersistedMcpCallRecord {
        let request = json!({
            "path": format!("src/file-{index}.rs"),
            "queryText": format!("return prism.file(\"src/file-{index}.rs\").read();"),
        });
        let response = json!({
            "kind": "object",
            "payload": "x".repeat(256),
        });
        PersistedMcpCallRecord {
            entry: new_log_entry(
                &test_runtime(),
                "tool",
                "prism_query",
                None,
                format!("record {index}"),
                1_700_000_000 + index as u64,
                10 + index as u64,
                Some("session:test".to_string()),
                Some("task:test".to_string()),
                true,
                None,
                vec!["typescript".to_string(), "fileRead".to_string()],
                vec![format!("src/file-{index}.rs")],
                Vec::new(),
                payload_summary(Some(&request)),
                payload_summary(Some(&response)),
            ),
            phases: vec![QueryPhaseView {
                operation: "fileRead".to_string(),
                started_at: 1_700_000_000 + index as u64,
                duration_ms: 10 + index as u64,
                args_summary: Some(summarize_value(&request)),
                touched: vec![format!("src/file-{index}.rs")],
                success: true,
                error: None,
            }],
            request_preview: preview_value(&request),
            response_preview: preview_value(&response),
            metadata: json!({
                "tool": "prism_query",
                "queryKind": "typescript",
                "index": index,
            }),
            query_compat: None,
        }
    }

    #[test]
    fn mcp_call_log_store_round_trips_records_and_trace() {
        let store = test_store(1024 * 1024);
        store.push(test_record(1)).unwrap();
        store.push(test_record(2)).unwrap();

        let entries = store.recent(McpCallFilter::from_args(
            Some(10),
            None,
            Some("tool".to_string()),
            Some("prism_query".to_string()),
            None,
            None,
            Some(true),
            None,
            Some("src/file-2.rs".to_string()),
            DEFAULT_MCP_LOG_LIMIT,
        ));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "record 2");
        assert_eq!(entries[0].server_instance_id, "mcp-instance:test");

        let trace = store.trace(&entries[0].id).expect("trace should exist");
        assert_eq!(trace.entry.summary, "record 2");
        assert_eq!(trace.metadata["index"], 2);
        assert!(trace.request_preview.is_some());
        assert!(trace.response_preview.is_some());
    }

    #[test]
    fn mcp_call_log_store_trims_oldest_records_when_file_exceeds_cap() {
        let store = test_store(1_600);
        for index in 0..8 {
            store.push(test_record(index)).unwrap();
        }

        let records = store.records();
        assert!(!records.is_empty());
        assert!(records.len() < 8);
        assert_eq!(
            records.last().expect("latest record").metadata["index"],
            json!(7)
        );
        assert!(
            records.first().expect("oldest retained record").metadata["index"]
                .as_u64()
                .unwrap_or_default()
                > 0
        );
        assert!(store.file_len().unwrap_or_default() <= 1_600);
    }
}
