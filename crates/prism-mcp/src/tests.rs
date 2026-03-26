use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rmcp::{
    model::{ClientJsonRpcMessage, ProtocolVersion, ServerJsonRpcMessage},
    transport::{IntoTransport, Transport},
    ServiceExt,
};

use super::*;
use prism_agent::InferredEdgeScope;
use prism_core::{index_workspace_session, index_workspace_session_with_curator};
use prism_curator::{
    CandidateEdge, CandidateMemory, CandidateRiskSummary, CandidateValidationRecipe,
    CuratorBackend, CuratorContext, CuratorJob, CuratorProposal, CuratorRun,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId,
    NodeKind, Span, TaskId,
};
use prism_memory::{
    MemoryKind, MemoryModule, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory,
    OutcomeResult, RecallQuery,
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

    let execution = QueryExecution::new(host.clone(), host.current_prism());
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
    assert!(tool_names.contains(&"prism_query"));
    assert!(tool_names.contains(&"prism_symbol"));
    assert!(tool_names.contains(&"prism_search"));
    assert!(tool_names.contains(&"prism_outcome"));
    assert!(tool_names.contains(&"prism_start_task"));
    assert!(tool_names.contains(&"prism_coordination"));
    assert!(tool_names.contains(&"prism_claim"));
    assert!(tool_names.contains(&"prism_artifact"));
    assert!(tool_names.contains(&"prism_curator_promote_edge"));
    assert!(tool_names.contains(&"prism_curator_promote_memory"));
    assert!(tool_names.contains(&"prism_curator_reject_proposal"));

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

    running.cancel().await.unwrap();
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

    let execution = QueryExecution::new(host.clone(), host.current_prism());
    let error = execution
        .dispatch("plan", r#"{ "planId": "plan:1" }"#)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("coordination workflow queries are disabled"));
}

#[tokio::test]
async fn mcp_server_simple_mode_hides_coordination_tools_and_reports_features() {
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
    assert!(!tool_names.contains(&"prism_coordination"));
    assert!(!tool_names.contains(&"prism_claim"));
    assert!(!tool_names.contains(&"prism_artifact"));

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

    client
        .send(call_tool_request(
            4,
            "prism_coordination",
            json!({
                "kind": "plan_create",
                "payload": { "goal": "Coordinate the main edit" }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let response = response_json(client.receive().await.unwrap());
    assert_eq!(response["error"]["message"], "tool not found");

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
            "prism_coordination",
            json!({
                "kind": "plan_create",
                "payload": { "goal": "Coordinate the main edit" }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let plan = first_tool_content_json(client.receive().await.unwrap());
    let plan_id = plan["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            3,
            "prism_coordination",
            json!({
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
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let task = first_tool_content_json(client.receive().await.unwrap());
    let task_id = task["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            4,
            "prism_claim",
            json!({
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
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let claim = first_tool_content_json(client.receive().await.unwrap());
    assert!(claim["claimId"].as_str().is_some());

    client
        .send(call_tool_request(
            5,
            "prism_artifact",
            json!({
                "action": "propose",
                "payload": {
                    "taskId": task["state"]["id"].as_str().unwrap(),
                    "diffRef": "patch:1"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    assert!(artifact["artifactId"].as_str().is_some());
    let artifact_id = artifact["artifactId"].as_str().unwrap().to_string();

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
        artifact["artifactId"]
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

    let execution = QueryExecution::new(host.clone(), host.current_prism());
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
            "prism_coordination",
            json!({
                "kind": "plan_create",
                "payload": {
                    "goal": "Review-gated change",
                    "policy": { "requireReviewForCompletion": true }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let plan = first_tool_content_json(client.receive().await.unwrap());
    let plan_id = plan["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            3,
            "prism_coordination",
            json!({
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
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let task = first_tool_content_json(client.receive().await.unwrap());
    let task_id = task["state"]["id"].as_str().unwrap().to_string();

    client
        .send(call_tool_request(
            4,
            "prism_artifact",
            json!({
                "action": "propose",
                "payload": {
                    "taskId": task_id,
                    "diffRef": "patch:review-gated"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    assert!(artifact["artifactId"].as_str().is_some());

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
        1
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
    assert_eq!(handed_off.result["assignee"], "agent-b");
    assert_eq!(handed_off.result["status"], "Ready");

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

    host.store_note(PrismNoteArgs {
        anchors: vec![AnchorRefInput::Node {
            crate_name: "demo".to_string(),
            path: "demo::alpha".to_string(),
            kind: "function".to_string(),
        }],
        content: "alpha previously regressed".to_string(),
        trust: Some(0.9),
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
            include_inferred: None,
        })
        .expect("search query should succeed");
    assert!(envelope.result.is_array());
    assert!(envelope.diagnostics.is_empty());
}

#[test]
fn first_mutation_auto_creates_session_task() {
    let host = host_with_node(demo_node());

    let memory = host
        .store_note(PrismNoteArgs {
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            content: "remember this".to_string(),
            trust: Some(0.8),
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

    host.store_note(PrismNoteArgs {
        anchors: vec![AnchorRefInput::Node {
            crate_name: "demo".to_string(),
            path: "demo::main".to_string(),
            kind: "function".to_string(),
        }],
        content: "main previously regressed on null handling".to_string(),
        trust: Some(0.9),
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
