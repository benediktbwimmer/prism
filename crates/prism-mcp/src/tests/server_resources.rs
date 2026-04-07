use rmcp::{
    model::ProtocolVersion,
    transport::{IntoTransport, Transport},
};
use serde_json::{json, Value};

use super::*;
use crate::tests_support::{
    client_message, demo_node, initialize_client, initialized_notification,
    list_resource_templates_request, list_resources_request, list_tools_request, ping_request,
    read_resource_request, response_json, server_with_node, server_with_node_and_features,
    temp_workspace, test_session, wait_until,
};
use prism_core::index_workspace_session;

fn resource_text(response: serde_json::Value) -> String {
    response["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resource should be text: {response}"))
        .to_string()
}

fn text_resource_contents(contents: rmcp::model::ResourceContents) -> String {
    match contents {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text,
        other => panic!("expected textual resource contents, got {other:?}"),
    }
}

fn compact_self_description_resource_text(server: &PrismMcpServer, uri: &str) -> String {
    if let Some(instruction_set_id) = crate::instructions::parse_instruction_resource_uri(uri) {
        return match instruction_set_id {
            None => {
                crate::instructions::render_instructions_index(server.host.features.mode_label())
            }
            Some(id) => {
                crate::instructions::render_instruction_set(&id, server.host.features.mode_label())
                    .unwrap_or_else(|| panic!("instruction set should exist for {uri}"))
            }
        };
    }
    if uri == CAPABILITIES_URI {
        return serde_json::to_string_pretty(
            &server
                .host
                .capabilities_resource_value()
                .expect("capabilities resource should build"),
        )
        .expect("capabilities resource should serialize");
    }
    if uri == SESSION_URI {
        return serde_json::to_string_pretty(
            &server
                .host
                .session_resource_value(server.session.as_ref())
                .expect("session resource should build"),
        )
        .expect("session resource should serialize");
    }
    if uri == VOCAB_URI {
        return serde_json::to_string_pretty(&server.host.vocab_resource_value())
            .expect("vocab resource should serialize");
    }
    if uri == SCHEMAS_URI {
        return serde_json::to_string_pretty(&server.host.schemas_resource_value())
            .expect("schemas resource should serialize");
    }
    if uri == TOOL_SCHEMAS_URI {
        return serde_json::to_string_pretty(&server.host.tool_schemas_resource_value())
            .expect("tool schemas resource should serialize");
    }
    if uri == crate::SELF_DESCRIPTION_AUDIT_URI {
        return text_resource_contents(
            crate::self_description_audit_resource_contents(
                server
                    .host
                    .capabilities_resource_value()
                    .expect("capabilities resource should build"),
                uri,
            )
            .expect("self-description audit resource should build"),
        );
    }
    if let Some((tool_name, action, tag)) = crate::parse_tool_variant_example_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_variant_example_resource_contents(&tool_name, &action, &tag, uri)
                .expect("tool variant example resource should exist"),
        );
    }
    if let Some((tool_name, action, tag)) = crate::parse_tool_variant_shape_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_variant_shape_resource_contents(&tool_name, &action, &tag, uri)
                .expect("tool variant shape resource should exist"),
        );
    }
    if let Some((tool_name, action, tag)) = crate::parse_tool_variant_recipe_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_recipe_resource_contents(&tool_name, &action, Some(&tag), uri)
                .expect("tool variant recipe resource should exist"),
        );
    }
    if let Some((tool_name, action)) = crate::parse_tool_action_example_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_action_example_resource_contents(&tool_name, &action, uri)
                .expect("tool action example resource should exist"),
        );
    }
    if let Some((tool_name, action)) = crate::parse_tool_action_shape_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_action_shape_resource_contents(&tool_name, &action, uri)
                .expect("tool action shape resource should exist"),
        );
    }
    if let Some((tool_name, action)) = crate::parse_tool_action_recipe_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_recipe_resource_contents(&tool_name, &action, None, uri)
                .expect("tool action recipe resource should exist"),
        );
    }
    if let Some(tool_name) = crate::parse_tool_example_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_example_resource_contents(&tool_name, uri)
                .expect("tool example resource should exist"),
        );
    }
    if let Some(tool_name) = crate::parse_tool_shape_resource_uri(uri) {
        return text_resource_contents(
            crate::tool_shape_resource_contents(&tool_name, uri)
                .expect("tool shape resource should exist"),
        );
    }
    if let Some(resource_kind) = crate::parse_resource_example_resource_uri(uri) {
        return text_resource_contents(
            crate::resource_example_resource_contents(&resource_kind, uri)
                .expect("resource example should exist"),
        );
    }
    if let Some(resource_kind) = crate::parse_resource_shape_resource_uri(uri) {
        return text_resource_contents(
            crate::resource_shape_resource_contents(&resource_kind, uri)
                .expect("resource shape should exist"),
        );
    }
    panic!("unsupported compact self-description resource uri: {uri}");
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
    assert!(initialize["result"]["instructions"]
        .as_str()
        .expect("initialize should include instructions")
        .contains("PRISM Instruction Sets"));
    assert!(initialize["result"]["instructions"]
        .as_str()
        .expect("initialize should include instructions")
        .contains("`prism://instructions/execution`"));
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
    for tool in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    let mutate_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "prism_mutate")
        .expect("prism_mutate tool should exist");
    let mutate_schema = mutate_tool["inputSchema"].to_string();
    assert_eq!(
        mutate_tool["inputSchema"]["required"],
        json!(["action", "input", "credential"])
    );
    assert!(mutate_tool["inputSchema"]["oneOf"].is_null());
    assert!(mutate_tool["inputSchema"]["properties"]["action"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "coordination"));
    assert_eq!(
        mutate_tool["inputSchema"]["properties"]["input"]["type"],
        "object"
    );
    assert!(mutate_schema.contains("schema/tool/prism_mutate/action/{action}"));

    client.send(list_resources_request(3)).await.unwrap();
    let resources = response_json(client.receive().await.unwrap());
    assert_eq!(resources["result"]["resources"][0]["uri"], INSTRUCTIONS_URI);
    assert_eq!(
        resources["result"]["resources"][0]["name"],
        "PRISM Instruction Sets"
    );
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://instructions/execution"));
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://instructions/planning"));
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == API_REFERENCE_URI));
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == CAPABILITIES_URI));
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == PROTECTED_STATE_URI));
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://self-description"));

    client
        .send(read_resource_request(4, INSTRUCTIONS_URI))
        .await
        .unwrap();
    let instructions = response_json(client.receive().await.unwrap());
    let instructions_text = instructions["result"]["contents"][0]["text"]
        .as_str()
        .expect("instructions resource should be text");
    assert_eq!(
        instructions_text,
        initialize["result"]["instructions"]
            .as_str()
            .expect("initialize instructions should be text")
    );
    assert!(instructions_text.contains("`prism://instructions/review`"));

    client
        .send(read_resource_request(41, "prism://instructions/execution"))
        .await
        .unwrap();
    let execution_instructions = response_json(client.receive().await.unwrap());
    let execution_text = execution_instructions["result"]["contents"][0]["text"]
        .as_str()
        .expect("execution instructions should be text");
    assert!(execution_text.contains("# PRISM Execution Instructions"));
    assert!(execution_text.contains("## Familiarization"));
    assert!(execution_text.contains("## Mutations"));

    client
        .send(read_resource_request(5, API_REFERENCE_URI))
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
    assert!(!api_reference.contains("mcpLog(options?: McpLogOptions): McpCallLogEntryView[];"));
    assert!(!api_reference.contains("queryLog(options?: QueryLogOptions): QueryLogEntryView[];"));

    client
        .send(read_resource_request(6, CAPABILITIES_URI))
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
    assert!(capabilities_payload["queryViews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|view| view["name"] == "repoPlaybook" && view["enabled"] == true));
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
        .any(|resource| resource["uri"] == INSTRUCTIONS_URI));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://instructions/coordination"));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == SESSION_URI));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == PROTECTED_STATE_URI));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == VOCAB_URI));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://self-description"
            && resource["shapeUri"] == "prism://shape/resource/self-description-audit"));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_surface_request_logs_include_common_envelope_phases() {
    let server = server_with_node(demo_node());
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

    client.send(list_tools_request(2)).await.unwrap();
    let _ = response_json(client.receive().await.unwrap());

    client
        .send(read_resource_request(3, API_REFERENCE_URI))
        .await
        .unwrap();
    let _ = response_json(client.receive().await.unwrap());

    client.send(ping_request(4)).await.unwrap();
    let _ = response_json(client.receive().await.unwrap());

    wait_until("common request traces to record envelope phases", || {
        let records = server_handle.host.mcp_call_log_store.records();
        let tool_list = records
            .iter()
            .find(|record| record.entry.call_type == "tool_list");
        let resource_read = records.iter().find(|record| {
            record.entry.call_type == "resource_read" && record.entry.name == API_REFERENCE_URI
        });
        let ping = records
            .iter()
            .find(|record| record.entry.call_type == "request" && record.entry.name == "ping");
        let Some(tool_list) = tool_list else {
            return false;
        };
        let Some(resource_read) = resource_read else {
            return false;
        };
        let Some(ping) = ping else {
            return false;
        };
        let all_ready = [tool_list, resource_read, ping].into_iter().all(|record| {
            let operations = record
                .phases
                .iter()
                .map(|phase| phase.operation.as_str())
                .collect::<Vec<_>>();
            operations.contains(&"mcp.receiveRequest")
                && operations.contains(&"mcp.routeRequest")
                && operations.contains(&"mcp.executeHandler")
                && operations.contains(&"mcp.encodeResponse")
        });
        all_ready
    });

    let records = server_handle.host.mcp_call_log_store.records();
    let tool_list = records
        .iter()
        .find(|record| record.entry.call_type == "tool_list")
        .expect("tool_list record should exist");
    let resource_read = records
        .iter()
        .find(|record| {
            record.entry.call_type == "resource_read" && record.entry.name == API_REFERENCE_URI
        })
        .expect("resource_read record should exist");

    for record in [tool_list, resource_read] {
        let operations = record
            .phases
            .iter()
            .map(|phase| phase.operation.as_str())
            .collect::<Vec<_>>();
        assert!(operations.contains(&"mcp.receiveRequest"));
        assert!(operations.contains(&"mcp.routeRequest"));
        assert!(operations.contains(&"mcp.executeHandler"));
        assert!(operations.contains(&"mcp.encodeResponse"));
    }

    let ping = records
        .iter()
        .find(|record| record.entry.call_type == "request" && record.entry.name == "ping")
        .expect("ping fallback request record should exist");
    let ping_operations = ping
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(ping_operations.contains(&"mcp.receiveRequest"));
    assert!(ping_operations.contains(&"mcp.routeRequest"));
    assert!(ping_operations.contains(&"mcp.executeHandler"));
    assert!(ping_operations.contains(&"mcp.encodeResponse"));

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
    assert!(resource_uris.contains(&INSTRUCTIONS_URI));
    assert!(resource_uris.contains(&CAPABILITIES_URI));
    assert!(resource_uris.contains(&VOCAB_URI));
    assert!(resource_uris.contains(&TOOL_SCHEMAS_URI));

    client
        .send(read_resource_request(25, VOCAB_URI))
        .await
        .unwrap();
    let vocab = response_json(client.receive().await.unwrap());
    let vocab_payload = serde_json::from_str::<Value>(
        vocab["result"]["contents"][0]["text"]
            .as_str()
            .expect("vocab resource should be text"),
    )
    .unwrap();
    assert!(vocab_payload["vocabularies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["key"] == "coordinationTaskStatus"));

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
    assert!(schema_payload.to_string().contains("\"fileId\""));
    assert!(schema_payload.to_string().contains("\"path\""));

    client
        .send(read_resource_request(
            5,
            "prism://schema/tool/prism_mutate/action/coordination",
        ))
        .await
        .unwrap();
    let action_schema = response_json(client.receive().await.unwrap());
    let action_schema_text = action_schema["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool action schema should be text: {action_schema:#?}"));
    let action_schema_payload = serde_json::from_str::<Value>(action_schema_text).unwrap();
    assert_eq!(
        action_schema_payload["$id"],
        "prism://schema/tool/prism_mutate/action/coordination"
    );
    assert_eq!(
        action_schema_payload["title"],
        "PRISM Tool Action Schema: prism_mutate.coordination"
    );
    assert_eq!(
        action_schema_payload["properties"]["payload"]["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(13)
    );
    assert!(action_schema_payload["examples"]
        .as_array()
        .expect("action schema examples")
        .iter()
        .any(|example| example["input"]["kind"] == "task_create"));

    client
        .send(read_resource_request(
            51,
            "prism://schema/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ))
        .await
        .unwrap();
    let variant_schema = response_json(client.receive().await.unwrap());
    let variant_schema_payload = serde_json::from_str::<Value>(
        variant_schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool variant schema should be text"),
    )
    .unwrap();
    assert_eq!(
        variant_schema_payload["$id"],
        "prism://schema/tool/prism_mutate/action/coordination/variant/plan_bootstrap"
    );
    assert_eq!(
        variant_schema_payload["title"],
        "PRISM Tool Variant Schema: prism_mutate.coordination.plan_bootstrap"
    );
    assert!(variant_schema_payload["examples"].is_array());

    client
        .send(read_resource_request(
            6,
            "prism://schema/tool/prism_mutate/action/validation_feedback",
        ))
        .await
        .unwrap();
    let validation_feedback_schema = response_json(client.receive().await.unwrap());
    let validation_feedback_schema_text = validation_feedback_schema["result"]["contents"][0]
        ["text"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "validation feedback action schema should be text: {validation_feedback_schema:#?}"
            )
        });
    assert!(validation_feedback_schema_text.contains("\"fileId\""));
    assert!(validation_feedback_schema_text.contains("\"path\""));
    assert!(validation_feedback_schema_text.contains("Preferred over `fileId`"));

    client
        .send(read_resource_request(8, "prism://schema/file"))
        .await
        .unwrap();
    let file_schema = response_json(client.receive().await.unwrap());
    let file_schema_payload = serde_json::from_str::<Value>(
        file_schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("file schema should be text"),
    )
    .unwrap();
    assert_eq!(file_schema_payload["$id"], "prism://schema/file");
    assert_eq!(file_schema_payload["title"], "PRISM File Resource Schema");

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_reads_file_resource_templates_for_workspace_paths() {
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

    client.send(list_resources_request(2)).await.unwrap();
    let resources = response_json(client.receive().await.unwrap());
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == SESSION_URI));

    client
        .send(read_resource_request(3, SESSION_URI))
        .await
        .unwrap();
    let session_resource = response_json(client.receive().await.unwrap());
    let session_payload = serde_json::from_str::<Value>(
        session_resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("session resource should be text"),
    )
    .unwrap();
    assert_eq!(session_payload["uri"], SESSION_URI);

    client
        .send(read_resource_request(4, CAPABILITIES_URI))
        .await
        .unwrap();
    let capabilities_resource = response_json(client.receive().await.unwrap());
    let capabilities_payload = serde_json::from_str::<Value>(
        capabilities_resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities resource should be text"),
    )
    .unwrap();
    assert_eq!(capabilities_payload["uri"], CAPABILITIES_URI);

    client
        .send(read_resource_request(
            5,
            "prism://file/src%2Flib.rs?startLine=1&endLine=1&maxChars=200",
        ))
        .await
        .unwrap();
    let resource = response_json(client.receive().await.unwrap());
    let payload = serde_json::from_str::<Value>(
        resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("file resource should be text"),
    )
    .unwrap();
    assert_eq!(payload["path"], "src/lib.rs");
    assert_eq!(payload["excerpt"]["startLine"], 1);
    assert!(payload["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha"));
    assert!(payload["relatedResources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://schema/file"));

    wait_until(
        "resource read traces to include resource refresh phases",
        || {
            let records = server_handle.host.mcp_call_log_store.records();
            let session_read = records.iter().find(|record| {
                record.entry.call_type == "resource_read" && record.entry.name == SESSION_URI
            });
            let capabilities_read = records.iter().find(|record| {
                record.entry.call_type == "resource_read" && record.entry.name == CAPABILITIES_URI
            });
            let file_read = records.iter().find(|record| {
                record.entry.call_type == "resource_read"
                    && record
                        .metadata
                        .get("uri")
                        .and_then(Value::as_str)
                        .map(|uri| uri.starts_with("prism://file/"))
                        .unwrap_or(false)
            });
            let Some(session_read) = session_read else {
                return false;
            };
            let Some(capabilities_read) = capabilities_read else {
                return false;
            };
            let Some(file_read) = file_read else {
                return false;
            };
            let all_ready = [session_read, capabilities_read, file_read]
                .into_iter()
                .all(|record| {
                    let operations = record
                        .phases
                        .iter()
                        .map(|phase| phase.operation.as_str())
                        .collect::<Vec<_>>();
                    operations.contains(&"resource.refreshWorkspace")
                        && operations.contains(&"resource.handler")
                });
            all_ready
        },
    );

    let records = server_handle.host.mcp_call_log_store.records();
    let session_read = records
        .iter()
        .find(|record| {
            record.entry.call_type == "resource_read" && record.entry.name == SESSION_URI
        })
        .expect("session resource_read record should exist");
    let capabilities_read = records
        .iter()
        .find(|record| {
            record.entry.call_type == "resource_read" && record.entry.name == CAPABILITIES_URI
        })
        .expect("capabilities resource_read record should exist");
    let file_read = records
        .iter()
        .find(|record| {
            record.entry.call_type == "resource_read"
                && record
                    .metadata
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(|uri| uri.starts_with("prism://file/"))
                    .unwrap_or(false)
        })
        .expect("file resource_read record should exist");
    for record in [session_read, capabilities_read, file_read] {
        let operations = record
            .phases
            .iter()
            .map(|phase| phase.operation.as_str())
            .collect::<Vec<_>>();
        assert!(operations.contains(&"mcp.receiveRequest"));
        assert!(operations.contains(&"mcp.routeRequest"));
        assert!(operations.contains(&"resource.refreshWorkspace"));
        assert!(operations.contains(&"resource.handler"));
        let refresh_args = record
            .phases
            .iter()
            .find(|phase| phase.operation == "resource.refreshWorkspace")
            .and_then(|phase| phase.args_summary.as_ref())
            .and_then(Value::as_object)
            .expect("resource refresh args should exist");
        assert!(refresh_args.contains_key("refreshPath"));
        assert!(refresh_args.contains_key("metrics"));
        if operations.contains(&"runtimeSync.waitLock") {
            assert!(operations.contains(&"runtimeSync.waitLock"));
            assert!(operations.contains(&"runtimeSync.refreshFs"));
            assert!(operations.contains(&"runtimeSync.snapshotRevisions"));
        }
        assert!(
            operations
                .iter()
                .any(|operation| operation.starts_with("resource_read.prism://file"))
                || operations
                    .iter()
                    .any(|operation| operation.starts_with("resource_read.prism://session"))
                || operations
                    .iter()
                    .any(|operation| operation.starts_with("resource_read.prism://capabilities"))
        );
    }

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_reads_protected_state_resource_for_workspace_streams() {
    let root = temp_workspace();
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
        .send(read_resource_request(
            30,
            "prism://protected-state?stream=concepts%3Aevents",
        ))
        .await
        .unwrap();
    let protected_state_resource = response_json(client.receive().await.unwrap());
    let protected_state_payload = serde_json::from_str::<Value>(
        protected_state_resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("protected-state resource should be text"),
    )
    .unwrap();
    assert_eq!(
        protected_state_payload["uri"],
        "prism://protected-state?stream=concepts%3Aevents"
    );
    assert_eq!(protected_state_payload["streamSelector"], "concepts:events");
    assert_eq!(protected_state_payload["allVerified"], true);
    assert_eq!(protected_state_payload["nonVerifiedStreamCount"], 0);
    assert_eq!(
        protected_state_payload["streams"][0]["streamId"],
        "concepts:events"
    );
    assert_eq!(
        protected_state_payload["streams"][0]["verificationStatus"],
        "Verified"
    );
    assert_eq!(
        protected_state_payload["streams"][0]["protectedPath"],
        ".prism/concepts/events.jsonl"
    );
    assert_eq!(
        protected_state_payload["streams"][0]["diagnosticCode"],
        Value::Null
    );
    assert!(protected_state_payload["relatedResources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == "prism://schema/protected-state"));

    client
        .send(read_resource_request(31, "prism://schema/protected-state"))
        .await
        .unwrap();
    let protected_state_schema = response_json(client.receive().await.unwrap());
    let protected_state_schema_payload = serde_json::from_str::<Value>(
        protected_state_schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("protected-state schema should be text"),
    )
    .unwrap();
    assert_eq!(
        protected_state_schema_payload["$id"],
        "prism://schema/protected-state"
    );
    assert_eq!(
        protected_state_schema_payload["title"],
        "PRISM Protected State Resource Schema"
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_lists_and_reads_plan_detail_resources() {
    let server = server_with_node(demo_node());
    let plan = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "title": "Migrate persistence storage semantics", "goal": "Migrate persistence storage semantics" }),
                task_id: None,
            },
        )
        .expect("plan create should succeed");
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({ "planId": plan_id, "title": "Classify authoritative tables" }),
                task_id: None,
            },
        )
        .expect("task create should succeed");

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
        .send(client_message(
            r#"{ "jsonrpc": "2.0", "id": 2, "method": "resources/templates/list" }"#,
        ))
        .await
        .unwrap();
    let templates = response_json(client.receive().await.unwrap());
    assert!(templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|template| template["uriTemplate"] == "prism://plan/{planId}"));

    client
        .send(read_resource_request(3, &plan_resource_uri(&plan_id)))
        .await
        .unwrap();
    let resource = response_json(client.receive().await.unwrap());
    let payload = serde_json::from_str::<Value>(
        resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("plan resource should be text"),
    )
    .unwrap();
    assert_eq!(payload["plan"]["id"], plan_id);
    assert_eq!(payload["summary"]["actionableNodes"], 1);
    assert!(payload["relatedResources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|link| link["uri"] == "prism://plans"));

    client
        .send(read_resource_request(4, "prism://schema/plan"))
        .await
        .unwrap();
    let schema = response_json(client.receive().await.unwrap());
    let schema_payload = serde_json::from_str::<Value>(
        schema["result"]["contents"][0]["text"]
            .as_str()
            .expect("plan schema should be text"),
    )
    .unwrap();
    assert_eq!(schema_payload["$id"], "prism://schema/plan");
    assert_eq!(schema_payload["title"], "PRISM Plan Resource Schema");
    assert_eq!(schema_payload["examples"][0]["plan"]["id"], "plan:1");

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
    assert!(api_reference.contains("mcpLog(options?: McpLogOptions): McpCallLogEntryView[];"));
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
        .any(|method| method["name"] == "mcpLog"));
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
    assert!(search_schema_payload["examples"][0]["results"].is_array());

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
        .any(|resource| resource["name"] == "PRISM Instruction Sets"
            && resource["exampleUri"] == "prism://instructions"));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |resource| resource["name"] == "PRISM Instructions: Execution"
                && resource["exampleUri"] == "prism://instructions/execution"
        ));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["name"] == "PRISM Session"
            && resource["exampleUri"] == "prism://session"
            && resource["shapeUri"] == "prism://shape/resource/session"));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["name"] == "PRISM Protected State"
            && resource["exampleUri"] == "prism://protected-state?stream=concepts%3Aevents"));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["name"] == "PRISM Vocabulary"
            && resource["exampleUri"] == "prism://vocab"));
    assert!(capabilities_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_locate"
            && tool["exampleInput"]["query"] == "session"
            && tool["exampleUri"] == "prism://example/tool/prism_locate"
            && tool["shapeUri"] == "prism://shape/tool/prism_locate"));
    assert!(capabilities_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_query"
            && tool["exampleInput"]["language"] == "ts"
            && tool["exampleUri"] == "prism://example/tool/prism_query"
            && tool["shapeUri"] == "prism://shape/tool/prism_query"));
    assert!(capabilities_payload["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|template| template["name"] == "PRISM Plan"
            && template["exampleUri"] == "prism://plan/plan%3A1"
            && template["shapeUri"] == "prism://shape/resource/plan"));
    assert!(capabilities_payload["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|template| template["name"] == "PRISM Tool Variant Schema"
            && template["exampleUri"]
                == "prism://schema/tool/prism_mutate/action/coordination/variant/plan_bootstrap"));

    let plan_entry = catalog_payload["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resourceKind"] == "plan")
        .expect("plan schema entry should exist");
    assert_eq!(plan_entry["exampleUri"], "prism://plan/plan%3A1");
    assert_eq!(plan_entry["shapeUri"], "prism://shape/resource/plan");
    let protected_state_entry = catalog_payload["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resourceKind"] == "protected-state")
        .expect("protected-state schema entry should exist");
    assert_eq!(
        protected_state_entry["exampleUri"],
        "prism://protected-state?stream=concepts%3Aevents"
    );
    let tool_shape_entry = catalog_payload["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["resourceKind"] == "tool-shape")
        .expect("tool-shape schema entry should exist");
    assert_eq!(
        tool_shape_entry["resourceUri"],
        "prism://shape/tool/{toolName}"
    );
    assert_eq!(
        tool_shape_entry["exampleUri"],
        "prism://shape/tool/prism_mutate"
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn self_description_surface_exposes_companions_templates_and_audit() {
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
        .send(list_resource_templates_request(70))
        .await
        .unwrap();
    let templates = response_json(client.receive().await.unwrap());
    let template_uris = templates["result"]["resourceTemplates"]
        .as_array()
        .expect("resource templates should be an array")
        .iter()
        .filter_map(|template| template["uriTemplate"].as_str())
        .collect::<Vec<_>>();
    assert!(template_uris.contains(&"prism://schema/tool/{toolName}/action/{action}/variant/{tag}"));
    assert!(
        template_uris.contains(&"prism://example/tool/{toolName}/action/{action}/variant/{tag}")
    );
    assert!(template_uris.contains(&"prism://shape/tool/{toolName}/action/{action}/variant/{tag}"));
    assert!(template_uris.contains(&"prism://example/resource/{resourceKind}"));
    assert!(template_uris.contains(&"prism://shape/resource/{resourceKind}"));
    assert!(template_uris.contains(&"prism://capabilities/{section}"));
    assert!(template_uris.contains(&"prism://vocab/{key}"));
    assert!(template_uris.contains(&"prism://recipe/tool/{toolName}/action/{action}/variant/{tag}"));

    client
        .send(read_resource_request(71, "prism://shape/tool/prism_mutate"))
        .await
        .unwrap();
    let tool_shape = response_json(client.receive().await.unwrap());
    let tool_shape_payload = serde_json::from_str::<Value>(
        tool_shape["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool shape should be text"),
    )
    .unwrap();
    assert_eq!(tool_shape_payload["toolName"], "prism_mutate");
    assert_eq!(
        tool_shape_payload["exampleUri"],
        "prism://example/tool/prism_mutate"
    );

    client
        .send(read_resource_request(
            72,
            "prism://example/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ))
        .await
        .unwrap();
    let tool_example = response_json(client.receive().await.unwrap());
    let tool_example_payload = serde_json::from_str::<Value>(
        tool_example["result"]["contents"][0]["text"]
            .as_str()
            .expect("tool example should be text"),
    )
    .unwrap();
    assert_eq!(tool_example_payload["variant"], "plan_bootstrap");
    assert_eq!(
        tool_example_payload["targetSchemaUri"],
        "prism://schema/tool/prism_mutate/action/coordination/variant/plan_bootstrap"
    );
    assert_eq!(
        tool_example_payload["shapeUri"],
        "prism://shape/tool/prism_mutate/action/coordination/variant/plan_bootstrap"
    );

    client
        .send(read_resource_request(73, "prism://shape/resource/search"))
        .await
        .unwrap();
    let resource_shape = response_json(client.receive().await.unwrap());
    let resource_shape_payload = serde_json::from_str::<Value>(
        resource_shape["result"]["contents"][0]["text"]
            .as_str()
            .expect("resource shape should be text"),
    )
    .unwrap();
    assert_eq!(resource_shape_payload["resourceKind"], "search");
    assert_eq!(
        resource_shape_payload["exampleUri"],
        "prism://example/resource/search"
    );

    client
        .send(read_resource_request(74, "prism://capabilities/tools"))
        .await
        .unwrap();
    let capabilities_section = response_json(client.receive().await.unwrap());
    let capabilities_section_payload = serde_json::from_str::<Value>(
        capabilities_section["result"]["contents"][0]["text"]
            .as_str()
            .expect("capabilities section should be text"),
    )
    .unwrap();
    assert_eq!(capabilities_section_payload["section"], "tools");
    assert!(capabilities_section_payload["value"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_mutate"));

    client
        .send(read_resource_request(
            75,
            "prism://vocab/coordinationMutationKind",
        ))
        .await
        .unwrap();
    let vocab_entry = response_json(client.receive().await.unwrap());
    let vocab_entry_payload = serde_json::from_str::<Value>(
        vocab_entry["result"]["contents"][0]["text"]
            .as_str()
            .expect("vocab entry should be text"),
    )
    .unwrap();
    assert_eq!(vocab_entry_payload["key"], "coordinationMutationKind");

    client
        .send(read_resource_request(
            76,
            "prism://recipe/tool/prism_mutate/action/coordination/variant/plan_bootstrap",
        ))
        .await
        .unwrap();
    let recipe = response_json(client.receive().await.unwrap());
    let recipe_text = recipe["result"]["contents"][0]["text"]
        .as_str()
        .expect("recipe should be text");
    assert!(recipe_text.contains("plan_bootstrap"));
    assert!(recipe_text.contains("Read the shape resource"));

    client
        .send(read_resource_request(77, "prism://self-description"))
        .await
        .unwrap();
    let audit = response_json(client.receive().await.unwrap());
    let audit_payload = serde_json::from_str::<Value>(&resource_text(audit)).unwrap();
    let invalid_entries = audit_payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue == "example_invalid")
        })
        .map(|entry| entry["name"].as_str().unwrap_or("<unknown>").to_string())
        .collect::<Vec<_>>();
    let non_operable_entries = audit_payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["sourceFreeOperable"] == Value::Bool(false))
        .map(|entry| entry["name"].as_str().unwrap_or("<unknown>").to_string())
        .collect::<Vec<_>>();
    assert_eq!(audit_payload["missingCompanionEntries"], 0);
    assert_eq!(audit_payload["missingRecipeEntries"], 0);
    assert_eq!(
        audit_payload["invalidExampleEntries"], 0,
        "{invalid_entries:?}"
    );
    assert_eq!(
        audit_payload["nonOperableEntries"], 0,
        "{non_operable_entries:?}"
    );
    let bootstrap_entry = audit_payload["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "prism_mutate.coordination.plan_bootstrap")
        .expect("plan_bootstrap audit entry should exist");
    assert!(!bootstrap_entry["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue == "missing_example"
            || issue == "missing_shape"
            || issue == "example_oversize"
            || issue == "shape_oversize"));
    assert!(bootstrap_entry["exampleBytes"].as_u64().unwrap() < 12_288);
    assert!(bootstrap_entry["shapeBytes"].as_u64().unwrap() < 12_288);
    assert_eq!(bootstrap_entry["exampleValid"], Value::Bool(true));
    assert_eq!(bootstrap_entry["sourceFreeOperable"], Value::Bool(true));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn instruction_sets_advertise_compact_self_description_ladder() {
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

    for (id, uri) in [
        (80_u64, "prism://instructions/planning"),
        (81, "prism://instructions/execution"),
        (82, "prism://instructions/coordination"),
    ] {
        client.send(read_resource_request(id, uri)).await.unwrap();
        let text = resource_text(response_json(client.receive().await.unwrap()));
        assert!(
            text.contains("prism://shape/tool/{toolName}"),
            "{uri}: {text}"
        );
        assert!(
            text.contains("prism://example/tool/{toolName}"),
            "{uri}: {text}"
        );
        assert!(
            text.contains("prism://recipe/tool/{toolName}/action/{action}"),
            "{uri}: {text}"
        );
        assert!(
            text.contains("prism://capabilities/{section}"),
            "{uri}: {text}"
        );
    }

    running.cancel().await.unwrap();
}

#[test]
fn self_description_audit_keeps_compact_discovery_surfaces_under_budget() {
    let server = server_with_node(demo_node());
    let audit_payload = serde_json::from_str::<Value>(&text_resource_contents(
        crate::self_description_audit_resource_contents(
            server
                .host
                .capabilities_resource_value()
                .expect("capabilities resource should build"),
            "prism://self-description",
        )
        .expect("self-description audit resource should build"),
    ))
    .unwrap();
    let budget = audit_payload["budgetBytes"]
        .as_u64()
        .expect("budget bytes should exist") as usize;

    let entries = audit_payload["entries"]
        .as_array()
        .expect("audit entries should be an array");
    assert!(entries
        .iter()
        .any(|entry| entry["surfaceKind"] == "resource_template"));

    for entry in entries {
        for key in ["exampleUri", "shapeUri", "recipeUri"] {
            let Some(uri) = entry[key].as_str() else {
                continue;
            };
            let payload = compact_self_description_resource_text(&server, uri);
            assert!(
                payload.len() <= budget,
                "resource `{uri}` exceeded compact budget of {budget} bytes with {} bytes",
                payload.len()
            );
        }
    }
}
