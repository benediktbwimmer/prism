use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use clap::Parser;
use rmcp::{
    model::{
        CallToolRequestParams, ClientJsonRpcMessage, ProtocolVersion, ReadResourceRequestParams,
        ServerJsonRpcMessage,
    },
    transport::{
        streamable_http_server::session::local::LocalSessionManager, IntoTransport,
        StreamableHttpServerConfig, StreamableHttpService, Transport,
    },
    ServiceExt,
};

use super::query_replay_cases::{replay_cases, ReplayExpectation, ReplayHostProfile};
use super::*;
use crate::server_surface::{MutationDashboardMeta, MutationRefreshPolicy};
use prism_agent::{InferenceSnapshot, InferredEdgeScope};
use prism_coordination::{CoordinationPolicy, CoordinationStore, PlanCreateInput, TaskCreateInput};
use prism_core::{
    index_workspace_session, index_workspace_session_with_curator, ValidationFeedbackCategory,
    ValidationFeedbackRecord, ValidationFeedbackVerdict,
};
use prism_curator::{
    CandidateConcept, CandidateConceptOperation, CandidateEdge, CandidateMemory,
    CandidateMemoryEvidence, CandidateRiskSummary, CandidateValidationRecipe, CuratorBackend,
    CuratorContext, CuratorJob, CuratorProposal, CuratorRun,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language,
    Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, PlanEdgeKind, PlanId, Span,
    SymbolFingerprint, TaskId,
};
use prism_memory::{
    MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeMemory, OutcomeResult, RecallQuery,
};
use prism_query::{ConceptDecodeLens, ConceptPacket, ConceptProvenance, ConceptScope};
use prism_store::{Graph, SqliteStore, Store};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;

fn host_with_node(node: Node) -> QueryHost {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    QueryHost::new(Prism::new(graph))
}

fn host_with_prism(prism: Prism) -> QueryHost {
    QueryHost::new(prism)
}

fn host_with_session_internal(workspace: WorkspaceSession) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        PrismMcpFeatures::full().with_internal_developer(true),
    )
}

fn host_with_session(workspace: WorkspaceSession) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        QueryLimits::default(),
        PrismMcpFeatures::full(),
    )
}

fn host_with_session_internal_and_limits(
    workspace: WorkspaceSession,
    limits: QueryLimits,
) -> QueryHost {
    QueryHost::with_session_and_limits_and_features(
        workspace,
        limits,
        PrismMcpFeatures::full().with_internal_developer(true),
    )
}

fn test_session(host: &QueryHost) -> Arc<SessionState> {
    host.cached_test_session()
}

fn temp_workspace() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("prism-mcp-test-{suffix}"));
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

fn repo_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root should exist")
        .to_path_buf()
}

fn wait_for_completed_curator_job(session: &WorkspaceSession) -> String {
    for _ in 0..200 {
        let snapshot = session.curator_snapshot();
        if let Some(record) = snapshot
            .records
            .iter()
            .find(|record| curator_job_status_label(record) == "completed")
        {
            return record.id.0.clone();
        }
        thread::sleep(Duration::from_millis(50));
    }
    let snapshot = session.curator_snapshot();
    panic!(
        "timed out waiting for completed curator job; statuses: {:?}",
        snapshot
            .records
            .iter()
            .map(|record| (record.id.0.clone(), curator_job_status_label(record)))
            .collect::<Vec<_>>()
    );
}

fn wait_until(description: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..200 {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {description}");
}

fn demo_node() -> Node {
    Node {
        id: NodeId::new("demo", "demo::main", NodeKind::Function),
        name: "main".into(),
        kind: NodeKind::Function,
        file: prism_ir::FileId(1),
        span: Span::new(1, 3),
        language: Language::Rust,
    }
}

fn server_with_node(node: Node) -> PrismMcpServer {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    PrismMcpServer::new(Prism::new(graph))
}

fn server_with_node_and_features(node: Node, features: PrismMcpFeatures) -> PrismMcpServer {
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();
    PrismMcpServer::new_with_features(Prism::new(graph), features)
}

async fn spawn_http_upstream(server: PrismMcpServer) -> (String, tokio::task::JoinHandle<()>) {
    let service: StreamableHttpService<PrismMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
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

fn client_message(raw: &str) -> ClientJsonRpcMessage {
    serde_json::from_str(raw).expect("invalid client json-rpc message")
}

#[test]
fn cli_no_coordination_flag_disables_coordination_features() {
    let cli = PrismMcpCli::parse_from(["prism-mcp", "--no-coordination"]);
    let features = cli.features();
    assert_eq!(features.mode_label(), "simple");
    assert!(!features.coordination.workflow);
    assert!(!features.coordination.claims);
    assert!(!features.coordination.artifacts);
}

fn initialize_request() -> ClientJsonRpcMessage {
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

fn initialized_notification() -> ClientJsonRpcMessage {
    client_message(r#"{ "jsonrpc": "2.0", "method": "notifications/initialized" }"#)
}

fn list_tools_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "tools/list" }}"#
    ))
}

fn list_resources_request(id: u64) -> ClientJsonRpcMessage {
    client_message(&format!(
        r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "resources/list" }}"#
    ))
}

fn read_resource_request(id: u64, uri: &str) -> ClientJsonRpcMessage {
    serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "resources/read",
        "params": { "uri": uri },
    }))
    .expect("resources/read request should deserialize")
}

fn call_tool_request(
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

async fn initialize_client(client: &mut impl Transport<rmcp::RoleClient>) -> serde_json::Value {
    client.send(initialize_request()).await.unwrap();
    let response = client.receive().await.unwrap();
    serde_json::to_value(response).expect("initialize response should serialize")
}

fn response_json(response: ServerJsonRpcMessage) -> serde_json::Value {
    serde_json::to_value(response).expect("response should serialize")
}

fn first_tool_content_json(response: ServerJsonRpcMessage) -> serde_json::Value {
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

fn write_memory_insight_workspace(root: &Path) {
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

fn write_dashboard_validation_workspace(root: &Path) {
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

fn write_compact_default_tools_workspace(root: &Path) {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# MCP\n\n## 11.2 Compact Default Tools\n\nThe target default agent tools are `prism_locate`, `prism_open`, `prism_workset`, and `prism_expand`.\nThese should stay compact and chain through handles.\n",
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

fn write_long_excerpt_workspace(root: &Path) {
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

#[test]
fn executes_symbol_query() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result["path"], "demo::main");
    assert_eq!(result.result["kind"], "Function");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn coordination_mutations_flow_through_query_runtime() {
    let host = host_with_node(demo_node());
    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship coordination" }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(plan.state["goal"], "Ship coordination");

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit main",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let claim = host
        .store_claim(
            test_session(&host).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(claim.claim_id.is_some());

    let artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:1"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(artifact.artifact_id.is_some());

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(test_session(&host).as_ref(), "test", "dispatch plan"),
    );
    let plan_value = execution
        .dispatch("plan", r#"{ "planId": "plan:1" }"#)
        .unwrap();
    let ready_value = execution
        .dispatch("readyTasks", r#"{ "planId": "plan:1" }"#)
        .unwrap();
    let claims_value = execution
            .dispatch(
                "claims",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }] }"#,
            )
            .unwrap();
    let simulated_value = execution
            .dispatch(
                "simulateClaim",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }], "capability": "Edit", "mode": "HardExclusive" }"#,
            )
            .unwrap_or_else(|error| panic!("simulateClaim dispatch failed: {error:#}"));
    let artifacts_value = execution
        .dispatch("artifacts", r#"{ "taskId": "coord-task:1" }"#)
        .unwrap();
    assert_eq!(plan_value["goal"], "Ship coordination");
    assert_eq!(ready_value.as_array().unwrap().len(), 1);
    assert_eq!(claims_value.as_array().unwrap().len(), 1);
    assert_eq!(artifacts_value.as_array().unwrap().len(), 1);
    assert!(simulated_value.as_array().unwrap().is_empty());
}

#[test]
fn plan_node_mutations_return_graph_native_views() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship first-class plans" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    host.current_prism()
        .replace_curated_concepts(vec![ConceptPacket {
            handle: "concept://native_plan_runtime".to_string(),
            canonical_name: "native_plan_runtime".to_string(),
            summary: "Native plan runtime concept.".to_string(),
            aliases: vec!["plan runtime".to_string()],
            confidence: 0.95,
            core_members: Vec::new(),
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Seeded for MCP native plan node mutation test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "seed".to_string(),
                task_id: None,
            },
            publication: None,
        }]);

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Track plan artifacts"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validation_artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:demo-main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let review_artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:review-main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validation_outcome = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::TestRan,
                anchors: Vec::new(),
                summary: "demo main validated".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();
    let review_outcome = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: Vec::new(),
                summary: "review main validated".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Review main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "kind": "validate",
                    "title": "Edit main",
                    "summary": "Gather validation evidence",
                    "validationRefs": [{ "id": "validation:demo-main" }],
                    "isAbstract": true,
                    "bindings": {
                        "conceptHandles": ["concept://native_plan_runtime"],
                        "artifactRefs": [validation_artifact.artifact_id.as_deref().unwrap()],
                        "memoryRefs": ["memory:demo-main"],
                        "outcomeRefs": [validation_outcome.event_id.as_str()]
                    },
                    "acceptance": [{
                        "label": "main is updated",
                        "requiredChecks": [{ "id": "validation:demo-main" }],
                        "evidencePolicy": "review-and-validation"
                    }],
                    "priority": 3,
                    "tags": ["plans", "validation", "plans"]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();
    assert_eq!(node.state["title"], "Edit main");
    assert_eq!(node.state["kind"], "Validate");
    assert_eq!(node.state["summary"], "Gather validation evidence");
    assert_eq!(node.state["isAbstract"], true);
    assert_eq!(node.state["priority"], 3);
    assert_eq!(node.state["tags"], json!(["plans", "validation"]));
    assert_eq!(
        node.state["bindings"]["conceptHandles"][0],
        "concept://native_plan_runtime"
    );
    assert_eq!(
        node.state["acceptance"][0]["requiredChecks"][0]["id"],
        "validation:demo-main"
    );
    assert_eq!(
        node.state["validationRefs"][0]["id"],
        "validation:demo-main"
    );
    assert_eq!(
        node.state["acceptance"][0]["evidencePolicy"],
        "ReviewAndValidation"
    );

    let updated = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeUpdate,
                payload: json!({
                    "nodeId": node_id.clone(),
                    "kind": "review",
                    "title": "Edit main safely",
                    "summary": "Review the validation evidence",
                    "status": "in-progress",
                    "assignee": "agent:reviewer",
                    "isAbstract": false,
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "bindings": {
                        "conceptHandles": ["concept://native_plan_runtime"],
                        "artifactRefs": [review_artifact.artifact_id.as_deref().unwrap()],
                        "memoryRefs": ["memory:review-main"],
                        "outcomeRefs": [review_outcome.event_id.as_str()]
                    },
                    "dependsOn": [dependency_id.clone()],
                    "acceptance": [{
                        "label": "main still compiles",
                        "requiredChecks": [{ "id": "validation:cargo-test" }],
                        "evidencePolicy": "validation-only"
                    }],
                    "priority": 7,
                    "tags": ["review", "validation", "review"]
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(updated.state["title"], "Edit main safely");
    assert_eq!(updated.state["kind"], "Review");
    assert_eq!(updated.state["summary"], "Review the validation evidence");
    assert_eq!(updated.state["status"], "InProgress");
    assert_eq!(updated.state["assignee"], "agent:reviewer");
    assert_eq!(updated.state["acceptance"].as_array().unwrap().len(), 1);
    assert_eq!(updated.state["isAbstract"], false);
    assert_eq!(updated.state["priority"], 7);
    assert_eq!(updated.state["tags"], json!(["review", "validation"]));
    assert_eq!(
        updated.state["bindings"]["anchors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        updated.state["bindings"]["artifactRefs"][0],
        review_artifact.artifact_id.as_deref().unwrap()
    );
    assert_eq!(
        updated.state["acceptance"][0]["requiredChecks"][0]["id"],
        "validation:cargo-test"
    );
    assert_eq!(
        updated.state["acceptance"][0]["evidencePolicy"],
        "ValidationOnly"
    );

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(graph.root_nodes.iter().any(|root| root.0 == dependency_id));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == node_id));
    let graph_node = graph
        .nodes
        .iter()
        .find(|node| node.id.0 == node_id)
        .expect("graph node");
    assert_eq!(graph_node.kind, prism_ir::PlanNodeKind::Review);
    assert_eq!(
        graph_node.summary.as_deref(),
        Some("Review the validation evidence")
    );
    assert_eq!(graph_node.priority, Some(7));
    assert_eq!(graph_node.tags, vec!["review", "validation"]);
    assert_eq!(
        graph_node.bindings.concept_handles,
        vec!["concept://native_plan_runtime"]
    );

    let cleared = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeUpdate,
                payload: json!({
                    "nodeId": node_id,
                    "assignee": { "op": "clear" },
                    "summary": { "op": "clear" },
                    "priority": { "op": "clear" }
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(cleared.state["summary"], Value::Null);
    assert_eq!(cleared.state["priority"], Value::Null);
    assert_eq!(cleared.state["assignee"], Value::Null);
}

#[test]
fn native_plan_node_completion_rejects_missing_review_and_validation() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Require completion evidence",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Ship main",
                    "acceptance": [{
                        "label": "main is validated",
                        "requiredChecks": [{ "id": "validation:ci" }],
                        "evidencePolicy": "review-and-validation"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "native completion blockers",
        ),
    );
    let blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{node_id}" }}"#),
        )
        .unwrap();
    let kinds = blockers
        .as_array()
        .unwrap()
        .iter()
        .map(|blocker| blocker["kind"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"ReviewRequired".to_string()));
    assert!(kinds.contains(&"ValidationRequired".to_string()));

    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeUpdate,
                payload: json!({
                    "nodeId": node_id,
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .expect_err("completion should reject without review/validation evidence");
    assert!(error.to_string().contains("cannot complete"));
}

#[test]
fn plan_edge_mutations_update_projected_dependency_graph() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Shape execution edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Prepare change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Apply change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let created = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": node_id.clone(),
                    "toNodeId": dependency_id.clone(),
                    "kind": "depends_on"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(created.state["from"], node_id);
    assert_eq!(created.state["to"], dependency_id);
    assert_eq!(created.state["kind"], "DependsOn");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == node_id));

    let deleted = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeDelete,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": node_id.clone(),
                    "toNodeId": dependency_id.clone(),
                    "kind": "depends_on"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(deleted.state["from"], node_id);
    assert_eq!(deleted.state["to"], dependency_id);
    assert_eq!(deleted.state["kind"], "DependsOn");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(!graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(graph.root_nodes.iter().any(|root| root.0 == node_id));
}

#[test]
fn plan_edge_mutations_support_non_dependency_edge_kinds() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Shape native graph edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let source_id = source.state["id"].as_str().unwrap().to_string();

    let target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "kind": "validate",
                    "title": "Validate change",
                    "validationRefs": [{ "id": "validation:change" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let target_id = target.state["id"].as_str().unwrap().to_string();

    let created = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": source_id.clone(),
                    "toNodeId": target_id.clone(),
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(created.state["kind"], "Validates");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == source_id
        && edge.to.0 == target_id
        && edge.kind == PlanEdgeKind::Validates));

    let deleted = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeDelete,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": source_id.clone(),
                    "toNodeId": target_id.clone(),
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(deleted.state["kind"], "Validates");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(!graph.edges.iter().any(|edge| edge.from.0 == source_id
        && edge.to.0 == target_id
        && edge.kind == PlanEdgeKind::Validates));
}

#[test]
fn plan_edge_mutations_enforce_native_edge_semantics() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Enforce edge semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let parent = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Parent group",
                    "isAbstract": true
                }),
                task_id: None,
            },
        )
        .unwrap();
    let parent_id = parent.state["id"].as_str().unwrap().to_string();

    let child = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Child work"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let child_id = child.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": child_id.clone(),
                "toNodeId": parent_id.clone(),
                "kind": "child_of"
            }),
            task_id: None,
        },
    )
    .unwrap();
    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.root_nodes.iter().any(|root| root.0 == parent_id));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == child_id));

    let invalid_target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Plain work"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let invalid_target_id = invalid_target.state["id"].as_str().unwrap().to_string();

    let validates_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": child_id.clone(),
                    "toNodeId": invalid_target_id,
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .expect_err("validates should require validate target");
    assert!(validates_error
        .to_string()
        .contains("must target a Validate node"));

    let handoff_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id,
                    "fromNodeId": child_id,
                    "toNodeId": parent_id,
                    "kind": "handoff_to"
                }),
                task_id: None,
            },
        )
        .expect_err("handoff should reject abstract target");
    assert!(handoff_error
        .to_string()
        .contains("must connect executable nodes"));
}

#[test]
fn plan_node_mutations_reject_runtime_only_binding_handles() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject runtime binding handles" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Bad binding node",
                    "bindings": {
                        "conceptHandles": ["handle:1"]
                    }
                }),
                task_id: None,
            },
        )
        .expect_err("runtime-only binding handle should reject");
    assert!(error
        .to_string()
        .contains("runtime-only handles like `handle:1`"));
}

#[test]
fn plan_node_mutations_reject_missing_published_binding_refs() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject missing published plan refs" }),
                task_id: None,
            },
        )
        .unwrap();
    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Bad binding node",
                    "bindings": {
                        "conceptHandles": ["concept://missing"]
                    }
                }),
                task_id: None,
            },
        )
        .expect_err("missing concept handle should reject");
    assert!(error
        .to_string()
        .contains("must reference an existing concept handle"));
}

#[test]
fn plan_query_reads_surface_native_ready_nodes_and_blockers() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Read native plan runtime semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let blocked = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Blocked" }),
                task_id: None,
            },
        )
        .unwrap();
    let blocked_id = blocked.state["id"].as_str().unwrap().to_string();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Dependency" }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let validator = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Validator",
                    "kind": "Validate",
                    "validationRefs": [{ "id": "validation:validator" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validator_id = validator.state["id"].as_str().unwrap().to_string();

    let handoff_source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Handoff source",
                    "status": "InProgress"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let handoff_source_id = handoff_source.state["id"].as_str().unwrap().to_string();

    let handoff_target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Handoff target" }),
                task_id: None,
            },
        )
        .unwrap();
    let handoff_target_id = handoff_target.state["id"].as_str().unwrap().to_string();

    let free = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Free" }),
                task_id: None,
            },
        )
        .unwrap();
    let free_id = free.state["id"].as_str().unwrap().to_string();

    for payload in [
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": blocked_id.clone(),
            "toNodeId": dependency_id.clone(),
            "kind": "depends_on"
        }),
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": blocked_id.clone(),
            "toNodeId": validator_id.clone(),
            "kind": "validates"
        }),
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": handoff_source_id.clone(),
            "toNodeId": handoff_target_id.clone(),
            "kind": "handoff_to"
        }),
    ] {
        host.store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload,
                task_id: None,
            },
        )
        .unwrap();
    }

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(test_session(&host).as_ref(), "test", "plan query reads"),
    );
    let ready_nodes = execution
        .dispatch("planReadyNodes", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let blocked_node_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{blocked_id}" }}"#),
        )
        .unwrap();
    let handoff_target_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{handoff_target_id}" }}"#),
        )
        .unwrap();
    let execution_overlays = execution
        .dispatch("planExecution", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let summary = execution
        .dispatch("planSummary", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let plans = execution
        .dispatch(
            "plans",
            r#"{ "contains": "native plan runtime", "limit": 5 }"#,
        )
        .unwrap();
    let next = execution
        .dispatch(
            "planNext",
            &format!(r#"{{ "planId": "{plan_id}", "limit": 3 }}"#),
        )
        .unwrap();

    let ready_ids = ready_nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        ready_ids,
        vec![
            dependency_id.clone(),
            validator_id.clone(),
            handoff_source_id.clone(),
            free_id,
        ]
    );

    let blocked_kinds = blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .map(|blocker| blocker["kind"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        blocked_kinds,
        vec![
            "Dependency".to_string(),
            "ValidationGate".to_string(),
            "ValidationRequired".to_string()
        ]
    );
    assert!(blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| {
            blocker["kind"] == Value::String("ValidationGate".to_string())
                && blocker["validationChecks"] == json!(["validation:validator"])
        }));
    assert!(blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| {
            blocker["kind"] == Value::String("ValidationRequired".to_string())
                && blocker["validationChecks"] == json!(["validation:validator"])
        }));
    assert_eq!(
        handoff_target_blockers[0]["kind"],
        Value::String("Handoff".to_string())
    );
    assert!(execution_overlays
        .as_array()
        .unwrap()
        .iter()
        .any(|overlay| {
            overlay["nodeId"] == Value::String(handoff_target_id.clone())
                && overlay["awaitingHandoffFrom"] == Value::String(handoff_source_id.clone())
        }));
    assert_eq!(summary["totalNodes"], Value::from(6));
    assert_eq!(summary["actionableNodes"], Value::from(4));
    assert_eq!(summary["executionBlockedNodes"], Value::from(2));
    assert_eq!(summary["completionGatedNodes"], Value::from(1));
    assert_eq!(summary["validationGatedNodes"], Value::from(1));
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(plans[0]["planId"], Value::String(plan_id.clone()));
    assert_eq!(plans[0]["summary"]["actionableNodes"], Value::from(4));
    let next_id = next[0]["node"]["id"].as_str().unwrap();
    assert!(matches!(
        next_id,
        id if id == dependency_id || id == validator_id || id == handoff_source_id
    ));
    assert_eq!(next[0]["actionable"], Value::Bool(true));
    assert_eq!(next[0]["unblocks"].as_array().unwrap().len(), 1);
}

#[test]
fn plan_query_reads_surface_child_hierarchy_completion_gates() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Read hierarchy completion semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let parent = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Parent",
                    "kind": "Note",
                    "isAbstract": true
                }),
                task_id: None,
            },
        )
        .unwrap();
    let parent_id = parent.state["id"].as_str().unwrap().to_string();

    let child = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Child",
                    "status": "InProgress"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let child_id = child.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": child_id,
                "toNodeId": parent_id.clone(),
                "kind": "child_of"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "child hierarchy semantics",
        ),
    );
    let parent_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{parent_id}" }}"#),
        )
        .unwrap();
    let next = execution
        .dispatch(
            "planNext",
            &format!(r#"{{ "planId": "{plan_id}", "limit": 3 }}"#),
        )
        .unwrap();

    assert_eq!(
        parent_blockers[0]["kind"],
        Value::String("ChildIncomplete".to_string())
    );
    assert!(next
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["node"]["id"] != parent_id)
        .and_then(|entry| entry["unblocks"].as_array())
        .is_some_and(|unblocks| {
            unblocks
                .iter()
                .any(|node| node == &Value::String(parent_id.clone()))
        }));
}

#[test]
fn mcp_returns_structured_coordination_rejections_and_persists_them() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Ship reviewed change",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();

    let rejected = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskUpdate,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected.rejected);
    assert!(!rejected.event_ids.is_empty());
    assert_eq!(rejected.state, Value::Null);
    assert!(rejected
        .violations
        .iter()
        .any(|violation| violation.code == "review_required"));

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let events = reloaded.current_prism().coordination_snapshot().events;
    assert_eq!(
        events.last().unwrap().kind,
        prism_ir::CoordinationEventKind::MutationRejected
    );
}

#[test]
fn mcp_exposes_policy_violations_through_prism_query() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Ship reviewed change",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let rejected = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskUpdate,
                payload: json!({
                    "taskId": task_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected.rejected);

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "dispatch policy violations",
        ),
    );
    let violations = execution
        .dispatch(
            "policyViolations",
            &json!({
                "planId": plan_id,
                "taskId": task_id,
                "limit": 5
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(violations.as_array().unwrap().len(), 1);
    assert!(violations[0]["violations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|violation| violation["code"] == "review_required"));
}

#[test]
fn configure_session_binds_current_agent_and_task_create_inherits_it() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let session = host
        .configure_session(
            test_session(&host).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-a".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();
    assert_eq!(session.current_agent.as_deref(), Some("agent-a"));

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Bind agent identity" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(task.state["assignee"], "agent-a");

    let claims = host.current_prism().coordination_snapshot().claims;
    assert!(claims.is_empty());
}

#[test]
fn configure_session_can_bind_coordination_task_without_current_task_id() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Bind a coordination task into session state" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let session = host
        .configure_session(
            test_session(&host).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: Some(task_id.clone()),
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: None,
                clear_current_agent: None,
            },
        )
        .unwrap();

    let current_task = session.current_task.expect("current task should be set");
    assert_eq!(current_task.task_id, task_id);
    assert_eq!(current_task.description.as_deref(), Some("Edit alpha"));
    assert_eq!(
        current_task.coordination_task_id.as_deref(),
        Some(current_task.task_id.as_str())
    );
}

#[test]
fn plan_edge_mutations_reject_invalid_scheduling_graphs() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject invalid native graph edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let source_id = source.state["id"].as_str().unwrap().to_string();

    let target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Validate change",
                    "kind": "Validate",
                    "validationRefs": [{ "id": "validation:change" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let target_id = target.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": source_id.clone(),
                "toNodeId": target_id.clone(),
                "kind": "validates"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let cycle_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": target_id.clone(),
                    "toNodeId": source_id.clone(),
                    "kind": "handoff_to"
                }),
                task_id: None,
            },
        )
        .expect_err("cross-kind cycle should be rejected");
    assert!(cycle_error.to_string().contains("introduce a cycle"));

    let self_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id,
                    "fromNodeId": source_id.clone(),
                    "toNodeId": source_id,
                    "kind": "blocks"
                }),
                task_id: None,
            },
        )
        .expect_err("self edge should be rejected");
    assert!(self_error.to_string().contains("cannot target itself"));
}

