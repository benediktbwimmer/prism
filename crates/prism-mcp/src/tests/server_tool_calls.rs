use rmcp::transport::{IntoTransport, Transport};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    call_tool_request, demo_node, first_tool_content_json, initialize_client,
    initialized_notification, mutation_credential_json, read_resource_request,
    register_test_agent_worktree, register_test_human_worktree, response_json, server_with_node,
    temp_workspace, workspace_session_with_owner_credential,
};
use prism_coordination::{TaskCreateInput, TaskUpdateInput};
use prism_core::{
    default_workspace_shared_runtime, index_workspace_session,
    index_workspace_session_with_options, BootstrapOwnerInput, MintPrincipalRequest,
    PrismRuntimeMode, WorkspaceSessionOptions,
};
use prism_ir::{
    CoordinationTaskId, CoordinationTaskStatus, CredentialCapability, CredentialId, EventActor,
    EventId, EventMeta, PlanStatus, PrincipalActor, PrincipalAuthorityId, PrincipalId,
    PrincipalKind, SessionId,
};

fn resource_text(response: serde_json::Value) -> String {
    response["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource should be text")
        .to_string()
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

    let records = server_handle.host.mcp_call_log_store.records();
    let prism_query = records
        .iter()
        .find(|record| record.entry.call_type == "tool" && record.entry.name == "prism_query")
        .expect("prism_query tool record should exist");
    let surfaced_entries = server_handle.host.mcp_call_entries(crate::McpLogArgs {
        limit: Some(20),
        since: None,
        scope: None,
        call_type: None,
        name: None,
        task_id: None,
        worktree_id: None,
        repo_id: None,
        workspace_root: None,
        session_id: None,
        server_instance_id: None,
        process_id: None,
        success: None,
        min_duration_ms: None,
        contains: None,
    });
    let delegated_request_wrappers = surfaced_entries
        .iter()
        .filter(|entry| entry.call_type == "request" && entry.name == "tools/call")
        .count();
    assert_eq!(delegated_request_wrappers, 0);

    let operations = prism_query
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mcp.receiveRequest"));
    assert!(operations.contains(&"mcp.routeRequest"));
    assert!(operations.contains(&"mcp.executeHandler"));
    assert!(operations.contains(&"mcp.encodeResponse"));
    let receive_started_at = prism_query
        .phases
        .iter()
        .find(|phase| phase.operation == "mcp.receiveRequest")
        .map(|phase| phase.started_at)
        .expect("mcp.receiveRequest phase should exist");
    assert_eq!(prism_query.entry.started_at, receive_started_at);
    assert_eq!(
        prism_query.request_payload.as_ref(),
        Some(&json!({
            "code": "return { ok: true };"
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
fn prism_mutate_declare_work_accepts_bootstrap_payload() {
    let args = serde_json::from_value::<PrismMutationArgs>(json!({
        "action": "declare_work",
        "credential": {
            "credentialId": "credential:test",
            "principalToken": "prism_ptok_test"
        },
        "input": {
            "title": "Curate principal identity concepts",
            "kind": "delegated",
            "summary": "Bootstrap durable work intent.",
            "parentWorkId": "work:parent",
            "coordinationTaskId": "coord-task:child",
            "planId": "plan:identity"
        }
    }))
    .expect("declare work mutation should deserialize");

    let PrismMutationKindArgs::DeclareWork(args) = args.mutation else {
        panic!("expected declare work mutation");
    };
    assert_eq!(args.title, "Curate principal identity concepts");
    assert!(matches!(
        args.kind,
        Some(WorkDeclarationKindInput::Delegated)
    ));
    assert_eq!(args.parent_work_id.as_deref(), Some("work:parent"));
    assert_eq!(
        args.coordination_task_id.as_deref(),
        Some("coord-task:child")
    );
    assert_eq!(args.plan_id.as_deref(), Some("plan:identity"));
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

    let credential = args
        .credential
        .expect("explicit credential should deserialize");
    assert_eq!(credential.credential_id, "credential:test");
    assert_eq!(credential.principal_token, "prism_ptok_test");
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
    assert!(message.contains("title"), "{message}");
    assert!(message.contains("required field"), "{message}");
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
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Coordinate the main edit"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "plan_create",
                    "payload": { "title": "Coordinate the main edit", "goal": "Coordinate the main edit" }
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
            4,
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
            5,
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
            6,
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

    server_handle
        .host
        .workspace_session()
        .expect("workspace session should exist")
        .flush_materializations()
        .expect("queued coordination materializations should flush before prism_query");

    let events = server_handle.host.current_prism().coordination_events();
    for (response, expected_request_id) in
        [(&plan, "3"), (&task, "4"), (&claim, "5"), (&artifact, "6")]
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
            7,
            "prism_query",
            json!({
                "code": format!(
                    r#"
const sym = prism.symbol("main");
return {{
  plan: prism.plan("{plan_id}"),
  children: prism.children("{plan_id}"),
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
    assert_eq!(envelope["result"]["plan"]["id"], plan_id);
    assert_eq!(
        envelope["result"]["children"]["children"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(envelope["result"]["children"]["children"][0]["id"], task_id);
    assert_eq!(envelope["result"]["ready"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["result"]["ready"][0]["id"], task_id);
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
async fn mcp_server_auto_resumes_stale_same_principal_task_on_update() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let authenticated = session
        .authenticate_principal_credential(
            &CredentialId::new(credential.credential_id.clone()),
            &credential.principal_token,
        )
        .expect("credential should authenticate");
    session
        .bind_or_validate_worktree_principal(&authenticated)
        .expect("principal should bind to the worktree");

    let prior_session = SessionId::new("session:stale-prior");
    let stale_ts = crate::current_timestamp().saturating_sub(8_000);
    let actor = EventActor::Principal(PrincipalActor {
        authority_id: authenticated.principal.authority_id.clone(),
        principal_id: authenticated.principal.principal_id.clone(),
        kind: Some(authenticated.principal.kind),
        name: Some(authenticated.principal.name.clone()),
    });
    let execution_context = Some(session.event_execution_context(
        Some(&prior_session),
        None,
        Some(&authenticated.credential.credential_id),
    ));
    let (_plan_id, task_id) = session
        .mutate_coordination_with_session_wait_observed(
            Some(&prior_session),
            |prism| {
                let plan_meta = EventMeta {
                    id: EventId::new("coordination:stale-resume:plan"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let plan_id = prism.create_native_plan(
                    plan_meta,
                    "Resume stale same-principal task".to_string(),
                    "Resume stale same-principal task".to_string(),
                    Some(PlanStatus::Active),
                    None,
                )?;
                let task_meta = EventMeta {
                    id: EventId::new("coordination:stale-resume:task"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let task = prism.create_native_task(
                    task_meta,
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: "Resume me through prism_mutate".to_string(),
                        status: Some(CoordinationTaskStatus::InProgress),
                        assignee: None,
                        session: Some(prior_session.clone()),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                        spec_refs: Vec::new(),
                    },
                )?;
                Ok((plan_id, CoordinationTaskId::new(task.task.id.0.clone())))
            },
            |_operation, _duration, _args, _success, _error| {},
        )
        .expect("stale seeded coordination mutation should succeed")
        .expect("coordination mutation should acquire the refresh lock");

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
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "update",
                    "payload": {
                        "id": task_id.0,
                        "status": "ready"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let updated = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        updated["result"]["state"]["id"],
        Value::from(task_id.0.to_string())
    );
    assert_eq!(updated["result"]["state"]["status"], Value::from("pending"));

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "update",
                    "payload": {
                        "id": task_id.0,
                        "status": "ready"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let updated_again = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        updated_again["result"]["state"]["id"],
        Value::from(task_id.0.to_string())
    );
    assert_eq!(
        updated_again["result"]["state"]["status"],
        Value::from("pending")
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_auto_resumes_stale_same_principal_ready_task_on_update() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let authenticated = session
        .authenticate_principal_credential(
            &CredentialId::new(credential.credential_id.clone()),
            &credential.principal_token,
        )
        .expect("credential should authenticate");
    session
        .bind_or_validate_worktree_principal(&authenticated)
        .expect("principal should bind to the worktree");

    let prior_session = SessionId::new("session:stale-ready-prior");
    let stale_ts = crate::current_timestamp().saturating_sub(8_000);
    let actor = EventActor::Principal(PrincipalActor {
        authority_id: authenticated.principal.authority_id.clone(),
        principal_id: authenticated.principal.principal_id.clone(),
        kind: Some(authenticated.principal.kind),
        name: Some(authenticated.principal.name.clone()),
    });
    let execution_context = Some(session.event_execution_context(
        Some(&prior_session),
        None,
        Some(&authenticated.credential.credential_id),
    ));
    let (_plan_id, task_id) = session
        .mutate_coordination_with_session_wait_observed(
            Some(&prior_session),
            |prism| {
                let plan_meta = EventMeta {
                    id: EventId::new("coordination:stale-ready-resume:plan"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let plan_id = prism.create_native_plan(
                    plan_meta,
                    "Resume stale same-principal ready task".to_string(),
                    "Resume stale same-principal ready task".to_string(),
                    Some(PlanStatus::Active),
                    None,
                )?;
                let task_meta = EventMeta {
                    id: EventId::new("coordination:stale-ready-resume:task"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let task = prism.create_native_task(
                    task_meta,
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: "Resume my stale ready lease".to_string(),
                        status: Some(CoordinationTaskStatus::Ready),
                        assignee: None,
                        session: Some(prior_session.clone()),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                        spec_refs: Vec::new(),
                    },
                )?;
                Ok((plan_id, CoordinationTaskId::new(task.task.id.0.clone())))
            },
            |_operation, _duration, _args, _success, _error| {},
        )
        .expect("stale seeded coordination mutation should succeed")
        .expect("coordination mutation should acquire the refresh lock");

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
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "update",
                    "payload": {
                        "id": task_id.0,
                        "summary": "resume should unblock ready follow-up updates"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let resumed = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        resumed["result"]["state"]["id"],
        Value::from(task_id.0.to_string())
    );
    assert_eq!(resumed["result"]["state"]["status"], Value::from("pending"));
    assert_eq!(
        resumed["result"]["state"]["summary"],
        Value::from("resume should unblock ready follow-up updates")
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_auto_resumes_stale_same_worktree_executor_task_on_update() {
    let root = temp_workspace();
    let (session, _credential) = workspace_session_with_owner_credential(&root);
    let registration = register_test_agent_worktree(&root);
    let slot = session
        .acquire_or_refresh_agent_worktree_mutator_slot(&SessionId::new("session:bridge-current"))
        .expect("agent worktree slot should be acquired");

    let prior_session = SessionId::new("session:bridge-stale-prior");
    let stale_ts = crate::current_timestamp().saturating_sub(8_000);
    let actor = EventActor::Principal(PrincipalActor {
        authority_id: PrincipalAuthorityId::new(slot.authority_id.clone()),
        principal_id: PrincipalId::new(slot.principal_id.clone()),
        kind: Some(slot.principal_kind),
        name: Some(slot.principal_name.clone()),
    });
    let execution_context = Some(session.event_execution_context(Some(&prior_session), None, None));
    let (_plan_id, task_id) = session
        .mutate_coordination_with_session_wait_observed(
            Some(&prior_session),
            |prism| {
                let plan_meta = EventMeta {
                    id: EventId::new("coordination:bridge-worktree-resume:plan"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let plan_id = prism.create_native_plan(
                    plan_meta,
                    "Resume stale same-worktree task".to_string(),
                    "Resume stale same-worktree task".to_string(),
                    Some(PlanStatus::Active),
                    None,
                )?;
                let task_meta = EventMeta {
                    id: EventId::new("coordination:bridge-worktree-resume:task"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let task = prism.create_native_task(
                    task_meta,
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: "Resume me through worktree continuity".to_string(),
                        status: Some(CoordinationTaskStatus::InProgress),
                        assignee: None,
                        session: Some(prior_session.clone()),
                        worktree_id: Some(registration.worktree_id.clone()),
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                        spec_refs: Vec::new(),
                    },
                )?;
                Ok((plan_id, CoordinationTaskId::new(task.task.id.0.clone())))
            },
            |_operation, _duration, _args, _success, _error| {},
        )
        .expect("stale seeded coordination mutation should succeed")
        .expect("coordination mutation should acquire the refresh lock");

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
                "bridgeExecution": {
                    "worktreeId": registration.worktree_id,
                    "agentLabel": registration.agent_label
                },
                "input": {
                    "kind": "update",
                    "payload": {
                        "id": task_id.0,
                        "summary": "bridge continuity resumed the stale task"
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let resumed = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        resumed["result"]["state"]["id"],
        Value::from(task_id.0.to_string())
    );
    assert_eq!(
        resumed["result"]["state"]["summary"],
        Value::from("bridge continuity resumed the stale task")
    );

    let reloaded = index_workspace_session(&root).expect("workspace should reload");
    let task = reloaded
        .prism()
        .coordination_task(&task_id)
        .expect("task should remain queryable");
    let holder = task.lease_holder.expect("task should carry a lease holder");
    let principal = holder
        .principal
        .expect("lease holder principal should be recorded");
    assert_eq!(principal.authority_id.0, "worktree_executor");
    assert_eq!(principal.principal_id.0, registration.worktree_id);

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_resumes_stale_same_principal_task_when_git_execution_start_is_require() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let authenticated = session
        .authenticate_principal_credential(
            &CredentialId::new(credential.credential_id.clone()),
            &credential.principal_token,
        )
        .expect("credential should authenticate");
    session
        .bind_or_validate_worktree_principal(&authenticated)
        .expect("principal should bind to the worktree");

    let prior_session = SessionId::new("session:stale-prior-git-exec");
    let stale_ts = crate::current_timestamp().saturating_sub(8_000);
    let actor = EventActor::Principal(PrincipalActor {
        authority_id: authenticated.principal.authority_id.clone(),
        principal_id: authenticated.principal.principal_id.clone(),
        kind: Some(authenticated.principal.kind),
        name: Some(authenticated.principal.name.clone()),
    });
    let execution_context = Some(session.event_execution_context(
        Some(&prior_session),
        None,
        Some(&authenticated.credential.credential_id),
    ));
    let (_plan_id, task_id) = session
        .mutate_coordination_with_session_wait_observed(
            Some(&prior_session),
            |prism| {
                let plan_meta = EventMeta {
                    id: EventId::new("coordination:stale-resume-git-exec:plan"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let plan_id = prism.create_native_plan(
                    plan_meta,
                    "Resume stale same-principal task with require start".to_string(),
                    "Resume stale same-principal task with require start".to_string(),
                    Some(PlanStatus::Active),
                    Some(prism_coordination::CoordinationPolicy {
                        git_execution: prism_coordination::GitExecutionPolicy {
                            start_mode: prism_coordination::GitExecutionStartMode::Require,
                            completion_mode: prism_coordination::GitExecutionCompletionMode::Off,
                            target_ref: None,
                            target_branch: "main".into(),
                            require_task_branch: true,
                            max_commits_behind_target: 0,
                            max_fetch_age_seconds: None,
                            integration_mode: prism_ir::GitIntegrationMode::External,
                        },
                        ..prism_coordination::CoordinationPolicy::default()
                    }),
                )?;
                let task_meta = EventMeta {
                    id: EventId::new("coordination:stale-resume-git-exec:task"),
                    ts: stale_ts,
                    actor: actor.clone(),
                    correlation: None,
                    causation: None,
                    execution_context: execution_context.clone(),
                };
                let task = prism.create_native_task(
                    task_meta,
                    TaskCreateInput {
                        plan_id: plan_id.clone(),
                        title: "Resume me through prism_mutate with require start".to_string(),
                        status: Some(CoordinationTaskStatus::Ready),
                        assignee: None,
                        session: Some(prior_session.clone()),
                        worktree_id: None,
                        branch_ref: None,
                        anchors: Vec::new(),
                        depends_on: Vec::new(),
                        coordination_depends_on: Vec::new(),
                        integrated_depends_on: Vec::new(),
                        acceptance: Vec::new(),
                        base_revision: prism.workspace_revision(),
                        spec_refs: Vec::new(),
                    },
                )?;
                let task = prism.update_native_task_authoritative_only(
                    EventMeta {
                        id: EventId::new("coordination:stale-resume-git-exec:authoritative"),
                        ts: stale_ts,
                        actor: actor.clone(),
                        correlation: None,
                        causation: None,
                        execution_context: execution_context.clone(),
                    },
                    prism_coordination::TaskUpdateInput {
                        task_id: CoordinationTaskId::new(task.task.id.0.clone()),
                        kind: None,
                        status: Some(CoordinationTaskStatus::InProgress),
                        published_task_status: None,
                        git_execution: None,
                        assignee: None,
                        session: None,
                        worktree_id: None,
                        branch_ref: None,
                        title: None,
                        summary: None,
                        anchors: None,
                        bindings: None,
                        depends_on: None,
                        coordination_depends_on: None,
                        integrated_depends_on: None,
                        acceptance: None,
                        validation_refs: None,
                        is_abstract: None,
                        base_revision: Some(prism.workspace_revision()),
                        priority: None,
                        tags: None,
                        completion_context: None,
                        spec_refs: None,
                    },
                    prism.workspace_revision(),
                    stale_ts,
                )?;
                Ok((plan_id, CoordinationTaskId::new(task.task.id.0.clone())))
            },
            |_operation, _duration, _args, _success, _error| {},
        )
        .expect("stale seeded coordination mutation should succeed")
        .expect("coordination mutation should acquire the refresh lock");

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
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "resume",
                    "payload": {
                        "taskId": task_id.0
                    }
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let resumed = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        resumed["result"]["state"]["id"],
        Value::from(task_id.0.to_string())
    );
    assert_eq!(resumed["result"]["state"]["status"], Value::from("active"));

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
async fn mcp_server_supports_mcp_only_self_described_workflows() {
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

    client
        .send(read_resource_request(200, "prism://capabilities/tools"))
        .await
        .unwrap();
    let tools = serde_json::from_str::<Value>(&resource_text(response_json(
        client.receive().await.unwrap(),
    )))
    .unwrap();
    assert!(tools["value"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_mutate"));

    for (id, uri) in [
        (
            201_u64,
            "prism://shape/tool/prism_mutate/action/declare_work",
        ),
        (
            202,
            "prism://shape/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ),
        (
            203,
            "prism://shape/tool/prism_mutate/action/coordination/variant/update",
        ),
        (
            204,
            "prism://shape/tool/prism_mutate/action/claim/variant/acquire",
        ),
        (
            205,
            "prism://shape/tool/prism_mutate/action/artifact/variant/review",
        ),
        (
            206,
            "prism://shape/tool/prism_mutate/action/validation_feedback",
        ),
        (
            207,
            "prism://example/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ),
        (
            208,
            "prism://recipe/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ),
        (
            209,
            "prism://recipe/tool/prism_mutate/action/claim/variant/acquire",
        ),
        (
            210,
            "prism://recipe/tool/prism_mutate/action/artifact/variant/review",
        ),
    ] {
        client.send(read_resource_request(id, uri)).await.unwrap();
        let text = resource_text(response_json(client.receive().await.unwrap()));
        assert!(!text.is_empty(), "{uri}");
    }

    client
        .send(call_tool_request(
            211,
            "prism_mutate",
            json!({
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Exercise self-described MCP workflows",
                    "summary": "Drive planning, coordination, claim, artifact, and feedback flows from discovery resources."
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], Value::from("declare_work"));

    let bootstrap_input = json!({
        "action": "coordination",
        "credential": mutation_credential_json(&credential),
        "input": {
            "kind": "plan_bootstrap",
            "payload": {
                "plan": {
                    "title": "Self-described MCP workflow plan",
                    "goal": "Validate source-free planning and execution flows"
                },
                "tasks": [{
                    "clientId": "t0",
                    "title": "Create reviewable state",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }]
            }
        }
    });
    client
        .send(call_tool_request(
            212,
            "prism_query",
            json!({
                "code": format!(
                    "return prism.validateToolInput(\"prism_mutate\", {});",
                    bootstrap_input
                )
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let bootstrap_validation = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(bootstrap_validation["result"]["valid"], Value::Bool(true));

    client
        .send(call_tool_request(
            213,
            "prism_mutate",
            bootstrap_input
                .as_object()
                .expect("bootstrap input should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let bootstrap = first_tool_content_json(client.receive().await.unwrap());
    let task_id = bootstrap["result"]["state"]["taskIdsByClientId"]["t0"]
        .as_str()
        .expect("bootstrapped task id should exist")
        .to_string();

    let update_input = json!({
        "action": "coordination",
        "credential": mutation_credential_json(&credential),
        "input": {
            "kind": "update",
            "payload": {
                "id": task_id.clone(),
                "status": "in_progress"
            }
        }
    });
    client
        .send(call_tool_request(
            214,
            "prism_query",
            json!({
                "code": format!(
                    "return prism.validateToolInput(\"prism_mutate\", {});",
                    update_input
                )
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let update_validation = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(update_validation["result"]["valid"], Value::Bool(true));

    client
        .send(call_tool_request(
            215,
            "prism_mutate",
            update_input
                .as_object()
                .expect("update input should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let updated = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(updated["result"]["state"]["status"], Value::from("active"));

    let claim_input = json!({
        "action": "claim",
        "credential": mutation_credential_json(&credential),
        "input": {
            "action": "acquire",
            "payload": {
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "capability": "edit",
                "mode": "soft_exclusive",
                "coordinationTaskId": task_id.clone()
            }
        }
    });
    client
        .send(call_tool_request(
            216,
            "prism_query",
            json!({
                "code": format!(
                    "return prism.validateToolInput(\"prism_mutate\", {});",
                    claim_input
                )
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let claim_validation = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(claim_validation["result"]["valid"], Value::Bool(true));

    client
        .send(call_tool_request(
            217,
            "prism_mutate",
            claim_input
                .as_object()
                .expect("claim input should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let claim = first_tool_content_json(client.receive().await.unwrap());
    assert!(claim["result"]["claimId"].as_str().is_some());

    let propose_artifact_input = json!({
        "action": "artifact",
        "credential": mutation_credential_json(&credential),
        "input": {
            "action": "propose",
            "payload": {
                "taskId": task_id.clone(),
                "diffRef": "patch:self-described"
            }
        }
    });
    client
        .send(call_tool_request(
            218,
            "prism_mutate",
            propose_artifact_input
                .as_object()
                .expect("artifact proposal should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let artifact = first_tool_content_json(client.receive().await.unwrap());
    let artifact_id = artifact["result"]["artifactId"]
        .as_str()
        .expect("artifact id should exist")
        .to_string();

    let review_input = json!({
        "action": "artifact",
        "credential": mutation_credential_json(&credential),
        "input": {
            "action": "review",
            "payload": {
                "artifactId": artifact_id.clone(),
                "verdict": "approved",
                "summary": "Reviewed entirely through the self-description surface."
            }
        }
    });
    client
        .send(call_tool_request(
            219,
            "prism_query",
            json!({
                "code": format!(
                    "return prism.validateToolInput(\"prism_mutate\", {});",
                    review_input
                )
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let review_validation = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(review_validation["result"]["valid"], Value::Bool(true));

    client
        .send(call_tool_request(
            220,
            "prism_mutate",
            review_input
                .as_object()
                .expect("artifact review should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let review = first_tool_content_json(client.receive().await.unwrap());
    assert!(review["result"]["reviewId"].as_str().is_some());
    assert_eq!(review["result"]["state"]["status"], Value::from("Approved"));

    let feedback_input = json!({
        "action": "validation_feedback",
        "credential": mutation_credential_json(&credential),
        "input": {
            "context": "Exercise the self-described mutation workflow.",
            "prismSaid": "The compact companion ladder should be enough.",
            "actuallyTrue": "The workflow succeeded through shapes, examples, recipes, validateToolInput, and direct tool calls.",
            "category": "other",
            "verdict": "helpful"
        }
    });
    client
        .send(call_tool_request(
            221,
            "prism_query",
            json!({
                "code": format!(
                    "return prism.validateToolInput(\"prism_mutate\", {});",
                    feedback_input
                )
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let feedback_validation = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(feedback_validation["result"]["valid"], Value::Bool(true));

    client
        .send(call_tool_request(
            222,
            "prism_mutate",
            feedback_input
                .as_object()
                .expect("feedback input should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let feedback = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(feedback["action"], Value::from("validation_feedback"));
    assert!(feedback["result"]["entryId"].as_str().is_some());
    assert!(feedback["result"]["taskId"].as_str().is_some());

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_prism_mutate_when_capability_is_denied() {
    let root = temp_workspace();
    let (session, owner_credential) = workspace_session_with_owner_credential(&root);
    let _ = register_test_agent_worktree(&root);
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
                kind: PrincipalKind::Service,
                name: "Memory Service".to_string(),
                role: Some("memory_only_service".to_string()),
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
                    "payload": { "title": "Try a coordination write with repo-memory-only capabilities", "goal": "Try a coordination write with repo-memory-only capabilities"
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

#[tokio::test]
async fn mcp_server_rejects_authenticated_mutation_from_second_principal_on_same_worktree() {
    let root = temp_workspace();
    let (session, owner_credential) = workspace_session_with_owner_credential(&root);
    let _ = register_test_agent_worktree(&root);
    let owner = session
        .authenticate_principal_credential(
            &CredentialId::new(owner_credential.credential_id.clone()),
            &owner_credential.principal_token,
        )
        .expect("owner credential should authenticate");
    let first_worker = session
        .mint_principal_credential(
            &owner,
            MintPrincipalRequest {
                authority_id: None,
                kind: PrincipalKind::Service,
                name: "First Service".to_string(),
                role: Some("first_service".to_string()),
                parent_principal_id: Some(PrincipalId::new(owner.principal.principal_id.0.clone())),
                capabilities: vec![CredentialCapability::All],
                profile: Value::Null,
            },
        )
        .expect("first child principal should mint");
    let second_worker = session
        .mint_principal_credential(
            &owner,
            MintPrincipalRequest {
                authority_id: None,
                kind: PrincipalKind::Service,
                name: "Second Service".to_string(),
                role: Some("second_service".to_string()),
                parent_principal_id: Some(PrincipalId::new(owner.principal.principal_id.0.clone())),
                capabilities: vec![CredentialCapability::All],
                profile: Value::Null,
            },
        )
        .expect("second child principal should mint");
    let first_worker_credential = json!({
        "credentialId": first_worker.credential.credential_id.0,
        "principalToken": first_worker.principal_token,
    });
    let second_worker_credential = json!({
        "credentialId": second_worker.credential.credential_id.0,
        "principalToken": second_worker.principal_token,
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
                "action": "declare_work",
                "credential": first_worker_credential.clone(),
                "input": {
                    "title": "Bind the worktree to the first service principal"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "credential": first_worker_credential,
                "input": {
                    "context": "Bind the workspace to the first service principal before a second service attempts to mutate it.",
                    "prismSaid": "First authenticated mutation should bind the worktree principal.",
                    "actuallyTrue": "The first authenticated service session to mutate the worktree holds the active mutator slot until it goes stale or a human explicitly takes it over.",
                    "category": "coordination",
                    "verdict": "helpful"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let owner_response = response_json(client.receive().await.unwrap());
    assert!(owner_response.get("error").is_none(), "{owner_response}");

    client
        .send(call_tool_request(
            4,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "credential": second_worker_credential,
                "input": {
                    "context": "Try to mutate the same worktree from a different authenticated service principal.",
                    "prismSaid": "Another service principal should be able to reuse the same worktree if it has valid credentials.",
                    "actuallyTrue": "Authenticated mutations are exclusive to the currently active worktree mutator session, even when both principals are valid service identities for the same machine.",
                    "category": "coordination",
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
    assert_eq!(
        response["error"]["data"]["code"],
        Value::String("mutation_worktree_mutator_slot_conflict".to_string())
    );
    assert_eq!(
        response["error"]["data"]["currentOwner"]["principalId"],
        Value::String(first_worker.principal.principal_id.0.to_string())
    );
    assert_eq!(
        response["error"]["data"]["attemptedPrincipal"]["principalId"],
        Value::String(second_worker.principal.principal_id.0.to_string())
    );
    assert!(response["error"]["data"]["currentOwner"]["sessionId"]
        .as_str()
        .is_some_and(|value| value.starts_with("session:")));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_authenticated_mutation_on_unregistered_worktree() {
    let root = temp_workspace();
    let session = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            runtime_mode: prism_core::PrismRuntimeMode::Full,
            shared_runtime: default_workspace_shared_runtime(&root)
                .expect("default shared runtime should resolve"),
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should index");
    let issued = session
        .bootstrap_owner_principal(BootstrapOwnerInput {
            authority_id: None,
            name: "Test Owner".to_string(),
            role: Some("test_owner".to_string()),
        })
        .expect("owner bootstrap should succeed");
    let credential = crate::tests_support::MutationCredentialFixture {
        credential_id: issued.credential.credential_id.0.to_string(),
        principal_id: issued.principal.principal_id.0.to_string(),
        principal_token: issued.principal_token,
    };
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
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Try direct mutation without registering the worktree"
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
        Value::String("mutation_worktree_unregistered".to_string())
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_allows_human_authenticated_mutation_on_registered_human_worktree() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let _ = register_test_human_worktree(&root);
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
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Direct human mutation from a registered human worktree"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let declared = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        declared["action"],
        Value::String("declare_work".to_string())
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_human_authenticated_mutation_on_agent_worktree() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let _ = register_test_agent_worktree(&root);
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
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Human direct mutation on an agent worktree should fail"
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
        Value::String("mutation_worktree_mode_mismatch".to_string())
    );
    assert_eq!(
        response["error"]["data"]["requiredWorktreeMode"],
        Value::String("human".to_string())
    );
    assert_eq!(
        response["error"]["data"]["worktreeMode"],
        Value::String("agent".to_string())
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_authenticated_mutation_without_declared_work_context() {
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

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "context": "Attempt an authenticated mutation before the agent declares work.",
                    "prismSaid": "Authenticated mutations can infer intent from session state.",
                    "actuallyTrue": "Authenticated mutations must reject until the agent declares work explicitly.",
                    "category": "coordination",
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
    assert_eq!(
        response["error"]["data"]["code"],
        Value::String("mutation_declared_work_required".to_string())
    );
    assert_eq!(
        response["error"]["data"]["action"],
        Value::String("validation_feedback".to_string())
    );
    let next_action = response["error"]["data"]["nextAction"]
        .as_str()
        .unwrap_or_default();
    assert!(next_action.contains("declare_work"), "{next_action}");

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_heartbeat_lease_mutation_round_trip() {
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
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Exercise claim and heartbeat mutations"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "claim",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "action": "acquire",
                    "payload": {
                        "anchors": [],
                        "capability": "edit"
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
    let claim_id = claim["result"]["claimId"]
        .as_str()
        .expect("claim id should be present")
        .to_string();
    assert!(claim["result"]["state"]["worktreeId"].as_str().is_some());
    assert!(claim["result"]["state"]["agent"].is_null());
    let event_count_before_heartbeat = server_handle
        .host
        .current_prism()
        .coordination_events()
        .len();

    client
        .send(call_tool_request(
            4,
            "prism_mutate",
            json!({
                "action": "heartbeat_lease",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "claimId": claim_id
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let heartbeat = first_tool_content_json(client.receive().await.unwrap());

    assert_eq!(heartbeat["action"], "heartbeat_lease");
    assert_eq!(heartbeat["result"]["claimId"], claim["result"]["claimId"]);
    assert_eq!(
        heartbeat["result"]["state"]["id"],
        claim["result"]["state"]["id"]
    );
    assert_eq!(heartbeat["result"]["rejected"], Value::Bool(false));
    assert_eq!(
        heartbeat["result"]["state"]["refreshedAt"],
        claim["result"]["state"]["refreshedAt"]
    );
    assert_eq!(
        server_handle
            .host
            .current_prism()
            .coordination_events()
            .len(),
        event_count_before_heartbeat
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_coordination_mutation_round_trip_in_coordination_only_mode() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_runtime_mode(PrismRuntimeMode::CoordinationOnly),
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
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Exercise coordination mutations in reduced runtime mode"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "plan_create",
                    "payload": {
                        "title": "Coordinate reduced runtime mutation",
                        "goal": "Verify coordination mutations remain executable in coordination_only mode"
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

    assert_eq!(plan["action"], "coordination");
    assert_eq!(plan["result"]["state"]["status"], "pending");
    assert_eq!(
        plan["result"]["state"]["title"],
        "Coordinate reduced runtime mutation"
    );
    assert!(plan["result"]["state"]["id"].as_str().is_some());

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_accepts_relative_file_anchor_paths_in_coordination_only_mode() {
    let root = temp_workspace();
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_runtime_mode(PrismRuntimeMode::CoordinationOnly),
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
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Exercise relative file anchors in reduced runtime mode"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "plan_bootstrap",
                    "payload": {
                        "plan": {
                            "title": "Coordinate reduced runtime file anchors",
                            "goal": "Verify coordination-only plan bootstrap accepts workspace-relative file anchor paths"
                        },
                        "tasks": [{
                            "clientId": "t0",
                            "title": "Create file-anchored task",
                            "anchors": [{
                                "type": "file",
                                "path": "src/lib.rs"
                            }]
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
    let bootstrap = first_tool_content_json(client.receive().await.unwrap());

    assert_eq!(bootstrap["action"], "coordination");
    assert!(bootstrap["result"]["state"]["id"].as_str().is_some());
    assert!(bootstrap["result"]["state"]["taskIdsByClientId"]["t0"]
        .as_str()
        .is_some());

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_rejects_absolute_file_anchor_paths_in_coordination_only_mode() {
    let root = temp_workspace();
    let absolute_file = root.join("src/lib.rs");
    let (session, credential) = workspace_session_with_owner_credential(&root);
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_runtime_mode(PrismRuntimeMode::CoordinationOnly),
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
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "declare_work",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "title": "Reject absolute file anchors in reduced runtime mode"
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();
    let declared_work = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(declared_work["action"], "declare_work");

    client
        .send(call_tool_request(
            3,
            "prism_mutate",
            json!({
                "action": "coordination",
                "credential": mutation_credential_json(&credential),
                "input": {
                    "kind": "plan_bootstrap",
                    "payload": {
                        "plan": {
                            "title": "Reject absolute file anchors",
                            "goal": "Verify coordination-only mode rejects absolute file anchor paths"
                        },
                        "tasks": [{
                            "clientId": "t0",
                            "title": "Reject absolute path",
                            "anchors": [{
                                "type": "file",
                                "path": absolute_file.to_string_lossy()
                            }]
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
    let response = response_json(client.receive().await.unwrap());
    assert_eq!(response["error"]["code"], -32603);
    assert!(
        response["error"]["data"]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("coordination-only mode only accepts workspace-relative file anchor paths"),
        "{response}"
    );

    running.cancel().await.unwrap();
}
