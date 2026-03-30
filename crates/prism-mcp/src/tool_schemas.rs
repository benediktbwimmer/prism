use prism_js::{
    ToolActionSchemaView, ToolCatalogEntryView, ToolFieldSchemaView, ToolPayloadVariantSchemaView,
    ToolSchemaView,
};
use rmcp::schemars::JsonSchema;
use serde_json::{json, Map, Value};

use crate::{
    capabilities_resource_view_link, dedupe_resource_link_views, json_resource_contents_with_meta,
    resource_meta, schema_resource_contents, schema_resource_uri, schema_resource_value,
    schema_resource_view_link, session_resource_view_link, tool_action_example,
    tool_action_examples, tool_action_schema_resource_uri, tool_action_schema_resource_view_link,
    tool_input_example, tool_input_examples, tool_schema_resource_uri,
    tool_schema_resource_view_link, tool_schemas_resource_view_link, vocab_resource_view_link,
    ArtifactProposePayload, ArtifactReviewPayload, ArtifactSupersedePayload, ClaimAcquirePayload,
    ClaimReleasePayload, ClaimRenewPayload, HandoffAcceptPayload, HandoffPayload,
    MemoryRetirePayload, MemoryStorePayload, PlanCreatePayload, PlanEdgeCreatePayload,
    PlanEdgeDeletePayload, PlanNodeCreatePayload, PlanUpdatePayload, PrismConceptArgs,
    PrismExpandArgs, PrismGatherArgs, PrismLocateArgs, PrismMutationArgs, PrismOpenArgs,
    PrismQueryArgs, PrismSessionArgs, PrismTaskBriefArgs, PrismWorksetArgs, ResourceLinkView,
    TaskCreatePayload, WorkflowUpdatePayload, TOOL_SCHEMAS_URI,
};
use rmcp::{model::ResourceContents, ErrorData as McpError};

const MAX_SCHEMA_SUMMARY_DEPTH: usize = 3;

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
            tool_name: "prism_locate".to_string(),
            schema_uri: tool_schema_resource_uri("prism_locate"),
            description: "Input schema for the compact first-hop target locator.".to_string(),
            example_input: tool_input_example("prism_locate").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_gather".to_string(),
            schema_uri: tool_schema_resource_uri("prism_gather"),
            description: "Input schema for gathering 1 to 3 bounded exact-text slices.".to_string(),
            example_input: tool_input_example("prism_gather").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_open".to_string(),
            schema_uri: tool_schema_resource_uri("prism_open"),
            description:
                "Input schema for opening one compact handle or exact workspace path as a bounded code slice."
                    .to_string(),
            example_input: tool_input_example("prism_open").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_workset".to_string(),
            schema_uri: tool_schema_resource_uri("prism_workset"),
            description: "Input schema for building a compact implementation workset.".to_string(),
            example_input: tool_input_example("prism_workset").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_expand".to_string(),
            schema_uri: tool_schema_resource_uri("prism_expand"),
            description: "Input schema for explicit depth-on-demand handle expansion.".to_string(),
            example_input: tool_input_example("prism_expand").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_task_brief".to_string(),
            schema_uri: tool_schema_resource_uri("prism_task_brief"),
            description: "Input schema for the compact coordination task brief tool.".to_string(),
            example_input: tool_input_example("prism_task_brief").expect("tool example"),
        },
        ToolSchemaCatalogEntry {
            tool_name: "prism_concept".to_string(),
            schema_uri: tool_schema_resource_uri("prism_concept"),
            description:
                "Input schema for resolving a broad repo concept into a compact concept packet."
                    .to_string(),
            example_input: tool_input_example("prism_concept").expect("tool example"),
        },
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
        vocab_resource_view_link(),
        schema_resource_view_link("tool-schemas"),
        session_resource_view_link(),
    ];
    related_resources.extend(
        tool_schema_catalog_entries()
            .iter()
            .map(|entry| tool_schema_resource_view_link(&entry.tool_name)),
    );
    related_resources.extend(tool_schema_catalog_entries().iter().flat_map(|entry| {
        tool_schema_view(&entry.tool_name)
            .into_iter()
            .flat_map(|schema| schema.actions.into_iter())
            .map(move |action| {
                tool_action_schema_resource_view_link(&entry.tool_name, &action.action)
            })
    }));
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
        example_inputs: tool_input_examples(tool_name)
            .unwrap_or_else(|| vec![entry.example_input.clone()]),
        actions: tool_action_views(tool_name, &input_schema, &entry.example_input),
        input_schema,
    })
}

