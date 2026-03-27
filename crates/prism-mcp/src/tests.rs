use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

use super::*;
use prism_agent::{InferenceSnapshot, InferredEdgeScope};
use prism_core::{index_workspace_session, index_workspace_session_with_curator};
use prism_curator::{
    CandidateEdge, CandidateMemory, CandidateMemoryEvidence, CandidateRiskSummary,
    CandidateValidationRecipe, CuratorBackend, CuratorContext, CuratorJob, CuratorProposal,
    CuratorRun,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId,
    NodeKind, Span, TaskId,
};
use prism_memory::{
    MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeMemory, OutcomeResult, RecallQuery,
};
use prism_store::Graph;
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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Ship coordination" }),
            task_id: None,
        })
        .unwrap();
    assert_eq!(plan.state["goal"], "Ship coordination");

    let task = host
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let claim = host
        .store_claim(PrismClaimArgs {
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
        })
        .unwrap();
    assert!(claim.claim_id.is_some());

    let artifact = host
        .store_artifact(PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task.state["id"].as_str().unwrap(),
                "diffRef": "patch:1"
            }),
            task_id: None,
        })
        .unwrap();
    assert!(artifact.artifact_id.is_some());

    let execution = QueryExecution::new(
        host.clone(),
        host.current_prism(),
        host.begin_query_run("test", "dispatch plan"),
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
fn mcp_returns_structured_coordination_rejections_and_persists_them() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Ship reviewed change",
                "policy": { "requireReviewForCompletion": true }
            }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskCreate,
            payload: json!({
                "planId": plan_id,
                "title": "Edit alpha",
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }]
            }),
            task_id: None,
        })
        .unwrap();

    let rejected = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskUpdate,
            payload: json!({
                "taskId": task.state["id"].as_str().unwrap(),
                "status": "completed"
            }),
            task_id: None,
        })
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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Ship reviewed change",
                "policy": { "requireReviewForCompletion": true }
            }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let rejected = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskUpdate,
            payload: json!({
                "taskId": task_id.clone(),
                "status": "completed"
            }),
            task_id: None,
        })
        .unwrap();
    assert!(rejected.rejected);

    let execution = QueryExecution::new(
        host.clone(),
        host.current_prism(),
        host.begin_query_run("test", "dispatch policy violations"),
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
        .configure_session(PrismConfigureSessionArgs {
            limits: None,
            current_task_id: None,
            current_task_description: None,
            current_task_tags: None,
            clear_current_task: None,
            current_agent: Some("agent-a".to_string()),
            clear_current_agent: None,
        })
        .unwrap();
    assert_eq!(session.current_agent.as_deref(), Some("agent-a"));

    let plan = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Bind agent identity" }),
            task_id: None,
        })
        .unwrap();
    let task = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskCreate,
            payload: json!({
                "planId": plan.state["id"].as_str().unwrap(),
                "title": "Edit alpha"
            }),
            task_id: None,
        })
        .unwrap();
    assert_eq!(task.state["assignee"], "agent-a");

    let claims = host.current_prism().coordination_snapshot().claims;
    assert!(claims.is_empty());
}

