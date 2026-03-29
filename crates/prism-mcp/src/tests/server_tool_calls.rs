use rmcp::transport::{IntoTransport, Transport};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    call_tool_request, demo_node, first_tool_content_json, initialize_client,
    initialized_notification, response_json, server_with_node,
};

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
    assert!(message.contains("prism.validateToolInput(\"prism_mutate\", <input>)"));
    assert!(message.contains("prism://schema/tool/prism_mutate/action/validation_feedback"));

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
