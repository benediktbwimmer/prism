use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use axum::{routing::get, Router};
use rmcp::transport::{
    streamable_http_server::session::local::LocalSessionManager, StreamableHttpServerConfig,
    StreamableHttpService,
};
use tokio::net::TcpListener;
use tokio::time::sleep;

use crate::proxy_server::ProxyMcpServer;
use crate::{PrismMcpCli, PrismMcpServer};

const DEFAULT_DAEMON_START_TIMEOUT_MS: u64 = 5_000;

pub(crate) fn default_http_uri_file_path(root: &Path) -> PathBuf {
    root.join(".prism").join("prism-mcp-http-uri")
}

pub(crate) fn default_log_path(root: &Path) -> PathBuf {
    root.join(".prism").join("prism-mcp-daemon.log")
}

pub async fn serve_with_mode(cli: PrismMcpCli) -> Result<()> {
    let root = cli.root.canonicalize()?;
    match cli.mode {
        crate::PrismMcpMode::Stdio => {
            let server = PrismMcpServer::from_workspace_with_features(&root, cli.features())?;
            server.serve_stdio().await
        }
        crate::PrismMcpMode::Daemon => run_daemon(&cli, &root).await,
        crate::PrismMcpMode::Bridge => run_bridge(&cli, &root).await,
    }
}

async fn run_daemon(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let server = PrismMcpServer::from_workspace_with_features(root, cli.features())?;
    let listener = TcpListener::bind(&cli.http_bind).await.with_context(|| {
        format!(
            "failed to bind PRISM MCP HTTP listener at {}",
            cli.http_bind
        )
    })?;
    let mcp_path = normalize_route_path(&cli.http_path);
    let health_path = normalize_route_path(&cli.health_path);
    let addr = listener.local_addr()?;
    let http_uri = format!("http://{addr}{mcp_path}");
    let uri_file_path = cli.http_uri_file_path(root);
    write_http_uri_file(&uri_file_path, &http_uri)?;
    let _uri_guard = HttpUriFileGuard(uri_file_path);

    let service_server = server.clone();
    let service: StreamableHttpService<PrismMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(service_server.clone()),
            Default::default(),
            StreamableHttpServerConfig::default(),
        );
    let router = Router::new()
        .route(&health_path, get(http_health))
        .nest_service(&mcp_path, service);

    axum::serve(listener, router)
        .await
        .context("PRISM MCP HTTP server exited unexpectedly")
}

async fn run_bridge(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let upstream_uri = resolve_upstream_uri(cli, root).await?;
    let proxy = ProxyMcpServer::connect(upstream_uri).await?;
    proxy.serve_stdio().await
}

async fn resolve_upstream_uri(cli: &PrismMcpCli, root: &Path) -> Result<String> {
    if let Some(uri) = &cli.upstream_uri {
        return Ok(uri.clone());
    }

    ensure_daemon_running(cli, root).await
}

async fn ensure_daemon_running(cli: &PrismMcpCli, root: &Path) -> Result<String> {
    if let Some(uri) = read_http_uri_file(&cli.http_uri_file_path(root))? {
        if can_connect_uri(&uri).await {
            return Ok(uri);
        }
    }

    spawn_daemon(cli, root)?;
    let timeout = Duration::from_millis(
        cli.daemon_start_timeout_ms
            .unwrap_or(DEFAULT_DAEMON_START_TIMEOUT_MS),
    );
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(uri) = read_http_uri_file(&cli.http_uri_file_path(root))? {
            if can_connect_uri(&uri).await {
                return Ok(uri);
            }
        }
        sleep(Duration::from_millis(50)).await;
    }

    Err(anyhow!(
        "timed out waiting for PRISM MCP HTTP daemon URI file at {}",
        cli.http_uri_file_path(root).display()
    ))
}

fn spawn_daemon(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let log_path = cli.log_path(root);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let current_exe = std::env::current_exe()?;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(
            r#"log_path="$1"
shift
exe="$1"
shift
nohup "$exe" "$@" >>"$log_path" 2>&1 </dev/null &
"#,
        )
        .arg("prism-mcp-daemon-launcher")
        .arg(&log_path)
        .arg(&current_exe)
        .args(cli.daemon_spawn_args(root))
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

async fn can_connect_uri(uri: &str) -> bool {
    match uri_authority(uri) {
        Some(authority) => tokio::net::TcpStream::connect(authority).await.is_ok(),
        None => false,
    }
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

async fn http_health() -> &'static str {
    "ok"
}

struct HttpUriFileGuard(PathBuf);

impl Drop for HttpUriFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
