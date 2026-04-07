use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use axum::Router;
use rmcp::{
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    transport::{
        streamable_http_server::session::local::LocalSessionManager, StreamableHttpServerConfig,
        StreamableHttpService, Transport,
    },
};
use serde_json::{json, Value};

use super::*;
use prism_core::{
    default_workspace_session_options, default_workspace_shared_runtime,
    hydrate_workspace_session_with_options, index_workspace_session,
    index_workspace_session_with_options, BootstrapOwnerInput, PrismPaths, WorkspaceSessionOptions,
    WorktreeMode, WorktreeRegistrationRecord,
};
use prism_ir::new_sortable_token;
use prism_ir::{Language, Node, NodeId, NodeKind, Span};
use prism_query::{Prism, QueryLimits};
use prism_store::Graph;

#[derive(Debug, Clone)]
pub(crate) struct MutationCredentialFixture {
    pub(crate) credential_id: String,
    #[allow(dead_code)]
    pub(crate) principal_id: String,
    pub(crate) principal_token: String,
}

thread_local! {
    static TEMP_TEST_DIRS: RefCell<TempTestDirState> = RefCell::new(TempTestDirState {
        paths: Vec::new(),
    });
}

struct TempTestDirState {
    paths: Vec<PathBuf>,
}

impl Drop for TempTestDirState {
    fn drop(&mut self) {
        for path in self.paths.drain(..).rev() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn track_temp_dir(path: &Path) {
    TEMP_TEST_DIRS.with(|state| state.borrow_mut().paths.push(path.to_path_buf()));
}

pub(crate) fn ensure_process_test_prism_home() -> &'static PathBuf {
    static TEST_PRISM_HOME: OnceLock<PathBuf> = OnceLock::new();
    TEST_PRISM_HOME.get_or_init(|| {
        let path = env::temp_dir().join(format!("prism-mcp-test-home-{}", new_sortable_token()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("mcp test prism home should be created");
        // SAFETY: test initialization sets a process-wide PRISM_HOME once, before the
        // helper-driven temp workspaces are indexed. We never mutate it again.
        unsafe {
            env::set_var("PRISM_HOME", &path);
            env::set_var("PRISM_TEST_DISABLE_LIVE_WATCHERS", "1");
            env::set_var("PRISM_TEST_DISABLE_SHARED_COORDINATION_REF_PUBLISH", "1");
            env::set_var("PRISM_TEST_FAST_PROXY_RECONNECT", "1");
        }
        path
    })
}

pub(crate) fn credentials_test_lock() -> MutexGuard<'static, ()> {
    static CREDENTIALS_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    CREDENTIALS_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("credentials test lock poisoned")
}

pub(crate) fn temp_test_dir(prefix: &str) -> PathBuf {
    let _ = ensure_process_test_prism_home();
    let root = std::env::temp_dir().join(format!("{prefix}-{}", new_sortable_token()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    track_temp_dir(&root);
    root
}

pub(crate) fn enable_shared_coordination_ref_publish(root: &Path) {
    let marker = root
        .join(".prism")
        .join("tests")
        .join("enable_shared_coordination_ref_publish");
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent).expect("shared coordination ref publish marker dir");
    }
    fs::write(marker, "1\n").expect("shared coordination ref publish marker");
}

pub(crate) fn host_with_node(node: Node) -> QueryHost {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    QueryHost::new(Prism::new(graph))
}

pub(crate) fn host_with_prism(prism: Prism) -> QueryHost {
    QueryHost::new(prism)
}

pub(crate) fn host_with_session_internal(workspace: WorkspaceSession) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_runtime_diagnostics_auto_refresh(false),
    )
}

pub(crate) fn host_with_session(workspace: WorkspaceSession) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        PrismMcpFeatures::full().with_runtime_diagnostics_auto_refresh(false),
    )
}

pub(crate) fn shared_workspace_session(root: &Path) -> Arc<WorkspaceSession> {
    let _ = ensure_process_test_prism_home();
    Arc::new(index_workspace_session(root).expect("workspace session should index"))
}

pub(crate) fn host_with_shared_session_internal(workspace: Arc<WorkspaceSession>) -> QueryHost {
    QueryHost::with_shared_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_runtime_diagnostics_auto_refresh(false),
    )
}

