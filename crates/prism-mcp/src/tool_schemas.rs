use prism_js::{ToolActionSchemaView, ToolCatalogEntryView, ToolFieldSchemaView, ToolSchemaView};
use rmcp::schemars::JsonSchema;
use serde_json::Value;

use crate::{
    capabilities_resource_view_link, dedupe_resource_link_views, resource_meta,
    schema_resource_contents, schema_resource_uri, schema_resource_value,
    schema_resource_view_link, session_resource_view_link, tool_input_example,
    tool_schema_resource_uri, tool_schema_resource_view_link, tool_schemas_resource_view_link,
    PrismMutationArgs, PrismQueryArgs, PrismSessionArgs, ResourceLinkView, TOOL_SCHEMAS_URI,
};
use rmcp::{model::ResourceContents, ErrorData as McpError};

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolSchemaCatalogEntry {
    pub(crate) tool_name: String,
    pub(crate) schema_uri: String,
    pub(crate) description: String,
    pub(crate) example_input: Value,
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
            example_input: tool_input_example("prism_query").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_session".to_string(),
            schema_uri: tool_schema_resource_uri("prism_session"),
            description: "Input schema for PRISM session and task-context mutations.".to_string(),
            example_input: tool_input_example("prism_session").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_mutate".to_string(),
            schema_uri: tool_schema_resource_uri("prism_mutate"),
            description: "Input schema for coarse PRISM state mutations and tagged action unions."
                .to_string(),
            example_input: tool_input_example("prism_mutate").expect("tool example"),
        },
    ]
}

pub(crate) fn tool_schemas_resource_value() -> ToolSchemaCatalogPayload {
    let mut related_resources = vec![
        capabilities_resource_view_link(),
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

pub(crate) fn tool_catalog_views() -> Vec<ToolCatalogEntryView> {
    tool_schema_catalog_entries()
        .into_iter()
        .map(|entry| ToolCatalogEntryView {
            tool_name: entry.tool_name,
            schema_uri: entry.schema_uri,
            description: entry.description,
            example_input: entry.example_input,
        })
        .collect()
}

pub(crate) fn tool_schema_view(tool_name: &str) -> Option<ToolSchemaView> {
    let entry = tool_schema_catalog_entries()
        .into_iter()
        .find(|entry| entry.tool_name == tool_name)?;
    let input_schema = tool_input_schema_value(tool_name)?;
    Some(ToolSchemaView {
        tool_name: entry.tool_name,
        schema_uri: entry.schema_uri,
        description: entry.description,
        example_input: entry.example_input.clone(),
        actions: tool_action_views(&input_schema, &entry.example_input),
        input_schema,
    })
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

pub(crate) fn tool_input_schema_value(tool_name: &str) -> Option<Value> {
    match tool_name {
        "prism_query" => Some(tool_input_schema_value_for::<PrismQueryArgs>(
            "prism_query",
            "JSON Schema for the `prism_query` tool input payload.",
        )),
        "prism_session" => Some(tool_input_schema_value_for::<PrismSessionArgs>(
            "prism_session",
            "JSON Schema for the `prism_session` tool input payload.",
        )),
        "prism_mutate" => Some(tool_input_schema_value_for::<PrismMutationArgs>(
            "prism_mutate",
            "JSON Schema for the `prism_mutate` tool input payload.",
        )),
        _ => None,
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

fn tool_input_schema_value_for<T: JsonSchema + std::any::Any>(
    tool_name: &str,
    description: &str,
) -> Value {
    schema_resource_value::<T>(
        &tool_schema_resource_uri(tool_name),
        &format!("PRISM Tool Input Schema: {tool_name}"),
        description,
        &format!("tool:{tool_name}"),
    )
}

fn tool_action_views(schema: &Value, example_input: &Value) -> Vec<ToolActionSchemaView> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|variants| variants.iter())
        .filter_map(|variant| tool_action_view(schema, variant, example_input))
        .collect()
}

fn tool_action_view(
    root_schema: &Value,
    variant_schema: &Value,
    example_input: &Value,
) -> Option<ToolActionSchemaView> {
    let properties = variant_schema.get("properties")?.as_object()?;
    let action = properties
        .get("action")?
        .get("const")?
        .as_str()?
        .to_string();
    let input_schema = resolve_schema_ref(root_schema, properties.get("input")?).clone();
    let required_fields = input_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let fields = input_schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|fields| fields.iter())
        .map(|(name, field_schema)| {
            tool_field_view(root_schema, name, field_schema, &required_fields)
        })
        .collect::<Vec<_>>();
    let example = example_input
        .get("action")
        .and_then(Value::as_str)
        .filter(|candidate| *candidate == action)
        .map(|_| example_input.clone());
    Some(ToolActionSchemaView {
        action,
        required_fields,
        fields,
        input_schema,
        example_input: example,
    })
}

fn tool_field_view(
    root_schema: &Value,
    name: &str,
    field_schema: &Value,
    required_fields: &[String],
) -> ToolFieldSchemaView {
    let resolved = resolve_schema_ref(root_schema, field_schema);
    ToolFieldSchemaView {
        name: name.to_string(),
        required: required_fields.iter().any(|field| field == name),
        description: resolved
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        types: schema_type_labels(root_schema, resolved),
        enum_values: schema_enum_values(root_schema, resolved),
        schema: resolved.clone(),
    }
}

fn resolve_schema_ref<'a>(root_schema: &'a Value, schema: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let Some(pointer) = reference.strip_prefix('#') else {
        return schema;
    };
    root_schema.pointer(pointer).unwrap_or(schema)
}

fn schema_type_labels(root_schema: &Value, schema: &Value) -> Vec<String> {
    if let Some(type_name) = schema.get("type").and_then(Value::as_str) {
        return vec![type_name.to_string()];
    }
    if let Some(types) = schema.get("type").and_then(Value::as_array) {
        return types
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(items) = schema.get(key).and_then(Value::as_array) {
            let mut labels = Vec::new();
            for item in items {
                let resolved = resolve_schema_ref(root_schema, item);
                for label in schema_type_labels(root_schema, resolved) {
                    if !labels.contains(&label) {
                        labels.push(label);
                    }
                }
            }
            if !labels.is_empty() {
                return labels;
            }
        }
    }
    Vec::new()
}

fn schema_enum_values(root_schema: &Value, schema: &Value) -> Vec<String> {
    if let Some(items) = schema.get("enum").and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(items) = schema.get(key).and_then(Value::as_array) {
            let mut values = Vec::new();
            for item in items {
                let resolved = resolve_schema_ref(root_schema, item);
                for value in schema_enum_values(root_schema, resolved) {
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
            }
            if !values.is_empty() {
                return values;
            }
        }
    }
    Vec::new()
}
