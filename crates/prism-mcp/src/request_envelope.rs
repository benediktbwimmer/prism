use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use prism_js::QueryPhaseView;
use rmcp::{
    model::{ClientRequest, RequestId, ServerInfo},
    service::{NotificationContext, RequestContext, RoleServer, Service, ServiceRole},
    ErrorData as McpError,
};
use serde_json::{json, Value};

use crate::mcp_call_log::{
    duration_to_ms, new_log_entry, payload_summary, preview_value, summarize_value,
    touches_for_value, unique_operations, unique_touches, PersistedMcpCallRecord,
};
use crate::{current_timestamp, PrismMcpServer};

tokio::task_local! {
    static CURRENT_MCP_REQUEST: RequestEnvelope;
}

tokio::task_local! {
    static CURRENT_HTTP_REQUEST: HttpRequestTiming;
}

static DELEGATED_REQUESTS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
const DELEGATED_REQUEST_TTL_SECS: u64 = 30;

#[derive(Clone)]
pub(crate) struct InstrumentedServerService {
    inner: PrismMcpServer,
}

#[derive(Clone)]
struct RequestEnvelope {
    state: Arc<RequestEnvelopeState>,
}

#[derive(Clone)]
struct HttpRequestTiming {
    state: Arc<HttpRequestTimingState>,
}

struct RequestEnvelopeState {
    name: String,
    request_key: Option<String>,
    delegated_surface: bool,
    summary: String,
    request_preview: Value,
    metadata: Value,
    started_at: u64,
    started: Instant,
    route_started_at: u64,
    route_duration_ms: u64,
    mcp_call_log_store: Arc<crate::mcp_call_log::McpCallLogStore>,
    dashboard: Arc<crate::DashboardState>,
    workspace: Option<Arc<prism_core::WorkspaceSession>>,
    session_id: String,
    task_id: Option<String>,
    awaiting_specialized_log: AtomicBool,
    logged: AtomicBool,
}

struct HttpRequestTimingState {
    started_at: u64,
    started: Instant,
    method: String,
    path: String,
}

#[derive(Clone)]
pub(crate) struct RequestEnvelopeSnapshot {
    state: Arc<RequestEnvelopeState>,
}

#[derive(Clone)]
struct HttpRequestTimingSnapshot {
    state: Arc<HttpRequestTimingState>,
}

impl InstrumentedServerService {
    pub(crate) fn new(inner: PrismMcpServer) -> Self {
        Self { inner }
    }
}

pub(crate) async fn with_http_request_timing<F, T>(method: String, path: String, future: F) -> T
where
    F: Future<Output = T>,
{
    let timing = HttpRequestTiming {
        state: Arc::new(HttpRequestTimingState {
            started_at: current_timestamp(),
            started: Instant::now(),
            method,
            path,
        }),
    };
    CURRENT_HTTP_REQUEST.scope(timing, future).await
}

pub(crate) async fn instrument_mcp_http_request(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    with_http_request_timing(method, path, next.run(request)).await
}

impl Service<RoleServer> for InstrumentedServerService {
    async fn handle_request(
        &self,
        request: <RoleServer as ServiceRole>::PeerReq,
        context: RequestContext<RoleServer>,
    ) -> Result<<RoleServer as ServiceRole>::Resp, McpError> {
        let inner = &self.inner;
        let envelope = RequestEnvelope::new(inner, &request, &context);
        CURRENT_MCP_REQUEST
            .scope(envelope.clone(), async move {
                let result = Service::handle_request(inner, request, context).await;
                envelope.finish_if_unlogged(result.as_ref());
                result
            })
            .await
    }

    async fn handle_notification(
        &self,
        notification: <RoleServer as ServiceRole>::PeerNot,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), McpError> {
        Service::handle_notification(&self.inner, notification, context).await
    }

    fn get_info(&self) -> ServerInfo {
        Service::get_info(&self.inner)
    }
}

pub(crate) fn apply_current_request_envelope(
    phases: &mut Vec<QueryPhaseView>,
    started_at: &mut u64,
    duration_ms: &mut u64,
    metadata: &mut Value,
) {
    let Some(request) = current_request_envelope() else {
        return;
    };
    let mut merged = request.outer_phases();
    merged.extend(phases.iter().cloned());
    *phases = merged;
    *started_at = request.started_at();
    *duration_ms = request.duration_ms();
    merge_request_metadata(metadata, &request.metadata());
    request.mark_logged();
}

