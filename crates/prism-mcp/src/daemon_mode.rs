use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use axum::{middleware, routing::get, Router};
use prism_core::PrismPaths;
use rmcp::transport::{
    streamable_http_server::session::local::LocalSessionManager, StreamableHttpServerConfig,
    StreamableHttpService,
};
use tokio::net::TcpListener;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::dashboard_router::{routes as dashboard_routes, DashboardAppState};
use crate::proxy_server::ProxyMcpServer;
use crate::runtime_state;
use crate::ui_router::{routes as prism_ui_routes, PrismUiState};
use crate::{PrismMcpCli, PrismMcpServer};

const DEFAULT_DAEMON_START_TIMEOUT_MS: u64 = 60_000;
const STABLE_HTTP_PORT_BASE: u16 = 41_000;
const STABLE_HTTP_PORT_RANGE: u16 = 20_000;
const STABLE_HTTP_PORT_ATTEMPTS: u16 = 128;
const PREFERRED_STABLE_HTTP_BIND_WAIT: Duration = Duration::from_secs(3);
const PREFERRED_STABLE_HTTP_BIND_POLL: Duration = Duration::from_millis(50);

pub(crate) fn default_http_uri_file_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.mcp_http_uri_path()
}

pub(crate) fn default_log_path(root: &Path) -> Result<PathBuf> {
    PrismPaths::for_workspace_root(root)?.mcp_daemon_log_path()
}

pub async fn serve_with_mode(cli: PrismMcpCli) -> Result<()> {
    let root = cli.root.canonicalize()?;
    match cli.mode {
        crate::PrismMcpMode::Stdio => {
            let server = PrismMcpServer::from_workspace_with_features_and_shared_runtime(
                &root,
                cli.features(),
                cli.shared_runtime_backend(&root)?,
            )?;
            server.serve_stdio().await
        }
        crate::PrismMcpMode::Daemon => run_daemon(&cli, &root).await,
        crate::PrismMcpMode::Bridge => run_bridge(&cli, &root).await,
    }
}

async fn run_daemon(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let started = Instant::now();
    let features = cli.features();
    let listener = bind_listener(cli, root).await?;
    let mcp_path = normalize_route_path(&cli.http_path);
    let health_path = normalize_route_path(&cli.health_path);
    let addr = listener.local_addr()?;
    let http_uri = format!("http://{addr}{mcp_path}");
    let uri_file_path = cli.http_uri_file_path(root)?;
    write_http_uri_file(&uri_file_path, &http_uri)?;
    let _uri_guard = HttpUriFileGuard {
        path: uri_file_path,
        expected_uri: http_uri.clone(),
    };
    let server = PrismMcpServer::from_workspace_with_features_and_shared_runtime(
        root,
        features.clone(),
        cli.shared_runtime_backend(root)?,
    )?;
    info!(
        mode = "daemon",
        root = %root.display(),
        listen_addr = %addr,
        http_uri = %http_uri,
        startup_ms = started.elapsed().as_millis(),
        "prism-mcp daemon ready"
    );
    if let Err(error) = runtime_state::record_daemon_ready(
        cli,
        root,
        &http_uri,
        &health_path,
        started.elapsed().as_millis(),
    ) {
        warn!(
            error = %error,
            root = %root.display(),
            "failed to update prism runtime state for daemon readiness"
        );
    }

    let service_server = server.clone();
    let service: StreamableHttpService<_, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(service_server.clone().instrumented_service()),
        Default::default(),
        StreamableHttpServerConfig::default(),
    );
    let dashboard_state = DashboardAppState {
        host: Arc::clone(&server.host),
        root: root.to_path_buf(),
    };
    let prism_ui_state = PrismUiState {
        host: Arc::clone(&server.host),
        root: root.to_path_buf(),
    };
    let mcp_router =
        Router::new()
            .nest_service(&mcp_path, service)
            .route_layer(middleware::from_fn(
                crate::request_envelope::instrument_mcp_http_request,
            ));
    let mut router = Router::new()
        .route(&health_path, get(http_health))
        .merge(mcp_router);
    if features.ui {
        router = router
            .merge(prism_ui_routes(prism_ui_state))
            .merge(dashboard_routes(dashboard_state));
    }

    axum::serve(listener, router)
        .await
        .context("PRISM MCP HTTP server exited unexpectedly")
}

