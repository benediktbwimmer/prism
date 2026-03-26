use rmcp::schemars::JsonSchema;

use crate::{
    dedupe_resource_link_views, resource_meta, schema_resource_contents, schema_resource_uri,
    schema_resource_view_link, session_resource_view_link, tool_schema_resource_uri,
    tool_schema_resource_view_link, tool_schemas_resource_view_link, PrismMutationArgs,
    PrismQueryArgs, PrismSessionArgs, ResourceLinkView, TOOL_SCHEMAS_URI,
};
use rmcp::{model::ResourceContents, ErrorData as McpError};

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolSchemaCatalogEntry {
    pub(crate) tool_name: String,
    pub(crate) schema_uri: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolSchemaCatalogPayload {
    pub(crate) uri: String,
    pub(crate) schema_uri: String,
    pub(crate) tools: Vec<ToolSchemaCatalogEntry>,
    pub(crate) related_resources: Vec<ResourceLinkView>,
}

pub(crate) fn tool_schema_catalog_entries() -> Vec<ToolSchemaCatalogEntry> {
    vec![
        ToolSchemaCatalogEntry {
            tool_name: "prism_query".to_string(),
            schema_uri: tool_schema_resource_uri("prism_query"),
            description: "Input schema for programmable read-only TypeScript PRISM queries."
                .to_string(),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_session".to_string(),
            schema_uri: tool_schema_resource_uri("prism_session"),
            description: "Input schema for PRISM session and task-context mutations.".to_string(),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_mutate".to_string(),
            schema_uri: tool_schema_resource_uri("prism_mutate"),
            description: "Input schema for coarse PRISM state mutations and tagged action unions."
                .to_string(),
        },
    ]
}

pub(crate) fn tool_schemas_resource_value() -> ToolSchemaCatalogPayload {
    let mut related_resources = vec![
        tool_schemas_resource_view_link(),
        schema_resource_view_link("tool-schemas"),
        session_resource_view_link(),
    ];
    related_resources.extend(
        tool_schema_catalog_entries()
            .iter()
            .map(|entry| tool_schema_resource_view_link(&entry.tool_name)),
    );
    ToolSchemaCatalogPayload {
        uri: TOOL_SCHEMAS_URI.to_string(),
        schema_uri: schema_resource_uri("tool-schemas"),
        tools: tool_schema_catalog_entries(),
        related_resources: dedupe_resource_link_views(related_resources),
    }
}

pub(crate) fn tool_schema_resource_contents(
    tool_name: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    match tool_name {
        "prism_query" => tool_input_schema_contents::<PrismQueryArgs>(
            uri,
            "prism_query",
            "JSON Schema for the `prism_query` tool input payload.",
        ),
        "prism_session" => tool_input_schema_contents::<PrismSessionArgs>(
            uri,
            "prism_session",
            "JSON Schema for the `prism_session` tool input payload.",
        ),
        "prism_mutate" => tool_input_schema_contents::<PrismMutationArgs>(
            uri,
            "prism_mutate",
            "JSON Schema for the `prism_mutate` tool input payload.",
        ),
        _ => Err(McpError::resource_not_found(
            "resource_not_found",
            Some(serde_json::json!({ "uri": uri })),
        )),
    }
}

fn tool_input_schema_contents<T: JsonSchema + std::any::Any>(
    uri: &str,
    tool_name: &str,
    description: &str,
) -> Result<ResourceContents, McpError> {
    schema_resource_contents::<T>(
        uri,
        &format!("PRISM Tool Input Schema: {tool_name}"),
        description,
        &format!("tool:{tool_name}"),
    )
    .map(|contents| contents.with_meta(resource_meta("tool-schema", None, Some(tool_name))))
}