pub(crate) fn tool_action_schema_view(
    tool_name: &str,
    action: &str,
) -> Option<ToolActionSchemaView> {
    let root_schema = tool_input_schema_value(tool_name)?;
    let example_input = tool_input_example(tool_name)?;
    root_schema
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|variants| variants.iter())
        .filter_map(|variant| tool_action_view(tool_name, &root_schema, variant, &example_input))
        .find(|candidate| candidate.action == action)
}

pub(crate) fn tool_action_schema_value(tool_name: &str, action: &str) -> Option<Value> {
    let action_view = tool_action_schema_view(tool_name, action)?;
    let mut schema = action_view.input_schema.clone();
    let description = action_view
        .description
        .clone()
        .unwrap_or_else(|| format!("Exact input schema for `{tool_name}` action `{action}`."));
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "$schema".to_string(),
            Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
        );
        object.insert(
            "$id".to_string(),
            Value::String(tool_action_schema_resource_uri(tool_name, action)),
        );
        object.insert(
            "title".to_string(),
            Value::String(format!("PRISM Tool Action Schema: {tool_name}.{action}")),
        );
        object.insert("description".to_string(), Value::String(description));
        if !action_view.example_inputs.is_empty() {
            object.insert(
                "examples".to_string(),
                Value::Array(action_view.example_inputs.clone()),
            );
        } else if let Some(example_input) = &action_view.example_input {
            object.insert(
                "examples".to_string(),
                Value::Array(vec![example_input.clone()]),
            );
        }
    }
    Some(schema)
}

pub(crate) fn tool_action_schema_resource_contents(
    tool_name: &str,
    action: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let schema = tool_action_schema_value(tool_name, action).ok_or_else(|| {
        McpError::resource_not_found(
            "resource_not_found",
            Some(serde_json::json!({ "uri": uri })),
        )
    })?;
    json_resource_contents_with_meta(
        schema,
        uri.to_string(),
        Some(resource_meta(
            "tool-action-schema",
            Some(tool_action_schema_resource_uri(tool_name, action)),
            Some(tool_name),
        )),
    )
    .map(|contents| contents.with_mime_type("application/schema+json"))
}