async fn bind_listener(cli: &PrismMcpCli, root: &Path) -> Result<TcpListener> {
    if let Some(host) = auto_bind_host(&cli.http_bind) {
        let candidates = stable_http_bind_candidates(host, root);
        if let Some(listener) = bind_stable_listener(cli, root, &candidates).await? {
            return Ok(listener);
        }
        warn!(
            root = %root.display(),
            configured_bind = %cli.http_bind,
            "failed to claim a stable PRISM MCP port; falling back to a dynamic port"
        );
    }

    TcpListener::bind(&cli.http_bind).await.with_context(|| {
        format!(
            "failed to bind PRISM MCP HTTP listener at {}",
            cli.http_bind
        )
    })
}

async fn bind_stable_listener(
    cli: &PrismMcpCli,
    root: &Path,
    candidates: &[String],
) -> Result<Option<TcpListener>> {
    let Some((preferred, fallbacks)) = candidates.split_first() else {
        return Ok(None);
    };

    if let Some(listener) = bind_preferred_stable_listener(preferred).await? {
        return Ok(Some(listener));
    }

    warn!(
        root = %root.display(),
        configured_bind = %cli.http_bind,
        preferred_bind = %preferred,
        wait_ms = PREFERRED_STABLE_HTTP_BIND_WAIT.as_millis(),
        "preferred PRISM MCP port stayed busy during startup; scanning fallback stable ports"
    );

    for candidate in fallbacks {
        match TcpListener::bind(candidate).await {
            Ok(listener) => return Ok(Some(listener)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to bind PRISM MCP HTTP listener at {candidate}")
                });
            }
        }
    }

    Ok(None)
}

async fn bind_preferred_stable_listener(preferred: &str) -> Result<Option<TcpListener>> {
    let deadline = Instant::now() + PREFERRED_STABLE_HTTP_BIND_WAIT;
    loop {
        match TcpListener::bind(preferred).await {
            Ok(listener) => return Ok(Some(listener)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                sleep(PREFERRED_STABLE_HTTP_BIND_POLL).await;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to bind PRISM MCP HTTP listener at {preferred}")
                });
            }
        }
    }
}

async fn run_bridge(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let upstream_source = BridgeUpstreamSource::from_cli(cli, root)?;
    let proxy =
        ProxyMcpServer::bootstrap_with_source_for_root(root, cli.clone(), upstream_source).await?;
    proxy.serve_stdio().await
}

pub(crate) struct UpstreamResolution {
    pub(crate) uri: String,
    pub(crate) source: &'static str,
    pub(crate) daemon_wait_ms: u128,
    pub(crate) spawned_daemon: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum BridgeUpstreamSource {
    Fixed(String),
    HttpUriFile(PathBuf),
}

impl BridgeUpstreamSource {
    pub(crate) fn from_cli(cli: &PrismMcpCli, root: &Path) -> Result<Self> {
        match &cli.upstream_uri {
            Some(uri) => Ok(Self::Fixed(uri.clone())),
            None => Ok(Self::HttpUriFile(cli.http_uri_file_path(root)?)),
        }
    }

