use rmcp::{
    model::ProtocolVersion,
    transport::{IntoTransport, Transport},
};
use serde_json::Value;

use super::*;
use crate::tests_support::{
    demo_node, initialize_client, initialized_notification, list_resources_request,
    list_tools_request, read_resource_request, response_json, server_with_node,
    server_with_node_and_features,
};

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
    let mutate_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "prism_mutate")
        .expect("prism_mutate tool should exist");
    let mutate_schema = mutate_tool["inputSchema"].to_string();
    assert!(mutate_schema.contains("\"plan_node_create\""));
    assert!(mutate_schema.contains("\"claimId\""));
    assert!(mutate_schema.contains("\"artifactId\""));
    assert!(mutate_schema.contains("\"content\""));

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
    assert!(!api_reference.contains("mcpLog(options?: McpLogOptions): McpCallLogEntryView[];"));
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
        .any(|resource| resource["uri"] == SESSION_URI));
    assert!(capabilities_payload["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|resource| resource["uri"] == VOCAB_URI));

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

    client
        .send(read_resource_request(
            6,
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
        Some(10)
    );
    assert!(action_schema_payload["examples"]
        .as_array()
        .expect("action schema examples")
        .iter()
        .any(|example| example["input"]["kind"] == "task_create"));

    client
        .send(read_resource_request(
            7,
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
        .any(|tool| tool["name"] == "prism_locate" && tool["exampleInput"]["query"] == "session"));
    assert!(capabilities_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "prism_query" && tool["exampleInput"]["language"] == "ts"));

    running.cancel().await.unwrap();
}
