use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use prism_core::{shared_coordination_ref_diagnostics, sync_live_runtime_descriptor, PrismPaths};

use crate::cli::McpCommand;
use crate::daemon_log;

const START_TIMEOUT: Duration = Duration::from_secs(180);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const RESTART_GRACE_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_HEALTH_PATH: &str = "/healthz";
const DEFAULT_HTTP_BIND_HOST: &str = "127.0.0.1";
// Keep this in sync with prism-mcp's stable port selection so the CLI can
// pass an explicit deterministic bind before the daemon starts.
const STABLE_HTTP_PORT_BASE: u16 = 41_000;
const STABLE_HTTP_PORT_RANGE: u16 = 20_000;

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

#[derive(Debug, Clone)]
struct McpPaths {
    uri_file: PathBuf,
    public_url_file: PathBuf,
    log_path: PathBuf,
    cache_path: PathBuf,
    runtime_state_path: PathBuf,
    startup_marker: PathBuf,
}

#[derive(Debug, Clone)]
struct HealthStatus {
    ok: bool,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupMarker {
    operation: String,
    nonce: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeDaemonRecord {
    pid: u32,
    health_path: Option<String>,
    http_uri: Option<String>,
    restart_nonce: Option<String>,
}

#[derive(Debug, Clone)]
struct DaemonConnectionInfo {
    transport: &'static str,
    mode: &'static str,
    bridge_role: &'static str,
    uri: Option<String>,
    health_uri: Option<String>,
    health: HealthStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortOwner {
    pid: u32,
    command: String,
}

struct StartupMarkerGuard {
    path: PathBuf,
}

impl StartupMarkerGuard {
    fn try_create(path: &Path, operation: &str, nonce: Option<&str>) -> Result<Option<Self>> {
        loop {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut file) => {
                    let payload = serde_json::to_vec(&serde_json::json!({
                        "operation": operation,
                        "nonce": nonce,
                    }))
                    .context("failed to serialize startup marker")?;
                    file.write_all(&payload).with_context(|| {
                        format!("failed to write startup marker {}", path.display())
                    })?;
                    return Ok(Some(Self {
                        path: path.to_path_buf(),
                    }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if read_startup_marker(path)?.is_some() {
                        return Ok(None);
                    }
                    fs::remove_file(path).ok();
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to create startup marker {}", path.display())
                    });
                }
            }
        }
    }
}

impl Drop for StartupMarkerGuard {
    fn drop(&mut self) {
        fs::remove_file(&self.path).ok();
    }
}

pub(crate) fn handle(root: &Path, command: McpCommand) -> Result<()> {
    let root = root.canonicalize()?;
    match command {
        McpCommand::Status => status(&root),
        McpCommand::Endpoint => endpoint(&root),
        McpCommand::PublicUrl { url, clear } => public_url(&root, url.as_deref(), clear),
        McpCommand::Cleanup => cleanup(&root),
        McpCommand::Bridge {
            no_coordination,
            internal_developer,
            shared_runtime_sqlite,
            shared_runtime_uri,
            bootstrap_build_worktree_release,
            bridge_daemon_binary,
        } => bridge(
            &root,
            no_coordination,
            internal_developer,
            shared_runtime_sqlite,
            shared_runtime_uri,
            bootstrap_build_worktree_release,
            bridge_daemon_binary,
        ),
        McpCommand::Start {
            no_coordination,
            internal_developer,
            ui,
            http_bind,
            shared_runtime_sqlite,
            shared_runtime_uri,
        } => start(
            &root,
            no_coordination,
            internal_developer,
            ui,
            http_bind,
            shared_runtime_sqlite,
            shared_runtime_uri,
            "start",
            None,
            None,
        ),
        McpCommand::Stop { kill_bridges } => stop(&root, kill_bridges),
        McpCommand::Restart {
            kill_bridges,
            no_coordination,
            internal_developer,
            ui,
            http_bind,
            shared_runtime_sqlite,
            shared_runtime_uri,
        } => {
            let paths = McpPaths::for_root(&root)?;
            let restart_nonce = next_restart_nonce();
            let Some(startup_marker) = StartupMarkerGuard::try_create(
                &paths.startup_marker,
                "restart",
                Some(&restart_nonce),
            )?
            else {
                let uri = wait_for_healthy_uri(&root, &paths, DEFAULT_HEALTH_PATH)?;
                println!("daemon startup already in progress");
                println!("uri: {uri}");
                return status(&root);
            };
            stop_impl(&root, kill_bridges, false)?;
            start(
                &root,
                no_coordination,
                internal_developer,
                ui,
                http_bind,
                shared_runtime_sqlite,
                shared_runtime_uri,
                "restart",
                Some(&restart_nonce),
                Some(startup_marker),
            )
        }
        McpCommand::Health => health(&root),
        McpCommand::Logs { lines } => logs(&root, lines),
    }
}

fn bridge(
    root: &Path,
    no_coordination: bool,
    internal_developer: bool,
    shared_runtime_sqlite: Option<PathBuf>,
    shared_runtime_uri: Option<String>,
    bootstrap_build_worktree_release: bool,
    bridge_daemon_binary: Option<PathBuf>,
) -> Result<()> {
    if shared_runtime_sqlite.is_some() && shared_runtime_uri.is_some() {
        bail!("configure either shared runtime sqlite or shared runtime uri, not both");
    }

    let paths = McpPaths::for_root(root)?;
    let binary = prism_mcp_binary()?;
    let args = bridge_exec_args(
        root,
        &paths,
        no_coordination,
        internal_developer,
        shared_runtime_sqlite.as_deref(),
        shared_runtime_uri.as_deref(),
        bootstrap_build_worktree_release,
        bridge_daemon_binary.as_deref(),
    );
    exec_bridge(binary, &args)
}

fn status(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let (processes, connection) = connection_snapshot_with_restart_grace(root, &paths)?;
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let bridges = select_kind(&processes, McpProcessKind::Bridge);
    let uri = connection.uri.clone();
    let connected_bridge_pids = uri
        .as_deref()
        .and_then(|uri| connected_process_ids_for_uri(uri).ok())
        .unwrap_or_default();
    let bridge_counts = classify_bridges(&bridges, &connected_bridge_pids);

    println!("root: {}", root.display());
    println!("daemon_count: {}", daemons.len());
    if let Some(daemon) = daemons.first() {
        println!("daemon_pid: {}", daemon.pid);
        println!("daemon_rss_mb: {:.1}", daemon.rss_kb as f64 / 1024.0);
        println!("daemon_elapsed: {}", daemon.elapsed);
    }
    println!("bridge_count: {}", bridges.len());
    println!("connected_bridge_count: {}", bridge_counts.connected);
    println!("idle_bridge_count: {}", bridge_counts.idle);
    println!("orphan_bridge_count: {}", bridge_counts.orphaned);
    println!("preferred_connection_mode: {}", connection.mode);
    println!("preferred_transport: {}", connection.transport);
    println!("uri_file: {}", paths.uri_file.display());
    println!("uri: {}", uri.as_deref().unwrap_or("<missing>"));
    println!(
        "public_url: {}",
        read_public_url(&paths)?.as_deref().unwrap_or("<unset>")
    );
    println!(
        "health_uri: {}",
        connection.health_uri.as_deref().unwrap_or("<missing>")
    );
    println!("health: {}", connection.health.detail);
    println!("bridge_role: {}", connection.bridge_role);
    if let Ok(bytes) = daemon_log::total_log_bytes(&paths.log_path) {
        println!("log_path: {} ({} bytes)", paths.log_path.display(), bytes);
    } else {
        println!("log_path: {} (missing)", paths.log_path.display());
    }
    if let Ok(metadata) = fs::metadata(&paths.cache_path) {
        println!(
            "cache_path: {} ({} bytes)",
            paths.cache_path.display(),
            metadata.len()
        );
    } else {
        println!("cache_path: {} (missing)", paths.cache_path.display());
    }
    if let Some(shared_coordination_ref) = shared_coordination_ref_diagnostics(root)? {
        println!(
            "shared_coordination_ref: {}",
            shared_coordination_ref.ref_name
        );
        println!(
            "shared_coordination_head: {}",
            shared_coordination_ref
                .head_commit
                .as_deref()
                .unwrap_or("<missing>")
        );
        println!(
            "shared_coordination_history_depth: {}",
            shared_coordination_ref.history_depth
        );
        println!(
            "shared_coordination_snapshot_file_count: {}",
            shared_coordination_ref.snapshot_file_count
        );
        println!(
            "shared_coordination_compaction_status: {}",
            shared_coordination_ref.compaction_status
        );
        println!(
            "shared_coordination_needs_compaction: {}",
            shared_coordination_ref.needs_compaction
        );
    }
    if daemons.len() > 1 {
        println!("warning: multiple daemon processes are running for this workspace");
    }
    if bridge_counts.orphaned > 0 {
        println!("warning: orphaned bridge processes are running for this workspace");
        println!("hint: run `prism mcp cleanup` to reap orphaned bridges");
    }
    if bridge_counts.idle > 0 {
        println!(
            "note: idle bridge processes are disconnected from the current daemon and do not count toward active daemon load"
        );
    }
    if daemons.is_empty() && !bridges.is_empty() {
        println!("warning: bridge processes exist without a daemon");
    }
    Ok(())
}

fn public_url(root: &Path, url: Option<&str>, clear: bool) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    if clear && url.is_some() {
        bail!("provide either a public URL or --clear, not both");
    }
    if clear {
        clear_public_url(&paths)?;
        sync_live_runtime_descriptor(root)?;
        println!("cleared public_url");
        return Ok(());
    }
    if let Some(url) = url {
        let url = normalize_public_url(url)?;
        write_public_url(&paths, &url)?;
        sync_live_runtime_descriptor(root)?;
        println!("public_url: {url}");
        return Ok(());
    }
    println!(
        "{}",
        read_public_url(&paths)?.as_deref().unwrap_or("<unset>")
    );
    Ok(())
}