    pub(crate) fn read_uri(&self) -> Result<String> {
        match self {
            Self::Fixed(uri) => Ok(uri.clone()),
            Self::HttpUriFile(path) => read_http_uri_file(path)?.ok_or_else(|| {
                anyhow!(
                    "PRISM MCP upstream URI file {} is not ready",
                    path.display()
                )
            }),
        }
    }
}

pub(crate) async fn resolve_upstream_uri(
    cli: &PrismMcpCli,
    root: &Path,
) -> Result<UpstreamResolution> {
    if let Some(uri) = &cli.upstream_uri {
        return Ok(UpstreamResolution {
            uri: uri.clone(),
            source: "explicit_upstream_uri",
            daemon_wait_ms: 0,
            spawned_daemon: false,
        });
    }

    ensure_daemon_running(cli, root).await
}

async fn ensure_daemon_running(cli: &PrismMcpCli, root: &Path) -> Result<UpstreamResolution> {
    if let Some(uri) = first_healthy_daemon_uri(cli, root).await? {
        debug!(root = %root.display(), uri = %uri, "reusing existing prism-mcp daemon");
        return Ok(UpstreamResolution {
            uri,
            source: "existing_healthy_daemon",
            daemon_wait_ms: 0,
            spawned_daemon: false,
        });
    }

    if live_daemon_process_count(root)? > 0 {
        let timeout = Duration::from_millis(
            cli.daemon_start_timeout_ms
                .unwrap_or(DEFAULT_DAEMON_START_TIMEOUT_MS),
        );
        let wait_started = Instant::now();
        let deadline = Instant::now() + timeout;
        warn!(
            root = %root.display(),
            "prism-mcp daemon process exists but no healthy URI is visible yet; waiting for readiness"
        );
        while Instant::now() < deadline {
            if let Some(uri) = first_healthy_daemon_uri(cli, root).await? {
                info!(root = %root.display(), uri = %uri, "prism-mcp daemon became healthy");
                return Ok(UpstreamResolution {
                    uri,
                    source: "waited_for_existing_daemon",
                    daemon_wait_ms: wait_started.elapsed().as_millis(),
                    spawned_daemon: false,
                });
            }
            sleep(Duration::from_millis(50)).await;
        }
        return Err(anyhow!(
            "a PRISM MCP daemon process exists for {} but never became healthy; use `prism mcp restart`",
            root.display()
        ));
    }

    spawn_daemon(cli, root)?;
    let timeout = Duration::from_millis(
        cli.daemon_start_timeout_ms
            .unwrap_or(DEFAULT_DAEMON_START_TIMEOUT_MS),
    );
    let wait_started = Instant::now();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(uri) = first_healthy_daemon_uri(cli, root).await? {
            info!(root = %root.display(), uri = %uri, "prism-mcp daemon became healthy");
            return Ok(UpstreamResolution {
                uri,
                source: "spawned_daemon",
                daemon_wait_ms: wait_started.elapsed().as_millis(),
                spawned_daemon: true,
            });
        }
        sleep(Duration::from_millis(50)).await;
    }

    Err(anyhow!(
        "timed out waiting for PRISM MCP HTTP daemon URI file at {}. Check {} for daemon startup logs.",
        cli.http_uri_file_path(root)?.display(),
        cli.log_path(root)?.display()
    ))
}

fn spawn_daemon(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let log_path = cli.log_path(root)?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let current_exe = cli
        .bridge_daemon_binary
        .clone()
        .unwrap_or(std::env::current_exe()?);
    info!(
        root = %root.display(),
        log_path = %log_path.display(),
        executable = %current_exe.display(),
        args = ?cli.daemon_spawn_args(root)?,
        "spawning detached prism-mcp daemon"
    );
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(detached_daemon_launcher_script())
        .arg("prism-mcp-daemon-launcher")
        .arg(&log_path)
        .arg(&current_exe)
        .args(cli.daemon_spawn_args(root)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn PRISM MCP daemon from {}",
            current_exe.display()
        )
    })?;
    if !status.success() {
        return Err(anyhow!(
            "failed to detach PRISM MCP daemon launcher for {}: {status}",
            current_exe.display()
        ));
    }
    Ok(())
}

fn detached_daemon_launcher_script() -> &'static str {
    r#"log_path="$1"
shift
exe="$1"
shift
nohup "$exe" "$@" </dev/null >>"$log_path" 2>&1 &
"#
}