pub(crate) fn current_request_envelope() -> Option<RequestEnvelopeSnapshot> {
    CURRENT_MCP_REQUEST
        .try_with(|request| RequestEnvelopeSnapshot {
            state: Arc::clone(&request.state),
        })
        .ok()
}

impl RequestEnvelope {
    fn new(
        server: &PrismMcpServer,
        request: &ClientRequest,
        context: &RequestContext<RoleServer>,
    ) -> Self {
        let http_request = current_http_request_timing();
        let fallback_started_at = current_timestamp();
        let fallback_started = Instant::now();
        let request_id = request_id_value(&context.id);
        let request_id_for_meta = request_id.clone();
        let (name, summary, request_preview, mut metadata) = classify_request(request, request_id);
        let request_key = delegated_request_key(&request_preview);
        let delegated_surface = request_uses_specialized_logging(&name);
        metadata["requestId"] = request_id_for_meta;
        let (started_at, started, route_started_at, route_duration_ms) =
            if let Some(http_request) = http_request.as_ref() {
                metadata["transport"] = json!({
                    "kind": "streamable-http",
                    "method": http_request.method(),
                    "path": http_request.path(),
                });
                (
                    http_request.started_at(),
                    http_request.started(),
                    http_request.started_at(),
                    http_request.duration_ms(),
                )
            } else {
                (
                    fallback_started_at,
                    fallback_started,
                    current_timestamp(),
                    0,
                )
            };
        let (session_id, task_id) = server.session_log_context();
        Self {
            state: Arc::new(RequestEnvelopeState {
                name,
                request_key,
                delegated_surface,
                summary,
                request_preview,
                metadata,
                started_at,
                started,
                route_started_at,
                route_duration_ms,
                mcp_call_log_store: server.mcp_call_log_store(),
                dashboard: server.dashboard_state(),
                workspace: server.workspace_session().map(Arc::clone),
                session_id,
                task_id,
                awaiting_specialized_log: AtomicBool::new(false),
                logged: AtomicBool::new(false),
            }),
        }
    }

    fn finish_if_unlogged<R>(&self, result: Result<&R, &McpError>)
    where
        R: serde::Serialize,
    {
        if delegated_request_is_settled(&self.state.request_key) {
            self.state.logged.store(true, Ordering::SeqCst);
            return;
        }
        if self.state.delegated_surface {
            self.state
                .awaiting_specialized_log
                .store(true, Ordering::SeqCst);
            self.state.logged.store(true, Ordering::SeqCst);
            return;
        }
        if self.state.logged.swap(true, Ordering::SeqCst) {
            return;
        }

        let duration_ms = duration_to_ms(self.state.started.elapsed());
        let success = result.is_ok();
        let error = result.as_ref().err().map(|error| error.to_string());
        let response_value = result
            .ok()
            .and_then(|value| serde_json::to_value(value).ok());
        let mut phases = self.outer_phases();
        let mut metadata = self.state.metadata.clone();
        phases.push(phase(
            "mcp.executeHandler",
            &metadata,
            duration_ms,
            success,
            error.clone(),
            self.state.started_at,
        ));
        phases.push(phase(
            "mcp.encodeResponse",
            &metadata,
            0,
            success,
            error.clone(),
            current_timestamp(),
        ));
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.state.dashboard.as_ref(),
            self.state.workspace.as_deref(),
        );
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                self.state.mcp_call_log_store.runtime(),
                "request",
                &self.state.name,
                None,
                self.state.summary.clone(),
                self.state.started_at,
                duration_ms,
                Some(self.state.session_id.clone()),
                self.state.task_id.clone(),
                success,
                error.clone(),
                unique_operations(&phases),
                unique_touches(&phases),
                Vec::new(),
                payload_summary(Some(&self.state.request_preview)),
                payload_summary(response_value.as_ref()),
            ),
            phases,
            request_preview: preview_value(&self.state.request_preview),
            response_preview: response_value.as_ref().and_then(preview_value),
            metadata,
            query_compat: None,
        };
        let _ = self.state.mcp_call_log_store.push(record);
    }

    fn outer_phases(&self) -> Vec<QueryPhaseView> {
        vec![
            phase(
                "mcp.receiveRequest",
                &self.state.request_preview,
                0,
                true,
                None,
                self.state.started_at,
            ),
            phase_at(
                self.state.route_started_at,
                "mcp.routeRequest",
                &self.state.metadata,
                self.state.route_duration_ms,
                true,
                None,
            ),
        ]
    }
}

