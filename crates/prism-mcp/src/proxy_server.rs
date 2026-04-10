use std::collections::HashMap;
use std::env;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use rmcp::{
    model::*,
    service::{Peer, RequestContext, RoleClient, RoleServer, RunningService, ServiceError},
    transport::{stdio, StreamableHttpClientTransport},
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::{self, JoinHandle};
use tracing::{info, warn};

use crate::bridge_auth::{BridgeAuthContext, BRIDGE_ADOPT_TOOL_NAME, BRIDGE_AUTH_URI};
use crate::daemon_mode::BridgeUpstreamSource;
use crate::*;

const DEFAULT_BRIDGE_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(100);
const DEFAULT_BRIDGE_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_BRIDGE_RECONNECT_TIMEOUT: Duration = Duration::from_secs(180);
const DEFAULT_BRIDGE_REQUEST_RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_UPSTREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_UPSTREAM_REQUEST_RETRY_ATTEMPTS: usize = 3;
const DEFAULT_STARTUP_POLL_AFTER_MS: u64 = 3_000;
const DEFAULT_BUILD_POLL_AFTER_MS: u64 = 10_000;

fn fast_proxy_reconnect_enabled() -> bool {
    env::var_os("PRISM_TEST_FAST_PROXY_RECONNECT")
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false)
}

fn bridge_reconnect_base_delay() -> Duration {
    if fast_proxy_reconnect_enabled() {
        Duration::from_millis(25)
    } else {
        DEFAULT_BRIDGE_RECONNECT_BASE_DELAY
    }
}

fn bridge_reconnect_max_delay() -> Duration {
    if fast_proxy_reconnect_enabled() {
        Duration::from_millis(250)
    } else {
        DEFAULT_BRIDGE_RECONNECT_MAX_DELAY
    }
}

