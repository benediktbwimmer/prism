use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use rmcp::ServiceExt;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::sleep;

use crate::{PrismMcpCli, PrismMcpServer};

const DEFAULT_DAEMON_START_TIMEOUT_MS: u64 = 5_000;

pub(crate) fn default_socket_path(root: &Path) -> PathBuf {
    root.join(".prism").join("prism-mcp.sock")
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
    let socket_path = cli.socket_path(root);
    prepare_socket_path(&socket_path).await?;
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind unix socket {}", socket_path.display()))?;
    let _socket_guard = SocketGuard(socket_path.clone());
    let server = PrismMcpServer::from_workspace_with_features(root, cli.features())?;

    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                eprintln!("prism daemon accept error: {error:#}");
                sleep(Duration::from_millis(50)).await;
                continue;
            }
        };

        let service = match server.clone().serve(stream).await {
            Ok(service) => service,
            Err(error) => {
                eprintln!("prism daemon transport setup error: {error:#}");
                continue;
            }
        };

        if let Err(error) = service.waiting().await {
            eprintln!("prism daemon session ended with error: {error:#}");
        }
    }
}

async fn run_bridge(cli: &PrismMcpCli, root: &Path) -> Result<()> {
    let socket_path = cli.socket_path(root);
    ensure_daemon_running(cli, root, &socket_path).await?;

    let stream = UnixStream::connect(&socket_path).await.with_context(|| {
        format!(
            "failed to connect to prism daemon at {}",
            socket_path.display()
        )
    })?;
    let (mut socket_read, mut socket_write) = io::split(stream);
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    let to_daemon = async {
        io::copy(&mut stdin, &mut socket_write).await?;
        socket_write.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let from_daemon = async {
        io::copy(&mut socket_read, &mut stdout).await?;
        stdout.flush().await?;
        Ok::<(), anyhow::Error>(())
    };

    tokio::try_join!(to_daemon, from_daemon)?;
    Ok(())
}

async fn ensure_daemon_running(cli: &PrismMcpCli, root: &Path, socket_path: &Path) -> Result<()> {
    if can_connect(socket_path).await {
        return Ok(());
    }

    spawn_daemon(cli, root)?;
    let timeout = Duration::from_millis(
        cli.daemon_start_timeout_ms
            .unwrap_or(DEFAULT_DAEMON_START_TIMEOUT_MS),
    );
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if can_connect(socket_path).await {
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }

    Err(anyhow!(
        "timed out waiting for prism daemon socket at {}",
        socket_path.display()
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
            "failed to spawn prism daemon from {}",
            current_exe.display()
        )
    })?;
    if !status.success() {
        return Err(anyhow!(
            "failed to detach prism daemon launcher for {}: {status}",
            current_exe.display()
        ));
    }
    Ok(())
}

async fn prepare_socket_path(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !socket_path.exists() {
        return Ok(());
    }

    if can_connect(socket_path).await {
        return Err(anyhow!(
            "prism daemon already appears to be running at {}",
            socket_path.display()
        ));
    }

    fs::remove_file(socket_path)
        .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    Ok(())
}

async fn can_connect(socket_path: &Path) -> bool {
    UnixStream::connect(socket_path).await.is_ok()
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