pub(crate) fn tool_schema_resource_contents(
    tool_name: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    match tool_name {
        "prism_locate" => tool_input_schema_contents::<PrismLocateArgs>(
            uri,
            "prism_locate",
            "JSON Schema for the `prism_locate` tool input payload.",
        ),
        "prism_gather" => tool_input_schema_contents::<PrismGatherArgs>(
            uri,
            "prism_gather",
            "JSON Schema for the `prism_gather` tool input payload.",
        ),
        "prism_open" => tool_input_schema_contents::<PrismOpenArgs>(
            uri,
            "prism_open",
            "JSON Schema for the `prism_open` tool input payload.",
        ),
        "prism_workset" => tool_input_schema_contents::<PrismWorksetArgs>(
            uri,
            "prism_workset",
            "JSON Schema for the `prism_workset` tool input payload.",
        ),
        "prism_expand" => tool_input_schema_contents::<PrismExpandArgs>(
            uri,
            "prism_expand",
            "JSON Schema for the `prism_expand` tool input payload.",
        ),
        "prism_task_brief" => tool_input_schema_contents::<PrismTaskBriefArgs>(
            uri,
            "prism_task_brief",
            "JSON Schema for the `prism_task_brief` tool input payload.",
        ),
        "prism_concept" => tool_input_schema_contents::<PrismConceptArgs>(
            uri,
            "prism_concept",
            "JSON Schema for the `prism_concept` tool input payload.",
        ),
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
        "prism_locate" => Some(tool_input_schema_value_for::<PrismLocateArgs>(
            "prism_locate",
            "JSON Schema for the `prism_locate` tool input payload.",
        )),
        "prism_gather" => Some(tool_input_schema_value_for::<PrismGatherArgs>(
            "prism_gather",
            "JSON Schema for the `prism_gather` tool input payload.",
        )),
        "prism_open" => Some(tool_input_schema_value_for::<PrismOpenArgs>(
            "prism_open",
            "JSON Schema for the `prism_open` tool input payload.",
        )),
        "prism_workset" => Some(tool_input_schema_value_for::<PrismWorksetArgs>(
            "prism_workset",
            "JSON Schema for the `prism_workset` tool input payload.",
        )),
        "prism_expand" => Some(tool_input_schema_value_for::<PrismExpandArgs>(
            "prism_expand",
            "JSON Schema for the `prism_expand` tool input payload.",
        )),
        "prism_task_brief" => Some(tool_input_schema_value_for::<PrismTaskBriefArgs>(
            "prism_task_brief",
            "JSON Schema for the `prism_task_brief` tool input payload.",
        )),
        "prism_concept" => Some(tool_input_schema_value_for::<PrismConceptArgs>(
            "prism_concept",
            "JSON Schema for the `prism_concept` tool input payload.",
        )),
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

pub(crate) fn tool_transport_input_schema_value(tool_name: &str) -> Option<Value> {
    let schema = tool_input_schema_value(tool_name)?;
    Some(bind_transport_root_schema(tool_name, schema))
}

fn bind_transport_root_schema(tool_name: &str, schema: Value) -> Value {
    let Some(variants) = schema.get("oneOf").and_then(Value::as_array) else {
        return schema;
    };

    let actions = variants
        .iter()
        .filter_map(|variant| {
            variant
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get("action"))
                .and_then(Value::as_object)
                .and_then(|action| action.get("const"))
                .and_then(Value::as_str)
                .map(|action| Value::String(action.to_string()))
        })
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return schema;
    }

    let mut output = Map::new();
    for key in ["$id", "$schema", "title", "description", "examples"] {
        if let Some(value) = schema.get(key) {
            output.insert(key.to_string(), value.clone());
        }
    }
    output.insert("type".to_string(), Value::String("object".to_string()));
    output.insert(
        "required".to_string(),
        Value::Array(vec![
            Value::String("action".to_string()),
            Value::String("input".to_string()),
        ]),
    );
    output.insert("additionalProperties".to_string(), Value::Bool(false));
    output.insert(
        "properties".to_string(),
        Value::Object(Map::from_iter([
            (
                "action".to_string(),
                json!({
                    "type": "string",
                    "enum": actions,
                    "description": format!(
                        "Tagged action for `{tool_name}`. Inspect `prism://schema/tool/{tool_name}/action/{{action}}` or `prism.tool(\"{tool_name}\")` for the exact action-specific payload."
                    ),
                }),
            ),
            (
                "input".to_string(),
                json!({
                    "type": "object",
                    "description": format!(
                        "Action payload nested under `input`. The exact shape depends on `action`; use `prism://schema/tool/{tool_name}/action/{{action}}` for the exact schema."
                    ),
                }),
            ),
        ])),
    );
    output.insert("x-prismTaggedUnion".to_string(), Value::Bool(true));

    Value::Object(output)
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

fn tool_action_views(
    tool_name: &str,
    schema: &Value,
    example_input: &Value,
) -> Vec<ToolActionSchemaView> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|variants| variants.iter())
        .filter_map(|variant| tool_action_view(tool_name, schema, variant, example_input))
        .collect()
}

