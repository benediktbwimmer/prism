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
use prism_agent::InferredEdgeScope;
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

#[test]
fn mcp_returns_structured_coordination_rejections_and_persists_them() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

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
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

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

    let execution = QueryExecution::new(host.clone(), host.current_prism());
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

    let execution = QueryExecution::new(host.clone(), host.current_prism());
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
    assert!(symbol_resource.spec_cluster.is_some());
    assert!(symbol_resource.spec_drift.is_some());
    assert!(!symbol_resource.suggested_reads.is_empty());
    assert!(!symbol_resource.read_context.suggested_reads.is_empty());
    assert!(!symbol_resource.edit_context.suggested_queries.is_empty());
    assert_eq!(symbol_resource.suggested_queries[0].label, "Read Context");
    assert_eq!(symbol_resource.suggested_queries[1].label, "Read Owners");
    assert_eq!(
        symbol_resource.suggested_queries[2].label,
        "Validation Recipe"
    );
    assert_eq!(symbol_resource.suggested_queries[3].label, "Edit Context");
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
    assert!(!payload.suggested_reads.is_empty());
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
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(
            "prism://search/memory%20recall?strategy=behavioral&ownerKind=read&kind=function&path=src%2Fmemory&includeInferred=false",
            "memory recall",
        )
        .unwrap();

    assert_eq!(payload.strategy, "behavioral");
    assert_eq!(payload.owner_kind.as_deref(), Some("read"));
    assert_eq!(payload.kind.as_deref(), Some("function"));
    assert_eq!(payload.path.as_deref(), Some("src/memory"));
    assert!(!payload.include_inferred);
}

#[test]
fn read_and_edit_context_queries_return_semantic_bundles() {
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
      read: prism.readContext(spec),
      edit: prism.editContext(spec),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result.result["read"]["directLinks"].is_array());
    assert!(result.result["read"]["suggestedReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["edit"]["writePaths"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["edit"]["checklist"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
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
        Some("Use prism.search(query, { path: ..., kind: ..., limit: ... }) to narrow the result set.")
    );
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
            category: "projection".to_string(),
            verdict: "wrong".to_string(),
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
            strategy: None,
            owner_kind: None,
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
    assert_eq!(
        result.result["relatedMemory"][0]["entry"]["content"],
        "main changes should always get a regression check"
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missing_validation"));
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
