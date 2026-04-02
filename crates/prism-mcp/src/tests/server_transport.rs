use std::fs;
use std::time::Duration;

use rmcp::{
    model::{CallToolRequestParams, ReadResourceRequestParams},
    transport::{IntoTransport, Transport},
    ServiceExt,
};
use serde_json::{json, Value};

use super::*;
use crate::runtime_state::{default_runtime_state_path, RuntimeProcessRecord, RuntimeState};
use crate::tests_support::{
    call_tool_request, demo_node, first_tool_content_json, initialize_client,
    initialized_notification, list_tools_request, read_resource_request, response_json,
    server_with_node, server_with_node_and_features, spawn_http_upstream, temp_workspace,
    test_session, workspace_session_with_owner_credential,
};
use crate::{PrismMcpCli, PrismMcpMode};
use prism_core::{
    index_workspace_session_with_options, CredentialProfile, CredentialsFile, SharedRuntimeBackend,
    WorkspaceSessionOptions,
};
use prism_ir::CredentialId;
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
    let session_text = match &session.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual session resource, got {other:?}"),
    };
    let session_payload =
        serde_json::from_str::<Value>(session_text).expect("session resource should be valid json");
    assert_eq!(session_payload["bridgeIdentity"]["status"], "unbound");

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
async fn bootstrap_proxy_exposes_startup_resource_and_warmup_errors_before_ready() {
    let root = temp_workspace();
    let proxy =
        crate::proxy_server::ProxyMcpServer::pending_for_test(&root, PrismMcpFeatures::full())
            .expect("pending proxy should build");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        proxy
            .serve_transport(server_transport)
            .await
            .expect("pending proxy should serve stdio");
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let resources = client
        .list_all_resources()
        .await
        .expect("pending proxy should list bootstrap resources");
    assert!(resources.iter().any(|resource| resource.uri == STARTUP_URI));

    let startup = client
        .read_resource(ReadResourceRequestParams::new(STARTUP_URI))
        .await
        .expect("startup resource should be readable before the daemon is ready");
    let startup_text = match &startup.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual startup resource, got {other:?}"),
    };
    let startup_payload =
        serde_json::from_str::<Value>(startup_text).expect("startup resource should be valid json");
    assert_eq!(startup_payload["ready"], false);
    assert_eq!(startup_payload["uri"], STARTUP_URI);

    let tools = client
        .list_all_tools()
        .await
        .expect("pending proxy should still expose bootstrap tools");
    assert!(tools.iter().any(|tool| tool.name == "prism_query"));

    let warmup_error = client
        .call_tool(CallToolRequestParams::new("prism_query").with_arguments(
            serde_json::Map::from_iter([(String::from("code"), json!("return 'not-yet';"))]),
        ))
        .await
        .expect("pending proxy should return a tool error payload");
    assert!(warmup_error.is_error.unwrap_or(false));
    let warmup_text = warmup_error
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|text| text.text.clone())
        .unwrap_or_default();
    assert!(warmup_text.contains(STARTUP_URI));

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
}