#[test]
fn mcp_plan_update_completes_plan_and_closed_plan_rejects_new_claims() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Single pass coordination" }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    host.store_coordination(PrismCoordinationArgs {
        kind: CoordinationMutationKindInput::TaskUpdate,
        payload: json!({
            "taskId": task_id.clone(),
            "status": "completed"
        }),
        task_id: None,
    })
    .unwrap();

    let completed_plan = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanUpdate,
            payload: json!({
                "planId": plan_id.clone(),
                "status": "completed"
            }),
            task_id: None,
        })
        .unwrap();
    assert!(!completed_plan.rejected);
    assert_eq!(completed_plan.state["status"], "Completed");

    let rejected_claim = host
        .store_claim(PrismClaimArgs {
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
        })
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
    assert_eq!(tool_names.len(), 3);
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
    assert!(api_reference.contains("PRISM Query API"));
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
        .any(|tool| tool["name"] == "prism_query" && tool["exampleInput"]["language"] == "ts"));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn stdio_proxy_forwards_to_streamable_http_upstream() {
    let upstream = server_with_node(demo_node());
    let service: StreamableHttpService<PrismMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(upstream.clone()),
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
    let upstream_task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let proxy = crate::proxy_server::ProxyMcpServer::connect(format!("http://{addr}/mcp"))
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

#[test]
fn simple_mode_disables_coordination_host_paths() {
    let host = QueryHost::new_with_limits_and_features(
        Prism::new(Graph::default()),
        QueryLimits::default(),
        PrismMcpFeatures::simple(),
    );

    let error = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Ship coordination" }),
            task_id: None,
        })
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination workflow mutations are disabled"));

    let execution = QueryExecution::new(
        host.clone(),
        host.current_prism(),
        host.begin_query_run("test", "dispatch simple-mode plan"),
    );
    let error = execution
        .dispatch("plan", r#"{ "planId": "plan:1" }"#)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination workflow queries are disabled"));
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
    assert_eq!(tool_names.len(), 3);
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
                    "context": "Missing anchors",
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
        "prism_mutate action `validation_feedback` is missing required field `input.anchors`"
    ));
    assert!(message
        .contains("required fields: anchors, context, prismSaid, actuallyTrue, category, verdict"));
    assert!(message.contains(
        "Inspect via prism.tool(\"prism_mutate\")?.actions.find((action) => action.action === \"validation_feedback\")"
    ));

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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Coordinate request handling" }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskCreate,
            payload: json!({
                "planId": plan_id,
                "title": "Implement request flow",
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::handle_request",
                    "kind": "function"
                }]
            }),
            task_id: None,
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let execution = QueryExecution::new(
        host.clone(),
        host.current_prism(),
        host.begin_query_run("test", "dispatch drift candidates"),
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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Coordinate alpha",
                "policy": {
                    "requireReviewForCompletion": true,
                    "maxParallelEditorsPerAnchor": 1
                }
            }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = writer
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    writer
        .store_claim(PrismClaimArgs {
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
        })
        .unwrap();

    writer
        .store_artifact(PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task.state["id"].as_str().unwrap(),
                "diffRef": "patch:alpha"
            }),
            task_id: None,
        })
        .unwrap();

    let result = host
        .execute(
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
        0
    );
    assert_eq!(
        result.result["inbox"]["pendingReviews"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result.result["context"]["task"]["id"], task_id);
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
        .configure_session(PrismConfigureSessionArgs {
            limits: None,
            current_task_id: None,
            current_task_description: None,
            current_task_tags: None,
            clear_current_task: None,
            current_agent: Some("agent-b".to_string()),
            clear_current_agent: None,
        })
        .unwrap();

    let plan = host_a
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Coordinate alpha across sessions",
                "policy": {
                    "requireReviewForCompletion": true,
                    "maxParallelEditorsPerAnchor": 1
                }
            }),
            task_id: None,
        })
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host_a
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let first_claim = host_a
        .store_claim(PrismClaimArgs {
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
        })
        .unwrap();
    assert!(first_claim.claim_id.is_some());

    let blocked_neighbor_claim = host_b
        .store_claim(PrismClaimArgs {
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
        })
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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::Handoff,
            payload: json!({
                "taskId": task_id.clone(),
                "toAgent": "agent-b",
                "summary": "handoff alpha implementation to agent-b"
            }),
            task_id: None,
        })
        .unwrap();

    let handed_off = host_b
        .execute(
            &format!(r#"return prism.task("{task_id}");"#),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(handed_off.result["assignee"], Value::Null);
    assert_eq!(handed_off.result["pendingHandoffTo"], "agent-b");
    assert_eq!(handed_off.result["status"], "Blocked");

    let blocked_update = host_b
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskUpdate,
            payload: json!({
                "taskId": task_id.clone(),
                "status": "in-progress"
            }),
            task_id: None,
        })
        .unwrap();
    assert!(blocked_update.rejected);
    assert!(blocked_update
        .violations
        .iter()
        .any(|violation| violation.code == "handoff_pending"));

    host_b
        .configure_session(PrismConfigureSessionArgs {
            limits: None,
            current_task_id: None,
            current_task_description: None,
            current_task_tags: None,
            clear_current_task: None,
            current_agent: None,
            clear_current_agent: Some(true),
        })
        .unwrap();
    let missing_agent = host_b
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::HandoffAccept,
            payload: json!({
                "taskId": task_id.clone(),
                "agent": "agent-b"
            }),
            task_id: None,
        })
        .unwrap();
    assert!(missing_agent.rejected);
    assert!(missing_agent
        .violations
        .iter()
        .any(|violation| violation.code == "agent_identity_required"));

    host_b
        .configure_session(PrismConfigureSessionArgs {
            limits: None,
            current_task_id: None,
            current_task_description: None,
            current_task_tags: None,
            clear_current_task: None,
            current_agent: Some("agent-b".to_string()),
            clear_current_agent: None,
        })
        .unwrap();

    let accepted = host_b
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::HandoffAccept,
            payload: json!({
                "taskId": task_id.clone(),
                "agent": "agent-b"
            }),
            task_id: None,
        })
        .unwrap();
    assert_eq!(accepted.state["assignee"], "agent-b");
    assert_eq!(accepted.state["pendingHandoffTo"], Value::Null);
    assert_eq!(accepted.state["status"], "Ready");

    let second_claim = host_b
        .store_claim(PrismClaimArgs {
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
        })
        .unwrap();
    assert!(second_claim.claim_id.is_some());

    let artifact = host_b
        .store_artifact(PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task.state["id"].as_str().unwrap(),
                "diffRef": "patch:alpha-shared"
            }),
            task_id: None,
        })
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    host_a
        .store_artifact(PrismArtifactArgs {
            action: ArtifactActionInput::Review,
            payload: json!({
                "artifactId": artifact_id,
                "verdict": "approved",
                "summary": "reviewed after handoff"
            }),
            task_id: None,
        })
        .unwrap();

    let completed = host_b
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskUpdate,
            payload: json!({
                "taskId": task_id.clone(),
                "status": "completed"
            }),
            task_id: None,
        })
        .unwrap();
    assert_eq!(completed.state["status"], "Completed");

    let final_state = host_a
        .execute(
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
        .promote_curator_edge(PrismCuratorPromoteEdgeArgs {
            job_id: job_id.clone(),
            proposal_index: 0,
            scope: Some(InferredEdgeScopeInput::Persisted),
            note: Some("accepted after review".into()),
            task_id: Some("task:promotion".into()),
        })
        .unwrap();
    assert!(promoted.edge_id.is_some());

    let proposal = host
        .execute(
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
        .promote_curator_memory(PrismCuratorPromoteMemoryArgs {
            job_id: job_id.clone(),
            proposal_index: 0,
            trust: None,
            note: Some("promote repeated routing knowledge".into()),
            task_id: Some("task:curator-memory".into()),
        })
        .expect("memory promotion should succeed");
    assert!(promoted.memory_id.is_some());
    assert!(promoted.edge_id.is_none());

    let proposal = host
        .execute(
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

    host.promote_curator_memory(PrismCuratorPromoteMemoryArgs {
        job_id,
        proposal_index: 0,
        trust: None,
        note: Some("promote semantic context".into()),
        task_id: Some("task:semantic-memory".into()),
    })
    .expect("semantic memory promotion should succeed");

    let result = host
        .execute(
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
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Change alpha safely" }),
            task_id: None,
        })
        .unwrap();
    let task = host
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();
    let artifact = host
        .store_artifact(PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task_id,
                "diffRef": "patch:alpha-risk"
            }),
            task_id: None,
        })
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    let before = host
        .execute(
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

    host.promote_curator_memory(PrismCuratorPromoteMemoryArgs {
        job_id: job_id.clone(),
        proposal_index: 0,
        trust: None,
        note: Some("promote validation recipe".into()),
        task_id: Some(task_id.clone()),
    })
    .expect("validation recipe promotion should succeed");
    host.promote_curator_memory(PrismCuratorPromoteMemoryArgs {
        job_id,
        proposal_index: 1,
        trust: None,
        note: Some("promote risk summary".into()),
        task_id: Some(task_id.clone()),
    })
    .expect("risk summary promotion should succeed");

    let after = host
        .execute(
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
        .reject_curator_proposal(PrismCuratorRejectProposalArgs {
            job_id: job_id.clone(),
            proposal_index: 0,
            reason: Some("not enough evidence".into()),
            task_id: Some("task:review".into()),
        })
        .unwrap();
    assert!(rejected.edge_id.is_none());

    let proposal = host
        .execute(
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
fn symbol_views_expose_source_locations_and_excerpts() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
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
        .execute(
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
        .execute(
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
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|tool| tool["toolName"] == "prism_mutate"));
    assert!(tools
        .iter()
        .any(|tool| tool["exampleInput"]["action"] == "validation_feedback"));

    let mutate = &result.result["mutate"];
    assert_eq!(mutate["toolName"], "prism_mutate");
    assert_eq!(
        mutate["actions"].as_array().map(|items| items.len()),
        Some(13)
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
            "anchors",
            "context",
            "prismSaid",
            "actuallyTrue",
            "category",
            "verdict",
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

    assert!(result.result["missing"].is_null());
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
    assert_eq!(recent[0]["operations"][0], "fileAround");
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
    assert_eq!(
        result.result["trace"]["entry"]["operations"][0],
        "fileAround"
    );
    let phases = result.result["trace"]["phases"]
        .as_array()
        .expect("trace phases");
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0]["operation"], "fileAround");
    assert_eq!(phases[0]["success"], true);
}

#[test]
fn prism_query_log_touched_prefers_semantic_targets() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        r#"
return prism.runtimeLogs({ level: "WARN", limit: 2 });
"#,
        QueryLanguage::Ts,
    )
    .expect("runtime log query should succeed");

    let result = host
        .execute(
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
fn prism_query_errors_include_js_message_and_stack() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            r#"
throw new Error("boom");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(
        message.contains("javascript query evaluation failed"),
        "{message}"
    );
    assert!(
        !message.contains("Exception generated by QuickJS"),
        "{message}"
    );
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
fn prism_runtime_views_prefer_structured_runtime_state() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind health server");
    let addr = listener.local_addr().expect("local addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept health check");
        let mut request = [0_u8; 512];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .expect("write health response");
    });

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

    server.join().expect("health server should exit cleanly");
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
        .execute("return prism.runtimeStatus();", QueryLanguage::Ts)
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
        .execute(
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
    let symbol_resource = host.symbol_resource_value(&spec_id).unwrap();
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
    assert!(payload.discovery.as_ref().is_some_and(|bundle| bundle
        .trust_signals
        .evidence_sources
        .iter()
        .any(|source| matches!(source, prism_js::EvidenceSourceKind::Inferred))));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| bundle
        .validation_context
        .suggested_queries
        .iter()
        .any(|query| query.label == "Validation Context")));
    assert!(payload
        .discovery
        .as_ref()
        .is_some_and(|bundle| !bundle.why.is_empty()));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| bundle
        .recent_change_context
        .suggested_queries
        .iter()
        .any(|query| query.label == "Recent Change Context")));
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
        .search_resource_value(
            "prism://search/workspace?strategy=direct&kind=toml-key&path=Cargo.toml&pathMode=exact&structuredPath=workspace&topLevelOnly=true&includeInferred=false",
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

    let symbol_payload = host.symbol_resource_value(&id).unwrap();
    let search_payload = host
        .search_resource_value(
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
fn discovery_helpers_surface_next_reads_and_behavioral_where_used() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
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

    let capped = QueryHost::new_with_limits(
        Prism::new(Graph::new()),
        QueryLimits {
            max_result_nodes: 1,
            max_call_graph_depth: 1,
            max_output_json_bytes: 32,
        },
    )
    .execute(
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
        result.diagnostics[0].data.as_ref().and_then(|data| data["nextAction"].as_str()),
        Some("Use prism.search(query, { path: ..., module: ..., kind: ..., taskId: ..., limit: ... }) to narrow the result set.")
    );
}

#[test]
fn unknown_host_operations_return_actionable_diagnostics() {
    let host = host_with_node(demo_node());
    let execution = QueryExecution::new(
        host.clone(),
        host.current_prism(),
        host.begin_query_run("test", "dispatch unknown operation"),
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
            r#"
const sym = prism.symbol("main");
return sym?.id.path;
"#,
            QueryLanguage::Ts,
        )
        .expect("first query should succeed");
    let second = host
        .execute(
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
        r#"
globalThis.__prismLeaked = 1;
return true;
"#,
        QueryLanguage::Ts,
    )
    .expect("first query should succeed");

    let second = host
        .execute(
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
    host.store_inferred_edge(PrismInferEdgeArgs {
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
    })
    .expect("inferred edge should store");

    let result = host
        .execute(
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

    host.store_inferred_edge(PrismInferEdgeArgs {
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
    })
    .expect("inferred edge should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = reloaded
        .execute(
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

    host.store_memory(PrismMemoryArgs {
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
    })
    .expect("note should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let replay = reloaded
        .current_prism()
        .resume_task(&TaskId::new("task:note"));
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);

    let recalled = reloaded
        .session
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
fn validation_feedback_mutation_persists_to_workspace_log() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(PrismValidationFeedbackArgs {
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
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
        })
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
fn auto_refreshes_workspace_and_records_patch_events() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
    )
    .unwrap();

    let result = host
        .execute(
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
        .expect("query should succeed after external edit");

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
fn refresh_workspace_reloads_updated_persisted_notes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let stored = host
        .store_memory(PrismMemoryArgs {
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
        })
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

    let result = host
        .execute(
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

    let entry = host
        .session
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

    host.store_inferred_edge(PrismInferEdgeArgs {
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
    })
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

    let result = host
        .execute(
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
        .symbol_query("missing")
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
        .search_query(SearchArgs {
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
            owner_kind: None,
            include_inferred: None,
        })
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
        .symbol_query("helper")
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
    assert!(
        envelope.result["id"]["path"]
            .as_str()
            .is_some_and(|path| !path.contains("::tests::"))
    );
    assert!(
        ambiguity["candidates"][0]["suggestedQueries"]
            .as_array()
            .is_some_and(|queries| !queries.is_empty())
    );
    assert!(
        ambiguity["suggestedQueries"]
            .as_array()
            .is_some_and(|queries| queries.iter().any(|query| {
                query["label"]
                    .as_str()
                    .is_some_and(|label| label == "Focused Block")
            }))
    );
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
        .search_query(SearchArgs {
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
            owner_kind: None,
            include_inferred: None,
        })
        .expect("module search should succeed");
    assert_eq!(module_envelope.result.as_array().map(|results| results.len()), Some(1));
    assert_eq!(
        module_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
    );

    let plan = host
        .store_coordination(PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Investigate helper collision" }),
            task_id: None,
        })
        .unwrap();
    let task = host
        .store_coordination(PrismCoordinationArgs {
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
        })
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let task_envelope = host
        .search_query(SearchArgs {
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
            owner_kind: None,
            include_inferred: None,
        })
        .expect("task-scoped search should succeed");
    assert_eq!(task_envelope.result.as_array().map(|results| results.len()), Some(1));
    assert_eq!(
        task_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
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
        .search_resource_value("prism://search/helper", "helper")
        .expect("search resource should succeed");

    assert!(payload.ambiguity.is_some());
    assert_eq!(
        payload
            .ambiguity
            .as_ref()
            .and_then(|ambiguity| ambiguity.returned.as_ref())
            .map(|symbol| symbol.id.path.as_str()),
        payload.results.first().map(|symbol| symbol.id.path.as_str())
    );
    assert!(payload
        .suggested_queries
        .iter()
        .any(|query| query.label == "Focused Block"));
}

#[test]
fn first_mutation_auto_creates_session_task() {
    let host = host_with_node(demo_node());

    let memory = host
        .store_memory(PrismMemoryArgs {
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
        })
        .expect("note should store");

    assert!(memory.memory_id.starts_with("memory:"));
    let task = host.session.current_task().expect("task should be created");
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.task, task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);
}

#[test]
fn recalls_session_memory_for_symbol_focus() {
    let host = host_with_node(demo_node());

    host.store_memory(PrismMemoryArgs {
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
    })
    .expect("note should store");

    let result = host
        .execute(
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
    host.session.notes.store(old_structural).unwrap();

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
    host.session.notes.store(fresh_semantic).unwrap();

    let result = host
        .execute(
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
        .start_task("Investigate main".to_string(), vec!["bug".to_string()])
        .expect("task should start");

    host.store_outcome(PrismOutcomeArgs {
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
    })
    .expect("validation outcome should store");

    let result = host
        .finish_task(PrismFinishTaskArgs {
            summary: "Closed out main investigation with validation coverage".to_string(),
            anchors: Some(vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }]),
            task_id: None,
        })
        .expect("finish task should succeed");

    assert_eq!(result.task_id, task.0);
    assert_eq!(host.session.current_task(), None);
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

    let memory = host
        .session
        .notes
        .entry(&MemoryId(result.memory_id.clone()))
        .expect("summary memory should exist");
    assert_eq!(
        memory.content,
        "Closed out main investigation with validation coverage"
    );
    assert_eq!(memory.metadata["taskLifecycle"]["disposition"], "completed");

    let resource = host
        .task_resource_value(&task_resource_uri(&result.task_id), &task)
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
    host.session.notes.store(memory).unwrap();

    let result = host
        .execute(
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
    assert!(result.diagnostics.iter().all(|diagnostic| diagnostic
        .data
        .as_ref()
        .and_then(|data| data["nextAction"].as_str())
        .is_some()));
}

#[test]
fn call_graph_depth_limit_diagnostic_includes_next_action() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
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
        .start_task("Investigate main".to_string(), Vec::new())
        .expect("task should start");

    host.store_outcome(PrismOutcomeArgs {
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
    })
    .expect("failure outcome should store");

    let result = host
        .abandon_task(PrismFinishTaskArgs {
            summary: "Stopped after upstream dependency failure".to_string(),
            anchors: None,
            task_id: None,
        })
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
        .start_task("Investigate main".to_string(), vec!["bug".to_string()])
        .expect("task should start");

    assert_eq!(host.session.current_task(), Some(task.clone()));
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::PlanCreated);
    assert_eq!(replay.events[0].summary, "Investigate main");
    assert_eq!(replay.events[0].metadata["tags"][0], "bug");
}

#[test]
fn explicit_task_override_does_not_replace_session_default() {
    let host = host_with_node(demo_node());
    let active = host
        .start_task("Primary task".to_string(), Vec::new())
        .expect("task should start");

    let explicit = TaskId::new("task:secondary:99");
    let event = host
        .store_outcome(PrismOutcomeArgs {
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
        })
        .expect("outcome should store");

    assert_eq!(host.session.current_task(), Some(active));
    let replay = host.current_prism().resume_task(&explicit);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].meta.id.0, event.event_id);
}