#[test]
fn mcp_plan_update_completes_plan_and_closed_plan_rejects_new_claims() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Single pass coordination" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let rejected_plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected_plan.rejected);
    assert!(rejected_plan
        .violations
        .iter()
        .any(|violation| violation.code == "incomplete_plan_tasks"));

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskUpdate,
            payload: json!({
                "taskId": task_id.clone(),
                "status": "completed"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let completed_plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(!completed_plan.rejected);
    assert_eq!(completed_plan.state["status"], "Completed");

    let rejected_claim = host
        .store_claim(
            test_session(&host).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected_claim.rejected);
    assert!(rejected_claim
        .violations
        .iter()
        .any(|violation| violation.code == "plan_closed"));
}

#[tokio::test]
async fn mcp_server_advertises_tools_and_api_reference_resource() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let initialize = initialize_client(&mut client).await;
    assert_eq!(
        initialize["result"]["protocolVersion"],
        ProtocolVersion::LATEST.as_str()
    );
    assert!(initialize["result"]["capabilities"]["tools"].is_object());
    assert!(initialize["result"]["capabilities"]["resources"].is_object());

    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client.send(list_tools_request(2)).await.unwrap();
    let tools = response_json(client.receive().await.unwrap());
    let tool_names = tools["result"]["tools"]
        .as_array()
        .expect("tools/list should return an array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(tool_names.len(), 10);
    assert!(tool_names.contains(&"prism_locate"));
    assert!(tool_names.contains(&"prism_gather"));
    assert!(tool_names.contains(&"prism_open"));
    assert!(tool_names.contains(&"prism_workset"));
    assert!(tool_names.contains(&"prism_expand"));
    assert!(tool_names.contains(&"prism_task_brief"));
    assert!(tool_names.contains(&"prism_concept"));
    assert!(tool_names.contains(&"prism_query"));
    assert!(tool_names.contains(&"prism_session"));
    assert!(tool_names.contains(&"prism_mutate"));
    for tool in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    client.send(list_resources_request(3)).await.unwrap();
    let resources = response_json(client.receive().await.unwrap());
    assert_eq!(
        resources["result"]["resources"][0]["uri"],
        API_REFERENCE_URI
    );
    assert_eq!(
        resources["result"]["resources"][0]["name"],
        "PRISM API Reference"
    );
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == CAPABILITIES_URI));

    client
        .send(read_resource_request(4, API_REFERENCE_URI))
        .await
        .unwrap();
    let resource = response_json(client.receive().await.unwrap());
    let api_reference = resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("api reference should be text");
    assert!(api_reference.contains("PRISM Agent API"));
    assert!(api_reference.contains("prism_locate"));
    assert!(api_reference.contains("prism_gather"));
    assert!(api_reference.contains("prism_open"));
    assert!(api_reference.contains("prism_query"));
    assert!(!api_reference.contains("runtimeStatus(): RuntimeStatusView;"));
    assert!(!api_reference.contains("queryLog(options?: QueryLogOptions): QueryLogEntryView[];"));

    client
        .send(read_resource_request(5, CAPABILITIES_URI))
        .await
        .unwrap();
    let capabilities = response_json(client.receive().await.unwrap());
    let capabilities_payload = serde_json::from_str::<Value>(
        capabilities["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities resource should be text"),
    )
    .unwrap();
    assert_eq!(capabilities_payload["build"]["serverName"], "prism-mcp");
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "readContext" && method["enabled"] == true));
    assert!(!capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "runtimeStatus"));
    assert_eq!(capabilities_payload["features"]["internalDeveloper"], false);
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == SESSION_URI));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_lists_and_reads_tool_schema_resources() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client.send(list_resources_request(2)).await.unwrap();
    let resources = response_json(client.receive().await.unwrap());
    let resource_uris = resources["result"]["resources"]
        .as_array()
        .expect("resources/list should return an array")
        .iter()
        .filter_map(|resource| resource["uri"].as_str())
        .collect::<Vec<_>>();
    assert!(resource_uris.contains(&CAPABILITIES_URI));
    assert!(resource_uris.contains(&TOOL_SCHEMAS_URI));

    client
        .send(read_resource_request(3, TOOL_SCHEMAS_URI))
        .await
        .unwrap();
    let catalog = response_json(client.receive().await.unwrap());
    let catalog_payload = serde_json::from_str::<Value>(
        catalog["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool schema catalog should be text"),
    )
    .unwrap();
    assert!(catalog_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["toolName"] == "prism_mutate"));

    client
        .send(read_resource_request(4, "prism://schema/tool/prism_mutate"))
        .await
        .unwrap();
    let schema = response_json(client.receive().await.unwrap());
    assert_eq!(
        schema["result"]["contents"][0]["mimeType"],
        "application/schema+json"
    );
    let schema_payload = serde_json::from_str::<Value>(
        schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool schema should be text"),
    )
    .unwrap();
    assert_eq!(
        schema_payload["title"],
        "PRISM Tool Input Schema: prism_mutate"
    );
    assert_eq!(schema_payload["$id"], "prism://schema/tool/prism_mutate");
    assert_eq!(schema_payload["type"], "object");
    assert!(schema_payload.get("oneOf").is_some());
    assert_eq!(
        schema_payload["examples"][0]["action"],
        "validation_feedback"
    );
    assert!(schema_payload.to_string().contains("\"action\""));
    assert!(schema_payload.to_string().contains("validation_feedback"));

    client
        .send(read_resource_request(
            5,
            "prism://schema/tool/prism_session",
        ))
        .await
        .unwrap();
    let session_schema = response_json(client.receive().await.unwrap());
    let session_schema_payload = serde_json::from_str::<Value>(
        session_schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool schema should be text"),
    )
    .unwrap();
    assert_eq!(session_schema_payload["type"], "object");
    assert!(session_schema_payload.get("oneOf").is_some());
    assert_eq!(
        session_schema_payload["examples"][0]["action"],
        "start_task"
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_internal_developer_mode_surfaces_runtime_and_query_history_queries() {
    let server = server_with_node_and_features(
        demo_node(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(read_resource_request(2, API_REFERENCE_URI))
        .await
        .unwrap();
    let resource = response_json(client.receive().await.unwrap());
    let api_reference = resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("api reference should be text");
    assert!(api_reference.contains("runtimeStatus(): RuntimeStatusView;"));
    assert!(api_reference.contains("queryLog(options?: QueryLogOptions): QueryLogEntryView[];"));
    assert!(api_reference.contains(
        "validationFeedback(options?: ValidationFeedbackOptions): ValidationFeedbackView[];"
    ));

    client
        .send(read_resource_request(3, CAPABILITIES_URI))
        .await
        .unwrap();
    let capabilities = response_json(client.receive().await.unwrap());
    let capabilities_payload = serde_json::from_str::<Value>(
        capabilities["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities resource should be text"),
    )
    .unwrap();
    assert_eq!(capabilities_payload["features"]["internalDeveloper"], true);
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "runtimeStatus"));
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "queryLog"));
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "validationFeedback"));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn schema_catalog_and_capabilities_surface_stable_examples() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(read_resource_request(20, "prism://schemas"))
        .await
        .unwrap();
    let catalog = response_json(client.receive().await.unwrap());
    let catalog_payload = serde_json::from_str::<Value>(
        catalog["result"]["contents"][0]["text"]
            .as_str()
            .expect("schema catalog should be text"),
    )
    .unwrap();
    let search_entry = catalog_payload["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resourceKind"] == "search")
        .expect("search schema entry should exist");
    assert_eq!(
        search_entry["exampleUri"],
        "prism://search/read%20context?strategy=behavioral&ownerKind=read&kind=function&path=src&pathMode=exact&structuredPath=workspace&topLevelOnly=true&includeInferred=true"
    );

    client
        .send(read_resource_request(21, "prism://schema/search"))
        .await
        .unwrap();
    let search_schema = response_json(client.receive().await.unwrap());
    let search_schema_payload = serde_json::from_str::<Value>(
        search_schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("search schema should be text"),
    )
    .unwrap();
    assert_eq!(
        search_schema_payload["examples"][0]["query"],
        "read context"
    );
    assert!(search_schema_payload["examples"][0]["topReadContext"].is_object());

    client
        .send(read_resource_request(22, CAPABILITIES_URI))
        .await
        .unwrap();
    let capabilities = response_json(client.receive().await.unwrap());
    let capabilities_payload = serde_json::from_str::<Value>(
        capabilities["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities should be text"),
    )
    .unwrap();
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["name"] == "PRISM Session"
            && resource["exampleUri"] == "prism://session"));
    assert!(capabilities_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_locate" && tool["exampleInput"]["query"] == "session"));
    assert!(capabilities_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_query" && tool["exampleInput"]["language"] == "ts"));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn stdio_proxy_forwards_to_streamable_http_upstream() {
    let (upstream_uri, upstream_task) = spawn_http_upstream(server_with_node(demo_node())).await;
    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_source(
        upstream_uri.clone(),
        crate::daemon_mode::BridgeUpstreamSource::Fixed(upstream_uri),
    )
    .await
    .expect("proxy should connect to upstream");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        let running = proxy
            .serve(server_transport)
            .await
            .expect("proxy should initialize on stdio transport");
        let _ = running.waiting().await;
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let resources = client
        .list_all_resources()
        .await
        .expect("proxy should forward resources/list");
    assert!(resources
        .iter()
        .any(|resource| resource.uri == API_REFERENCE_URI));

    let templates = client
        .list_all_resource_templates()
        .await
        .expect("proxy should forward resource template listing");
    assert!(templates
        .iter()
        .any(|template| template.uri_template == ENTRYPOINTS_RESOURCE_TEMPLATE_URI));

    let tools = client
        .list_all_tools()
        .await
        .expect("proxy should forward tools/list");
    assert!(tools.iter().any(|tool| tool.name == "prism_locate"));
    assert!(tools.iter().any(|tool| tool.name == "prism_gather"));
    assert!(tools.iter().any(|tool| tool.name == "prism_task_brief"));
    assert!(tools.iter().any(|tool| tool.name == "prism_query"));

    let session = client
        .read_resource(ReadResourceRequestParams::new(SESSION_URI))
        .await
        .expect("proxy should forward resources/read");
    assert_eq!(session.contents.len(), 1);

    let query = client
        .call_tool(CallToolRequestParams::new("prism_query").with_arguments(
            serde_json::Map::from_iter([(String::from("code"), json!("return 'proxy-ok';"))]),
        ))
        .await
        .expect("proxy should forward tools/call");
    let query_payload = query.structured_content.unwrap_or_else(|| {
        serde_json::from_str(
            &query.content[0]
                .as_text()
                .expect("query result should expose text content")
                .text,
        )
        .expect("query text content should be valid json")
    });
    assert_eq!(query_payload["result"], "proxy-ok");

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    upstream_task.abort();
    let _ = upstream_task.await;
}

#[tokio::test]
async fn stdio_proxy_stays_alive_while_idle_until_client_disconnects() {
    let (upstream_uri, upstream_task) = spawn_http_upstream(server_with_node(demo_node())).await;
    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_source(
        upstream_uri.clone(),
        crate::daemon_mode::BridgeUpstreamSource::Fixed(upstream_uri),
    )
    .await
    .expect("proxy should connect to upstream");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        proxy
            .serve_transport(server_transport)
            .await
            .expect("proxy should stay alive until the client disconnects");
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let tools = client
        .list_all_tools()
        .await
        .expect("proxy should forward tools/list before becoming idle");
    assert!(tools.iter().any(|tool| tool.name == "prism_query"));

    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        !proxy_task.is_finished(),
        "idle bridge should not exit just because it has been inactive"
    );

    let resources = client
        .list_all_resources()
        .await
        .expect("proxy should still be alive after an idle period");
    assert!(resources
        .iter()
        .any(|resource| resource.uri == API_REFERENCE_URI));

    client.cancel().await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), proxy_task)
        .await
        .expect("proxy should exit after the client disconnects")
        .expect("proxy task should complete cleanly");
    upstream_task.abort();
    let _ = upstream_task.await;
}

#[tokio::test]
async fn stdio_proxy_reconnects_after_upstream_restart_from_uri_file() {
    let uri_file = temp_workspace().join("bridge-uri.txt");
    let (first_uri, first_upstream_task) = spawn_http_upstream(server_with_node(demo_node())).await;
    fs::write(&uri_file, format!("{first_uri}\n")).expect("uri file should be written");

    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_source(
        first_uri.clone(),
        crate::daemon_mode::BridgeUpstreamSource::HttpUriFile(uri_file.clone()),
    )
    .await
    .expect("proxy should connect to upstream");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        proxy
            .serve_transport(server_transport)
            .await
            .expect("proxy should initialize on stdio transport");
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");
    let first_query = client
        .call_tool(CallToolRequestParams::new("prism_query").with_arguments(
            serde_json::Map::from_iter([(
                String::from("code"),
                json!(r#"return prism.symbol("main")?.id.path ?? null;"#),
            )]),
        ))
        .await
        .expect("proxy should forward the first query");
    let first_payload = first_query
        .structured_content
        .expect("query result should be structured");
    assert_eq!(first_payload["result"], "demo::main");

    first_upstream_task.abort();
    let _ = first_upstream_task.await;

    let replacement_node = Node {
        id: NodeId::new("demo", "demo::replacement", NodeKind::Function),
        name: "replacement".into(),
        kind: NodeKind::Function,
        file: prism_ir::FileId(2),
        span: Span::new(1, 1),
        language: Language::Rust,
    };
    let (second_uri, second_upstream_task) =
        spawn_http_upstream(server_with_node(replacement_node)).await;
    fs::write(&uri_file, format!("{second_uri}\n")).expect("uri file should be updated");

    let second_query = tokio::time::timeout(
        Duration::from_secs(10),
        client.call_tool(CallToolRequestParams::new("prism_query").with_arguments(
            serde_json::Map::from_iter([(
                String::from("code"),
                json!(r#"return prism.symbol("replacement")?.id.path ?? null;"#),
            )]),
        )),
    )
    .await
    .expect("reconnect query should complete before the timeout")
    .expect("proxy should reconnect and forward the second query");
    let second_payload = second_query
        .structured_content
        .expect("query result should be structured after reconnect");
    assert_eq!(second_payload["result"], "demo::replacement");

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    second_upstream_task.abort();
    let _ = second_upstream_task.await;
}

#[test]
fn simple_mode_disables_coordination_host_paths() {
    let host = QueryHost::new_with_limits_and_features(
        Prism::new(Graph::default()),
        QueryLimits::default(),
        PrismMcpFeatures::simple(),
    );

    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship coordination" }),
                task_id: None,
            },
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination workflow mutations are disabled"));

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "dispatch simple-mode plan",
        ),
    );
    let error = execution
        .dispatch("plan", r#"{ "planId": "plan:1" }"#)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination workflow queries are disabled"));
}

#[test]
fn query_host_uses_configured_worker_pool_size() {
    let host = QueryHost::new_with_limits_features_and_worker_count(
        Prism::new(Graph::default()),
        QueryLimits::default(),
        PrismMcpFeatures::default(),
        JsWorkerPool::with_worker_count(3),
    );

    assert_eq!(host.worker_pool.worker_count(), 3);
    let result = host
        .execute(test_session(&host), "return 'pool-ok';", QueryLanguage::Ts)
        .expect("typescript query should execute");
    assert_eq!(result.result, json!("pool-ok"));
}