pub(crate) fn host_with_shared_session_and_features(
    workspace: Arc<WorkspaceSession>,
    features: PrismMcpFeatures,
) -> QueryHost {
    QueryHost::with_shared_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        features.with_runtime_diagnostics_auto_refresh(false),
    )
}

pub(crate) fn workspace_session_with_owner_credential(
    root: &Path,
) -> (WorkspaceSession, MutationCredentialFixture) {
    let _ = ensure_process_test_prism_home();
    let mut options = default_workspace_session_options(root)
        .expect("default workspace session options should resolve");
    options.runtime_mode = prism_ir::PrismRuntimeMode::Full;
    options.hydrate_persisted_projections = false;
    options.hydrate_persisted_co_change = false;
    let session = index_workspace_session_with_options(root, options)
        .expect("workspace session should index");
    register_test_human_worktree(root);
    let issued = session
        .bootstrap_owner_principal(BootstrapOwnerInput {
            authority_id: None,
            name: "Test Owner".to_string(),
            role: Some("test_owner".to_string()),
        })
        .expect("owner bootstrap should succeed");
    (
        session,
        MutationCredentialFixture {
            credential_id: issued.credential.credential_id.0.to_string(),
            principal_id: issued.principal.principal_id.0.to_string(),
            principal_token: issued.principal_token,
        },
    )
}

pub(crate) fn hydrate_workspace_session_with_shared_runtime(root: &Path) -> WorkspaceSession {
    let _ = ensure_process_test_prism_home();
    hydrate_workspace_session_with_options(
        root,
        WorkspaceSessionOptions {
            runtime_mode: prism_ir::PrismRuntimeMode::Full,
            shared_runtime: default_workspace_shared_runtime(root)
                .expect("default shared runtime should resolve"),
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should hydrate")
}

pub(crate) fn index_workspace_session_with_shared_runtime(root: &Path) -> WorkspaceSession {
    let _ = ensure_process_test_prism_home();
    index_workspace_session_with_options(
        root,
        WorkspaceSessionOptions {
            runtime_mode: prism_ir::PrismRuntimeMode::Full,
            shared_runtime: default_workspace_shared_runtime(root)
                .expect("default shared runtime should resolve"),
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should index")
}

pub(crate) fn register_test_human_worktree(root: &Path) -> String {
    register_test_worktree(root, "operator", WorktreeMode::Human).agent_label
}

pub(crate) fn register_test_agent_worktree(root: &Path) -> WorktreeRegistrationRecord {
    register_test_worktree(root, "agent", WorktreeMode::Agent)
}

fn register_test_worktree(
    root: &Path,
    prefix: &str,
    mode: WorktreeMode,
) -> WorktreeRegistrationRecord {
    let label = format!(
        "{prefix}-{}-{}",
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("test-root"),
        new_sortable_token()
    );
    PrismPaths::for_workspace_root(root)
        .expect("paths should resolve")
        .register_worktree(&label, mode)
        .expect("test worktree registration should persist")
}

pub(crate) fn mutation_credential_json(credential: &MutationCredentialFixture) -> Value {
    json!({
        "credentialId": credential.credential_id,
        "principalToken": credential.principal_token,
    })
}

pub(crate) fn host_with_session_internal_and_limits(
    workspace: WorkspaceSession,
    limits: QueryLimits,
) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        limits,
        PrismMcpFeatures::full()
            .with_internal_developer(true)
            .with_runtime_diagnostics_auto_refresh(false),
    )
}

pub(crate) fn test_session(host: &QueryHost) -> Arc<SessionState> {
    host.cached_test_session()
}

pub(crate) fn retry_on_runtime_sync_busy<T, F>(mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_error = None;
    for _ in 0..20 {
        match op() {
            Ok(value) => return Ok(value),
            Err(error)
                if error
                    .to_string()
                    .contains("request admission busy for `refreshWorkspaceForMutation`")
                    || is_transient_sqlite_lock(&error) =>
            {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("runtime sync busy retry should capture the final error"))
}

pub(crate) fn retry_on_transient_sqlite_lock<T, F>(mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut last_error = None;
    for _ in 0..50 {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if is_transient_sqlite_lock(&error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("sqlite lock retry should capture the final error"))
}

fn is_transient_sqlite_lock(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let text = cause.to_string().to_ascii_lowercase();
        text.contains("database is locked")
            || text.contains("database table is locked")
            || text.contains("database schema is locked")
            || text.contains("locked database")
            || text.contains("sql busy")
    })
}

pub(crate) fn temp_workspace() -> PathBuf {
    let root = temp_test_dir("prism-mcp-test");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    )
    .unwrap();
    root
}

pub(crate) fn repo_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist")
        .to_path_buf()
}

pub(crate) fn wait_for_completed_curator_job(session: &WorkspaceSession) -> String {
    for _ in 0..200 {
        let snapshot = session
            .curator_snapshot()
            .expect("curator snapshot should load");
        if let Some(record) = snapshot
            .records
            .iter()
            .find(|record| curator_job_status_label(record) == "completed")
        {
            return record.id.0.clone();
        }
        thread::sleep(Duration::from_millis(50));
    }
    let snapshot = session
        .curator_snapshot()
        .expect("curator snapshot should load");
    panic!(
        "timed out waiting for completed curator job; statuses: {:?}",
        snapshot
            .records
            .iter()
            .map(|record| (record.id.0.clone(), curator_job_status_label(record)))
            .collect::<Vec<_>>()
    );
}

pub(crate) fn wait_until(description: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..400 {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {description}");
}

pub(crate) fn demo_node() -> Node {
    Node {
        id: NodeId::new("demo", "demo::main", NodeKind::Function),
        name: "main".into(),
        kind: NodeKind::Function,
        file: prism_ir::FileId(1),
        span: Span::new(1, 3),
        language: Language::Rust,
    }
}

pub(crate) fn server_with_node(node: Node) -> PrismMcpServer {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    PrismMcpServer::new(Prism::new(graph))
}

pub(crate) fn server_with_node_and_features(
    node: Node,
    features: PrismMcpFeatures,
) -> PrismMcpServer {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    PrismMcpServer::new_with_features(Prism::new(graph), features)
}

pub(crate) async fn spawn_http_upstream(
    server: PrismMcpServer,
) -> (String, tokio::task::JoinHandle<()>) {
    let service: StreamableHttpService<_, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(server.clone().instrumented_service()),
        Default::default(),
        StreamableHttpServerConfig {
            sse_keep_alive: None,
            ..Default::default()
        },
    );
    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("listener should expose addr");
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), task)
}

