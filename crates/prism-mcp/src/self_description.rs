use rmcp::{
    model::{RawResource, ResourceContents},
    ErrorData as McpError,
};
use serde_json::{json, Value};

use crate::{
    capabilities_resource_uri, json_resource_contents_with_meta, resource_link_view, resource_meta,
    schema_resource_uri, schema_resource_value, session_resource_view_link, split_resource_uri,
    tool_action_schema_value, tool_action_schema_view, tool_schema_resource_uri, tool_schema_view,
    tool_schemas_resource_view_link, tool_variant_schema_value, vocab_resource_uri,
    CapabilitiesResourcePayload, CapabilitiesSectionResourcePayload, ContractsResourcePayload,
    EdgeResourcePayload, EntrypointsResourcePayload, EventResourcePayload, FileResourcePayload,
    LineageResourcePayload, MemoryResourcePayload, PlanResourcePayload, PlansResourcePayload,
    ProtectedStateResourcePayload, ResourceExampleResourcePayload, ResourceLinkView,
    ResourceShapeResourcePayload, SearchResourcePayload, SelfDescriptionAuditEntry,
    SelfDescriptionAuditPayload, SessionResourcePayload, ShapeFieldView, SymbolResourcePayload,
    TaskResourcePayload, ToolActionShapeView, ToolExampleResourcePayload, ToolShapeResourcePayload,
    ToolVariantShapeView, VocabularyEntryResourcePayload, VocabularyResourcePayload,
};

pub(crate) const SELF_DESCRIPTION_BUDGET_BYTES: usize = 12 * 1024;
const MAX_COMPACT_EXAMPLES_PER_SURFACE: usize = 3;

pub(crate) fn self_description_audit_resource_uri() -> String {
    crate::SELF_DESCRIPTION_AUDIT_URI.to_string()
}

pub(crate) fn capabilities_section_resource_uri(section: &str) -> String {
    format!(
        "prism://capabilities/{}",
        crate::percent_encode_component(section)
    )
}

pub(crate) fn parse_capabilities_section_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://capabilities/")
        .map(crate::percent_decode_lossy)
        .filter(|section| !section.trim().is_empty())
}

pub(crate) fn vocab_entry_resource_uri(key: &str) -> String {
    format!("prism://vocab/{}", crate::percent_encode_component(key))
}

pub(crate) fn parse_vocab_entry_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://vocab/")
        .map(crate::percent_decode_lossy)
        .filter(|key| !key.trim().is_empty())
}

pub(crate) fn tool_example_resource_uri(tool_name: &str) -> String {
    format!(
        "prism://example/tool/{}",
        crate::percent_encode_component(tool_name)
    )
}

pub(crate) fn tool_action_example_resource_uri(tool_name: &str, action: &str) -> String {
    format!(
        "prism://example/tool/{}/action/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action)
    )
}

pub(crate) fn tool_variant_example_resource_uri(
    tool_name: &str,
    action: &str,
    tag: &str,
) -> String {
    format!(
        "prism://example/tool/{}/action/{}/variant/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action),
        crate::percent_encode_component(tag)
    )
}

pub(crate) fn parse_tool_example_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://example/tool/")
        .map(crate::percent_decode_lossy)
        .filter(|tool_name| !tool_name.trim().is_empty() && !tool_name.contains("/action/"))
}

pub(crate) fn parse_tool_action_example_resource_uri(uri: &str) -> Option<(String, String)> {
    parse_nested_tool_resource_uri(uri, "prism://example/tool/", "/action/")
}

pub(crate) fn parse_tool_variant_example_resource_uri(
    uri: &str,
) -> Option<(String, String, String)> {
    parse_nested_tool_variant_resource_uri(uri, "prism://example/tool/")
}

pub(crate) fn tool_shape_resource_uri(tool_name: &str) -> String {
    format!(
        "prism://shape/tool/{}",
        crate::percent_encode_component(tool_name)
    )
}

pub(crate) fn tool_action_shape_resource_uri(tool_name: &str, action: &str) -> String {
    format!(
        "prism://shape/tool/{}/action/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action)
    )
}

pub(crate) fn tool_variant_shape_resource_uri(tool_name: &str, action: &str, tag: &str) -> String {
    format!(
        "prism://shape/tool/{}/action/{}/variant/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action),
        crate::percent_encode_component(tag)
    )
}

pub(crate) fn parse_tool_shape_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://shape/tool/")
        .map(crate::percent_decode_lossy)
        .filter(|tool_name| !tool_name.trim().is_empty() && !tool_name.contains("/action/"))
}

pub(crate) fn parse_tool_action_shape_resource_uri(uri: &str) -> Option<(String, String)> {
    parse_nested_tool_resource_uri(uri, "prism://shape/tool/", "/action/")
}

pub(crate) fn parse_tool_variant_shape_resource_uri(uri: &str) -> Option<(String, String, String)> {
    parse_nested_tool_variant_resource_uri(uri, "prism://shape/tool/")
}

pub(crate) fn tool_variant_schema_resource_uri(tool_name: &str, action: &str, tag: &str) -> String {
    format!(
        "prism://schema/tool/{}/action/{}/variant/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action),
        crate::percent_encode_component(tag)
    )
}

pub(crate) fn parse_tool_variant_schema_resource_uri(
    uri: &str,
) -> Option<(String, String, String)> {
    parse_nested_tool_variant_resource_uri(uri, "prism://schema/tool/")
}

pub(crate) fn resource_example_resource_uri(resource_kind: &str) -> String {
    format!(
        "prism://example/resource/{}",
        crate::percent_encode_component(resource_kind)
    )
}

pub(crate) fn parse_resource_example_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://example/resource/")
        .map(crate::percent_decode_lossy)
        .filter(|resource_kind| !resource_kind.trim().is_empty())
}

pub(crate) fn resource_shape_resource_uri(resource_kind: &str) -> String {
    format!(
        "prism://shape/resource/{}",
        crate::percent_encode_component(resource_kind)
    )
}

pub(crate) fn parse_resource_shape_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://shape/resource/")
        .map(crate::percent_decode_lossy)
        .filter(|resource_kind| !resource_kind.trim().is_empty())
}

pub(crate) fn tool_action_recipe_resource_uri(tool_name: &str, action: &str) -> String {
    format!(
        "prism://recipe/tool/{}/action/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action)
    )
}

pub(crate) fn tool_variant_recipe_resource_uri(tool_name: &str, action: &str, tag: &str) -> String {
    format!(
        "prism://recipe/tool/{}/action/{}/variant/{}",
        crate::percent_encode_component(tool_name),
        crate::percent_encode_component(action),
        crate::percent_encode_component(tag)
    )
}

pub(crate) fn parse_tool_action_recipe_resource_uri(uri: &str) -> Option<(String, String)> {
    parse_nested_tool_resource_uri(uri, "prism://recipe/tool/", "/action/")
}

pub(crate) fn parse_tool_variant_recipe_resource_uri(
    uri: &str,
) -> Option<(String, String, String)> {
    parse_nested_tool_variant_resource_uri(uri, "prism://recipe/tool/")
}

pub(crate) fn self_description_audit_resource_link() -> RawResource {
    RawResource::new(self_description_audit_resource_uri(), "PRISM Self-Description Audit")
        .with_description(
            "Audit the MCP self-description surface, companion compact resources, and byte-budget risks.",
        )
        .with_mime_type("application/json")
}