#[tokio::test]
async fn failed_bootstrap_proxy_recovers_once_an_upstream_becomes_available() {
    let uri_file = temp_workspace().join("bridge-uri.txt");
    let root = temp_workspace();
    let proxy = crate::proxy_server::ProxyMcpServer::failed_for_test(
        &root,
        PrismMcpFeatures::full(),
        "bootstrap detach failed",
        crate::daemon_mode::BridgeUpstreamSource::HttpUriFile(uri_file.clone()),
    )
    .expect("failed proxy should build");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        proxy
            .serve_transport(server_transport)
            .await
            .expect("failed proxy should serve stdio");
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let startup = client
        .read_resource(ReadResourceRequestParams::new(STARTUP_URI))
        .await
        .expect("startup resource should expose the failed state");
    let startup_text = match &startup.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual startup resource, got {other:?}"),
    };
    let startup_payload =
        serde_json::from_str::<Value>(startup_text).expect("startup resource should be valid json");
    assert_eq!(startup_payload["ready"], false);
    assert_eq!(startup_payload["phase"], "failed");

    let (upstream_uri, upstream_task) = spawn_http_upstream(server_with_node(demo_node())).await;
    fs::write(&uri_file, format!("{upstream_uri}\n")).expect("uri file should be written");

    let query = tokio::time::timeout(
        Duration::from_secs(10),
        client.call_tool(CallToolRequestParams::new("prism_query").with_arguments(
            serde_json::Map::from_iter([(String::from("code"), json!("return 'recovered';"))]),
        )),
    )
    .await
    .expect("recovery query should complete before the timeout")
    .expect("failed proxy should reconnect once the upstream is available");
    let query_payload = query.structured_content.unwrap_or_else(|| {
        serde_json::from_str(
            &query.content[0]
                .as_text()
                .expect("query result should expose text content")
                .text,
        )
        .expect("query text content should be valid json")
    });
    assert_eq!(query_payload["result"], "recovered");

    let startup = client
        .read_resource(ReadResourceRequestParams::new(STARTUP_URI))
        .await
        .expect("startup resource should reflect the recovered state");
    let startup_text = match &startup.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual startup resource, got {other:?}"),
    };
    let startup_payload =
        serde_json::from_str::<Value>(startup_text).expect("startup resource should be valid json");
    assert_eq!(startup_payload["ready"], true);
    assert_eq!(startup_payload["phase"], "ready");
    assert_eq!(startup_payload["upstreamUri"], upstream_uri);

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
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
    let first_result = first_query
        .structured_content
        .as_ref()
        .map(|payload| payload["result"].clone())
        .or_else(|| {
            first_query
                .content
                .first()
                .and_then(|content| content.as_text())
                .map(|text| {
                    serde_json::from_str::<Value>(&text.text)
                        .ok()
                        .and_then(|payload| payload.get("result").cloned())
                        .unwrap_or_else(|| Value::String(text.text.clone()))
                })
        })
        .expect("query result should expose a value");
    assert_eq!(first_result, "demo::main");

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

#[tokio::test]
async fn bootstrap_proxy_self_heals_stale_uri_file_from_runtime_state() {
    let root = temp_workspace();
    let cli = PrismMcpCli {
        root: root.clone(),
        mode: PrismMcpMode::Bridge,
        no_coordination: false,
        internal_developer: false,
        enable_coordination: Vec::new(),
        disable_coordination: Vec::new(),
        enable_query_view: Vec::new(),
        disable_query_view: Vec::new(),
        daemon_log: None,
        shared_runtime_sqlite: None,
        shared_runtime_uri: None,
        restart_nonce: None,
        daemon_start_timeout_ms: Some(500),
        http_bind: "127.0.0.1:0".to_string(),
        http_path: "/mcp".to_string(),
        health_path: "/healthz".to_string(),
        http_uri_file: None,
        upstream_uri: None,
        bootstrap_build_worktree_release: false,
        bridge_daemon_binary: None,
        daemonize: false,
    };
    let uri_file = cli
        .http_uri_file_path(&root)
        .expect("uri file path should resolve");
    fs::create_dir_all(
        uri_file
            .parent()
            .expect("uri file should have a parent directory"),
    )
    .expect("uri file parent should exist");

    let (first_uri, first_upstream_task) = spawn_http_upstream(server_with_node(demo_node())).await;
    fs::write(&uri_file, format!("{first_uri}\n")).expect("uri file should be written");

    let upstream_source = crate::daemon_mode::BridgeUpstreamSource::from_cli(&cli, &root)
        .expect("upstream source should build");
    let proxy = crate::proxy_server::ProxyMcpServer::bootstrap_with_source_for_root(
        &root,
        cli.clone(),
        upstream_source,
    )
    .await
    .expect("bootstrap proxy should initialize");
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        proxy
            .serve_transport(server_transport)
            .await
            .expect("bootstrap proxy should serve stdio");
    });
    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let query_result = |response: &rmcp::model::CallToolResult| {
        response
            .structured_content
            .as_ref()
            .map(|payload| payload["result"].clone())
            .or_else(|| {
                response
                    .content
                    .first()
                    .and_then(|content| content.as_text())
                    .map(|text| {
                        serde_json::from_str::<Value>(&text.text)
                            .ok()
                            .and_then(|payload| payload.get("result").cloned())
                            .unwrap_or_else(|| Value::String(text.text.clone()))
                    })
            })
            .expect("query result should expose a value")
    };

    let first_result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let response = client
                .call_tool(CallToolRequestParams::new("prism_query").with_arguments(
                    serde_json::Map::from_iter([(
                        String::from("code"),
                        json!(r#"return prism.symbol("main")?.id.path ?? null;"#),
                    )]),
                ))
                .await
                .expect("proxy should forward the first query");
            let result = query_result(&response);
            if result == "demo::main" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("bootstrap query should complete before the timeout");
    assert_eq!(first_result, "demo::main");

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

    let runtime_state_path =
        default_runtime_state_path(&root).expect("runtime state path should resolve");
    fs::create_dir_all(
        runtime_state_path
            .parent()
            .expect("runtime state path should have a parent directory"),
    )
    .expect("runtime state parent should exist");
    let stale_pid = 999_997_u32;
    fs::write(
        &runtime_state_path,
        serde_json::to_vec_pretty(&RuntimeState {
            processes: vec![RuntimeProcessRecord {
                pid: stale_pid,
                kind: "daemon".to_string(),
                started_at: 1,
                health_path: Some("/healthz".to_string()),
                http_uri: Some(second_uri.clone()),
                upstream_uri: None,
                restart_nonce: Some("replacement-daemon".to_string()),
            }],
            events: Vec::new(),
        })
        .expect("runtime state should serialize"),
    )
    .expect("runtime state should be written");

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
    .expect("self-healing reconnect should complete before the timeout")
    .expect("bootstrapped proxy should recover using runtime-state daemon candidates");
    let second_result = query_result(&second_query);
    assert_eq!(second_result, "demo::replacement");
    let runtime_state = crate::runtime_state::read_runtime_state(&root)
        .expect("runtime state should be readable")
        .expect("runtime state should exist after reconnect");
    assert!(runtime_state.events.iter().any(|event| {
        event.message == "prism-mcp observed dead runtime process"
            && event.fields["process"]["pid"] == stale_pid
    }));
    assert!(runtime_state.events.iter().any(|event| {
        event.message == "prism-mcp bridge resolved upstream"
            && event.fields["upstreamUri"] == second_uri
    }));

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    second_upstream_task.abort();
    let _ = second_upstream_task.await;
}

