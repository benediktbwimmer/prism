use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use prism_core::PrismPaths;
use prism_ir::{new_prefixed_id, new_sortable_token};
use prism_js::{
    McpCallLogEntryView, McpCallPayloadSummaryView, McpCallStatsBucketView, McpCallStatsView,
    McpCallTraceView, QueryDiagnostic, QueryPhaseView, QueryResultSummaryView, QueryTraceView,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::log_scope::{LogScope, RepoLogSource, select_log_sources};
use crate::mutation_trace::MutationTraceView;
use crate::{McpLogArgs, QueryHost};

const DEFAULT_MCP_LOG_LIMIT: usize = 20;
const DEFAULT_SLOW_MCP_LIMIT: usize = 20;
const DEFAULT_SLOW_MCP_MIN_DURATION_MS: u64 = 100;
const DEFAULT_MCP_CALL_LOG_MAX_BYTES: u64 = 1024 * 1024 * 1024;
const DEFAULT_MCP_CALL_LOG_SEGMENT_MAX_BYTES: u64 = 8 * 1024 * 1024;
const MCP_CALL_LOG_MAX_BYTES_ENV: &str = "PRISM_MCP_CALL_LOG_MAX_BYTES";
const MAX_SUMMARY_CHARS: usize = 160;
const MAX_SUMMARY_ITEMS: usize = 16;
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
    root: Option<PathBuf>,
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
    pub(crate) request_payload: Option<Value>,
    pub(crate) request_preview: Option<Value>,
    pub(crate) response_preview: Option<Value>,
    pub(crate) metadata: Value,
    pub(crate) query_compat: Option<QueryTraceView>,
    pub(crate) mutation_compat: Option<MutationTraceView>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpCallFilter {
    pub(crate) limit: usize,
    pub(crate) since: Option<u64>,
    pub(crate) scope: Option<LogScope>,
    pub(crate) call_type: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) worktree_id: Option<String>,
    pub(crate) repo_id: Option<String>,
    pub(crate) workspace_root: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) server_instance_id: Option<String>,
    pub(crate) process_id: Option<u32>,
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
        let path = root.map(default_mcp_call_log_path);
        Self {
            root: root.map(Path::to_path_buf),
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
            .and_then(|path| total_log_bytes(path).ok())
    }

    pub(crate) fn push(&self, record: PersistedMcpCallRecord) -> Result<()> {
        if let Some(path) = &self.path {
            let _guard = self.io_lock.lock().expect("mcp call log lock poisoned");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let line = serde_json::to_string(&record)?;
            rotate_active_segment_if_needed(path, line.len() + 1, self.max_bytes)?;
            append_line(path, &line)?;
            prune_archived_segments(path, self.max_bytes)?;
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
        let mut matches = filter_delegated_request_wrappers(self.records_for_filter(&filter))
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
        let mut matches = filter_delegated_request_wrappers(self.records_for_filter(&filter))
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
        self.trace_records()
            .into_iter()
            .find(|record| record.entry.id == id)
            .map(|record| McpCallTraceView {
                entry: record.entry,
                phases: record.phases,
                request_payload: record.request_payload,
                request_preview: record.request_preview,
                response_preview: record.response_preview,
                metadata: record.metadata,
            })
    }

    pub(crate) fn stats(&self, filter: McpCallFilter) -> McpCallStatsView {
        let records = filter_delegated_request_wrappers(self.records_for_filter(&filter))
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

    fn trace_records(&self) -> Vec<PersistedMcpCallRecord> {
        let Some(root) = self.root.as_deref() else {
            return self.records();
        };
        let Ok(sources) = select_log_sources(root, Some(LogScope::Repo), None) else {
            return self.records();
        };
        records_from_sources(&sources)
    }

    fn records_for_filter(&self, filter: &McpCallFilter) -> Vec<PersistedMcpCallRecord> {
        let Some(root) = self.root.as_deref() else {
            return self.records();
        };
        let Ok(sources) = select_log_sources(root, filter.scope, filter.worktree_id.as_deref())
        else {
            return self.records();
        };
        if sources.is_empty() {
            return Vec::new();
        }
        records_from_sources(&sources)
    }
}

fn filter_delegated_request_wrappers(
    records: Vec<PersistedMcpCallRecord>,
) -> Vec<PersistedMcpCallRecord> {
    let delegated_keys = records
        .iter()
        .filter(|record| record.entry.call_type != "request")
        .filter_map(delegated_request_key_for_record)
        .collect::<BTreeSet<_>>();
    records
        .into_iter()
        .filter(|record| {
            if record.entry.call_type != "request" {
                return true;
            }
            match delegated_request_key_for_record(record) {
                Some(key) => !delegated_keys.contains(&key),
                None => true,
            }
        })
        .collect()
}

fn delegated_request_key_for_record(record: &PersistedMcpCallRecord) -> Option<String> {
    if record.entry.call_type == "request" {
        delegated_request_key(record.request_preview.as_ref()?)
    } else {
        delegated_request_key(record.metadata.get("mcpRequest")?)
    }
}

fn delegated_request_key(value: &Value) -> Option<String> {
    let method = value.get("method")?.as_str()?;
    let request_id = serde_json::to_string(value.get("requestId")?).ok()?;
    let mut key = format!("{method}|{request_id}");
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        key.push('|');
        key.push_str(name);
    }
    Some(key)
}