impl RequestEnvelopeSnapshot {
    fn started_at(&self) -> u64 {
        self.state.started_at
    }

    fn duration_ms(&self) -> u64 {
        duration_to_ms(self.state.started.elapsed())
    }

    fn metadata(&self) -> Value {
        self.state.metadata.clone()
    }

    fn outer_phases(&self) -> Vec<QueryPhaseView> {
        RequestEnvelope {
            state: Arc::clone(&self.state),
        }
        .outer_phases()
    }

    fn mark_logged(&self) {
        register_delegated_request(&self.state.request_key);
        self.state.logged.store(true, Ordering::SeqCst);
    }
}

impl Drop for RequestEnvelope {
    fn drop(&mut self) {
        if delegated_request_is_settled(&self.state.request_key) {
            self.state.logged.store(true, Ordering::SeqCst);
            return;
        }
        if self.state.awaiting_specialized_log.load(Ordering::SeqCst) {
            self.state.logged.store(true, Ordering::SeqCst);
            return;
        }
        if self.state.logged.swap(true, Ordering::SeqCst) {
            return;
        }

        let duration_ms = duration_to_ms(self.state.started.elapsed());
        let error = "request dropped before completion".to_string();
        let mut phases = self.outer_phases();
        let mut metadata = self.state.metadata.clone();
        metadata["lifecycle"] = json!({
            "state": "dropped",
            "finalized": false,
        });
        phases.push(phase(
            "mcp.executeHandler",
            &metadata,
            duration_ms,
            false,
            Some(error.clone()),
            self.state.started_at,
        ));
        crate::slow_call_snapshot::attach_slow_call_snapshot(
            &mut metadata,
            duration_ms,
            self.state.dashboard.as_ref(),
            self.state.workspace.as_deref(),
        );
        let record = PersistedMcpCallRecord {
            entry: new_log_entry(
                self.state.mcp_call_log_store.runtime(),
                "request",
                &self.state.name,
                None,
                self.state.summary.clone(),
                self.state.started_at,
                duration_ms,
                Some(self.state.session_id.clone()),
                self.state.task_id.clone(),
                false,
                Some(error.clone()),
                unique_operations(&phases),
                unique_touches(&phases),
                Vec::new(),
                payload_summary(Some(&self.state.request_preview)),
                payload_summary(None),
            ),
            phases,
            request_preview: preview_value(&self.state.request_preview),
            response_preview: None,
            metadata,
            query_compat: None,
        };
        let _ = self.state.mcp_call_log_store.push(record);
    }
}

fn merge_request_metadata(metadata: &mut Value, request_metadata: &Value) {
    let Value::Object(request_object) = request_metadata else {
        return;
    };
    if !metadata.is_object() {
        *metadata = json!({});
    }
    if let Value::Object(metadata_object) = metadata {
        metadata_object.insert(
            "mcpRequest".to_string(),
            Value::Object(request_object.clone()),
        );
    }
}