#[tokio::test]
async fn stdio_proxy_can_adopt_local_profile_and_mutate_without_explicit_credential() {
    let root = temp_workspace();
    let (workspace, credential) = workspace_session_with_owner_credential(&root);
    let authenticated = workspace
        .authenticate_principal_credential(
            &CredentialId::new(credential.credential_id.clone()),
            &credential.principal_token,
        )
        .expect("owner credential should authenticate");
    let credentials_path = root.join("bridge-credentials.toml");
    let mut credentials = CredentialsFile {
        version: 1,
        ..CredentialsFile::default()
    };
    credentials.upsert_profile(
        CredentialProfile {
            profile: "agent-a".to_string(),
            authority_id: authenticated.principal.authority_id.0.to_string(),
            principal_id: authenticated.principal.principal_id.0.to_string(),
            credential_id: credential.credential_id.clone(),
            principal_token: credential.principal_token.clone(),
        },
        true,
    );
    credentials
        .save(&credentials_path)
        .expect("bridge credentials should save");

    let upstream = PrismMcpServer::with_session_and_features(workspace, PrismMcpFeatures::full());
    let (upstream_uri, upstream_task) = spawn_http_upstream(upstream).await;
    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_credentials_path(
        credentials_path,
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
            .expect("proxy should serve the local bridge");
    });

    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    let unbound_error = client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge auth smoke test.",
                        "prismSaid": "A bridge-bound mutation should work without an explicit credential.",
                        "actuallyTrue": "The bridge injected the locally stored credential after adoption.",
                        "category": "coordination",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect_err("unbound bridge should reject credential-less mutations");
    assert!(
        unbound_error.to_string().contains("bridge_auth_required"),
        "{}",
        unbound_error
    );

    let adopt = client
        .call_tool(
            CallToolRequestParams::new("prism_bridge_adopt").with_arguments(
                serde_json::Map::from_iter([("profile".to_string(), json!("agent-a"))]),
            ),
        )
        .await
        .expect("bridge adopt should succeed");
    let adopt_payload = adopt
        .structured_content
        .expect("bridge adopt should return structured content");
    assert_eq!(adopt_payload["status"], "bound");
    assert_eq!(adopt_payload["profile"], "agent-a");

    let declare_work = client
        .call_tool(CallToolRequestParams::new("prism_mutate").with_arguments(
            serde_json::Map::from_iter([
                ("action".to_string(), json!("declare_work")),
                (
                    "input".to_string(),
                    json!({
                        "title": "Bridge auth smoke test"
                    }),
                ),
            ]),
        ))
        .await
        .expect("bound bridge should allow declare_work without an explicit credential");
    let declare_work_payload = declare_work
        .structured_content
        .expect("declare_work result should be structured");
    assert_eq!(declare_work_payload["action"], "declare_work");

    let mutation = client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge auth smoke test.",
                        "prismSaid": "A bridge-bound mutation should work without an explicit credential.",
                        "actuallyTrue": "The bridge injected the locally stored credential after adoption.",
                        "category": "coordination",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect("bound bridge should inject credentials for prism_mutate");
    let mutation_payload = mutation
        .structured_content
        .expect("mutation result should be structured");
    assert_eq!(mutation_payload["action"], "validation_feedback");

    let bridge_auth = client
        .read_resource(ReadResourceRequestParams::new("prism://bridge/auth"))
        .await
        .expect("bridge auth resource should be readable");
    let bridge_auth_text = match &bridge_auth.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual bridge auth resource, got {other:?}"),
    };
    let bridge_auth_payload = serde_json::from_str::<Value>(bridge_auth_text)
        .expect("bridge auth resource should be valid json");
    assert_eq!(bridge_auth_payload["status"], "bound");
    assert_eq!(bridge_auth_payload["profile"], "agent-a");

    let session = client
        .read_resource(ReadResourceRequestParams::new(SESSION_URI))
        .await
        .expect("session resource should be readable");
    let session_text = match &session.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual session resource, got {other:?}"),
    };
    let session_payload =
        serde_json::from_str::<Value>(session_text).expect("session resource should be valid json");
    assert_eq!(session_payload["bridgeIdentity"]["status"], "bound");
    assert_eq!(session_payload["bridgeIdentity"]["profile"], "agent-a");
    assert_eq!(
        session_payload["bridgeIdentity"]["principalId"],
        authenticated.principal.principal_id.0.as_str()
    );
    assert_eq!(
        session_payload["bridgeIdentity"]["credentialId"],
        credential.credential_id.as_str()
    );

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    upstream_task.abort();
    let _ = upstream_task.await;
}