pub(crate) fn resource_shape_resource_view_link(resource_kind: &str) -> ResourceLinkView {
    resource_link_view(
        resource_shape_resource_uri(resource_kind),
        format!("PRISM Resource Shape: {resource_kind}"),
        format!("Compact shape summary for resource `{resource_kind}`"),
    )
}

pub(crate) fn tool_shape_resource_contents(
    tool_name: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = tool_shape_payload(tool_name, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-shape",
            Some(schema_resource_uri("tool-shape")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_action_shape_resource_contents(
    tool_name: &str,
    action: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = tool_action_shape_payload(tool_name, action, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-shape",
            Some(schema_resource_uri("tool-shape")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_variant_shape_resource_contents(
    tool_name: &str,
    action: &str,
    tag: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = tool_variant_shape_payload(tool_name, action, tag, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-shape",
            Some(schema_resource_uri("tool-shape")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_example_resource_contents(
    tool_name: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = tool_example_payload(tool_name, None, None, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-example",
            Some(schema_resource_uri("tool-example")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_action_example_resource_contents(
    tool_name: &str,
    action: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = tool_example_payload(tool_name, Some(action), None, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-example",
            Some(schema_resource_uri("tool-example")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_variant_example_resource_contents(
    tool_name: &str,
    action: &str,
    tag: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload =
        tool_example_payload(tool_name, Some(action), Some(tag), uri).ok_or_else(|| {
            McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
        })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "tool-example",
            Some(schema_resource_uri("tool-example")),
            Some(tool_name),
        )),
    )
}

pub(crate) fn tool_variant_schema_resource_contents(
    tool_name: &str,
    action: &str,
    tag: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let schema = tool_variant_schema_value(tool_name, action, tag).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        schema,
        uri.to_string(),
        Some(resource_meta(
            "tool-variant-schema",
            Some(tool_variant_schema_resource_uri(tool_name, action, tag)),
            Some(tool_name),
        )),
    )
    .map(|contents| contents.with_mime_type("application/schema+json"))
}

pub(crate) fn resource_shape_resource_contents(
    resource_kind: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = resource_shape_payload(resource_kind, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "resource-shape",
            Some(schema_resource_uri("resource-shape")),
            Some(resource_kind),
        )),
    )
}

pub(crate) fn resource_example_resource_contents(
    resource_kind: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = resource_example_payload(resource_kind, uri).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "resource-example",
            Some(schema_resource_uri("resource-example")),
            Some(resource_kind),
        )),
    )
}

pub(crate) fn capabilities_section_resource_contents(
    payload: CapabilitiesResourcePayload,
    section: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let value = serde_json::to_value(&payload).map_err(internal_serialize_error)?;
    let section_value = value.get(section).cloned().ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    let body = CapabilitiesSectionResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("capabilities-section"),
        section: section.to_string(),
        value: section_value,
        related_resources: vec![
            resource_link_view(
                capabilities_resource_uri(),
                "PRISM Capabilities",
                "Canonical capability map for query methods, resources, features, and build info",
            ),
            resource_shape_resource_view_link("capabilities"),
        ],
    };
    json_resource_contents_with_meta(
        body,
        uri.to_string(),
        Some(resource_meta(
            "capabilities-section",
            Some(schema_resource_uri("capabilities-section")),
            Some("capabilities"),
        )),
    )
}

pub(crate) fn vocab_entry_resource_contents(
    payload: VocabularyResourcePayload,
    key: &str,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let value = serde_json::to_value(&payload).map_err(internal_serialize_error)?;
    let vocabulary = value
        .get("vocabularies")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.get("key").and_then(Value::as_str) == Some(key))
                .cloned()
        })
        .ok_or_else(|| {
            McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
        })?;
    let body = VocabularyEntryResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("vocab-entry"),
        key: key.to_string(),
        vocabulary,
        related_resources: vec![
            resource_link_view(
                vocab_resource_uri(),
                "PRISM Vocabulary",
                "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads",
            ),
            resource_shape_resource_view_link("vocab"),
        ],
    };
    json_resource_contents_with_meta(
        body,
        uri.to_string(),
        Some(resource_meta(
            "vocab-entry",
            Some(schema_resource_uri("vocab-entry")),
            Some("vocab"),
        )),
    )
}

pub(crate) fn tool_recipe_resource_contents(
    tool_name: &str,
    action: &str,
    variant: Option<&str>,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let markdown = tool_recipe_markdown(tool_name, action, variant).ok_or_else(|| {
        McpError::resource_not_found("resource_not_found", Some(json!({ "uri": uri })))
    })?;
    Ok(ResourceContents::text(markdown, uri.to_string()).with_mime_type("text/markdown"))
}

pub(crate) fn self_description_audit_resource_contents(
    capabilities: CapabilitiesResourcePayload,
    uri: &str,
) -> Result<ResourceContents, McpError> {
    let payload = self_description_audit_payload(capabilities, uri)?;
    json_resource_contents_with_meta(
        payload,
        uri.to_string(),
        Some(resource_meta(
            "self-description-audit",
            Some(schema_resource_uri("self-description-audit")),
            None,
        )),
    )
}

fn tool_shape_payload(tool_name: &str, uri: &str) -> Option<ToolShapeResourcePayload> {
    let tool = tool_schema_view(tool_name)?;
    let required_fields = tool
        .actions
        .iter()
        .find(|action| action.action == "default")
        .map(|action| action.required_fields.clone())
        .unwrap_or_else(|| schema_required_fields(&tool.input_schema));
    let fields = compact_shape_fields(&tool.input_schema);
    let optional_fields = field_names(&fields, &required_fields);
    Some(ToolShapeResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("tool-shape"),
        tool_name: tool.tool_name.clone(),
        tool_schema_uri: tool.schema_uri.clone(),
        example_uri: Some(tool_example_resource_uri(tool_name)),
        description: tool.description,
        required_fields,
        optional_fields,
        fields,
        actions: tool
            .actions
            .iter()
            .map(|action| action_shape_view(tool_name, action, None, false, false, false))
            .collect(),
        related_resources: vec![
            tool_schemas_resource_view_link(),
            resource_link_view(
                tool_schema_resource_uri(tool_name),
                format!("PRISM Tool Schema: {tool_name}"),
                format!("JSON Schema for the `{tool_name}` tool input payload"),
            ),
        ],
    })
}

fn tool_action_shape_payload(
    tool_name: &str,
    action: &str,
    uri: &str,
) -> Option<ToolShapeResourcePayload> {
    let tool = tool_schema_view(tool_name)?;
    let action_view = tool
        .actions
        .iter()
        .find(|candidate| candidate.action == action)?;
    Some(ToolShapeResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("tool-shape"),
        tool_name: tool.tool_name.clone(),
        tool_schema_uri: tool.schema_uri.clone(),
        example_uri: Some(tool_action_example_resource_uri(tool_name, action)),
        description: action_view
            .description
            .clone()
            .unwrap_or_else(|| tool.description.clone()),
        required_fields: action_view.required_fields.clone(),
        optional_fields: field_names_from_tool_fields(
            &action_view.fields,
            &action_view.required_fields,
        ),
        fields: compact_tool_fields(&action_view.fields),
        actions: vec![action_shape_view(
            tool_name,
            action_view,
            None,
            true,
            true,
            false,
        )],
        related_resources: vec![
            tool_schemas_resource_view_link(),
            resource_link_view(
                crate::tool_action_schema_resource_uri(tool_name, action),
                format!("PRISM Tool Action Schema: {tool_name}.{action}"),
                format!("Exact action schema for `{tool_name}` action `{action}`"),
            ),
        ],
    })
}

