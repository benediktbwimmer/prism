use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use crate::cli::McpCommand;

const START_TIMEOUT: Duration = Duration::from_secs(180);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_HEALTH_PATH: &str = "/healthz";

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
    log_path: PathBuf,
    cache_path: PathBuf,
}

#[derive(Debug, Clone)]
struct HealthStatus {
    ok: bool,
    detail: String,
}

pub(crate) fn handle(root: &Path, command: McpCommand) -> Result<()> {
    let root = root.canonicalize()?;
    match command {
        McpCommand::Status => status(&root),
        McpCommand::Start {
            no_coordination,
            internal_developer,
        } => start(&root, no_coordination, internal_developer),
        McpCommand::Stop { kill_bridges } => stop(&root, kill_bridges),
        McpCommand::Restart {
            kill_bridges,
            no_coordination,
            internal_developer,
        } => {
            stop(&root, kill_bridges)?;
            start(&root, no_coordination, internal_developer)
        }
        McpCommand::Health => health(&root),
        McpCommand::Logs { lines } => logs(&root, lines),
    }
}

fn status(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root);
    let processes = list_processes(root)?;
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let bridges = select_kind(&processes, McpProcessKind::Bridge);
    let uri = resolve_daemon_uri(root, &paths, &daemons)?;
    let health = health_status(&uri, daemon_health_path(&daemons))?;
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
    println!("uri_file: {}", paths.uri_file.display());
    println!("uri: {}", uri.as_deref().unwrap_or("<missing>"));
    println!("health: {}", health.detail);
    if let Ok(metadata) = fs::metadata(&paths.log_path) {
        println!(
            "log_path: {} ({} bytes)",
            paths.log_path.display(),
            metadata.len()
        );
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
    if daemons.len() > 1 {
        println!("warning: multiple daemon processes are running for this workspace");
    }
    if bridge_counts.orphaned > 0 {
        println!("warning: orphaned bridge processes are running for this workspace");
    }
    if daemons.is_empty() && !bridges.is_empty() {
        println!("warning: bridge processes exist without a daemon");
    }
    Ok(())
}

fn start(root: &Path, no_coordination: bool, internal_developer: bool) -> Result<()> {
    let paths = McpPaths::for_root(root);
    let processes = list_processes(root)?;
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
    fs::remove_file(&paths.uri_file).ok();

    let binary = prism_mcp_binary()?;
    spawn_daemon(root, &binary, &paths, no_coordination, internal_developer)?;
    let uri = wait_for_healthy_uri(root, &paths, DEFAULT_HEALTH_PATH)?;
    println!("started daemon");
    println!("uri: {uri}");
    status(root)
}

fn stop(root: &Path, kill_bridges: bool) -> Result<()> {
    let paths = McpPaths::for_root(root);
    let processes = list_processes(root)?;
    let mut targets = select_kind(&processes, McpProcessKind::Daemon);
    if kill_bridges {
        targets.extend(select_kind(&processes, McpProcessKind::Bridge));
    }

    if targets.is_empty() {
        println!("no matching prism-mcp processes found");
        fs::remove_file(&paths.uri_file).ok();
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
    }

    println!("stopped {} process(es)", targets.len());
    Ok(())
}

fn health(root: &Path) -> Result<()> {
    let paths = McpPaths::for_root(root);
    let processes = list_processes(root)?;
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let uri = resolve_daemon_uri(root, &paths, &daemons)?;
    let health = health_status(&uri, daemon_health_path(&daemons))?;
    println!("{}", health.detail);
    if !health.ok {
        bail!("daemon is not healthy");
    }
    Ok(())
}