fn endpoint(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let processes = list_processes(root)?;
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let connection = daemon_connection_info(&paths, &daemons)?;
    let Some(uri) = connection.uri else {
        bail!(
            "PRISM MCP daemon endpoint is unavailable; start the daemon first with `prism-cli mcp start` or `prism-cli mcp restart`"
        );
    };
    println!("{uri}");
    Ok(())
}

fn cleanup(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let processes = list_processes(root)?;
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let bridges = select_kind(&processes, McpProcessKind::Bridge);
    let uri = resolve_daemon_uri(&paths, &daemons)?;
    let connected_bridge_pids = uri
        .as_deref()
        .and_then(|uri| connected_process_ids_for_uri(uri).ok())
        .unwrap_or_default();
    let stale = cleanup_candidate_bridges(&bridges, &connected_bridge_pids);
    if stale.is_empty() {
        println!("no orphaned bridge processes found");
        return Ok(());
    }
    reap_processes(root, &stale)?;
    println!("reaped {} orphaned bridge process(es)", stale.len());
    Ok(())
}

fn start(
    root: &Path,
    no_coordination: bool,
    internal_developer: bool,
    ui: bool,
    http_bind: Option<String>,
    shared_runtime_sqlite: Option<PathBuf>,
    shared_runtime_uri: Option<String>,
    operation: &str,
    restart_nonce: Option<&str>,
    startup_marker: Option<StartupMarkerGuard>,
) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let mut processes = list_processes(root)?;
    let orphaned = orphaned_bridges(&processes);
    if !orphaned.is_empty() {
        reap_processes(root, &orphaned)?;
        println!("reaped {} orphaned bridge process(es)", orphaned.len());
        processes = list_processes(root)?;
    }
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    if !daemons.is_empty() {
        let uri = read_uri_file(&paths.uri_file)?;
        let health = health_status(&uri, daemon_health_path(&daemons))?;
        if health.ok {
            println!("daemon already running");
            return status(root);
        }
        bail!("stale daemon detected; use `prism mcp restart`");
    }

    if let Some(parent) = paths.log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let http_bind = resolve_http_bind(root, http_bind.as_deref());
    let _startup_marker = match startup_marker {
        Some(startup_marker) => startup_marker,
        None => {
            let Some(startup_marker) =
                StartupMarkerGuard::try_create(&paths.startup_marker, operation, restart_nonce)?
            else {
                let uri = wait_for_healthy_uri(root, &paths, DEFAULT_HEALTH_PATH)?;
                println!("daemon startup already in progress");
                println!("uri: {uri}");
                return status(root);
            };
            startup_marker
        }
    };
    fs::remove_file(&paths.uri_file).ok();
    ensure_expected_bind_available(root, &http_bind, operation)?;

    let binary = prism_mcp_binary()?;
    spawn_daemon(
        root,
        &binary,
        &paths,
        &http_bind,
        no_coordination,
        internal_developer,
        ui,
        shared_runtime_sqlite.as_deref(),
        shared_runtime_uri.as_deref(),
        restart_nonce,
    )?;
    let uri = wait_for_healthy_uri(root, &paths, DEFAULT_HEALTH_PATH)?;
    println!("started daemon");
    println!("uri: {uri}");
    status(root)
}

fn stop(root: &Path, kill_bridges: bool) -> Result<()> {
    stop_impl(root, kill_bridges, true)
}

fn stop_impl(root: &Path, kill_bridges: bool, clear_startup_marker: bool) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let processes = list_processes(root)?;
    let mut targets = select_kind(&processes, McpProcessKind::Daemon);
    if kill_bridges {
        targets.extend(select_kind(&processes, McpProcessKind::Bridge));
    }

    if targets.is_empty() {
        println!("no matching prism-mcp processes found");
        fs::remove_file(&paths.uri_file).ok();
        if clear_startup_marker {
            fs::remove_file(&paths.startup_marker).ok();
        }
        return Ok(());
    }

    signal_processes(&targets, "-TERM")?;
    wait_for_exit(root, &targets, STOP_TIMEOUT)?;

    let remaining = list_processes(root)?
        .into_iter()
        .filter(|process| targets.iter().any(|target| target.pid == process.pid))
        .collect::<Vec<_>>();
    if !remaining.is_empty() {
        signal_processes(&remaining, "-KILL")?;
        wait_for_exit(root, &remaining, Duration::from_secs(2))?;
    }

    if list_processes(root)?
        .into_iter()
        .all(|process| process.kind != McpProcessKind::Daemon)
    {
        fs::remove_file(&paths.uri_file).ok();
        if clear_startup_marker {
            fs::remove_file(&paths.startup_marker).ok();
        }
    }

    println!("stopped {} process(es)", targets.len());
    Ok(())
}