#[tokio::test]
async fn mcp_server_simple_mode_keeps_minimal_surface_and_reports_features() {
    let server = server_with_node_and_features(demo_node(), PrismMcpFeatures::simple());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client.send(list_tools_request(2)).await.unwrap();
    let tools = response_json(client.receive().await.unwrap());
    let tool_names = tools["result"]["tools"]
        .as_array()
        .expect("tools/list should return an array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(tool_names.len(), 10);
    assert!(tool_names.contains(&"prism_locate"));
    assert!(tool_names.contains(&"prism_gather"));
    assert!(tool_names.contains(&"prism_open"));
    assert!(tool_names.contains(&"prism_workset"));
    assert!(tool_names.contains(&"prism_expand"));
    assert!(tool_names.contains(&"prism_task_brief"));
    assert!(tool_names.contains(&"prism_concept"));
    assert!(tool_names.contains(&"prism_query"));
    assert!(tool_names.contains(&"prism_session"));
    assert!(tool_names.contains(&"prism_mutate"));

    client
        .send(read_resource_request(3, SESSION_URI))
        .await
        .unwrap();
    let session = response_json(client.receive().await.unwrap());
    assert_eq!(
        session["result"]["contents"][0]["mimeType"],
        "application/json"
    );
    let session_payload = serde_json::from_str::<Value>(
        session["result"]["contents"][0]["text"]
            .as_str()
            .expect("session resource should be text"),
    )
    .unwrap();
    assert_eq!(session_payload["features"]["mode"], "simple");
    assert_eq!(
        session_payload["features"]["coordination"]["workflow"],
        false
    );
    assert_eq!(session_payload["features"]["coordination"]["claims"], false);
    assert_eq!(
        session_payload["features"]["coordination"]["artifacts"],
        false
    );
    assert_eq!(session_payload["features"]["internalDeveloper"], false);

    client
        .send(read_resource_request(4, CAPABILITIES_URI))
        .await
        .unwrap();
    let capabilities = response_json(client.receive().await.unwrap());
    let capabilities_payload = serde_json::from_str::<Value>(
        capabilities["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities resource should be text"),
    )
    .unwrap();
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "plan" && method["enabled"] == false));
    assert!(capabilities_payload["queryMethods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|method| method["name"] == "claims" && method["enabled"] == false));

    client
        .send(call_tool_request(
            5,
            "prism_mutate",
            json!({
                "action": "coordination",
                "input": {
                    "kind": "plan_create",
                    "payload": { "goal": "Coordinate the main edit" }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let response = response_json(client.receive().await.unwrap());
    assert_eq!(response["error"]["message"], "prism query failed");
    assert_eq!(
        response["error"]["data"]["error"],
        "coordination workflow mutations are disabled by the PRISM MCP server feature flags"
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_prism_query_tool_round_trip() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_query",
            json!({
                "code": r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
                "language": "ts",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();

    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(envelope["result"]["path"], "demo::main");
    assert_eq!(envelope["result"]["kind"], "Function");
    assert_eq!(
        envelope["diagnostics"]
            .as_array()
            .map(|diagnostics| diagnostics.len()),
        Some(0)
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_reports_actionable_tool_input_errors() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "input": {
                    "anchors": [],
                    "prismSaid": "bad",
                    "actuallyTrue": "worse",
                    "category": "projection",
                    "verdict": "helpful"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let response = response_json(client.receive().await.unwrap());
    assert_eq!(response["error"]["code"], -32602);
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("failed to deserialize parameters:"));
    assert!(message.contains(
        "prism_mutate action `validation_feedback` is missing required field `input.context`"
    ));
    assert!(
        message.contains("required fields: context, prismSaid, actuallyTrue, category, verdict")
    );
    assert!(message.contains(
        "Inspect via prism.tool(\"prism_mutate\")?.actions.find((action) => action.action === \"validation_feedback\")"
    ));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_accepts_flat_prism_session_shorthand_input() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_session",
            json!({
                "action": "start_task",
                "description": "Investigate shorthand prism session input",
                "tags": ["mcp", "ergonomics"]
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(envelope["action"], "start_task");
    assert_eq!(
        envelope["session"]["currentTask"]["description"],
        "Investigate shorthand prism session input"
    );
    assert_eq!(
        envelope["session"]["currentTask"]["tags"][0],
        Value::String("mcp".to_string())
    );

    running.cancel().await.unwrap();
}

#[test]
fn prism_mutate_validation_feedback_accepts_flat_snake_case_fields() {
    let args = serde_json::from_value::<PrismMutationArgs>(json!({
        "action": "validation_feedback",
        "context": "Dogfooding broad subsystem workset queries.",
        "prism_said": "Concept routing and recall were helpful.",
        "actually_true": "The concept path found the right subsystem, but the workset route needed improvement.",
        "category": "memory",
        "verdict": "helpful",
        "corrected_manually": true,
        "task_id": "task:dogfood-memory"
    }))
    .expect("snake_case shorthand should deserialize");

    let PrismMutationArgs::ValidationFeedback(args) = args else {
        panic!("expected validation feedback mutation");
    };
    assert_eq!(args.prism_said, "Concept routing and recall were helpful.");
    assert_eq!(
        args.actually_true,
        "The concept path found the right subsystem, but the workset route needed improvement."
    );
    assert_eq!(args.corrected_manually, Some(true));
    assert_eq!(args.task_id.as_deref(), Some("task:dogfood-memory"));
}

#[tokio::test]
async fn mcp_server_accepts_prism_session_start_task_aliases() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_session",
            json!({
                "action": "start_task",
                "label": "Investigate aliased prism session input",
                "tags": ["mcp", "ergonomics"]
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(envelope["action"], "start_task");
    assert_eq!(
        envelope["session"]["currentTask"]["description"],
        "Investigate aliased prism session input"
    );

    running.cancel().await.unwrap();
}

#[test]
fn prism_session_accepts_bind_coordination_task_action() {
    let args = serde_json::from_value::<PrismSessionArgs>(json!({
        "action": "bind_coordination_task",
        "coordinationTaskId": "coord-task:12",
        "tags": ["coordination", "dogfood"]
    }))
    .expect("bind_coordination_task shorthand should deserialize");

    let PrismSessionArgs::BindCoordinationTask(args) = args else {
        panic!("expected bind_coordination_task action");
    };
    assert_eq!(args.coordination_task_id, "coord-task:12");
    assert_eq!(
        args.tags,
        Some(vec!["coordination".to_string(), "dogfood".to_string()])
    );
}

#[tokio::test]
async fn mcp_server_surfaces_structured_prism_query_error_categories() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_query",
            json!({
                "code": "const broken = ;\nreturn broken;"
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let response = response_json(client.receive().await.unwrap());
    assert_eq!(response["error"]["code"], -32603);
    assert_eq!(response["error"]["message"], "prism_query parse failed");
    assert_eq!(response["error"]["data"]["code"], "query_parse_failed");
    assert_eq!(response["error"]["data"]["line"], 1);
    assert_eq!(response["error"]["data"]["column"], 16);
    assert!(response["error"]["data"]["nextAction"]
        .as_str()
        .unwrap_or_default()
        .contains("single expression such as `({ ... })`"));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_coordination_mutations_and_reads_via_prism_query() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "coordination",
                "input": {
                    "kind": "plan_create",
                    "payload": { "goal": "Coordinate the main edit" }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let plan = first_tool_content_json(client.receive().await.unwrap());
    let plan_id = plan["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "input": {
                    "kind": "task_create",
                    "payload": {
                        "planId": plan_id,
                        "title": "Edit main",
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }]
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let task = first_tool_content_json(client.receive().await.unwrap());
    let task_id = task["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            4,
            "prism_mutate",
            json!({
                "action": "claim",
                "input": {
                    "action": "acquire",
                    "payload": {
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }],
                        "capability": "Edit",
                        "mode": "SoftExclusive",
                        "coordinationTaskId": task_id
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let claim = first_tool_content_json(client.receive().await.unwrap());
    assert!(claim["result"]["claimId"].as_str().is_some());

    client
        .send(call_tool_request(
            5,
            "prism_mutate",
            json!({
                "action": "artifact",
                "input": {
                    "action": "propose",
                    "payload": {
                        "taskId": task["result"]["state"]["id"].as_str().unwrap(),
                        "diffRef": "patch:1"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    assert!(artifact["result"]["artifactId"].as_str().is_some());
    let artifact_id = artifact["result"]["artifactId"]
        .as_str()
        .unwrap()
        .to_string();

    client
        .send(call_tool_request(
            6,
            "prism_query",
            json!({
                "code": format!(
                    r#"
const sym = prism.symbol("main");
return {{
  plan: prism.plan("{plan_id}"),
  planGraph: prism.planGraph("{plan_id}"),
  planExecution: prism.planExecution("{plan_id}"),
  ready: prism.readyTasks("{plan_id}"),
  claims: sym ? prism.claims(sym) : [],
  artifacts: prism.artifacts("{task_id}"),
  taskBlastRadius: prism.taskBlastRadius("{task_id}"),
  taskValidationRecipe: prism.taskValidationRecipe("{task_id}"),
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}"),
}};
"#
                ),
                "language": "ts",
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        envelope["result"]["plan"]["goal"],
        "Coordinate the main edit"
    );
    assert_eq!(envelope["result"]["planGraph"]["id"], plan_id);
    assert_eq!(
        envelope["result"]["planGraph"]["nodes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        envelope["result"]["planGraph"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let execution = envelope["result"]["planExecution"].as_array().unwrap();
    assert_eq!(execution.len(), 1);
    assert_eq!(execution[0]["nodeId"], task_id);
    assert!(execution[0]["session"].as_str().is_some());
    assert!(execution[0]["pendingHandoffTo"].is_null());
    assert_eq!(envelope["result"]["ready"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["result"]["claims"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["result"]["artifacts"].as_array().unwrap().len(), 1);
    assert!(envelope["result"]["taskBlastRadius"]["lineages"]
        .as_array()
        .is_some());
    assert_eq!(
        envelope["result"]["taskValidationRecipe"]["taskId"],
        task_id
    );
    assert!(envelope["result"]["taskRisk"]["riskScore"].is_number());
    assert_eq!(
        envelope["result"]["artifactRisk"]["artifactId"],
        artifact["result"]["artifactId"]
    );

    running.cancel().await.unwrap();
}

#[test]
fn drift_candidates_and_task_intent_flow_through_prism_query_reads() {
    let spec = NodeId::new("demo", "docs::request_spec", NodeKind::Document);
    let implementation = NodeId::new("demo", "demo::handle_request", NodeKind::Function);
    let related = NodeId::new("demo", "demo::audit_request", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: spec.clone(),
        name: "request_spec".into(),
        kind: NodeKind::Document,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: implementation.clone(),
        name: "handle_request".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: related.clone(),
        name: "audit_request".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(3),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Specifies,
        source: spec.clone(),
        target: implementation.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::RelatedTo,
        source: spec.clone(),
        target: related.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let host = host_with_prism(Prism::new(graph));
    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Coordinate request handling" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement request flow",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::handle_request",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "dispatch drift candidates",
        ),
    );
    let drift_value = execution.dispatch("driftCandidates", r#"{}"#).unwrap();
    let intent_value = execution
        .dispatch("taskIntent", &format!(r#"{{ "taskId": "{task_id}" }}"#))
        .unwrap();

    assert_eq!(drift_value.as_array().unwrap().len(), 1);
    assert_eq!(drift_value[0]["spec"]["path"], "docs::request_spec");
    assert_eq!(drift_value[0]["reasons"][0], "no validation links");

    assert_eq!(intent_value["taskId"], task_id);
    assert_eq!(intent_value["specs"][0]["path"], "docs::request_spec");
    assert_eq!(
        intent_value["implementations"][0]["path"],
        "demo::handle_request"
    );
    assert_eq!(intent_value["related"][0]["path"], "demo::audit_request");
    assert_eq!(intent_value["driftCandidates"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn mcp_server_reports_review_queues_and_blockers_via_prism_query() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "coordination",
                "input": {
                    "kind": "plan_create",
                    "payload": {
                        "goal": "Review-gated change",
                        "policy": { "requireReviewForCompletion": true }
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let plan = first_tool_content_json(client.receive().await.unwrap());
    let plan_id = plan["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "input": {
                    "kind": "task_create",
                    "payload": {
                        "planId": plan_id,
                        "title": "Patch main",
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }]
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let task = first_tool_content_json(client.receive().await.unwrap());
    let task_id = task["result"]["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            4,
            "prism_mutate",
            json!({
                "action": "artifact",
                "input": {
                    "action": "propose",
                    "payload": {
                        "taskId": task_id,
                        "diffRef": "patch:review-gated"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    assert!(artifact["result"]["artifactId"].as_str().is_some());

    client
        .send(call_tool_request(
            5,
            "prism_query",
            json!({
                "code": format!(
                    r#"
return {{
  blockers: prism.blockers("{task_id}"),
  pendingReviews: prism.pendingReviews("{plan_id}"),
}};
"#
                ),
                "language": "ts",
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        envelope["result"]["blockers"][0]["kind"],
        Value::String("ReviewRequired".to_string())
    );
    assert_eq!(
        envelope["result"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        Value::String("task_blocked".to_string())
    );

    running.cancel().await.unwrap();
}

#[test]
fn coordination_workflow_helpers_summarize_inbox_context_and_claim_preview() {
    let root = temp_workspace();
    let writer = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = writer
        .store_coordination(
            test_session(&writer).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Coordinate alpha",
                    "policy": {
                        "requireReviewForCompletion": true,
                        "maxParallelEditorsPerAnchor": 1
                    }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = writer
        .store_coordination(
            test_session(&writer).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    writer
        .store_claim(
            test_session(&writer).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
        .unwrap();

    writer
        .store_artifact(
            test_session(&writer).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:alpha"
                }),
                task_id: None,
            },
        )
        .unwrap();

    let result = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const alpha = prism.symbol("alpha");
return {{
  inbox: prism.coordinationInbox("{plan_id}"),
  context: prism.taskContext("{task_id}"),
  preview: prism.claimPreview({{
    anchors: alpha ? [alpha] : [],
    capability: "Edit",
    mode: "SoftExclusive",
  }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();

    assert_eq!(
        result.result["inbox"]["readyTasks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result.result["inbox"]["plan"]["id"], plan_id);
    assert_eq!(result.result["inbox"]["planGraph"]["id"], plan_id);
    assert_eq!(
        result.result["inbox"]["planExecution"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        result.result["inbox"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result.result["inbox"]["planSummary"]["planId"], plan_id);
    assert_eq!(
        result.result["inbox"]["planNext"][0]["node"]["id"],
        Value::String(task_id.clone())
    );
    assert_eq!(result.result["context"]["task"]["id"], task_id);
    assert_eq!(result.result["context"]["taskNode"]["id"], task_id);
    assert!(result.result["context"]["taskExecution"].is_null());
    assert_eq!(result.result["context"]["planGraph"]["id"], plan_id);
    assert_eq!(result.result["context"]["planSummary"]["planId"], plan_id);
    assert_eq!(
        result.result["context"]["planNext"][0]["node"]["id"],
        Value::String(task_id.clone())
    );
    assert_eq!(
        result.result["context"]["claims"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        result.result["context"]["blockers"][0]["kind"],
        Value::String("ReviewRequired".to_string())
    );
    assert_eq!(result.result["preview"]["blocked"], Value::Bool(true));
    assert!(result.result["preview"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|conflict| conflict["severity"] == Value::String("Block".to_string())));
}

#[test]
fn multi_session_hosts_coordinate_handoff_review_and_neighbor_claims() {
    let root = temp_workspace();
    let host_a = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let host_b = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-b".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();

    let plan = host_a
        .store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Coordinate alpha across sessions",
                    "policy": {
                        "requireReviewForCompletion": true,
                        "maxParallelEditorsPerAnchor": 1
                    }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host_a
        .store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let first_claim = host_a
        .store_claim(
            test_session(&host_a).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(first_claim.claim_id.is_some());

    let blocked_neighbor_claim = host_b
        .store_claim(
            test_session(&host_b).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::beta",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(blocked_neighbor_claim.claim_id.is_none());
    assert!(blocked_neighbor_claim
        .conflicts
        .iter()
        .any(|conflict| conflict["severity"] == Value::String("Block".to_string())));
    assert!(blocked_neighbor_claim.conflicts.iter().any(|conflict| {
        conflict["overlapKinds"]
            .as_array()
            .map(|kinds| kinds.iter().any(|kind| kind == "File"))
            .unwrap_or(false)
    }));

    host_a
        .store_coordination(
            test_session(&host_a).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Handoff,
                payload: json!({
                    "taskId": task_id.clone(),
                    "toAgent": "agent-b",
                    "summary": "handoff alpha implementation to agent-b"
                }),
                task_id: None,
            },
        )
        .unwrap();

    let handed_off = host_b
        .execute(
            test_session(&host_b),
            &format!(r#"return prism.task("{task_id}");"#),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(handed_off.result["assignee"], Value::Null);
    assert_eq!(handed_off.result["pendingHandoffTo"], "agent-b");
    assert_eq!(handed_off.result["status"], "Blocked");

    let blocked_update = host_b
        .store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskUpdate,
                payload: json!({
                    "taskId": task_id.clone(),
                    "status": "in-progress"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(blocked_update.rejected);
    assert!(blocked_update
        .violations
        .iter()
        .any(|violation| violation.code == "handoff_pending"));

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: None,
                clear_current_agent: Some(true),
            },
        )
        .unwrap();
    let missing_agent = host_b
        .store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::HandoffAccept,
                payload: json!({
                    "taskId": task_id.clone(),
                    "agent": "agent-b"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(missing_agent.rejected);
    assert!(missing_agent
        .violations
        .iter()
        .any(|violation| violation.code == "agent_identity_required"));

    host_b
        .configure_session(
            test_session(&host_b).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-b".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();

    let accepted = host_b
        .store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::HandoffAccept,
                payload: json!({
                    "taskId": task_id.clone(),
                    "agent": "agent-b"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(accepted.state["assignee"], "agent-b");
    assert_eq!(accepted.state["pendingHandoffTo"], Value::Null);
    assert_eq!(accepted.state["status"], "Ready");

    let second_claim = host_b
        .store_claim(
            test_session(&host_b).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id.clone()
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(second_claim.claim_id.is_some());

    let artifact = host_b
        .store_artifact(
            test_session(&host_b).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:alpha-shared"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    host_a
        .store_artifact(
            test_session(&host_a).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Review,
                payload: json!({
                    "artifactId": artifact_id,
                    "verdict": "approved",
                    "summary": "reviewed after handoff"
                }),
                task_id: None,
            },
        )
        .unwrap();

    let completed = host_b
        .store_coordination(
            test_session(&host_b).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskUpdate,
                payload: json!({
                    "taskId": task_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(completed.state["status"], "Completed");

    let final_state = host_a
        .execute(
            test_session(&host_a),
            &format!(
                r#"
return {{
  task: prism.task("{task_id}"),
  inbox: prism.coordinationInbox("{plan_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(final_state.result["task"]["status"], "Completed");
    assert_eq!(
        final_state.result["inbox"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn curator_reads_flow_through_prism_query_and_edge_promotion_is_explicit() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::InferredEdge(CandidateEdge {
                    edge: Edge {
                        kind: EdgeKind::Calls,
                        source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                        target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                        origin: prism_ir::EdgeOrigin::Inferred,
                        confidence: 0.82,
                    },
                    scope: InferredEdgeScope::SessionOnly,
                    evidence: vec!["observed repeated edits".into()],
                    rationale: "alpha usually routes to beta after validation".into(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validated"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let jobs = host
        .execute(
            test_session(&host),
            r#"
return prism.curator.jobs({ status: "completed", limit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(jobs.result.as_array().map(|items| items.len()), Some(1));
    assert_eq!(jobs.result[0]["id"], job_id);
    assert_eq!(jobs.result[0]["proposals"][0]["kind"], "inferred_edge");
    assert_eq!(jobs.result[0]["proposals"][0]["disposition"], "pending");

    let promoted = host
        .promote_curator_edge(
            test_session(&host).as_ref(),
            PrismCuratorPromoteEdgeArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                scope: Some(InferredEdgeScopeInput::Persisted),
                note: Some("accepted after review".into()),
                task_id: Some("task:promotion".into()),
            },
        )
        .unwrap();
    assert!(promoted.edge_id.is_some());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(proposal.result["disposition"], "applied");
    assert_eq!(proposal.result["output"], promoted.edge_id.unwrap());
}

#[test]
fn curator_memory_promotion_persists_and_recall_sees_promoted_entry() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::StructuralMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Structural,
                    content: "alpha owns request routing".to_string(),
                    trust: 0.82,
                    rationale: "Repeated successful fixes anchored on alpha".to_string(),
                    category: Some("ownership_rule".to_string()),
                    evidence: CandidateMemoryEvidence::default(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:routing-note"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_memory(
            test_session(&host).as_ref(),
            PrismCuratorPromoteMemoryArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                trust: None,
                note: Some("promote repeated routing knowledge".into()),
                task_id: Some("task:curator-memory".into()),
            },
        )
        .expect("memory promotion should succeed");
    assert!(promoted.memory_id.is_some());
    assert!(promoted.edge_id.is_none());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  memory: prism.memory.recall({{
    focus: sym ? [sym] : [],
    text: "routing",
    limit: 5,
  }}),
  structuralOnly: prism.memory.recall({{
    focus: sym ? [sym] : [],
    kinds: ["structural"],
    limit: 5,
  }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(
        proposal.result["proposal"]["output"],
        promoted.memory_id.unwrap()
    );
    assert_eq!(proposal.result["memory"][0]["entry"]["kind"], "Structural");
    assert_eq!(
        proposal.result["memory"][0]["entry"]["content"],
        "alpha owns request routing"
    );
    assert_eq!(
        proposal.result["structuralOnly"][0]["entry"]["kind"],
        "Structural"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["provenance"]["origin"],
        "curator"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["rationale"],
        "Repeated successful fixes anchored on alpha"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["structuralRule"]["kind"],
        "ownership_rule"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["structuralRule"]["promoted"],
        true
    );
}

#[test]
fn curator_concept_promotion_persists_session_concept_and_marks_proposal_applied() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha_route() {}
pub fn beta_route() {}
"#,
    )
    .unwrap();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::ConceptCandidate(CandidateConcept {
                    recommended_operation: CandidateConceptOperation::Promote,
                    canonical_name: "route_cluster".to_string(),
                    summary: "Routing hotspot cluster.".to_string(),
                    aliases: vec!["routing".to_string()],
                    core_members: vec![
                        NodeId::new("demo", "demo::alpha_route", NodeKind::Function),
                        NodeId::new("demo", "demo::beta_route", NodeKind::Function),
                    ],
                    supporting_members: Vec::new(),
                    likely_tests: Vec::new(),
                    evidence: vec!["Hotspot edit kept touching the same routing pair.".to_string()],
                    confidence: 0.79,
                    rationale: "Repeated co-change and hotspot edits justify a reusable concept."
                        .to_string(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha_route")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:route-fix"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-concept")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_concept(
            test_session(&host).as_ref(),
            PrismCuratorPromoteConceptArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                scope: None,
                note: Some("accept hotspot concept".into()),
                task_id: Some("task:curator-concept".into()),
            },
        )
        .expect("concept promotion should succeed");
    assert_eq!(promoted.memory_id, None);
    assert_eq!(promoted.edge_id, None);
    assert_eq!(
        promoted.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  concept: prism.conceptByHandle("concept://route_cluster", {{ includeBindingMetadata: true }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(proposal.result["proposal"]["kind"], "concept_candidate");
    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(
        proposal.result["proposal"]["output"],
        Value::String("concept://route_cluster".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["canonicalName"],
        Value::String("route_cluster".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["provenance"]["origin"],
        Value::String("curator".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["provenance"]["kind"],
        Value::String("curator_concept_candidate".to_string())
    );
}

#[test]
fn curator_apply_proposal_promotes_repo_scoped_concept_with_override() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha_route() {}
pub fn beta_route() {}
"#,
    )
    .unwrap();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::ConceptCandidate(CandidateConcept {
                    recommended_operation: CandidateConceptOperation::Promote,
                    canonical_name: "route_cluster".to_string(),
                    summary: "Routing hotspot cluster.".to_string(),
                    aliases: vec!["routing".to_string()],
                    core_members: vec![
                        NodeId::new("demo", "demo::alpha_route", NodeKind::Function),
                        NodeId::new("demo", "demo::beta_route", NodeKind::Function),
                    ],
                    supporting_members: Vec::new(),
                    likely_tests: Vec::new(),
                    evidence: vec!["Hotspot edit kept touching the same routing pair.".to_string()],
                    confidence: 0.79,
                    rationale: "Repeated co-change and hotspot edits justify a reusable concept."
                        .to_string(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha_route")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:route-apply"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-apply-concept")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let applied = host
        .apply_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorApplyProposalArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                note: Some("accept hotspot concept as published knowledge".into()),
                options: Some(PrismCuratorApplyProposalOptionsArgs {
                    edge_scope: None,
                    concept_scope: Some(ConceptScopeInput::Repo),
                    memory_trust: None,
                }),
                task_id: Some("task:curator-apply-concept".into()),
            },
        )
        .expect("generic curator apply should promote concept");
    assert_eq!(applied.kind, "concept_candidate");
    assert_eq!(applied.decision, CuratorProposalDecision::Applied);
    assert_eq!(
        applied.created.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );
    assert_eq!(
        applied.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  concept: prism.conceptByHandle("concept://route_cluster", {{ includeBindingMetadata: true }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(proposal.result["concept"]["scope"], "repo");
    assert_eq!(
        proposal.result["concept"]["publication"]["status"],
        "active"
    );
}

#[test]
fn curator_apply_proposal_routes_risk_summary_through_memory_promotion() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    summary: "alpha is a risky coordination hotspot".to_string(),
                    severity: "high".to_string(),
                    evidence_events: Vec::new(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic-fix"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic-repeat"),
                ts: 52,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed again under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let applied = host
        .apply_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorApplyProposalArgs {
                job_id,
                proposal_index: 0,
                note: Some("promote risk summary after review".into()),
                options: Some(PrismCuratorApplyProposalOptionsArgs {
                    edge_scope: None,
                    concept_scope: None,
                    memory_trust: Some(0.93),
                }),
                task_id: Some("task:alpha-risk-generic".into()),
            },
        )
        .expect("generic curator apply should route risk summary to memory promotion");
    assert_eq!(applied.kind, "risk_summary");
    assert_eq!(applied.decision, CuratorProposalDecision::Applied);
    assert_eq!(applied.edge_id, None);
    assert!(applied.memory_id.is_some());
    assert_eq!(applied.created.memory_id, applied.memory_id);

    let stored = test_session(&host)
        .notes
        .entry(&MemoryId(applied.memory_id.clone().unwrap()))
        .expect("promoted memory should exist");
    assert_eq!(stored.trust, 0.93);
    assert_eq!(stored.content, "alpha is a risky coordination hotspot");
}

#[test]
fn concept_relation_mutation_populates_query_and_compact_neighbor_views() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.9),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-relation".to_string()),
        },
    )
    .unwrap();
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://runtime_surface".to_string()),
            canonical_name: Some("runtime_surface".to_string()),
            summary: Some("Runtime status and entry points.".to_string()),
            aliases: Some(vec!["runtime".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::start_task".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.88),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-relation".to_string()),
        },
    )
    .unwrap();

    let stored = host
        .store_concept_relation(
            session.as_ref(),
            PrismConceptRelationMutationArgs {
                operation: ConceptRelationMutationOperationInput::Upsert,
                source_handle: "concept://custom_validation".to_string(),
                target_handle: "concept://runtime_surface".to_string(),
                kind: ConceptRelationKindInput::OftenUsedWith,
                confidence: Some(0.82),
                evidence: Some(vec![
                    "Validation work usually routes through runtime status.".to_string(),
                ]),
                scope: Some(ConceptScopeInput::Session),
                task_id: Some("task:concept-relation".to_string()),
            },
        )
        .expect("concept relation should store");
    assert_eq!(stored.relation.related_handle, "concept://runtime_surface");

    let query = host
        .execute(
            session,
            r#"
return {
  concept: prism.conceptByHandle("concept://custom_validation", { includeBindingMetadata: true }),
  relations: prism.conceptRelations("concept://custom_validation"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(
        query.result["concept"]["relations"][0]["relatedHandle"],
        Value::String("concept://runtime_surface".to_string())
    );
    assert_eq!(
        query.result["relations"][0]["kind"],
        Value::String("often_used_with".to_string())
    );

    let neighbors = host
        .compact_expand(
            test_session(&host),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: None,
            },
        )
        .expect("neighbor expand should accept concept handles");
    assert_eq!(neighbors.kind, prism_js::AgentExpandKind::Neighbors);
    assert_eq!(
        neighbors.result["relations"][0]["relatedHandle"],
        Value::String("concept://runtime_surface".to_string())
    );
}

#[test]
fn semantic_curator_memory_promotion_persists_and_is_recallable() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::SemanticMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Semantic,
                    content: "Recent outcome context: alpha failed under routing load; validated alpha routing follow-up".to_string(),
                    trust: 0.74,
                    rationale: "Repeated outcomes around alpha form reusable fuzzy context.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: CandidateMemoryEvidence {
                        event_ids: vec![EventId::new("outcome:alpha-risk")],
                        memory_ids: vec![MemoryId("memory:episodic-alpha".to_string())],
                        validation_checks: vec!["test:alpha_regression".to_string()],
                        co_change_lineages: Vec::new(),
                    },
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:semantic-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-fix"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:semantic-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_memory(
            test_session(&host).as_ref(),
            PrismCuratorPromoteMemoryArgs {
                job_id,
                proposal_index: 0,
                trust: None,
                note: Some("promote semantic context".into()),
                task_id: Some("task:semantic-memory".into()),
            },
        )
        .expect("semantic memory promotion should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  kinds: ["semantic"],
  text: "routing load",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("semantic memory recall should succeed");

    assert_eq!(result.result[0]["entry"]["kind"], "Semantic");
    assert_eq!(
        result.result[0]["entry"]["metadata"]["category"],
        "risk_summary"
    );
    assert_eq!(
        result.result[0]["entry"]["metadata"]["evidence"]["validationChecks"][0],
        "test:alpha_regression"
    );
    assert_eq!(
        result.result[0]["entry"]["metadata"]["evidence"]["memoryIds"][0],
        "memory:episodic-alpha"
    );

    let events = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.memory.events({{
  memoryId: "{}",
  limit: 5,
}});
"#,
                promoted.memory_id.clone().unwrap()
            ),
            QueryLanguage::Ts,
        )
        .expect("memory event query should succeed");
    assert_eq!(events.result[0]["promotedFrom"][0], "memory:episodic-alpha");
}

#[test]
fn curator_proposals_query_flattens_pending_proposals_across_jobs() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::SemanticMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Semantic,
                    content: "Recent outcome context: alpha follow-up stayed green".to_string(),
                    trust: 0.72,
                    rationale: "Recent validated changes are reusable context.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: CandidateMemoryEvidence::default(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validated"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let proposals = host
        .execute(
            test_session(&host),
            r#"
return prism.curator.proposals({
  status: "completed",
  disposition: "pending",
  kind: "semantic_memory",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .unwrap();

    assert_eq!(
        proposals.result.as_array().map(|items| items.len()),
        Some(1)
    );
    assert_eq!(proposals.result[0]["jobId"], job_id);
    assert_eq!(proposals.result[0]["jobStatus"], "completed");
    assert_eq!(proposals.result[0]["kind"], "semantic_memory");
    assert_eq!(proposals.result[0]["disposition"], "pending");
}

#[test]
fn promoted_curator_knowledge_feeds_validation_and_risk_queries() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![
                    CuratorProposal::ValidationRecipe(CandidateValidationRecipe {
                        target: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                        checks: vec!["test:alpha_regression".to_string()],
                        rationale: "Repeated alpha regressions need a dedicated test".to_string(),
                        evidence: vec!["failure clusters around alpha routing".to_string()],
                    }),
                    CuratorProposal::RiskSummary(CandidateRiskSummary {
                        anchors: vec![AnchorRef::Node(NodeId::new(
                            "demo",
                            "demo::alpha",
                            NodeKind::Function,
                        ))],
                        summary: "alpha is a risky coordination hotspot".to_string(),
                        severity: "high".to_string(),
                        evidence_events: Vec::new(),
                    }),
                ],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-curator"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-repeat"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed again under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Change alpha safely" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();
    let artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task_id,
                    "diffRef": "patch:alpha-risk"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    let before = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  recipe: sym ? prism.validationRecipe(sym) : null,
  taskRecipe: prism.taskValidationRecipe("{task_id}"),
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("baseline query should succeed");

    assert!(!before.result["recipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert_eq!(
        before.result["taskRisk"]["promotedSummaries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        before.result["artifactRisk"]["promotedSummaries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    host.promote_curator_memory(
        test_session(&host).as_ref(),
        PrismCuratorPromoteMemoryArgs {
            job_id: job_id.clone(),
            proposal_index: 0,
            trust: None,
            note: Some("promote validation recipe".into()),
            task_id: Some(task_id.clone()),
        },
    )
    .expect("validation recipe promotion should succeed");
    host.promote_curator_memory(
        test_session(&host).as_ref(),
        PrismCuratorPromoteMemoryArgs {
            job_id,
            proposal_index: 1,
            trust: None,
            note: Some("promote risk summary".into()),
            task_id: Some(task_id.clone()),
        },
    )
    .expect("risk summary promotion should succeed");

    let after = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  recipe: sym ? prism.validationRecipe(sym) : null,
  taskRecipe: prism.taskValidationRecipe("{task_id}"),
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("post-promotion query should succeed");

    assert!(after.result["recipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert!(after.result["taskRecipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert_eq!(
        after.result["taskRisk"]["promotedSummaries"][0],
        "alpha is a risky coordination hotspot"
    );
    assert_eq!(
        after.result["artifactRisk"]["promotedSummaries"][0],
        "alpha is a risky coordination hotspot"
    );
    assert!(
        after.result["taskRisk"]["riskScore"].as_f64().unwrap()
            > before.result["taskRisk"]["riskScore"].as_f64().unwrap()
    );
    assert!(
        after.result["artifactRisk"]["riskScore"].as_f64().unwrap()
            > before.result["artifactRisk"]["riskScore"].as_f64().unwrap()
    );
}

#[test]
fn curator_rejection_is_a_distinct_mutation() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: Vec::new(),
                    summary: "alpha looks risky".into(),
                    severity: "medium".into(),
                    evidence_events: Vec::new(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:failure"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "alpha follow-up validated".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let rejected = host
        .reject_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorRejectProposalArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                reason: Some("not enough evidence".into()),
                task_id: Some("task:review".into()),
            },
        )
        .unwrap();
    assert!(rejected.edge_id.is_none());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(proposal.result["disposition"], "rejected");
    assert_eq!(proposal.result["note"], "not enough evidence");
}

#[test]
fn js_views_use_camel_case_and_enriched_nested_symbols() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha.clone(),
        target: beta,
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
const graph = sym?.callGraph(1);
const lineage = sym?.lineage();
return {
  crateName: sym?.id.crateName,
  callees: sym?.relations().callees.map((node) => node.id.path) ?? [],
  graphNodes: graph?.nodes.map((node) => node.id.path) ?? [],
  graphDepth: graph?.maxDepthReached ?? null,
  lineageId: lineage?.lineageId ?? null,
  lineageStatus: lineage?.status ?? null,
  currentPath: lineage?.current.id.path ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["crateName"], "demo");
    assert_eq!(result.result["callees"][0], "demo::beta");
    assert_eq!(result.result["graphNodes"][0], "demo::alpha");
    assert_eq!(result.result["graphNodes"][1], "demo::beta");
    assert_eq!(result.result["graphDepth"], 1);
    assert!(result.result["lineageId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("lineage:"));
    assert_eq!(result.result["lineageStatus"], "active");
    assert_eq!(result.result["currentPath"], "demo::alpha");
}

#[test]
fn lineage_views_expose_summaries_and_evidence_details() {
    let renamed = NodeId::new("demo", "demo::new_name", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: renamed.clone(),
        name: "new_name".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::old_name", NodeKind::Function)]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("change:rename"),
            ts: 7,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added: vec![ObservedNode {
            node: Node {
                id: renamed.clone(),
                name: "new_name".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
        }],
        removed: vec![ObservedNode {
            node: Node {
                id: NodeId::new("demo", "demo::old_name", NodeKind::Function),
                name: "old_name".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
        }],
        updated: Vec::new(),
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const lineage = prism.symbol("new_name")?.lineage();
return {
  status: lineage?.status ?? null,
  summary: lineage?.summary ?? null,
  uncertainty: lineage?.uncertainty ?? [],
  eventSummary: lineage?.history[0]?.summary ?? null,
  evidenceCodes: lineage?.history[0]?.evidenceDetails.map((item) => item.code) ?? [],
  evidenceLabels: lineage?.history[0]?.evidenceDetails.map((item) => item.label) ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["status"], "active");
    assert!(result.result["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("Latest event: Renamed from demo::old_name to demo::new_name."));
    assert_eq!(
        result.result["uncertainty"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        result.result["eventSummary"],
        "Renamed from demo::old_name to demo::new_name."
    );
    let evidence_codes = result.result["evidenceCodes"]
        .as_array()
        .expect("evidence codes should be present")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(evidence_codes.contains(&"fingerprint_match"));
    assert!(evidence_codes.contains(&"signature_match"));
    let evidence_labels = result.result["evidenceLabels"]
        .as_array()
        .expect("evidence labels should be present")
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(evidence_labels.contains(&"Exact Fingerprint Match"));
}

#[test]
fn symbol_views_expose_source_locations_and_excerpts() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return {
  location: sym?.location ?? null,
  sourceExcerpt: sym?.sourceExcerpt ?? null,
  excerpt: sym?.excerpt() ?? null,
  tunedExcerpt: sym?.excerpt({ maxChars: 10 }) ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["location"]["startLine"], 1);
    assert_eq!(result.result["location"]["endLine"], 1);
    assert!(
        result.result["location"]["startColumn"]
            .as_u64()
            .expect("startColumn should be numeric")
            >= 1
    );
    assert_eq!(
        result.result["sourceExcerpt"]["text"],
        result.result["excerpt"]["text"]
    );
    assert!(result.result["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha()"));
    assert_eq!(result.result["tunedExcerpt"]["truncated"], true);
}

#[test]
fn structured_config_keys_expose_precise_locations_and_local_excerpts() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("config/app.json"),
        "{\n  \"service\": {\n    \"port\": 8080,\n    \"logging\": true\n  },\n  \"other\": 1\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("config/app.yaml"),
        "service:\n  port: 8080\n  logging: true\nother: 1\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/alpha\"]\nresolver = \"2\"\n\n[dependencies]\nserde = \"1.0\"\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const jsonKey = prism.search("port", { path: "config/app.json", kind: "json-key", limit: 1 })[0];
const yamlKey = prism.search("port", { path: "config/app.yaml", kind: "yaml-key", limit: 1 })[0];
const tomlKey = prism.search("members", { path: "Cargo.toml", kind: "toml-key", limit: 1 })[0];
return {
  json: {
    location: jsonKey?.location ?? null,
    excerpt: jsonKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
  yaml: {
    location: yamlKey?.location ?? null,
    excerpt: yamlKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
  toml: {
    location: tomlKey?.location ?? null,
    excerpt: tomlKey?.excerpt({ contextLines: 0, maxChars: 200 }) ?? null,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("structured config query should succeed");

    assert_eq!(result.result["json"]["location"]["startLine"], 3);
    assert_eq!(result.result["json"]["location"]["endLine"], 3);
    assert!(result.result["json"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"port\": 8080"));
    assert!(!result.result["json"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("\"other\": 1"));

    assert_eq!(result.result["yaml"]["location"]["startLine"], 2);
    assert_eq!(result.result["yaml"]["location"]["endLine"], 2);
    assert!(result.result["yaml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("port: 8080"));
    assert!(!result.result["yaml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("other: 1"));

    assert_eq!(result.result["toml"]["location"]["startLine"], 2);
    assert_eq!(result.result["toml"]["location"]["endLine"], 2);
    assert!(result.result["toml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("members = [\"crates/alpha\"]"));
    assert!(!result.result["toml"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("serde = \"1.0\""));
}

#[test]
fn symbol_views_expose_edit_slices_with_exact_focus_mapping() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.search("memory_recall", { path: "src/recall.rs", kind: "function", limit: 1 })[0];
return {
  location: sym?.location ?? null,
  editSlice: sym?.editSlice({ beforeLines: 2, afterLines: 2, maxLines: 6, maxChars: 120 }) ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(
        result.result["editSlice"]["focus"]["startLine"],
        result.result["location"]["startLine"]
    );
    assert_eq!(
        result.result["editSlice"]["focus"]["endLine"],
        result.result["location"]["endLine"]
    );
    assert_eq!(
        result.result["editSlice"]["startLine"],
        result.result["location"]["startLine"]
    );
    assert_eq!(
        result.result["editSlice"]["endLine"],
        result.result["location"]["endLine"]
    );
    assert_eq!(result.result["editSlice"]["relativeFocus"]["startLine"], 1);
    assert_eq!(
        result.result["editSlice"]["relativeFocus"]["endLine"]
            .as_u64()
            .expect("relative focus end line should be numeric"),
        result.result["location"]["endLine"]
            .as_u64()
            .expect("end line should be numeric")
            - result.result["location"]["startLine"]
                .as_u64()
                .expect("start line should be numeric")
            + 1
    );
    assert!(result.result["editSlice"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn memory_recall()"));
    assert_eq!(result.result["editSlice"]["truncated"], true);
}

#[test]
fn focused_blocks_return_exact_local_context_for_code_and_doc_targets() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(test_session(&host),
            r#"
const fnSym = prism.search("memory_recall", { path: "src/recall.rs", kind: "function", limit: 1 })[0];
const spec = prism.search("Integration Points", { path: "docs/SPEC.md", kind: "markdown-heading", limit: 1 })[0];
return {
  functionBlock: fnSym ? prism.focusedBlock(fnSym, { beforeLines: 1, afterLines: 1, maxLines: 6, maxChars: 180 }) : null,
  specBlock: spec ? prism.focusedBlock(spec, { maxLines: 4, maxChars: 160 }) : null,
  readQueries: fnSym ? prism.readContext(fnSym).suggestedQueries.map((query) => query.label) : [],
  editQueries: fnSym ? prism.editContext(fnSym).suggestedQueries.map((query) => query.label) : [],
  validationQueries: fnSym ? prism.validationContext(fnSym).suggestedQueries.map((query) => query.label) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("focused-block query should succeed");

    assert_eq!(result.result["functionBlock"]["strategy"], "edit_slice");
    assert_eq!(
        result.result["functionBlock"]["symbol"]["name"],
        "memory_recall"
    );
    assert_eq!(
        result.result["functionBlock"]["slice"]["focus"]["startLine"],
        1
    );
    assert!(result.result["functionBlock"]["slice"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn memory_recall()"));

    let spec_block = &result.result["specBlock"];
    assert_eq!(spec_block["symbol"]["kind"], "MarkdownHeading");
    assert!(spec_block["strategy"] == "edit_slice" || spec_block["strategy"] == "excerpt_fallback");
    let spec_text = spec_block["slice"]["text"]
        .as_str()
        .or_else(|| spec_block["excerpt"]["text"].as_str())
        .unwrap_or_default();
    assert!(spec_text.contains("## Integration Points"));

    for key in ["readQueries", "editQueries", "validationQueries"] {
        assert!(result.result[key]
            .as_array()
            .expect("query labels should be an array")
            .iter()
            .any(|label| label == "Focused Block"));
    }
}

#[test]
fn prism_tool_queries_surface_schema_actions_and_examples() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(test_session(&host),
            r#"
const tools = prism.tools();
const mutate = prism.tool("prism_mutate");
const validationFeedback = mutate?.actions.find((action) => action.action === "validation_feedback");
const missing = prism.tool("bogus_tool");
return {
  tools,
  mutate,
  validationFeedback,
  missing,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("tool schema query should succeed");

    let tools = result.result["tools"].as_array().expect("tool catalog");
    assert_eq!(tools.len(), 10);
    assert!(tools.iter().any(|tool| tool["toolName"] == "prism_locate"));
    assert!(tools
        .iter()
        .any(|tool| tool["toolName"] == "prism_task_brief"));
    assert!(tools.iter().any(|tool| tool["toolName"] == "prism_concept"));
    assert!(tools.iter().any(|tool| tool["toolName"] == "prism_mutate"));
    assert!(tools
        .iter()
        .any(|tool| tool["exampleInput"]["action"] == "validation_feedback"));

    let mutate = &result.result["mutate"];
    assert_eq!(mutate["toolName"], "prism_mutate");
    assert_eq!(
        mutate["actions"].as_array().map(|items| items.len()),
        Some(17)
    );
    assert_eq!(
        mutate["exampleInput"]["input"]["prismSaid"],
        "Search result ordering was helpful."
    );

    let validation_feedback = &result.result["validationFeedback"];
    assert_eq!(validation_feedback["action"], "validation_feedback");
    assert_eq!(
        validation_feedback["requiredFields"]
            .as_array()
            .expect("required fields")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec![
            "context",
            "prismSaid",
            "actuallyTrue",
            "category",
            "verdict"
        ]
    );
    let verdict_field = validation_feedback["fields"]
        .as_array()
        .expect("field summaries")
        .iter()
        .find(|field| field["name"] == "verdict")
        .expect("verdict field");
    assert_eq!(
        verdict_field["enumValues"]
            .as_array()
            .expect("verdict enum values")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["wrong", "stale", "noisy", "helpful", "mixed"]
    );
    let anchors_field = validation_feedback["fields"]
        .as_array()
        .expect("field summaries")
        .iter()
        .find(|field| field["name"] == "anchors")
        .expect("anchors field");
    let nested_anchor_fields = anchors_field["nestedFields"]
        .as_array()
        .expect("nested anchor field summaries")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect::<Vec<_>>();
    assert!(nested_anchor_fields.contains(&"type"));
    assert!(
        nested_anchor_fields.contains(&"crateName") || nested_anchor_fields.contains(&"crate_name")
    );
    assert!(nested_anchor_fields.contains(&"path"));
    assert!(nested_anchor_fields.contains(&"kind"));
    assert!(
        nested_anchor_fields.contains(&"lineageId") || nested_anchor_fields.contains(&"lineage_id")
    );
    assert!(anchors_field["schema"]
        .to_string()
        .contains("\"properties\""));

    assert!(result.result["missing"].is_null());
}

#[test]
fn compact_agent_tools_keep_handles_stable_within_one_session() {
    let host = host_with_node(demo_node());
    let session = test_session(&host);

    let first = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "main".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("first locate should succeed");
    let second = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "main".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("second locate should succeed");

    assert_eq!(first.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(first.candidates.len(), 1);
    assert_eq!(second.candidates.len(), 1);
    assert_eq!(first.candidates[0].handle, second.candidates[0].handle);
}

#[test]
fn compact_locate_schema_surfaces_filters_and_preview_knobs() {
    let schema = crate::tool_schema_view("prism_locate").expect("locate schema should exist");
    let properties = schema.input_schema["properties"]
        .as_object()
        .expect("locate schema should expose object properties");

    assert!(properties["path"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("file path fragment")));
    assert!(properties["glob"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("glob")));
    assert!(properties["includeTopPreview"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("top-ranked candidate")));
}

#[test]
fn compact_gather_schema_surfaces_exact_text_knobs() {
    let schema = crate::tool_schema_view("prism_gather").expect("gather schema should exist");
    let properties = schema.input_schema["properties"]
        .as_object()
        .expect("gather schema should expose object properties");

    assert!(properties["query"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("Exact text")));
    assert!(properties["path"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("file path fragment")));
    assert!(properties["glob"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("glob")));
}

#[test]
fn prism_mutate_schema_surfaces_concept_action() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let concept = schema
        .actions
        .iter()
        .find(|action| action.action == "concept")
        .expect("concept action should exist");

    assert!(concept.required_fields.contains(&"operation".to_string()));
    assert!(concept
        .fields
        .iter()
        .any(|field| field.name == "canonicalName"));
    assert!(concept
        .fields
        .iter()
        .any(|field| field.name == "coreMembers"));
}

#[test]
fn prism_mutate_schema_surfaces_action_specific_examples() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let missing_examples = schema
        .actions
        .iter()
        .filter(|action| action.example_input.is_none())
        .map(|action| action.action.as_str())
        .collect::<Vec<_>>();
    assert!(
        missing_examples.is_empty(),
        "missing examples for actions: {missing_examples:?}"
    );

    let validation_feedback = schema
        .actions
        .iter()
        .find(|action| action.action == "validation_feedback")
        .expect("validation feedback action should exist");
    let memory = schema
        .actions
        .iter()
        .find(|action| action.action == "memory")
        .expect("memory action should exist");
    let concept = schema
        .actions
        .iter()
        .find(|action| action.action == "concept")
        .expect("concept action should exist");

    assert_eq!(
        validation_feedback
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("validation_feedback")
    );
    assert_eq!(
        memory
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("memory")
    );
    assert_eq!(
        concept
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("concept")
    );

    let mutate_schema =
        crate::tool_input_schema_value("prism_mutate").expect("mutate schema value should exist");
    let mutate_examples = mutate_schema["examples"]
        .as_array()
        .expect("mutate examples should be an array")
        .iter()
        .filter_map(|value| value.get("action").and_then(Value::as_str))
        .collect::<Vec<_>>();
    for action in [
        "validation_feedback",
        "outcome",
        "memory",
        "concept",
        "concept_relation",
        "infer_edge",
        "coordination",
        "claim",
        "artifact",
        "test_ran",
        "failure_observed",
        "fix_validated",
        "curator_apply_proposal",
        "curator_promote_edge",
        "curator_promote_concept",
        "curator_promote_memory",
        "curator_reject_proposal",
    ] {
        assert!(
            mutate_examples.contains(&action),
            "missing mutate example for action `{action}`"
        );
    }
}

#[test]
fn prism_mutate_schema_expands_payload_shapes_for_structured_actions() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");

    let payload_fields = ["memory", "coordination", "claim", "artifact"]
        .into_iter()
        .map(|action| {
            let payload = schema
                .actions
                .iter()
                .find(|candidate| candidate.action == action)
                .and_then(|candidate| {
                    candidate
                        .fields
                        .iter()
                        .find(|field| field.name == "payload")
                })
                .expect("payload field should exist");
            (action, payload)
        })
        .collect::<Vec<_>>();

    for (action, payload) in &payload_fields {
        assert_ne!(
            payload.schema,
            Value::Bool(true),
            "{action} payload stayed opaque"
        );
        assert!(
            payload.schema.to_string().contains("\"properties\"")
                || payload.schema.to_string().contains("\"oneOf\""),
            "{action} payload schema should expose structure"
        );
    }

    let memory_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "memory")
        .expect("memory payload should exist")
        .1;
    let memory_nested = memory_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(memory_nested.contains(&"anchors"));
    assert!(memory_nested.contains(&"kind"));
    assert!(memory_nested.contains(&"content"));

    let coordination_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "coordination")
        .expect("coordination payload should exist")
        .1;
    assert_eq!(
        coordination_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(10)
    );
    let coordination_nested = coordination_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(coordination_nested.contains(&"planId"));
    assert!(coordination_nested.contains(&"taskId"));
    assert!(coordination_nested.contains(&"title"));

    let claim_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "claim")
        .expect("claim payload should exist")
        .1;
    assert_eq!(
        claim_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(3)
    );
    let claim_nested = claim_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(claim_nested.contains(&"anchors"));
    assert!(claim_nested.contains(&"capability"));
    assert!(claim_nested.contains(&"claimId"));

    let artifact_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "artifact")
        .expect("artifact payload should exist")
        .1;
    assert_eq!(
        artifact_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(3)
    );
    let artifact_nested = artifact_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(artifact_nested.contains(&"taskId"));
    assert!(artifact_nested.contains(&"artifactId"));
    assert!(artifact_nested.contains(&"verdict"));
}

#[test]
fn prism_query_reports_complete_mutate_examples_and_payload_shapes() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const mutate = prism.tool("prism_mutate");
return {
  missingExamples: mutate?.actions.filter((action) => !action.exampleInput).map((action) => action.action) ?? [],
  opaquePayloadActions: mutate?.actions
    .filter((action) =>
      action.fields.some(
        (field) => field.name === "payload" && JSON.stringify(field.schema) === "true"
      )
    )
    .map((action) => action.action) ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("tool schema query should succeed");

    assert_eq!(result.result["missingExamples"], json!([]));
    assert_eq!(result.result["opaquePayloadActions"], json!([]));
}

#[test]
fn compact_locate_uses_intent_to_choose_between_code_and_docs() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
# Demo

## Event Journal

This section explains the event journal flow.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let edit = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("edit locate should succeed");
    assert_eq!(edit.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(edit.candidates[0].kind, NodeKind::Function);
    assert_eq!(edit.candidates[0].path, "demo::event_journal_snapshot");

    let explain = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("explain locate should succeed");
    assert_eq!(explain.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(explain.candidates[0].kind, NodeKind::MarkdownHeading);
    assert!(explain.candidates[0]
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("docs/SPEC.md")));
}

#[test]
fn compact_locate_defaults_to_docs_friendly_intent_for_docs_paths() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
# Demo

## Event Journal

This section explains the event journal flow.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: None,
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);
}

#[test]
fn prism_locate_accepts_docs_alias_task_intent() {
    let args: PrismLocateArgs = serde_json::from_value(json!({
        "query": "event journal",
        "taskIntent": "docs",
    }))
    .expect("docs alias should deserialize");

    assert!(matches!(
        args.task_intent,
        Some(PrismLocateTaskIntentInput::Explain)
    ));
}

#[test]
fn prism_locate_accepts_code_and_read_alias_task_intent() {
    for alias in ["code", "read"] {
        let args: PrismLocateArgs = serde_json::from_value(json!({
            "query": "event journal",
            "taskIntent": alias,
        }))
        .expect("inspect alias should deserialize");

        assert!(matches!(
            args.task_intent,
            Some(PrismLocateTaskIntentInput::Inspect)
        ));
    }
}

#[test]
fn compact_locate_prefers_identifier_matches_over_test_helpers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
pub mod helpers;
pub mod tests;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn compact_open() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn cached_related_memory() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/tests.rs"),
        r#"
pub fn compact_open_returns_compact_related_handles() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open related_handles".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::Function);
    assert_eq!(
        locate.candidates[0].path,
        "demo::compact_tools::compact_open"
    );
}

#[test]
fn compact_locate_promotes_exact_identifier_candidates_before_fuzzy_matches() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
pub mod locate_helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn locate_intent_profile() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/locate_helpers.rs"),
        r#"
pub fn locate_query_tokens() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "locate_intent_profile compact_tools".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(
        locate.candidates[0].path,
        "demo::compact_tools::locate_intent_profile"
    );
}

#[test]
fn compact_locate_can_include_top_preview() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn compact_open() {
    println!("preview");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open".to_string(),
                path: Some("src/compact_tools.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    let preview = locate
        .top_preview
        .expect("locate should include a top preview");
    assert_eq!(preview.handle, locate.candidates[0].handle);
    assert!(preview.text.contains("pub fn compact_open"));
}

#[test]
fn compact_locate_can_return_text_fragment_handles_for_exact_script_metrics() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_query_calls = 0
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": prism_compact_tool_calls}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::Document);
    assert!(locate.candidates[0].path.contains("benchmark_codex.py:"));
    assert!(locate.candidates[0]
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("benchmark_codex.py")));
    let preview = locate
        .top_preview
        .expect("text-fragment locate should include a preview");
    assert!(preview.text.contains("prism_compact_tool_calls"));

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    assert!(open.text.contains("prism_compact_tool_calls"));
}

#[test]
fn compact_open_remaps_stale_text_fragment_handles_after_file_edits() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        "def helper():\n    prism_query_calls = 0\n    prism_compact_tool_calls = 0\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::Document);

    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        "def helper():\n    prism_query_calls = 0\n    helper_value = 1\n    prism_compact_tool_calls = 0\n",
    )
    .unwrap();

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should remap the stale text-fragment handle");

    assert!(open.remapped);
    assert_eq!(open.start_line, 4);
    assert_eq!(open.end_line, 4);
    assert!(open.text.contains("prism_compact_tool_calls = 0"));
}

#[test]
fn compact_open_returns_compact_related_handles() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {
    persist_event_journal();
}
fn persist_event_journal() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
## Event Journal

The event journal snapshot should persist journal entries.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    let related = open
        .related_handles
        .expect("open should surface compact related handles");
    assert!(!related.is_empty());
    assert!(related.len() <= 2);
    assert!(related.iter().all(|target| target.file_path.is_none()));
    assert!(related
        .iter()
        .any(|target| target.kind == NodeKind::Function));
}

#[test]
fn compact_open_raw_reads_the_literal_symbol_span_instead_of_only_the_signature_line() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn compact_open() {
    let preview = "preview";
    println!("{preview}");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("raw open should succeed");

    assert!(open.text.contains("let preview = \"preview\";"));
    assert!(open.text.contains("println!"));
    assert!(open.end_line > open.start_line);
}

#[test]
fn compact_open_supports_exact_workspace_paths_without_locate() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha() {}
pub fn beta() {
    let value = 42;
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Raw),
                line: Some(3),
                before_lines: Some(0),
                after_lines: Some(2),
                max_chars: Some(200),
            },
        )
        .expect("path open should succeed");

    assert_eq!(
        open.handle_category,
        prism_js::AgentHandleCategoryView::TextFragment
    );
    assert!(open.handle.starts_with("handle:"));
    assert!(open.text.contains("pub fn beta()"));
    assert!(open.text.contains("let value = 42;"));

    let reopened = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(open.handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("reopening the exact-path handle should succeed");
    assert_eq!(reopened.text, open.text);
    assert_eq!(reopened.start_line, open.start_line);
    assert_eq!(reopened.end_line, open.end_line);
}

#[test]
fn compact_open_rejects_non_raw_modes_for_exact_paths() {
    let root = temp_workspace();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let error = host
        .compact_open(
            test_session(&host),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect_err("path open should reject focus mode");

    assert!(error
        .to_string()
        .contains("path-based prism_open currently supports only raw mode"));
}

#[test]
fn compact_workset_for_text_fragment_handles_surfaces_related_slices() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": 1}
    print("prism_compact_tool_calls")
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.why.contains("Exact text hit"));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open")));
}

#[test]
fn compact_concept_returns_validation_packet_and_decode() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::start_task".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:compact-concept".to_string()),
        },
    )
    .expect("concept setup should succeed");

    let concept = host
        .compact_concept(
            Arc::clone(&session),
            PrismConceptArgs {
                handle: None,
                query: Some("validation".to_string()),
                lens: Some(PrismConceptLensInput::Validation),
                include_binding_metadata: Some(true),
            },
        )
        .expect("concept tool should succeed");

    assert_eq!(concept.packet.handle, "concept://custom_validation");
    assert!(!concept.packet.core_members.is_empty());
    assert!(concept.packet.binding_metadata.is_some());
    assert!(concept
        .packet
        .resolution
        .as_ref()
        .is_some_and(|resolution| !resolution.reasons.is_empty()));
    assert!(concept
        .decode
        .as_ref()
        .and_then(|decode| decode.validation_recipe.as_ref())
        .is_some());
}

#[test]
fn compact_concept_returns_alternates_for_ambiguous_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn healthcheck_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://validation_pipeline".to_string()),
            canonical_name: Some("validation_pipeline".to_string()),
            summary: Some("Validation checks and likely tests.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.92),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:ambiguous-validation".to_string()),
        },
    )
    .expect("validation concept setup should succeed");
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://validation_health_checks".to_string()),
            canonical_name: Some("validation_health_checks".to_string()),
            summary: Some(
                "Validation-oriented runtime health checks and status probes.".to_string(),
            ),
            aliases: Some(vec!["validation".to_string(), "health checks".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::healthcheck_status".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.9),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:ambiguous-validation".to_string()),
        },
    )
    .expect("runtime concept setup should succeed");

    let concept = host
        .compact_concept(
            Arc::clone(&session),
            PrismConceptArgs {
                handle: None,
                query: Some("validation".to_string()),
                lens: None,
                include_binding_metadata: Some(false),
            },
        )
        .expect("concept tool should succeed");

    assert!(!concept.alternates.is_empty());
    assert!(concept
        .alternates
        .iter()
        .any(|alternate| alternate.handle == "concept://validation_health_checks"));
}

#[test]
fn compact_tools_route_or_reject_concept_handles_with_clear_followups() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn validation_recipe_test() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe_test".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![
                PrismConceptLensInput::Open,
                PrismConceptLensInput::Workset,
                PrismConceptLensInput::Validation,
            ]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-followup".to_string()),
        },
    )
    .expect("concept setup should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some("concept://custom_validation".to_string()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should accept concept handles");
    assert_eq!(open.handle, "concept://custom_validation");
    assert_eq!(
        open.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert!(open.file_path.ends_with("/src/lib.rs"));
    assert!(open.text.contains("pub fn validation_recipe()"));
    assert_eq!(
        open.promoted_handle
            .as_ref()
            .expect("concept open should stage a primary member")
            .path,
        "demo::validation_recipe"
    );
    assert_eq!(
        open.related_handles
            .as_ref()
            .expect("concept open should expose related members")[0]
            .path,
        "demo::runtime_status"
    );
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset")));
    assert!(open
        .suggested_actions
        .iter()
        .any(|action| action.tool == "prism_expand"));

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some("concept://custom_validation".to_string()),
                query: None,
            },
        )
        .expect("workset should accept concept handles");
    assert_eq!(workset.primary.path, "demo::validation_recipe");
    assert_eq!(
        workset.primary.handle_category,
        prism_js::AgentHandleCategoryView::Symbol
    );
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_concept")));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation expand should accept concept handles");
    assert_eq!(
        validation.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(validation.kind, prism_js::AgentExpandKind::Validation);
    assert!(validation.result["likelyTests"].is_array());
    assert!(validation.result["why"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(validation
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset")));

    let health = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Health,
                include_top_preview: None,
            },
        )
        .expect("health expand should accept concept handles");
    assert_eq!(
        health.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(health.kind, prism_js::AgentExpandKind::Health);
    assert_eq!(health.result["status"], "drifted");
    assert!(health.result["repairTaskPayload"].is_object());
    assert!(health.result["signals"]["staleValidationLinks"]
        .as_bool()
        .unwrap());
    assert!(health
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("timeline")));

    host.store_outcome(
        session.as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FailureObserved,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validation failed".to_string(),
            result: Some(OutcomeResultInput::Failure),
            evidence: None,
            task_id: None,
        },
    )
    .expect("failure outcome should store");
    let mut memory = MemoryEntry::new(MemoryKind::Structural, "validation concept memory");
    memory.anchors = vec![prism_ir::AnchorRef::Node(NodeId::new(
        "demo",
        "demo::validation_recipe",
        NodeKind::Function,
    ))];
    session.notes.store(memory).expect("memory should store");

    let timeline = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Timeline,
                include_top_preview: None,
            },
        )
        .expect("timeline expand should accept concept handles");
    assert_eq!(
        timeline.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(timeline.kind, prism_js::AgentExpandKind::Timeline);
    assert!(timeline.result["recentEvents"].is_array());
    assert_eq!(
        timeline.result["lastFailure"]["summary"],
        "validation failed"
    );

    let memory = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Memory,
                include_top_preview: None,
            },
        )
        .expect("memory expand should accept concept handles");
    assert_eq!(
        memory.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(memory.kind, prism_js::AgentExpandKind::Memory);
    assert!(memory.result["memories"].is_array());
    assert_eq!(
        memory.result["memories"][0]["summary"],
        "validation concept memory"
    );

    let unsupported_expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Lineage,
                include_top_preview: None,
            },
        )
        .expect_err("unsupported expand kind should still reject concept handles");
    assert!(
        unsupported_expand.to_string().contains("prism_concept"),
        "{unsupported_expand}"
    );
    assert!(
        !unsupported_expand
            .to_string()
            .contains("rerun prism_locate"),
        "{unsupported_expand}"
    );
}

#[test]
fn query_runtime_exposes_concept_packets_and_decode() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    host.store_concept(
        test_session(&host).as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::start_task".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:query-concept".to_string()),
        },
    )
    .expect("concept setup should succeed");
    let envelope = host
        .execute(
            test_session(&host),
            r#"
return {
  concept: prism.concept("validation", { includeBindingMetadata: true }),
  byHandle: prism.conceptByHandle("concept://custom_validation", { includeBindingMetadata: true }),
  decoded: prism.decodeConcept({ query: "validation", lens: "validation", includeBindingMetadata: true }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");

    assert_eq!(
        envelope.result["concept"]["handle"],
        Value::String("concept://custom_validation".to_string())
    );
    assert!(envelope.result["concept"]["resolution"].is_object());
    assert!(envelope.result["concept"]["bindingMetadata"].is_object());
    assert!(envelope.result["byHandle"]["bindingMetadata"].is_object());
    assert!(envelope.result["decoded"]["concept"]["bindingMetadata"].is_object());
    assert!(envelope.result["decoded"]["validationRecipe"].is_object());
}

#[test]
fn concept_mutation_promotes_updates_and_reloads_repo_concepts() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let promoted = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: Some("custom_validation".to_string()),
                summary: Some("Custom curated validation concept.".to_string()),
                aliases: Some(vec!["validation".to_string(), "custom checks".to_string()]),
                core_members: Some(vec![
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::validation_recipe".to_string(),
                        kind: "function".to_string(),
                    },
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::runtime_status".to_string(),
                        kind: "function".to_string(),
                    },
                ]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Promoted from live repo work.".to_string()]),
                risk_hint: Some(SparsePatchInput::Value("Keep checks aligned.".to_string())),
                confidence: Some(0.91),
                decode_lenses: Some(vec![
                    PrismConceptLensInput::Validation,
                    PrismConceptLensInput::Memory,
                ]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://legacy_validation".to_string()]),
                retirement_reason: None,
                task_id: Some("task:concept-promote".to_string()),
            },
        )
        .expect("concept promote should succeed");

    assert!(promoted.event_id.starts_with("concept-event:"));
    assert_eq!(promoted.concept_handle, "concept://custom_validation");
    assert_eq!(
        promoted.packet.summary,
        "Custom curated validation concept."
    );
    assert_eq!(promoted.packet.provenance.origin, "repo_mutation");
    assert_eq!(
        promoted
            .packet
            .publication
            .as_ref()
            .map(|value| value.status),
        Some(prism_js::ConceptPublicationStatusView::Active)
    );
    assert_eq!(
        promoted
            .packet
            .publication
            .as_ref()
            .map(|value| value.supersedes.clone()),
        Some(vec!["concept://legacy_validation".to_string()])
    );

    let updated = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Update,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: Some("Updated curated validation concept.".to_string()),
                aliases: Some(vec!["validation".to_string(), "updated checks".to_string()]),
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Updated after more repo work.".to_string()]),
                risk_hint: Some(SparsePatchInput::Value(
                    "Config drift is common.".to_string(),
                )),
                confidence: Some(0.95),
                decode_lenses: Some(vec![
                    PrismConceptLensInput::Open,
                    PrismConceptLensInput::Validation,
                ]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://older_validation_flow".to_string()]),
                retirement_reason: None,
                task_id: Some("task:concept-update".to_string()),
            },
        )
        .expect("concept update should succeed");

    assert_eq!(
        updated.packet.summary,
        "Updated curated validation concept."
    );
    assert_eq!(updated.packet.aliases[1], "updated checks");
    assert_eq!(
        updated
            .packet
            .publication
            .as_ref()
            .map(|value| value.supersedes.clone()),
        Some(vec!["concept://older_validation_flow".to_string()])
    );

    let cleared = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Update,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: None,
                aliases: None,
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: None,
                risk_hint: Some(SparsePatchInput::Patch(SparsePatchObjectInput {
                    op: SparsePatchOpInput::Clear,
                    value: None,
                })),
                confidence: None,
                decode_lenses: None,
                scope: None,
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:concept-clear-risk-hint".to_string()),
            },
        )
        .expect("concept riskHint clear should succeed");
    assert_eq!(cleared.packet.risk_hint, None);

    let retired = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Retire,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: None,
                aliases: None,
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: None,
                risk_hint: None,
                confidence: None,
                decode_lenses: None,
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://validation_pipeline".to_string()]),
                retirement_reason: Some(
                    "Replaced by the canonical validation pipeline concept.".to_string(),
                ),
                task_id: Some("task:concept-retire".to_string()),
            },
        )
        .expect("concept retire should succeed");
    assert_eq!(
        retired
            .packet
            .publication
            .as_ref()
            .map(|value| value.status),
        Some(prism_js::ConceptPublicationStatusView::Retired)
    );

    let queried = host
        .execute(
            test_session(&host),
            r#"
return prism.conceptByHandle("concept://custom_validation");
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");
    assert_eq!(queried.result, Value::Null);

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let persisted = reloaded
        .execute(
            test_session(&reloaded),
            r#"
return prism.conceptByHandle("concept://custom_validation");
"#,
            QueryLanguage::Ts,
        )
        .expect("reloaded concept query should succeed");
    assert_eq!(persisted.result, Value::Null);
}

#[test]
fn concept_mutation_rejects_weak_repo_concepts() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let error = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://weak_validation".to_string()),
                canonical_name: Some("weak_validation".to_string()),
                summary: Some("Too weak".to_string()),
                aliases: Some(vec!["validation".to_string()]),
                core_members: Some(vec![NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                }]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["thin".to_string()]),
                risk_hint: None,
                confidence: Some(0.55),
                decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:weak-concept".to_string()),
            },
        )
        .expect_err("weak repo concept should be rejected");

    assert!(error
        .to_string()
        .contains("concept coreMembers must contain at least 2"));
}

#[test]
fn concept_mutation_persists_session_scope_but_not_local_scope() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let session_concept = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://workspace_validation".to_string()),
                canonical_name: Some("workspace_validation".to_string()),
                summary: Some("Workspace-scoped validation concept for local reuse.".to_string()),
                aliases: Some(vec![
                    "validation".to_string(),
                    "workspace checks".to_string(),
                ]),
                core_members: Some(vec![
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::validation_recipe".to_string(),
                        kind: "function".to_string(),
                    },
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::runtime_status".to_string(),
                        kind: "function".to_string(),
                    },
                ]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Promoted for workspace reuse.".to_string()]),
                risk_hint: None,
                confidence: Some(0.86),
                decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
                scope: Some(ConceptScopeInput::Session),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:session-concept".to_string()),
            },
        )
        .expect("session concept should store");
    assert_eq!(
        session_concept.packet.scope,
        prism_js::ConceptScopeView::Session
    );
    assert!(session_concept.packet.publication.is_none());

    let local_concept = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://local_validation_probe".to_string()),
                canonical_name: Some("local_validation_probe".to_string()),
                summary: Some(
                    "Runtime-only validation cluster for the current debugging pass.".to_string(),
                ),
                aliases: Some(vec!["validation probe".to_string()]),
                core_members: Some(vec![NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                }]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Temporary local concept.".to_string()]),
                risk_hint: None,
                confidence: Some(0.6),
                decode_lenses: Some(vec![PrismConceptLensInput::Open]),
                scope: Some(ConceptScopeInput::Local),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:local-concept".to_string()),
            },
        )
        .expect("local concept should store");
    assert_eq!(
        local_concept.packet.scope,
        prism_js::ConceptScopeView::Local
    );

    let visible_now = host
        .execute(
            test_session(&host),
            r#"
return {
  session: prism.conceptByHandle("concept://workspace_validation"),
  local: prism.conceptByHandle("concept://local_validation_probe"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");
    assert_eq!(visible_now.result["session"]["scope"], "session");
    assert_eq!(visible_now.result["local"]["scope"], "local");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let persisted = reloaded
        .execute(
            test_session(&reloaded),
            r#"
return {
  session: prism.conceptByHandle("concept://workspace_validation"),
  local: prism.conceptByHandle("concept://local_validation_probe"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("reloaded concept query should succeed");
    assert_eq!(persisted.result["session"]["scope"], "session");
    assert_eq!(persisted.result["local"], Value::Null);
}

#[test]
fn session_memory_persists_locally_while_local_memory_does_not_reload() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "semantic",
                "scope": "session",
                "content": "alpha keeps a workspace-scoped validation hint",
                "trust": 0.8
            }),
            task_id: Some("task:session-memory".to_string()),
        },
    )
    .expect("session memory should persist");

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "episodic",
                "scope": "local",
                "content": "temporary alpha debugging note that should stay runtime-only",
                "trust": 0.6
            }),
            task_id: Some("task:local-memory".to_string()),
        },
    )
    .expect("local memory should store");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let recalled = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  limit: 10,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    let rendered = recalled.result.to_string();
    assert!(rendered.contains("workspace-scoped validation hint"));
    assert!(!rendered.contains("runtime-only"));

    let events = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  limit: 10,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory events query should succeed");
    let event_json = events.result.to_string();
    assert!(event_json.contains("\"scope\":\"Session\""));
    assert!(!event_json.contains("\"scope\":\"Local\""));
}

#[test]
fn compact_gather_returns_multiple_exact_slices() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": 1}
    print("prism_compact_tool_calls")
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                limit: Some(3),
            },
        )
        .expect("gather should succeed");

    assert_eq!(gather.matches.len(), 3);
    assert!(!gather.truncated);
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.text.contains("prism_compact_tool_calls")));
    assert!(gather.matches.iter().all(|matched| {
        matched
            .next_action
            .as_deref()
            .is_some_and(|next| next.contains("prism_gather"))
    }));
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.promoted_handle.is_none()));
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.suggested_actions.iter().any(|action| {
            action.tool == "prism_workset"
                && action.handle.as_deref() == Some(matched.handle.as_str())
        })));
}

#[test]
fn compact_locate_promotes_numbered_markdown_headings_to_semantic_handles() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Memory\n\n## 9.10 Integration Points\n\nPRISM should enrich memory recall with lineage and prior outcomes.\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn memory_recall() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);
    assert_eq!(
        locate
            .top_preview
            .as_ref()
            .map(|preview| preview.start_line),
        Some(3)
    );
}

#[test]
fn compact_fragment_followups_surface_semantic_config_targets() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("crates/member-a")).unwrap();
    fs::create_dir_all(root.join("crates/member-b")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/prism\"]\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/member-a/Cargo.toml"),
        "[package]\nname = \"member-a\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/member-b/Cargo.toml"),
        "[package]\nname = \"member-b\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1.0\"\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| !targets.is_empty()));
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().all(|target| {
            target.kind == NodeKind::TomlKey
                && !target.path.contains("member_a")
                && !target.path.contains("member_b")
                && !target.path.contains("crates/")
                && target
                    .file_path
                    .as_deref()
                    .is_none_or(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets
            .iter()
            .any(|target| target.path.contains("::workspace"))));
    assert!(gather.matches[0]
        .next_action
        .as_deref()
        .is_some_and(|next| next.contains("strongest semantic related handle")));
    assert!(gather.matches[0]
        .next_action
        .as_deref()
        .is_some_and(|next| next.contains("prism_open on it")));
    let promoted_handle = gather.matches[0]
        .promoted_handle
        .as_ref()
        .expect("semantic gather should lift a promoted handle");
    assert_eq!(promoted_handle.kind, NodeKind::TomlKey);
    assert!(promoted_handle.path.contains("::workspace::dependencies"));
    assert!(gather.matches[0].suggested_actions.iter().any(|action| {
        action.tool == "prism_workset"
            && action.handle.as_deref() == Some(promoted_handle.handle.as_str())
    }));
    assert!(gather.matches[0].suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(promoted_handle.handle.as_str())
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    let handle = gather.matches[0].handle.clone();

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(!workset.supporting_reads.is_empty());
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.kind == NodeKind::TomlKey));
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("::workspace")));
    assert!(workset.supporting_reads.iter().all(|target| {
        !target.path.contains("member_a")
            && !target.path.contains("member_b")
            && !target.path.contains("crates/")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path.ends_with("Cargo.toml"))
    }));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let neighbors = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("neighbors should succeed");
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "TomlKey")));
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| item["filePath"] == "Cargo.toml")));
    assert!(neighbors.top_preview.is_some());
    let first_neighbor_handle = neighbors.result["neighbors"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["handle"].as_str())
        .expect("neighbors should include a top handle");
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(first_neighbor_handle)
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation should succeed");
    assert!(validation.result["checks"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    let first_next_read = validation.result["nextReads"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["handle"].as_str())
        .expect("validation should include nextReads");
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(first_next_read)
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Neighbors)
    }));
}

#[test]
fn compact_gather_prefers_root_workspace_dependencies_with_many_child_manifests() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/member-0\"]\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    for index in 0..12 {
        let crate_dir = root.join(format!("crates/member-{index}"));
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"member-{index}\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n"
            ),
        )
        .unwrap();
    }

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");

    let related = gather.matches[0]
        .related_handles
        .as_ref()
        .expect("related handles");
    assert!(related.iter().any(|target| {
        target.kind == NodeKind::TomlKey
            && target.path.contains("::workspace::dependencies")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path == "Cargo.toml")
    }));
    assert!(related.iter().all(|target| {
        !target.path.contains("member_")
            && !target.path.contains("member-")
            && !target.path.contains("crates/")
    }));
}

#[test]
fn compact_structured_config_handles_prefer_same_file_family_over_tests() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/member-0\"]\nresolver = \"2\"\n\n[workspace.package]\nedition = \"2021\"\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    for index in 0..4 {
        let crate_dir = root.join(format!("crates/member-{index}"));
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"member-{index}\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n"
            ),
        )
        .unwrap();
    }

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");
    let semantic_handle = gather.matches[0]
        .related_handles
        .as_ref()
        .and_then(|targets| {
            targets
                .iter()
                .find(|target| target.path.contains("::workspace::dependencies"))
        })
        .map(|target| target.handle.clone())
        .expect("semantic handle");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(semantic_handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.supporting_reads.iter().all(|target| {
        target.kind == NodeKind::TomlKey
            && !target.path.contains("tests::")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path.ends_with("Cargo.toml"))
    }));
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("::workspace")));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("validation")));

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(semantic_handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| !targets.is_empty()));
    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().all(|target| {
            target.kind == NodeKind::TomlKey
                && !target.path.contains("tests::")
                && target
                    .file_path
                    .as_deref()
                    .is_none_or(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(open.text.contains("[workspace]"));
    assert!(open.text.contains("[workspace.dependencies]"));
    assert!(open.text.contains("anyhow = \"1.0\""));
    assert!(open.text.contains("serde = \"1.0\""));
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("validation") && text.contains("neighbors")));
    assert!(open.promoted_handle.is_none());
    assert!(open.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(open.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let neighbors = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: semantic_handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("neighbors should succeed");
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            item["kind"] == "TomlKey"
                && item["path"]
                    .as_str()
                    .is_some_and(|path| !path.contains("tests::"))
                && item["filePath"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("Cargo.toml"))
        })));
    let preview = neighbors
        .top_preview
        .expect("structured config neighbors should include a top preview");
    let first_neighbor = neighbors.result["neighbors"]
        .as_array()
        .and_then(|items| items.first())
        .expect("neighbors should contain at least one item");
    assert_eq!(
        preview.handle,
        first_neighbor["handle"].as_str().unwrap_or_default()
    );
    assert!(preview.text.contains("[workspace]"));
    assert!(preview.text.contains("[workspace.dependencies]"));
    assert!(neighbors
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("validation")));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: semantic_handle.clone(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation should succeed");
    assert!(validation.result["nextReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(validation.result["likelyTests"]
        .as_array()
        .is_some_and(|items| items.is_empty()));
    assert!(validation.result["checks"]
        .as_array()
        .is_some_and(|items| items.len() >= 2));
    assert!(validation.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            item["kind"] == "TomlKey"
                && item["path"]
                    .as_str()
                    .is_some_and(|path| !path.contains("tests::"))
                && item["filePath"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(validation
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("neighbors")));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Neighbors)
    }));
}

#[test]
fn compact_workset_query_prefers_strong_concept_resolution_for_broad_subsystems() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn session_memory() {}
pub fn outcome_memory() {}
pub fn memory_system_test() {}
"#,
    )
    .unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://memory_system".to_string()),
            canonical_name: Some("memory system".to_string()),
            summary: Some(
                "Session memory recall and outcome history form the repo memory subsystem."
                    .to_string(),
            ),
            aliases: Some(vec![
                "memory layer".to_string(),
                "recall system".to_string(),
            ]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::session_memory".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::outcome_memory".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::memory_system_test".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec![
                "Promoted from repeated memory-system dogfooding.".to_string()
            ]),
            risk_hint: None,
            decode_lenses: None,
            scope: None,
            confidence: Some(0.94),
            task_id: None,
            supersedes: None,
            retirement_reason: None,
        },
    )
    .expect("concept store should succeed");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: None,
                query: Some("memory system".to_string()),
            },
        )
        .expect("workset should succeed");

    assert_eq!(workset.primary.path, "demo::session_memory");
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path == "demo::outcome_memory"));
    assert!(workset.remapped);
    assert!(workset
        .why
        .contains("Session memory recall and outcome history"));
}