impl McpCallFilter {
    pub(crate) fn from_args(
        limit: Option<usize>,
        since: Option<u64>,
        scope: Option<LogScope>,
        call_type: Option<String>,
        name: Option<String>,
        task_id: Option<String>,
        worktree_id: Option<String>,
        repo_id: Option<String>,
        workspace_root: Option<String>,
        session_id: Option<String>,
        server_instance_id: Option<String>,
        process_id: Option<u32>,
        success: Option<bool>,
        min_duration_ms: Option<u64>,
        contains: Option<String>,
        default_limit: usize,
    ) -> Self {
        Self {
            limit: limit.unwrap_or(default_limit).min(500),
            since,
            scope,
            call_type,
            name,
            task_id,
            worktree_id,
            repo_id,
            workspace_root,
            session_id,
            server_instance_id,
            process_id,
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
        if let Some(worktree_id) = &self.worktree_id {
            if entry.worktree_id.as_deref() != Some(worktree_id.as_str()) {
                return false;
            }
        }
        if let Some(repo_id) = &self.repo_id {
            if entry.repo_id.as_deref() != Some(repo_id.as_str()) {
                return false;
            }
        }
        if let Some(workspace_root) = &self.workspace_root {
            if entry.workspace_root.as_deref() != Some(workspace_root.as_str()) {
                return false;
            }
        }
        if let Some(session_id) = &self.session_id {
            if entry.session_id.as_deref() != Some(session_id.as_str()) {
                return false;
            }
        }
        if let Some(server_instance_id) = &self.server_instance_id {
            if entry.server_instance_id != *server_instance_id {
                return false;
            }
        }
        if let Some(process_id) = self.process_id {
            if entry.process_id != process_id {
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

pub(crate) fn default_mcp_call_log_path(root: &Path) -> PathBuf {
    PrismPaths::for_workspace_root(root)
        .and_then(|paths| paths.mcp_call_log_path())
        .unwrap_or_else(|_| root.join(".prism").join("prism-mcp-call-log.jsonl"))
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
        repo_id: None,
        worktree_id: None,
        log_path: None,
        trace_available: true,
    }
}

impl QueryHost {
    pub(crate) fn mcp_call_entries(&self, args: McpLogArgs) -> Vec<McpCallLogEntryView> {
        self.mcp_call_log_store.recent(McpCallFilter::from_args(
            args.limit,
            args.since,
            args.scope,
            args.call_type,
            args.name,
            args.task_id,
            args.worktree_id,
            args.repo_id,
            args.workspace_root,
            args.session_id,
            args.server_instance_id,
            args.process_id,
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
            args.scope,
            args.call_type,
            args.name,
            args.task_id,
            args.worktree_id,
            args.repo_id,
            args.workspace_root,
            args.session_id,
            args.server_instance_id,
            args.process_id,
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
            args.scope,
            args.call_type,
            args.name,
            args.task_id,
            args.worktree_id,
            args.repo_id,
            args.workspace_root,
            args.session_id,
            args.server_instance_id,
            args.process_id,
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

fn read_records_from_path(path: &Path) -> Result<Vec<PersistedMcpCallRecord>> {
    let mut records = Vec::new();
    for segment_path in segment_paths_in_read_order(path)? {
        let file = File::open(&segment_path)
            .with_context(|| format!("failed to open {}", segment_path.display()))?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line =
                line.with_context(|| format!("failed to read {}", segment_path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<PersistedMcpCallRecord>(&line) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn records_from_sources(sources: &[RepoLogSource]) -> Vec<PersistedMcpCallRecord> {
    let mut records = Vec::new();
    for source in sources {
        let path = &source.mcp_call_log_path;
        let mut source_records = read_records_from_path(path).unwrap_or_default();
        for record in &mut source_records {
            enrich_record_source(record, source, path);
        }
        records.extend(source_records);
    }
    records
}

fn enrich_record_source(record: &mut PersistedMcpCallRecord, source: &RepoLogSource, path: &Path) {
    if record.entry.workspace_root.is_none() {
        record.entry.workspace_root = Some(source.workspace_root.clone());
    }
    record.entry.repo_id = Some(source.repo_id.clone());
    record.entry.worktree_id = Some(source.worktree_id.clone());
    record.entry.log_path = Some(path.display().to_string());
}

#[derive(Debug, Clone)]
struct SegmentFile {
    path: PathBuf,
    bytes: u64,
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{line}").with_context(|| format!("failed to append {}", path.display()))?;
    Ok(())
}

fn rotate_active_segment_if_needed(
    path: &Path,
    next_write_bytes: usize,
    max_bytes: u64,
) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let active_len = metadata.len();
    if active_len == 0 {
        return Ok(());
    }
    if active_len.saturating_add(next_write_bytes as u64) <= segment_max_bytes(max_bytes) {
        return Ok(());
    }
    let archive_path = archived_segment_path(path, &new_sortable_token());
    fs::rename(path, &archive_path).with_context(|| {
        format!(
            "failed to rotate {} to {}",
            path.display(),
            archive_path.display()
        )
    })?;
    Ok(())
}

fn prune_archived_segments(path: &Path, max_bytes: u64) -> Result<()> {
    let mut total_bytes = total_log_bytes(path)?;
    if total_bytes <= max_bytes {
        return Ok(());
    }
    let prune_target = prune_target_bytes(max_bytes);
    for segment in archived_segments(path)? {
        if total_bytes <= prune_target {
            break;
        }
        fs::remove_file(&segment.path)
            .with_context(|| format!("failed to remove {}", segment.path.display()))?;
        total_bytes = total_bytes.saturating_sub(segment.bytes);
    }
    Ok(())
}

fn total_log_bytes(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for segment in archived_segments(path)? {
        total = total.saturating_add(segment.bytes);
    }
    if let Ok(metadata) = fs::metadata(path) {
        total = total.saturating_add(metadata.len());
    }
    Ok(total)
}

fn segment_paths_in_read_order(path: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = archived_segments(path)?
        .into_iter()
        .map(|segment| segment.path)
        .collect::<Vec<_>>();
    if path.exists() {
        paths.push(path.to_path_buf());
    }
    Ok(paths)
}

fn archived_segments(path: &Path) -> Result<Vec<SegmentFile>> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let (prefix, suffix) = archived_segment_name_parts(path);
    let mut segments = Vec::new();
    for entry in
        fs::read_dir(parent).with_context(|| format!("failed to read {}", parent.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", parent.display()))?;
        let entry_path = entry.path();
        if entry_path == path {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with(&prefix) || !name.ends_with(&suffix) {
            continue;
        }
        let bytes = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", entry_path.display()))?
            .len();
        segments.push(SegmentFile {
            path: entry_path,
            bytes,
        });
    }
    segments.sort_by(|left, right| left.path.file_name().cmp(&right.path.file_name()));
    Ok(segments)
}

fn archived_segment_path(path: &Path, token: &str) -> PathBuf {
    let (prefix, suffix) = archived_segment_name_parts(path);
    path.with_file_name(format!("{prefix}{token}{suffix}"))
}

fn archived_segment_name_parts(path: &Path) -> (String, String) {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("prism-mcp-call-log.jsonl");
    match (
        path.file_stem().and_then(|value| value.to_str()),
        path.extension().and_then(|value| value.to_str()),
    ) {
        (Some(stem), Some(ext)) => (format!("{stem}."), format!(".{ext}")),
        _ => (format!("{file_name}."), String::new()),
    }
}

fn segment_max_bytes(max_bytes: u64) -> u64 {
    DEFAULT_MCP_CALL_LOG_SEGMENT_MAX_BYTES.min((max_bytes / 4).max(1))
}

fn prune_target_bytes(max_bytes: u64) -> u64 {
    max_bytes.saturating_mul(3).saturating_div(4).max(1)
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
            workspace_root: Some(temp_test_dir().display().to_string()),
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
            root: None,
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
            request_payload: Some(request.clone()),
            request_preview: preview_value(&request),
            response_preview: preview_value(&response),
            metadata: json!({
                "tool": "prism_query",
                "queryKind": "typescript",
                "index": index,
            }),
            query_compat: None,
            mutation_compat: None,
        }
    }

    fn request_wrapper_record(request_id: usize, name: &str) -> PersistedMcpCallRecord {
        let request_preview = json!({
            "method": "tools/call",
            "requestId": request_id,
            "name": name,
            "taskInvocation": false,
        });
        PersistedMcpCallRecord {
            entry: new_log_entry(
                &test_runtime(),
                "request",
                "tools/call",
                None,
                format!("call {name}"),
                1_700_001_000 + request_id as u64,
                250,
                Some("session:test".to_string()),
                Some("task:test".to_string()),
                false,
                Some("request dropped before completion".to_string()),
                vec![
                    "mcp.receiveRequest".to_string(),
                    "mcp.executeHandler".to_string(),
                ],
                vec!["tools/call".to_string()],
                Vec::new(),
                payload_summary(Some(&request_preview)),
                payload_summary(None),
            ),
            phases: vec![],
            request_payload: Some(request_preview.clone()),
            request_preview: Some(request_preview.clone()),
            response_preview: None,
            metadata: request_preview,
            query_compat: None,
            mutation_compat: None,
        }
    }

    fn delegated_tool_record(request_id: usize, name: &str) -> PersistedMcpCallRecord {
        let mut record = test_record(request_id);
        record.entry.name = name.to_string();
        record.entry.summary = format!("tool {name}");
        record.entry.started_at = 1_700_001_000 + request_id as u64;
        record.metadata["mcpRequest"] = json!({
            "method": "tools/call",
            "requestId": request_id,
            "name": name,
            "taskInvocation": false,
        });
        record
    }

    #[test]
    fn mcp_call_log_store_round_trips_records_and_trace() {
        let store = test_store(1024 * 1024);
        store.push(test_record(1)).unwrap();
        store.push(test_record(2)).unwrap();

        let entries = store.recent(McpCallFilter::from_args(
            Some(10),
            None,
            None,
            Some("tool".to_string()),
            Some("prism_query".to_string()),
            None,
            None,
            None,
            None,
            None,
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
        assert_eq!(
            trace
                .request_payload
                .as_ref()
                .and_then(|value| value["path"].as_str()),
            Some("src/file-2.rs")
        );
        assert!(trace.request_preview.is_some());
        assert!(trace.response_preview.is_some());
    }

    #[test]
    fn recent_and_stats_filter_delegated_request_wrappers() {
        let store = McpCallLogStore {
            root: None,
            path: None,
            max_bytes: DEFAULT_MCP_CALL_LOG_MAX_BYTES,
            io_lock: Mutex::new(()),
            fallback_records: Mutex::new(VecDeque::from(vec![
                request_wrapper_record(61, "prism_query"),
                delegated_tool_record(61, "prism_query"),
            ])),
            runtime: test_runtime(),
        };

        let recent = store.recent(McpCallFilter::from_args(
            Some(10),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            DEFAULT_MCP_LOG_LIMIT,
        ));
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].call_type, "tool");
        assert_eq!(recent[0].name, "prism_query");

        let stats = store.stats(McpCallFilter::from_args(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            DEFAULT_MCP_LOG_LIMIT,
        ));
        assert_eq!(stats.total_calls, 1);
        assert_eq!(stats.error_count, 0);
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
        assert!(
            store.file_len().unwrap_or_default() <= 1_600 + segment_max_bytes(1_600),
            "active segment may temporarily exceed the nominal cap by one oversized record"
        );
    }

    #[test]
    fn mcp_call_log_store_rotates_into_archived_segments() {
        let store = test_store(5_000);
        for index in 0..4 {
            store.push(test_record(index)).unwrap();
        }

        let log_path = store.path().expect("store path");
        assert!(log_path.exists());
        assert!(!archived_segments(log_path).unwrap().is_empty());
        assert!(store.file_len().unwrap_or_default() > 0);
    }

    #[test]
    fn default_mcp_call_log_path_is_stable_across_restarts() {
        let _ = crate::tests_support::ensure_process_test_prism_home();
        let root = temp_test_dir();
        assert_eq!(
            default_mcp_call_log_path(&root),
            PrismPaths::for_workspace_root(&root)
                .unwrap()
                .mcp_call_log_path()
                .unwrap()
        );
    }
}