fn tool_action_view(
    tool_name: &str,
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
    let input_schema =
        enrich_action_input_schema(tool_name, &action, root_schema, properties.get("input")?);
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
            tool_field_view(&input_schema, name, field_schema, &required_fields)
        })
        .collect::<Vec<_>>();
    let example = tool_action_example(tool_name, &action).or_else(|| {
        example_input
            .get("action")
            .and_then(Value::as_str)
            .filter(|candidate| *candidate == action)
            .map(|_| example_input.clone())
    });
    let example_inputs = tool_action_examples(tool_name, &action);
    let payload_discriminator = action_payload_discriminator(tool_name, &action);
    let payload_variants = payload_variant_views(
        tool_name,
        &action,
        payload_discriminator,
        &example_inputs,
        &input_schema,
    );
    Some(ToolActionSchemaView {
        action: action.clone(),
        schema_uri: tool_action_schema_resource_uri(tool_name, &action),
        description: Some(action_description(tool_name, &action)),
        required_fields,
        fields,
        input_schema,
        example_input: example,
        example_inputs,
        payload_discriminator: payload_discriminator.map(ToString::to_string),
        payload_variants,
    })
}

fn action_description(tool_name: &str, action: &str) -> String {
    match action_payload_discriminator(tool_name, action) {
        Some(discriminator) => format!(
            "Exact input schema for `{tool_name}` action `{action}`. Match `input.payload` to `input.{discriminator}`."
        ),
        None => format!("Exact input schema for `{tool_name}` action `{action}`."),
    }
}

fn enrich_action_input_schema(
    tool_name: &str,
    action: &str,
    root_schema: &Value,
    input_schema: &Value,
) -> Value {
    let mut schema = expand_schema_refs(
        root_schema,
        resolve_schema_ref(root_schema, input_schema),
        0,
    );
    let Some(payload_schema) = action_payload_schema(tool_name, action) else {
        return schema;
    };
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert("payload".to_string(), payload_schema);
    }
    schema
}

fn action_payload_discriminator(tool_name: &str, action: &str) -> Option<&'static str> {
    match (tool_name, action) {
        ("prism_mutate", "memory") => Some("action"),
        ("prism_mutate", "coordination") => Some("kind"),
        ("prism_mutate", "claim") => Some("action"),
        ("prism_mutate", "artifact") => Some("action"),
        _ => None,
    }
}