fn health(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let (_, connection) = connection_snapshot_with_restart_grace(root, &paths)?;
    let health = connection.health;
    println!("{}", health.detail);
    if !health.ok {
        bail!("daemon is not healthy");
    }
    Ok(())
}

fn logs(root: &Path, lines: usize) -> Result<()> {
    let paths = McpPaths::for_root(root)?;
    let lines = daemon_log::tail_lines(&paths.log_path, lines)?;
    if lines.is_empty() {
        println!("log file is empty");
        return Ok(());
    }
    for line in lines {
        println!("{line}");
    }
    Ok(())
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

fn parse_ps_line(line: &str) -> Option<McpProcess> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse::<u32>().ok()?;
    let rss_kb = parts.next()?.parse().ok()?;
    let elapsed = parts.next()?.to_string();
    let command = parts.collect::<Vec<_>>().join(" ");
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
        health_path: command_option_value(&command, "--health-path"),
        command,
        kind,
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

fn orphaned_bridges(processes: &[McpProcess]) -> Vec<McpProcess> {
    processes
        .iter()
        .filter(|process| process.kind == McpProcessKind::Bridge && process.ppid == 1)
        .cloned()
        .collect()
}

fn cleanup_candidate_bridges(
    processes: &[McpProcess],
    connected_bridge_pids: &BTreeSet<u32>,
) -> Vec<McpProcess> {
    processes
        .iter()
        .filter(|process| {
            matches!(
                bridge_state(process, connected_bridge_pids),
                Some(BridgeState::Orphaned)
            )
        })
        .cloned()
        .collect()
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

fn join_health_uri(uri: &str, health_path: &str) -> String {
    let base = uri
        .split_once("://")
        .map(|(scheme, rest)| {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}")
        })
        .unwrap_or_else(|| uri.to_string());
    format!(
        "{}{}",
        base.trim_end_matches('/'),
        normalize_route_path(health_path)
    )
}

fn normalize_route_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        DEFAULT_HEALTH_PATH.to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn command_option_value(command: &str, option: &str) -> Option<String> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens
        .windows(2)
        .find_map(|window| (window[0] == option).then(|| window[1].to_string()))
}

fn bridge_exec_args(
    root: &Path,
    paths: &McpPaths,
    no_coordination: bool,
    internal_developer: bool,
    shared_runtime_sqlite: Option<&Path>,
    shared_runtime_uri: Option<&str>,
    bootstrap_build_worktree_release: bool,
    bridge_daemon_binary: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--mode"),
        OsString::from("bridge"),
        OsString::from("--root"),
        root.as_os_str().to_os_string(),
        OsString::from("--http-uri-file"),
        paths.uri_file.as_os_str().to_os_string(),
    ];
    if no_coordination {
        args.push(OsString::from("--no-coordination"));
    }
    if internal_developer {
        args.push(OsString::from("--internal-developer"));
    }
    if let Some(shared_runtime_sqlite) = shared_runtime_sqlite {
        args.push(OsString::from("--shared-runtime-sqlite"));
        args.push(shared_runtime_sqlite.as_os_str().to_os_string());
    }
    if let Some(shared_runtime_uri) = shared_runtime_uri {
        args.push(OsString::from("--shared-runtime-uri"));
        args.push(OsString::from(shared_runtime_uri));
    }
    if bootstrap_build_worktree_release {
        args.push(OsString::from("--bootstrap-build-worktree-release"));
    }
    if let Some(bridge_daemon_binary) = bridge_daemon_binary {
        args.push(OsString::from("--bridge-daemon-binary"));
        args.push(bridge_daemon_binary.as_os_str().to_os_string());
    }
    args
}

#[cfg(unix)]
fn exec_bridge(binary: PathBuf, args: &[OsString]) -> Result<()> {
    let error = Command::new(&binary).args(args).exec();
    Err(error).with_context(|| format!("failed to exec bridge via {}", binary.display()))
}

#[cfg(not(unix))]
fn exec_bridge(binary: PathBuf, args: &[OsString]) -> Result<()> {
    let status = Command::new(&binary)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn bridge via {}", binary.display()))?;
    if status.success() {
        Ok(())
    } else {
        bail!("bridge exited with status {status}");
    }
}

fn prism_mcp_binary() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to resolve current executable path")?;
    let bin_dir = current
        .parent()
        .ok_or_else(|| anyhow!("current executable has no parent directory"))?;
    let mut candidates = Vec::new();
    if let Some(target_dir) = bin_dir.parent() {
        candidates.push(target_dir.join("release").join("prism-mcp"));
    }
    candidates.push(bin_dir.join("prism-mcp"));

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "could not locate sibling prism-mcp binary near {}",
        current.display()
    );
}

fn spawn_daemon(
    root: &Path,
    binary: &Path,
    paths: &McpPaths,
    http_bind: &str,
    no_coordination: bool,
    internal_developer: bool,
    ui: bool,
    shared_runtime_sqlite: Option<&Path>,
    shared_runtime_uri: Option<&str>,
    restart_nonce: Option<&str>,
) -> Result<()> {
    if shared_runtime_sqlite.is_some() && shared_runtime_uri.is_some() {
        bail!("configure either shared runtime sqlite or shared runtime uri, not both");
    }
    let mut args = vec![
        "--mode".to_string(),
        "daemon".to_string(),
        "--daemonize".to_string(),
        "--root".to_string(),
        root.display().to_string(),
        "--http-bind".to_string(),
        http_bind.to_string(),
        "--http-uri-file".to_string(),
        paths.uri_file.display().to_string(),
        "--daemon-log".to_string(),
        paths.log_path.display().to_string(),
        "--http-path".to_string(),
        "/mcp".to_string(),
        "--health-path".to_string(),
        DEFAULT_HEALTH_PATH.to_string(),
    ];
    if no_coordination {
        args.push("--no-coordination".to_string());
    }
    if internal_developer {
        args.push("--internal-developer".to_string());
    }
    if ui {
        args.push("--ui".to_string());
    }
    if let Some(shared_runtime_sqlite) = shared_runtime_sqlite {
        args.push("--shared-runtime-sqlite".to_string());
        args.push(shared_runtime_sqlite.display().to_string());
    }
    if let Some(shared_runtime_uri) = shared_runtime_uri {
        args.push("--shared-runtime-uri".to_string());
        args.push(shared_runtime_uri.to_string());
    }
    if let Some(restart_nonce) = restart_nonce {
        args.push("--restart-nonce".to_string());
        args.push(restart_nonce.to_string());
    }

    let child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn daemon via {}", binary.display()))?;

    let pid = child.id();
    daemon_log::append_log_line(
        &paths.log_path,
        &format!(
            "{} prism-cli mcp start spawned pid={pid} binary={}",
            chrono_like_timestamp(),
            binary.display()
        ),
    )
    .ok();
    Ok(())
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

fn resolve_http_bind(root: &Path, override_bind: Option<&str>) -> String {
    override_bind
        .map(ToString::to_string)
        .unwrap_or_else(|| preferred_http_bind(root))
}

fn preferred_http_bind(root: &Path) -> String {
    format!("{DEFAULT_HTTP_BIND_HOST}:{}", preferred_http_port(root))
}

fn preferred_http_port(root: &Path) -> u16 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.to_string_lossy().hash(&mut hasher);
    STABLE_HTTP_PORT_BASE + (hasher.finish() % u64::from(STABLE_HTTP_PORT_RANGE)) as u16
}