#[test]
fn compact_expand_drift_surfaces_spec_gap_summary() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Drift);
    let drift_reasons = expand.result["driftReasons"]
        .as_array()
        .expect("driftReasons should be an array");
    assert!(!drift_reasons.is_empty());
    let next_reads = expand.result["nextReads"]
        .as_array()
        .expect("nextReads should be an array");
    assert!(!next_reads.is_empty());
    assert!(next_reads.iter().all(|item| item["filePath"].is_null()));
    assert!(matches!(
        expand.result["confidence"].as_str(),
        Some("medium" | "high")
    ));
}

#[test]
fn compact_expand_neighbors_can_include_top_preview() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Neighbors);
    let neighbors = expand.result["neighbors"]
        .as_array()
        .expect("neighbors should be an array");
    assert!(!neighbors.is_empty());
    let preview = expand
        .top_preview
        .expect("neighbors expand should include top preview");
    assert_eq!(
        preview.handle,
        neighbors[0]["handle"].as_str().unwrap_or_default()
    );
    assert!(!preview.text.is_empty());
}

#[test]
fn compact_tool_query_trace_records_refresh_and_handler_phases() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.compact_locate(
        Arc::clone(&session),
        PrismLocateArgs {
            query: "Integration Points".to_string(),
            path: Some("docs/SPEC.md".to_string()),
            glob: None,
            task_intent: Some(PrismLocateTaskIntentInput::Explain),
            limit: Some(3),
            include_top_preview: None,
        },
    )
    .expect("locate should succeed");

    let recent = host.query_log_entries(QueryLogArgs {
        limit: Some(5),
        since: None,
        target: None,
        operation: None,
        task_id: None,
        min_duration_ms: None,
    });
    let entry = recent
        .iter()
        .find(|entry| entry.kind == "prism_locate")
        .expect("compact locate query log entry");
    let trace = host
        .query_trace_view(&entry.id)
        .expect("compact locate query trace");
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"compact.refreshWorkspace"));
    assert!(operations.contains(&"compact.handler"));
    assert!(trace
        .phases
        .iter()
        .find(|phase| phase.operation == "compact.handler")
        .is_some_and(|phase| phase.success));
}