fn tool_variant_shape_payload(
    tool_name: &str,
    action: &str,
    tag: &str,
    uri: &str,
) -> Option<ToolShapeResourcePayload> {
    let action_view = tool_action_schema_view(tool_name, action)?;
    let variant = action_view
        .payload_variants
        .iter()
        .find(|candidate| candidate.tag == tag)?;
    let fields = compact_tool_fields(&variant.fields);
    let required_fields = variant.required_fields.clone();
    let optional_fields = field_names_from_tool_fields(&variant.fields, &required_fields);
    Some(ToolShapeResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("tool-shape"),
        tool_name: tool_name.to_string(),
        tool_schema_uri: crate::tool_action_schema_resource_uri(tool_name, action),
        example_uri: Some(tool_variant_example_resource_uri(tool_name, action, tag)),
        description: format!(
            "Compact shape summary for `{tool_name}` action `{action}` variant `{tag}`"
        ),
        required_fields,
        optional_fields,
        fields,
        actions: Vec::new(),
        related_resources: vec![
            resource_link_view(
                tool_variant_schema_resource_uri(tool_name, action, tag),
                format!("PRISM Tool Variant Schema: {tool_name}.{action}.{tag}"),
                format!("Exact payload schema for `{tool_name}` action `{action}` variant `{tag}`"),
            ),
            tool_schemas_resource_view_link(),
        ],
    })
}

fn tool_example_payload(
    tool_name: &str,
    action: Option<&str>,
    variant: Option<&str>,
    uri: &str,
) -> Option<ToolExampleResourcePayload> {
    let (examples, target_schema_uri, shape_uri, recipe_uri, discriminator) =
        match (action, variant) {
            (None, None) => (
                crate::tool_input_examples(tool_name)?,
                Some(tool_schema_resource_uri(tool_name)),
                Some(tool_shape_resource_uri(tool_name)),
                None,
                None,
            ),
            (Some(action), None) => {
                let action_view = tool_action_schema_view(tool_name, action)?;
                (
                    action_view.example_inputs.clone(),
                    Some(crate::tool_action_schema_resource_uri(tool_name, action)),
                    Some(tool_action_shape_resource_uri(tool_name, action)),
                    Some(tool_action_recipe_resource_uri(tool_name, action)),
                    action_view.payload_discriminator.clone(),
                )
            }
            (Some(action), Some(tag)) => {
                let action_view = tool_action_schema_view(tool_name, action)?;
                (
                    crate::tool_action_examples(tool_name, action)
                        .into_iter()
                        .filter_map(|example| {
                            let input = example.get("input")?;
                            let discriminator = action_view.payload_discriminator.as_ref()?;
                            (input.get(discriminator).and_then(Value::as_str) == Some(tag))
                                .then(|| input.get("payload").cloned())
                                .flatten()
                        })
                        .collect::<Vec<_>>(),
                    Some(tool_variant_schema_resource_uri(tool_name, action, tag)),
                    Some(tool_variant_shape_resource_uri(tool_name, action, tag)),
                    Some(tool_variant_recipe_resource_uri(tool_name, action, tag)),
                    action_view.payload_discriminator.clone(),
                )
            }
            (None, Some(_)) => return None,
        };
    let examples = compact_examples(examples);
    let example = examples.first().cloned()?;
    Some(ToolExampleResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("tool-example"),
        tool_name: tool_name.to_string(),
        action: action.map(ToString::to_string),
        variant: variant.map(ToString::to_string),
        discriminator,
        target_schema_uri,
        shape_uri,
        recipe_uri,
        example,
        examples,
        related_resources: vec![tool_schemas_resource_view_link()],
    })
}

fn resource_shape_payload(resource_kind: &str, uri: &str) -> Option<ResourceShapeResourcePayload> {
    let (schema, description) = resource_schema_shape_source(resource_kind)?;
    let required_fields = schema_required_fields(&schema);
    let fields = compact_shape_fields(&schema);
    let optional_fields = field_names(&fields, &required_fields);
    Some(ResourceShapeResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("resource-shape"),
        resource_kind: resource_kind.to_string(),
        resource_schema_uri: schema_resource_uri(resource_kind),
        example_uri: Some(resource_example_resource_uri(resource_kind)),
        description: description.to_string(),
        required_fields,
        optional_fields,
        fields,
        related_resources: vec![
            resource_link_view(
                schema_resource_uri(resource_kind),
                format!("PRISM Schema: {resource_kind}"),
                format!("JSON Schema for the `{resource_kind}` PRISM resource payload"),
            ),
            session_resource_view_link(),
        ],
    })
}

fn resource_example_payload(
    resource_kind: &str,
    uri: &str,
) -> Option<ResourceExampleResourcePayload> {
    let example = crate::resource_payload_example(resource_kind)?;
    Some(ResourceExampleResourcePayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("resource-example"),
        resource_kind: resource_kind.to_string(),
        resource_schema_uri: schema_resource_uri(resource_kind),
        shape_uri: resource_shape_resource_uri(resource_kind),
        example,
        related_resources: vec![
            resource_link_view(
                schema_resource_uri(resource_kind),
                format!("PRISM Schema: {resource_kind}"),
                format!("JSON Schema for the `{resource_kind}` PRISM resource payload"),
            ),
            resource_shape_resource_view_link(resource_kind),
        ],
    })
}

