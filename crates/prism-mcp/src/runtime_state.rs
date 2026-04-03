use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use prism_core::{PrismPaths, WorkspaceSession};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::WorkspaceRefreshMetrics;
use crate::{current_timestamp, PrismMcpCli, PrismMcpFeatures, PrismMcpMode};

const MAX_RUNTIME_EVENTS: usize = 200;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct RuntimeState {
    pub(crate) processes: Vec<RuntimeProcessRecord>,
    pub(crate) events: Vec<RuntimeEventRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeProcessRecord {
    pub(crate) pid: u32,
    pub(crate) kind: String,
    pub(crate) started_at: u64,
    pub(crate) health_path: Option<String>,
    pub(crate) http_uri: Option<String>,
    pub(crate) upstream_uri: Option<String>,
    pub(crate) restart_nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeEventRecord {
    pub(crate) ts: u64,
    pub(crate) timestamp: String,
    pub(crate) level: String,
    pub(crate) message: String,
    pub(crate) target: String,
    pub(crate) file: Option<String>,
    pub(crate) line_number: Option<u64>,
    pub(crate) fields: Value,
}

pub(crate) fn default_runtime_state_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.mcp_runtime_state_path()
}

pub(crate) fn read_runtime_state(root: &Path) -> Result<Option<RuntimeState>> {
    let path = default_runtime_state_path(root)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read runtime state {}", path.display()))?;
    match serde_json::from_slice::<RuntimeState>(&bytes) {
        Ok(state) => Ok(Some(state)),
        Err(_) => Ok(None),
    }
}

pub(crate) fn record_process_start(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let kind = mode_name(cli.mode).to_string();
    let pid = std::process::id();
    update_runtime_state(root, |state| {
        state
            .processes
            .retain(|process| !(process.pid == pid && process.kind == kind));
        state.processes.push(RuntimeProcessRecord {
            pid,
            kind: kind.clone(),
            started_at: current_timestamp(),
            health_path: (cli.mode == PrismMcpMode::Daemon).then(|| cli.health_path.clone()),
            http_uri: None,
            upstream_uri: cli.upstream_uri.clone(),
            restart_nonce: None,
        });
        push_event(
            state,
            "INFO",
            "starting prism-mcp",
            "prism_mcp::logging",
            Some("crates/prism-mcp/src/logging.rs"),
            None,
            json!({
                "mode": kind,
                "coordination": cli.features().mode_label(),
                "root": root.display().to_string(),
                "httpBind": cli.http_bind,
                "httpPath": cli.http_path,
                "healthPath": cli.health_path,
            }),
        );
    })
}

pub(crate) fn record_process_exit(
    cli: &PrismMcpCli,
    root: &Path,
    error_chain: Option<&str>,
) -> Result<()> {
    let kind = mode_name(cli.mode).to_string();
    let pid = std::process::id();
    update_runtime_state(root, |state| {
        let process = remove_process(state, pid, &kind);
        push_event(
            state,
            if error_chain.is_some() {
                "ERROR"
            } else {
                "INFO"
            },
            if error_chain.is_some() {
                "prism-mcp exited with error"
            } else {
                "prism-mcp exited cleanly"
            },
            "prism_mcp::logging",
            Some("crates/prism-mcp/src/logging.rs"),
            None,
            json!({
                "mode": kind,
                "pid": pid,
                "process": process,
                "errorChain": error_chain,
            }),
        );
    })
}

pub(crate) fn record_process_panic(
    cli: &PrismMcpCli,
    root: &Path,
    panic_message: Option<&str>,
    location_file: Option<&str>,
    location_line: Option<u32>,
    location_column: Option<u32>,
) -> Result<()> {
    let kind = mode_name(cli.mode).to_string();
    let pid = std::process::id();
    update_runtime_state(root, |state| {
        let process = remove_process(state, pid, &kind);
        push_event(
            state,
            "ERROR",
            "prism-mcp panicked",
            "prism_mcp::logging",
            Some("crates/prism-mcp/src/logging.rs"),
            location_line.map(u64::from),
            json!({
                "mode": kind,
                "pid": pid,
                "process": process,
                "panicMessage": panic_message,
                "locationFile": location_file,
                "locationLine": location_line,
                "locationColumn": location_column,
            }),
        );
    })
}

pub(crate) fn record_workspace_server_built(
    root: &Path,
    features: &PrismMcpFeatures,
    node_count: usize,
    edge_count: usize,
    file_count: usize,
    build_ms: u128,
) -> Result<()> {
    update_runtime_state(root, |state| {
        push_event(
            state,
            "INFO",
            "built prism-mcp workspace server",
            "prism_mcp::lib",
            Some("crates/prism-mcp/src/lib.rs"),
            None,
            json!({
                "coordination": features.mode_label(),
                "nodeCount": node_count,
                "edgeCount": edge_count,
                "fileCount": file_count,
                "buildMs": build_ms,
            }),
        );
    })
}

pub(crate) fn record_daemon_ready(
    cli: &PrismMcpCli,
    root: &Path,
    http_uri: &str,
    health_path: &str,
    startup_ms: u128,
) -> Result<()> {
    update_runtime_state(root, |state| {
        for process in &mut state.processes {
            if process.pid == std::process::id() && process.kind == "daemon" {
                process.http_uri = Some(http_uri.to_string());
                process.health_path = Some(health_path.to_string());
                process.restart_nonce = cli.restart_nonce.clone();
            }
        }
        push_event(
            state,
            "INFO",
            "prism-mcp daemon ready",
            "prism_mcp::daemon_mode",
            Some("crates/prism-mcp/src/daemon_mode.rs"),
            None,
            json!({
                "httpUri": http_uri,
                "healthPath": health_path,
                "startupMs": startup_ms,
                "restartNonce": cli.restart_nonce.as_deref(),
            }),
        );
    })
}

pub(crate) fn record_bridge_upstream_resolved(
    root: &Path,
    upstream_uri: &str,
    resolution_source: &str,
    resolution_ms: u128,
    daemon_wait_ms: u128,
    spawned_daemon: bool,
) -> Result<()> {
    update_runtime_state(root, |state| {
        for process in &mut state.processes {
            if process.pid == std::process::id() && process.kind == "bridge" {
                process.upstream_uri = Some(upstream_uri.to_string());
            }
        }
        push_event(
            state,
            "INFO",
            "prism-mcp bridge resolved upstream",
            "prism_mcp::daemon_mode",
            Some("crates/prism-mcp/src/daemon_mode.rs"),
            None,
            json!({
                "upstreamUri": upstream_uri,
                "resolutionSource": resolution_source,
                "resolutionMs": resolution_ms,
                "daemonWaitMs": daemon_wait_ms,
                "spawnedDaemon": spawned_daemon,
            }),
        );
    })
}

pub(crate) fn record_bridge_connected_with_latency(
    root: &Path,
    upstream_uri: &str,
    connect_ms: Option<u128>,
) -> Result<()> {
    update_runtime_state(root, |state| {
        for process in &mut state.processes {
            if process.pid == std::process::id() && process.kind == "bridge" {
                process.upstream_uri = Some(upstream_uri.to_string());
            }
        }
        push_event(
            state,
            "INFO",
            "prism-mcp bridge connected",
            "prism_mcp::daemon_mode",
            Some("crates/prism-mcp/src/daemon_mode.rs"),
            None,
            json!({
                "upstreamUri": upstream_uri,
                "connectMs": connect_ms,
            }),
        );
    })
}

pub(crate) fn record_bridge_connection_failure(
    root: &Path,
    upstream_uri: &str,
    phase: &str,
    reason: &str,
    attempt: Option<usize>,
    delay_ms: Option<u128>,
    error: &str,
) -> Result<()> {
    update_runtime_state(root, |state| {
        push_event(
            state,
            "WARN",
            "prism-mcp bridge failed to connect to upstream",
            "prism_mcp::proxy_server",
            Some("crates/prism-mcp/src/proxy_server.rs"),
            None,
            json!({
                "upstreamUri": upstream_uri,
                "phase": phase,
                "reason": reason,
                "attempt": attempt,
                "delayMs": delay_ms,
                "error": error,
            }),
        );
    })
}

pub(crate) fn record_workspace_refresh(
    root: &Path,
    refresh_path: &str,
    workspace: &WorkspaceSession,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    workspace_revision: Option<u64>,
    loaded_workspace_revision: u64,
    episodic_revision: Option<u64>,
    loaded_episodic_revision: u64,
    inference_revision: Option<u64>,
    loaded_inference_revision: u64,
    coordination_revision: Option<u64>,
    loaded_coordination_revision: u64,
    duration_ms: u128,
    metrics: WorkspaceRefreshMetrics,
) -> Result<()> {
    update_runtime_state(root, |state| {
        push_event(
            state,
            "INFO",
            "prism-mcp workspace refresh",
            "prism_mcp::lib",
            Some("crates/prism-mcp/src/lib.rs"),
            None,
            json!({
                "refreshPath": refresh_path,
                "fsObserved": workspace.observed_fs_revision(),
                "fsApplied": workspace.applied_fs_revision(),
                "episodicReloaded": episodic_reloaded,
                "inferenceReloaded": inference_reloaded,
                "coordinationReloaded": coordination_reloaded,
                "lockWaitMs": metrics.lock_wait_ms,
                "lockHoldMs": metrics.lock_hold_ms,
                "fsRefreshMs": metrics.fs_refresh_ms,
                "planRefreshMs": metrics.plan_refresh_ms,
                "buildIndexerMs": metrics.build_indexer_ms,
                "indexWorkspaceMs": metrics.index_workspace_ms,
                "publishGenerationMs": metrics.publish_generation_ms,
                "assistedLeaseMs": metrics.assisted_lease_ms,
                "curatorEnqueueMs": metrics.curator_enqueue_ms,
                "attachColdQueryBackendsMs": metrics.attach_cold_query_backends_ms,
                "finalizeRefreshStateMs": metrics.finalize_refresh_state_ms,
                "snapshotRevisionsMs": metrics.snapshot_revisions_ms,
                "loadEpisodicMs": metrics.load_episodic_ms,
                "loadInferenceMs": metrics.load_inference_ms,
                "loadCoordinationMs": metrics.load_coordination_ms,
                "loadedBytes": metrics.loaded_bytes,
                "replayVolume": metrics.replay_volume,
                "fullRebuildCount": metrics.full_rebuild_count,
                "workspaceReloaded": metrics.workspace_reloaded,
                "workspaceRevision": workspace_revision,
                "loadedWorkspaceRevision": loaded_workspace_revision,
                "episodicRevision": episodic_revision,
                "loadedEpisodicRevision": loaded_episodic_revision,
                "inferenceRevision": inference_revision,
                "loadedInferenceRevision": loaded_inference_revision,
                "coordinationRevision": coordination_revision,
                "loadedCoordinationRevision": loaded_coordination_revision,
                "durationMs": duration_ms,
            }),
        );
    })
}

fn update_runtime_state(root: &Path, update: impl FnOnce(&mut RuntimeState)) -> Result<()> {
    let path = default_runtime_state_path(root)?;
    let mut state = read_runtime_state(root)?.unwrap_or_default();
    update(&mut state);
    observe_dead_processes(&mut state);
    dedupe_processes(&mut state.processes);
    trim_events(&mut state.events);
    write_runtime_state(&path, &state)
}

fn write_runtime_state(path: &Path, state: &RuntimeState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create runtime state dir {}", parent.display()))?;
    }
    let temp_path = runtime_state_temp_path(path);
    let bytes = serde_json::to_vec_pretty(state).context("failed to serialize runtime state")?;
    fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write runtime state {}", temp_path.display()))?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to move runtime state {} into place at {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn runtime_state_temp_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_extension(format!("json.{}.{}.tmp", std::process::id(), nonce))
}