#[test]
fn compact_expand_lineage_returns_compact_recent_history() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    fs::write(&source_path, "pub fn latest_name() {}\n").unwrap();

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let current_id = NodeId::new("demo", "demo::latest_name", NodeKind::Function);
    graph.add_node(Node {
        id: current_id.clone(),
        name: "latest_name".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    let rename_chain = [
        "demo::old_name",
        "demo::legacy_name",
        "demo::older_name",
        "demo::previous_name",
        "demo::latest_name",
    ];
    history.seed_nodes([NodeId::new("demo", rename_chain[0], NodeKind::Function)]);
    for (index, names) in rename_chain.windows(2).enumerate() {
        history.apply(&ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new(format!("change:rename:{index}")),
                ts: (index + 1) as u64,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![file_id],
            previous_path: Some(source_path.to_string_lossy().into_owned().into()),
            current_path: Some(source_path.to_string_lossy().into_owned().into()),
            added: vec![ObservedNode {
                node: Node {
                    id: NodeId::new("demo", names[1], NodeKind::Function),
                    name: names[1].rsplit("::").next().unwrap().into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
            }],
            removed: vec![ObservedNode {
                node: Node {
                    id: NodeId::new("demo", names[0], NodeKind::Function),
                    name: names[0].rsplit("::").next().unwrap().into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
            }],
            updated: Vec::new(),
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        });
    }

    let host = host_with_prism(Prism::with_history(graph, history));
    let session = test_session(&host);
    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: current_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&current_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "latest_name".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("latest_name".into()),
        why_short: "exact symbol".into(),
        start_line: Some(1),
        end_line: Some(1),
        start_column: None,
        end_column: None,
    });

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Lineage,
                include_top_preview: None,
            },
        )
        .expect("lineage expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Lineage);
    assert_eq!(expand.result["currentPath"], "demo::latest_name");
    assert_eq!(expand.result["status"], "active");
    assert!(expand.result.get("history").is_none());
    let recent_history = expand.result["recentHistory"]
        .as_array()
        .expect("recentHistory should be an array");
    assert_eq!(recent_history.len(), 3);
    assert!(expand.result["truncated"].as_bool().unwrap_or(false));
    assert!(expand.result["nextAction"]
        .as_str()
        .is_some_and(|text| text.contains("prism_query")));
    assert!(recent_history.iter().all(|event| {
        event.get("before").is_none()
            && event.get("after").is_none()
            && event.get("evidenceDetails").is_none()
            && event["summary"].as_str().is_some()
            && event["evidence"]
                .as_array()
                .is_some_and(|evidence| evidence.len() <= 2)
    }));
}

#[test]
fn compact_expand_diff_returns_compact_recent_patch_summaries() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn alpha() {}\n";
    fs::write(&source_path, source).unwrap();
    let alpha_span = {
        let start = source.find("alpha").expect("alpha span");
        Span::new(start, start + "alpha".len())
    };

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: alpha_span,
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone()]);

    let outcomes = OutcomeMemory::new();
    for index in 0..4 {
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(format!("outcome:patch:{index}")),
                    ts: (index + 1) as u64,
                    actor: EventActor::System,
                    correlation: None,
                    causation: None,
                },
                anchors: vec![AnchorRef::File(file_id), AnchorRef::Node(alpha_id.clone())],
                kind: OutcomeKind::PatchApplied,
                result: OutcomeResult::Success,
                summary: format!("patched alpha {index}"),
                evidence: Vec::new(),
                metadata: json!({
                    "trigger": "ManualReindex",
                    "filePaths": [source_path.to_string_lossy().into_owned()],
                    "changedSymbols": [
                        {
                            "status": "updated_after",
                            "id": alpha_id,
                            "name": "alpha",
                            "kind": NodeKind::Function,
                            "filePath": source_path.to_string_lossy().into_owned(),
                            "span": alpha_span,
                        }
                    ],
                }),
            })
            .unwrap();
    }

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let session = test_session(&host);
    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: alpha_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&alpha_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "alpha".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("alpha".into()),
        why_short: "exact symbol".into(),
        start_line: Some(1),
        end_line: Some(1),
        start_column: None,
        end_column: None,
    });

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Diff,
                include_top_preview: None,
            },
        )
        .expect("diff expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Diff);
    assert!(expand.result.get("diff").is_none());
    let recent_diffs = expand.result["recentDiffs"]
        .as_array()
        .expect("recentDiffs should be an array");
    assert_eq!(recent_diffs.len(), 3);
    assert!(expand.result["truncated"].as_bool().unwrap_or(false));
    assert!(expand.result["nextAction"]
        .as_str()
        .is_some_and(|text| text.contains("prism_query")));
    assert!(recent_diffs.iter().all(|diff| {
        diff["symbolPath"] == "demo::alpha"
            && diff["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("patched alpha"))
            && diff["filePath"]
                .as_str()
                .is_some_and(|path| path.ends_with("src/lib.rs"))
    }));
}

#[test]
fn compact_expand_perception_lenses_surface_impact_timeline_and_memory() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn beta() {}\n\npub fn alpha() { beta(); }\n";
    fs::write(&source_path, source).unwrap();
    let test_path = root.join("tests/alpha.rs");
    fs::write(&test_path, "#[test]\nfn alpha_test() { super::alpha(); }\n").unwrap();

    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };
    let alpha_start = source.rfind("alpha").expect("alpha span");
    let alpha_span = Span::new(alpha_start, alpha_start + "alpha".len());

    let mut graph = Graph::new();
    let source_file = graph.ensure_file(&source_path);
    let test_file = graph.ensure_file(&test_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta_id = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_test_id = NodeId::new("demo", "demo::alpha_test", NodeKind::Function);
    graph.add_node(Node {
        id: beta_id.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: beta_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: alpha_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_test_id.clone(),
        name: "alpha_test".into(),
        kind: NodeKind::Function,
        file: test_file,
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha_id.clone(),
        target: beta_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Validates,
        source: alpha_test_id.clone(),
        target: alpha_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone(), beta_id.clone(), alpha_test_id.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:failure"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha regression".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:patch"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![
                AnchorRef::File(source_file),
                AnchorRef::Node(alpha_id.clone()),
            ],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "patched alpha".into(),
            evidence: Vec::new(),
            metadata: json!({
                "trigger": "ManualReindex",
                "filePaths": [source_path.to_string_lossy().into_owned()],
                "changedSymbols": [{
                    "status": "updated_after",
                    "id": alpha_id.clone(),
                    "name": "alpha",
                    "kind": NodeKind::Function,
                    "filePath": source_path.to_string_lossy().into_owned(),
                    "span": alpha_span,
                }],
            }),
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:validated"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let session = test_session(&host);
    let mut memory = MemoryEntry::new(
        MemoryKind::Structural,
        "alpha edits usually require checking beta and alpha_test together",
    );
    memory.anchors = vec![AnchorRef::Node(alpha_id.clone())];
    memory.trust = 0.9;
    session.notes.store(memory).unwrap();

    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: alpha_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&alpha_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "alpha".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("alpha".into()),
        why_short: "exact symbol".into(),
        start_line: Some(3),
        end_line: Some(3),
        start_column: None,
        end_column: None,
    });

    let impact = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Impact,
                include_top_preview: None,
            },
        )
        .expect("impact expand should succeed");
    assert_eq!(impact.kind, prism_js::AgentExpandKind::Impact);
    assert!(impact.result["likelyTouch"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["path"] == "demo::beta")));
    assert!(impact.result["recentFailures"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(impact.result["riskHint"].as_str().is_some());

    let timeline = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Timeline,
                include_top_preview: None,
            },
        )
        .expect("timeline expand should succeed");
    assert_eq!(timeline.kind, prism_js::AgentExpandKind::Timeline);
    assert!(timeline.result["recentEvents"]
        .as_array()
        .is_some_and(|items| items.len() >= 2));
    assert!(timeline.result["recentPatches"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert_eq!(
        timeline.result["lastFailure"]["summary"],
        "alpha regression"
    );
    assert_eq!(
        timeline.result["lastValidation"]["summary"],
        "validated alpha"
    );

    let memory_expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Memory,
                include_top_preview: None,
            },
        )
        .expect("memory expand should succeed");
    assert_eq!(memory_expand.kind, prism_js::AgentExpandKind::Memory);
    assert!(memory_expand.result["memories"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["summary"]
            == "alpha edits usually require checking beta and alpha_test together")));
}

#[test]
fn compact_task_brief_summarizes_coordination_outcomes_and_next_reads() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn gamma() {}\n\npub fn beta() {}\n\npub fn alpha() { beta(); }\n";
    fs::write(&source_path, source).unwrap();
    let gamma_span = {
        let start = source.find("gamma").expect("gamma span");
        Span::new(start, start + "gamma".len())
    };
    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };
    let alpha_start = source.rfind("alpha").expect("alpha span");
    let alpha_span = Span::new(alpha_start, alpha_start + "alpha".len());

    let mut graph = Graph::new();
    let source_file = graph.ensure_file(&source_path);
    let gamma_id = NodeId::new("demo", "demo::gamma", NodeKind::Function);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta_id = NodeId::new("demo", "demo::beta", NodeKind::Function);
    graph.add_node(Node {
        id: gamma_id.clone(),
        name: "gamma".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: gamma_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta_id.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: beta_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: alpha_span,
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha_id.clone(),
        target: beta_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone(), beta_id.clone(), gamma_id.clone()]);
    let host = host_with_prism(Prism::with_history(graph, history));

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Coordinate alpha",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Review gamma",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::gamma",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "dependsOn": [dependency_id]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    host.store_claim(
        test_session(&host).as_ref(),
        PrismClaimArgs {
            action: ClaimActionInput::Acquire,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "capability": "Edit",
                "mode": "SoftExclusive",
                "coordinationTaskId": task_id.clone()
            }),
            task_id: None,
        },
    )
    .unwrap();
    host.store_artifact(
        test_session(&host).as_ref(),
        PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task_id.clone(),
                "diffRef": "patch:alpha"
            }),
            task_id: None,
        },
    )
    .unwrap();
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated alpha".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: Some(task_id.clone()),
        },
    )
    .unwrap();

    let brief = host
        .compact_task_brief(
            test_session(&host),
            PrismTaskBriefArgs {
                task_id: task_id.clone(),
            },
        )
        .expect("task brief should succeed");

    assert_eq!(brief.task_id, task_id);
    assert_eq!(brief.title, "Edit alpha");
    assert!(!brief.blockers.is_empty());
    assert_eq!(brief.claim_holders.len(), 1);
    assert!(brief
        .recent_outcomes
        .iter()
        .any(|event| event.summary == "validated alpha"));
    assert!(brief
        .next_reads
        .iter()
        .any(|target| target.path == "demo::beta"));
    assert!(brief
        .next_reads
        .iter()
        .any(|target| target.path == "demo::gamma"));
    assert_eq!(brief.next_reads[0].path, "demo::gamma");
    assert!(brief.next_action.as_deref().is_some_and(|value| {
        value.contains("current task blockers") && value.contains("prism.blockers(taskId)")
    }));
}

#[test]
fn compact_task_brief_prefers_refresh_for_stale_current_task() {
    let mut graph = Graph::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:task-brief-stale"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![(
            ObservedNode {
                node: Node {
                    id: alpha.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
            ObservedNode {
                node: Node {
                    id: alpha.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:task-brief-stale"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Refresh stale task".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:task-brief-stale"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    prism.replace_coordination_snapshot(coordination.snapshot());
    let host = host_with_prism(prism);

    let brief = host
        .compact_task_brief(
            test_session(&host),
            PrismTaskBriefArgs {
                task_id: task_id.0.to_string(),
            },
        )
        .expect("task brief should succeed");

    assert!(brief
        .blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision));
    assert!(brief
        .next_action
        .as_deref()
        .is_some_and(|value| value.contains("Refresh this task")));
}

#[test]
fn compact_workset_for_spec_targets_surfaces_drift_reads_and_gap_summary() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.supporting_reads.iter().any(|target| {
        target.path.contains("memory_recall")
            || target.path.contains("reanchor_persisted_memory_snapshot")
    }));
    assert!(workset.why.contains("Gap summary:") || workset.why.contains("gap summary"));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("drift")));
}

#[test]
fn compact_workset_for_spec_targets_prefers_owner_paths_over_text_adjacent_helpers() {
    let root = temp_workspace();
    write_dashboard_validation_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Validation view".to_string(),
                path: Some("docs/DASHBOARD_IMPLEMENTATION_SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("validation_feedback_view")
            || target.path.contains("store_validation_feedback")));
    assert!(workset.supporting_reads.iter().all(|target| {
        !target
            .path
            .contains("strip_internal_developer_api_reference")
    }));

    let drift = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("drift should succeed");
    assert!(drift.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            !item["path"]
                .as_str()
                .unwrap_or_default()
                .contains("strip_internal_developer_api_reference")
        })));
    assert!(drift
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("prism_workset")));
}

#[test]
fn compact_workset_for_product_surface_spec_headings_lifts_body_identifiers() {
    let root = temp_workspace();
    write_compact_default_tools_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Compact Default Tools".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(workset.supporting_reads.iter().any(|target| {
        target.path.contains("prism_locate")
            || target.path.contains("prism_open")
            || target.path.contains("prism_workset")
            || target.path.contains("prism_expand")
    }));
    assert!(workset
        .supporting_reads
        .iter()
        .all(|target| !target.path.contains("tests::")));

    let drift = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("drift should succeed");
    assert!(drift.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            let path = item["path"].as_str().unwrap_or_default();
            path.contains("prism_locate")
                || path.contains("prism_open")
                || path.contains("prism_workset")
                || path.contains("prism_expand")
        })));
}

#[test]
fn compact_open_for_product_surface_spec_headings_prefers_identifier_owners() {
    let root = temp_workspace();
    write_compact_default_tools_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Compact Default Tools".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");

    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().any(|target| {
            target.path.contains("prism_locate")
                || target.path.contains("prism_open")
                || target.path.contains("prism_workset")
                || target.path.contains("prism_expand")
        })));
    assert!(open.related_handles.as_ref().is_some_and(|targets| {
        targets
            .iter()
            .all(|target| !target.path.contains("tests::"))
    }));
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset") && text.contains("drift")));
}