fn self_description_audit_payload(
    capabilities: CapabilitiesResourcePayload,
    uri: &str,
) -> Result<SelfDescriptionAuditPayload, McpError> {
    let mut entries = Vec::new();
    for tool in crate::tool_schema_catalog_entries() {
        let schema = crate::tool_input_schema_value(&tool.tool_name);
        let full_bytes = schema.as_ref().and_then(value_bytes);
        let example_validation = tool_example_validation(&tool.tool_name, None, None);
        let example_bytes = tool
            .example_uri
            .as_ref()
            .and_then(|uri| tool_example_payload(&tool.tool_name, None, None, uri))
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
        let shape_bytes = tool
            .shape_uri
            .as_ref()
            .and_then(|uri| tool_shape_payload(&tool.tool_name, uri))
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
        entries.push(SelfDescriptionAuditEntry {
            surface_kind: "tool".to_string(),
            name: tool.tool_name.clone(),
            full_uri: Some(tool.schema_uri.clone()),
            schema_uri: Some(tool.schema_uri.clone()),
            example_uri: tool.example_uri.clone(),
            shape_uri: tool.shape_uri.clone(),
            recipe_uri: None,
            full_bytes,
            schema_bytes: full_bytes,
            example_bytes,
            shape_bytes,
            recipe_bytes: None,
            example_valid: example_validation
                .as_ref()
                .map(|validation| validation.valid),
            example_validation_issue_codes: example_validation_issue_codes(
                example_validation.as_ref(),
            ),
            source_free_operable: source_free_operable(
                tool.example_uri.is_some(),
                tool.shape_uri.is_some(),
                false,
                example_bytes,
                shape_bytes,
                None,
                example_validation
                    .as_ref()
                    .map(|validation| validation.valid),
            ),
            issues: audit_issues(
                tool.example_uri.is_some(),
                tool.shape_uri.is_some(),
                false,
                full_bytes,
                example_bytes,
                shape_bytes,
                None,
                example_validation
                    .as_ref()
                    .map(|validation| validation.valid),
            ),
        });
        if let Some(tool_view) = tool_schema_view(&tool.tool_name) {
            for action in tool_view.actions {
                let action_schema_uri =
                    crate::tool_action_schema_resource_uri(&tool.tool_name, &action.action);
                let action_example_uri = Some(tool_action_example_resource_uri(
                    &tool.tool_name,
                    &action.action,
                ));
                let action_shape_uri = Some(tool_action_shape_resource_uri(
                    &tool.tool_name,
                    &action.action,
                ));
                let action_recipe_uri = Some(tool_action_recipe_resource_uri(
                    &tool.tool_name,
                    &action.action,
                ));
                let action_recipe_bytes =
                    tool_recipe_markdown(&tool.tool_name, &action.action, None)
                        .map(|markdown| markdown.len());
                let schema_bytes = tool_action_schema_value(&tool.tool_name, &action.action)
                    .as_ref()
                    .and_then(value_bytes);
                let example_validation =
                    tool_example_validation(&tool.tool_name, Some(&action.action), None);
                let example_bytes = action_example_uri
                    .as_ref()
                    .and_then(|uri| {
                        tool_example_payload(&tool.tool_name, Some(&action.action), None, uri)
                    })
                    .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
                let shape_bytes = action_shape_uri
                    .as_ref()
                    .and_then(|uri| tool_action_shape_payload(&tool.tool_name, &action.action, uri))
                    .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
                entries.push(SelfDescriptionAuditEntry {
                    surface_kind: "tool_action".to_string(),
                    name: format!("{}.{}", tool.tool_name, action.action),
                    full_uri: Some(action_schema_uri.clone()),
                    schema_uri: Some(action_schema_uri),
                    example_uri: action_example_uri.clone(),
                    shape_uri: action_shape_uri.clone(),
                    recipe_uri: action_recipe_uri.clone(),
                    full_bytes: schema_bytes,
                    schema_bytes,
                    example_bytes,
                    shape_bytes,
                    recipe_bytes: action_recipe_bytes,
                    example_valid: example_validation
                        .as_ref()
                        .map(|validation| validation.valid),
                    example_validation_issue_codes: example_validation_issue_codes(
                        example_validation.as_ref(),
                    ),
                    source_free_operable: source_free_operable(
                        true,
                        true,
                        true,
                        example_bytes,
                        shape_bytes,
                        action_recipe_bytes,
                        example_validation
                            .as_ref()
                            .map(|validation| validation.valid),
                    ),
                    issues: audit_issues(
                        true,
                        true,
                        true,
                        schema_bytes,
                        example_bytes,
                        shape_bytes,
                        action_recipe_bytes,
                        example_validation
                            .as_ref()
                            .map(|validation| validation.valid),
                    ),
                });
                for variant in action.payload_variants {
                    let schema_uri = tool_variant_schema_resource_uri(
                        &tool.tool_name,
                        &action.action,
                        &variant.tag,
                    );
                    let example_uri = Some(tool_variant_example_resource_uri(
                        &tool.tool_name,
                        &action.action,
                        &variant.tag,
                    ));
                    let shape_uri = Some(tool_variant_shape_resource_uri(
                        &tool.tool_name,
                        &action.action,
                        &variant.tag,
                    ));
                    let recipe_uri = Some(tool_variant_recipe_resource_uri(
                        &tool.tool_name,
                        &action.action,
                        &variant.tag,
                    ));
                    let recipe_bytes =
                        tool_recipe_markdown(&tool.tool_name, &action.action, Some(&variant.tag))
                            .map(|markdown| markdown.len());
                    let schema_bytes =
                        tool_variant_schema_value(&tool.tool_name, &action.action, &variant.tag)
                            .as_ref()
                            .and_then(value_bytes);
                    let example_validation = tool_example_validation(
                        &tool.tool_name,
                        Some(&action.action),
                        Some(&variant.tag),
                    );
                    let example_bytes = example_uri
                        .as_ref()
                        .and_then(|uri| {
                            tool_example_payload(
                                &tool.tool_name,
                                Some(&action.action),
                                Some(&variant.tag),
                                uri,
                            )
                        })
                        .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
                    let shape_bytes = shape_uri
                        .as_ref()
                        .and_then(|uri| {
                            tool_variant_shape_payload(
                                &tool.tool_name,
                                &action.action,
                                &variant.tag,
                                uri,
                            )
                        })
                        .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
                    entries.push(SelfDescriptionAuditEntry {
                        surface_kind: "tool_variant".to_string(),
                        name: format!("{}.{}.{}", tool.tool_name, action.action, variant.tag),
                        full_uri: Some(schema_uri.clone()),
                        schema_uri: Some(schema_uri),
                        example_uri,
                        shape_uri,
                        recipe_uri,
                        full_bytes: schema_bytes,
                        schema_bytes,
                        example_bytes,
                        shape_bytes,
                        recipe_bytes,
                        example_valid: example_validation
                            .as_ref()
                            .map(|validation| validation.valid),
                        example_validation_issue_codes: example_validation_issue_codes(
                            example_validation.as_ref(),
                        ),
                        source_free_operable: source_free_operable(
                            true,
                            true,
                            true,
                            example_bytes,
                            shape_bytes,
                            recipe_bytes,
                            example_validation
                                .as_ref()
                                .map(|validation| validation.valid),
                        ),
                        issues: audit_issues(
                            true,
                            true,
                            true,
                            schema_bytes,
                            example_bytes,
                            shape_bytes,
                            recipe_bytes,
                            example_validation
                                .as_ref()
                                .map(|validation| validation.valid),
                        ),
                    });
                }
            }
        }
    }
    for resource in capabilities.resources.iter() {
        let resource_kind = resource_name_to_kind(resource);
        let structured = resource.schema_uri.is_some();
        let compact_example_uri = structured.then(|| resource_example_resource_uri(resource_kind));
        let full_bytes = resource.example_uri.as_ref().and_then(|_uri| {
            crate::resource_payload_example(resource_kind)
                .as_ref()
                .and_then(value_bytes)
        });
        let shape_bytes = resource
            .shape_uri
            .as_ref()
            .and_then(|uri| resource_shape_payload(resource_kind, uri))
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
        let example_bytes = compact_example_uri
            .as_ref()
            .and_then(|uri| resource_example_payload(resource_kind, uri))
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
        entries.push(SelfDescriptionAuditEntry {
            surface_kind: "resource".to_string(),
            name: resource.name.clone(),
            full_uri: Some(resource.uri.clone()),
            schema_uri: resource.schema_uri.clone(),
            example_uri: compact_example_uri
                .clone()
                .or_else(|| resource.example_uri.clone()),
            shape_uri: resource.shape_uri.clone(),
            recipe_uri: None,
            full_bytes,
            schema_bytes: resource.schema_uri.as_ref().and_then(|_schema_uri| {
                resource_schema_shape_source(resource_kind)
                    .as_ref()
                    .and_then(|(schema, _)| value_bytes(schema))
            }),
            example_bytes,
            shape_bytes,
            recipe_bytes: None,
            example_valid: None,
            example_validation_issue_codes: Vec::new(),
            source_free_operable: if structured {
                source_free_operable(
                    compact_example_uri.is_some(),
                    resource.shape_uri.is_some(),
                    false,
                    example_bytes,
                    shape_bytes,
                    None,
                    None,
                )
            } else {
                true
            },
            issues: if structured {
                audit_issues(
                    compact_example_uri.is_some(),
                    resource.shape_uri.is_some(),
                    false,
                    full_bytes,
                    example_bytes,
                    shape_bytes,
                    None,
                    None,
                )
            } else {
                Vec::new()
            },
        });
    }
    for template in capabilities.resource_templates.iter() {
        let (schema_uri, compact_example_uri, recipe_uri, has_shape, expects_recipe) =
            if let Some((tool_name, action, tag)) = template
                .shape_uri
                .as_deref()
                .and_then(parse_tool_variant_shape_resource_uri)
            {
                (
                    Some(tool_variant_schema_resource_uri(&tool_name, &action, &tag)),
                    Some(tool_variant_example_resource_uri(&tool_name, &action, &tag)),
                    Some(tool_variant_recipe_resource_uri(&tool_name, &action, &tag)),
                    true,
                    true,
                )
            } else if let Some((tool_name, action)) = template
                .shape_uri
                .as_deref()
                .and_then(parse_tool_action_shape_resource_uri)
            {
                (
                    Some(crate::tool_action_schema_resource_uri(&tool_name, &action)),
                    Some(tool_action_example_resource_uri(&tool_name, &action)),
                    Some(tool_action_recipe_resource_uri(&tool_name, &action)),
                    true,
                    true,
                )
            } else if let Some(tool_name) = template
                .shape_uri
                .as_deref()
                .and_then(parse_tool_shape_resource_uri)
            {
                (
                    Some(tool_schema_resource_uri(&tool_name)),
                    Some(tool_example_resource_uri(&tool_name)),
                    None,
                    true,
                    false,
                )
            } else if let Some(resource_kind) = template
                .shape_uri
                .as_deref()
                .and_then(parse_resource_shape_resource_uri)
            {
                (
                    Some(schema_resource_uri(&resource_kind)),
                    Some(resource_example_resource_uri(&resource_kind)),
                    None,
                    true,
                    false,
                )
            } else if template.uri_template.starts_with("prism://recipe/tool/") {
                (
                    None,
                    template.example_uri.clone(),
                    template.example_uri.clone(),
                    true,
                    false,
                )
            } else {
                (None, None, None, false, false)
            };
        let schema_bytes = schema_uri.as_deref().and_then(schema_uri_to_bytes);
        let example_bytes = compact_example_uri
            .as_deref()
            .and_then(compact_surface_bytes);
        let shape_bytes = if template.uri_template.starts_with("prism://recipe/tool/") {
            example_bytes
        } else {
            template
                .shape_uri
                .as_deref()
                .and_then(compact_surface_bytes)
        };
        let recipe_bytes = recipe_uri.as_deref().and_then(compact_surface_bytes);
        let has_example = compact_example_uri.is_some();
        entries.push(SelfDescriptionAuditEntry {
            surface_kind: "resource_template".to_string(),
            name: template.name.clone(),
            full_uri: Some(template.uri_template.clone()),
            schema_uri,
            example_uri: compact_example_uri.clone(),
            shape_uri: template.shape_uri.clone(),
            recipe_uri,
            full_bytes: None,
            schema_bytes,
            example_bytes,
            shape_bytes,
            recipe_bytes,
            example_valid: None,
            example_validation_issue_codes: Vec::new(),
            source_free_operable: source_free_operable(
                has_example,
                has_shape,
                expects_recipe,
                example_bytes,
                shape_bytes,
                recipe_bytes,
                None,
            ),
            issues: audit_issues(
                has_example,
                has_shape,
                expects_recipe,
                schema_bytes,
                example_bytes,
                shape_bytes,
                recipe_bytes,
                None,
            ),
        });
    }
    let oversize_entries = entries
        .iter()
        .filter(|entry| {
            entry.schema_bytes.unwrap_or(0) > SELF_DESCRIPTION_BUDGET_BYTES
                || entry.example_bytes.unwrap_or(0) > SELF_DESCRIPTION_BUDGET_BYTES
                || entry.shape_bytes.unwrap_or(0) > SELF_DESCRIPTION_BUDGET_BYTES
                || entry.recipe_bytes.unwrap_or(0) > SELF_DESCRIPTION_BUDGET_BYTES
        })
        .count();
    let missing_companion_entries = entries
        .iter()
        .filter(|entry| {
            entry
                .issues
                .iter()
                .any(|issue| issue == "missing_example" || issue == "missing_shape")
        })
        .count();
    let missing_recipe_entries = entries
        .iter()
        .filter(|entry| entry.issues.iter().any(|issue| issue == "missing_recipe"))
        .count();
    let invalid_example_entries = entries
        .iter()
        .filter(|entry| entry.issues.iter().any(|issue| issue == "example_invalid"))
        .count();
    let non_operable_entries = entries
        .iter()
        .filter(|entry| !entry.source_free_operable)
        .count();
    Ok(SelfDescriptionAuditPayload {
        uri: uri.to_string(),
        schema_uri: schema_resource_uri("self-description-audit"),
        budget_bytes: SELF_DESCRIPTION_BUDGET_BYTES,
        total_entries: entries.len(),
        oversize_entries,
        missing_companion_entries,
        missing_recipe_entries,
        invalid_example_entries,
        non_operable_entries,
        entries,
        related_resources: vec![
            resource_link_view(
                capabilities_resource_uri(),
                "PRISM Capabilities",
                "Canonical capability map for query methods, resources, features, and build info",
            ),
            tool_schemas_resource_view_link(),
            resource_shape_resource_view_link("capabilities"),
        ],
    })
}