fn ensure_expected_bind_available(root: &Path, http_bind: &str, operation: &str) -> Result<()> {
    let Some(port) = bind_port(http_bind) else {
        bail!("invalid MCP HTTP bind `{http_bind}`");
    };
    let owners = port_owners(port)?;
    if owners.is_empty() {
        return Ok(());
    }

    if operation == "restart"
        && owners
            .iter()
            .all(|owner| is_same_root_prism_mcp(owner, root))
    {
        reclaim_port_from_owners(port, &owners)?;
        let remaining = port_owners(port)?;
        if remaining.is_empty() {
            return Ok(());
        }
        bail!("{}", format_port_conflict(http_bind, &remaining));
    }

    bail!("{}", format_port_conflict(http_bind, &owners));
}

fn bind_port(bind: &str) -> Option<u16> {
    bind.rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

fn port_owners(port: u16) -> Result<Vec<PortOwner>> {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fp"])
        .output()
        .context("failed to inspect listening TCP ports with lsof")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut owners = Vec::new();
    for pid in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('p'))
        .filter_map(|pid| pid.parse::<u32>().ok())
        .collect::<BTreeSet<_>>()
    {
        let Some(command) = process_command(pid)? else {
            continue;
        };
        owners.push(PortOwner { pid, command });
    }
    Ok(owners)
}

fn process_command(pid: u32) -> Result<Option<String>> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .context("failed to inspect process command with ps")?;
    if !output.status.success() {
        return Ok(None);
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if command.is_empty() {
        return Ok(None);
    }
    Ok(Some(command))
}

fn is_same_root_prism_mcp(owner: &PortOwner, root: &Path) -> bool {
    let Some(command_root) = command_option_value(&owner.command, "--root") else {
        return false;
    };
    owner.command.contains("prism-mcp") && Path::new(&command_root) == root
}

fn reclaim_port_from_owners(port: u16, owners: &[PortOwner]) -> Result<()> {
    let pids = owners.iter().map(|owner| owner.pid).collect::<Vec<_>>();
    signal_pids(&pids, "-TERM")?;
    wait_for_port_release(port, STOP_TIMEOUT)?;
    let remaining = port_owners(port)?;
    if remaining.is_empty() {
        return Ok(());
    }
    let remaining_pids = remaining.iter().map(|owner| owner.pid).collect::<Vec<_>>();
    signal_pids(&remaining_pids, "-KILL")?;
    wait_for_port_release(port, Duration::from_secs(2))
}

fn wait_for_port_release(port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if port_owners(port)?.is_empty() {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }
    let remaining = port_owners(port)?;
    bail!(
        "{}",
        format_port_conflict(&format!("{DEFAULT_HTTP_BIND_HOST}:{port}"), &remaining)
    )
}

fn format_port_conflict(http_bind: &str, owners: &[PortOwner]) -> String {
    let mut message = format!("PRISM MCP expected HTTP bind {http_bind} is already in use.");
    for owner in owners {
        message.push_str(&format!("\n- pid={} command={}", owner.pid, owner.command));
    }
    message
}