fn logs(root: &Path, lines: usize) -> Result<()> {
    let paths = McpPaths::for_root(root);
    let lines = tail_lines(&paths.log_path, lines)?;
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

fn command_option_value(command: &str, option: &str) -> Option<String> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens
        .windows(2)
        .find_map(|window| (window[0] == option).then(|| window[1].to_string()))
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
    no_coordination: bool,
    internal_developer: bool,
) -> Result<()> {
    let mut args = vec![
        "--mode".to_string(),
        "daemon".to_string(),
        "--daemonize".to_string(),
        "--root".to_string(),
        root.display().to_string(),
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

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_path)
        .with_context(|| format!("failed to open daemon log {}", paths.log_path.display()))?;
    let stderr_file = log_file
        .try_clone()
        .with_context(|| format!("failed to clone daemon log {}", paths.log_path.display()))?;

    let child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .with_context(|| format!("failed to spawn daemon via {}", binary.display()))?;

    let pid = child.id();
    writeln!(
        &mut OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.log_path)
            .with_context(|| format!("failed to reopen daemon log {}", paths.log_path.display()))?,
        "{} prism-cli mcp start spawned pid={pid} binary={}",
        chrono_like_timestamp(),
        binary.display()
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

fn wait_for_healthy_uri(root: &Path, paths: &McpPaths, health_path: &str) -> Result<String> {
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        let daemons = select_kind(&list_processes(root)?, McpProcessKind::Daemon);
        let uri = resolve_daemon_uri(root, paths, &daemons)?;
        if let Some(uri) = uri {
            let health = health_status(&Some(uri.clone()), health_path)?;
            if health.ok {
                return Ok(uri);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }

    let tail = tail_lines(&paths.log_path, 20)
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
    if processes.is_empty() {
        return Ok(());
    }
    let mut command = Command::new("kill");
    command.arg(signal);
    for process in processes {
        command.arg(process.pid.to_string());
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

fn resolve_daemon_uri(
    root: &Path,
    paths: &McpPaths,
    daemons: &[McpProcess],
) -> Result<Option<String>> {
    if let Some(uri) = read_uri_file(&paths.uri_file)? {
        return Ok(Some(uri));
    }
    runtime_state_uri(root, daemons)
}

fn runtime_state_uri(root: &Path, daemons: &[McpProcess]) -> Result<Option<String>> {
    if daemons.is_empty() {
        return Ok(None);
    }
    let live_pids = daemons.iter().map(|daemon| daemon.pid).collect::<Vec<_>>();
    let path = root.join(".prism").join("prism-mcp-runtime.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read runtime state {}", path.display()))?;
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(None);
    };
    Ok(value
        .get("processes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|process| {
            process
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "daemon")
                && process
                    .get("pid")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|pid| live_pids.contains(&(pid as u32)))
        })
        .and_then(|process| process.get("http_uri"))
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string))
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
        Ok(()) => Ok(HealthStatus {
            ok: true,
            detail: format!("ok ({uri})"),
        }),
        Err(error) => Ok(HealthStatus {
            ok: false,
            detail: format!("unhealthy ({uri}): {error}"),
        }),
    }
}

fn http_health_check(uri: &str, health_path: &str) -> Result<()> {
    let authority = uri_authority(uri).ok_or_else(|| anyhow!("invalid uri"))?;
    let addr = resolve_socket_addr(authority)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .with_context(|| format!("failed to connect to {authority}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let request =
        format!("GET {health_path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") {
        return Ok(());
    }
    bail!(
        "unexpected response: {}",
        response.lines().next().unwrap_or("<empty>")
    )
}

fn uri_port(uri: &str) -> Option<u16> {
    uri_authority(uri)?
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
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

fn tail_lines(path: &Path, limit: usize) -> Result<Vec<String>> {
    if limit == 0 {
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

impl McpPaths {
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
    use std::env;
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
        fs::create_dir_all(root.join(".prism")).unwrap();
        root
    }

    #[test]
    fn parses_ps_lines_for_prism_mcp() {
        let process = parse_ps_line(
            "29267 1 4454352 02:12:24 /Users/bene/code/prism/target/release/prism-mcp --mode daemon --root /Users/bene/code/prism --http-uri-file /Users/bene/code/prism/.prism/prism-mcp-http-uri --http-path /mcp --health-path /healthz --no-coordination",
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
        assert_eq!(counts.idle, 1);
        assert_eq!(counts.orphaned, 1);
    }

    #[test]
    fn command_option_value_reads_flag_arguments() {
        let command = "prism-mcp --mode bridge --root /tmp/work --http-uri-file /tmp/uri";
        assert_eq!(
            command_option_value(command, "--http-uri-file").as_deref(),
            Some("/tmp/uri")
        );
        assert_eq!(command_option_value(command, "--missing"), None);
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
        fs::write(
            root.join(".prism").join("prism-mcp-runtime.json"),
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
            &root,
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
}