fn bridge_request_reconnect_timeout() -> Duration {
    if fast_proxy_reconnect_enabled() {
        Duration::from_secs(2)
    } else {
        DEFAULT_BRIDGE_REQUEST_RECONNECT_TIMEOUT
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum BridgeStartupPhase {
    BuildingRelease,
    StartingDaemon,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeStartupPayload {
    uri: String,
    phase: BridgeStartupPhase,
    ready: bool,
    message: String,
    next_action: String,
    poll_after_ms: u64,
    started_at_ms: u64,
    selected_root: String,
    bridge_binary: String,
    daemon_binary: Option<String>,
    upstream_uri: Option<String>,
    daemon_log_path: Option<String>,
    build_log_path: Option<String>,
    error: Option<String>,
}

impl BridgeStartupPayload {
    fn pending(
        root: &Path,
        bridge_binary: PathBuf,
        daemon_binary: Option<PathBuf>,
        daemon_log_path: Option<PathBuf>,
        build_log_path: Option<PathBuf>,
    ) -> Self {
        Self {
            uri: STARTUP_URI.to_string(),
            phase: BridgeStartupPhase::StartingDaemon,
            ready: false,
            message: "PRISM bridge warmup is in progress.".to_string(),
            next_action: format!(
                "Wait a few seconds, then read {STARTUP_URI} again before using PRISM tools."
            ),
            poll_after_ms: DEFAULT_STARTUP_POLL_AFTER_MS,
            started_at_ms: current_timestamp_ms(),
            selected_root: root.display().to_string(),
            bridge_binary: bridge_binary.display().to_string(),
            daemon_binary: daemon_binary.map(|path| path.display().to_string()),
            upstream_uri: None,
            daemon_log_path: daemon_log_path.map(|path| path.display().to_string()),
            build_log_path: build_log_path.map(|path| path.display().to_string()),
            error: None,
        }
    }
}

#[derive(Debug)]
struct BridgeStartupState {
    payload: RwLock<BridgeStartupPayload>,
    client_peer: Mutex<Option<Peer<RoleServer>>>,
}

impl BridgeStartupState {
    fn new(payload: BridgeStartupPayload) -> Self {
        Self {
            payload: RwLock::new(payload),
            client_peer: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> BridgeStartupPayload {
        self.payload
            .read()
            .map(|payload| payload.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    fn capture_peer(&self, peer: &Peer<RoleServer>) {
        match self.client_peer.lock() {
            Ok(mut slot) => *slot = Some(peer.clone()),
            Err(poisoned) => *poisoned.into_inner() = Some(peer.clone()),
        }
    }

    fn update_phase(&self, phase: BridgeStartupPhase, message: String, poll_after_ms: u64) {
        match self.payload.write() {
            Ok(mut payload) => {
                payload.phase = phase;
                payload.ready = matches!(phase, BridgeStartupPhase::Ready);
                payload.message = message;
                payload.poll_after_ms = poll_after_ms;
                payload.next_action = if payload.ready {
                    "PRISM tools are ready.".to_string()
                } else {
                    format!(
                        "Wait at least {} seconds, then read {STARTUP_URI} again before using PRISM tools.",
                        (poll_after_ms / 1000).max(1)
                    )
                };
            }
            Err(poisoned) => {
                let mut payload = poisoned.into_inner();
                payload.phase = phase;
                payload.ready = matches!(phase, BridgeStartupPhase::Ready);
                payload.message = message;
                payload.poll_after_ms = poll_after_ms;
                payload.next_action = if payload.ready {
                    "PRISM tools are ready.".to_string()
                } else {
                    format!(
                        "Wait at least {} seconds, then read {STARTUP_URI} again before using PRISM tools.",
                        (poll_after_ms / 1000).max(1)
                    )
                };
            }
        }
    }

    fn mark_building_release(&self) {
        self.update_phase(
            BridgeStartupPhase::BuildingRelease,
            "Building release binaries for this worktree.".to_string(),
            DEFAULT_BUILD_POLL_AFTER_MS,
        );
    }

    fn mark_starting_daemon(&self) {
        self.update_phase(
            BridgeStartupPhase::StartingDaemon,
            "Starting or reconnecting the PRISM runtime for this worktree.".to_string(),
            DEFAULT_STARTUP_POLL_AFTER_MS,
        );
    }

    fn mark_ready(&self, upstream_uri: &str) {
        match self.payload.write() {
            Ok(mut payload) => {
                payload.phase = BridgeStartupPhase::Ready;
                payload.ready = true;
                payload.message = format!("PRISM bridge is ready and connected to {upstream_uri}.");
                payload.next_action = "PRISM tools are ready.".to_string();
                payload.poll_after_ms = 0;
                payload.upstream_uri = Some(upstream_uri.to_string());
                payload.error = None;
            }
            Err(poisoned) => {
                let mut payload = poisoned.into_inner();
                payload.phase = BridgeStartupPhase::Ready;
                payload.ready = true;
                payload.message = format!("PRISM bridge is ready and connected to {upstream_uri}.");
                payload.next_action = "PRISM tools are ready.".to_string();
                payload.poll_after_ms = 0;
                payload.upstream_uri = Some(upstream_uri.to_string());
                payload.error = None;
            }
        }
    }

    fn mark_failed(&self, error: &str) {
        match self.payload.write() {
            Ok(mut payload) => {
                payload.phase = BridgeStartupPhase::Failed;
                payload.ready = false;
                payload.message = "PRISM bridge startup failed.".to_string();
                payload.next_action = format!(
                    "Inspect the startup logs, fix the failure, then reopen the bridge or read {STARTUP_URI} again."
                );
                payload.poll_after_ms = DEFAULT_BUILD_POLL_AFTER_MS;
                payload.error = Some(error.to_string());
            }
            Err(poisoned) => {
                let mut payload = poisoned.into_inner();
                payload.phase = BridgeStartupPhase::Failed;
                payload.ready = false;
                payload.message = "PRISM bridge startup failed.".to_string();
                payload.next_action = format!(
                    "Inspect the startup logs, fix the failure, then reopen the bridge or read {STARTUP_URI} again."
                );
                payload.poll_after_ms = DEFAULT_BUILD_POLL_AFTER_MS;
                payload.error = Some(error.to_string());
            }
        }
    }

    fn startup_resource(&self) -> Resource {
        Annotated::new(
            RawResource::new(STARTUP_URI, "PRISM Startup")
                .with_description(
                    "Bridge startup status for release builds, runtime readiness, and PRISM warmup guidance.",
                )
                .with_mime_type("application/json"),
            None,
        )
    }

    fn startup_resource_contents(&self) -> ResourceContents {
        ResourceContents::text(
            serde_json::to_string_pretty(&self.snapshot())
                .expect("startup payload should serialize"),
            STARTUP_URI,
        )
        .with_mime_type("application/json")
    }

    fn startup_instructions_suffix(&self) -> String {
        let payload = self.snapshot();
        if payload.ready {
            "This bridge has finished warming up and can proxy PRISM requests normally.".to_string()
        } else {
            format!(
                "This bridge is warming up PRISM for `{}`. Before using PRISM tools, read `{}` and wait until `phase` becomes `ready`. If startup is still in progress, wait about {} seconds and read `{}` again.",
                payload.selected_root,
                STARTUP_URI,
                (payload.poll_after_ms / 1000).max(1),
                STARTUP_URI
            )
        }
    }

    async fn notify_ready_surface(&self) {
        let peer = match self.client_peer.lock() {
            Ok(slot) => slot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let Some(peer) = peer else {
            return;
        };
        let _ = peer
            .notify_resource_updated(ResourceUpdatedNotificationParam {
                uri: STARTUP_URI.to_string(),
            })
            .await;
        let _ = peer.notify_resource_list_changed().await;
        let _ = peer.notify_tool_list_changed().await;
    }
}

#[derive(Debug)]
struct ProxyActivityTracker {
    in_flight: AtomicUsize,
}

impl ProxyActivityTracker {
    fn new() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
        }
    }

    fn begin_request(self: &Arc<Self>) -> ProxyRequestGuard {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        ProxyRequestGuard {
            tracker: Arc::clone(self),
        }
    }
}

struct ProxyRequestGuard {
    tracker: Arc<ProxyActivityTracker>,
}

impl Drop for ProxyRequestGuard {
    fn drop(&mut self) {
        self.tracker.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

struct UpstreamConnection {
    _client: RunningService<RoleClient, ()>,
    peer: rmcp::service::Peer<RoleClient>,
}

pub(crate) struct ProxyMcpServer {
    root: PathBuf,
    reconnect_cli: Option<PrismMcpCli>,
    upstream: Arc<AsyncMutex<Option<UpstreamConnection>>>,
    upstream_source: BridgeUpstreamSource,
    reconnect_lock: Arc<AsyncMutex<()>>,
    server_info: Arc<Mutex<ServerInfo>>,
    tool_cache: Arc<RwLock<HashMap<String, Tool>>>,
    activity: Arc<ProxyActivityTracker>,
    bridge_auth: BridgeAuthContext,
    startup: Arc<BridgeStartupState>,
    warmup_task: AsyncMutex<Option<JoinHandle<()>>>,
}

impl ProxyMcpServer {
    #[cfg(test)]
    pub(crate) async fn connect_with_source(
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        Self::connect_with_bridge_auth(upstream_uri, upstream_source, BridgeAuthContext::disabled())
            .await
    }

    #[cfg(test)]
    pub(crate) async fn connect_with_root(
        root: std::path::PathBuf,
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        Self::connect_with_bridge_auth(
            upstream_uri,
            upstream_source,
            BridgeAuthContext::from_root(root),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) fn pending_for_test(root: &Path, features: PrismMcpFeatures) -> Result<Self> {
        let startup = Arc::new(BridgeStartupState::new(BridgeStartupPayload::pending(
            root,
            std::env::current_exe().context("failed to resolve bridge executable path")?,
            None,
            None,
            None,
        )));
        Ok(Self {
            root: root.to_path_buf(),
            reconnect_cli: None,
            upstream: Arc::new(AsyncMutex::new(None)),
            upstream_source: BridgeUpstreamSource::Fixed("http://127.0.0.1:9/mcp".to_string()),
            reconnect_lock: Arc::new(AsyncMutex::new(())),
            server_info: Arc::new(Mutex::new(bootstrap_server_info())),
            tool_cache: Arc::new(RwLock::new(bootstrap_tool_cache(features))),
            activity: Arc::new(ProxyActivityTracker::new()),
            bridge_auth: BridgeAuthContext::disabled(),
            startup,
            warmup_task: AsyncMutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn failed_for_test(
        root: &Path,
        features: PrismMcpFeatures,
        error: &str,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        let startup = Arc::new(BridgeStartupState::new(BridgeStartupPayload::pending(
            root,
            std::env::current_exe().context("failed to resolve bridge executable path")?,
            None,
            None,
            None,
        )));
        startup.mark_failed(error);
        Ok(Self {
            root: root.to_path_buf(),
            reconnect_cli: None,
            upstream: Arc::new(AsyncMutex::new(None)),
            upstream_source,
            reconnect_lock: Arc::new(AsyncMutex::new(())),
            server_info: Arc::new(Mutex::new(bootstrap_server_info())),
            tool_cache: Arc::new(RwLock::new(bootstrap_tool_cache(features))),
            activity: Arc::new(ProxyActivityTracker::new()),
            bridge_auth: BridgeAuthContext::disabled(),
            startup,
            warmup_task: AsyncMutex::new(None),
        })
    }

    #[cfg(test)]
    async fn connect_with_bridge_auth(
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
        bridge_auth: BridgeAuthContext,
    ) -> Result<Self> {
        let (connection, server_info, tools) = Self::open_upstream(&upstream_uri).await?;
        let startup = Arc::new(BridgeStartupState::new(BridgeStartupPayload {
            uri: STARTUP_URI.to_string(),
            phase: BridgeStartupPhase::Ready,
            ready: true,
            message: format!("PRISM bridge is ready and connected to {upstream_uri}."),
            next_action: "PRISM tools are ready.".to_string(),
            poll_after_ms: 0,
            started_at_ms: current_timestamp_ms(),
            selected_root: String::new(),
            bridge_binary: std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string()),
            daemon_binary: None,
            upstream_uri: Some(upstream_uri.clone()),
            daemon_log_path: None,
            build_log_path: None,
            error: None,
        }));
        Ok(Self {
            root: PathBuf::new(),
            reconnect_cli: None,
            upstream: Arc::new(AsyncMutex::new(Some(connection))),
            upstream_source,
            reconnect_lock: Arc::new(AsyncMutex::new(())),
            server_info: Arc::new(Mutex::new(server_info)),
            tool_cache: Arc::new(RwLock::new(
                tools
                    .into_iter()
                    .map(|tool| (tool.name.to_string(), tool))
                    .collect(),
            )),
            activity: Arc::new(ProxyActivityTracker::new()),
            bridge_auth,
            startup,
            warmup_task: AsyncMutex::new(None),
        })
    }

    pub(crate) async fn bootstrap_with_source_for_root(
        root: &Path,
        cli: PrismMcpCli,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        let features = cli.features();
        let startup = Arc::new(BridgeStartupState::new(BridgeStartupPayload::pending(
            root,
            std::env::current_exe().context("failed to resolve bridge executable path")?,
            cli.bridge_daemon_binary.clone(),
            cli.log_path(root).ok(),
            bootstrap_build_log_path(root).ok(),
        )));
        let tool_cache = bootstrap_tool_cache(features);
        let server = Self {
            root: root.to_path_buf(),
            reconnect_cli: Some(cli.clone()),
            upstream: Arc::new(AsyncMutex::new(None)),
            upstream_source: upstream_source.clone(),
            reconnect_lock: Arc::new(AsyncMutex::new(())),
            server_info: Arc::new(Mutex::new(bootstrap_server_info())),
            tool_cache: Arc::new(RwLock::new(tool_cache)),
            activity: Arc::new(ProxyActivityTracker::new()),
            bridge_auth: BridgeAuthContext::for_root(root)?,
            startup: Arc::clone(&startup),
            warmup_task: AsyncMutex::new(None),
        };
        let task = tokio::spawn(Self::warm_up(
            root.to_path_buf(),
            cli,
            Arc::clone(&startup),
            Arc::clone(&server.tool_cache),
            Arc::clone(&server.server_info),
            Arc::clone(&server.upstream),
        ));
        *server.warmup_task.lock().await = Some(task);
        Ok(server)
    }

    pub(crate) async fn serve_stdio(self) -> Result<()> {
        let running = self.serve(stdio()).await?;
        running
            .waiting()
            .await
            .map(|_| ())
            .context("PRISM MCP bridge transport exited unexpectedly")
    }

    async fn open_upstream(
        upstream_uri: &str,
    ) -> Result<(UpstreamConnection, ServerInfo, Vec<Tool>)> {
        let client = ()
            .serve(StreamableHttpClientTransport::from_uri(
                upstream_uri.to_string(),
            ))
            .await
            .with_context(|| {
                format!("failed to connect to upstream PRISM MCP server at {upstream_uri}")
            })?;
        let peer = client.peer().clone();
        let server_info = peer
            .peer_info()
            .cloned()
            .ok_or_else(|| anyhow!("upstream PRISM MCP server did not complete initialize"))?;
        let tools = peer
            .list_all_tools()
            .await
            .context("failed to list upstream PRISM MCP tools")?;
        Ok((
            UpstreamConnection {
                _client: client,
                peer,
            },
            server_info,
            tools,
        ))
    }

    fn update_tool_cache(&self, tools: &[Tool]) {
        if let Ok(mut cache) = self.tool_cache.write() {
            cache.clear();
            cache.extend(
                tools
                    .iter()
                    .cloned()
                    .map(|tool| (tool.name.to_string(), tool)),
            );
        }
    }

    fn update_server_info(&self, server_info: &ServerInfo) {
        match self.server_info.lock() {
            Ok(mut current) => *current = server_info.clone(),
            Err(poisoned) => *poisoned.into_inner() = server_info.clone(),
        }
    }

    fn inject_bridge_identity_into_session_content(
        &self,
        content: ResourceContents,
    ) -> Result<ResourceContents, McpError> {
        match content {
            ResourceContents::TextResourceContents {
                uri, text, meta, ..
            } => {
                let mut payload: Value = serde_json::from_str(&text).map_err(|error| {
                    McpError::internal_error(
                        "failed to parse upstream session resource payload",
                        Some(json!({ "error": error.to_string() })),
                    )
                })?;
                let object = payload.as_object_mut().ok_or_else(|| {
                    McpError::internal_error(
                        "upstream session resource payload must be a JSON object",
                        None,
                    )
                })?;
                object.insert(
                    "bridgeIdentity".to_string(),
                    serde_json::to_value(self.bridge_auth.session_bridge_identity()).map_err(
                        |error| {
                            McpError::internal_error(
                                "failed to serialize bridge identity for the session resource",
                                Some(json!({ "error": error.to_string() })),
                            )
                        },
                    )?,
                );
                json_resource_contents_with_meta(payload, uri, meta)
            }
            other => Ok(other),
        }
    }

    fn inject_bridge_identity_into_session_result(
        &self,
        mut result: ReadResourceResult,
    ) -> Result<ReadResourceResult, McpError> {
        result.contents = result
            .contents
            .into_iter()
            .map(|content| self.inject_bridge_identity_into_session_content(content))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(result)
    }

    async fn recover_failed_startup_if_needed(&self) -> Result<()> {
        let payload = self.startup.snapshot();
        if payload.ready || payload.error.is_none() {
            return Ok(());
        }

        self.reconnect_with_backoff(
            "startup previously failed before an upstream was available",
            true,
            DEFAULT_BRIDGE_RECONNECT_TIMEOUT,
        )
        .await
    }

    async fn active_peer(&self) -> Result<rmcp::service::Peer<RoleClient>> {
        let peer = {
            let upstream = self.upstream.lock().await;
            upstream.as_ref().map(|upstream| upstream.peer.clone())
        };
        if let Some(peer) = peer {
            if !peer.is_transport_closed() {
                return Ok(peer);
            }
            self.reconnect_with_backoff(
                "upstream transport closed before request",
                false,
                bridge_request_reconnect_timeout(),
            )
            .await?;
            let upstream = self.upstream.lock().await;
            if let Some(upstream) = upstream.as_ref() {
                return Ok(upstream.peer.clone());
            }
        }
        self.recover_failed_startup_if_needed().await?;
        let upstream = self.upstream.lock().await;
        if let Some(upstream) = upstream.as_ref() {
            if !upstream.peer.is_transport_closed() {
                return Ok(upstream.peer.clone());
            }
        }
        let payload = self.startup.snapshot();
        if payload.ready {
            Err(anyhow!(
                "PRISM bridge is ready but no upstream connection is available yet; retry shortly"
            ))
        } else if let Some(error) = payload.error {
            Err(anyhow!(error))
        } else {
            Err(anyhow!(
                "PRISM bridge is warming up; read {STARTUP_URI} and retry after {} ms",
                payload.poll_after_ms
            ))
        }
    }

    async fn reconnect_with_backoff(
        &self,
        reason: &str,
        force: bool,
        timeout: Duration,
    ) -> Result<()> {
        let _reconnect = self.reconnect_lock.lock().await;
        if !force {
            let upstream = self.upstream.lock().await;
            if upstream
                .as_ref()
                .is_some_and(|upstream| !upstream.peer.is_transport_closed())
            {
                return Ok(());
            }
        }
        {
            let mut upstream = self.upstream.lock().await;
            *upstream = None;
        }

        let started = Instant::now();
        let mut attempt = 0usize;
        let mut delay = bridge_reconnect_base_delay();
        let mut last_error = None;
        while started.elapsed() < timeout {
            attempt += 1;
            let upstream_uri = match self.resolve_reconnect_upstream().await {
                Ok(uri) => uri,
                Err(error) => {
                    last_error = Some(error);
                    warn!(
                        attempt,
                        reason,
                        delay_ms = delay.as_millis(),
                        "prism-mcp bridge failed to resolve reconnect upstream; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2).min(bridge_reconnect_max_delay());
                    continue;
                }
            };

            match Self::open_upstream(&upstream_uri).await {
                Ok((connection, server_info, tools)) => {
                    {
                        let mut upstream = self.upstream.lock().await;
                        *upstream = Some(connection);
                    }
                    self.update_server_info(&server_info);
                    self.update_tool_cache(&tools);
                    self.startup.mark_ready(&upstream_uri);
                    self.startup.notify_ready_surface().await;
                    info!(
                        attempt,
                        reason,
                        upstream_uri = %upstream_uri,
                        "prism-mcp bridge reconnected to upstream"
                    );
                    return Ok(());
                }
                Err(error) => {
                    last_error = Some(error);
                    if let Some(error_value) = last_error.as_ref() {
                        if let Err(runtime_state_error) =
                            crate::runtime_state::record_bridge_connection_failure(
                                &self.root,
                                &upstream_uri,
                                "reconnect",
                                reason,
                                Some(attempt),
                                Some(delay.as_millis()),
                                &error_value.to_string(),
                            )
                        {
                            warn!(
                                error = %runtime_state_error,
                                root = %self.root.display(),
                                upstream_uri = %upstream_uri,
                                "failed to update prism runtime state for bridge reconnect failure"
                            );
                        }
                    }
                    warn!(
                        attempt,
                        reason,
                        upstream_uri = %upstream_uri,
                        delay_ms = delay.as_millis(),
                        "prism-mcp bridge failed to reconnect to upstream; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2).min(bridge_reconnect_max_delay());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("failed to reconnect PRISM MCP bridge upstream")))
    }

    async fn resolve_reconnect_upstream(&self) -> Result<String> {
        let Some(cli) = self.reconnect_cli.as_ref() else {
            return self.upstream_source.read_uri();
        };

        let resolution_started = Instant::now();
        let resolution = crate::daemon_mode::resolve_upstream_uri(cli, &self.root).await?;
        if let Err(error) = crate::runtime_state::record_bridge_upstream_resolved(
            &self.root,
            &resolution.uri,
            resolution.source,
            resolution_started.elapsed().as_millis(),
            resolution.daemon_wait_ms,
            resolution.spawned_daemon,
        ) {
            warn!(
                error = %error,
                root = %self.root.display(),
                upstream_uri = %resolution.uri,
                "failed to update prism runtime state for bridge reconnect upstream resolution"
            );
        }
        Ok(resolution.uri)
    }

    fn should_reconnect(error: &ServiceError) -> bool {
        matches!(
            error,
            ServiceError::TransportClosed
                | ServiceError::TransportSend(_)
                | ServiceError::Timeout { .. }
        )
    }

    async fn call_upstream<Request, Output, Op, Fut>(
        &self,
        request: Request,
        op_name: &'static str,
        op: Op,
    ) -> Result<Output, McpError>
    where
        Request: Clone + Send,
        Output: Send,
        Op: Fn(rmcp::service::Peer<RoleClient>, Request) -> Fut + Copy + Send,
        Fut: Future<Output = Result<Output, ServiceError>> + Send,
    {
        for attempt in 1..=DEFAULT_UPSTREAM_REQUEST_RETRY_ATTEMPTS {
            let peer = self.active_peer().await.map_err(map_connect_error)?;
            match tokio::time::timeout(DEFAULT_UPSTREAM_REQUEST_TIMEOUT, op(peer, request.clone()))
                .await
            {
                Ok(Ok(result)) => return Ok(result),
                Ok(Err(error))
                    if Self::should_reconnect(&error)
                        && attempt < DEFAULT_UPSTREAM_REQUEST_RETRY_ATTEMPTS =>
                {
                    warn!(
                        operation = op_name,
                        attempt,
                        error = %error,
                        "prism-mcp bridge request failed because the upstream transport was unavailable; rebuilding upstream client"
                    );
                    self.reconnect_with_backoff(op_name, true, bridge_request_reconnect_timeout())
                        .await
                        .map_err(map_connect_error)?;
                }
                Ok(Err(error)) => return Err(map_proxy_error(error)),
                Err(_) if attempt < DEFAULT_UPSTREAM_REQUEST_RETRY_ATTEMPTS => {
                    warn!(
                        operation = op_name,
                        attempt,
                        timeout_ms = DEFAULT_UPSTREAM_REQUEST_TIMEOUT.as_millis(),
                        "prism-mcp bridge request timed out while waiting for upstream; rebuilding upstream client"
                    );
                    self.reconnect_with_backoff(op_name, true, bridge_request_reconnect_timeout())
                        .await
                        .map_err(map_connect_error)?;
                }
                Err(_) => {
                    return Err(McpError::internal_error(
                        format!(
                            "upstream {op_name} timed out after {} ms",
                            DEFAULT_UPSTREAM_REQUEST_TIMEOUT.as_millis()
                        ),
                        None,
                    ))
                }
            }
        }
        Err(McpError::internal_error(
            format!(
                "upstream {op_name} remained unavailable after {} reconnect attempts",
                DEFAULT_UPSTREAM_REQUEST_RETRY_ATTEMPTS - 1
            ),
            None,
        ))
    }

    #[cfg(test)]
    pub(crate) async fn serve_transport<T, E, A>(self, transport: T) -> Result<()>
    where
        T: rmcp::transport::IntoTransport<RoleServer, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let running = self.serve(transport).await?;
        running
            .waiting()
            .await
            .map(|_| ())
            .context("PRISM MCP bridge transport exited unexpectedly")
    }

    async fn warm_up(
        root: PathBuf,
        cli: PrismMcpCli,
        startup: Arc<BridgeStartupState>,
        tool_cache: Arc<RwLock<HashMap<String, Tool>>>,
        server_info: Arc<Mutex<ServerInfo>>,
        upstream_slot: Arc<AsyncMutex<Option<UpstreamConnection>>>,
    ) {
        if cli.bootstrap_build_worktree_release {
            startup.mark_building_release();
            let build_log_path = bootstrap_build_log_path(&root).ok();
            if let Err(error) = build_release_binaries(&root, build_log_path.as_deref()).await {
                warn!(error = %error, root = %root.display(), "failed to build worktree release binaries for bridge bootstrap");
                startup.mark_failed(&error.to_string());
                startup.notify_ready_surface().await;
                return;
            }
        }

        startup.mark_starting_daemon();
        let resolution_started = Instant::now();
        let upstream_resolution = crate::daemon_mode::resolve_upstream_uri(&cli, &root).await;
        let upstream_resolution = match upstream_resolution {
            Ok(resolution) => resolution,
            Err(error) => {
                warn!(error = %error, root = %root.display(), "failed to resolve bridge upstream");
                startup.mark_failed(&error.to_string());
                startup.notify_ready_surface().await;
                return;
            }
        };

        if let Err(error) = crate::runtime_state::record_bridge_upstream_resolved(
            &root,
            &upstream_resolution.uri,
            upstream_resolution.source,
            resolution_started.elapsed().as_millis(),
            upstream_resolution.daemon_wait_ms,
            upstream_resolution.spawned_daemon,
        ) {
            warn!(
                error = %error,
                root = %root.display(),
                "failed to update prism runtime state for bridge upstream resolution"
            );
        }

        let connect_started = Instant::now();
        match Self::open_upstream(&upstream_resolution.uri).await {
            Ok((connection, info, tools)) => {
                {
                    let mut upstream = upstream_slot.lock().await;
                    *upstream = Some(connection);
                }
                match tool_cache.write() {
                    Ok(mut cache) => {
                        cache.clear();
                        cache.extend(
                            tools
                                .iter()
                                .cloned()
                                .map(|tool| (tool.name.to_string(), tool)),
                        );
                    }
                    Err(poisoned) => {
                        let mut cache = poisoned.into_inner();
                        cache.clear();
                        cache.extend(
                            tools
                                .iter()
                                .cloned()
                                .map(|tool| (tool.name.to_string(), tool)),
                        );
                    }
                }
                match server_info.lock() {
                    Ok(mut slot) => *slot = info,
                    Err(poisoned) => *poisoned.into_inner() = info,
                }
                if let Err(error) = crate::runtime_state::record_bridge_connected_with_latency(
                    &root,
                    &upstream_resolution.uri,
                    Some(connect_started.elapsed().as_millis()),
                ) {
                    warn!(
                        error = %error,
                        root = %root.display(),
                        "failed to update prism runtime state for bridge connection"
                    );
                }
                info!(
                    root = %root.display(),
                    upstream_uri = %upstream_resolution.uri,
                    "prism-mcp bridge connected"
                );
                startup.mark_ready(&upstream_resolution.uri);
                startup.notify_ready_surface().await;
            }
            Err(error) => {
                if let Err(runtime_state_error) =
                    crate::runtime_state::record_bridge_connection_failure(
                        &root,
                        &upstream_resolution.uri,
                        "warmup",
                        "failed to complete bridge warmup",
                        Some(1),
                        None,
                        &error.to_string(),
                    )
                {
                    warn!(
                        error = %runtime_state_error,
                        root = %root.display(),
                        upstream_uri = %upstream_resolution.uri,
                        "failed to update prism runtime state for bridge warmup failure"
                    );
                }
                let error = anyhow!(
                    "failed to connect bridge to upstream {}: {error}",
                    upstream_resolution.uri
                );
                warn!(error = %error, root = %root.display(), "failed to complete bridge warmup");
                startup.mark_failed(&error.to_string());
                startup.notify_ready_surface().await;
            }
        }
    }
}

impl ServerHandler for ProxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = match self.server_info.lock() {
            Ok(info) => info.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let suffix = format!(
            "{}\n\n{}",
            self.startup.startup_instructions_suffix(),
            self.bridge_auth.bridge_instructions_suffix()
        );
        info.instructions = Some(match info.instructions.take() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{suffix}"),
            _ => suffix.to_string(),
        });
        info
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let _request = self.activity.begin_request();
        self.startup.capture_peer(&context.peer);
        let mut result = match self.active_peer().await {
            Ok(peer) => peer
                .list_resources(request)
                .await
                .map_err(map_proxy_error)?,
            Err(_) => ListResourcesResult {
                resources: Vec::new(),
                next_cursor: None,
                meta: None,
            },
        };
        result.resources.push(self.startup.startup_resource());
        result
            .resources
            .push(self.bridge_auth.bridge_auth_resource());
        Ok(result)
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let _request = self.activity.begin_request();
        self.startup.capture_peer(&context.peer);
        if let Ok(peer) = self.active_peer().await {
            peer.list_resource_templates(request)
                .await
                .map_err(map_proxy_error)
        } else {
            Ok(ListResourceTemplatesResult {
                resource_templates: Vec::new(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let _request = self.activity.begin_request();
        self.startup.capture_peer(&context.peer);
        let base_uri = split_resource_uri(request.uri.as_str()).0.to_string();
        if request.uri == STARTUP_URI {
            return Ok(ReadResourceResult::new(vec![self
                .startup
                .startup_resource_contents()]));
        }
        if request.uri == BRIDGE_AUTH_URI {
            return Ok(ReadResourceResult::new(vec![self
                .bridge_auth
                .bridge_auth_resource_contents()]));
        }
        let result = self
            .call_upstream(request, "resources/read", |peer, request| async move {
                peer.read_resource(request).await
            })
            .await?;
        if base_uri == SESSION_URI {
            self.inject_bridge_identity_into_session_result(result)
        } else {
            Ok(result)
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let _request = self.activity.begin_request();
        self.startup.capture_peer(&context.peer);
        if request.name.as_ref() == BRIDGE_ADOPT_TOOL_NAME {
            return self.bridge_auth.handle_adopt(request.arguments);
        }
        if self.active_peer().await.is_err() {
            let payload = self.startup.snapshot();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "{} Read {STARTUP_URI} and retry after {} ms.",
                payload.message, payload.poll_after_ms
            ))]));
        }
        let request = if request.name.as_ref() == "prism_code" {
            let mut request = request;
            request.arguments = self
                .bridge_auth
                .inject_mutation_bridge_execution(request.arguments)?;
            request
        } else {
            request
        };
        let result = self
            .call_upstream(request, "tools/call", |peer, request| async move {
                peer.call_tool(request).await
            })
            .await;
        result
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let _request = self.activity.begin_request();
        self.startup.capture_peer(&context.peer);
        if let Ok(peer) = self.active_peer().await {
            let mut result = peer.list_tools(request).await.map_err(map_proxy_error)?;
            for tool in &mut result.tools {
                if tool.name.as_ref() == "prism_code" {
                    *tool = self.bridge_auth.patch_mutation_tool(tool.clone());
                }
            }
            result.tools.push(self.bridge_auth.bridge_adopt_tool());
            self.update_tool_cache(&result.tools);
            Ok(result)
        } else {
            let mut tools = self
                .tool_cache
                .read()
                .ok()
                .map(|cache| cache.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            tools.sort_by(|left, right| left.name.cmp(&right.name));
            for tool in &mut tools {
                if tool.name.as_ref() == "prism_code" {
                    *tool = self.bridge_auth.patch_mutation_tool(tool.clone());
                }
            }
            tools.push(self.bridge_auth.bridge_adopt_tool());
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        if name == BRIDGE_ADOPT_TOOL_NAME {
            return Some(self.bridge_auth.bridge_adopt_tool());
        }
        self.tool_cache
            .read()
            .ok()
            .and_then(|cache| cache.get(name).cloned())
            .map(|tool| {
                if name == "prism_code" {
                    self.bridge_auth.patch_mutation_tool(tool)
                } else {
                    tool
                }
            })
    }
}

fn map_proxy_error(error: ServiceError) -> McpError {
    match error {
        ServiceError::McpError(error) => error,
        other => McpError::internal_error(other.to_string(), None),
    }
}

fn map_connect_error(error: anyhow::Error) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

fn bootstrap_server_info() -> ServerInfo {
    ServerInfo::new(
        ServerCapabilities::builder()
            .enable_resources()
            .enable_resources_list_changed()
            .enable_resources_subscribe()
            .enable_tools()
            .enable_tool_list_changed()
            .build(),
    )
    .with_server_info(Implementation::from_build_env())
    .with_protocol_version(ProtocolVersion::LATEST)
}

fn bootstrap_tool_cache(features: PrismMcpFeatures) -> HashMap<String, Tool> {
    PrismMcpServer::build_tool_router()
        .list_all()
        .into_iter()
        .filter(|tool| features.is_tool_enabled(&tool.name))
        .map(|tool| PrismMcpServer::transport_bind_tool_schema(tool, &features))
        .map(|tool| (tool.name.to_string(), tool))
        .collect()
}

fn bootstrap_build_log_path(root: &Path) -> Result<PathBuf> {
    let daemon_log_path = crate::daemon_mode::default_log_path(root)?;
    let parent = daemon_log_path
        .parent()
        .ok_or_else(|| anyhow!("daemon log path has no parent directory"))?;
    Ok(parent.join("bridge-bootstrap-build.log"))
}

async fn build_release_binaries(root: &Path, build_log_path: Option<&Path>) -> Result<()> {
    let root = root.to_path_buf();
    let build_log_path = build_log_path.map(PathBuf::from);
    let status = task::spawn_blocking(move || -> Result<std::process::ExitStatus> {
        let mut command = std::process::Command::new("cargo");
        command
            .arg("build")
            .arg("--release")
            .arg("-p")
            .arg("prism-cli")
            .arg("-p")
            .arg("prism-mcp")
            .current_dir(&root);

        if let Some(build_log_path) = build_log_path.as_deref() {
            if let Some(parent) = build_log_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let stdout = std::fs::File::create(build_log_path).with_context(|| {
                format!("failed to create build log {}", build_log_path.display())
            })?;
            let stderr = stdout.try_clone().with_context(|| {
                format!("failed to clone build log {}", build_log_path.display())
            })?;
            command
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
        }

        command
            .status()
            .context("failed to run cargo build for bridge bootstrap")
    })
    .await
    .context("bridge bootstrap build task join failed")??;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "bridge bootstrap build failed with status {status}"
        ))
    }
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordination_only_bootstrap_tool_cache_matches_reduced_surface() {
        let cache = bootstrap_tool_cache(
            PrismMcpFeatures::full().with_runtime_mode(PrismRuntimeMode::CoordinationOnly),
        );
        let mut tool_names = cache.keys().cloned().collect::<Vec<_>>();
        tool_names.sort();
        assert_eq!(
            tool_names,
            vec!["prism_mutate", "prism_query", "prism_task_brief"]
        );
    }
}