pub(crate) fn client_message(raw: &str) -> ClientJsonRpcMessage {
    serde_json::from_str(raw).expect("invalid client json-rpc message")
}

pub(crate) fn initialize_request() -> ClientJsonRpcMessage {
    client_message(
        r#"{
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "prism-mcp-test", "version": "0.0.1" }
                }
            }"#,
    )
}

pub(crate) fn initialized_notification() -> ClientJsonRpcMessage {
    client_message(r#"{ "jsonrpc": "2.0", "method": "notifications/initialized" }"#)
}

pub(crate) fn list_tools_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "tools/list" }}"#
    ))
}

pub(crate) fn list_resources_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "resources/list" }}"#
    ))
}

pub(crate) fn list_resource_templates_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "resources/templates/list" }}"#
    ))
}

pub(crate) fn ping_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "ping" }}"#
    ))
}

pub(crate) fn read_resource_request(id: u64, uri: &str) -> ClientJsonRpcMessage {
    serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "resources/read",
        "params": { "uri": uri },
    }))
    .expect("resources/read request should deserialize")
}

pub(crate) fn call_tool_request(
    id: u64,
    name: &str,
    arguments: serde_json::Map<String, Value>,
) -> ClientJsonRpcMessage {
    serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
        },
    }))
    .expect("tools/call request should deserialize")
}

pub(crate) async fn initialize_client(
    client: &mut impl Transport<rmcp::RoleClient>,
) -> serde_json::Value {
    client.send(initialize_request()).await.unwrap();
    let response = client.receive().await.unwrap();
    serde_json::to_value(response).expect("initialize response should serialize")
}