#[tokio::test]
async fn mcp_server_executes_compact_agent_tool_round_trip() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn main() {
    println!("hello");
}
"#,
    )
    .unwrap();
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_locate",
            json!({
                "query": "main",
                "taskIntent": "edit",
                "limit": 3,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let locate = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(locate["status"], "ok");

    client
        .send(call_tool_request(
            3,
            "prism_open",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "mode": "focus",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let open = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(open["handle"], locate["candidates"][0]["handle"]);
    assert!(open["text"]
        .as_str()
        .expect("open text should be a string")
        .contains("fn main"));
    assert!(open["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            4,
            "prism_workset",
            json!({
                "handle": locate["candidates"][0]["handle"],
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let workset = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        workset["primary"]["handle"],
        locate["candidates"][0]["handle"]
    );
    assert!(workset["why"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(workset["truncated"], false);
    assert!(workset["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism_open")));
    assert!(workset["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            5,
            "prism_expand",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "kind": "diagnostics",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let expand = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(expand["kind"], "diagnostics");
    assert_eq!(
        expand["result"]["whyShort"],
        locate["candidates"][0]["whyShort"]
    );
    assert!(expand["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            6,
            "prism_gather",
            json!({
                "query": "println!(\"hello\")",
                "path": "src/lib.rs",
                "limit": 2,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let gather = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(gather["truncated"], false);
    assert_eq!(gather["matches"].as_array().map(Vec::len), Some(1));
    assert_eq!(gather["matches"][0]["filePath"], "src/lib.rs");
    assert!(gather["matches"][0]["text"]
        .as_str()
        .is_some_and(|value| value.contains("println!(\"hello\")")));
    assert!(gather["matches"][0]["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_prism_task_brief_round_trip() {
    let server = server_with_node(demo_node());
    let plan = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Coordinate main" }),
                task_id: None,
            },
        )
        .expect("plan create should succeed");
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let task = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Inspect main",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .expect("task create should succeed");
    let task_id = task.state["id"].as_str().unwrap().to_string();
    server
        .host
        .store_outcome(
            test_session(&server.host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                summary: "validated main".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: Some(task_id.clone()),
            },
        )
        .expect("outcome should store");

    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_task_brief",
            json!({
                "taskId": task_id,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let brief = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(brief["title"], "Inspect main");
    assert!(brief["recentOutcomes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["summary"] == "validated main")));
    assert!(brief["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism_open")));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_keeps_compact_handles_stable_across_parallel_follow_up_calls() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_locate",
            json!({
                "query": "Integration Points",
                "path": "docs/SPEC.md",
                "taskIntent": "explain",
                "limit": 3,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let locate = first_tool_content_json(client.receive().await.unwrap());
    let handle = locate["candidates"][0]["handle"].clone();

    for (id, tool, args) in [
        (
            3,
            "prism_open",
            json!({
                "handle": handle,
                "mode": "focus",
            }),
        ),
        (
            4,
            "prism_workset",
            json!({
                "handle": locate["candidates"][0]["handle"],
            }),
        ),
        (
            5,
            "prism_expand",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "kind": "drift",
            }),
        ),
    ] {
        client
            .send(call_tool_request(
                id,
                tool,
                args.as_object()
                    .expect("tool args should be an object")
                    .clone(),
            ))
            .await
            .unwrap();
    }

    let first = first_tool_content_json(client.receive().await.unwrap());
    let second = first_tool_content_json(client.receive().await.unwrap());
    let third = first_tool_content_json(client.receive().await.unwrap());
    let payloads = [first, second, third];

    assert!(payloads
        .iter()
        .any(|payload| payload["handle"] == locate["candidates"][0]["handle"]));
    assert!(payloads
        .iter()
        .any(|payload| payload["primary"]["handle"] == locate["candidates"][0]["handle"]));
    assert!(payloads
        .iter()
        .any(|payload| payload["kind"] == "drift" && payload["result"]["nextReads"].is_array()));

    running.cancel().await.unwrap();
}

#[test]
fn bundle_helpers_collapse_search_and_target_context_into_one_query() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const symbol = prism.symbolBundle("helper", { includeDiscovery: true });
const search = prism.searchBundle("helper", { limit: 5 });
const target = prism.targetBundle(search);
const richSearch = prism.searchBundle("helper", { limit: 5, includeDiscovery: true });
const richTarget = prism.targetBundle(richSearch, { includeDiscovery: true });
return {
  symbol: {
    query: symbol?.query ?? null,
    resultPath: symbol?.result?.id?.path ?? null,
    candidateCount: symbol?.candidates?.length ?? 0,
    discoveryPath: symbol?.discovery?.target?.id?.path ?? null,
    readContextPath: symbol?.readContext?.target?.id?.path ?? null,
    suggestedReadsCount: symbol?.suggestedReads?.length ?? 0,
    summary: symbol?.summary ?? null,
  },
  search: {
    query: search.query,
    resultCount: search.results.length,
    topResultPath: search.topResult?.id?.path ?? null,
    focusedPath: search.focusedBlock?.symbol?.id?.path ?? null,
    readContextPath: search.readContext?.target?.id?.path ?? null,
    validationPath: search.validationContext?.target?.id?.path ?? null,
    recentChangePath: search.recentChangeContext?.target?.id?.path ?? null,
    hasDiscovery: search.discovery != null,
    suggestedReadsCount: search.suggestedReads.length,
    summary: search.summary,
    diagnosticCodes: search.diagnostics.map((diagnostic) => diagnostic.code),
  },
  target: target == null ? null : {
    targetPath: target.target.id.path,
    editContextPath: target.editContext.target.id.path,
    readContextPath: target.readContext.target.id.path,
    hasDiscovery: target.discovery != null,
    suggestedReadsCount: target.suggestedReads.length,
    diffCount: target.diff.length,
    likelyTestsCount: target.likelyTests.length,
    summary: target.summary,
  },
  richSearch: {
    topResultPath: richSearch.topResult?.id?.path ?? null,
    discoveryPath: richSearch.discovery?.target?.id?.path ?? null,
    readContextPath: richSearch.readContext?.target?.id?.path ?? null,
    suggestedReadsCount: richSearch.suggestedReads.length,
    summary: richSearch.summary,
  },
  richTarget: richTarget == null ? null : {
    targetPath: richTarget.target.id.path,
    discoveryPath: richTarget.discovery?.target?.id?.path ?? null,
    editContextPath: richTarget.editContext.target.id.path,
    readContextPath: richTarget.readContext.target.id.path,
    suggestedReadsCount: richTarget.suggestedReads.length,
    summary: richTarget.summary,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("bundle helpers query should succeed");

    let symbol = &result.result["symbol"];
    assert_eq!(symbol["query"], "helper");
    assert_eq!(symbol["resultPath"], "demo::alpha::helper");
    assert_eq!(symbol["candidateCount"], 2);
    assert_eq!(symbol["discoveryPath"], symbol["resultPath"]);
    assert_eq!(symbol["readContextPath"], symbol["resultPath"]);
    assert!(symbol["suggestedReadsCount"].as_u64().unwrap_or_default() > 0);
    assert_eq!(symbol["summary"]["kind"], "symbol");
    assert_eq!(symbol["summary"]["resultCount"], 2);
    assert_eq!(symbol["summary"]["empty"], false);
    assert_eq!(symbol["summary"]["ambiguous"], true);

    let search = &result.result["search"];
    assert_eq!(search["query"], "helper");
    assert_eq!(search["resultCount"], 2);
    assert_eq!(search["focusedPath"], search["topResultPath"]);
    assert_eq!(search["readContextPath"], search["topResultPath"]);
    assert_eq!(search["validationPath"], search["topResultPath"]);
    assert_eq!(search["recentChangePath"], search["topResultPath"]);
    assert_eq!(search["hasDiscovery"], false);
    assert!(search["suggestedReadsCount"].as_u64().is_some());
    assert_eq!(search["summary"]["kind"], "search");
    assert_eq!(search["summary"]["resultCount"], 2);
    assert_eq!(search["summary"]["ambiguous"], true);
    assert!(search["diagnosticCodes"]
        .as_array()
        .expect("bundle diagnostics")
        .iter()
        .any(|diagnostic| diagnostic == "ambiguous_search"));

    let target = &result.result["target"];
    assert_eq!(target["targetPath"], search["topResultPath"]);
    assert_eq!(target["hasDiscovery"], false);
    assert_eq!(target["editContextPath"], target["targetPath"]);
    assert_eq!(target["readContextPath"], target["targetPath"]);
    assert!(target["suggestedReadsCount"].as_u64().is_some());
    assert_eq!(target["summary"]["kind"], "target");
    assert_eq!(target["summary"]["resultCount"], 1);
    assert!(target["diffCount"].as_u64().is_some());
    assert!(target["likelyTestsCount"].as_u64().is_some());

    let rich_search = &result.result["richSearch"];
    assert_eq!(rich_search["discoveryPath"], rich_search["topResultPath"]);
    assert_eq!(rich_search["readContextPath"], rich_search["topResultPath"]);
    assert!(
        rich_search["suggestedReadsCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(rich_search["summary"]["kind"], "search");

    let rich_target = &result.result["richTarget"];
    assert_eq!(rich_target["targetPath"], rich_search["topResultPath"]);
    assert_eq!(rich_target["discoveryPath"], rich_target["targetPath"]);
    assert_eq!(rich_target["editContextPath"], rich_target["targetPath"]);
    assert_eq!(rich_target["readContextPath"], rich_target["targetPath"]);
    assert!(
        rich_target["suggestedReadsCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(rich_target["summary"]["kind"], "target");
}

#[test]
fn bundle_helpers_keep_diagnostics_local_to_each_helper_call() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const broad = prism.searchBundle("helper", { limit: 5 });
const exact = prism.symbolBundle("core");
const text = prism.textSearchBundle("core", {
  path: "src/alpha.rs",
  semanticLimit: 2,
  aroundBefore: 1,
  aroundAfter: 2,
});
return {
  broad: {
    diagnosticCodes: broad.diagnostics.map((diagnostic) => diagnostic.code),
    summary: broad.summary,
  },
  exact: {
    resultPath: exact.result?.id?.path ?? null,
    diagnosticCodes: exact.diagnostics.map((diagnostic) => diagnostic.code),
    summary: exact.summary,
  },
  text: {
    topMatchPath: text.topMatch?.path ?? null,
    diagnosticCodes: text.diagnostics.map((diagnostic) => diagnostic.code),
    summary: text.summary,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("bundle-local diagnostics query should succeed");

    let broad = &result.result["broad"];
    assert!(broad["diagnosticCodes"]
        .as_array()
        .expect("broad diagnostics")
        .iter()
        .any(|diagnostic| diagnostic == "ambiguous_search"));
    assert_eq!(broad["summary"]["ambiguous"], true);

    let exact = &result.result["exact"];
    assert_eq!(exact["resultPath"], "demo::alpha::core");
    assert_eq!(exact["summary"]["kind"], "symbol");
    assert_eq!(exact["summary"]["resultCount"], 1);
    assert_eq!(exact["summary"]["ambiguous"], false);
    assert_eq!(exact["summary"]["truncated"], false);
    assert_eq!(exact["diagnosticCodes"], json!([]));
    assert_eq!(exact["summary"]["diagnosticCodes"], json!([]));

    let text = &result.result["text"];
    assert_eq!(text["topMatchPath"], "src/alpha.rs");
    assert_eq!(text["summary"]["kind"], "text_search");
    assert_eq!(text["summary"]["ambiguous"], false);
    assert_eq!(text["summary"]["truncated"], false);
    assert_eq!(text["diagnosticCodes"], json!([]));
    assert_eq!(text["summary"]["diagnosticCodes"], json!([]));
}

#[test]
fn text_search_bundle_collapses_raw_match_and_semantic_context_into_one_query() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const bundle = prism.textSearchBundle("helper", {
  path: "src/alpha.rs",
  semanticLimit: 3,
  aroundBefore: 1,
  aroundAfter: 4,
});
const regexBundle = prism.textSearchBundle("helper\\(", {
  regex: true,
  path: "src/alpha.rs",
  semanticQuery: "helper",
  semanticLimit: 3,
  includeDiscovery: true,
});
return { bundle, regexBundle };
"#,
            QueryLanguage::Ts,
        )
        .expect("text search bundle query should succeed");

    let bundle = &result.result["bundle"];
    assert_eq!(bundle["query"], "helper");
    assert_eq!(bundle["topMatch"]["path"], "src/alpha.rs");
    assert_eq!(
        bundle["rawContext"]["focus"]["startLine"],
        bundle["topMatch"]["location"]["startLine"]
    );
    assert_eq!(bundle["semanticQuery"], "helper");
    assert_eq!(
        bundle["focusedBlock"]["symbol"]["id"]["path"],
        bundle["topSymbol"]["id"]["path"]
    );
    assert_eq!(
        bundle["readContext"]["target"]["id"]["path"],
        bundle["topSymbol"]["id"]["path"]
    );
    assert!(bundle["suggestedReads"].is_array());
    assert_eq!(bundle["summary"]["kind"], "text_search");
    assert_eq!(bundle["summary"]["resultCount"], 1);
    assert!(bundle["discovery"].is_null());
    assert!(bundle["diagnostics"].is_array());

    let regex_bundle = &result.result["regexBundle"];
    assert_eq!(regex_bundle["query"], "helper\\(");
    assert_eq!(regex_bundle["semanticQuery"], "helper");
    assert_eq!(
        regex_bundle["discovery"]["target"]["id"]["path"],
        regex_bundle["topSymbol"]["id"]["path"]
    );
    assert_eq!(
        regex_bundle["readContext"]["target"]["id"]["path"],
        regex_bundle["topSymbol"]["id"]["path"]
    );
    assert!(regex_bundle["suggestedReads"].is_array());
    assert_eq!(regex_bundle["summary"]["kind"], "text_search");
}

#[test]
fn lineage_targets_remap_stale_symbol_ids_to_current_edit_slices() {
    let root = temp_workspace();
    let source = "pub fn alpha_v2() { beta(); }\npub fn beta() {}\n";
    fs::write(root.join("src/lib.rs"), source).unwrap();

    let alpha_old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let alpha_new = NodeId::new("demo", "demo::alpha_v2", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let alpha_start = source.find("pub fn alpha_v2").expect("alpha_v2 source");
    let alpha_end = source.find('\n').expect("alpha_v2 line end");
    let beta_start = source.find("pub fn beta").expect("beta source");
    let beta_end = source[source.len() - 1..]
        .find('\n')
        .map(|index| source.len() - 1 + index)
        .unwrap_or(source.len());

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&root.join("src/lib.rs"));
    graph.add_node(Node {
        id: alpha_new.clone(),
        name: "alpha_v2".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::new(alpha_start, alpha_end),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::new(beta_start, beta_end),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_old.clone(), beta.clone()]);
    history.apply(&prism_ir::ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:rename-alpha"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![file_id],
        previous_path: Some(
            root.join("src/lib.rs")
                .to_string_lossy()
                .into_owned()
                .into(),
        ),
        current_path: Some(
            root.join("src/lib.rs")
                .to_string_lossy()
                .into_owned()
                .into(),
        ),
        added: vec![prism_ir::ObservedNode {
            node: Node {
                id: alpha_new.clone(),
                name: "alpha_v2".into(),
                kind: NodeKind::Function,
                file: file_id,
                span: Span::new(alpha_start, alpha_end),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
        }],
        removed: vec![prism_ir::ObservedNode {
            node: Node {
                id: alpha_old.clone(),
                name: "alpha".into(),
                kind: NodeKind::Function,
                file: file_id,
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
        }],
        updated: vec![(
            prism_ir::ObservedNode {
                node: Node {
                    id: beta.clone(),
                    name: "beta".into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(2),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(20), None, None),
            },
            prism_ir::ObservedNode {
                node: Node {
                    id: beta.clone(),
                    name: "beta".into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::new(beta_start, beta_end),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(21), None, None),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let lineage_id = history
        .lineage_of(&alpha_new)
        .expect("renamed node should keep lineage")
        .0
        .to_string();
    let host = host_with_prism(Prism::with_history(graph, history));

    let lineage = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.lineage(stale);
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded lineage query should succeed: {error:#}"),
    };
    let slice = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.editSlice(stale, {{ maxLines: 2, maxChars: 120 }});
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded edit slice query should succeed: {error:#}"),
    };
    let full = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.full(stale);
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded full query should succeed: {error:#}"),
    };

    assert!(lineage
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "target_remapped_via_lineage"));
    assert!(slice.result["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha_v2()"));
    assert!(full
        .result
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha_v2()"));
    assert_eq!(lineage.result["current"]["id"]["path"], "demo::alpha_v2");
}

#[test]
fn prism_file_queries_read_exact_ranges_and_around_line_slices() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  read: prism.file("src/recall.rs").read({ startLine: 2, endLine: 4 }),
  around: prism.file("src/recall.rs").around({ line: 3, before: 1, after: 1 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["read"]["startLine"], 2);
    assert_eq!(result.result["read"]["endLine"], 4);
    let read_text = result.result["read"]["text"].as_str().unwrap_or_default();
    assert!(read_text.contains("let alpha = \"lineage context\";"));
    assert!(read_text.contains("let beta = \"prior outcomes\";"));
    assert!(read_text.contains("let gamma = \"task journal\";"));
    assert!(!read_text.contains("pub fn memory_recall()"));

    assert_eq!(result.result["around"]["startLine"], 2);
    assert_eq!(result.result["around"]["endLine"], 4);
    assert_eq!(result.result["around"]["focus"]["startLine"], 3);
    assert_eq!(result.result["around"]["focus"]["endLine"], 3);
    assert_eq!(result.result["around"]["relativeFocus"]["startLine"], 2);
    assert_eq!(result.result["around"]["relativeFocus"]["endLine"], 2);
    let around_text = result.result["around"]["text"].as_str().unwrap_or_default();
    assert!(around_text.contains("let alpha = \"lineage context\";"));
    assert!(around_text.contains("let beta = \"prior outcomes\";"));
    assert!(around_text.contains("let gamma = \"task journal\";"));
    assert!(!around_text.contains("pub fn memory_recall()"));
}

#[test]
fn prism_text_search_returns_exact_locations_and_honors_filters() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  literal: prism.searchText("read context", {
    path: "src/recall.rs",
    limit: 2,
    contextLines: 0,
  }),
  regex: prism.searchText("read context|edit context", {
    regex: true,
    path: "src/recall.rs",
    limit: 2,
    contextLines: 0,
  }),
  folded: prism.searchText("READ CONTEXT", {
    path: "src/recall.rs",
    limit: 1,
    contextLines: 0,
  }),
  strict: prism.searchText("READ CONTEXT", {
    path: "src/recall.rs",
    caseSensitive: true,
    limit: 1,
    contextLines: 0,
  }),
  globbed: prism.searchText("Integration Points", {
    glob: "docs/**/*.md",
    limit: 1,
    contextLines: 0,
  }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let literal = result.result["literal"]
        .as_array()
        .expect("literal results");
    assert_eq!(literal.len(), 2);
    assert_eq!(literal[0]["path"], "src/recall.rs");
    assert_eq!(literal[0]["location"]["startLine"], 8);
    assert_eq!(literal[0]["excerpt"]["startLine"], 8);
    assert!(literal[0]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("let eta = \"read context\";"));

    let regex = result.result["regex"].as_array().expect("regex results");
    assert_eq!(regex.len(), 2);
    assert_eq!(regex[0]["path"], "src/recall.rs");
    assert_eq!(regex[0]["location"]["startLine"], 8);
    assert_eq!(regex[1]["location"]["startLine"], 9);

    let folded = result.result["folded"].as_array().expect("folded results");
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0]["location"]["startLine"], 8);

    let strict = result.result["strict"].as_array().expect("strict results");
    assert!(strict.is_empty());

    let globbed = result.result["globbed"]
        .as_array()
        .expect("globbed results");
    assert_eq!(globbed.len(), 1);
    assert_eq!(globbed[0]["path"], "docs/SPEC.md");
    assert_eq!(globbed[0]["location"]["startLine"], 3);
    assert!(globbed[0]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("## Integration Points"));
}

#[test]
fn prism_query_log_exposes_recent_slow_and_trace_views() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
return prism.searchText("read context", {
  path: "src/recall.rs",
  limit: 1,
  contextLines: 0,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("text search query should succeed");

    host.execute(
        test_session(&host),
        r#"
return prism.file("src/recall.rs").around({
  line: 8,
  before: 1,
  after: 1,
});
"#,
        QueryLanguage::Ts,
    )
    .expect("file slice query should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.queryLog({ limit: 5, target: "src/recall.rs" });
const slow = prism.slowQueries({
  limit: 5,
  minDurationMs: 0,
  target: "src/recall.rs",
});
return {
  recent,
  slow,
  trace: recent[0] ? prism.queryTrace(recent[0].id) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query log query should succeed");

    let recent = result.result["recent"]
        .as_array()
        .expect("recent query log");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0]["kind"], "typescript");
    assert_eq!(recent[0]["success"], true);
    assert!(recent[0]["sessionId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("session:"));
    assert!(recent[0]["operations"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|value| value == "fileAround")));
    let touched = recent[0]["touched"].as_array().expect("touched values");
    assert!(touched.iter().any(|value| value == "src/recall.rs"));
    assert!(
        recent[0]["result"]["jsonBytes"]
            .as_u64()
            .expect("json bytes should be present")
            > 0
    );

    let slow = result.result["slow"].as_array().expect("slow query log");
    assert_eq!(slow.len(), 2);
    assert!(
        slow[0]["durationMs"].as_u64().unwrap_or_default()
            >= slow[1]["durationMs"].as_u64().unwrap_or_default()
    );

    assert_eq!(result.result["trace"]["entry"]["id"], recent[0]["id"]);
    assert!(result.result["trace"]["entry"]["operations"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|value| value == "fileAround")));
    let phases = result.result["trace"]["phases"]
        .as_array()
        .expect("trace phases");
    let operations = phases
        .iter()
        .filter_map(|phase| phase["operation"].as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"typescript.refreshWorkspace"));
    assert!(operations.contains(&"typescript.statement_body.prepare"));
    assert!(operations.contains(&"typescript.statement_body.transpile"));
    assert!(operations.contains(&"typescript.statement_body.workerRoundTrip"));
    assert!(operations.contains(&"fileAround"));
    assert!(phases
        .iter()
        .find(|phase| phase["operation"] == "fileAround")
        .is_some_and(|phase| phase["success"] == true));
}

#[test]
fn prism_query_log_touched_prefers_semantic_targets() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
return prism.runtimeLogs({ level: "WARN", limit: 2 });
"#,
        QueryLanguage::Ts,
    )
    .expect("runtime log query should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.queryLog({ limit: 1, operation: "runtimeLogs" })[0];
"#,
            QueryLanguage::Ts,
        )
        .expect("query log lookup should succeed");

    let touched = result.result["touched"].as_array().expect("touched values");
    assert!(!touched.iter().any(|value| value == "WARN"));
}

#[test]
fn mutation_trace_records_internal_phases_for_persisted_only_mutations() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    let result = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::alpha".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated alpha".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect("mutation should succeed");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mutation.refreshWorkspace"));
    assert!(operations.contains(&"mutation.operation"));
    assert!(operations.contains(&"mutation.encodeResult"));
    assert!(operations.contains(&"mutation.publishTaskUpdate"));
    assert!(trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .and_then(|phase| phase.args_summary.as_ref())
        .is_some_and(|args| args["refreshPath"] != Value::String("skipped".to_string())));
}

#[test]
fn follow_up_queries_skip_persisted_refresh_after_local_outcome_write() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.store_outcome_without_refresh(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated alpha".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: None,
        },
    )
    .expect("local outcome write should succeed");

    host.execute(
        test_session(&host),
        r#"
return prism.runtimeStatus();
"#,
        QueryLanguage::Ts,
    )
    .expect("follow-up query should succeed");

    let trace = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.queryLog({ limit: 1, operation: "runtimeStatus" });
return recent[0] ? prism.queryTrace(recent[0].id) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("trace lookup should succeed")
        .result;

    let refresh_phase = trace["phases"]
        .as_array()
        .expect("trace phases")
        .iter()
        .find(|phase| phase["operation"] == "typescript.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase["argsSummary"]
        .as_object()
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("none".to_string()))
    );
    assert_eq!(args.get("episodicReloaded"), Some(&Value::Bool(false)));
    assert_eq!(args.get("inferenceReloaded"), Some(&Value::Bool(false)));
    assert_eq!(args.get("coordinationReloaded"), Some(&Value::Bool(false)));
}

#[test]
fn first_mutation_after_workspace_refresh_skips_persisted_reload() {
    let root = temp_workspace();
    let session = index_workspace_session(&root).unwrap();
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn gamma() { delta(); }\npub fn delta() {}\n",
    )
    .expect("workspace edit should succeed");
    server
        .host
        .workspace
        .as_ref()
        .expect("workspace session")
        .refresh_fs()
        .expect("workspace refresh should succeed");

    let result = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::gamma".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated gamma".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect("mutation should succeed");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("none".to_string()))
    );
    assert_eq!(args.get("deferred"), Some(&Value::Bool(false)));
}

#[test]
fn prism_query_errors_include_js_message_and_stack() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
throw new Error("boom");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query runtime failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
    assert!(
        !message.contains("Exception generated by QuickJS"),
        "{message}"
    );
}

#[test]
fn prism_query_parse_errors_map_back_to_user_snippet_locations() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const broken = ;
return broken;
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query parse failed"), "{message}");
    assert!(
        message.contains("user snippet line 2, column 16"),
        "{message}"
    );
    assert!(message.contains("Statement-body mode"), "{message}");
    assert!(message.contains("Implicit-expression mode"), "{message}");
    assert!(
        message.contains("single expression such as `({ ... })`"),
        "{message}"
    );
    assert!(
        message.contains("statement-style snippet with an explicit `return ...`"),
        "{message}"
    );
}

#[test]
fn prism_query_runtime_errors_map_back_to_user_snippet_locations() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const value = 1;
throw new Error("boom");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query runtime failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(
        message.contains("Inspect the referenced user-snippet line"),
        "{message}"
    );
}

#[test]
fn prism_query_serialization_failures_have_actionable_hints() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const value = {};
value.self = value;
return value;
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(
        message.contains("prism_query result is not JSON-serializable"),
        "{message}"
    );
    assert!(message.contains("circular reference"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(message.contains("JSON-serializable values"), "{message}");
}

#[test]
fn prism_query_missing_return_emits_actionable_diagnostic() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result, Value::Null);
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "query_return_missing"));
}

#[test]
fn prism_query_supports_async_style_multi_statement_snippets() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const results = await prism.search("alpha", { limit: 2, kind: "function" });
const sym = await prism.symbol("alpha");
return {
  top: results[0]?.id.path ?? null,
  exact: sym?.id.path ?? null,
  count: results.length,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("async-style query should succeed");

    assert_eq!(result.result["top"], "demo::alpha");
    assert_eq!(result.result["exact"], "demo::alpha");
    assert_eq!(result.result["count"], 1);
}

#[test]
fn prism_query_supports_implicit_expression_object_results() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
({
  top: (await prism.search("alpha", { limit: 1, kind: "function" }))[0]?.id.path ?? null,
  exact: (await prism.symbol("alpha"))?.id.path ?? null,
  count: (await prism.search("alpha", { limit: 2, kind: "function" })).length,
})
"#,
            QueryLanguage::Ts,
        )
        .expect("implicit expression query should succeed");

    assert_eq!(result.result["top"], "demo::alpha");
    assert_eq!(result.result["exact"], "demo::alpha");
    assert_eq!(result.result["count"], 1);
    assert!(!result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "query_return_missing"));
}

#[test]
fn prism_query_supports_implicit_expression_values() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
(await prism.symbol("alpha"))?.id.path ?? null
"#,
            QueryLanguage::Ts,
        )
        .expect("implicit expression value query should succeed");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert!(result.diagnostics.is_empty());
}

#[test]
#[ignore = "heavy end-to-end replay case; run explicitly when validating replay coverage"]
fn query_replay_cases_cover_real_failures_and_repo_queries() {
    let fixture_root = temp_workspace();
    let repo_root = repo_workspace_root();

    let mut fixture_default_host = None;
    let mut fixture_tiny_output_cap_host = None;
    let mut repo_default_host = None;

    for case in replay_cases() {
        let host = match case.profile {
            ReplayHostProfile::FixtureDefault => fixture_default_host.get_or_insert_with(|| {
                host_with_session_internal(index_workspace_session(&fixture_root).unwrap())
            }),
            ReplayHostProfile::FixtureTinyOutputCap => fixture_tiny_output_cap_host
                .get_or_insert_with(|| {
                    let mut limits = QueryLimits::default();
                    limits.max_output_json_bytes = 64;
                    host_with_session_internal_and_limits(
                        index_workspace_session(&fixture_root).unwrap(),
                        limits,
                    )
                }),
            ReplayHostProfile::RepoDefault => repo_default_host.get_or_insert_with(|| {
                host_with_session_internal(index_workspace_session(&repo_root).unwrap())
            }),
        };

        match case.expectation {
            ReplayExpectation::Success(assertion) => {
                let envelope = host
                    .execute(test_session(&host), case.code, QueryLanguage::Ts)
                    .unwrap_or_else(|error| {
                        panic!("replay case `{}` should succeed: {error}", case.name)
                    });
                assertion(&envelope);
            }
            ReplayExpectation::Error(assertion) => {
                let error = host
                    .execute(test_session(&host), case.code, QueryLanguage::Ts)
                    .expect_err(&format!("replay case `{}` should fail", case.name));
                assertion(&error.to_string());
            }
        }
    }
}

#[test]
fn prism_runtime_views_surface_status_logs_and_timeline() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("health listener");
    let addr = listener.local_addr().expect("listener addr");
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(
        prism_dir.join("prism-mcp-daemon.log"),
        [
            json!({
                "timestamp": "2026-03-26T15:12:35Z",
                "level": "INFO",
                "message": "starting prism-mcp",
                "target": "prism_mcp::logging",
                "filename": "crates/prism-mcp/src/logging.rs",
                "line_number": 53,
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:12:36Z",
                "level": "INFO",
                "message": "completed prism workspace indexing",
                "target": "prism_core::indexer",
                "filename": "crates/prism-core/src/indexer.rs",
                "line_number": 435,
                "total_ms": "6227",
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:12:42Z",
                "level": "INFO",
                "message": "prism-mcp daemon ready",
                "target": "prism_mcp::daemon_mode",
                "filename": "crates/prism-mcp/src/daemon_mode.rs",
                "line_number": 57,
                "startup_ms": "6534",
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:16:23Z",
                "level": "WARN",
                "message": "response error",
                "target": "rmcp::service",
                "filename": "service.rs",
                "line_number": 873,
                "error": "query_execution_failed",
            })
            .to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  status: prism.runtimeStatus(),
  warnings: prism.runtimeLogs({ level: "WARN", limit: 5 }),
  timeline: prism.runtimeTimeline({ limit: 10 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("runtime views query should succeed");

    let status = &result.result["status"];
    assert_eq!(status["health"]["ok"], true);
    assert_eq!(status["daemonCount"], 0);
    assert_eq!(status["bridgeCount"], 0);
    assert_eq!(status["healthPath"], "/healthz");
    assert_eq!(status["connection"]["mode"], "direct-daemon");
    assert_eq!(status["connection"]["transport"], "streamable-http");
    assert_eq!(
        status["connection"]["bridgeRole"],
        "stdio-compatibility-only"
    );
    assert_eq!(
        status["connection"]["healthUri"]
            .as_str()
            .unwrap_or_default(),
        format!("http://{addr}/healthz")
    );
    assert_eq!(
        status["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    assert!(status["logPath"]
        .as_str()
        .unwrap_or_default()
        .ends_with(".prism/prism-mcp-daemon.log"));
    assert!(status["cachePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with(".prism/cache.db"));

    let warnings = result.result["warnings"]
        .as_array()
        .expect("runtime warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["message"], "response error");
    assert_eq!(warnings[0]["target"], "rmcp::service");
    assert_eq!(warnings[0]["fields"]["error"], "query_execution_failed");

    let timeline = result.result["timeline"]
        .as_array()
        .expect("runtime timeline");
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0]["message"], "starting prism-mcp");
    assert_eq!(timeline[1]["message"], "completed prism workspace indexing");
    assert_eq!(timeline[2]["message"], "prism-mcp daemon ready");

    server.join().expect("health server should exit cleanly");
}

#[test]
fn prism_connection_info_surfaces_direct_daemon_endpoint_without_internal_mode() {
    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();
    let addr = "127.0.0.1:9";

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(prism_dir.join("prism-mcp-daemon.log"), "").unwrap();

    let host = host_with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            "return prism.connectionInfo();",
            QueryLanguage::Ts,
        )
        .expect("connection info query should succeed");

    assert_eq!(result.result["mode"], "direct-daemon");
    assert_eq!(result.result["transport"], "streamable-http");
    assert_eq!(result.result["bridgeRole"], "stdio-compatibility-only");
    assert_eq!(
        result.result["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    assert_eq!(
        result.result["healthUri"].as_str().unwrap_or_default(),
        format!("http://{addr}/healthz")
    );
}

#[test]
fn prism_runtime_views_prefer_structured_runtime_state() {
    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();
    let addr = "127.0.0.1:9";

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(prism_dir.join("prism-mcp-daemon.log"), "").unwrap();
    fs::write(
        prism_dir.join("prism-mcp-runtime.json"),
        json!({
            "processes": [
                {
                    "pid": std::process::id(),
                    "kind": "daemon",
                    "started_at": current_timestamp(),
                    "health_path": "/healthz",
                    "http_uri": format!("http://{addr}/mcp"),
                    "upstream_uri": null,
                }
            ],
            "events": [
                {
                    "ts": 10,
                    "timestamp": "10",
                    "level": "INFO",
                    "message": "starting prism-mcp",
                    "target": "prism_mcp::logging",
                    "file": "crates/prism-mcp/src/logging.rs",
                    "line_number": null,
                    "fields": { "mode": "daemon" }
                },
                {
                    "ts": 11,
                    "timestamp": "11",
                    "level": "INFO",
                    "message": "built prism-mcp workspace server",
                    "target": "prism_mcp::lib",
                    "file": "crates/prism-mcp/src/lib.rs",
                    "line_number": null,
                    "fields": { "fileCount": 12 }
                },
                {
                    "ts": 12,
                    "timestamp": "12",
                    "level": "INFO",
                    "message": "prism-mcp daemon ready",
                    "target": "prism_mcp::daemon_mode",
                    "file": "crates/prism-mcp/src/daemon_mode.rs",
                    "line_number": null,
                    "fields": { "httpUri": format!("http://{addr}/mcp") }
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  status: prism.runtimeStatus(),
  timeline: prism.runtimeTimeline({ limit: 10 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("runtime state query should succeed");

    let status = &result.result["status"];
    assert_eq!(status["health"]["ok"], true);
    assert_eq!(status["daemonCount"], 1);
    assert_eq!(status["bridgeCount"], 0);
    assert_eq!(status["healthPath"], "/healthz");
    assert_eq!(status["connection"]["mode"], "direct-daemon");
    assert_eq!(
        status["connection"]["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    let processes = status["processes"].as_array().expect("runtime processes");
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0]["kind"], "daemon");

    let timeline = result.result["timeline"]
        .as_array()
        .expect("runtime timeline");
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0]["message"], "starting prism-mcp");
    assert_eq!(timeline[1]["message"], "built prism-mcp workspace server");
    assert_eq!(timeline[2]["message"], "prism-mcp daemon ready");
}

#[test]
fn prism_runtime_views_ignore_invalid_runtime_state_sidecar() {
    let root = temp_workspace();
    fs::write(root.join(".gitignore"), ".prism/\n").unwrap();
    fs::create_dir_all(root.join(".prism")).unwrap();
    fs::write(
        root.join(".prism").join("prism-mcp-runtime.json"),
        "{ invalid",
    )
    .unwrap();
    fs::write(root.join(".prism").join("prism-mcp-daemon.log"), "").unwrap();
    fs::write(
        root.join(".prism").join("prism-mcp-http-uri"),
        "http://127.0.0.1:9/mcp",
    )
    .unwrap();

    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
        )
        .expect("invalid runtime state should not break runtime status");

    assert_eq!(result.result["health"]["ok"], false);
    assert_eq!(result.result["daemonCount"], 0);
    assert_eq!(result.result["bridgeCount"], 0);
}

#[test]
fn prism_change_views_surface_recent_files_symbols_and_task_changes() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn alpha() {}\npub fn beta() {}\n";
    fs::write(&source_path, source).unwrap();

    let alpha_span = {
        let start = source.find("alpha").expect("alpha span");
        Span::new(start, start + "alpha".len())
    };
    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: alpha_span,
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone()]);

    let task_id = TaskId::new("task:change-view");
    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:change-view"),
                ts: 10,
                actor: EventActor::System,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::File(file_id), AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "patched src/lib.rs".into(),
            evidence: Vec::new(),
            metadata: json!({
                "trigger": "ManualReindex",
                "filePaths": [source_path.to_string_lossy().into_owned()],
                "changedSymbols": [
                    {
                        "status": "updated_after",
                        "id": alpha_id,
                        "name": "alpha",
                        "kind": NodeKind::Function,
                        "filePath": source_path.to_string_lossy().into_owned(),
                        "span": alpha_span,
                    },
                    {
                        "status": "removed",
                        "id": NodeId::new("demo", "demo::beta", NodeKind::Function),
                        "name": "beta",
                        "kind": NodeKind::Function,
                        "filePath": source_path.to_string_lossy().into_owned(),
                        "span": beta_span,
                    }
                ],
            }),
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  files: prism.changedFiles({ limit: 5, path: "src/lib.rs" }),
  symbols: prism.changedSymbols("src/lib.rs", { limit: 5 }),
  patches: prism.recentPatches({ path: "src/lib.rs", limit: 5 }),
  diff: (() => {
    const sym = prism.symbol("alpha");
    return sym ? prism.diffFor(sym, { limit: 5 }) : [];
  })(),
  lineageDiff: (() => {
    const sym = prism.symbol("alpha");
    return sym?.lineageId ? prism.diffFor({ lineageId: sym.lineageId }, { limit: 5 }) : [];
  })(),
  task: prism.taskChanges("task:change-view", { limit: 5 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("change-view query should succeed");

    let changed_file = &result.result["files"][0];
    assert!(changed_file["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("src/lib.rs"));
    assert_eq!(changed_file["changedSymbolCount"], 2);
    assert_eq!(changed_file["removedCount"], 1);
    assert_eq!(changed_file["updatedCount"], 1);

    let symbols = result.result["symbols"]
        .as_array()
        .expect("changed symbols");
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().any(|symbol| {
        symbol["status"] == "updated_after"
            && symbol["location"]["startLine"] == 1
            && symbol["excerpt"]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("alpha")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["status"] == "removed"
            && symbol["location"]["startLine"] == 2
            && symbol["excerpt"]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("beta")
    }));

    let patch = &result.result["patches"][0];
    assert_eq!(patch["trigger"], "ManualReindex");
    assert_eq!(patch["taskId"], "task:change-view");
    assert_eq!(patch["changedSymbols"].as_array().unwrap().len(), 2);
    assert!(patch["files"][0]
        .as_str()
        .unwrap_or_default()
        .ends_with("src/lib.rs"));

    let diff = result.result["diff"].as_array().expect("target diff");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0]["eventId"], "outcome:change-view");
    assert_eq!(diff[0]["symbol"]["name"], "alpha");
    assert_eq!(diff[0]["symbol"]["location"]["startLine"], 1);
    assert!(diff[0]["symbol"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("alpha"));
    assert!(diff[0]["symbol"]["lineageId"].as_str().is_some());

    let lineage_diff = result.result["lineageDiff"]
        .as_array()
        .expect("lineage diff");
    assert_eq!(lineage_diff.len(), 1);
    assert_eq!(lineage_diff[0]["symbol"]["name"], "alpha");
    assert_eq!(
        lineage_diff[0]["symbol"]["lineageId"],
        diff[0]["symbol"]["lineageId"]
    );

    let task_patch = &result.result["task"][0];
    assert_eq!(task_patch["eventId"], "outcome:change-view");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prism_search_surfaces_toml_config_keys_through_normal_queries() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"

[workspace]
members = ["crates/alpha"]

[dependencies]
serde = "1.0"
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const workspaceKey = prism.search("workspace", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
const membersKey = prism.search("members", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
const serdeKey = prism.search("serde", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
return {
  workspaceKey,
  membersKey,
  serdeKey,
  workspaceContains: workspaceKey?.relations().contains ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("toml query should succeed");

    assert_eq!(result.result["workspaceKey"]["name"], "workspace");
    assert!(result.result["workspaceKey"]["filePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with("/Cargo.toml"));
    assert_eq!(result.result["membersKey"]["name"], "members");
    assert_eq!(result.result["serdeKey"]["name"], "serde");
    let workspace_contains = result.result["workspaceContains"]
        .as_array()
        .expect("workspace contains");
    assert!(workspace_contains
        .iter()
        .any(|value| value["name"] == "members"));
}

#[test]
fn prism_search_supports_exact_path_and_structured_key_narrowing() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("crates/alpha/src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/alpha"]

[package]
name = "demo"
version.workspace = true
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/alpha/Cargo.toml"),
        r#"[package]
name = "alpha"
version.workspace = true
"#,
    )
    .unwrap();
    fs::write(root.join("crates/alpha/src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  topLevel: prism.search("workspace", {
    path: "Cargo.toml",
    pathMode: "exact",
    kind: "toml-key",
    topLevelOnly: true,
    limit: 5,
  }),
  nested: prism.search("workspace", {
    path: "Cargo.toml",
    pathMode: "exact",
    kind: "toml-key",
    structuredPath: "package.version.workspace",
    limit: 5,
  }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("exact path search query should succeed");

    let top_level = result.result["topLevel"]
        .as_array()
        .expect("top-level results");
    assert_eq!(top_level.len(), 1);
    assert_eq!(top_level[0]["name"], "workspace");
    assert!(top_level[0]["id"]["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("::workspace"));
    assert!(top_level[0]["filePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with("/Cargo.toml"));

    let nested = result.result["nested"].as_array().expect("nested results");
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0]["name"], "workspace");
    assert!(nested[0]["id"]["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("::package::version::workspace"));
}

#[test]
fn markdown_heading_symbols_cover_their_section_body() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Top\nalpha\n## Child\nbeta\n# Next\ngamma\n",
    )
    .unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(test_session(&host),
            r#"
const heading = prism.search("Top", { path: "docs/SPEC.md", kind: "markdown-heading", limit: 1 })[0];
return {
  full: heading?.full() ?? null,
  excerpt: heading?.excerpt() ?? null,
  location: heading?.location ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let full = result.result["full"].as_str().unwrap_or_default();
    assert!(full.contains("## Child"));
    assert!(!full.contains("# Next"));
    assert_eq!(result.result["location"]["startLine"], 1);
    assert_eq!(result.result["location"]["endLine"], 4);
    assert_eq!(result.result["excerpt"]["startLine"], 1);
}

#[test]
fn spec_cluster_and_drift_surface_behavioral_owners() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return {
  cluster: spec ? prism.specCluster(spec) : null,
  drift: spec ? prism.explainDrift(spec) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let read_paths = result.result["cluster"]["readPath"]
        .as_array()
        .expect("cluster readPath should be an array");
    assert!(read_paths.iter().any(|candidate| {
        candidate["symbol"]["id"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("memory_recall")
    }));

    let persistence_paths = result.result["cluster"]["persistencePath"]
        .as_array()
        .expect("cluster persistencePath should be an array");
    assert!(persistence_paths.iter().any(|candidate| {
        candidate["symbol"]["id"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("reanchor_persisted_memory_snapshot")
    }));

    let tests = result.result["cluster"]["tests"]
        .as_array()
        .expect("cluster tests should be an array");
    assert!(tests.iter().any(|candidate| {
        candidate["symbol"]["filePath"]
            .as_str()
            .unwrap_or_default()
            .contains("/tests/")
    }));

    let next_reads = result.result["drift"]["nextReads"]
        .as_array()
        .expect("drift nextReads should be an array");
    assert!(!next_reads.is_empty());
    assert!(matches!(
        result.result["drift"]["trustSignals"]["confidenceLabel"].as_str(),
        Some("medium" | "high")
    ));
    assert!(result.result["drift"]["trustSignals"]["evidenceSources"]
        .as_array()
        .is_some_and(|items| items.iter().any(|value| value == "inferred")));
    assert!(result.result["drift"]["expectations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value
            .as_str()
            .unwrap_or_default()
            .contains("prior outcomes")));

    let spec_id = host
        .current_prism()
        .search(
            "Integration Points",
            1,
            Some(NodeKind::MarkdownHeading),
            Some("docs/SPEC.md"),
        )
        .first()
        .expect("spec heading should be indexed")
        .id()
        .clone();
    let symbol_resource = host
        .symbol_resource_value(test_session(&host), &spec_id)
        .unwrap();
    assert!(symbol_resource.workspace_revision.graph_version > 0);
    assert!(symbol_resource.spec_cluster.is_some());
    assert!(symbol_resource.spec_drift.is_some());
    assert!(!symbol_resource.suggested_reads.is_empty());
    assert!(!symbol_resource.read_context.suggested_reads.is_empty());
    assert!(!symbol_resource.edit_context.suggested_queries.is_empty());
    assert!(!symbol_resource.discovery.suggested_reads.is_empty());
    assert!(!symbol_resource
        .discovery
        .validation_context
        .suggested_queries
        .is_empty());
    assert!(
        symbol_resource
            .discovery
            .recent_change_context
            .suggested_queries
            .len()
            >= 3
    );
    assert!(symbol_resource
        .discovery
        .trust_signals
        .evidence_sources
        .iter()
        .any(|source| matches!(
            source,
            prism_js::EvidenceSourceKind::DirectGraph | prism_js::EvidenceSourceKind::Inferred
        )));
    assert!(!symbol_resource.discovery.where_used_behavioral.is_empty());
    assert!(!symbol_resource.discovery.why.is_empty());
    for expected in [
        "Read Context",
        "Focused Block",
        "Next Reads",
        "Where Used",
        "Validation Recipe",
        "Edit Context",
    ] {
        assert!(symbol_resource
            .suggested_queries
            .iter()
            .any(|query| query.label == expected));
    }
    assert_eq!(
        symbol_resource.related_resources[0].uri,
        symbol_resource.uri
    );
}

#[test]
fn owner_lookup_and_behavioral_search_prefer_behavioral_owners() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return {
  behavioral: prism.search("memory recall", {
    strategy: "behavioral",
    ownerKind: "read",
    limit: 5,
  }),
  owners: spec ? prism.owners(spec, { kind: "read", limit: 5 }) : [],
  implementationOwners: spec
    ? prism.implementationFor(spec, { mode: "owners", ownerKind: "read" })
    : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let behavioral = result.result["behavioral"]
        .as_array()
        .expect("behavioral search should return an array");
    assert!(!behavioral.is_empty());
    assert!(behavioral.iter().any(|symbol| {
        symbol["ownerHint"]["kind"].as_str() == Some("read")
            && symbol["id"]["path"]
                .as_str()
                .unwrap_or_default()
                .contains("memory_recall")
    }));
    assert!(behavioral.iter().any(|symbol| {
        symbol["ownerHint"]["trustSignals"]["evidenceSources"]
            .as_array()
            .is_some_and(|items| items.iter().any(|value| value == "inferred"))
    }));

    let owners = result.result["owners"]
        .as_array()
        .expect("owners should return an array");
    assert!(owners.iter().any(|candidate| {
        candidate["kind"].as_str() == Some("read")
            && candidate["why"]
                .as_str()
                .unwrap_or_default()
                .contains("read-oriented")
    }));
    assert!(owners.iter().any(|candidate| {
        matches!(
            candidate["trustSignals"]["confidenceLabel"].as_str(),
            Some("medium" | "high")
        ) && candidate["trustSignals"]["evidenceSources"]
            .as_array()
            .is_some_and(|items| items.iter().any(|value| value == "inferred"))
    }));

    let implementation_owners = result.result["implementationOwners"]
        .as_array()
        .expect("owner-mode implementationFor should return an array");
    assert!(implementation_owners.iter().any(|symbol| {
        symbol["ownerHint"]["kind"].as_str() == Some("read")
            && symbol["id"]["path"]
                .as_str()
                .unwrap_or_default()
                .contains("memory_recall")
    }));
}

#[test]
fn search_resource_payload_surfaces_suggested_reads() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(
            test_session(&host),
            "prism://search/memory%20recall?strategy=behavioral&ownerKind=read",
            "memory recall",
        )
        .unwrap();

    assert_eq!(payload.strategy, "behavioral");
    assert_eq!(payload.owner_kind.as_deref(), Some("read"));
    assert!(payload.workspace_revision.graph_version > 0);
    assert!(!payload.suggested_reads.is_empty());
    assert!(payload.discovery.is_some());
    assert!(payload
        .discovery
        .as_ref()
        .is_some_and(|bundle| !bundle.suggested_reads.is_empty()));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .trust_signals
            .evidence_sources
            .iter()
            .any(|source| matches!(source, prism_js::EvidenceSourceKind::Inferred))
    }));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .validation_context
            .suggested_queries
            .iter()
            .any(|query| query.label == "Validation Context")
    }));
    assert!(payload
        .discovery
        .as_ref()
        .is_some_and(|bundle| !bundle.why.is_empty()));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .recent_change_context
            .suggested_queries
            .iter()
            .any(|query| query.label == "Recent Change Context")
    }));
    assert!(payload.suggested_reads.iter().any(|candidate| {
        candidate.kind == "read" && candidate.symbol.id.path.contains("memory_recall")
    }));
    assert!(payload.results.iter().any(|symbol| {
        symbol
            .owner_hint
            .as_ref()
            .is_some_and(|hint| hint.kind == "read")
    }));
    assert!(payload.top_read_context.is_some());
    assert!(!payload.suggested_queries.is_empty());
    assert_eq!(payload.suggested_queries[0].label, "Direct Search");
    assert_eq!(payload.suggested_queries[1].label, "Behavioral Search");
    assert_eq!(payload.suggested_queries[2].label, "Read Context");
    assert!(payload.related_resources[0]
        .uri
        .starts_with("prism://search/memory%20recall"));
    assert!(payload.related_resources[1]
        .uri
        .starts_with("prism://symbol/"));
}

#[test]
fn search_resource_payload_echoes_applied_uri_options() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/alpha"]

[package]
name = "demo"
version.workspace = true
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(test_session(&host),
            "prism://search/workspace?strategy=direct&kind=toml-key&path=Cargo.toml&pathMode=exact&structuredPath=workspace&topLevelOnly=true&preferCallableCode=false&preferEditableTargets=true&preferBehavioralOwners=true&includeInferred=false",
            "workspace",
        )
        .unwrap();

    assert_eq!(payload.strategy, "direct");
    assert_eq!(payload.owner_kind, None);
    assert_eq!(payload.kind.as_deref(), Some("toml-key"));
    assert_eq!(payload.path.as_deref(), Some("Cargo.toml"));
    assert_eq!(payload.path_mode.as_deref(), Some("exact"));
    assert_eq!(payload.structured_path.as_deref(), Some("workspace"));
    assert_eq!(payload.top_level_only, Some(true));
    assert_eq!(payload.prefer_callable_code, Some(false));
    assert_eq!(payload.prefer_editable_targets, Some(true));
    assert_eq!(payload.prefer_behavioral_owners, Some(true));
    assert!(!payload.include_inferred);
    assert_eq!(payload.results.len(), 1);
    assert_eq!(
        payload.results[0].id.path,
        "demo::document::Cargo_toml::workspace"
    );
}

#[test]
fn resource_suggested_candidates_use_compact_default_excerpts() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let id = host
        .current_prism()
        .search(
            "memory_recall",
            1,
            Some(NodeKind::Function),
            Some("src/recall.rs"),
        )
        .first()
        .expect("memory_recall should be indexed")
        .id()
        .clone();

    let symbol_payload = host
        .symbol_resource_value(test_session(&host), &id)
        .unwrap();
    let search_payload = host
        .search_resource_value(
            test_session(&host),
            "prism://search/memory_recall?strategy=behavioral&ownerKind=read",
            "memory_recall",
        )
        .unwrap();

    let symbol_candidate_excerpt = symbol_payload
        .suggested_reads
        .iter()
        .filter_map(|candidate| candidate.symbol.source_excerpt.as_ref())
        .next()
        .expect("symbol resource suggested candidate should include excerpt");
    assert!(symbol_candidate_excerpt.text.chars().count() <= 240);

    let search_excerpt = search_payload
        .suggested_reads
        .iter()
        .find(|candidate| candidate.symbol.id.path.contains("memory_recall"))
        .and_then(|candidate| candidate.symbol.source_excerpt.as_ref())
        .expect("search resource suggested candidate should include excerpt");
    assert!(search_excerpt.text.chars().count() <= 240);
    assert!(search_excerpt.truncated);
}

#[test]
fn read_and_edit_context_queries_return_semantic_bundles() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let session = index_workspace_session(&root).unwrap();
    let spec_id = session
        .prism()
        .search(
            "Integration Points",
            1,
            Some(NodeKind::MarkdownHeading),
            Some("docs/SPEC.md"),
        )
        .first()
        .expect("spec heading should be indexed")
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validation-context"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:validation-context")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(spec_id)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "integration-point regression surfaced during validation".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let host = QueryHost::with_session(session);

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec
  ? {
      read: prism.readContext(spec),
      edit: prism.editContext(spec),
      validation: prism.validationContext(spec),
      recentChange: prism.recentChangeContext(spec),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result.result["read"]["directLinks"].is_array());
    assert_eq!(
        result.result["read"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["read"]["directLinkBlocks"].is_array());
    assert!(result.result["read"]["suggestedReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["read"]["testBlocks"].is_array());
    assert!(result.result["edit"]["writePaths"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert_eq!(
        result.result["edit"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["edit"]["writePathBlocks"].is_array());
    assert!(result.result["edit"]["checklist"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["validation"]["tests"].is_array());
    assert_eq!(
        result.result["validation"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["validation"]["testBlocks"].is_array());
    assert!(result.result["validation"]["recentFailures"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["recentChange"]["recentEvents"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["recentChange"]["suggestedQueries"].is_array());
}

#[test]
fn discovery_bundle_query_trace_records_internal_subphases() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.discovery(spec) : null;
"#,
        QueryLanguage::Ts,
    )
    .expect("discovery query should succeed");

    let trace = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.queryLog({ limit: 5 });
const discovery = recent.find((entry) => entry.operations.includes("discoveryBundle"));
return discovery ? prism.queryTrace(discovery.id) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query trace lookup should succeed");

    let phases = trace.result["phases"].as_array().expect("trace phases");
    let operations = phases
        .iter()
        .filter_map(|phase| phase["operation"].as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"discoveryBundle.prefetch"));
    assert!(operations.contains(&"discoveryBundle.entrypointsFor"));
    assert!(operations.contains(&"discoveryBundle.whereUsedBehavioral"));
    assert!(operations.contains(&"discoveryBundle.sharedContext"));
    assert!(operations.contains(&"discoveryBundle"));
}

#[test]
fn discovery_helpers_surface_next_reads_and_behavioral_where_used() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec
  ? {
      nextReads: prism.nextReads(spec, { limit: 5 }),
      whereUsed: prism.whereUsed(spec, { mode: "behavioral", limit: 5 }),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result.result["nextReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["whereUsed"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[test]
fn discovery_helpers_surface_direct_where_used_and_entrypoints() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const beta = prism.symbol("beta");
return beta
  ? {
      whereUsed: prism.whereUsed(beta, { mode: "direct", limit: 5 }).map((sym) => sym.id.path),
      entrypoints: prism.entrypointsFor(beta, { limit: 5 }).map((sym) => sym.id.path),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["whereUsed"], json!(["demo::alpha"]));
    assert_eq!(result.result["entrypoints"], json!(["demo::alpha"]));
}

#[test]
fn custom_query_limits_apply_per_host() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::beta", NodeKind::Function),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        target: NodeId::new("demo", "demo::beta", NodeKind::Function),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let host = QueryHost::new_with_limits(
        Prism::new(graph),
        QueryLimits {
            max_result_nodes: 1,
            max_call_graph_depth: 1,
            max_output_json_bytes: 512,
        },
    );

    let search = host
        .execute(
            test_session(&host),
            r#"
return prism.search("a", { limit: 10 }).map((sym) => sym.id.path);
"#,
            QueryLanguage::Ts,
        )
        .expect("search should succeed");
    assert_eq!(search.result.as_array().map(Vec::len), Some(1));
    assert_eq!(search.diagnostics[0].code, "result_truncated");

    let depth = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym?.callGraph(9);
"#,
            QueryLanguage::Ts,
        )
        .expect("call graph should succeed");
    assert_eq!(depth.result["maxDepthReached"], 1);
    assert!(depth
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "depth_limited"));

    let capped_host = QueryHost::new_with_limits(
        Prism::new(Graph::new()),
        QueryLimits {
            max_result_nodes: 1,
            max_call_graph_depth: 1,
            max_output_json_bytes: 32,
        },
    );
    let capped = capped_host
        .execute(
            test_session(&capped_host),
            r#"
return "abcdefghijklmnopqrstuvwxyz0123456789";
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(capped.result, Value::Null);
    assert!(capped
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "result_truncated"));
}

#[test]
fn search_kind_filter_uses_cli_style_names() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.search("main", { kind: "function" });
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
}

#[test]
fn reports_diagnostics_for_overbroad_searches() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
prism.search("main", { limit: 1000 });
return prism.diagnostics();
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "result_truncated");
    assert_eq!(
        result.diagnostics[0]
            .data
            .as_ref()
            .and_then(|data| data["nextAction"].as_str()),
        Some(
            "Use prism.search(query, { path: ..., module: ..., kind: ..., taskId: ..., limit: ... }) to narrow the result set."
        )
    );
}

#[test]
fn unknown_host_operations_return_actionable_diagnostics() {
    let host = host_with_node(demo_node());
    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "dispatch unknown operation",
        ),
    );

    let error = execution
        .dispatch("bogusOperation", r#"{}"#)
        .expect_err("unknown operation should fail");

    assert!(error.to_string().contains("unsupported host operation"));
    assert_eq!(execution.diagnostics().len(), 1);
    assert_eq!(execution.diagnostics()[0].code, "unknown_method");
    assert!(execution.diagnostics()[0]
        .data
        .as_ref()
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism://capabilities")));
}

#[test]
fn reuses_warm_runtime_across_queries() {
    let host = host_with_node(demo_node());

    let first = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return sym?.id.path;
"#,
            QueryLanguage::Ts,
        )
        .expect("first query should succeed");
    let second = host
        .execute(
            test_session(&host),
            r#"
return prism.entrypoints().map((sym) => sym.id.path);
"#,
            QueryLanguage::Ts,
        )
        .expect("second query should succeed");

    assert_eq!(first.result, Value::String("demo::main".to_owned()));
    assert_eq!(second.result.as_array().map(|items| items.len()), Some(1));
}

#[test]
fn cleans_up_user_globals_between_queries() {
    let host = host_with_node(demo_node());

    host.execute(
        test_session(&host),
        r#"
globalThis.__prismLeaked = 1;
return true;
"#,
        QueryLanguage::Ts,
    )
    .expect("first query should succeed");

    let second = host
        .execute(
            test_session(&host),
            r#"
return typeof globalThis.__prismLeaked;
"#,
            QueryLanguage::Ts,
        )
        .expect("second query should succeed");

    assert_eq!(second.result, Value::String("undefined".to_owned()));
}

#[test]
fn exposes_blast_radius_and_related_failures() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha.clone(),
        target: beta.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone(), beta.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:test"),
                ts: 10,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha previously failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_unit".into(),
                passed: false,
            }],
            metadata: Value::Null,
        })
        .expect("outcome event should store");

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return {
  blast: sym ? prism.blastRadius(sym) : null,
  failures: sym ? prism.relatedFailures(sym) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(
        result.result["blast"]["directNodes"][0]["path"],
        "demo::beta"
    );
    assert_eq!(
        result.result["failures"][0]["summary"],
        "alpha previously failed"
    );
}

#[test]
fn exposes_validation_recipe() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:9"),
                ts: 9,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha broke validation".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_validation".into(),
                passed: false,
            }],
            metadata: Value::Null,
        })
        .expect("outcome event should store");

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? prism.validationRecipe(sym) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["target"]["path"], "demo::alpha");
    assert_eq!(
        result.result["checks"][0],
        Value::String("test:alpha_validation".to_string())
    );
    assert_eq!(
        result.result["scoredChecks"][0]["label"],
        Value::String("test:alpha_validation".to_string())
    );
    assert_eq!(
        result.result["recentFailures"][0]["summary"],
        "alpha broke validation"
    );
}