fn classify_request(request: &ClientRequest, request_id: Value) -> (String, String, Value, Value) {
    match request {
        ClientRequest::InitializeRequest(request) => (
            "initialize".to_string(),
            "initialize".to_string(),
            json!({
                "method": "initialize",
                "requestId": request_id,
                "protocolVersion": request.params.protocol_version,
            }),
            json!({
                "method": "initialize",
            }),
        ),
        ClientRequest::PingRequest(_) => (
            "ping".to_string(),
            "ping".to_string(),
            json!({
                "method": "ping",
                "requestId": request_id,
            }),
            json!({
                "method": "ping",
            }),
        ),
        ClientRequest::CompleteRequest(request) => (
            "completion/complete".to_string(),
            "complete prompt value".to_string(),
            json!({
                "method": "completion/complete",
                "requestId": request_id,
                "argName": request.params.argument.name,
                "ref": request.params.r#ref,
            }),
            json!({
                "method": "completion/complete",
            }),
        ),
        ClientRequest::SetLevelRequest(request) => (
            "logging/setLevel".to_string(),
            "set log level".to_string(),
            json!({
                "method": "logging/setLevel",
                "requestId": request_id,
                "level": request.params.level,
            }),
            json!({
                "method": "logging/setLevel",
            }),
        ),
        ClientRequest::GetPromptRequest(request) => (
            "prompts/get".to_string(),
            format!("get prompt {}", request.params.name),
            json!({
                "method": "prompts/get",
                "requestId": request_id,
                "name": request.params.name,
            }),
            json!({
                "method": "prompts/get",
                "name": request.params.name,
            }),
        ),
        ClientRequest::ListPromptsRequest(_) => (
            "prompts/list".to_string(),
            "list prompts".to_string(),
            json!({
                "method": "prompts/list",
                "requestId": request_id,
            }),
            json!({
                "method": "prompts/list",
            }),
        ),
        ClientRequest::ListResourcesRequest(_) => (
            "resources/list".to_string(),
            "list resources".to_string(),
            json!({
                "method": "resources/list",
                "requestId": request_id,
            }),
            json!({
                "method": "resources/list",
            }),
        ),
        ClientRequest::ListResourceTemplatesRequest(_) => (
            "resources/templates/list".to_string(),
            "list resource templates".to_string(),
            json!({
                "method": "resources/templates/list",
                "requestId": request_id,
            }),
            json!({
                "method": "resources/templates/list",
            }),
        ),
        ClientRequest::ReadResourceRequest(request) => (
            "resources/read".to_string(),
            format!("read resource {}", request.params.uri),
            json!({
                "method": "resources/read",
                "requestId": request_id,
                "uri": request.params.uri,
            }),
            json!({
                "method": "resources/read",
                "uri": request.params.uri,
            }),
        ),
        ClientRequest::SubscribeRequest(request) => (
            "resources/subscribe".to_string(),
            format!("subscribe {}", request.params.uri),
            json!({
                "method": "resources/subscribe",
                "requestId": request_id,
                "uri": request.params.uri,
            }),
            json!({
                "method": "resources/subscribe",
                "uri": request.params.uri,
            }),
        ),
        ClientRequest::UnsubscribeRequest(request) => (
            "resources/unsubscribe".to_string(),
            format!("unsubscribe {}", request.params.uri),
            json!({
                "method": "resources/unsubscribe",
                "requestId": request_id,
                "uri": request.params.uri,
            }),
            json!({
                "method": "resources/unsubscribe",
                "uri": request.params.uri,
            }),
        ),
        ClientRequest::CallToolRequest(request) => (
            "tools/call".to_string(),
            format!("call {}", request.params.name),
            json!({
                "method": "tools/call",
                "requestId": request_id,
                "name": request.params.name,
                "taskInvocation": request.params.task.is_some(),
            }),
            json!({
                "method": "tools/call",
                "name": request.params.name,
                "taskInvocation": request.params.task.is_some(),
            }),
        ),
        ClientRequest::ListToolsRequest(_) => (
            "tools/list".to_string(),
            "list tools".to_string(),
            json!({
                "method": "tools/list",
                "requestId": request_id,
            }),
            json!({
                "method": "tools/list",
            }),
        ),
        ClientRequest::CustomRequest(request) => (
            request.method.clone(),
            format!("custom request {}", request.method),
            json!({
                "method": request.method,
                "requestId": request_id,
                "params": request.params,
            }),
            json!({
                "method": request.method,
            }),
        ),
        ClientRequest::ListTasksRequest(_) => (
            "tasks/list".to_string(),
            "list tasks".to_string(),
            json!({
                "method": "tasks/list",
                "requestId": request_id,
            }),
            json!({
                "method": "tasks/list",
            }),
        ),
        ClientRequest::GetTaskInfoRequest(request) => (
            "tasks/get".to_string(),
            format!("get task {}", request.params.task_id),
            json!({
                "method": "tasks/get",
                "requestId": request_id,
                "taskId": request.params.task_id,
            }),
            json!({
                "method": "tasks/get",
                "taskId": request.params.task_id,
            }),
        ),
        ClientRequest::GetTaskResultRequest(request) => (
            "tasks/result".to_string(),
            format!("get task result {}", request.params.task_id),
            json!({
                "method": "tasks/result",
                "requestId": request_id,
                "taskId": request.params.task_id,
            }),
            json!({
                "method": "tasks/result",
                "taskId": request.params.task_id,
            }),
        ),
        ClientRequest::CancelTaskRequest(request) => (
            "tasks/cancel".to_string(),
            format!("cancel task {}", request.params.task_id),
            json!({
                "method": "tasks/cancel",
                "requestId": request_id,
                "taskId": request.params.task_id,
            }),
            json!({
                "method": "tasks/cancel",
                "taskId": request.params.task_id,
            }),
        ),
    }
}

