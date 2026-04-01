use rmcp::transport::{IntoTransport, Transport};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    call_tool_request, demo_node, first_tool_content_json, initialize_client,
    initialized_notification, mutation_credential_json, response_json, server_with_node,
    temp_workspace, workspace_session_with_owner_credential,
};
use prism_core::{index_workspace_session, MintPrincipalRequest};
use prism_ir::{CredentialCapability, CredentialId, EventActor, PrincipalId, PrincipalKind};

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
                "credential": {
                    "credentialId": "credential:test",
                    "principalToken": "prism_ptok_test"
                },
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
    assert!(
        message.contains("prism_mutate action `validation_feedback`"),
        "{message}"
    );
    assert!(message.contains("context"), "{message}");
    assert!(message.contains("required fields:"), "{message}");
    assert!(message.contains("prism.validateToolInput(\"prism_mutate\", <input>)"));
    assert!(message.contains("prism://schema/tool/prism_mutate/action/validation_feedback"));
    assert!(message.contains("Minimal valid example:"));
    assert!(message.contains("\"action\":\"validation_feedback\""));

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

#[tokio::test]
async fn mcp_server_accepts_snake_case_compact_tool_aliases() {
    let root = temp_workspace();
    std::fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn main() {
    println!("hello");
}

#[tokio::test]
async fn mcp_server_rejects_prism_mutate_without_credential() {
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
                    "context": "Dogfooding auth envelope validation.",
                    "prismSaid": "Mutation should accept ambient session state.",
                    "actuallyTrue": "Mutation should reject calls without an explicit credential envelope.",
                    "category": "coordination",
                    "verdict": "harmful"
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
    assert!(message.contains("credential"), "{message}");

    running.cancel().await.unwrap();
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
                "task_intent": "documentation",
                "include_top_preview": true,
                "limit": 1,
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let locate = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(locate["status"], "ok");
    assert!(locate["topPreview"].is_object());

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_tool_call_logs_inherit_request_envelope_phases() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let server_handle = server.clone();
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
                "code": "return { ok: true };"
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let _ = first_tool_content_json(client.receive().await.unwrap());

    client
        .send(call_tool_request(
            3,
            "prism_session",
            json!({
                "action": "start_task",
                "description": "Verify request envelope inheritance"
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let _ = first_tool_content_json(client.receive().await.unwrap());

    let records = server_handle.host.mcp_call_log_store.records();
    let prism_query = records
        .iter()
        .find(|record| record.entry.call_type == "tool" && record.entry.name == "prism_query")
        .expect("prism_query tool record should exist");
    let prism_session = records
        .iter()
        .find(|record| record.entry.call_type == "tool" && record.entry.name == "prism_session")
        .expect("prism_session tool record should exist");
    let surfaced_entries = server_handle.host.mcp_call_entries(crate::McpLogArgs {
        limit: Some(20),
        since: None,
        call_type: None,
        name: None,
        task_id: None,
        session_id: None,
        success: None,
        min_duration_ms: None,
        contains: None,
    });
    let delegated_request_wrappers = surfaced_entries
        .iter()
        .filter(|entry| entry.call_type == "request" && entry.name == "tools/call")
        .count();
    assert_eq!(delegated_request_wrappers, 0);

    for record in [prism_query, prism_session] {
        let operations = record
            .phases
            .iter()
            .map(|phase| phase.operation.as_str())
            .collect::<Vec<_>>();
        assert!(operations.contains(&"mcp.receiveRequest"));
        assert!(operations.contains(&"mcp.routeRequest"));
        assert!(operations.contains(&"mcp.executeHandler"));
        assert!(operations.contains(&"mcp.encodeResponse"));
        let receive_started_at = record
            .phases
            .iter()
            .find(|phase| phase.operation == "mcp.receiveRequest")
            .map(|phase| phase.started_at)
            .expect("mcp.receiveRequest phase should exist");
        assert_eq!(record.entry.started_at, receive_started_at);
    }
    assert_eq!(
        prism_query.request_payload.as_ref(),
        Some(&json!({
            "code": "return { ok: true };"
        }))
    );
    assert_eq!(
        prism_session.request_payload.as_ref(),
        Some(&json!({
            "action": "start_task",
            "description": "Verify request envelope inheritance"
        }))
    );
    let query_operations = prism_query
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(query_operations.contains(&"runtimeSync.waitLock"));
    assert!(query_operations.contains(&"runtimeSync.refreshFs"));
    assert!(query_operations.contains(&"runtimeSync.snapshotRevisions"));

    running.cancel().await.unwrap();
}

#[test]
fn prism_mutate_validation_feedback_accepts_flat_snake_case_fields() {
    let args = serde_json::from_value::<PrismMutationArgs>(json!({
        "action": "validation_feedback",
        "credential": {
            "credential_id": "credential:test",
            "principal_token": "prism_ptok_test"
        },
        "context": "Dogfooding broad subsystem workset queries.",
        "prism_said": "Concept routing and recall were helpful.",
        "actually_true": "The concept path found the right subsystem, but the workset route needed improvement.",
        "category": "memory",
        "verdict": "helpful",
        "corrected_manually": true,
        "task_id": "task:dogfood-memory"
    }))
    .expect("snake_case shorthand should deserialize");

    assert_eq!(args.credential.credential_id, "credential:test");
    assert_eq!(args.credential.principal_token, "prism_ptok_test");
    let PrismMutationKindArgs::ValidationFeedback(input) = args.mutation else {
        panic!("expected validation feedback mutation");
    };
    assert_eq!(input.prism_said, "Concept routing and recall were helpful.");
    assert_eq!(
        input.actually_true,
        "The concept path found the right subsystem, but the workset route needed improvement."
    );
    assert_eq!(input.corrected_manually, Some(true));
    assert_eq!(input.task_id.as_deref(), Some("task:dogfood-memory"));
}

#[test]
fn prism_mutate_session_repair_accepts_clear_current_task_operation() {
    let args = serde_json::from_value::<PrismMutationArgs>(json!({
        "action": "session_repair",
        "credential": {
            "credentialId": "credential:test",
            "principalToken": "prism_ptok_test"
        },
        "input": {
            "operation": "clear_current_task"
        }
    }))
    .expect("session repair mutation should deserialize");

    let PrismMutationKindArgs::SessionRepair(args) = args.mutation else {
        panic!("expected session repair mutation");
    };
    assert!(matches!(
        args.operation,
        SessionRepairOperationInput::ClearCurrentTask
    ));
}

#[test]
fn prism_mutate_coordination_rejects_missing_typed_payload_fields() {
    let error = serde_json::from_value::<PrismMutationArgs>(json!({
        "action": "coordination",
        "credential": {
            "credentialId": "credential:test",
            "principalToken": "prism_ptok_test"
        },
        "input": {
            "kind": "plan_create",
            "payload": {}
        }
    }))
    .expect_err("missing typed payload fields should fail");

    let message = error.to_string();
    assert!(message.contains("goal"), "{message}");
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
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session(session);
    let server_handle = server.clone();
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
                "credential": mutation_credential_json(&credential),
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
                "credential": mutation_credential_json(&credential),
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
                "credential": mutation_credential_json(&credential),
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
                "credential": mutation_credential_json(&credential),
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

    let events = server_handle.host.current_prism().coordination_events();
    for (response, expected_request_id) in
        [(&plan, "2"), (&task, "3"), (&claim, "4"), (&artifact, "5")]
    {
        let event_ids = response["result"]["eventIds"]
            .as_array()
            .expect("mutation should report event ids");
        let event_id = event_ids
            .first()
            .and_then(|value| value.as_str())
            .expect("mutation should report the primary event id");
        let event = events
            .iter()
            .find(|event| event.meta.id.0 == event_id)
            .expect("persisted coordination event should exist");
        let EventActor::Principal(principal) = &event.meta.actor else {
            panic!("expected principal actor, got {:?}", event.meta.actor);
        };
        assert_eq!(principal.principal_id.0, credential.principal_id);
        assert!(!principal.authority_id.0.is_empty());
        let context = event
            .meta
            .execution_context
            .as_ref()
            .expect("authenticated mutation should record execution context");
        assert_eq!(context.request_id.as_deref(), Some(expected_request_id));
        assert_eq!(
            context.credential_id.as_ref().map(|value| value.0.as_str()),
            Some(credential.credential_id.as_str())
        );
    }

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
    assert!(execution.is_empty() || execution[0]["nodeId"] == task_id);
    assert_eq!(envelope["result"]["ready"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["result"]["claims"].as_array().unwrap().len(), 0);
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

#[tokio::test]
async fn mcp_server_rejects_invalid_prism_mutate_credential() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session(session);
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    let mut invalid_credential = mutation_credential_json(&credential);
    invalid_credential["principalToken"] = Value::String("prism_ptok_wrong".to_string());

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "credential": invalid_credential,
                "input": {
                    "context": "Dogfooding mutation credential rejection.",
                    "prismSaid": "Any credential id should be accepted.",
                    "actuallyTrue": "The principal token must authenticate successfully before the mutation runs.",
                    "category": "freshness",
                    "verdict": "wrong"
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
    assert!(message.contains("credential rejected"), "{message}");

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_prism_mutate_when_capability_is_denied() {
    let root = temp_workspace();
    let (session, owner_credential) = workspace_session_with_owner_credential(&root);
    let owner = session
        .authenticate_principal_credential(
            &CredentialId::new(owner_credential.credential_id.clone()),
            &owner_credential.principal_token,
        )
        .expect("owner credential should authenticate");
    let worker = session
        .mint_principal_credential(
            &owner,
            MintPrincipalRequest {
                authority_id: None,
                kind: PrincipalKind::Agent,
                name: "Memory Worker".to_string(),
                role: Some("memory_only".to_string()),
                parent_principal_id: Some(PrincipalId::new(owner.principal.principal_id.0.clone())),
                capabilities: vec![CredentialCapability::MutateRepoMemory],
                profile: Value::Null,
            },
        )
        .expect("child principal should mint");
    let worker_credential = json!({
        "credentialId": worker.credential.credential_id.0,
        "principalToken": worker.principal_token,
    });
    let server = PrismMcpServer::with_session(session);
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
                "credential": worker_credential,
                "input": {
                    "kind": "plan_create",
                    "payload": {
                        "goal": "Try a coordination write with repo-memory-only capabilities"
                    }
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
    assert_eq!(
        response["error"]["data"]["code"],
        Value::String("mutation_capability_denied".to_string())
    );
    assert_eq!(
        response["error"]["data"]["requiredCapability"],
        Value::String("mutate_coordination".to_string())
    );

    running.cancel().await.unwrap();
}