#[test]
fn exposes_co_change_neighbors() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    for (id, line) in [(&alpha, 1), (&beta, 2)] {
        graph.add_node(Node {
            id: id.clone(),
            name: id.path.rsplit("::").next().unwrap().into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(line),
            language: Language::Rust,
        });
    }

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone(), beta.clone()]);
    history.apply(&prism_ir::ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: None,
        current_path: None,
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![
            (
                prism_ir::ObservedNode {
                    node: Node {
                        id: alpha.clone(),
                        name: "alpha".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(1),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
                },
                prism_ir::ObservedNode {
                    node: Node {
                        id: alpha.clone(),
                        name: "alpha".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(1),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(11), None, None),
                },
            ),
            (
                prism_ir::ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(20), None, None),
                },
                prism_ir::ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(21), None, None),
                },
            ),
        ],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? prism.coChangeNeighbors(sym) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result[0]["count"], 1);
    assert_eq!(result.result[0]["nodes"][0]["path"], "demo::beta");
}

#[test]
fn inferred_edge_overlay_affects_relations_queries() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });

    let host = host_with_prism(Prism::new(graph));
    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.9,
            scope: Some(InferredEdgeScopeInput::SessionOnly),
            evidence: Some(vec!["task-local inference".to_string()]),
            task_id: None,
        },
    )
    .expect("inferred edge should store");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result[0], "demo::beta");
}

#[test]
fn persisted_inferred_edges_reload_with_workspace_session() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.95,
            scope: Some(InferredEdgeScopeInput::Persisted),
            evidence: Some(vec!["persisted inference".to_string()]),
            task_id: Some("task:persist".to_string()),
        },
    )
    .expect("inferred edge should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result
        .result
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::beta"));
}

#[test]
fn persisted_notes_reload_with_workspace_session() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "episodic",
                "content": "alpha previously regressed",
                "trust": 0.9
            }),
            task_id: Some("task:note".to_string()),
        },
    )
    .expect("note should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let replay = reloaded
        .current_prism()
        .resume_task(&TaskId::new("task:note"));
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);

    let recalled = test_session(&reloaded)
        .notes
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            text: Some("regressed".to_string()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Episodic]),
            since: None,
        })
        .expect("recall should succeed");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entry.content, "alpha previously regressed");
}

#[test]
fn repo_memory_events_are_queryable_and_visible_in_memory_resource_history() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha ownership belongs in committed shared memory",
                    "promotedFrom": ["memory:seed"],
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory".to_string()),
            },
        )
        .expect("repo memory should persist");

    let queried = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  scope: "repo",
  actions: ["promoted"],
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory events query should succeed");

    assert_eq!(queried.result[0]["scope"], "Repo");
    assert_eq!(queried.result[0]["action"], "Promoted");
    assert_eq!(queried.result[0]["promotedFrom"][0], "memory:seed");

    let payload = host
        .memory_resource_value(
            test_session(&host).as_ref(),
            &MemoryId(result.memory_id.clone()),
        )
        .expect("memory resource should load");
    assert_eq!(payload.memory.scope, "Repo");
    assert_eq!(
        payload.memory.metadata["provenance"]["origin"],
        "manual_store"
    );
    assert_eq!(payload.memory.metadata["publication"]["status"], "active");
    assert_eq!(payload.history.len(), 1);
    assert_eq!(payload.history[0].memory_id, result.memory_id);
}

#[test]
fn repo_memory_store_rejects_weak_published_memory() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let error = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [],
                    "kind": "episodic",
                    "scope": "repo",
                    "content": "short memory",
                    "trust": 0.5
                }),
                task_id: Some("task:weak-memory".to_string()),
            },
        )
        .expect_err("weak repo memory should be rejected");

    assert!(error.to_string().contains("repo-published memory"));
}

#[test]
fn validation_feedback_mutation_persists_to_workspace_log() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::alpha".to_string(),
                    kind: "function".to_string(),
                }]),
                context: "blast-radius check for alpha".to_string(),
                prism_said: "Prism only reported alpha".to_string(),
                actually_true: "beta was also affected through the call graph".to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Wrong,
                corrected_manually: Some(true),
                correction: Some("checked callers directly and updated the plan".to_string()),
                metadata: Some(json!({
                    "query": "prism.blastRadius(alpha)",
                })),
                task_id: Some("task:feedback".to_string()),
            },
        )
        .expect("validation feedback should persist");

    assert!(result.entry_id.starts_with("feedback:"));
    assert_eq!(result.task_id, "task:feedback");

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].context, "blast-radius check for alpha");
    assert_eq!(entries[0].metadata["query"], "prism.blastRadius(alpha)");
}

#[test]
fn validation_feedback_mutation_allows_workspace_level_feedback_without_anchors() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: None,
                context: "compact locate docs intent dogfood".to_string(),
                prism_said: "taskIntent `docs` was rejected".to_string(),
                actually_true:
                    "docs paths should be easy to inspect without fabricating a symbol anchor"
                        .to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Mixed,
                corrected_manually: Some(true),
                correction: Some("reran locate with inspect semantics".to_string()),
                metadata: Some(json!({
                    "tool": "prism_locate",
                    "scope": "workspace",
                })),
                task_id: Some("task:feedback".to_string()),
            },
        )
        .expect("anchorless validation feedback should persist");

    assert!(result.entry_id.starts_with("feedback:"));

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].anchors.is_empty());
    assert_eq!(entries[0].metadata["tool"], "prism_locate");
}

#[test]
fn validation_feedback_query_reads_internal_feedback_stream() {
    let root = temp_workspace();
    let workspace = index_workspace_session(&root).unwrap();
    workspace
        .append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some("task:feedback".to_string()),
            context: "session behavioral-owner dogfood".to_string(),
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            prism_said: "dashboard and schema helpers ranked too high".to_string(),
            actually_true: "deeper runtime owners should rank first".to_string(),
            category: ValidationFeedbackCategory::Projection,
            verdict: ValidationFeedbackVerdict::Helpful,
            corrected_manually: true,
            correction: Some("tightened behavioral penalties".to_string()),
            metadata: json!({ "query": "prism.search(\"session\")" }),
        })
        .expect("feedback append should succeed");
    let host = host_with_session_internal(workspace);

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.validationFeedback({
  limit: 5,
  category: "projection",
  verdict: "helpful",
  contains: "session",
  correctedManually: true,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("validation feedback query should succeed");

    let entries = result
        .result
        .as_array()
        .expect("feedback results should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "projection");
    assert_eq!(entries[0]["verdict"], "helpful");
    assert_eq!(entries[0]["taskId"], "task:feedback");
    assert_eq!(entries[0]["correctedManually"], true);
    assert_eq!(entries[0]["metadata"]["query"], "prism.search(\"session\")");
    assert!(entries[0]["anchors"][0].to_string().contains("demo::alpha"));
}

#[test]
fn auto_refreshes_workspace_and_records_patch_events() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
    )
    .unwrap();

    host.refresh_workspace_for_query().unwrap();
    wait_until("background workspace refresh after external edit", || {
        host.execute(
            test_session(&host),
            r#"
const sym = prism.symbol("gamma");
const alpha = prism.symbol("alpha");
return {
  path: sym?.id.path,
  callers: alpha ? alpha.relations().callees.map((node) => node.id.path) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .map(|result| result.result["path"] == Value::String("demo::gamma".to_string()))
        .unwrap_or(false)
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("gamma");
const alpha = prism.symbol("alpha");
return {
  path: sym?.id.path,
  callers: alpha ? alpha.relations().callees.map((node) => node.id.path) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed after background refresh");

    assert_eq!(result.result["path"], "demo::gamma");
    assert!(result.result["callers"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::gamma"));

    let patch_events = host
        .current_prism()
        .outcome_memory()
        .outcomes_for(
            &[AnchorRef::Node(NodeId::new(
                "demo",
                "demo::gamma",
                NodeKind::Function,
            ))],
            10,
        )
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .collect::<Vec<_>>();
    assert_eq!(patch_events.len(), 1);
}

#[test]
fn unchanged_query_skips_workspace_refresh() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");

    assert_eq!(workspace.observed_fs_revision(), 0);
    assert_eq!(workspace.applied_fs_revision(), 0);

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.symbol("alpha")?.id.path ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed without a refresh");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert_eq!(workspace.observed_fs_revision(), 0);
    assert_eq!(workspace.applied_fs_revision(), 0);
}

#[test]
fn queries_use_nonblocking_persisted_refresh_phase() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let started = Instant::now();
    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.symbol("alpha")?.id.path ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed while refresh lock is held");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "query spent too long in request-path refresh work"
    );

    let trace = host
        .query_trace_view(
            &host.query_log_entries(QueryLogArgs {
                limit: Some(1),
                since: None,
                target: None,
                operation: Some("typescript.refreshWorkspace".to_string()),
                task_id: None,
                min_duration_ms: None,
            })[0]
                .id,
        )
        .expect("query trace should exist");
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "typescript.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("none".to_string()))
    );
}

#[test]
fn refresh_workspace_reloads_updated_persisted_notes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let stored = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::feature::alpha",
                        "kind": "function"
                    }],
                    "kind": "episodic",
                    "content": "alpha needs care during routing changes",
                    "trust": 0.8
                }),
                task_id: None,
            },
        )
        .expect("note should store");

    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");
    let initial_applied_fs_revision = workspace.applied_fs_revision();
    let initial_observed_fs_revision = workspace.observed_fs_revision();
    let mut snapshot = workspace
        .load_episodic_snapshot()
        .unwrap()
        .expect("persisted snapshot should exist");
    let entry = snapshot
        .entries
        .iter_mut()
        .find(|candidate| candidate.id.0 == stored.memory_id)
        .expect("stored note should be persisted");
    entry.anchors = vec![AnchorRef::Node(beta.clone())];
    workspace.persist_episodic(&snapshot).unwrap();

    host.refresh_workspace().unwrap();
    wait_until("persisted notes background reload", || {
        host.refresh_workspace().unwrap();
        test_session(&host)
            .notes
            .entry(&MemoryId(stored.memory_id.clone()))
            .is_some_and(|entry| entry.anchors.contains(&AnchorRef::Node(beta.clone())))
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("beta");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "routing changes",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed after snapshot reload");

    let entry = test_session(&host)
        .notes
        .entry(&MemoryId(stored.memory_id.clone()))
        .expect("stored note should remain in session memory");
    assert!(entry.anchors.contains(&AnchorRef::Node(beta)));

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(
        result.result[0]["entry"]["content"],
        "alpha needs care during routing changes"
    );
    assert_eq!(workspace.applied_fs_revision(), initial_applied_fs_revision);
    assert_eq!(
        workspace.observed_fs_revision(),
        initial_observed_fs_revision
    );
}

#[test]
fn refresh_workspace_reloads_updated_persisted_inference_without_fs_refresh() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.95,
            scope: Some(InferredEdgeScopeInput::Persisted),
            evidence: Some(vec!["persisted inference".to_string()]),
            task_id: Some("task:persist".to_string()),
        },
    )
    .expect("inferred edge should persist");

    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");
    let initial_applied_fs_revision = workspace.applied_fs_revision();
    let initial_observed_fs_revision = workspace.observed_fs_revision();
    let mut snapshot = workspace
        .load_inference_snapshot()
        .unwrap()
        .unwrap_or(InferenceSnapshot {
            records: Vec::new(),
        });
    assert_eq!(snapshot.records.len(), 1);
    snapshot.records[0].edge.target = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    workspace.persist_inference(&snapshot).unwrap();

    host.refresh_workspace().unwrap();
    wait_until("persisted inference background reload", || {
        host.refresh_workspace().unwrap();
        host.execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .map(|result| {
            result
                .result
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .any(|value| value == "demo::alpha")
        })
        .unwrap_or(false)
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed after inference reload");

    assert!(result
        .result
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::alpha"));
    assert_eq!(workspace.applied_fs_revision(), initial_applied_fs_revision);
    assert_eq!(
        workspace.observed_fs_revision(),
        initial_observed_fs_revision
    );
}

#[test]
fn convenience_symbol_query_returns_diagnostics() {
    let host = host_with_node(demo_node());

    let envelope = host
        .symbol_query(test_session(&host), "missing")
        .expect("symbol query should succeed");
    assert!(envelope.result.is_object() || envelope.result.is_null());
    assert!(envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "anchor_unresolved"));
    assert!(envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "anchor_unresolved")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["suggestedQueries"].as_array())
        .is_some_and(|queries| !queries.is_empty()));
    assert!(envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "anchor_unresolved")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism.search")));
}