fn wait_for_healthy_uri(root: &Path, paths: &McpPaths, health_path: &str) -> Result<String> {
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        let daemons = select_kind(&list_processes(root)?, McpProcessKind::Daemon);
        let uri = resolve_daemon_uri(paths, &daemons)?;
        if let Some(uri) = uri {
            let health = health_status(&Some(uri.clone()), health_path)?;
            if health.ok {
                return Ok(uri);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }

    let tail = daemon_log::tail_lines(&paths.log_path, 20)
        .unwrap_or_default()
        .join("\n");
    bail!(
        "timed out waiting for daemon health; recent log lines:\n{}",
        if tail.is_empty() {
            "<none>".to_string()
        } else {
            tail
        }
    );
}

fn signal_processes(processes: &[McpProcess], signal: &str) -> Result<()> {
    let pids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    signal_pids(&pids, signal)
}

fn signal_pids(pids: &[u32], signal: &str) -> Result<()> {
    if pids.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("kill");
    command.arg(signal);
    for pid in pids {
        command.arg(pid.to_string());
    }
    let output = command.output().context("failed to invoke kill")?;
    if !output.status.success() {
        bail!(
            "kill failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn reap_processes(root: &Path, processes: &[McpProcess]) -> Result<()> {
    if processes.is_empty() {
        return Ok(());
    }
    signal_processes(processes, "-TERM")?;
    wait_for_exit(root, processes, STOP_TIMEOUT)?;
    let remaining = list_processes(root)?
        .into_iter()
        .filter(|process| processes.iter().any(|target| target.pid == process.pid))
        .collect::<Vec<_>>();
    if !remaining.is_empty() {
        signal_processes(&remaining, "-KILL")?;
        wait_for_exit(root, &remaining, Duration::from_secs(2))?;
    }
    Ok(())
}

fn wait_for_exit(root: &Path, targets: &[McpProcess], timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = list_processes(root)?
            .into_iter()
            .filter(|process| targets.iter().any(|target| target.pid == process.pid))
            .count();
        if remaining == 0 {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
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

fn read_startup_marker(path: &Path) -> Result<Option<StartupMarker>> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to inspect startup marker {}", path.display()))?;
    let is_fresh = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed <= START_TIMEOUT);
    if !is_fresh {
        fs::remove_file(path).ok();
        return Ok(None);
    }
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read startup marker {}", path.display()))?;
    if let Ok(marker) = serde_json::from_str::<serde_json::Value>(&contents) {
        let operation = marker
            .get("operation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if matches!(operation, "start" | "restart") {
            return Ok(Some(StartupMarker {
                operation: operation.to_string(),
                nonce: marker
                    .get("nonce")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string),
            }));
        }
        fs::remove_file(path).ok();
        return Ok(None);
    }
    let operation = contents.trim();
    if matches!(operation, "start" | "restart") {
        return Ok(Some(StartupMarker {
            operation: operation.to_string(),
            nonce: None,
        }));
    }
    fs::remove_file(path).ok();
    Ok(None)
}

fn daemon_connection_info(
    paths: &McpPaths,
    daemons: &[McpProcess],
) -> Result<DaemonConnectionInfo> {
    let uri = resolve_daemon_uri(paths, daemons)?;
    let health_path = daemon_health_path(daemons);
    let health_uri = uri.as_ref().map(|uri| join_health_uri(uri, health_path));
    let health = health_status(&uri, health_path)?;
    Ok(DaemonConnectionInfo {
        transport: "streamable-http",
        mode: "direct-daemon",
        bridge_role: "stdio-compatibility-only",
        uri,
        health_uri,
        health,
    })
}

fn missing_daemon_connection_info() -> DaemonConnectionInfo {
    DaemonConnectionInfo {
        transport: "streamable-http",
        mode: "direct-daemon",
        bridge_role: "stdio-compatibility-only",
        uri: None,
        health_uri: None,
        health: HealthStatus {
            ok: false,
            detail: "missing uri file".to_string(),
        },
    }
}

fn daemon_connection_snapshot(
    paths: &McpPaths,
    observed_daemons: &[McpProcess],
    startup_marker: Option<&StartupMarker>,
) -> Result<(Vec<McpProcess>, DaemonConnectionInfo)> {
    let Some(startup_marker) = startup_marker else {
        let connection = daemon_connection_info(paths, observed_daemons)?;
        return Ok((observed_daemons.to_vec(), connection));
    };
    if startup_marker.operation != "restart" {
        let connection = daemon_connection_info(paths, observed_daemons)?;
        return Ok((observed_daemons.to_vec(), connection));
    }

    let Some(restart_nonce) = startup_marker.nonce.as_deref() else {
        return Ok((Vec::new(), missing_daemon_connection_info()));
    };
    let runtime_daemons = live_runtime_daemon_records(paths, observed_daemons)?;
    let matching = runtime_daemons
        .iter()
        .filter(|record| {
            record.restart_nonce.as_deref() == Some(restart_nonce) && record.http_uri.is_some()
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Ok((Vec::new(), missing_daemon_connection_info()));
    }

    let trusted_pids = matching
        .iter()
        .map(|record| record.pid)
        .collect::<BTreeSet<_>>();
    let trusted_daemons = observed_daemons
        .iter()
        .filter(|daemon| trusted_pids.contains(&daemon.pid))
        .cloned()
        .collect::<Vec<_>>();
    let uri = matching.first().and_then(|record| record.http_uri.clone());
    let health_path = matching
        .first()
        .and_then(|record| record.health_path.as_deref())
        .unwrap_or_else(|| daemon_health_path(&trusted_daemons));
    let health_uri = uri.as_ref().map(|uri| join_health_uri(uri, health_path));
    let health = health_status(&uri, health_path)?;
    Ok((
        trusted_daemons,
        DaemonConnectionInfo {
            transport: "streamable-http",
            mode: "direct-daemon",
            bridge_role: "stdio-compatibility-only",
            uri,
            health_uri,
            health,
        },
    ))
}

fn connection_snapshot_with_restart_grace(
    root: &Path,
    paths: &McpPaths,
) -> Result<(Vec<McpProcess>, DaemonConnectionInfo)> {
    let mut observed_processes = list_processes(root)?;
    let mut observed_daemons = select_kind(&observed_processes, McpProcessKind::Daemon);
    let mut bridges = select_kind(&observed_processes, McpProcessKind::Bridge);
    let mut startup_marker = read_startup_marker(&paths.startup_marker)?;
    let (mut daemons, mut connection) =
        daemon_connection_snapshot(paths, &observed_daemons, startup_marker.as_ref())?;
    if should_wait_for_restart_grace(&connection, &daemons, &bridges, startup_marker.as_ref()) {
        let deadline = Instant::now() + RESTART_GRACE_TIMEOUT;
        while Instant::now() < deadline {
            thread::sleep(POLL_INTERVAL);
            observed_processes = list_processes(root)?;
            observed_daemons = select_kind(&observed_processes, McpProcessKind::Daemon);
            bridges = select_kind(&observed_processes, McpProcessKind::Bridge);
            startup_marker = read_startup_marker(&paths.startup_marker)?;
            (daemons, connection) =
                daemon_connection_snapshot(paths, &observed_daemons, startup_marker.as_ref())?;
            if !should_wait_for_restart_grace(
                &connection,
                &daemons,
                &bridges,
                startup_marker.as_ref(),
            ) {
                break;
            }
        }
        if should_wait_for_restart_grace(&connection, &daemons, &bridges, startup_marker.as_ref()) {
            connection.health.detail = startup_marker
                .as_ref()
                .map(|marker| format!("daemon {} is in progress; retry shortly", marker.operation))
                .unwrap_or_else(|| {
                    "daemon restart appears to be in progress; retry shortly".to_string()
                });
        }
    }
    let mut visible_processes = daemons;
    visible_processes.extend(bridges);
    Ok((visible_processes, connection))
}

fn should_wait_for_restart_grace(
    connection: &DaemonConnectionInfo,
    daemons: &[McpProcess],
    bridges: &[McpProcess],
    startup_marker: Option<&StartupMarker>,
) -> bool {
    if startup_marker.is_some_and(|marker| marker.operation == "restart") {
        return !connection.health.ok;
    }
    !connection.health.ok
        && connection.uri.is_none()
        && daemons.is_empty()
        && (startup_marker.is_some() || !bridges.is_empty())
}

fn resolve_daemon_uri(paths: &McpPaths, daemons: &[McpProcess]) -> Result<Option<String>> {
    if let Some(uri) = read_uri_file(&paths.uri_file)? {
        return Ok(Some(uri));
    }
    runtime_state_uri(paths, daemons)
}

fn runtime_state_uri(paths: &McpPaths, daemons: &[McpProcess]) -> Result<Option<String>> {
    if daemons.is_empty() {
        return Ok(None);
    }
    Ok(live_runtime_daemon_records(paths, daemons)?
        .into_iter()
        .find_map(|record| record.http_uri))
}

fn live_runtime_daemon_records(
    paths: &McpPaths,
    daemons: &[McpProcess],
) -> Result<Vec<RuntimeDaemonRecord>> {
    if daemons.is_empty() {
        return Ok(Vec::new());
    }
    let live_pids = daemons
        .iter()
        .map(|daemon| daemon.pid)
        .collect::<BTreeSet<_>>();
    let path = &paths.runtime_state_path;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read runtime state {}", path.display()))?;
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(Vec::new());
    };
    Ok(value
        .get("processes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|process| {
            process
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "daemon")
                && process
                    .get("pid")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|pid| live_pids.contains(&(pid as u32)))
        })
        .map(|process| RuntimeDaemonRecord {
            pid: process
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default() as u32,
            health_path: process
                .get("health_path")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            http_uri: process
                .get("http_uri")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            restart_nonce: process
                .get("restart_nonce")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
        })
        .collect())
}

fn next_restart_nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("restart-{}-{now}", std::process::id())
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

