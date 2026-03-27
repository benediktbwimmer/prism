use std::collections::HashMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use rmcp::{
    model::*,
    service::{RequestContext, RoleClient, RunningService, ServiceError},
    transport::{stdio, IntoTransport, StreamableHttpClientTransport},
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use tracing::{info, warn};

const DEFAULT_BRIDGE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_BRIDGE_IDLE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const BRIDGE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
struct ProxyActivityTracker {
    last_activity: Mutex<Instant>,
    in_flight: AtomicUsize,
}

impl ProxyActivityTracker {
    fn new() -> Self {
        Self {
            last_activity: Mutex::new(Instant::now()),
            in_flight: AtomicUsize::new(0),
        }
    }

    fn touch(&self) {
        if let Ok(mut last_activity) = self.last_activity.lock() {
            *last_activity = Instant::now();
        }
    }

    fn begin_request(self: &Arc<Self>) -> ProxyRequestGuard {
        self.touch();
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        ProxyRequestGuard {
            tracker: Arc::clone(self),
        }
    }

    fn idle_elapsed(&self) -> Option<Duration> {
        if self.in_flight.load(Ordering::Relaxed) > 0 {
            return None;
        }
        self.last_activity.lock().ok().map(|last| last.elapsed())
    }
}

struct ProxyRequestGuard {
    tracker: Arc<ProxyActivityTracker>,
}

impl Drop for ProxyRequestGuard {
    fn drop(&mut self) {
        self.tracker.touch();
        self.tracker.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) struct ProxyMcpServer {
    upstream: Mutex<Option<RunningService<RoleClient, ()>>>,
    peer: rmcp::service::Peer<RoleClient>,
    server_info: ServerInfo,
    tool_cache: RwLock<HashMap<String, Tool>>,
    activity: Arc<ProxyActivityTracker>,
}

impl ProxyMcpServer {
    pub(crate) async fn connect(upstream_uri: String) -> Result<Self> {
        let client = ()
            .serve(StreamableHttpClientTransport::from_uri(
                upstream_uri.clone(),
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

        Ok(Self {
            upstream: Mutex::new(Some(client)),
            peer,
            server_info,
            tool_cache: RwLock::new(
                tools
                    .into_iter()
                    .map(|tool| (tool.name.to_string(), tool))
                    .collect(),
            ),
            activity: Arc::new(ProxyActivityTracker::new()),
        })
    }

    pub(crate) async fn serve_stdio(self) -> Result<()> {
        self.serve_transport_with_idle_timeout(
            stdio(),
            DEFAULT_BRIDGE_IDLE_TIMEOUT,
            DEFAULT_BRIDGE_IDLE_POLL_INTERVAL,
        )
        .await
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

    pub(crate) async fn serve_transport_with_idle_timeout<T, E, A>(
        self,
        transport: T,
        idle_timeout: Duration,
        poll_interval: Duration,
    ) -> Result<()>
    where
        T: IntoTransport<RoleServer, E, A>,
        E: Error + Send + Sync + 'static,
    {
        let mut service = self.serve(transport).await?;
        let activity = Arc::clone(&service.service().activity);

        loop {
            tokio::time::sleep(poll_interval).await;
            if service.is_closed() || service.peer().is_transport_closed() {
                exit_if_close_stalls(&mut service, "bridge transport closed").await?;
                return Ok(());
            }

            let Some(idle_for) = activity.idle_elapsed() else {
                continue;
            };
            if idle_for < idle_timeout {
                continue;
            }

            info!(
                idle_ms = idle_for.as_millis(),
                idle_timeout_ms = idle_timeout.as_millis(),
                "prism-mcp bridge idle timeout expired"
            );
            handle_idle_timeout(&mut service).await?;
            return Ok(());
        }
    }
}

impl ServerHandler for ProxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.activity.touch();
        self.server_info.clone()
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let _request = self.activity.begin_request();
        self.peer
            .list_resources(request)
            .await
            .map_err(map_proxy_error)
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let _request = self.activity.begin_request();
        self.peer
            .list_resource_templates(request)
            .await
            .map_err(map_proxy_error)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let _request = self.activity.begin_request();
        self.peer
            .read_resource(request)
            .await
            .map_err(map_proxy_error)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let _request = self.activity.begin_request();
        self.peer.call_tool(request).await.map_err(map_proxy_error)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let _request = self.activity.begin_request();
        let result = self
            .peer
            .list_tools(request)
            .await
            .map_err(map_proxy_error)?;
        self.update_tool_cache(&result.tools);
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.activity.touch();
        self.tool_cache
            .read()
            .ok()
            .and_then(|cache| cache.get(name).cloned())
    }
}

fn map_proxy_error(error: ServiceError) -> McpError {
    match error {
        ServiceError::McpError(error) => error,
        other => McpError::internal_error(other.to_string(), None),
    }
}

async fn exit_if_close_stalls(
    service: &mut RunningService<RoleServer, ProxyMcpServer>,
    reason: &str,
) -> Result<()> {
    let closed = service.close_with_timeout(BRIDGE_SHUTDOWN_TIMEOUT).await?;
    if closed.is_none() {
        warn!(
            reason = reason,
            shutdown_timeout_ms = BRIDGE_SHUTDOWN_TIMEOUT.as_millis(),
            "prism-mcp bridge did not close within the shutdown timeout; forcing process exit"
        );
        std::process::exit(0);
    }
    Ok(())
}

#[cfg(test)]
async fn handle_idle_timeout(
    service: &mut RunningService<RoleServer, ProxyMcpServer>,
) -> Result<()> {
    exit_if_close_stalls(service, "bridge idle timeout expired").await
}

#[cfg(not(test))]
async fn handle_idle_timeout(
    _service: &mut RunningService<RoleServer, ProxyMcpServer>,
) -> Result<()> {
    std::process::exit(0);
}

impl Drop for ProxyMcpServer {
    fn drop(&mut self) {
        if let Ok(mut upstream) = self.upstream.lock() {
            upstream.take();
        }
    }
}