fn push_event(
    state: &mut RuntimeState,
    level: &str,
    message: &str,
    target: &str,
    file: Option<&str>,
    line_number: Option<u64>,
    fields: Value,
) {
    let ts = current_timestamp();
    state.events.push(RuntimeEventRecord {
        ts,
        timestamp: ts.to_string(),
        level: level.to_string(),
        message: message.to_string(),
        target: target.to_string(),
        file: file.map(ToString::to_string),
        line_number,
        fields,
    });
}

fn trim_events(events: &mut Vec<RuntimeEventRecord>) {
    if events.len() > MAX_RUNTIME_EVENTS {
        let start = events.len() - MAX_RUNTIME_EVENTS;
        events.drain(0..start);
    }
}

fn dedupe_processes(processes: &mut Vec<RuntimeProcessRecord>) {
    let mut seen = BTreeSet::new();
    processes.retain(|process| seen.insert((process.pid, process.kind.clone())));
}

fn remove_process(state: &mut RuntimeState, pid: u32, kind: &str) -> Option<RuntimeProcessRecord> {
    let index = state
        .processes
        .iter()
        .position(|process| process.pid == pid && process.kind == kind)?;
    Some(state.processes.remove(index))
}

pub(crate) fn process_is_live(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(code) if code == libc::EPERM => true,
        Some(code) if code == libc::ESRCH => false,
        _ => false,
    }
}