#[test]
fn convenience_search_query_returns_structured_envelope() {
    let host = host_with_node(demo_node());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "main".to_string(),
                limit: Some(1),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");
    assert!(envelope.result.is_array());
    assert!(envelope.diagnostics.is_empty());
}

#[test]
fn ambiguous_symbol_queries_surface_ranked_narrowing_context() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .symbol_query(test_session(&host), "helper")
        .expect("symbol query should succeed");
    let diagnostic = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ambiguous_symbol")
        .expect("ambiguity diagnostic should be present");
    let ambiguity = diagnostic
        .data
        .as_ref()
        .and_then(|data| data["ambiguity"].as_object())
        .expect("ambiguity payload should be present");

    assert_eq!(ambiguity["candidateCount"].as_u64(), Some(2));
    assert_eq!(
        ambiguity["returned"]["id"]["path"].as_str(),
        envelope.result["id"]["path"].as_str()
    );
    assert!(envelope.result["id"]["path"]
        .as_str()
        .is_some_and(|path| !path.contains("::tests::")));
    assert!(ambiguity["candidates"][0]["suggestedQueries"]
        .as_array()
        .is_some_and(|queries| !queries.is_empty()));
    assert!(ambiguity["suggestedQueries"]
        .as_array()
        .is_some_and(|queries| queries.iter().any(|query| {
            query["label"]
                .as_str()
                .is_some_and(|label| label == "Focused Block")
        })));
}

#[test]
fn search_supports_module_and_task_scope_narrowing() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(root.join("src/alpha.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let module_envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: Some("demo::beta".to_string()),
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("module search should succeed");
    assert_eq!(
        module_envelope
            .result
            .as_array()
            .map(|results| results.len()),
        Some(1)
    );
    assert_eq!(
        module_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
    );

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Investigate helper collision" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Inspect beta helper",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::beta::helper",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let task_envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: Some(task_id),
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("task-scoped search should succeed");
    assert_eq!(
        task_envelope.result.as_array().map(|results| results.len()),
        Some(1)
    );
    assert_eq!(
        task_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
    );
}

#[test]
fn broad_noun_queries_prefer_callable_code_over_module_collisions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
pub mod planner;
"#,
    )
    .unwrap();
    fs::write(root.join("src/helpers.rs"), "pub fn bootstrap() {}\n").unwrap();
    fs::write(
        root.join("src/planner.rs"),
        "pub fn build_helper_plan() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::planner::build_helper_plan")
    );
    assert!(
        envelope.diagnostics.is_empty()
            || envelope
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ambiguous_search")
    );
}

#[test]
fn explicit_search_modes_prefer_callable_code_over_exact_module_collisions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod session;

pub fn session() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/session.rs"), "pub fn load() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Function");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session")
    );
}

#[test]
fn broad_noun_queries_prefer_non_test_code_over_test_name_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper_registry() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helper_registry")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results
            .iter()
            .any(|symbol| symbol["id"]["path"] == "demo::tests::helper")
    }));
}

#[test]
fn broad_noun_queries_overfetch_past_test_and_module_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod query_helpers;
pub mod discovery_helpers;

pub fn build_helper_plan() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
    pub fn bundle_helpers_keep_diagnostics_local_to_each_helper_call() {}
    pub fn discovery_helpers_surface_direct_where_used_and_entrypoints() {}
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_helpers.rs"),
        "pub fn hydrate_owner() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/discovery_helpers.rs"),
        "pub fn collect_related_owner() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(3),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::build_helper_plan")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("::tests::"))
        })
    }));
}

#[test]
fn broad_noun_queries_prefer_owner_module_over_path_inherited_helper_functions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn anchor_sort_key() {}
pub fn conflict_between() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Module");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers")
    );
}

#[test]
fn broad_noun_queries_deprioritize_replay_fixture_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod query_replay_cases;

pub fn build_helper_plan() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_replay_cases.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::build_helper_plan")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results
            .iter()
            .any(|symbol| symbol["id"]["path"] == "demo::query_replay_cases::helper")
    }));
}

#[test]
fn broad_noun_queries_deprioritize_dependency_lockfile_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{
  "packages": {
    "node_modules/@babel/helper-globals": {
      "version": "7.28.0"
    }
  }
}"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helper")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("@babel/helper-globals"))
        })
    }));
}

#[test]
fn broad_noun_queries_prefer_exact_modules_over_exact_field_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod session;

pub struct QueryHost {
    pub session: usize,
}
"#,
    )
    .unwrap();
    fs::write(root.join("src/session.rs"), "pub fn load() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Module");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session")
    );
}

#[test]
fn explicit_search_modes_can_prefer_behavioral_owners_without_behavioral_strategy() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "memory recall".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert!(envelope
        .result
        .as_array()
        .is_some_and(|results| results.iter().take(3).any(|symbol| {
            symbol["ownerHint"]["kind"].as_str() == Some("read")
                && symbol["id"]["path"]
                    .as_str()
                    .is_some_and(|path| path.contains("memory_recall"))
        })));
}

#[test]
fn direct_search_merges_behavioral_owner_hints_for_same_symbol() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn session_view() {
    let current_session = read_session_state();
    assert!(!current_session.is_empty());
}

fn read_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let session_view = envelope
        .result
        .as_array()
        .and_then(|results| {
            results
                .iter()
                .find(|symbol| symbol["id"]["path"].as_str() == Some("demo::session_view"))
        })
        .expect("session_view should be returned");
    assert_eq!(session_view["ownerHint"]["kind"].as_str(), Some("read"));
}

#[test]
fn behavioral_owner_search_prefers_deeper_session_implementations_over_surface_wrappers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod dashboard;
mod resources;
mod runtime;

pub use dashboard::dashboard_session_view;
pub use resources::session_resource_link;
pub use runtime::load_session_memory;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

fn fetch_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/dashboard.rs"),
        r#"
pub fn dashboard_session_view() -> &'static str {
    "session"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let top_paths = envelope
        .result
        .as_array()
        .expect("behavioral search should return an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        top_paths
            .iter()
            .all(|path| path.starts_with("demo::runtime::")),
        "expected deeper runtime implementations to outrank surface wrappers, got {top_paths:?}"
    );
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::resources::session_resource_link"
                || path == &"demo::dashboard::dashboard_session_view"),
        "expected surface wrappers to rank below runtime implementations, got {top_paths:?}"
    );
}

#[test]
fn behavioral_owner_search_deprioritizes_schema_examples_for_broad_read_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod resources;
mod runtime;
mod schema_examples;

pub use resources::session_resource_link;
pub use runtime::{fetch_session_state, load_session_history, load_session_memory};
pub use schema_examples::session_payload_example;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

pub fn fetch_session_state() -> &'static str {
    "ready"
}

pub fn load_session_history() -> &'static str {
    fetch_session_state()
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r#"
pub fn session_payload_example() -> &'static str {
    "{\"currentTask\":\"demo-session\"}"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(6),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let top_paths = envelope
        .result
        .as_array()
        .expect("behavioral search should return an array")
        .iter()
        .take(3)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        top_paths
            .iter()
            .all(|path| path.starts_with("demo::runtime::")),
        "expected runtime implementations to outrank schema/resource surfaces, got {top_paths:?}"
    );
}

#[test]
fn ambiguity_payload_exposes_buckets_for_broad_first_hop_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod dashboard;
mod resources;
mod runtime;
mod schema_examples;

pub use dashboard::dashboard_session_view;
pub use resources::session_resource_link;
pub use runtime::load_session_memory;
pub use schema_examples::session_payload_example;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

fn fetch_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/dashboard.rs"),
        r#"
pub fn dashboard_session_view() -> &'static str {
    "session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r#"
pub fn session_payload_example() -> &'static str {
    "{\"currentTask\":\"demo-session\"}"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(8),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let ambiguity = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ambiguous_search")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["ambiguity"].as_object())
        .expect("ambiguous search payload should be present");

    let why = ambiguity["why"].as_array().expect("why should be an array");
    assert!(why.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|line| line.contains("Broad-query bucketing grouped candidates"))
    }));

    let candidates = ambiguity["candidates"]
        .as_array()
        .expect("ambiguity candidates should be an array");
    assert_eq!(candidates[0]["bucket"], "implementation");
    assert!(candidates
        .iter()
        .any(|candidate| candidate["bucket"] == "surface"));
    assert!(candidates
        .iter()
        .any(|candidate| candidate["bucket"] != "implementation"));
}

#[test]
fn behavioral_owner_search_ignores_excerpt_only_schema_example_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod runtime;
pub mod schema_examples;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn lookup_session_state() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r##"
pub fn read_payload_example() {
    let _payload = r#"{"helper":true}"#;
}
"##,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: None,
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::runtime::lookup_session_state")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("schema_examples"))
        })
    }));
}

#[test]
fn explicit_callable_behavioral_owner_search_prefers_functions_over_exact_module_nouns() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Function");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
}

#[test]
fn broad_helper_queries_deprioritize_generic_query_helpers_utilities() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
pub mod query_helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_helpers.rs"),
        r#"
pub fn compact_owner_candidate_excerpts() {}
pub fn helper_candidate_summary() {}
pub fn next_reads() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path.starts_with("demo::query_helpers::")),
        "expected generic query_helpers utilities to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_deprioritize_low_level_helpers_module_utilities() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}

pub fn anchor_sort_key() {}
pub fn conflict_between() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::helpers::anchor_sort_key"
                || path == &"demo::helpers::conflict_between"),
        "expected low-level helpers.rs utilities to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_deprioritize_internal_helpers_module_plumbing() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}

pub(crate) fn derived_event_meta() {}
fn expire_claims_locked() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::helpers::derived_event_meta"
                || path == &"demo::helpers::expire_claims_locked"),
        "expected internal helpers.rs plumbing to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_surface_weak_match_diagnostics_for_internal_modules() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub(crate) fn derived_event_meta() {}
fn expire_claims_locked() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert!(envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "weak_search_match"));
    let weak = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "weak_search_match")
        .expect("weak_search_match diagnostic should be present");
    assert!(weak
        .data
        .as_ref()
        .and_then(|data| data["reason"].as_str())
        .is_some_and(|reason| !reason.trim().is_empty()));
}

#[test]
fn broad_implementation_search_deprioritizes_lib_rs_facade_wrappers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod session;

pub fn index_workspace_session() {
    session::load_session_state();
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/session.rs"),
        r#"
pub fn load_session_state() {
    let session_state = "ready";
    assert!(!session_state.is_empty());
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session::load_session_state")
    );
}

#[test]
fn file_around_truncation_emits_actionable_diagnostic() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.file("src/recall.rs").around({
  line: 8,
  before: 0,
  after: 20,
  maxChars: 80,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["truncated"], true);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "result_truncated"
                && diagnostic
                    .data
                    .as_ref()
                    .and_then(|data| data.get("operation"))
                    .and_then(Value::as_str)
                    == Some("fileAround")
        })
        .expect("fileAround truncation diagnostic");
    assert!(diagnostic.message.contains("raise `maxChars`"));
    assert_eq!(
        diagnostic
            .data
            .as_ref()
            .and_then(|data| data.get("maxChars"))
            .and_then(Value::as_u64),
        Some(80)
    );
}

#[test]
fn search_resource_payload_surfaces_ambiguity_context() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(root.join("src/alpha.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(test_session(&host), "prism://search/helper", "helper")
        .expect("search resource should succeed");

    assert!(payload.ambiguity.is_some());
    assert_eq!(
        payload
            .ambiguity
            .as_ref()
            .and_then(|ambiguity| ambiguity.returned.as_ref())
            .map(|symbol| symbol.id.path.as_str()),
        payload
            .results
            .first()
            .map(|symbol| symbol.id.path.as_str())
    );
    assert!(payload
        .suggested_queries
        .iter()
        .any(|query| query.label == "Focused Block"));
}

#[test]
fn plans_resource_payload_surfaces_filters_and_root_nodes() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Migrate persistence storage semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanNodeCreate,
            payload: json!({ "planId": plan_id, "title": "Classify authoritative tables" }),
            task_id: None,
        },
    )
    .unwrap();

    let payload = host
        .plans_resource_value(
            test_session(&host),
            "prism://plans?contains=persistence&limit=1",
        )
        .expect("plans resource should succeed");

    assert_eq!(payload.contains.as_deref(), Some("persistence"));
    assert_eq!(payload.page.returned, 1);
    assert_eq!(payload.plans.len(), 1);
    assert_eq!(payload.plans[0].summary.actionable_nodes, 1);
    assert!(payload
        .related_resources
        .iter()
        .any(|link| link.uri == "prism://plans?contains=persistence"));
    assert_eq!(payload.plans[0].root_node_ids, vec!["coord-task:1"]);
}

#[test]
fn first_mutation_auto_creates_session_task() {
    let host = host_with_node(demo_node());

    let memory = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "kind": "episodic",
                    "content": "remember this",
                    "trust": 0.8
                }),
                task_id: None,
            },
        )
        .expect("note should store");

    assert!(memory.memory_id.starts_with("memory:"));
    let task = test_session(&host)
        .current_task()
        .expect("task should be created");
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.task, task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);
}

#[test]
fn recalls_session_memory_for_symbol_focus() {
    let host = host_with_node(demo_node());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::main",
                    "kind": "function"
                }],
                "kind": "episodic",
                "content": "main previously regressed on null handling",
                "trust": 0.9
            }),
            task_id: None,
        },
    )
    .expect("note should store");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "null",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    assert_eq!(
        result.result[0]["entry"]["content"],
        "main previously regressed on null handling"
    );
    assert_eq!(result.result[0]["entry"]["kind"], "Episodic");
}

#[test]
fn memory_recall_respects_kinds_and_since_filters() {
    let host = host_with_node(demo_node());

    let mut old_structural =
        MemoryEntry::new(MemoryKind::Structural, "main changes require a migration");
    old_structural.anchors = vec![AnchorRef::Node(NodeId::new(
        "demo",
        "demo::main",
        NodeKind::Function,
    ))];
    old_structural.created_at = 10;
    old_structural.source = MemorySource::User;
    old_structural.trust = 1.0;
    test_session(&host).notes.store(old_structural).unwrap();

    let mut fresh_semantic =
        MemoryEntry::new(MemoryKind::Semantic, "main often flakes during retries");
    fresh_semantic.anchors = vec![AnchorRef::Node(NodeId::new(
        "demo",
        "demo::main",
        NodeKind::Function,
    ))];
    fresh_semantic.created_at = 50;
    fresh_semantic.source = MemorySource::System;
    fresh_semantic.trust = 0.8;
    test_session(&host).notes.store(fresh_semantic).unwrap();

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  kinds: ["semantic"],
  since: 20,
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(result.result[0]["entry"]["kind"], "Semantic");
    assert_eq!(
        result.result[0]["entry"]["content"],
        "main often flakes during retries"
    );
}

#[test]
fn memory_outcomes_support_filtered_history_queries() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:1"),
                ts: 5,
                actor: EventActor::System,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "system saw alpha fail".into(),
            evidence: vec![],
            metadata: Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:2"),
                ts: 15,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "agent saw alpha fail".into(),
            evidence: vec![],
            metadata: Value::Null,
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.outcomes({
  focus: sym ? [sym] : [],
  taskId: "task:alpha",
  kinds: ["failure"],
  result: "failure",
  actor: "agent",
  since: 10,
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory outcome query should succeed");

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(result.result[0]["summary"], "agent saw alpha fail");
}

#[test]
fn finish_task_writes_summary_memory_clears_session_task_and_updates_task_resource() {
    let host = host_with_node(demo_node());
    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .expect("task should start");

    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated main behavior".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: None,
        },
    )
    .expect("validation outcome should store");

    let result = host
        .finish_task(
            test_session(&host).as_ref(),
            PrismFinishTaskArgs {
                summary: "Closed out main investigation with validation coverage".to_string(),
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }]),
                task_id: None,
            },
        )
        .expect("finish task should succeed");

    assert_eq!(result.task_id, task.0);
    assert_eq!(test_session(&host).current_task(), None);
    assert_eq!(result.journal.disposition, "completed");
    assert_eq!(
        result.journal.summary.final_summary.as_deref(),
        Some("Closed out main investigation with validation coverage")
    );

    let replay = host.current_prism().resume_task(&task);
    let closing = replay
        .events
        .iter()
        .find(|event| event.meta.id.0 == result.event_id)
        .expect("closing outcome should be present");
    assert_eq!(closing.kind, OutcomeKind::NoteAdded);
    assert_eq!(
        closing.metadata["taskLifecycle"]["disposition"],
        "completed"
    );
    assert_eq!(
        closing.metadata["taskLifecycle"]["memoryId"],
        result.memory_id
    );

    let memory = test_session(&host)
        .notes
        .entry(&MemoryId(result.memory_id.clone()))
        .expect("summary memory should exist");
    assert_eq!(
        memory.content,
        "Closed out main investigation with validation coverage"
    );
    assert_eq!(memory.metadata["taskLifecycle"]["disposition"], "completed");

    let resource = host
        .task_resource_value(
            test_session(&host).as_ref(),
            &task_resource_uri(&result.task_id),
            &task,
        )
        .expect("task resource should load");
    assert_eq!(resource.journal.disposition, "completed");
    assert_eq!(
        resource.journal.summary.final_summary.as_deref(),
        Some("Closed out main investigation with validation coverage")
    );
}

#[test]
fn task_journal_query_reports_missing_validation_and_related_memory() {
    let main = demo_node();
    let main_id = main.id.clone();
    let task_id = TaskId::new("task:journal");

    let mut graph = Graph::default();
    graph.nodes.insert(main_id.clone(), main);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:journal:plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(main_id.clone())],
            kind: OutcomeKind::PlanCreated,
            result: OutcomeResult::Success,
            summary: "Investigate main".into(),
            evidence: Vec::new(),
            metadata: json!({ "tags": ["bug"] }),
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:journal:patch"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(main_id.clone())],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Partial,
            summary: "patched main".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();

    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::main", NodeKind::Function)]);
    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let mut memory = MemoryEntry::new(
        MemoryKind::Structural,
        "main changes should always get a regression check",
    );
    memory.anchors = vec![AnchorRef::Node(main_id)];
    memory.trust = 0.9;
    test_session(&host).notes.store(memory).unwrap();

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.taskJournal("task:journal", { eventLimit: 10, memoryLimit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(result.result["taskId"], "task:journal");
    assert_eq!(result.result["disposition"], "open");
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "missing_validation"));
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "missing_close_summary"));
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["data"]["nextAction"].as_str().is_some()));
    assert_eq!(
        result.result["relatedMemory"][0]["entry"]["content"],
        "main changes should always get a regression check"
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missing_validation"));
    assert!(result.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .data
            .as_ref()
            .and_then(|data| data["nextAction"].as_str())
            .is_some()
    }));
}

#[test]
fn call_graph_depth_limit_diagnostic_includes_next_action() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return sym?.callGraph(50);
"#,
            QueryLanguage::Ts,
        )
        .expect("call graph query should succeed");

    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "depth_limited"));
    assert!(result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "depth_limited")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism.callGraph")));
}

#[test]
fn abandon_task_suppresses_unresolved_failure_diagnostic() {
    let host = host_with_node(demo_node());
    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            Vec::new(),
            None,
        )
        .expect("task should start");

    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FailureObserved,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            summary: "main is blocked by upstream".to_string(),
            result: Some(OutcomeResultInput::Failure),
            evidence: None,
            task_id: None,
        },
    )
    .expect("failure outcome should store");

    let result = host
        .abandon_task(
            test_session(&host).as_ref(),
            PrismFinishTaskArgs {
                summary: "Stopped after upstream dependency failure".to_string(),
                anchors: None,
                task_id: None,
            },
        )
        .expect("abandon task should succeed");

    assert_eq!(result.task_id, task.0);
    assert_eq!(result.journal.disposition, "abandoned");
    assert!(result
        .journal
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "unresolved_failure"));
    assert_eq!(
        result.journal.summary.final_summary.as_deref(),
        Some("Stopped after upstream dependency failure")
    );
}

#[test]
fn explicit_start_task_sets_session_default_and_logs_plan() {
    let host = host_with_node(demo_node());

    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .expect("task should start");

    assert_eq!(test_session(&host).current_task(), Some(task.clone()));
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::PlanCreated);
    assert_eq!(replay.events[0].summary, "Investigate main");
    assert_eq!(replay.events[0].metadata["tags"][0], "bug");

    let journal = host
        .execute(
            test_session(&host),
            &format!(
                "return prism.taskJournal(\"{}\", {{ eventLimit: 10, memoryLimit: 5 }});",
                task.0
            ),
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn start_task_can_bind_directly_to_coordination_task() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Dogfood coordination task session binding" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let started = host
        .start_task(
            test_session(&host).as_ref(),
            None,
            vec!["ux".to_string()],
            Some(task_id.clone()),
        )
        .expect("task should bind to coordination task");

    assert_eq!(started.0.as_str(), task_id);
    let current_task = test_session(&host)
        .current_task_state()
        .expect("current task should be set");
    assert_eq!(current_task.id, started);
    assert_eq!(current_task.description.as_deref(), Some("Edit alpha"));
    assert_eq!(current_task.tags, vec!["ux".to_string()]);
    assert_eq!(
        current_task.coordination_task_id.as_deref(),
        Some(task_id.as_str())
    );

    let replay = host.current_prism().resume_task(&started);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].summary, "Edit alpha");
    assert_eq!(replay.events[0].metadata["coordinationTaskId"], task_id);

    let session = host
        .session_resource_value(test_session(&host).as_ref())
        .expect("session resource should load");
    assert_eq!(
        session
            .current_task
            .as_ref()
            .and_then(|task| task.coordination_task_id.as_deref()),
        Some(task_id.as_str())
    );
}

#[test]
fn coordination_task_journal_falls_back_to_task_title_without_outcomes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Dogfood coordination task journal metadata fallback" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Validate persistence task",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap();

    let journal = host
        .execute(
            test_session(&host),
            &format!(
                "return prism.taskJournal(\"{task_id}\", {{ eventLimit: 10, memoryLimit: 5 }});"
            ),
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(journal.result["taskId"], task_id);
    assert_eq!(journal.result["description"], "Validate persistence task");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn task_journal_without_outcome_history_does_not_claim_missing_plan() {
    let host = host_with_node(demo_node());
    let task = TaskId::new("task:empty");
    test_session(&host).set_current_task(
        task.clone(),
        Some("Investigate empty task".to_string()),
        Vec::new(),
        None,
    );

    let journal = host
        .execute(
            test_session(&host),
            r#"
return prism.taskJournal("task:empty", { eventLimit: 10, memoryLimit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(journal.result["taskId"], "task:empty");
    assert_eq!(journal.result["disposition"], "active");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn explicit_task_override_does_not_replace_session_default() {
    let host = host_with_node(demo_node());
    let active = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Primary task".to_string()),
            Vec::new(),
            None,
        )
        .expect("task should start");

    let explicit = TaskId::new("task:secondary:99");
    let event = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FailureObserved,
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                summary: "secondary failure".to_string(),
                result: Some(OutcomeResultInput::Failure),
                evidence: None,
                task_id: Some(explicit.0.to_string()),
            },
        )
        .expect("outcome should store");

    assert_eq!(test_session(&host).current_task(), Some(active));
    let replay = host.current_prism().resume_task(&explicit);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].meta.id.0, event.event_id);
}

#[test]
fn cloned_servers_isolate_session_state_but_share_persisted_state() {
    let server = server_with_node(demo_node());
    let client_a = server.clone();
    let client_b = server.clone();

    assert!(Arc::ptr_eq(&client_a.host, &client_b.host));
    assert_ne!(
        client_a.session.session_id().0,
        client_b.session.session_id().0
    );

    client_a
        .host
        .configure_session_without_refresh(
            client_a.session.as_ref(),
            PrismConfigureSessionArgs {
                limits: Some(QueryLimitsInput {
                    max_result_nodes: Some(3),
                    max_call_graph_depth: None,
                    max_output_json_bytes: None,
                }),
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-a".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();
    let task_a = client_a
        .host
        .start_task(
            client_a.session.as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .unwrap();

    let session_a = client_a
        .host
        .session_resource_value(client_a.session.as_ref())
        .unwrap();
    let session_b = client_b
        .host
        .session_resource_value(client_b.session.as_ref())
        .unwrap();
    assert_eq!(session_a.current_agent.as_deref(), Some("agent-a"));
    assert_eq!(
        session_a
            .current_task
            .as_ref()
            .map(|task| task.task_id.as_str()),
        Some(task_a.0.as_str())
    );
    assert_eq!(session_a.limits.max_result_nodes, 3);
    assert_eq!(session_b.current_agent, None);
    assert!(session_b.current_task.is_none());
    assert_eq!(
        session_b.limits.max_result_nodes,
        QueryLimits::default().max_result_nodes
    );

    let session_edge = client_a
        .host
        .store_inferred_edge(
            client_a.session.as_ref(),
            PrismInferEdgeArgs {
                kind: "calls".to_string(),
                source: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                target: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                confidence: 0.8,
                scope: Some(InferredEdgeScopeInput::SessionOnly),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();
    assert!(client_a
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(session_edge.edge_id.clone()))
        .is_some());
    assert!(client_b
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(session_edge.edge_id.clone()))
        .is_none());

    let persisted_edge = client_a
        .host
        .store_inferred_edge(
            client_a.session.as_ref(),
            PrismInferEdgeArgs {
                kind: "calls".to_string(),
                source: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                target: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                confidence: 0.9,
                scope: Some(InferredEdgeScopeInput::Persisted),
                evidence: Some(vec!["shared persisted inference".to_string()]),
                task_id: None,
            },
        )
        .unwrap();
    assert!(client_b
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(persisted_edge.edge_id.clone()))
        .is_some());

    client_a
        .host
        .execute(
            Arc::clone(&client_a.session),
            r#"return prism.symbol("main")?.id.path;"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    client_b
        .host
        .execute(
            Arc::clone(&client_b.session),
            r#"return prism.symbol("main")?.id.path;"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    let query_log = client_a.host.query_log_entries(QueryLogArgs {
        limit: Some(10),
        since: None,
        target: None,
        operation: None,
        task_id: None,
        min_duration_ms: None,
    });
    assert!(query_log
        .iter()
        .any(|entry| entry.session_id == client_a.session.session_id().0));
    assert!(query_log
        .iter()
        .any(|entry| entry.session_id == client_b.session.session_id().0));
}

#[test]
fn workspace_coordination_persistence_records_mcp_session_scope() {
    let root = temp_workspace();
    let workspace = index_workspace_session(&root).unwrap();
    let host = host_with_session(workspace);
    let session_a = host.new_session_state();
    let session_b = host.new_session_state();

    host.store_coordination(
        session_a.as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Track session-scoped persistence context"
            }),
            task_id: None,
        },
    )
    .unwrap();
    let cache = root.join(".prism").join("cache.db");
    let mut store = SqliteStore::open(&cache).unwrap();
    let context_after_a = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("coordination mutation log should retain a latest context");
    assert_eq!(
        context_after_a.session_id.as_deref(),
        Some(session_a.session_id().0.as_str())
    );
    drop(store);

    host.store_coordination(
        session_b.as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Track a second session-scoped persistence context"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let mut store = SqliteStore::open(&cache).unwrap();
    let logged_context = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("coordination mutation log should retain a latest context");
    assert_eq!(
        logged_context.session_id.as_deref(),
        Some(session_b.session_id().0.as_str())
    );
    assert_eq!(store.load_coordination_events().unwrap().len(), 2);

    drop(store);
    drop(host);
    let _ = fs::remove_dir_all(root);
}