pub(crate) fn response_json(response: ServerJsonRpcMessage) -> serde_json::Value {
    serde_json::to_value(response).expect("response should serialize")
}

pub(crate) fn first_tool_content_json(response: ServerJsonRpcMessage) -> serde_json::Value {
    let response = response_json(response);
    if !response["error"].is_null() {
        panic!("tool call failed: {response}");
    }
    if !response["result"]["structuredContent"].is_null() {
        return response["result"]["structuredContent"].clone();
    }
    if let Some(content) = response["result"]["content"].as_array() {
        if let Some(json) = content.iter().find_map(|item| {
            let json = item.get("json")?;
            if json.is_null() {
                None
            } else {
                Some(json.clone())
            }
        }) {
            return json;
        }
    }
    let text = response["result"]["content"]
        .as_array()
        .and_then(|content| {
            content
                .iter()
                .find_map(|item| item.get("text").and_then(Value::as_str))
        })
        .unwrap_or_else(|| panic!("tool result should contain json text: {response}"));
    serde_json::from_str(text).expect("tool content should decode as json")
}

pub(crate) fn write_memory_insight_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Memory\n\n## Integration Points\n\nPRISM should enrich memory recall with lineage and prior outcomes.\n\n* current node\n* mapped lineage\n* prior outcomes\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod recall;\nmod persist;\n\npub struct OutcomeMemory;\npub struct SessionMemory;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/recall.rs"),
        "pub fn memory_recall() {}\n\npub fn task_journal_view() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/persist.rs"),
        "pub fn reanchor_persisted_memory_snapshot() {}\n\npub fn persist_memory_snapshot() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/memory_paths.rs"),
        "#[test]\nfn memory_recall_keeps_lineage_context() {}\n",
    )
    .unwrap();
}

pub(crate) fn write_dashboard_validation_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/DASHBOARD_IMPLEMENTATION_SPEC.md"),
        "# Dashboard\n\n## Validation view\n\nThe validation view should surface validation feedback counts and trends.\nIt should read validation feedback and trust metrics from the MCP layer.\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod query_runtime;\nmod host_mutations;\nmod helpers;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/query_runtime.rs"),
        "pub fn validation_feedback_view() {}\n\npub fn validation_feedback_contains() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/host_mutations.rs"),
        "pub struct QueryHost;\n\nimpl QueryHost {\n    pub fn store_validation_feedback(&self) {}\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        "pub fn strip_internal_developer_api_reference() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/validation_feedback.rs"),
        "#[test]\nfn validation_feedback_view_stays_connected() {}\n",
    )
    .unwrap();
}

pub(crate) fn write_compact_default_tools_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# MCP\n\n## 11.2 Compact Default Tools\n\nThe target default agent tools are `prism_locate`, `prism_open`, `prism_workset`, and `prism_expand`.\nThese should stay compact and chain through handles.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/GOVERNANCE.md"),
        "# Governance\n\n## Compact Default Tools Governance\n\nThis governing section defines how compact default tools should chain across locate, open, workset, and expand without losing reviewable follow-through.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/IMPLEMENTATION_SPEC.md"),
        "# Implementation Spec\n\n## Compact Default Tools Runtime Flow\n\nThis adjacent spec explains how `prism_locate`, `prism_open`, `prism_workset`, and `prism_expand` should preserve the same compact handle flow through runtime execution.\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod server_surface;\nmod compact_tools;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/server_surface.rs"),
        "pub fn prism_locate() {}\npub fn prism_open() {}\npub fn prism_workset() {}\npub fn prism_expand() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        "pub fn compact_target_view() {}\npub fn resolve_handle_target() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/compact_tools.rs"),
        "#[test]\nfn compact_locate_promotes_numbered_markdown_headings_to_semantic_handles() {}\n",
    )
    .unwrap();
}

