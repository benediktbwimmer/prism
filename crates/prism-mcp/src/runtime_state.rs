use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use prism_core::WorkspaceSession;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

pub(crate) fn default_runtime_state_path(root: &Path) -> PathBuf {
    root.join(".prism").join("prism-mcp-runtime.json")
}

pub(crate) fn read_runtime_state(root: &Path) -> Result<Option<RuntimeState>> {
    let path = default_runtime_state_path(root);
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

pub(crate) fn record_workspace_server_built(
    root: &Path,
    features: &PrismMcpFeatures,
    node_count: usize,
    edge_count: usize,
    file_count: usize,
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
            }),
        );
    })
}

pub(crate) fn record_daemon_ready(
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

pub(crate) fn record_workspace_refresh(
    root: &Path,
    refresh_path: &str,
    workspace: &WorkspaceSession,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    duration_ms: u128,
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
                "durationMs": duration_ms,
            }),
        );
    })
}

fn update_runtime_state(root: &Path, update: impl FnOnce(&mut RuntimeState)) -> Result<()> {
    let path = default_runtime_state_path(root);
    let mut state = read_runtime_state(root)?.unwrap_or_default();
    update(&mut state);
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{read_runtime_state, runtime_state_temp_path};

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
        let prism_dir = root.join(".prism");
        fs::create_dir_all(&prism_dir).unwrap();
        fs::write(prism_dir.join("prism-mcp-runtime.json"), "{ invalid").unwrap();

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
}

fn mode_name(mode: PrismMcpMode) -> &'static str {
    match mode {
        PrismMcpMode::Stdio => "stdio",
        PrismMcpMode::Daemon => "daemon",
        PrismMcpMode::Bridge => "bridge",
    }
}