fn health_status(uri: &Option<String>, health_path: &str) -> Result<HealthStatus> {
    let Some(uri) = uri else {
        return Ok(HealthStatus {
            ok: false,
            detail: "missing uri file".to_string(),
        });
    };
    match http_health_check(uri, health_path) {
        Ok(()) => match http_mcp_protocol_check(uri) {
            Ok(()) => Ok(HealthStatus {
                ok: true,
                detail: format!("ok ({uri}; /healthz ok; mcp ok)"),
            }),
            Err(error) => Ok(HealthStatus {
                ok: true,
                detail: format!("ok ({uri}; /healthz ok; mcp probe failed: {error})"),
            }),
        },
        Err(error) => Ok(HealthStatus {
            ok: false,
            detail: format!("unhealthy ({uri}): {error}"),
        }),
    }
}

fn http_health_check(uri: &str, health_path: &str) -> Result<()> {
    let authority = uri_authority(uri).ok_or_else(|| anyhow!("invalid uri"))?;
    let request =
        format!("GET {health_path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    let response = send_http_request(authority, &request, 1)?;
    let status_line = http_status_line(&response);
    if status_line.starts_with("HTTP/1.1 200") || status_line.starts_with("HTTP/1.0 200") {
        return Ok(());
    }
    bail!("unexpected response: {}", status_line)
}

fn http_mcp_protocol_check(uri: &str) -> Result<()> {
    let authority = uri_authority(uri).ok_or_else(|| anyhow!("invalid uri"))?;
    let path = uri_path(uri).unwrap_or("/mcp");
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"prism-cli-health","version":"0"}}}"#;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let response = send_http_request(authority, &request, 1)?;
    let status_line = http_status_line(&response);
    if !status_line.starts_with("HTTP/1.1 200") && !status_line.starts_with("HTTP/1.0 200") {
        bail!("unexpected response: {}", status_line);
    }

    let response_text = String::from_utf8_lossy(&response);
    let has_session_header = response_text
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("mcp-session-id:");
    let has_initialize_payload =
        response_text.contains("\"result\"") || response_text.contains("\"protocolVersion\"");
    if has_session_header || has_initialize_payload {
        return Ok(());
    }

    bail!("initialize response did not include an MCP session or payload")
}

fn send_http_request(authority: &str, request: &str, min_body_bytes: usize) -> Result<Vec<u8>> {
    let addr = resolve_socket_addr(authority)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .with_context(|| format!("failed to connect to {authority}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(request.as_bytes())?;

    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                response.extend_from_slice(&chunk[..read]);
                if header_end.is_none() {
                    header_end = http_header_end(&response);
                }
                if let Some(end) = header_end {
                    if response.len() >= end + min_body_bytes {
                        break;
                    }
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if let Some(end) = header_end {
                    if response.len() > end {
                        break;
                    }
                }
                return Err(error).context("timed out while reading HTTP response");
            }
            Err(error) => {
                return Err(error).context("failed to read HTTP response");
            }
        }
    }
    Ok(response)
}

