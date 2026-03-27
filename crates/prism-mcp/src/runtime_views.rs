use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use prism_js::{RuntimeHealthView, RuntimeLogEventView, RuntimeProcessView, RuntimeStatusView};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::runtime_state::{read_runtime_state, RuntimeEventRecord, RuntimeProcessRecord};
use crate::{QueryHost, RuntimeLogArgs, RuntimeTimelineArgs};

const DEFAULT_HEALTH_PATH: &str = "/healthz";
const DEFAULT_RUNTIME_LOG_LIMIT: usize = 50;
const DEFAULT_RUNTIME_TIMELINE_LIMIT: usize = 20;
const DEFAULT_LOG_SCAN_LINES: usize = 400;
const MAX_LOG_SCAN_LINES: usize = 4_000;

#[derive(Debug, Clone)]
struct RuntimePaths {
    uri_file: PathBuf,
    log_path: PathBuf,
    cache_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpProcessKind {
    Daemon,
    Bridge,
}

#[derive(Debug, Clone)]
struct McpProcess {
    pid: u32,
    ppid: u32,
    rss_kb: u64,
    elapsed: String,
    command: String,
    kind: McpProcessKind,
    health_path: Option<String>,
    http_uri: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeState {
    Connected,
    Idle,
    Orphaned,
}

#[derive(Debug, Clone, Default)]
struct BridgeCounts {
    connected: usize,
    idle: usize,
    orphaned: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonLogRecord {
    timestamp: Option<String>,
    level: Option<String>,
    message: Option<String>,
    target: Option<String>,
    filename: Option<String>,
    line_number: Option<u64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub(crate) fn runtime_status(host: &QueryHost) -> Result<RuntimeStatusView> {
    let root = workspace_root(host)?;
    let paths = RuntimePaths::for_root(root);
    let runtime_state = read_runtime_state(root)?;
    let state_processes = runtime_state
        .as_ref()
        .map(|state| state.processes.as_slice())
        .unwrap_or(&[]);
    let (processes, process_error) = match list_runtime_processes(root, state_processes) {
        Ok(processes) => (processes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let bridges = select_kind(&processes, McpProcessKind::Bridge);
    let uri = read_uri_file(&paths.uri_file)?
        .or_else(|| daemons.iter().find_map(|process| process.http_uri.clone()));
    let connected_bridge_pids = uri
        .as_deref()
        .and_then(|uri| connected_process_ids_for_uri(uri).ok())
        .unwrap_or_default();
    let bridge_counts = classify_bridges(&bridges, &connected_bridge_pids);
    let health_path = daemon_health_path(&daemons).to_string();
    let health = health_status(&uri, &daemons, process_error.as_deref());

    Ok(RuntimeStatusView {
        root: root.display().to_string(),
        uri,
        uri_file: paths.uri_file.display().to_string(),
        log_path: paths.log_path.display().to_string(),
        log_bytes: file_len(&paths.log_path),
        cache_path: paths.cache_path.display().to_string(),
        cache_bytes: file_len(&paths.cache_path),
        health_path,
        health,
        daemon_count: daemons.len(),
        bridge_count: bridges.len(),
        connected_bridge_count: bridge_counts.connected,
        idle_bridge_count: bridge_counts.idle,
        orphan_bridge_count: bridge_counts.orphaned,
        processes: processes
            .into_iter()
            .map(|process| runtime_process_view(process, &connected_bridge_pids))
            .collect(),
        process_error,
    })
}

pub(crate) fn runtime_logs(
    host: &QueryHost,
    args: RuntimeLogArgs,
) -> Result<Vec<RuntimeLogEventView>> {
    let root = workspace_root(host)?;
    let paths = RuntimePaths::for_root(root);
    let limit = args.limit.unwrap_or(DEFAULT_RUNTIME_LOG_LIMIT);
    if limit == 0 {
        return Ok(Vec::new());
    }

    let lines = tail_lines(&paths.log_path, scan_limit(limit))?;
    let level = args
        .level
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let target = args.target.as_deref();
    let contains = args
        .contains
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let mut results = Vec::new();
    for line in lines.into_iter().rev() {
        let event = parse_log_event(&line);
        if !matches_runtime_log(&event, &line, level.as_deref(), target, contains.as_deref()) {
            continue;
        }
        results.push(event);
        if results.len() >= limit {
            break;
        }
    }
    Ok(results)
}

pub(crate) fn runtime_timeline(
    host: &QueryHost,
    args: RuntimeTimelineArgs,
) -> Result<Vec<RuntimeLogEventView>> {
    let root = workspace_root(host)?;
    let paths = RuntimePaths::for_root(root);
    let limit = args.limit.unwrap_or(DEFAULT_RUNTIME_TIMELINE_LIMIT);
    if limit == 0 {
        return Ok(Vec::new());
    }

    let contains = args
        .contains
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    if let Some(state) = read_runtime_state(root)? {
        let mut events = state
            .events
            .into_iter()
            .map(runtime_state_event_view)
            .filter(|event| {
                contains
                    .as_deref()
                    .is_none_or(|needle| log_contains(event, &event.message, needle))
            })
            .collect::<Vec<_>>();
        if !events.is_empty() {
            if events.len() > limit {
                events = events.split_off(events.len() - limit);
            }
            return Ok(events);
        }
    }

    let mut events = tail_lines(&paths.log_path, scan_limit(limit))?
        .into_iter()
        .map(|line| (line.clone(), parse_log_event(&line)))
        .filter(|(line, event)| {
            is_timeline_event(event)
                && contains
                    .as_deref()
                    .is_none_or(|needle| log_contains(event, line, needle))
        })
        .map(|(_, event)| event)
        .collect::<Vec<_>>();
    if events.len() > limit {
        events = events.split_off(events.len() - limit);
    }
    Ok(events)
}

fn workspace_root(host: &QueryHost) -> Result<&Path> {
    host.workspace
        .as_ref()
        .map(|workspace| workspace.root())
        .ok_or_else(|| anyhow!("runtime introspection requires a workspace-backed PRISM session"))
}

fn file_len(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn runtime_process_view(
    process: McpProcess,
    connected_bridge_pids: &BTreeSet<u32>,
) -> RuntimeProcessView {
    let bridge_state = bridge_state(&process, connected_bridge_pids).map(bridge_state_label);
    RuntimeProcessView {
        pid: process.pid,
        parent_pid: process.ppid,
        rss_kb: process.rss_kb,
        rss_mb: process.rss_kb as f64 / 1024.0,
        elapsed: process.elapsed,
        kind: match process.kind {
            McpProcessKind::Daemon => "daemon",
            McpProcessKind::Bridge => "bridge",
        }
        .to_string(),
        command: process.command,
        health_path: process.health_path,
        bridge_state,
    }
}

fn runtime_state_event_view(event: RuntimeEventRecord) -> RuntimeLogEventView {
    RuntimeLogEventView {
        timestamp: Some(event.timestamp),
        level: Some(event.level),
        message: event.message,
        target: Some(event.target),
        file: event.file,
        line_number: event.line_number,
        fields: value_object_fields(event.fields),
    }
}

fn health_status(
    uri: &Option<String>,
    daemons: &[McpProcess],
    process_error: Option<&str>,
) -> RuntimeHealthView {
    if let Some(error) = process_error {
        return RuntimeHealthView {
            ok: false,
            detail: format!("process listing failed: {error}"),
        };
    }

    let Some(uri) = uri else {
        return RuntimeHealthView {
            ok: false,
            detail: "missing uri file".to_string(),
        };
    };

    if daemons.is_empty() && !health_probe_ok(uri, DEFAULT_HEALTH_PATH) {
        return RuntimeHealthView {
            ok: false,
            detail: format!("no daemon process for {uri}"),
        };
    }

    let detail = if daemons.len() == 1 {
        format!("ok ({uri})")
    } else {
        format!("ok ({uri}; {} daemon processes)", daemons.len())
    };
    RuntimeHealthView { ok: true, detail }
}

fn health_probe_ok(uri: &str, health_path: &str) -> bool {
    let Some(authority) = uri_authority(uri) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect(authority) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    let request =
        format!("GET {health_path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }
    response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200")
}

fn list_runtime_processes(
    root: &Path,
    state_processes: &[RuntimeProcessRecord],
) -> Result<Vec<McpProcess>> {
    if !state_processes.is_empty() {
        let processes = probe_runtime_state_processes(state_processes)?;
        if !processes.is_empty() {
            return Ok(processes);
        }
    }
    list_processes(root)
}

fn list_processes(root: &Path) -> Result<Vec<McpProcess>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss=,etime=,command="])
        .output()
        .context("failed to list processes with ps")?;
    if !output.status.success() {
        bail!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let root_flag = format!("--root {}", root.display());
    let lines = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    for line in lines.lines() {
        let Some(process) = parse_ps_line(line) else {
            continue;
        };
        if !process.command.contains("prism-mcp") || !process.command.contains(&root_flag) {
            continue;
        }
        processes.push(process);
    }
    Ok(processes)
}

fn probe_runtime_state_processes(
    state_processes: &[RuntimeProcessRecord],
) -> Result<Vec<McpProcess>> {
    let pids = state_processes
        .iter()
        .map(|process| process.pid.to_string())
        .collect::<Vec<_>>();
    let output = Command::new("ps")
        .args([
            "-o",
            "pid=,ppid=,rss=,etime=,command=",
            "-p",
            &pids.join(","),
        ])
        .output()
        .context("failed to inspect runtime state processes with ps")?;
    if !output.status.success() {
        bail!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let records = state_processes
        .iter()
        .map(|record| (record.pid, record))
        .collect::<BTreeMap<_, _>>();
    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((pid, ppid, rss_kb, elapsed, command)) = parse_process_snapshot_line(line) else {
            continue;
        };
        let Some(record) = records.get(&pid) else {
            continue;
        };
        let Some(kind) = process_kind(record.kind.as_str()) else {
            continue;
        };
        processes.push(McpProcess {
            pid,
            ppid,
            rss_kb,
            elapsed,
            command,
            kind,
            health_path: record.health_path.clone(),
            http_uri: record.http_uri.clone(),
        });
    }
    Ok(processes)
}

fn parse_ps_line(line: &str) -> Option<McpProcess> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse::<u32>().ok()?;
    let rss_kb = parts.next()?.parse().ok()?;
    let elapsed = parts.next()?.to_string();
    let command = parts.collect::<Vec<_>>().join(" ");
    let health_path = command_option_value(&command, "--health-path");
    let kind = match command_option_value(&command, "--mode").as_deref() {
        Some("daemon") => McpProcessKind::Daemon,
        Some("bridge") => McpProcessKind::Bridge,
        _ => return None,
    };
    Some(McpProcess {
        pid,
        ppid,
        rss_kb,
        elapsed,
        command,
        kind,
        health_path,
        http_uri: None,
    })
}

fn bridge_state(
    process: &McpProcess,
    connected_bridge_pids: &BTreeSet<u32>,
) -> Option<BridgeState> {
    if process.kind != McpProcessKind::Bridge {
        return None;
    }
    if connected_bridge_pids.contains(&process.pid) {
        Some(BridgeState::Connected)
    } else if process.ppid == 1 {
        Some(BridgeState::Orphaned)
    } else {
        Some(BridgeState::Idle)
    }
}

fn bridge_state_label(state: BridgeState) -> String {
    match state {
        BridgeState::Connected => "connected",
        BridgeState::Idle => "idle",
        BridgeState::Orphaned => "orphaned",
    }
    .to_string()
}

fn classify_bridges(bridges: &[McpProcess], connected_bridge_pids: &BTreeSet<u32>) -> BridgeCounts {
    let mut counts = BridgeCounts::default();
    for process in bridges {
        match bridge_state(process, connected_bridge_pids) {
            Some(BridgeState::Connected) => counts.connected += 1,
            Some(BridgeState::Idle) => counts.idle += 1,
            Some(BridgeState::Orphaned) => counts.orphaned += 1,
            None => {}
        }
    }
    counts
}

fn connected_process_ids_for_uri(uri: &str) -> Result<BTreeSet<u32>> {
    let Some(port) = uri_port(uri) else {
        return Ok(BTreeSet::new());
    };
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fp"])
        .output()
        .context("failed to inspect established TCP connections with lsof")?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('p'))
        .filter_map(|pid| pid.parse::<u32>().ok())
        .collect())
}

fn uri_port(uri: &str) -> Option<u16> {
    uri_authority(uri)?
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

fn uri_authority(uri: &str) -> Option<&str> {
    uri.strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .filter(|authority| !authority.is_empty())
}

fn parse_process_snapshot_line(line: &str) -> Option<(u32, u32, u64, String, String)> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse().ok()?;
    let rss_kb = parts.next()?.parse().ok()?;
    let elapsed = parts.next()?.to_string();
    let command = parts.collect::<Vec<_>>().join(" ");
    Some((pid, ppid, rss_kb, elapsed, command))
}

fn select_kind(processes: &[McpProcess], kind: McpProcessKind) -> Vec<McpProcess> {
    processes
        .iter()
        .filter(|process| process.kind == kind)
        .cloned()
        .collect()
}

fn daemon_health_path(daemons: &[McpProcess]) -> &str {
    daemons
        .first()
        .and_then(|daemon| daemon.health_path.as_deref())
        .unwrap_or(DEFAULT_HEALTH_PATH)
}

fn process_kind(kind: &str) -> Option<McpProcessKind> {
    match kind {
        "daemon" => Some(McpProcessKind::Daemon),
        "bridge" => Some(McpProcessKind::Bridge),
        _ => None,
    }
}

fn command_option_value(command: &str, option: &str) -> Option<String> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens
        .windows(2)
        .find_map(|window| (window[0] == option).then(|| window[1].to_string()))
}

fn read_uri_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)
        .with_context(|| format!("failed to read URI file {}", path.display()))?;
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn tail_lines(path: &Path, limit: usize) -> Result<Vec<String>> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open log file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut lines = VecDeque::with_capacity(limit);
    for line in reader.lines() {
        let line = line?;
        if lines.len() == limit {
            lines.pop_front();
        }
        lines.push_back(line);
    }
    Ok(lines.into_iter().collect())
}

fn scan_limit(limit: usize) -> usize {
    limit
        .saturating_mul(8)
        .max(DEFAULT_LOG_SCAN_LINES)
        .min(MAX_LOG_SCAN_LINES)
}

fn parse_log_event(line: &str) -> RuntimeLogEventView {
    match serde_json::from_str::<DaemonLogRecord>(line) {
        Ok(record) => runtime_log_event_view(record),
        Err(_) => RuntimeLogEventView {
            timestamp: None,
            level: None,
            message: line.to_string(),
            target: None,
            file: None,
            line_number: None,
            fields: None,
        },
    }
}

fn runtime_log_event_view(record: DaemonLogRecord) -> RuntimeLogEventView {
    RuntimeLogEventView {
        timestamp: record.timestamp,
        level: record.level,
        message: record
            .message
            .unwrap_or_else(|| "<missing message>".to_string()),
        target: record.target,
        file: record.filename,
        line_number: record.line_number,
        fields: (!record.extra.is_empty()).then_some(Value::Object(record.extra)),
    }
}

fn value_object_fields(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(other),
    }
}

fn matches_runtime_log(
    event: &RuntimeLogEventView,
    line: &str,
    level: Option<&str>,
    target: Option<&str>,
    contains: Option<&str>,
) -> bool {
    if level.is_some_and(|expected| {
        event
            .level
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some(expected)
    }) {
        return false;
    }
    if target.is_some_and(|expected| event.target.as_deref() != Some(expected)) {
        return false;
    }
    if contains.is_some_and(|needle| !log_contains(event, line, needle)) {
        return false;
    }
    true
}

fn log_contains(event: &RuntimeLogEventView, line: &str, needle: &str) -> bool {
    if line.to_ascii_lowercase().contains(needle) {
        return true;
    }
    event
        .fields
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains(needle)
}

fn is_timeline_event(event: &RuntimeLogEventView) -> bool {
    let message = event.message.as_str();
    matches!(
        message,
        "starting prism-mcp"
            | "building prism-mcp workspace server"
            | "opened prism sqlite store"
            | "loaded prism graph snapshot"
            | "loaded prism projection snapshot"
            | "prepared prism workspace indexer"
            | "starting prism workspace indexing"
            | "collected prism pending file parses"
            | "finished prism parse and update loop"
            | "finished prism missing-file removal phase"
            | "finished prism edge resolution phase"
            | "skipped prism index persistence batch because workspace state is unchanged"
            | "reanchored persisted prism memory"
            | "completed prism workspace indexing"
            | "built prism query state"
            | "built prism workspace session"
            | "built prism-mcp workspace server"
            | "prism-mcp daemon ready"
            | "prism-mcp workspace refresh"
    )
}

impl RuntimePaths {
    fn for_root(root: &Path) -> Self {
        Self {
            uri_file: root.join(".prism").join("prism-mcp-http-uri"),
            log_path: root.join(".prism").join("prism-mcp-daemon.log"),
            cache_path: root.join(".prism").join("cache.db"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ps_lines_for_runtime_status() {
        let process = parse_ps_line(
            "29267 1 4454352 02:12:24 /Users/bene/code/prism/target/release/prism-mcp --mode daemon --root /Users/bene/code/prism --http-uri-file /Users/bene/code/prism/.prism/prism-mcp-http-uri --http-path /mcp --health-path /healthz",
        )
        .expect("expected prism-mcp process");

        assert_eq!(process.pid, 29267);
        assert_eq!(process.ppid, 1);
        assert_eq!(process.rss_kb, 4454352);
        assert_eq!(process.elapsed, "02:12:24");
        assert_eq!(process.kind, McpProcessKind::Daemon);
        assert_eq!(process.health_path.as_deref(), Some("/healthz"));
        assert_eq!(process.http_uri, None);
    }

    #[test]
    fn parses_scoped_ps_lines_for_runtime_state_processes() {
        let (pid, ppid, rss_kb, elapsed, command) = parse_process_snapshot_line(
            "29267 1 4454352 02:12:24 /Users/bene/code/prism/target/release/prism-mcp --mode daemon",
        )
        .expect("expected process snapshot");

        assert_eq!(pid, 29267);
        assert_eq!(ppid, 1);
        assert_eq!(rss_kb, 4454352);
        assert_eq!(elapsed, "02:12:24");
        assert_eq!(
            command,
            "/Users/bene/code/prism/target/release/prism-mcp --mode daemon"
        );
    }

    #[test]
    fn parses_json_log_lines_into_runtime_events() {
        let event = parse_log_event(
            r#"{"timestamp":"2026-03-26T15:12:35Z","level":"INFO","message":"starting prism-mcp","target":"prism_mcp::logging","filename":"crates/prism-mcp/src/logging.rs","line_number":53,"mode":"daemon"}"#,
        );

        assert_eq!(event.timestamp.as_deref(), Some("2026-03-26T15:12:35Z"));
        assert_eq!(event.level.as_deref(), Some("INFO"));
        assert_eq!(event.message, "starting prism-mcp");
        assert_eq!(event.target.as_deref(), Some("prism_mcp::logging"));
        assert_eq!(
            event.file.as_deref(),
            Some("crates/prism-mcp/src/logging.rs")
        );
        assert_eq!(event.line_number, Some(53));
        assert_eq!(
            event.fields.as_ref().and_then(|value| value.get("mode")),
            Some(&Value::String("daemon".to_string()))
        );
    }

    #[test]
    fn runtime_timeline_filters_to_startup_and_refresh_events() {
        assert!(is_timeline_event(&RuntimeLogEventView {
            timestamp: Some("2026-03-26T15:12:35Z".to_string()),
            level: Some("INFO".to_string()),
            message: "completed prism workspace indexing".to_string(),
            target: Some("prism_core::indexer".to_string()),
            file: None,
            line_number: None,
            fields: None,
        }));
        assert!(!is_timeline_event(&RuntimeLogEventView {
            timestamp: Some("2026-03-26T15:12:35Z".to_string()),
            level: Some("WARN".to_string()),
            message: "response error".to_string(),
            target: Some("rmcp::service".to_string()),
            file: None,
            line_number: None,
            fields: None,
        }));
    }

    #[test]
    fn health_status_uses_process_state_instead_of_self_http_probe() {
        let daemon = McpProcess {
            pid: 29267,
            ppid: 1,
            rss_kb: 4454352,
            elapsed: "02:12:24".to_string(),
            command: "/Users/bene/code/prism/target/release/prism-mcp --mode daemon".to_string(),
            kind: McpProcessKind::Daemon,
            health_path: Some("/healthz".to_string()),
            http_uri: Some("http://127.0.0.1:52695/mcp".to_string()),
        };

        let healthy = health_status(
            &Some("http://127.0.0.1:52695/mcp".to_string()),
            &[daemon.clone()],
            None,
        );
        assert!(healthy.ok);
        assert_eq!(healthy.detail, "ok (http://127.0.0.1:52695/mcp)");

        let missing_daemon = health_status(&Some("http://127.0.0.1:9/mcp".to_string()), &[], None);
        assert!(!missing_daemon.ok);
        assert_eq!(
            missing_daemon.detail,
            "no daemon process for http://127.0.0.1:9/mcp"
        );

        let process_error = health_status(
            &Some("http://127.0.0.1:52695/mcp".to_string()),
            &[daemon],
            Some("ps failed"),
        );
        assert!(!process_error.ok);
        assert_eq!(process_error.detail, "process listing failed: ps failed");
    }

    #[test]
    fn classify_bridges_distinguishes_connected_idle_and_orphaned() {
        let bridges = vec![
            McpProcess {
                pid: 10,
                ppid: 1000,
                rss_kb: 1,
                elapsed: "00:01".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
            },
            McpProcess {
                pid: 11,
                ppid: 1001,
                rss_kb: 1,
                elapsed: "00:02".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
            },
            McpProcess {
                pid: 12,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:03".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
            },
        ];
        let connected = BTreeSet::from([10_u32]);

        let counts = classify_bridges(&bridges, &connected);

        assert_eq!(counts.connected, 1);
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.orphaned, 1);
        assert_eq!(
            bridge_state(&bridges[0], &connected).map(bridge_state_label),
            Some("connected".to_string())
        );
        assert_eq!(
            bridge_state(&bridges[1], &connected).map(bridge_state_label),
            Some("idle".to_string())
        );
        assert_eq!(
            bridge_state(&bridges[2], &connected).map(bridge_state_label),
            Some("orphaned".to_string())
        );
    }
}