fn current_http_request_timing() -> Option<HttpRequestTimingSnapshot> {
    CURRENT_HTTP_REQUEST
        .try_with(|request| HttpRequestTimingSnapshot {
            state: Arc::clone(&request.state),
        })
        .ok()
}

fn request_id_value(id: &RequestId) -> Value {
    serde_json::to_value(id).unwrap_or(Value::Null)
}

fn delegated_request_key(request_preview: &Value) -> Option<String> {
    let method = request_preview.get("method")?.as_str()?;
    let request_id = request_preview.get("requestId")?;
    let mut key = format!("{method}|{}", request_id);
    if let Some(name) = request_preview.get("name").and_then(Value::as_str) {
        key.push('|');
        key.push_str(name);
    }
    Some(key)
}

fn request_uses_specialized_logging(name: &str) -> bool {
    matches!(
        name,
        "tools/call"
            | "tools/list"
            | "resources/read"
            | "resources/list"
            | "resources/templates/list"
    )
}

fn delegated_requests() -> &'static Mutex<HashMap<String, Instant>> {
    DELEGATED_REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_delegated_requests(now: Instant, requests: &mut HashMap<String, Instant>) {
    requests.retain(|_, recorded_at| {
        now.duration_since(*recorded_at).as_secs() <= DELEGATED_REQUEST_TTL_SECS
    });
}

fn register_delegated_request(request_key: &Option<String>) {
    let Some(request_key) = request_key else {
        return;
    };
    let now = Instant::now();
    let mut requests = delegated_requests()
        .lock()
        .expect("delegated request registry poisoned");
    prune_delegated_requests(now, &mut requests);
    requests.insert(request_key.clone(), now);
}

fn delegated_request_is_settled(request_key: &Option<String>) -> bool {
    let Some(request_key) = request_key else {
        return false;
    };
    let now = Instant::now();
    let mut requests = delegated_requests()
        .lock()
        .expect("delegated request registry poisoned");
    prune_delegated_requests(now, &mut requests);
    requests.contains_key(request_key)
}

fn phase(
    operation: &str,
    args: &Value,
    duration_ms: u64,
    success: bool,
    error: Option<String>,
    started_at: u64,
) -> QueryPhaseView {
    phase_at(started_at, operation, args, duration_ms, success, error)
}

fn phase_at(
    started_at: u64,
    operation: &str,
    args: &Value,
    duration_ms: u64,
    success: bool,
    error: Option<String>,
) -> QueryPhaseView {
    QueryPhaseView {
        operation: operation.to_string(),
        started_at,
        duration_ms,
        args_summary: Some(summarize_value(args)),
        touched: touches_for_value(args),
        success,
        error,
    }
}

impl HttpRequestTimingSnapshot {
    fn started_at(&self) -> u64 {
        self.state.started_at
    }

    fn started(&self) -> Instant {
        self.state.started
    }

    fn duration_ms(&self) -> u64 {
        duration_to_ms(self.state.started.elapsed())
    }

    fn method(&self) -> &str {
        self.state.method.as_str()
    }