fn action_payload_schema(tool_name: &str, action: &str) -> Option<Value> {
    match (tool_name, action) {
        ("prism_mutate", "memory") => Some(tagged_union_payload_schema(
            "Payload for `prism_mutate` action `memory`. Match this shape to `input.action`.",
            "action",
            vec![
                (
                    "store",
                    described_schema::<MemoryStorePayload>(
                        "Payload when `input.action` is `store`.",
                    ),
                ),
                (
                    "retire",
                    described_schema::<MemoryRetirePayload>(
                        "Payload when `input.action` is `retire`.",
                    ),
                ),
            ],
        )),
        ("prism_mutate", "coordination") => Some(tagged_union_payload_schema(
            "Payload for `prism_mutate` action `coordination`. Match this shape to `input.kind`.",
            "kind",
            vec![
                (
                    "plan_create",
                    described_schema::<PlanCreatePayload>(
                        "Payload when `input.kind` is `plan_create`.",
                    ),
                ),
                (
                    "plan_update",
                    described_schema::<PlanUpdatePayload>(
                        "Payload when `input.kind` is `plan_update`.",
                    ),
                ),
                (
                    "task_create",
                    described_schema::<TaskCreatePayload>(
                        "Payload when `input.kind` is `task_create`.",
                    ),
                ),
                (
                    "update",
                    described_schema::<WorkflowUpdatePayload>(
                        "Payload when `input.kind` is `update`.",
                    ),
                ),
                (
                    "plan_node_create",
                    described_schema::<PlanNodeCreatePayload>(
                        "Payload when `input.kind` is `plan_node_create`.",
                    ),
                ),
                (
                    "plan_edge_create",
                    described_schema::<PlanEdgeCreatePayload>(
                        "Payload when `input.kind` is `plan_edge_create`.",
                    ),
                ),
                (
                    "plan_edge_delete",
                    described_schema::<PlanEdgeDeletePayload>(
                        "Payload when `input.kind` is `plan_edge_delete`.",
                    ),
                ),
                (
                    "handoff",
                    described_schema::<HandoffPayload>("Payload when `input.kind` is `handoff`."),
                ),
                (
                    "handoff_accept",
                    described_schema::<HandoffAcceptPayload>(
                        "Payload when `input.kind` is `handoff_accept`.",
                    ),
                ),
            ],
        )),
        ("prism_mutate", "claim") => Some(tagged_union_payload_schema(
            "Payload for `prism_mutate` action `claim`. Match this shape to `input.action`.",
            "action",
            vec![
                (
                    "acquire",
                    described_schema::<ClaimAcquirePayload>(
                        "Payload when `input.action` is `acquire`.",
                    ),
                ),
                (
                    "renew",
                    described_schema::<ClaimRenewPayload>(
                        "Payload when `input.action` is `renew`.",
                    ),
                ),
                (
                    "release",
                    described_schema::<ClaimReleasePayload>(
                        "Payload when `input.action` is `release`.",
                    ),
                ),
            ],
        )),
        ("prism_mutate", "artifact") => Some(tagged_union_payload_schema(
            "Payload for `prism_mutate` action `artifact`. Match this shape to `input.action`.",
            "action",
            vec![
                (
                    "propose",
                    described_schema::<ArtifactProposePayload>(
                        "Payload when `input.action` is `propose`.",
                    ),
                ),
                (
                    "supersede",
                    described_schema::<ArtifactSupersedePayload>(
                        "Payload when `input.action` is `supersede`.",
                    ),
                ),
                (
                    "review",
                    described_schema::<ArtifactReviewPayload>(
                        "Payload when `input.action` is `review`.",
                    ),
                ),
            ],
        )),
        _ => None,
    }
}

fn payload_variant_views(
    tool_name: &str,
    action: &str,
    payload_discriminator: Option<&str>,
    action_examples: &[Value],
    action_input_schema: &Value,
) -> Vec<ToolPayloadVariantSchemaView> {
    let Some(discriminator) = payload_discriminator else {
        return Vec::new();
    };
    let payload_schema = action_input_schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get("payload"));
    let Some(variants) = payload_schema
        .and_then(|schema| schema.get("oneOf"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    variants
        .iter()
        .filter_map(|variant| {
            payload_variant_view(tool_name, action, discriminator, action_examples, variant)
        })
        .collect()
}

fn payload_variant_view(
    tool_name: &str,
    action: &str,
    discriminator: &str,
    action_examples: &[Value],
    variant_schema: &Value,
) -> Option<ToolPayloadVariantSchemaView> {
    let title = variant_schema.get("title")?.as_str()?;
    let (_, tag) = title.split_once('=')?;
    let required_fields = variant_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let fields = variant_schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.iter())
        .map(|(name, schema)| tool_field_view(variant_schema, name, schema, &required_fields))
        .collect::<Vec<_>>();
    let example_inputs = payload_variant_examples(action_examples, discriminator, tag);
    Some(ToolPayloadVariantSchemaView {
        tag: tag.to_string(),
        schema_uri: format!(
            "{}#payloadVariant={tag}",
            tool_action_schema_resource_uri(tool_name, action)
        ),
        required_fields,
        fields,
        schema: variant_schema.clone(),
        example_input: example_inputs.first().cloned(),
        example_inputs,
    })
}