fn write_http_uri_file(path: &Path, uri: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{uri}\n"))
        .with_context(|| format!("failed to write PRISM MCP HTTP URI file {}", path.display()))
}

fn read_http_uri_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let uri = fs::read_to_string(path)
        .with_context(|| format!("failed to read PRISM MCP HTTP URI file {}", path.display()))?;
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

async fn first_healthy_daemon_uri(cli: &PrismMcpCli, root: &Path) -> Result<Option<String>> {
    for uri in daemon_uri_candidates(cli, root)? {
        if can_connect_uri(&uri).await {
            return Ok(Some(uri));
        }
    }
    Ok(None)
}

fn daemon_uri_candidates(cli: &PrismMcpCli, root: &Path) -> Result<Vec<String>> {
    let mut candidates = Vec::new();
    if let Some(uri) = read_http_uri_file(&cli.http_uri_file_path(root)?)? {
        candidates.push(uri);
    }
    if let Some(state) = runtime_state::read_runtime_state(root)? {
        for process in state.processes {
            if process.kind != "daemon" {
                continue;
            }
            let Some(uri) = process.http_uri else {
                continue;
            };
            if !candidates.contains(&uri) {
                candidates.push(uri);
            }
        }
    }
    Ok(candidates)
}

async fn can_connect_uri(uri: &str) -> bool {
    match uri_authority(uri) {
        Some(authority) => tokio::net::TcpStream::connect(authority).await.is_ok(),
        None => false,
    }
}

fn live_daemon_process_count(root: &Path) -> Result<usize> {
    let output = Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .context("failed to list processes with ps")?;
    if !output.status.success() {
        return Err(anyhow!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let root_flag = format!("--root {}", root.display());
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("prism-mcp"))
        .filter(|line| line.contains("--mode daemon"))
        .filter(|line| line.contains(&root_flag))
        .count())
}

fn uri_authority(uri: &str) -> Option<&str> {
    uri.strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .filter(|authority| !authority.is_empty())
}

fn normalize_route_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let without_trailing = trimmed.trim_end_matches('/');
    if without_trailing.starts_with('/') {
        without_trailing.to_string()
    } else {
        format!("/{without_trailing}")
    }
}

fn auto_bind_host(bind: &str) -> Option<&str> {
    bind.rsplit_once(':')
        .and_then(|(host, port)| (port == "0").then_some(host))
}

fn stable_http_bind_candidates(host: &str, root: &Path) -> Vec<String> {
    let mut hasher = DefaultHasher::new();
    root.to_string_lossy().hash(&mut hasher);
    let offset = (hasher.finish() % u64::from(STABLE_HTTP_PORT_RANGE)) as u16;
    let attempts = STABLE_HTTP_PORT_ATTEMPTS.min(STABLE_HTTP_PORT_RANGE);
    (0..attempts)
        .map(|step| {
            let port = STABLE_HTTP_PORT_BASE + ((offset + step) % STABLE_HTTP_PORT_RANGE);
            format!("{host}:{port}")
        })
        .collect()
}

async fn http_health() -> &'static str {
    "ok"
}

struct HttpUriFileGuard {
    path: PathBuf,
    expected_uri: String,
}

impl Drop for HttpUriFileGuard {
    fn drop(&mut self) {
        let should_remove = match fs::read_to_string(&self.path) {
            Ok(current) => current.trim() == self.expected_uri,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                let error_chain = error.to_string();
                warn!(
                    uri_file = %self.path.display(),
                    error = %error,
                    error_chain = %error_chain,
                    "failed to inspect prism-mcp URI file before cleanup"
                );
                false
            }
        };
        if !should_remove {
            return;
        }
        if let Err(error) = fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                let error_chain = error.to_string();
                warn!(
                    uri_file = %self.path.display(),
                    error = %error,
                    error_chain = %error_chain,
                    "failed to remove prism-mcp URI file"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_bind_host, bind_listener, detached_daemon_launcher_script,
        stable_http_bind_candidates, HttpUriFileGuard, PREFERRED_STABLE_HTTP_BIND_POLL,
    };
    use crate::{PrismMcpCli, PrismMcpMode};
    use prism_core::PrismPaths;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::TcpListener as TokioTcpListener;
    use tokio::time::sleep;

