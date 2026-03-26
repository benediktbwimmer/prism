use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use anyhow::{anyhow, Context, Result};
use rmcp::{
    model::*,
    service::{RequestContext, RoleClient, RunningService, ServiceError},
    transport::{stdio, StreamableHttpClientTransport},
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};

pub(crate) struct ProxyMcpServer {
    upstream: Mutex<Option<RunningService<RoleClient, ()>>>,
    peer: rmcp::service::Peer<RoleClient>,
    server_info: ServerInfo,
    tool_cache: RwLock<HashMap<String, Tool>>,
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
        })
    }

    pub(crate) async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
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
}

impl ServerHandler for ProxyMcpServer {
    fn get_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
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
        self.peer.call_tool(request).await.map_err(map_proxy_error)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let result = self
            .peer
            .list_tools(request)
            .await
            .map_err(map_proxy_error)?;
        self.update_tool_cache(&result.tools);
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
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

impl Drop for ProxyMcpServer {
    fn drop(&mut self) {
        if let Ok(mut upstream) = self.upstream.lock() {
            upstream.take();
        }
    }
}
