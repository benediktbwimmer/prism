use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use rmcp::{
    model::*,
    service::{RequestContext, RoleClient, RunningService, ServiceError},
    transport::{stdio, StreamableHttpClientTransport},
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

use crate::bridge_auth::{BridgeAuthContext, BRIDGE_ADOPT_TOOL_NAME, BRIDGE_AUTH_URI};
use crate::daemon_mode::BridgeUpstreamSource;

const DEFAULT_BRIDGE_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(100);
const DEFAULT_BRIDGE_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_BRIDGE_RECONNECT_TIMEOUT: Duration = Duration::from_secs(180);

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
    upstream: AsyncMutex<UpstreamConnection>,
    upstream_source: BridgeUpstreamSource,
    reconnect_lock: AsyncMutex<()>,
    server_info: Mutex<ServerInfo>,
    tool_cache: RwLock<HashMap<String, Tool>>,
    activity: Arc<ProxyActivityTracker>,
    bridge_auth: BridgeAuthContext,
}

impl ProxyMcpServer {
    #[cfg(test)]
    pub(crate) async fn connect_with_source(
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        Self::connect_with_bridge_auth(
            upstream_uri,
            upstream_source,
            BridgeAuthContext::disabled(),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn connect_with_credentials_path(
        credentials_path: std::path::PathBuf,
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        Self::connect_with_bridge_auth(
            upstream_uri,
            upstream_source,
            BridgeAuthContext::from_credentials_path(credentials_path),
        )
        .await
    }

    pub(crate) async fn connect_with_source_for_root(
        root: &Path,
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
    ) -> Result<Self> {
        Self::connect_with_bridge_auth(
            upstream_uri,
            upstream_source,
            BridgeAuthContext::for_root(root)?,
        )
        .await
    }

    async fn connect_with_bridge_auth(
        upstream_uri: String,
        upstream_source: BridgeUpstreamSource,
        bridge_auth: BridgeAuthContext,
    ) -> Result<Self> {
        let (connection, server_info, tools) = Self::open_upstream(&upstream_uri).await?;
        Ok(Self {
            upstream: AsyncMutex::new(connection),
            upstream_source,
            reconnect_lock: AsyncMutex::new(()),
            server_info: Mutex::new(server_info),
            tool_cache: RwLock::new(
                tools
                    .into_iter()
                    .map(|tool| (tool.name.to_string(), tool))
                    .collect(),
            ),
            activity: Arc::new(ProxyActivityTracker::new()),
            bridge_auth,
        })
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

    async fn active_peer(&self) -> Result<rmcp::service::Peer<RoleClient>> {
        let peer = {
            let upstream = self.upstream.lock().await;
            upstream.peer.clone()
        };
        if !peer.is_transport_closed() {
            return Ok(peer);
        }
        self.reconnect_with_backoff("upstream transport closed before request", false)
            .await?;
        let upstream = self.upstream.lock().await;
        Ok(upstream.peer.clone())
    }

    async fn reconnect_with_backoff(&self, reason: &str, force: bool) -> Result<()> {
        let _reconnect = self.reconnect_lock.lock().await;
        if !force {
            let upstream = self.upstream.lock().await;
            if !upstream.peer.is_transport_closed() {
                return Ok(());
            }
        }

        let started = Instant::now();
        let mut attempt = 0usize;
        let mut delay = DEFAULT_BRIDGE_RECONNECT_BASE_DELAY;
        let mut last_error = None;
        while started.elapsed() < DEFAULT_BRIDGE_RECONNECT_TIMEOUT {
            attempt += 1;
            let upstream_uri = match self.upstream_source.read_uri() {
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
                    delay = delay
                        .saturating_mul(2)
                        .min(DEFAULT_BRIDGE_RECONNECT_MAX_DELAY);
                    continue;
                }
            };

            match Self::open_upstream(&upstream_uri).await {
                Ok((connection, server_info, tools)) => {
                    {
                        let mut upstream = self.upstream.lock().await;
                        *upstream = connection;
                    }
                    self.update_server_info(&server_info);
                    self.update_tool_cache(&tools);
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
                    warn!(
                        attempt,
                        reason,
                        upstream_uri = %upstream_uri,
                        delay_ms = delay.as_millis(),
                        "prism-mcp bridge failed to reconnect to upstream; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay
                        .saturating_mul(2)
                        .min(DEFAULT_BRIDGE_RECONNECT_MAX_DELAY);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("failed to reconnect PRISM MCP bridge upstream")))
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
        let peer = self.active_peer().await.map_err(map_connect_error)?;
        match op(peer, request.clone()).await {
            Ok(result) => Ok(result),
            Err(error) if Self::should_reconnect(&error) => {
                warn!(
                    operation = op_name,
                    error = %error,
                    "prism-mcp bridge request failed because the upstream transport was unavailable; reconnecting"
                );
                self.reconnect_with_backoff(op_name, true)
                    .await
                    .map_err(map_connect_error)?;
                let peer = self.active_peer().await.map_err(map_connect_error)?;
                op(peer, request).await.map_err(map_proxy_error)
            }
            Err(error) => Err(map_proxy_error(error)),
        }
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
}

impl ServerHandler for ProxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = match self.server_info.lock() {
            Ok(info) => info.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let suffix = self.bridge_auth.bridge_instructions_suffix();
        info.instructions = Some(match info.instructions.take() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{suffix}"),
            _ => suffix.to_string(),
        });
        info
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let _request = self.activity.begin_request();
        let mut result = self.call_upstream(request, "resources/list", |peer, request| async move {
            peer.list_resources(request).await
        })
        .await?;
        result.resources.push(self.bridge_auth.bridge_auth_resource());
        Ok(result)
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let _request = self.activity.begin_request();
        self.call_upstream(
            request,
            "resources/templates/list",
            |peer, request| async move { peer.list_resource_templates(request).await },
        )
        .await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let _request = self.activity.begin_request();
        if request.uri == BRIDGE_AUTH_URI {
            return Ok(ReadResourceResult::new(vec![
                self.bridge_auth.bridge_auth_resource_contents()
            ]));
        }
        self.call_upstream(request, "resources/read", |peer, request| async move {
            peer.read_resource(request).await
        })
        .await
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let _request = self.activity.begin_request();
        if request.name.as_ref() == BRIDGE_ADOPT_TOOL_NAME {
            return self.bridge_auth.handle_adopt(request.arguments);
        }
        let request = if request.name.as_ref() == "prism_mutate" {
            let mut request = request;
            request.arguments = self.bridge_auth.inject_mutation_credential(request.arguments)?;
            request
        } else {
            request
        };
        self.call_upstream(request, "tools/call", |peer, request| async move {
            peer.call_tool(request).await
        })
        .await
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let _request = self.activity.begin_request();
        let mut result = self
            .call_upstream(request, "tools/list", |peer, request| async move {
                peer.list_tools(request).await
            })
            .await?;
        for tool in &mut result.tools {
            if tool.name.as_ref() == "prism_mutate" {
                *tool = self.bridge_auth.patch_mutation_tool(tool.clone());
            }
        }
        result.tools.push(self.bridge_auth.bridge_adopt_tool());
        self.update_tool_cache(&result.tools);
        Ok(result)
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
                if name == "prism_mutate" {
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