fn payload_variant_examples(
    action_examples: &[Value],
    discriminator: &str,
    tag: &str,
) -> Vec<Value> {
    action_examples
        .iter()
        .filter_map(|example| {
            let input = example.get("input")?;
            let matches_variant = input.get(discriminator).and_then(Value::as_str) == Some(tag);
            matches_variant
                .then(|| input.get("payload").cloned())
                .flatten()
        })
        .collect()
}

fn described_schema<T: JsonSchema + 'static>(description: &str) -> Value {
    let mut schema = schema_value_for_type::<T>();
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    schema
}

fn tagged_union_payload_schema(
    description: &str,
    discriminator: &str,
    variants: Vec<(&str, Value)>,
) -> Value {
    let one_of = variants
        .into_iter()
        .map(|(tag, mut schema)| {
            if let Some(object) = schema.as_object_mut() {
                object.insert(
                    "title".to_string(),
                    Value::String(format!("{discriminator}={tag}")),
                );
            }
            schema
        })
        .collect::<Vec<_>>();
    json!({
        "description": description,
        "oneOf": one_of,
    })
}

fn schema_value_for_type<T: JsonSchema + 'static>() -> Value {
    let root = Value::Object(
        rmcp::handler::server::tool::schema_for_type::<T>()
            .as_ref()
            .clone(),
    );
    expand_schema_refs(&root, &root, 0)
}

fn tool_field_view(
    root_schema: &Value,
    name: &str,
    field_schema: &Value,
    required_fields: &[String],
) -> ToolFieldSchemaView {
    tool_field_view_with_depth(root_schema, name, field_schema, required_fields, 0)
}

fn tool_field_view_with_depth(
    root_schema: &Value,
    name: &str,
    field_schema: &Value,
    required_fields: &[String],
    depth: usize,
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
        nested_fields: nested_schema_fields(root_schema, resolved, depth + 1),
        schema: expand_schema_refs(root_schema, resolved, depth),
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

fn nested_schema_fields(
    root_schema: &Value,
    schema: &Value,
    depth: usize,
) -> Vec<ToolFieldSchemaView> {
    if depth > MAX_SCHEMA_SUMMARY_DEPTH {
        return Vec::new();
    }

    let resolved = resolve_schema_ref(root_schema, schema);
    if let Some(properties) = resolved.get("properties").and_then(Value::as_object) {
        let required_fields = resolved
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flat_map(|items| items.iter())
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        return properties
            .iter()
            .map(|(name, field_schema)| {
                tool_field_view_with_depth(root_schema, name, field_schema, &required_fields, depth)
            })
            .collect();
    }

    if let Some(items) = resolved.get("items") {
        return nested_schema_fields(root_schema, items, depth);
    }

    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(variants) = resolved.get(key).and_then(Value::as_array) {
            let mut merged = Vec::new();
            for variant in variants {
                for field in nested_schema_fields(root_schema, variant, depth) {
                    merge_nested_field(&mut merged, field);
                }
            }
            if !merged.is_empty() {
                return merged;
            }
        }
    }

    Vec::new()
}

fn merge_nested_field(fields: &mut Vec<ToolFieldSchemaView>, incoming: ToolFieldSchemaView) {
    if let Some(existing) = fields.iter_mut().find(|field| field.name == incoming.name) {
        existing.required &= incoming.required;
        if existing.description.is_none() {
            existing.description = incoming.description;
        }
        merge_unique_strings(&mut existing.types, incoming.types);
        merge_unique_strings(&mut existing.enum_values, incoming.enum_values);
        for nested in incoming.nested_fields {
            merge_nested_field(&mut existing.nested_fields, nested);
        }
        return;
    }
    fields.push(incoming);
}

fn expand_schema_refs(root_schema: &Value, schema: &Value, depth: usize) -> Value {
    if depth >= MAX_SCHEMA_SUMMARY_DEPTH {
        return resolve_schema_ref(root_schema, schema).clone();
    }

    match resolve_schema_ref(root_schema, schema) {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| expand_schema_refs(root_schema, item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        expand_schema_refs(root_schema, value, depth + 1),
                    )
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn merge_unique_strings(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}