#[tokio::test]
async fn stdio_proxy_keeps_bound_bridge_auth_across_long_daemon_restart_gap() {
    let root = temp_workspace();
    let shared_runtime_root = temp_workspace();
    let shared_runtime_sqlite = shared_runtime_root.join("shared-runtime.db");
    let workspace = prism_core::index_workspace_session_with_options(
        &root,
        prism_core::WorkspaceSessionOptions {
            coordination: true,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: shared_runtime_sqlite.clone(),
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should index");
    let issued = workspace
        .bootstrap_owner_principal(prism_core::BootstrapOwnerInput {
            authority_id: None,
            name: "Test Owner".to_string(),
            role: Some("test_owner".to_string()),
        })
        .expect("owner bootstrap should succeed");
    let authenticated = workspace
        .authenticate_principal_credential(
            &issued.credential.credential_id,
            &issued.principal_token,
        )
        .expect("owner credential should authenticate");
    let credentials_path = root.join("bridge-credentials.toml");
    let mut credentials = CredentialsFile {
        version: 1,
        ..CredentialsFile::default()
    };
    credentials.upsert_profile(
        CredentialProfile {
            profile: "agent-a".to_string(),
            authority_id: authenticated.principal.authority_id.0.to_string(),
            principal_id: authenticated.principal.principal_id.0.to_string(),
            credential_id: issued.credential.credential_id.0.to_string(),
            principal_token: issued.principal_token.clone(),
        },
        true,
    );
    credentials
        .save(&credentials_path)
        .expect("bridge credentials should save");

    let uri_file = root.join("bridge-uri.txt");
    let first_upstream =
        PrismMcpServer::with_session_and_features(workspace, PrismMcpFeatures::full());
    let (first_uri, first_upstream_task) = spawn_http_upstream(first_upstream).await;
    fs::write(&uri_file, format!("{first_uri}\n")).expect("uri file should be written");

    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_credentials_path(
        credentials_path.clone(),
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
            .expect("proxy should serve the local bridge");
    });
    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    client
        .call_tool(
            CallToolRequestParams::new("prism_bridge_adopt").with_arguments(
                serde_json::Map::from_iter([("profile".to_string(), json!("agent-a"))]),
            ),
        )
        .await
        .expect("bridge adopt should succeed");

    client
        .call_tool(CallToolRequestParams::new("prism_mutate").with_arguments(
            serde_json::Map::from_iter([
                ("action".to_string(), json!("declare_work")),
                (
                    "input".to_string(),
                    json!({
                        "title": "Bridge restart smoke test"
                    }),
                ),
            ]),
        ))
        .await
        .expect("declare_work should succeed before the first bridge mutation");

    client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge restart smoke test.",
                        "prismSaid": "The bridge should reconnect after a daemon restart.",
                        "actuallyTrue": "The bridge kept its bound principal and resumed mutation forwarding after the daemon came back.",
                        "category": "freshness",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect("initial mutation should succeed");

    first_upstream_task.abort();
    let _ = first_upstream_task.await;

    tokio::time::sleep(Duration::from_secs(8)).await;

    let reloaded = prism_core::hydrate_workspace_session_with_options(
        &root,
        prism_core::WorkspaceSessionOptions {
            coordination: true,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: shared_runtime_sqlite.clone(),
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should hydrate after restart");
    reloaded
        .authenticate_principal_credential(
            &issued.credential.credential_id,
            &issued.principal_token,
        )
        .expect("reloaded workspace should still authenticate the bridge-bound credential");

    let second_upstream = PrismMcpServer::from_workspace_with_features_and_shared_runtime(
        &root,
        PrismMcpFeatures::full(),
        SharedRuntimeBackend::Sqlite {
            path: shared_runtime_sqlite.clone(),
        },
    )
    .expect("replacement workspace-backed server should build");
    let (second_uri, second_upstream_task) = spawn_http_upstream(second_upstream).await;
    fs::write(&uri_file, format!("{second_uri}\n")).expect("uri file should be updated");

    let mutation = tokio::time::timeout(
        Duration::from_secs(20),
        client.call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge restart smoke test.",
                        "prismSaid": "The bridge should reconnect after a daemon restart.",
                        "actuallyTrue": "The bridge kept its bound principal and resumed mutation forwarding after the daemon came back.",
                        "category": "freshness",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        ),
    )
    .await
    .expect("mutation should survive a long daemon restart gap")
    .expect("bound bridge should reconnect and inject credentials after restart");
    let mutation_payload = mutation
        .structured_content
        .expect("mutation result should be structured after reconnect");
    assert_eq!(mutation_payload["action"], "validation_feedback");

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    second_upstream_task.abort();
    let _ = second_upstream_task.await;
    let _ = fs::remove_dir_all(shared_runtime_root);
}

#[tokio::test]
async fn stdio_proxy_marks_bridge_auth_stale_after_upstream_rejects_bound_credential() {
    let root = temp_workspace();
    let first_shared_runtime_root = temp_workspace();
    let second_shared_runtime_root = temp_workspace();
    let first_shared_runtime_sqlite = first_shared_runtime_root.join("shared-runtime.db");
    let second_shared_runtime_sqlite = second_shared_runtime_root.join("shared-runtime.db");
    let workspace = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            coordination: true,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: first_shared_runtime_sqlite.clone(),
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("workspace session should index");
    let issued = workspace
        .bootstrap_owner_principal(prism_core::BootstrapOwnerInput {
            authority_id: None,
            name: "Test Owner".to_string(),
            role: Some("test_owner".to_string()),
        })
        .expect("owner bootstrap should succeed");
    let authenticated = workspace
        .authenticate_principal_credential(
            &issued.credential.credential_id,
            &issued.principal_token,
        )
        .expect("owner credential should authenticate");
    let credentials_path = root.join("bridge-credentials.toml");
    let mut credentials = CredentialsFile {
        version: 1,
        ..CredentialsFile::default()
    };
    credentials.upsert_profile(
        CredentialProfile {
            profile: "agent-a".to_string(),
            authority_id: authenticated.principal.authority_id.0.to_string(),
            principal_id: authenticated.principal.principal_id.0.to_string(),
            credential_id: issued.credential.credential_id.0.to_string(),
            principal_token: issued.principal_token.clone(),
        },
        true,
    );
    credentials
        .save(&credentials_path)
        .expect("bridge credentials should save");

    let uri_file = root.join("bridge-uri.txt");
    let first_upstream =
        PrismMcpServer::with_session_and_features(workspace, PrismMcpFeatures::full());
    let (first_uri, first_upstream_task) = spawn_http_upstream(first_upstream).await;
    fs::write(&uri_file, format!("{first_uri}\n")).expect("uri file should be written");

    let proxy = crate::proxy_server::ProxyMcpServer::connect_with_credentials_path(
        credentials_path.clone(),
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
            .expect("proxy should serve the local bridge");
    });
    let client = ().serve(client_transport).await.expect("client should connect through proxy");

    client
        .call_tool(
            CallToolRequestParams::new("prism_bridge_adopt").with_arguments(
                serde_json::Map::from_iter([("profile".to_string(), json!("agent-a"))]),
            ),
        )
        .await
        .expect("bridge adopt should succeed");

    client
        .call_tool(CallToolRequestParams::new("prism_mutate").with_arguments(
            serde_json::Map::from_iter([
                ("action".to_string(), json!("declare_work")),
                (
                    "input".to_string(),
                    json!({
                        "title": "Bridge stale auth smoke test"
                    }),
                ),
            ]),
        ))
        .await
        .expect("declare_work should succeed before the first bridge mutation");

    client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge stale auth smoke test.",
                        "prismSaid": "A bridge-bound mutation should work without an explicit credential.",
                        "actuallyTrue": "The bridge injected the locally stored credential after adoption.",
                        "category": "coordination",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect("initial mutation should succeed");

    first_upstream_task.abort();
    let _ = first_upstream_task.await;

    let second_workspace = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            coordination: true,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: second_shared_runtime_sqlite.clone(),
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: false,
        },
    )
    .expect("replacement workspace session should index");
    let second_issued = second_workspace
        .bootstrap_owner_principal(prism_core::BootstrapOwnerInput {
            authority_id: None,
            name: "Replacement Owner".to_string(),
            role: Some("test_owner".to_string()),
        })
        .expect("replacement owner bootstrap should succeed");
    let second_authenticated = second_workspace
        .authenticate_principal_credential(
            &second_issued.credential.credential_id,
            &second_issued.principal_token,
        )
        .expect("replacement owner credential should authenticate");
    credentials.upsert_profile(
        CredentialProfile {
            profile: "agent-a".to_string(),
            authority_id: second_authenticated.principal.authority_id.0.to_string(),
            principal_id: second_authenticated.principal.principal_id.0.to_string(),
            credential_id: second_issued.credential.credential_id.0.to_string(),
            principal_token: second_issued.principal_token.clone(),
        },
        true,
    );
    credentials
        .save(&credentials_path)
        .expect("bridge credentials should update to the replacement credential");

    let second_upstream =
        PrismMcpServer::with_session_and_features(second_workspace, PrismMcpFeatures::full());
    let (second_uri, second_upstream_task) = spawn_http_upstream(second_upstream).await;
    fs::write(&uri_file, format!("{second_uri}\n")).expect("uri file should be updated");

    let stale_error = tokio::time::timeout(
        Duration::from_secs(20),
        client.call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge stale auth smoke test.",
                        "prismSaid": "A bridge-bound mutation should keep working forever.",
                        "actuallyTrue": "The bridge should mark itself stale when the upstream rejects the injected credential after restart.",
                        "category": "freshness",
                        "verdict": "wrong",
                        "correctedManually": false,
                    }),
                ),
            ])),
        ),
    )
    .await
    .expect("stale-auth mutation should complete before the timeout")
    .expect_err("replacement upstream should reject the stale bridge credential");
    assert!(
        stale_error.to_string().contains("mutation_auth_failed"),
        "{}",
        stale_error
    );

    let bridge_auth = client
        .read_resource(ReadResourceRequestParams::new("prism://bridge/auth"))
        .await
        .expect("bridge auth resource should still be readable");
    let bridge_auth_text = match &bridge_auth.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual bridge auth resource, got {other:?}"),
    };
    let bridge_auth_payload = serde_json::from_str::<Value>(bridge_auth_text)
        .expect("bridge auth resource should be valid json");
    assert_eq!(bridge_auth_payload["status"], "stale");
    assert_eq!(bridge_auth_payload["profile"], "agent-a");
    assert_eq!(
        bridge_auth_payload["credentialId"].as_str(),
        Some(issued.credential.credential_id.0.as_str())
    );

    let stale_bridge_error = client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge stale auth smoke test.",
                        "prismSaid": "The bridge should keep retrying the stale credential.",
                        "actuallyTrue": "The bridge should fail locally with a stale-binding error until it is re-adopted.",
                        "category": "freshness",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect_err("stale bridge should stop forwarding credential-less mutations");
    assert!(
        stale_bridge_error.to_string().contains("bridge_auth_stale"),
        "{}",
        stale_bridge_error
    );

    let readopt = client
        .call_tool(
            CallToolRequestParams::new("prism_bridge_adopt").with_arguments(
                serde_json::Map::from_iter([("profile".to_string(), json!("agent-a"))]),
            ),
        )
        .await
        .expect("bridge should re-adopt after the local credential is refreshed");
    let readopt_payload = readopt
        .structured_content
        .expect("readopt should return structured content");
    assert_eq!(readopt_payload["status"], "bound");
    assert_eq!(
        readopt_payload["credentialId"].as_str(),
        Some(second_issued.credential.credential_id.0.as_str())
    );

    client
        .call_tool(CallToolRequestParams::new("prism_mutate").with_arguments(
            serde_json::Map::from_iter([
                ("action".to_string(), json!("declare_work")),
                (
                    "input".to_string(),
                    json!({
                        "title": "Bridge stale auth recovery smoke test"
                    }),
                ),
            ]),
        ))
        .await
        .expect(
            "re-adopted bridge should be able to declare fresh work on the replacement authority",
        );

    let recovered_mutation = client
        .call_tool(
            CallToolRequestParams::new("prism_mutate").with_arguments(serde_json::Map::from_iter([
                ("action".to_string(), json!("validation_feedback")),
                (
                    "input".to_string(),
                    json!({
                        "context": "Bridge stale auth recovery smoke test.",
                        "prismSaid": "Re-adopting the refreshed local credential should restore credential-less mutations.",
                        "actuallyTrue": "The bridge bound the replacement credential and resumed authoritative mutations after re-adoption.",
                        "category": "freshness",
                        "verdict": "helpful",
                        "correctedManually": false,
                    }),
                ),
            ])),
        )
        .await
        .expect("re-adopted bridge should recover credential-less mutations");
    let recovered_payload = recovered_mutation
        .structured_content
        .expect("recovered mutation should be structured");
    assert_eq!(recovered_payload["action"], "validation_feedback");

    let recovered_bridge_auth = client
        .read_resource(ReadResourceRequestParams::new("prism://bridge/auth"))
        .await
        .expect("bridge auth resource should reflect the recovered binding");
    let recovered_bridge_auth_text = match &recovered_bridge_auth.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.as_str(),
        other => panic!("expected textual bridge auth resource, got {other:?}"),
    };
    let recovered_bridge_auth_payload = serde_json::from_str::<Value>(recovered_bridge_auth_text)
        .expect("recovered bridge auth resource should be valid json");
    assert_eq!(recovered_bridge_auth_payload["status"], "bound");
    assert_eq!(
        recovered_bridge_auth_payload["credentialId"].as_str(),
        Some(second_issued.credential.credential_id.0.as_str())
    );

    client.cancel().await.unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    second_upstream_task.abort();
    let _ = second_upstream_task.await;
    let _ = fs::remove_dir_all(first_shared_runtime_root);
    let _ = fs::remove_dir_all(second_shared_runtime_root);
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
    assert_eq!(tool_names.len(), 9);
    assert!(tool_names.contains(&"prism_locate"));
    assert!(tool_names.contains(&"prism_gather"));
    assert!(tool_names.contains(&"prism_open"));
    assert!(tool_names.contains(&"prism_workset"));
    assert!(tool_names.contains(&"prism_expand"));
    assert!(tool_names.contains(&"prism_task_brief"));
    assert!(tool_names.contains(&"prism_concept"));
    assert!(tool_names.contains(&"prism_query"));
    assert!(tool_names.contains(&"prism_mutate"));
    assert!(!tool_names.contains(&"prism_session"));

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
                "credential": {
                    "credentialId": "credential:test",
                    "principalToken": "prism_ptok_test"
                },
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