fn action_shape_view(
    tool_name: &str,
    action: &prism_js::ToolActionSchemaView,
    variant_filter: Option<&str>,
    include_action_fields: bool,
    include_variants: bool,
    include_variant_fields: bool,
) -> ToolActionShapeView {
    let compact_index = !include_action_fields && !include_variants && !include_variant_fields;
    let required_fields = action.required_fields.clone();
    ToolActionShapeView {
        action: action.action.clone(),
        schema_uri: action.schema_uri.clone(),
        example_uri: (!compact_index)
            .then(|| tool_action_example_resource_uri(tool_name, &action.action)),
        shape_uri: tool_action_shape_resource_uri(tool_name, &action.action),
        recipe_uri: (!compact_index)
            .then(|| tool_action_recipe_resource_uri(tool_name, &action.action)),
        description: (!compact_index)
            .then(|| action.description.clone())
            .flatten(),
        required_fields: if include_action_fields {
            required_fields.clone()
        } else {
            Vec::new()
        },
        optional_fields: if include_action_fields {
            field_names_from_tool_fields(&action.fields, &required_fields)
        } else {
            Vec::new()
        },
        fields: if include_action_fields {
            compact_tool_fields(&action.fields)
        } else {
            Vec::new()
        },
        payload_discriminator: include_variants
            .then(|| action.payload_discriminator.clone())
            .flatten(),
        variants: if include_variants {
            action
                .payload_variants
                .iter()
                .filter(|variant| variant_filter.map(|tag| variant.tag == tag).unwrap_or(true))
                .map(|variant| {
                    variant_shape_view(
                        tool_name,
                        &action.action,
                        action.payload_discriminator.as_deref(),
                        variant,
                        include_variant_fields,
                    )
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

fn variant_shape_view(
    tool_name: &str,
    action: &str,
    payload_discriminator: Option<&str>,
    variant: &prism_js::ToolPayloadVariantSchemaView,
    include_fields: bool,
) -> ToolVariantShapeView {
    ToolVariantShapeView {
        tag: variant.tag.clone(),
        discriminator: payload_discriminator
            .map(ToString::to_string)
            .unwrap_or_else(|| "variant".to_string()),
        schema_uri: tool_variant_schema_resource_uri(tool_name, action, &variant.tag),
        example_uri: Some(tool_variant_example_resource_uri(
            tool_name,
            action,
            &variant.tag,
        )),
        shape_uri: tool_variant_shape_resource_uri(tool_name, action, &variant.tag),
        recipe_uri: Some(tool_variant_recipe_resource_uri(
            tool_name,
            action,
            &variant.tag,
        )),
        required_fields: if include_fields {
            variant.required_fields.clone()
        } else {
            Vec::new()
        },
        optional_fields: if include_fields {
            field_names_from_tool_fields(&variant.fields, &variant.required_fields)
        } else {
            Vec::new()
        },
        fields: if include_fields {
            compact_tool_fields(&variant.fields)
        } else {
            Vec::new()
        },
    }
}

fn compact_tool_fields(fields: &[prism_js::ToolFieldSchemaView]) -> Vec<ShapeFieldView> {
    fields
        .iter()
        .map(|field| ShapeFieldView {
            name: field.name.clone(),
            required: field.required,
            description: None,
            types: field.types.clone(),
            enum_values: field.enum_values.clone(),
            nested_fields: Vec::new(),
        })
        .collect()
}

fn field_names(fields: &[ShapeFieldView], required_fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .filter(|field| {
            !required_fields
                .iter()
                .any(|required| required == &field.name)
        })
        .map(|field| field.name.clone())
        .collect()
}

fn field_names_from_tool_fields(
    fields: &[prism_js::ToolFieldSchemaView],
    required_fields: &[String],
) -> Vec<String> {
    fields
        .iter()
        .filter(|field| {
            !required_fields
                .iter()
                .any(|required| required == &field.name)
        })
        .map(|field| field.name.clone())
        .collect()
}

fn compact_shape_fields(root_schema: &Value) -> Vec<ShapeFieldView> {
    let required_fields = schema_required_fields(root_schema);
    root_schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.iter())
        .map(|(name, schema)| ShapeFieldView {
            name: name.to_string(),
            required: required_fields.iter().any(|field| field == name),
            description: None,
            types: schema_type_labels(root_schema, resolve_schema_ref(root_schema, schema)),
            enum_values: schema_enum_values(root_schema, resolve_schema_ref(root_schema, schema)),
            nested_fields: Vec::new(),
        })
        .collect()
}

fn schema_required_fields(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
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
    schema
        .get("type")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .chain(
            schema
                .get("anyOf")
                .and_then(Value::as_array)
                .into_iter()
                .flat_map(|variants| variants.iter())
                .flat_map(|variant| {
                    schema_type_labels(root_schema, resolve_schema_ref(root_schema, variant))
                }),
        )
        .collect()
}

fn schema_enum_values(root_schema: &Value, schema: &Value) -> Vec<String> {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .chain(
            schema
                .get("const")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        )
        .chain(
            schema
                .get("anyOf")
                .and_then(Value::as_array)
                .into_iter()
                .flat_map(|variants| variants.iter())
                .flat_map(|variant| {
                    schema_enum_values(root_schema, resolve_schema_ref(root_schema, variant))
                }),
        )
        .collect()
}

fn resource_schema_shape_source(resource_kind: &str) -> Option<(Value, &'static str)> {
    match resource_kind {
        "capabilities" => Some((
            schema_resource_value::<CapabilitiesResourcePayload>(
                &schema_resource_uri("capabilities"),
                "PRISM Capabilities Resource Schema",
                "JSON Schema for the canonical PRISM capabilities resource payload.",
                "capabilities",
            ),
            "Compact shape summary for the canonical PRISM capabilities resource payload.",
        )),
        "session" => Some((
            schema_resource_value::<SessionResourcePayload>(
                &schema_resource_uri("session"),
                "PRISM Session Resource Schema",
                "JSON Schema for the PRISM session resource payload.",
                "session",
            ),
            "Compact shape summary for the PRISM session resource payload.",
        )),
        "protected-state" => Some((
            schema_resource_value::<ProtectedStateResourcePayload>(
                &schema_resource_uri("protected-state"),
                "PRISM Protected State Resource Schema",
                "JSON Schema for protected .prism stream verification status, trust diagnostics, and repair guidance.",
                "protected-state",
            ),
            "Compact shape summary for the protected-state resource payload.",
        )),
        "vocab" => Some((
            schema_resource_value::<VocabularyResourcePayload>(
                &schema_resource_uri("vocab"),
                "PRISM Vocabulary Resource Schema",
                "JSON Schema for the canonical PRISM vocabulary resource payload.",
                "vocab",
            ),
            "Compact shape summary for the vocabulary resource payload.",
        )),
        "plans" => Some((
            schema_resource_value::<PlansResourcePayload>(
                &schema_resource_uri("plans"),
                "PRISM Plans Resource Schema",
                "JSON Schema for the PRISM plans discovery resource payload.",
                "plans",
            ),
            "Compact shape summary for the plans discovery resource payload.",
        )),
        "plan" => Some((
            schema_resource_value::<PlanResourcePayload>(
                &schema_resource_uri("plan"),
                "PRISM Plan Resource Schema",
                "JSON Schema for the PRISM plan detail resource payload.",
                "plan",
            ),
            "Compact shape summary for the plan detail resource payload.",
        )),
        "contracts" => Some((
            schema_resource_value::<ContractsResourcePayload>(
                &schema_resource_uri("contracts"),
                "PRISM Contracts Resource Schema",
                "JSON Schema for the PRISM contracts discovery resource payload.",
                "contracts",
            ),
            "Compact shape summary for the contracts discovery resource payload.",
        )),
        "schemas" => Some((
            schema_resource_value::<crate::ResourceSchemaCatalogPayload>(
                &schema_resource_uri("schemas"),
                "PRISM Resource Schema Catalog Schema",
                "JSON Schema for the PRISM resource schema catalog payload.",
                "schemas",
            ),
            "Compact shape summary for the resource schema catalog payload.",
        )),
        "tool-schemas" => Some((
            schema_resource_value::<crate::ToolSchemaCatalogPayload>(
                &schema_resource_uri("tool-schemas"),
                "PRISM Tool Schema Catalog Schema",
                "JSON Schema for the PRISM MCP tool schema catalog payload.",
                "tool-schemas",
            ),
            "Compact shape summary for the tool schema catalog payload.",
        )),
        "entrypoints" => Some((
            schema_resource_value::<EntrypointsResourcePayload>(
                &schema_resource_uri("entrypoints"),
                "PRISM Entrypoints Resource Schema",
                "JSON Schema for the PRISM entrypoints resource payload.",
                "entrypoints",
            ),
            "Compact shape summary for the entrypoints resource payload.",
        )),
        "search" => Some((
            schema_resource_value::<SearchResourcePayload>(
                &schema_resource_uri("search"),
                "PRISM Search Resource Schema",
                "JSON Schema for the PRISM search resource payload.",
                "search",
            ),
            "Compact shape summary for the search resource payload.",
        )),
        "file" => Some((
            schema_resource_value::<FileResourcePayload>(
                &schema_resource_uri("file"),
                "PRISM File Resource Schema",
                "JSON Schema for read-only workspace file excerpt resources.",
                "file",
            ),
            "Compact shape summary for the file resource payload.",
        )),
        "symbol" => Some((
            schema_resource_value::<SymbolResourcePayload>(
                &schema_resource_uri("symbol"),
                "PRISM Symbol Resource Schema",
                "JSON Schema for the PRISM symbol resource payload.",
                "symbol",
            ),
            "Compact shape summary for the symbol resource payload.",
        )),
        "lineage" => Some((
            schema_resource_value::<LineageResourcePayload>(
                &schema_resource_uri("lineage"),
                "PRISM Lineage Resource Schema",
                "JSON Schema for the PRISM lineage resource payload.",
                "lineage",
            ),
            "Compact shape summary for the lineage resource payload.",
        )),
        "task" => Some((
            schema_resource_value::<TaskResourcePayload>(
                &schema_resource_uri("task"),
                "PRISM Task Resource Schema",
                "JSON Schema for the PRISM task replay resource payload.",
                "task",
            ),
            "Compact shape summary for the task replay resource payload.",
        )),
        "event" => Some((
            schema_resource_value::<EventResourcePayload>(
                &schema_resource_uri("event"),
                "PRISM Event Resource Schema",
                "JSON Schema for the PRISM event resource payload.",
                "event",
            ),
            "Compact shape summary for the event resource payload.",
        )),
        "memory" => Some((
            schema_resource_value::<MemoryResourcePayload>(
                &schema_resource_uri("memory"),
                "PRISM Memory Resource Schema",
                "JSON Schema for the PRISM memory resource payload.",
                "memory",
            ),
            "Compact shape summary for the memory resource payload.",
        )),
        "edge" => Some((
            schema_resource_value::<EdgeResourcePayload>(
                &schema_resource_uri("edge"),
                "PRISM Inferred Edge Resource Schema",
                "JSON Schema for the PRISM inferred-edge resource payload.",
                "edge",
            ),
            "Compact shape summary for the inferred-edge resource payload.",
        )),
        "tool-shape" => Some((
            schema_resource_value::<ToolShapeResourcePayload>(
                &schema_resource_uri("tool-shape"),
                "PRISM Tool Shape Resource Schema",
                "JSON Schema for compact tool shape resources.",
                "tool-shape",
            ),
            "Compact shape summary for the tool-shape companion resource payload.",
        )),
        "tool-example" => Some((
            schema_resource_value::<ToolExampleResourcePayload>(
                &schema_resource_uri("tool-example"),
                "PRISM Tool Example Resource Schema",
                "JSON Schema for tool example companion resources.",
                "tool-example",
            ),
            "Compact shape summary for the tool-example companion resource payload.",
        )),
        "resource-shape" => Some((
            schema_resource_value::<ResourceShapeResourcePayload>(
                &schema_resource_uri("resource-shape"),
                "PRISM Resource Shape Resource Schema",
                "JSON Schema for compact resource shape companion resources.",
                "resource-shape",
            ),
            "Compact shape summary for the resource-shape companion resource payload.",
        )),
        "resource-example" => Some((
            schema_resource_value::<ResourceExampleResourcePayload>(
                &schema_resource_uri("resource-example"),
                "PRISM Resource Example Resource Schema",
                "JSON Schema for resource example companion resources.",
                "resource-example",
            ),
            "Compact shape summary for the resource-example companion resource payload.",
        )),
        "capabilities-section" => Some((
            schema_resource_value::<CapabilitiesSectionResourcePayload>(
                &schema_resource_uri("capabilities-section"),
                "PRISM Capabilities Section Resource Schema",
                "JSON Schema for segmented PRISM capabilities sections.",
                "capabilities-section",
            ),
            "Compact shape summary for capabilities-section resource payloads.",
        )),
        "vocab-entry" => Some((
            schema_resource_value::<VocabularyEntryResourcePayload>(
                &schema_resource_uri("vocab-entry"),
                "PRISM Vocabulary Entry Resource Schema",
                "JSON Schema for segmented vocabulary entry resources.",
                "vocab-entry",
            ),
            "Compact shape summary for vocab-entry resource payloads.",
        )),
        "self-description-audit" => Some((
            schema_resource_value::<SelfDescriptionAuditPayload>(
                &schema_resource_uri("self-description-audit"),
                "PRISM Self-Description Audit Resource Schema",
                "JSON Schema for the self-description audit resource.",
                "self-description-audit",
            ),
            "Compact shape summary for self-description audit resource payloads.",
        )),
        _ => None,
    }
}

fn parse_nested_tool_resource_uri(
    uri: &str,
    prefix: &str,
    delimiter: &str,
) -> Option<(String, String)> {
    let (base, _) = split_resource_uri(uri);
    let rest = base.strip_prefix(prefix)?;
    let (tool_name, action) = rest.split_once(delimiter)?;
    let tool_name = crate::percent_decode_lossy(tool_name);
    let action = crate::percent_decode_lossy(action);
    if tool_name.trim().is_empty() || action.trim().is_empty() || action.contains("/variant/") {
        return None;
    }
    Some((tool_name, action))
}

fn parse_nested_tool_variant_resource_uri(
    uri: &str,
    prefix: &str,
) -> Option<(String, String, String)> {
    let (base, _) = split_resource_uri(uri);
    let rest = base.strip_prefix(prefix)?;
    let (tool_name, tail) = rest.split_once("/action/")?;
    let (action, tag) = tail.split_once("/variant/")?;
    let tool_name = crate::percent_decode_lossy(tool_name);
    let action = crate::percent_decode_lossy(action);
    let tag = crate::percent_decode_lossy(tag);
    if tool_name.trim().is_empty() || action.trim().is_empty() || tag.trim().is_empty() {
        return None;
    }
    Some((tool_name, action, tag))
}

fn value_bytes(value: &Value) -> Option<usize> {
    serde_json::to_vec_pretty(value)
        .ok()
        .map(|bytes| bytes.len())
}

fn compact_examples(mut examples: Vec<Value>) -> Vec<Value> {
    if examples.len() > MAX_COMPACT_EXAMPLES_PER_SURFACE {
        examples.truncate(MAX_COMPACT_EXAMPLES_PER_SURFACE);
    }
    examples
}

fn compact_surface_bytes(uri: &str) -> Option<usize> {
    if let Some((tool_name, action, tag)) = parse_tool_variant_example_resource_uri(uri) {
        return tool_example_payload(&tool_name, Some(&action), Some(&tag), uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some((tool_name, action)) = parse_tool_action_example_resource_uri(uri) {
        return tool_example_payload(&tool_name, Some(&action), None, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some(tool_name) = parse_tool_example_resource_uri(uri) {
        return tool_example_payload(&tool_name, None, None, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some((tool_name, action, tag)) = parse_tool_variant_shape_resource_uri(uri) {
        return tool_variant_shape_payload(&tool_name, &action, &tag, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some((tool_name, action)) = parse_tool_action_shape_resource_uri(uri) {
        return tool_action_shape_payload(&tool_name, &action, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some(tool_name) = parse_tool_shape_resource_uri(uri) {
        return tool_shape_payload(&tool_name, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some(resource_kind) = parse_resource_example_resource_uri(uri) {
        return resource_example_payload(&resource_kind, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some(resource_kind) = parse_resource_shape_resource_uri(uri) {
        return resource_shape_payload(&resource_kind, uri)
            .and_then(|payload| value_bytes(&serde_json::to_value(payload).ok()?));
    }
    if let Some((tool_name, action, tag)) = parse_tool_variant_recipe_resource_uri(uri) {
        return tool_recipe_markdown(&tool_name, &action, Some(&tag))
            .map(|markdown| markdown.len());
    }
    if let Some((tool_name, action)) = parse_tool_action_recipe_resource_uri(uri) {
        return tool_recipe_markdown(&tool_name, &action, None).map(|markdown| markdown.len());
    }
    None
}

fn schema_uri_to_bytes(uri: &str) -> Option<usize> {
    if let Some((tool_name, action, tag)) = parse_tool_variant_schema_resource_uri(uri) {
        return tool_variant_schema_value(&tool_name, &action, &tag)
            .as_ref()
            .and_then(value_bytes);
    }
    if let Some((tool_name, action)) = crate::parse_tool_action_schema_resource_uri(uri) {
        return tool_action_schema_value(&tool_name, &action)
            .as_ref()
            .and_then(value_bytes);
    }
    if let Some(tool_name) = crate::parse_tool_schema_resource_uri(uri) {
        return crate::tool_input_schema_value(&tool_name)
            .as_ref()
            .and_then(value_bytes);
    }
    if let Some(resource_kind) = crate::parse_schema_resource_uri(uri) {
        return resource_schema_shape_source(&resource_kind)
            .as_ref()
            .and_then(|(schema, _)| value_bytes(schema));
    }
    None
}

fn tool_example_validation(
    tool_name: &str,
    action: Option<&str>,
    variant: Option<&str>,
) -> Option<prism_js::ToolInputValidationView> {
    let example = tool_example_input(tool_name, action, variant)?;
    Some(crate::tool_args::validate_tool_input_value(
        tool_name, example,
    ))
}

fn tool_example_input(
    tool_name: &str,
    action: Option<&str>,
    variant: Option<&str>,
) -> Option<Value> {
    match (action, variant) {
        (None, None) => {
            crate::tool_input_examples(tool_name).and_then(|examples| examples.into_iter().next())
        }
        (Some(action), None) => crate::tool_action_example(tool_name, action),
        (Some(action), Some(tag)) => {
            let action_view = tool_action_schema_view(tool_name, action)?;
            let discriminator = action_view.payload_discriminator?;
            crate::tool_action_examples(tool_name, action)
                .into_iter()
                .find(|example| {
                    example
                        .get("input")
                        .and_then(|input| input.get(&discriminator))
                        .and_then(Value::as_str)
                        == Some(tag)
                })
        }
        (None, Some(_)) => None,
    }
}

fn example_validation_issue_codes(
    validation: Option<&prism_js::ToolInputValidationView>,
) -> Vec<String> {
    validation
        .map(|validation| {
            validation
                .issues
                .iter()
                .map(|issue| issue.code.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn audit_issues(
    has_example: bool,
    has_shape: bool,
    expects_recipe: bool,
    schema_bytes: Option<usize>,
    example_bytes: Option<usize>,
    shape_bytes: Option<usize>,
    recipe_bytes: Option<usize>,
    example_valid: Option<bool>,
) -> Vec<String> {
    let mut issues = Vec::new();
    if !has_example {
        issues.push("missing_example".to_string());
    }
    if !has_shape {
        issues.push("missing_shape".to_string());
    }
    if expects_recipe && recipe_bytes.is_none() {
        issues.push("missing_recipe".to_string());
    }
    if schema_bytes.is_some_and(|bytes| bytes > SELF_DESCRIPTION_BUDGET_BYTES) {
        issues.push("schema_oversize".to_string());
    }
    if example_bytes.is_some_and(|bytes| bytes > SELF_DESCRIPTION_BUDGET_BYTES) {
        issues.push("example_oversize".to_string());
    }
    if shape_bytes.is_some_and(|bytes| bytes > SELF_DESCRIPTION_BUDGET_BYTES) {
        issues.push("shape_oversize".to_string());
    }
    if recipe_bytes.is_some_and(|bytes| bytes > SELF_DESCRIPTION_BUDGET_BYTES) {
        issues.push("recipe_oversize".to_string());
    }
    if example_valid == Some(false) {
        issues.push("example_invalid".to_string());
    }
    issues
}

fn source_free_operable(
    has_example: bool,
    has_shape: bool,
    expects_recipe: bool,
    example_bytes: Option<usize>,
    shape_bytes: Option<usize>,
    recipe_bytes: Option<usize>,
    example_valid: Option<bool>,
) -> bool {
    has_example
        && has_shape
        && (!expects_recipe || recipe_bytes.is_some())
        && example_bytes.is_some_and(|bytes| bytes <= SELF_DESCRIPTION_BUDGET_BYTES)
        && shape_bytes.is_some_and(|bytes| bytes <= SELF_DESCRIPTION_BUDGET_BYTES)
        && recipe_bytes
            .map(|bytes| bytes <= SELF_DESCRIPTION_BUDGET_BYTES)
            .unwrap_or(!expects_recipe)
        && example_valid.unwrap_or(true)
}

fn resource_name_to_kind(resource: &crate::ResourceCapabilityView) -> &str {
    match resource.uri.as_str() {
        uri if uri == crate::INSTRUCTIONS_URI => "instructions",
        uri if uri == crate::CAPABILITIES_URI => "capabilities",
        uri if uri == crate::SESSION_URI => "session",
        uri if uri == crate::PROTECTED_STATE_URI => "protected-state",
        uri if uri == crate::VOCAB_URI => "vocab",
        uri if uri == crate::PLANS_URI => "plans",
        uri if uri == crate::CONTRACTS_URI => "contracts",
        uri if uri == crate::ENTRYPOINTS_URI => "entrypoints",
        uri if uri == crate::SCHEMAS_URI => "schemas",
        uri if uri == crate::TOOL_SCHEMAS_URI => "tool-schemas",
        uri if uri == crate::SELF_DESCRIPTION_AUDIT_URI => "self-description-audit",
        _ => "unknown",
    }
}

fn internal_serialize_error(err: serde_json::Error) -> McpError {
    McpError::internal_error(
        "failed to serialize self-description payload",
        Some(json!({ "error": err.to_string() })),
    )
}

fn tool_recipe_markdown(tool_name: &str, action: &str, variant: Option<&str>) -> Option<String> {
    let common = format!(
        "# PRISM Recipe: {tool_name}.{action}\n\nUse the compact path first:\n1. Read the shape resource.\n2. Read the example resource.\n3. Draft the payload.\n4. Call `validateToolInput` or the target tool.\n"
    );
    match (tool_name, action, variant) {
        ("prism_mutate", "coordination", Some("plan_bootstrap")) => Some(format!(
            "{common}\nUse `plan_bootstrap` when creating a plan from scratch in one authoritative mutation.\n\nMinimum flow:\n- set `plan.title` and `plan.goal`\n- add stable client ids for every task and node\n- express ordering with `dependsOn`\n- use a validation node only for terminal validation work\n- prefer the variant schema and shape resources over the full coordination union schema\n"
        )),
        ("prism_mutate", "coordination", Some("update")) => Some(format!(
            "{common}\nUse `update` to change one existing coordination task or plan node by durable id.\n\nPrefer this path for status transitions, title/summary updates, and graph-safe follow-up edits after bootstrap.\n"
        )),
        ("prism_mutate", "claim", Some("acquire")) => Some(format!(
            "{common}\nAcquire a claim only after identifying the exact anchors you need. Prefer the narrowest capability and shortest reasonable lease.\n"
        )),
        ("prism_mutate", "artifact", Some("review")) => Some(format!(
            "{common}\nArtifact review should include a verdict, a concise summary, and any validated checks that justify approval or requested changes.\n"
        )),
        ("prism_mutate", "validation_feedback", None) => Some(format!(
            "{common}\nRecord validation feedback whenever PRISM is materially wrong, stale, noisy, or unusually helpful during live repo work.\n"
        )),
        ("prism_mutate", _, Some(_)) | ("prism_mutate", _, None) => Some(common),
        _ => Some(common),
    }
}