    fn test_cli(root: PathBuf, http_bind: &str) -> PrismMcpCli {
        PrismMcpCli {
            root,
            mode: PrismMcpMode::Daemon,
            no_coordination: false,
            internal_developer: false,
            ui: false,
            shared_runtime_sqlite: None,
            shared_runtime_uri: None,
            restart_nonce: None,
            enable_coordination: Vec::new(),
            disable_coordination: Vec::new(),
            enable_query_view: Vec::new(),
            disable_query_view: Vec::new(),
            daemon_log: None,
            daemon_start_timeout_ms: None,
            http_bind: http_bind.to_string(),
            http_path: "/mcp".to_string(),
            health_path: "/healthz".to_string(),
            http_uri_file: None,
            upstream_uri: None,
            bootstrap_build_worktree_release: false,
            bridge_daemon_binary: None,
            daemonize: false,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism-mcp-daemon-mode-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn uri_guard_only_removes_matching_uri_file() {
        let root = temp_root("uri-guard");
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        fs::write(&uri_path, "http://127.0.0.1:41000/mcp\n").unwrap();

        {
            let _guard = HttpUriFileGuard {
                path: uri_path.clone(),
                expected_uri: "http://127.0.0.1:42000/mcp".to_string(),
            };
        }
        assert!(uri_path.exists());

        {
            let _guard = HttpUriFileGuard {
                path: uri_path.clone(),
                expected_uri: "http://127.0.0.1:41000/mcp".to_string(),
            };
        }
        assert!(!uri_path.exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn auto_bind_host_detects_dynamic_port_inputs() {
        assert_eq!(auto_bind_host("127.0.0.1:0"), Some("127.0.0.1"));
        assert_eq!(auto_bind_host("localhost:0"), Some("localhost"));
        assert_eq!(auto_bind_host("127.0.0.1:4242"), None);
    }

    #[test]
    fn stable_http_bind_candidates_are_deterministic_for_a_root() {
        let root = temp_root("stable-candidates");
        let left = stable_http_bind_candidates("127.0.0.1", &root);
        let right = stable_http_bind_candidates("127.0.0.1", &root);
        assert_eq!(left, right);
        assert!(!left.is_empty());
    }

    #[test]
    fn detached_daemon_launcher_script_is_valid_sh() {
        let status = Command::new("/bin/sh")
            .arg("-n")
            .arg("-c")
            .arg(detached_daemon_launcher_script())
            .status()
            .unwrap();

        assert!(status.success());
    }

    #[tokio::test]
    async fn bind_listener_reuses_the_same_workspace_port_after_restart() {
        let root = temp_root("rebind");
        let cli = test_cli(root.clone(), "127.0.0.1:0");

        let first = bind_listener(&cli, &root).await.unwrap();
        let first_addr = first.local_addr().unwrap();
        drop(first);

        let second = bind_listener(&cli, &root).await.unwrap();
        let second_addr = second.local_addr().unwrap();

        assert_eq!(first_addr, second_addr);
    }

    #[tokio::test]
    async fn bind_listener_waits_for_the_preferred_workspace_port_before_fallback() {
        let root = temp_root("preferred-port-wait");
        let cli = test_cli(root.clone(), "127.0.0.1:0");
        let preferred = stable_http_bind_candidates("127.0.0.1", &root)
            .into_iter()
            .next()
            .unwrap();

        let blocker = TokioTcpListener::bind(&preferred).await.unwrap();
        let release = tokio::spawn(async move {
            sleep(PREFERRED_STABLE_HTTP_BIND_POLL * 2).await;
            drop(blocker);
        });

        let listener = bind_listener(&cli, &root).await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        release.await.unwrap();
        assert_eq!(addr, preferred);
    }
}
