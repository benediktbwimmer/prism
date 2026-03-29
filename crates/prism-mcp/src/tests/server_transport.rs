use std::fs;
use std::time::Duration;

use rmcp::{
    model::{CallToolRequestParams, ReadResourceRequestParams},
    transport::{IntoTransport, Transport},
    ServiceExt,
};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    call_tool_request, demo_node, first_tool_content_json, initialize_client,
    initialized_notification, list_tools_request, read_resource_request, response_json,
    server_with_node, server_with_node_and_features, spawn_http_upstream, temp_workspace,
    test_session,
};
use prism_ir::{Language, Node, NodeId, NodeKind, Span};
use prism_store::Graph;

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