fn http_header_end(response: &[u8]) -> Option<usize> {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn http_status_line(response: &[u8]) -> String {
    String::from_utf8_lossy(response)
        .lines()
        .next()
        .unwrap_or("<empty>")
        .to_string()
}

fn uri_port(uri: &str) -> Option<u16> {
    uri_authority(uri)?
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

fn uri_path(uri: &str) -> Option<&str> {
    let stripped = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    let (_, path) = stripped.split_once('/').unwrap_or((stripped, ""));
    if path.is_empty() {
        Some("/")
    } else {
        Some(&stripped[stripped.len() - path.len() - 1..])
    }
}

fn resolve_socket_addr(authority: &str) -> Result<SocketAddr> {
    authority
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("could not resolve {authority}"))
}

fn uri_authority(uri: &str) -> Option<&str> {
    uri.strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .filter(|value| !value.is_empty())
}

impl McpPaths {
    fn for_root(root: &Path) -> Result<Self> {
        let prism_paths = PrismPaths::for_workspace_root(root)?;
        Ok(Self {
            uri_file: prism_paths.mcp_http_uri_path()?,
            public_url_file: prism_paths.mcp_public_url_path()?,
            log_path: prism_paths.mcp_daemon_log_path()?,
            cache_path: prism_paths.worktree_cache_db_path()?,
            runtime_state_path: prism_paths.mcp_runtime_state_path()?,
            startup_marker: prism_paths.mcp_startup_marker_path()?,
        })
    }
}

fn read_public_url(paths: &McpPaths) -> Result<Option<String>> {
    let Ok(value) = fs::read_to_string(&paths.public_url_file) else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn write_public_url(paths: &McpPaths, url: &str) -> Result<()> {
    if let Some(parent) = paths.public_url_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.public_url_file, format!("{url}\n"))
        .with_context(|| format!("failed to write {}", paths.public_url_file.display()))
}

fn clear_public_url(paths: &McpPaths) -> Result<()> {
    match fs::remove_file(&paths.public_url_file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove {}", paths.public_url_file.display())),
    }
}

fn normalize_public_url(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("public URL must not be empty");
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        bail!("public URL must start with http:// or https://");
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::net::TcpListener;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "prism-cli-mcp-tests-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn create_parent(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
    }

    fn spawn_health_probe_server(
        health_response: &'static str,
        mcp_response: &'static str,
        keep_health_open: bool,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0_u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            request.extend_from_slice(&buf[..n]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                ErrorKind::WouldBlock | ErrorKind::TimedOut
                            ) =>
                        {
                            break;
                        }
                        Err(error) => panic!("failed to read test request: {error}"),
                    }
                }
                let request_text = String::from_utf8_lossy(&request);
                let response = if request_text.starts_with("GET /healthz") {
                    health_response
                } else {
                    mcp_response
                };
                stream.write_all(response.as_bytes()).unwrap();
                if keep_health_open && request_text.starts_with("GET /healthz") {
                    thread::sleep(Duration::from_secs(3));
                }
            }
        });
        format!("http://{addr}/mcp")
    }

    #[test]
    fn mcp_paths_report_worktree_cache_db() {
        let root = temp_root("paths-cache");
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();

        let paths = McpPaths::for_root(&root).unwrap();
        let prism_paths = PrismPaths::for_workspace_root(&root).unwrap();

        assert_eq!(
            paths.cache_path,
            prism_paths.worktree_cache_db_path().unwrap()
        );
        assert_ne!(
            paths.cache_path,
            prism_paths.shared_runtime_db_path().unwrap()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalize_public_url_trims_and_strips_trailing_slash() {
        assert_eq!(
            normalize_public_url(" https://runtime.example/peer/query/ ").unwrap(),
            "https://runtime.example/peer/query"
        );
    }

    #[test]
    fn normalize_public_url_rejects_non_http_values() {
        assert!(normalize_public_url("runtime.example").is_err());
    }

    #[test]
    fn public_url_round_trips_through_state_file() {
        let root = temp_root("public-url");
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let paths = McpPaths::for_root(&root).unwrap();

        write_public_url(&paths, "https://runtime.example/peer/query").unwrap();
        assert_eq!(
            read_public_url(&paths).unwrap().as_deref(),
            Some("https://runtime.example/peer/query")
        );

        clear_public_url(&paths).unwrap();
        assert!(read_public_url(&paths).unwrap().is_none());
    }

    #[test]
    fn parses_ps_lines_for_prism_mcp() {
        let root = temp_root("parse-ps-lines");
        let prism_mcp = root.join("target/release/prism-mcp");
        let uri_file = root.join(".prism/prism-mcp-http-uri");
        let process = parse_ps_line(
            &format!(
                "29267 1 4454352 02:12:24 {} --mode daemon --root {} --http-uri-file {} --http-path /mcp --health-path /healthz --no-coordination",
                prism_mcp.display(),
                root.display(),
                uri_file.display()
            ),
        )
        .expect("process should parse");
        assert_eq!(process.pid, 29267);
        assert_eq!(process.ppid, 1);
        assert_eq!(process.kind, McpProcessKind::Daemon);
        assert_eq!(process.health_path.as_deref(), Some("/healthz"));
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
            },
            McpProcess {
                pid: 11,
                ppid: 1001,
                rss_kb: 1,
                elapsed: "00:02".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 12,
                ppid: 1002,
                rss_kb: 1,
                elapsed: "02:01".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 13,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:03".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
        ];
        let connected = BTreeSet::from([10_u32]);

        let counts = classify_bridges(&bridges, &connected);

        assert_eq!(counts.connected, 1);
        assert_eq!(counts.idle, 2);
        assert_eq!(counts.orphaned, 1);
    }

    #[test]
    fn cleanup_candidates_include_orphaned_bridges_only() {
        let bridges = vec![
            McpProcess {
                pid: 10,
                ppid: 1000,
                rss_kb: 1,
                elapsed: "00:20".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 11,
                ppid: 1001,
                rss_kb: 1,
                elapsed: "02:10".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 12,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:03".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
        ];

        let candidates = cleanup_candidate_bridges(&bridges, &BTreeSet::new());

        assert_eq!(candidates.len(), 1);
        assert!(candidates.iter().any(|process| process.pid == 12));
    }

    #[test]
    fn daemon_connection_info_prefers_direct_daemon_endpoint() {
        let root = temp_root("daemon-connection");
        let paths = McpPaths::for_root(&root).unwrap();
        create_parent(&paths.uri_file);
        fs::write(&paths.uri_file, "http://127.0.0.1:9/mcp\n").unwrap();

        let info = daemon_connection_info(&paths, &[]).unwrap();

        assert_eq!(info.mode, "direct-daemon");
        assert_eq!(info.transport, "streamable-http");
        assert_eq!(info.bridge_role, "stdio-compatibility-only");
        assert_eq!(info.uri.as_deref(), Some("http://127.0.0.1:9/mcp"));
        assert_eq!(
            info.health_uri.as_deref(),
            Some("http://127.0.0.1:9/healthz")
        );
    }

    #[test]
    fn http_health_check_accepts_partial_response_without_waiting_for_close() {
        let uri = spawn_health_probe_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            "HTTP/1.1 200 OK\r\nmcp-session-id: test\r\nContent-Length: 20\r\n\r\n{\"result\":{\"ok\":true}}",
            true,
        );

        let result = http_health_check(&uri, "/healthz");

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn health_status_keeps_daemon_healthy_when_only_mcp_probe_fails() {
        let uri = spawn_health_probe_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\n\r\nerror",
            false,
        );

        let health = health_status(&Some(uri.clone()), "/healthz").unwrap();

        assert!(health.ok);
        assert!(health.detail.contains("/healthz ok"), "{}", health.detail);
        assert!(
            health.detail.contains("mcp probe failed"),
            "{}",
            health.detail
        );
    }

    #[test]
    fn restart_grace_only_applies_while_bridges_outlive_the_daemon() {
        let connection = DaemonConnectionInfo {
            transport: "streamable-http",
            mode: "direct-daemon",
            bridge_role: "stdio-compatibility-only",
            uri: None,
            health_uri: None,
            health: HealthStatus {
                ok: false,
                detail: "missing uri file".to_string(),
            },
        };
        let bridges = vec![McpProcess {
            pid: 10,
            ppid: 1000,
            rss_kb: 1,
            elapsed: "00:01".to_string(),
            command: "prism-mcp --mode bridge".to_string(),
            kind: McpProcessKind::Bridge,
            health_path: None,
        }];

        assert!(should_wait_for_restart_grace(
            &connection,
            &[],
            &bridges,
            None
        ));
        assert!(should_wait_for_restart_grace(
            &connection,
            &[],
            &[],
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("restart-1".to_string()),
            })
        ));

        let healthy = DaemonConnectionInfo {
            health: HealthStatus {
                ok: true,
                detail: "ok".to_string(),
            },
            ..connection.clone()
        };
        assert!(!should_wait_for_restart_grace(
            &healthy,
            &[],
            &bridges,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("restart-1".to_string()),
            })
        ));

        let with_uri = DaemonConnectionInfo {
            uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            ..connection.clone()
        };
        assert!(should_wait_for_restart_grace(
            &with_uri,
            &[],
            &bridges,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("restart-1".to_string()),
            })
        ));

        let daemons = vec![McpProcess {
            pid: 11,
            ppid: 1,
            rss_kb: 1,
            elapsed: "00:02".to_string(),
            command: "prism-mcp --mode daemon".to_string(),
            kind: McpProcessKind::Daemon,
            health_path: Some("/healthz".to_string()),
        }];
        assert!(should_wait_for_restart_grace(
            &connection,
            &daemons,
            &bridges,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("restart-1".to_string()),
            })
        ));
        let healthy_with_daemon = DaemonConnectionInfo {
            health: HealthStatus {
                ok: true,
                detail: "ok".to_string(),
            },
            uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            ..connection.clone()
        };
        assert!(!should_wait_for_restart_grace(
            &healthy_with_daemon,
            &daemons,
            &bridges,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("restart-1".to_string()),
            })
        ));
        assert!(!should_wait_for_restart_grace(&connection, &[], &[], None));
    }

    #[test]
    fn daemon_connection_snapshot_ignores_daemons_without_matching_restart_nonce() {
        let root = temp_root("restart-snapshot-stale");
        let paths = McpPaths::for_root(&root).unwrap();
        create_parent(&paths.runtime_state_path);
        fs::write(
            &paths.runtime_state_path,
            r#"{
  "processes": [
    {
      "pid": 12,
      "kind": "daemon",
      "health_path": "/healthz",
      "http_uri": "http://127.0.0.1:41000/mcp",
      "restart_nonce": "old"
    }
  ],
  "events": []
}"#,
        )
        .unwrap();
        let daemons = vec![McpProcess {
            pid: 12,
            ppid: 1,
            rss_kb: 0,
            elapsed: "00:01".to_string(),
            command: "prism-mcp --mode daemon".to_string(),
            kind: McpProcessKind::Daemon,
            health_path: Some("/healthz".to_string()),
        }];

        let (trusted_daemons, connection) = daemon_connection_snapshot(
            &paths,
            &daemons,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("new".to_string()),
            }),
        )
        .unwrap();

        assert!(trusted_daemons.is_empty());
        assert!(connection.uri.is_none());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn daemon_connection_snapshot_uses_matching_restart_nonce() {
        let root = temp_root("restart-snapshot-current");
        let paths = McpPaths::for_root(&root).unwrap();
        create_parent(&paths.runtime_state_path);
        fs::write(
            &paths.runtime_state_path,
            r#"{
  "processes": [
    {
      "pid": 12,
      "kind": "daemon",
      "health_path": "/healthz",
      "http_uri": "http://127.0.0.1:41000/mcp",
      "restart_nonce": "new"
    }
  ],
  "events": []
}"#,
        )
        .unwrap();
        let daemons = vec![McpProcess {
            pid: 12,
            ppid: 1,
            rss_kb: 0,
            elapsed: "00:01".to_string(),
            command: "prism-mcp --mode daemon".to_string(),
            kind: McpProcessKind::Daemon,
            health_path: Some("/healthz".to_string()),
        }];

        let (trusted_daemons, connection) = daemon_connection_snapshot(
            &paths,
            &daemons,
            Some(&StartupMarker {
                operation: "restart".to_string(),
                nonce: Some("new".to_string()),
            }),
        )
        .unwrap();

        assert_eq!(trusted_daemons.len(), 1);
        assert_eq!(
            connection.uri.as_deref(),
            Some("http://127.0.0.1:41000/mcp")
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn join_health_uri_uses_authority_not_mcp_path() {
        assert_eq!(
            join_health_uri("http://127.0.0.1:52695/mcp", "/healthz"),
            "http://127.0.0.1:52695/healthz"
        );
        assert_eq!(
            join_health_uri("http://127.0.0.1:52695/mcp", "healthz"),
            "http://127.0.0.1:52695/healthz"
        );
    }

    #[test]
    fn orphaned_bridges_only_selects_bridge_processes_with_init_parent() {
        let processes = vec![
            McpProcess {
                pid: 10,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:01".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 11,
                ppid: 1000,
                rss_kb: 1,
                elapsed: "00:02".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
            },
            McpProcess {
                pid: 12,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:03".to_string(),
                command: "prism-mcp --mode daemon".to_string(),
                kind: McpProcessKind::Daemon,
                health_path: Some("/healthz".to_string()),
            },
        ];

        let orphaned = orphaned_bridges(&processes);

        assert_eq!(orphaned.len(), 1);
        assert_eq!(orphaned[0].pid, 10);
    }

    #[test]
    fn command_option_value_reads_flag_arguments() {
        let command = "prism-mcp --mode bridge --root workspace --http-uri-file workspace/uri";
        assert_eq!(
            command_option_value(command, "--http-uri-file").as_deref(),
            Some("workspace/uri")
        );
        assert_eq!(command_option_value(command, "--missing"), None);
    }

    #[test]
    fn bridge_exec_args_include_required_bridge_flags() {
        let root = temp_root("bridge-exec-args");
        let paths = McpPaths::for_root(&root).unwrap();
        let args = bridge_exec_args(&root, &paths, false, true, None, None, false, None)
            .into_iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            vec![
                "--mode".to_string(),
                "bridge".to_string(),
                "--root".to_string(),
                root.display().to_string(),
                "--http-uri-file".to_string(),
                paths.uri_file.display().to_string(),
                "--internal-developer".to_string(),
            ]
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn bridge_exec_args_forward_shared_runtime_selection() {
        let root = temp_root("bridge-exec-shared-runtime");
        let paths = McpPaths::for_root(&root).unwrap();
        let sqlite = root.join("shared-runtime.db");
        let args = bridge_exec_args(
            &root,
            &paths,
            true,
            false,
            Some(sqlite.as_path()),
            None,
            false,
            None,
        )
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

        assert!(args.windows(2).any(|window| {
            window
                == [
                    "--shared-runtime-sqlite".to_string(),
                    sqlite.display().to_string(),
                ]
        }));
        assert!(args.contains(&"--no-coordination".to_string()));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn uri_authority_extracts_host_and_port() {
        assert_eq!(
            uri_authority("http://127.0.0.1:43123/mcp"),
            Some("127.0.0.1:43123")
        );
        assert_eq!(
            uri_authority("https://example.com/foo"),
            Some("example.com")
        );
        assert_eq!(uri_authority("not-a-uri"), None);
    }

    #[test]
    fn runtime_state_uri_falls_back_when_uri_file_is_missing() {
        let root = temp_root("runtime-state-uri");
        let paths = McpPaths::for_root(&root).unwrap();
        create_parent(&paths.runtime_state_path);
        fs::write(
            &paths.runtime_state_path,
            r#"{
  "processes": [
    { "pid": 12, "kind": "daemon", "http_uri": "http://127.0.0.1:41000/mcp" },
    { "pid": 99, "kind": "bridge", "http_uri": null }
  ],
  "events": []
}"#,
        )
        .unwrap();

        let uri = runtime_state_uri(
            &paths,
            &[McpProcess {
                pid: 12,
                ppid: 1,
                rss_kb: 0,
                elapsed: "00:01".to_string(),
                command: "prism-mcp --mode daemon".to_string(),
                kind: McpProcessKind::Daemon,
                health_path: Some("/healthz".to_string()),
            }],
        )
        .unwrap();

        assert_eq!(uri.as_deref(), Some("http://127.0.0.1:41000/mcp"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn startup_marker_try_create_is_exclusive_for_fresh_markers() {
        let root = temp_root("startup-marker");
        let marker_path = McpPaths::for_root(&root).unwrap().startup_marker;

        let first = StartupMarkerGuard::try_create(&marker_path, "restart", Some("restart-1"))
            .unwrap()
            .expect("first marker acquisition should succeed");
        let marker = read_startup_marker(&marker_path)
            .unwrap()
            .expect("marker should deserialize");
        assert_eq!(marker.operation, "restart");
        assert_eq!(marker.nonce.as_deref(), Some("restart-1"));

        let second = StartupMarkerGuard::try_create(&marker_path, "start", None).unwrap();
        assert!(second.is_none());

        drop(first);

        let third = StartupMarkerGuard::try_create(&marker_path, "start", None)
            .unwrap()
            .expect("marker should be acquirable after the first guard drops");
        drop(third);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn preferred_http_bind_is_deterministic_for_a_workspace_root() {
        let root = temp_root("preferred-http-bind");
        let left = preferred_http_bind(&root);
        let right = preferred_http_bind(&root);
        assert_eq!(left, right);
        assert!(left.starts_with("127.0.0.1:"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn same_root_prism_mcp_conflicts_are_reclaimable() {
        let root = temp_root("same-root-owner");
        let owner = PortOwner {
            pid: 42,
            command: format!(
                "{} --mode daemon --root {} --http-bind 127.0.0.1:52695",
                root.join("bin/prism-mcp").display(),
                root.display()
            ),
        };
        assert!(is_same_root_prism_mcp(&owner, &root));
        fs::remove_dir_all(root).ok();
    }
}