fn observe_dead_processes(state: &mut RuntimeState) {
    let mut dead = Vec::new();
    state.processes.retain(|process| {
        let live = process_is_live(process.pid);
        if !live {
            dead.push(process.clone());
        }
        live
    });
    for process in dead {
        push_event(
            state,
            "WARN",
            "prism-mcp observed dead runtime process",
            "prism_mcp::runtime_state",
            Some("crates/prism-mcp/src/runtime_state.rs"),
            None,
            json!({
                "process": process,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        default_runtime_state_path, process_is_live, read_runtime_state,
        record_bridge_connection_failure, record_daemon_ready, record_process_exit,
        record_process_start, runtime_state_temp_path, RuntimeProcessRecord, RuntimeState,
    };
    use crate::{PrismMcpCli, PrismMcpMode};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = env::temp_dir().join(format!(
            "prism-mcp-runtime-state-tests-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn invalid_runtime_state_is_treated_as_missing_state() {
        let root = test_dir("invalid-state");
        let path = default_runtime_state_path(&root).unwrap();
        fs::write(path, "{ invalid").unwrap();

        let state = read_runtime_state(&root).unwrap();
        assert!(state.is_none());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn runtime_state_temp_paths_are_unique_per_call() {
        let root = test_dir("temp-path");
        let path = root.join("prism-mcp-runtime.json");

        let first = runtime_state_temp_path(&path);
        let second = runtime_state_temp_path(&path);

        assert_ne!(first, second);
        assert_ne!(first.extension(), Some(std::ffi::OsStr::new("json")));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn daemon_ready_persists_restart_nonce() {
        let root = test_dir("restart-nonce");
        let cli = PrismMcpCli {
            root: root.clone(),
            mode: PrismMcpMode::Daemon,
            no_coordination: false,
            internal_developer: false,
            ui: false,
            enable_coordination: Vec::new(),
            disable_coordination: Vec::new(),
            enable_query_view: Vec::new(),
            disable_query_view: Vec::new(),
            daemon_log: None,
            shared_runtime_sqlite: None,
            shared_runtime_uri: None,
            restart_nonce: Some("restart-1".to_string()),
            daemon_start_timeout_ms: None,
            http_bind: "127.0.0.1:0".to_string(),
            http_path: "/mcp".to_string(),
            health_path: "/healthz".to_string(),
            http_uri_file: None,
            upstream_uri: None,
            bootstrap_build_worktree_release: false,
            bridge_daemon_binary: None,
            daemonize: false,
        };

        record_process_start(&cli, &root).unwrap();
        record_daemon_ready(&cli, &root, "http://127.0.0.1:41000/mcp", "/healthz", 1).unwrap();

        let state = read_runtime_state(&root)
            .unwrap()
            .expect("runtime state should exist");
        let daemon = state
            .processes
            .into_iter()
            .find(|process| process.pid == std::process::id() && process.kind == "daemon")
            .expect("daemon process should be recorded");
        assert_eq!(daemon.restart_nonce.as_deref(), Some("restart-1"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn process_is_live_recognizes_current_process() {
        assert!(process_is_live(std::process::id()));
    }

    #[test]
    fn record_process_start_prunes_dead_runtime_process_records() {
        let root = test_dir("prune-dead-processes");
        let path = default_runtime_state_path(&root).unwrap();
        let stale_pid = 999_999_u32;
        fs::write(
            &path,
            serde_json::to_vec(&RuntimeState {
                processes: vec![RuntimeProcessRecord {
                    pid: stale_pid,
                    kind: "daemon".to_string(),
                    started_at: 1,
                    health_path: Some("/healthz".to_string()),
                    http_uri: Some("http://127.0.0.1:41000/mcp".to_string()),
                    upstream_uri: None,
                    restart_nonce: Some("old".to_string()),
                }],
                events: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();

        let cli = PrismMcpCli {
            root: root.clone(),
            mode: PrismMcpMode::Daemon,
            no_coordination: false,
            internal_developer: false,
            ui: false,
            enable_coordination: Vec::new(),
            disable_coordination: Vec::new(),
            enable_query_view: Vec::new(),
            disable_query_view: Vec::new(),
            daemon_log: None,
            shared_runtime_sqlite: None,
            shared_runtime_uri: None,
            restart_nonce: Some("restart-2".to_string()),
            daemon_start_timeout_ms: None,
            http_bind: "127.0.0.1:0".to_string(),
            http_path: "/mcp".to_string(),
            health_path: "/healthz".to_string(),
            http_uri_file: None,
            upstream_uri: None,
            bootstrap_build_worktree_release: false,
            bridge_daemon_binary: None,
            daemonize: false,
        };

        record_process_start(&cli, &root).unwrap();

        let state = read_runtime_state(&root).unwrap().unwrap();
        assert!(state
            .processes
            .iter()
            .all(|process| process.pid != stale_pid));
        assert!(state.events.iter().any(|event| {
            event.message == "prism-mcp observed dead runtime process"
                && event.fields["process"]["pid"] == stale_pid
        }));
        assert!(state
            .processes
            .iter()
            .any(|process| process.pid == std::process::id() && process.kind == "daemon"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn record_process_exit_removes_current_process_and_persists_event() {
        let root = test_dir("process-exit");
        let cli = PrismMcpCli {
            root: root.clone(),
            mode: PrismMcpMode::Bridge,
            no_coordination: false,
            internal_developer: false,
            ui: false,
            enable_coordination: Vec::new(),
            disable_coordination: Vec::new(),
            enable_query_view: Vec::new(),
            disable_query_view: Vec::new(),
            daemon_log: None,
            shared_runtime_sqlite: None,
            shared_runtime_uri: None,
            restart_nonce: None,
            daemon_start_timeout_ms: None,
            http_bind: "127.0.0.1:0".to_string(),
            http_path: "/mcp".to_string(),
            health_path: "/healthz".to_string(),
            http_uri_file: None,
            upstream_uri: Some("http://127.0.0.1:41000/mcp".to_string()),
            bootstrap_build_worktree_release: false,
            bridge_daemon_binary: None,
            daemonize: false,
        };

        record_process_start(&cli, &root).unwrap();
        record_process_exit(&cli, &root, None).unwrap();

        let state = read_runtime_state(&root).unwrap().unwrap();
        assert!(!state
            .processes
            .iter()
            .any(|process| process.pid == std::process::id() && process.kind == "bridge"));
        assert!(state.events.iter().any(|event| {
            event.message == "prism-mcp exited cleanly" && event.fields["pid"] == std::process::id()
        }));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn bridge_connection_failure_records_warning_and_prunes_dead_daemon() {
        let root = test_dir("bridge-connection-failure");
        let path = default_runtime_state_path(&root).unwrap();
        let stale_pid = 999_998_u32;
        fs::write(
            &path,
            serde_json::to_vec(&RuntimeState {
                processes: vec![RuntimeProcessRecord {
                    pid: stale_pid,
                    kind: "daemon".to_string(),
                    started_at: 1,
                    health_path: Some("/healthz".to_string()),
                    http_uri: Some("http://127.0.0.1:41000/mcp".to_string()),
                    upstream_uri: None,
                    restart_nonce: Some("restart-3".to_string()),
                }],
                events: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();

        record_bridge_connection_failure(
            &root,
            "http://127.0.0.1:41000/mcp",
            "reconnect",
            "upstream transport closed before request",
            Some(2),
            Some(500),
            "failed to connect to upstream PRISM MCP server",
        )
        .unwrap();

        let state = read_runtime_state(&root).unwrap().unwrap();
        assert!(state.processes.is_empty());
        assert!(state.events.iter().any(|event| {
            event.message == "prism-mcp bridge failed to connect to upstream"
                && event.fields["phase"] == "reconnect"
                && event.fields["attempt"] == 2
        }));
        assert!(state.events.iter().any(|event| {
            event.message == "prism-mcp observed dead runtime process"
                && event.fields["process"]["pid"] == stale_pid
        }));
        fs::remove_dir_all(root).ok();
    }
}

fn mode_name(mode: PrismMcpMode) -> &'static str {
    match mode {
        PrismMcpMode::Stdio => "stdio",
        PrismMcpMode::Daemon => "daemon",
        PrismMcpMode::Bridge => "bridge",
    }
}