pub(crate) fn write_spec_drift_validation_matrix_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# PRISM\n\n## 11.2 Compact Default Tools\n\nThe compact default tools are `prism_locate`, `prism_open`, `prism_workset`, and `prism_expand`.\nThese tools should stay compact and chain through handles.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/DASHBOARD_IMPLEMENTATION_SPEC.md"),
        "# Dashboard\n\n## Validation view\n\nThe validation view should surface `validation_feedback_view` and keep `store_validation_feedback` connected to live trust metrics.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/CONTRACTS_SPEC.md"),
        "# Contracts\n\n## 6.9 Contract Health\n\nContract health should surface `contract_health_report` and persist durable state through `persist_contract_health`.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/PRISM_FIRST_CLASS_PLANS_SPEC.md"),
        "# Plans\n\n### 16.1 Ready node calculation\n\nReady node calculation should rely on `compute_ready_nodes` and hydrate overlays through `hydrate_plan_overlay`.\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/VALIDATION.md"),
        "# Validation\n\n### 8.7 PRISM MCP / Query Surface Validation\n\nThe query validation path should report through `query_surface_validation_report` and persist feedback with `persist_validation_feedback`.\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod compact_tools;\nmod contracts;\nmod dashboard;\nmod plans;\nmod validation;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        "pub fn prism_locate() {}\npub fn prism_open() {}\npub fn prism_workset() {}\npub fn prism_expand() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/dashboard.rs"),
        "pub fn validation_feedback_view() {}\n\npub fn store_validation_feedback() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/contracts.rs"),
        "pub fn contract_health_report() {}\n\npub fn persist_contract_health() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/plans.rs"),
        "pub fn compute_ready_nodes() {}\n\npub fn hydrate_plan_overlay() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/validation.rs"),
        "pub fn query_surface_validation_report() {}\n\npub fn persist_validation_feedback() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/compact_tools.rs"),
        "#[test]\nfn compact_default_tools_stay_compact() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/dashboard_validation.rs"),
        "#[test]\nfn validation_view_stays_connected() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/contracts_health.rs"),
        "#[test]\nfn contract_health_retains_persistence_signal() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/plans_ready_nodes.rs"),
        "#[test]\nfn ready_node_calculation_remains_grounded() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/query_surface_validation.rs"),
        "#[test]\nfn query_surface_validation_keeps_feedback_paths() {}\n",
    )
    .unwrap();
}

pub(crate) fn write_long_excerpt_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Memory\n\n## Integration Points\n\nPRISM should enrich memory recall with lineage and prior outcomes.\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "mod recall;\n").unwrap();
    fs::write(
        root.join("src/recall.rs"),
        r#"pub fn memory_recall() {
    let alpha = "lineage context";
    let beta = "prior outcomes";
    let gamma = "task journal";
    let delta = "behavioral ranking";
    let epsilon = "validation recipe";
    let zeta = "related failures";
    let eta = "read context";
    let theta = "edit context";
    let iota = "co change neighbors";
    let kappa = "owner hints";
    let lambda = "symbol excerpts";
    let mu = "candidate ranking";
    let nu = "session memory";
    let xi = "evidence chain";
    let omicron = "workspace revision";
    let pi = alpha.len()
        + beta.len()
        + gamma.len()
        + delta.len()
        + epsilon.len()
        + zeta.len()
        + eta.len()
        + theta.len()
        + iota.len()
        + kappa.len()
        + lambda.len()
        + mu.len()
        + nu.len()
        + xi.len()
        + omicron.len();
    assert!(pi > 0);
}

pub fn memory_recall_support() {
    let alpha = "lineage context";
    let beta = "prior outcomes";
    let gamma = "task journal";
    let delta = "behavioral ranking";
    let epsilon = "validation recipe";
    let zeta = "related failures";
    let eta = "read context";
    let theta = "edit context";
    let iota = "co change neighbors";
    let kappa = "owner hints";
    let lambda = "symbol excerpts";
    let mu = "candidate ranking";
    let nu = "session memory";
    let xi = "evidence chain";
    let omicron = "workspace revision";
    let pi = alpha.len()
        + beta.len()
        + gamma.len()
        + delta.len()
        + epsilon.len()
        + zeta.len()
        + eta.len()
        + theta.len()
        + iota.len()
        + kappa.len()
        + lambda.len()
        + mu.len()
        + nu.len()
        + xi.len()
        + omicron.len();
    assert!(pi > 0);
}
"#,
    )
    .unwrap();
}