    fn path(&self) -> &str {
        self.state.path.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_request_envelope_persists_aborted_request_record() {
        let store = Arc::new(crate::mcp_call_log::McpCallLogStore::for_root(None));
        let dashboard = Arc::new(crate::DashboardState::default());

        let envelope = RequestEnvelope {
            state: Arc::new(RequestEnvelopeState {
                name: "ping".to_string(),
                request_key: Some("ping|1".to_string()),
                delegated_surface: false,
                summary: "ping".to_string(),
                request_preview: json!({
                    "method": "ping",
                    "requestId": 1,
                }),
                metadata: json!({
                    "method": "ping",
                    "requestId": 1,
                }),
                started_at: current_timestamp(),
                started: Instant::now(),
                route_started_at: current_timestamp(),
                route_duration_ms: 0,
                mcp_call_log_store: Arc::clone(&store),
                dashboard,
                workspace: None,
                session_id: "session:test".to_string(),
                task_id: Some("task:test".to_string()),
                awaiting_specialized_log: AtomicBool::new(false),
                logged: AtomicBool::new(false),
            }),
        };

        drop(envelope);

        let records = store.records();
        let record = records
            .iter()
            .find(|record| record.entry.name == "ping")
            .expect("dropped request record should exist");
        assert!(!record.entry.success);
        assert_eq!(
            record.entry.error.as_deref(),
            Some("request dropped before completion")
        );
        let operations = record
            .phases
            .iter()
            .map(|phase| phase.operation.as_str())
            .collect::<Vec<_>>();
        assert!(operations.contains(&"mcp.receiveRequest"));
        assert!(operations.contains(&"mcp.routeRequest"));
        assert!(operations.contains(&"mcp.executeHandler"));
        assert!(!operations.contains(&"mcp.encodeResponse"));
    }

    #[test]
    fn dropped_request_envelope_skips_logging_for_delegated_request() {
        let store = Arc::new(crate::mcp_call_log::McpCallLogStore::for_root(None));
        let dashboard = Arc::new(crate::DashboardState::default());
        let request_key = Some("tools/call|61|prism_query".to_string());
        register_delegated_request(&request_key);

        let envelope = RequestEnvelope {
            state: Arc::new(RequestEnvelopeState {
                name: "tools/call".to_string(),
                request_key,
                delegated_surface: true,
                summary: "call prism_query".to_string(),
                request_preview: json!({
                    "method": "tools/call",
                    "name": "prism_query",
                    "requestId": 61,
                }),
                metadata: json!({
                    "method": "tools/call",
                    "name": "prism_query",
                    "requestId": 61,
                }),
                started_at: current_timestamp(),
                started: Instant::now(),
                route_started_at: current_timestamp(),
                route_duration_ms: 0,
                mcp_call_log_store: Arc::clone(&store),
                dashboard,
                workspace: None,
                session_id: "session:test".to_string(),
                task_id: Some("task:test".to_string()),
                awaiting_specialized_log: AtomicBool::new(false),
                logged: AtomicBool::new(false),
            }),
        };

        drop(envelope);

        assert!(store.records().is_empty());
    }

    #[test]
    fn successful_delegated_request_envelope_skips_generic_wrapper_log() {
        let store = Arc::new(crate::mcp_call_log::McpCallLogStore::for_root(None));
        let dashboard = Arc::new(crate::DashboardState::default());

        let envelope = RequestEnvelope {
            state: Arc::new(RequestEnvelopeState {
                name: "tools/call".to_string(),
                request_key: Some("tools/call|61|prism_query".to_string()),
                delegated_surface: true,
                summary: "call prism_query".to_string(),
                request_preview: json!({
                    "method": "tools/call",
                    "name": "prism_query",
                    "requestId": 61,
                }),
                metadata: json!({
                    "method": "tools/call",
                    "name": "prism_query",
                    "requestId": 61,
                }),
                started_at: current_timestamp(),
                started: Instant::now(),
                route_started_at: current_timestamp(),
                route_duration_ms: 0,
                mcp_call_log_store: Arc::clone(&store),
                dashboard,
                workspace: None,
                session_id: "session:test".to_string(),
                task_id: Some("task:test".to_string()),
                awaiting_specialized_log: AtomicBool::new(false),
                logged: AtomicBool::new(false),
            }),
        };

        envelope.finish_if_unlogged(Ok(&json!({ "ok": true })));
        drop(envelope);

        assert!(store.records().is_empty());
    }

    #[tokio::test]
    async fn http_request_timing_scope_exposes_transport_start() {
        let started_at = with_http_request_timing("POST".to_string(), "/mcp".to_string(), async {
            let timing = current_http_request_timing()
                .expect("http request timing should be present inside scope");
            assert_eq!(timing.method(), "POST");
            assert_eq!(timing.path(), "/mcp");
            timing.started_at()
        })
        .await;
        assert!(started_at > 0);
        assert!(current_http_request_timing().is_none());
    }
}
