use prism_js::{
    ConceptPacketView, ConceptRelationView, ToolActionSchemaView, ToolInputValidationView,
    ToolValidationIssueView,
};
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;

use crate::{
    tool_schema_resource_uri, tool_schema_view, vocabulary_error, ContractPacketView, SessionView,
};

fn ensure_root_object_input_schema(schema: &mut Schema) {
    if schema.get("type").is_none() {
        schema.insert("type".to_string(), Value::String("object".to_string()));
    }
}

fn parse_tagged_tool_input<T>(tool_name: &str, value: Value) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let validation = validate_tool_input_value(tool_name, value);
    if !validation.valid {
        return Err(format_tool_validation_error(&validation));
    }
    serde_json::from_value(validation.normalized_input).map_err(|error| {
        format!(
            "invalid {tool_name} input: {}. Inspect via prism.tool(\"{tool_name}\") or prism.validateToolInput(\"{tool_name}\", <input>).",
            error
        )
    })
}

macro_rules! impl_schema_from_wire {
    ($target:ty, $wire:ty, $name:literal) => {
        impl JsonSchema for $target {
            fn inline_schema() -> bool {
                <$wire>::inline_schema()
            }

            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }

            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                <$wire>::json_schema(generator)
            }
        }
    };
}

pub(crate) fn is_flat_tagged_tool_input(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("action") && !object.contains_key("input") && object.len() > 1
    })
}

fn preserved_tagged_input_keys(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        "prism_mutate" => &["action", "credential"],
        _ => &["action"],
    }
}

pub(crate) fn normalize_tagged_tool_input(tool_name: &str, mut value: Value) -> Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if !is_flat_tagged_tool_input(&Value::Object(object.clone())) {
        return value;
    }

    let preserved_keys = preserved_tagged_input_keys(tool_name);
    let mut input = serde_json::Map::new();
    let keys = object
        .keys()
        .filter(|key| !preserved_keys.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(field) = object.remove(&key) {
            input.insert(key, field);
        }
    }
    if !input.is_empty() {
        object.insert("input".to_string(), Value::Object(input));
    }
    value
}

pub(crate) fn validate_tool_input_value(tool_name: &str, value: Value) -> ToolInputValidationView {
    let Some(tool) = tool_schema_view(tool_name) else {
        let summary = format!("Unknown PRISM MCP tool `{tool_name}`.");
        return ToolInputValidationView {
            tool_name: tool_name.to_string(),
            schema_uri: tool_schema_resource_uri(tool_name),
            valid: false,
            normalized_input: value,
            action: None,
            action_schema_uri: None,
            summary: summary.clone(),
            issues: vec![ToolValidationIssueView {
                code: "unknown_tool".to_string(),
                path: None,
                summary,
                allowed_values: Vec::new(),
                required_fields: Vec::new(),
            }],
            example_inputs: Vec::new(),
        };
    };

    let normalized_input = if tool.actions.is_empty() {
        value
    } else {
        normalize_tagged_tool_input(tool_name, value)
    };
    let action = normalized_input
        .get("action")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let action_schema = action
        .as_deref()
        .and_then(|name| {
            tool.actions
                .iter()
                .find(|candidate| candidate.action == name)
        })
        .cloned();

    if !tool.actions.is_empty() {
        let valid_actions = tool
            .actions
            .iter()
            .map(|candidate| candidate.action.clone())
            .collect::<Vec<_>>();
        match action.as_deref() {
            None => {
                let summary = format!(
                    "{tool_name} requires `action`; valid actions: {}.",
                    valid_actions.join(", ")
                );
                return invalid_tool_validation(
                    &tool,
                    normalized_input,
                    None,
                    None,
                    ToolValidationIssueView {
                        code: "missing_action".to_string(),
                        path: Some("action".to_string()),
                        summary,
                        allowed_values: valid_actions,
                        required_fields: Vec::new(),
                    },
                );
            }
            Some(action_name)
                if !tool
                    .actions
                    .iter()
                    .any(|candidate| candidate.action == action_name) =>
            {
                let summary = format!(
                    "unknown {tool_name} action `{action_name}`; valid actions: {}.",
                    valid_actions.join(", ")
                );
                return invalid_tool_validation(
                    &tool,
                    normalized_input,
                    Some(action_name.to_string()),
                    None,
                    ToolValidationIssueView {
                        code: "invalid_action".to_string(),
                        path: Some("action".to_string()),
                        summary,
                        allowed_values: valid_actions,
                        required_fields: Vec::new(),
                    },
                );
            }
            _ => {}
        }
    }

    match validate_tool_value_against_schema(
        tool_name,
        normalized_input.clone(),
        &tool,
        action_schema.as_ref(),
    ) {
        Ok(()) => ToolInputValidationView {
            tool_name: tool.tool_name.clone(),
            schema_uri: tool.schema_uri.clone(),
            valid: true,
            normalized_input,
            action: action.clone(),
            action_schema_uri: action_schema
                .as_ref()
                .map(|schema| schema.schema_uri.clone()),
            summary: action
                .as_deref()
                .map(|action_name| {
                    format!("Input is valid for `{tool_name}` action `{action_name}`.")
                })
                .unwrap_or_else(|| format!("Input is valid for `{tool_name}`.")),
            issues: Vec::new(),
            example_inputs: action_examples(&tool, action_schema.as_ref()),
        },
        Err(issue) => invalid_tool_validation(
            &tool,
            normalized_input,
            action,
            action_schema.as_ref(),
            issue,
        ),
    }
}

fn deserialize_optional_nonempty_enum<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let Some(value) = Option::<Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if value.as_str().is_some_and(|raw| raw.trim().is_empty()) {
        return Ok(None);
    }
    T::deserialize(value).map(Some).map_err(de::Error::custom)
}

fn invalid_tool_validation(
    tool: &prism_js::ToolSchemaView,
    normalized_input: Value,
    action: Option<String>,
    action_schema: Option<&ToolActionSchemaView>,
    issue: ToolValidationIssueView,
) -> ToolInputValidationView {
    let summary = if issue.code == "missing_required_field" {
        if let Some(action_name) = action.as_deref() {
            format!(
                "{} action `{}` is missing required field `{}`; required fields: {}.",
                tool.tool_name,
                action_name,
                issue.path.clone().unwrap_or_else(|| "input".to_string()),
                issue.required_fields.join(", ")
            )
        } else {
            format!(
                "{} is missing required field `{}`; required fields: {}.",
                tool.tool_name,
                issue.path.clone().unwrap_or_else(|| "input".to_string()),
                issue.required_fields.join(", ")
            )
        }
    } else if let Some(action_name) = action.as_deref() {
        format!(
            "invalid {} action `{}` input: {}",
            tool.tool_name, action_name, issue.summary
        )
    } else {
        format!("invalid {} input: {}", tool.tool_name, issue.summary)
    };

    ToolInputValidationView {
        tool_name: tool.tool_name.clone(),
        schema_uri: tool.schema_uri.clone(),
        valid: false,
        normalized_input,
        action,
        action_schema_uri: action_schema.map(|schema| schema.schema_uri.clone()),
        summary,
        issues: vec![issue],
        example_inputs: action_examples(tool, action_schema),
    }
}

fn action_examples(
    tool: &prism_js::ToolSchemaView,
    action_schema: Option<&ToolActionSchemaView>,
) -> Vec<Value> {
    action_schema.map_or_else(
        || tool.example_inputs.clone(),
        |schema| {
            if !schema.example_inputs.is_empty() {
                schema.example_inputs.clone()
            } else {
                schema
                    .example_input
                    .clone()
                    .map(|value| vec![value])
                    .unwrap_or_default()
            }
        },
    )
}

fn validate_tool_value_against_schema(
    tool_name: &str,
    value: Value,
    tool: &prism_js::ToolSchemaView,
    action_schema: Option<&ToolActionSchemaView>,
) -> Result<(), ToolValidationIssueView> {
    let required_fields = action_schema
        .map(|schema| schema.required_fields.clone())
        .unwrap_or_else(|| root_required_fields(tool));
    match tool_name {
        "prism_locate" => {
            deserialize_or_issue::<PrismLocateArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_gather" => {
            deserialize_or_issue::<PrismGatherArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_open" => {
            deserialize_or_issue::<PrismOpenArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_workset" => {
            deserialize_or_issue::<PrismWorksetArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_expand" => {
            deserialize_or_issue::<PrismExpandArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_task_brief" => {
            deserialize_or_issue::<PrismTaskBriefArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_concept" => {
            deserialize_or_issue::<PrismConceptArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_query" => {
            deserialize_or_issue::<PrismQueryArgs>(value, None, &required_fields).map(|_| ())
        }
        "prism_mutate" => validate_prism_mutate_input(value, &required_fields),
        _ => Ok(()),
    }
}

fn validate_prism_mutate_input(
    value: Value,
    required_fields: &[String],
) -> Result<(), ToolValidationIssueView> {
    let root_required = tool_schema_view("prism_mutate")
        .map(|tool| root_required_fields(&tool))
        .unwrap_or_default();
    deserialize_or_issue::<PrismMutationArgsWire>(value.clone(), None, &root_required)?;
    let action = value.get("action").and_then(Value::as_str);
    let input = value.get("input").cloned().unwrap_or(Value::Null);
    match action {
        Some("memory") => deserialize_or_issue::<PrismMemoryArgsValidationWire>(
            input,
            Some("input"),
            required_fields,
        )
        .and_then(validate_memory_payload),
        Some("coordination") => deserialize_or_issue::<PrismCoordinationArgsValidationWire>(
            input,
            Some("input"),
            required_fields,
        )
        .and_then(validate_coordination_payload),
        Some("claim") => deserialize_or_issue::<PrismClaimArgsValidationWire>(
            input,
            Some("input"),
            required_fields,
        )
        .and_then(validate_claim_payload),
        Some("artifact") => deserialize_or_issue::<PrismArtifactArgsValidationWire>(
            input,
            Some("input"),
            required_fields,
        )
        .and_then(validate_artifact_payload),
        Some("heartbeat_lease") => deserialize_or_issue::<PrismHeartbeatLeaseArgsValidationWire>(
            input,
            Some("input"),
            required_fields,
        )
        .and_then(validate_heartbeat_lease_payload),
        _ => deserialize_or_issue::<PrismMutationArgsWire>(value, Some("input"), required_fields)
            .map(|_| ()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismMemoryArgsValidationWire {
    action: MemoryMutationActionInput,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismCoordinationArgsValidationWire {
    kind: CoordinationMutationKindInput,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismClaimArgsValidationWire {
    action: ClaimActionInput,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismArtifactArgsValidationWire {
    action: ArtifactActionInput,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismHeartbeatLeaseArgsValidationWire {
    task_id: Option<String>,
    claim_id: Option<String>,
}

fn validate_memory_payload(
    args: PrismMemoryArgsValidationWire,
) -> Result<(), ToolValidationIssueView> {
    let tag = match args.action {
        MemoryMutationActionInput::Store => "store",
        MemoryMutationActionInput::Retire => "retire",
    };
    let required_fields = payload_required_fields("prism_mutate", "memory", tag);
    match args.action {
        MemoryMutationActionInput::Store => deserialize_or_issue::<MemoryStorePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        MemoryMutationActionInput::Retire => deserialize_or_issue::<MemoryRetirePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
    }
}

fn validate_coordination_payload(
    args: PrismCoordinationArgsValidationWire,
) -> Result<(), ToolValidationIssueView> {
    let tag = coordination_kind_tag(&args.kind);
    let required_fields = payload_required_fields("prism_mutate", "coordination", tag);
    match args.kind {
        CoordinationMutationKindInput::PlanCreate => deserialize_or_issue::<PlanCreatePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::PlanUpdate => deserialize_or_issue::<PlanUpdatePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::PlanArchive => deserialize_or_issue::<PlanArchivePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::TaskCreate => deserialize_or_issue::<TaskCreatePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::Update => deserialize_or_issue::<WorkflowUpdatePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::PlanNodeCreate => {
            deserialize_or_issue::<PlanNodeCreatePayload>(
                args.payload,
                Some("input.payload"),
                &required_fields,
            )
            .map(|_| ())
        }
        CoordinationMutationKindInput::PlanEdgeCreate => {
            deserialize_or_issue::<PlanEdgeCreatePayload>(
                args.payload,
                Some("input.payload"),
                &required_fields,
            )
            .map(|_| ())
        }
        CoordinationMutationKindInput::PlanEdgeDelete => {
            deserialize_or_issue::<PlanEdgeDeletePayload>(
                args.payload,
                Some("input.payload"),
                &required_fields,
            )
            .map(|_| ())
        }
        CoordinationMutationKindInput::Handoff => deserialize_or_issue::<HandoffPayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::Resume => deserialize_or_issue::<TaskResumePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::Reclaim => deserialize_or_issue::<TaskReclaimPayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        CoordinationMutationKindInput::HandoffAccept => {
            deserialize_or_issue::<HandoffAcceptPayload>(
                args.payload,
                Some("input.payload"),
                &required_fields,
            )
            .map(|_| ())
        }
    }
}

fn validate_claim_payload(
    args: PrismClaimArgsValidationWire,
) -> Result<(), ToolValidationIssueView> {
    let tag = claim_action_tag(&args.action);
    let required_fields = payload_required_fields("prism_mutate", "claim", tag);
    match args.action {
        ClaimActionInput::Acquire => deserialize_or_issue::<ClaimAcquirePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        ClaimActionInput::Renew => deserialize_or_issue::<ClaimRenewPayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        ClaimActionInput::Release => deserialize_or_issue::<ClaimReleasePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
    }
}

fn validate_artifact_payload(
    args: PrismArtifactArgsValidationWire,
) -> Result<(), ToolValidationIssueView> {
    let tag = artifact_action_tag(&args.action);
    let required_fields = payload_required_fields("prism_mutate", "artifact", tag);
    match args.action {
        ArtifactActionInput::Propose => deserialize_or_issue::<ArtifactProposePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        ArtifactActionInput::Supersede => deserialize_or_issue::<ArtifactSupersedePayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
        ArtifactActionInput::Review => deserialize_or_issue::<ArtifactReviewPayload>(
            args.payload,
            Some("input.payload"),
            &required_fields,
        )
        .map(|_| ()),
    }
}

fn validate_heartbeat_lease_payload(
    args: PrismHeartbeatLeaseArgsValidationWire,
) -> Result<(), ToolValidationIssueView> {
    let target_count = usize::from(args.task_id.is_some()) + usize::from(args.claim_id.is_some());
    if target_count == 1 {
        return Ok(());
    }
    Err(ToolValidationIssueView {
        code: "invalid_input".to_string(),
        path: Some("input".to_string()),
        summary: "Provide exactly one of `input.taskId` or `input.claimId`.".to_string(),
        allowed_values: Vec::new(),
        required_fields: vec!["taskId | claimId".to_string()],
    })
}

fn deserialize_or_issue<T>(
    value: Value,
    path_prefix: Option<&str>,
    required_fields: &[String],
) -> Result<T, ToolValidationIssueView>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| {
        validation_issue_from_error(&error.to_string(), path_prefix, required_fields)
    })
}

fn validation_issue_from_error(
    parse_error: &str,
    path_prefix: Option<&str>,
    required_fields: &[String],
) -> ToolValidationIssueView {
    if let Some(field) = missing_field_name(parse_error) {
        return ToolValidationIssueView {
            code: "missing_required_field".to_string(),
            path: Some(prefixed_field_path(path_prefix, field)),
            summary: format!(
                "Missing required field `{}`.",
                prefixed_field_path(path_prefix, field)
            ),
            allowed_values: Vec::new(),
            required_fields: required_fields.to_vec(),
        };
    }
    ToolValidationIssueView {
        code: if parse_error.contains("Allowed values:") {
            "invalid_value".to_string()
        } else {
            "invalid_input".to_string()
        },
        path: path_prefix.map(ToString::to_string),
        summary: parse_error.to_string(),
        allowed_values: Vec::new(),
        required_fields: required_fields.to_vec(),
    }
}

fn prefixed_field_path(path_prefix: Option<&str>, field: &str) -> String {
    path_prefix
        .map(|prefix| format!("{prefix}.{field}"))
        .unwrap_or_else(|| field.to_string())
}

fn root_required_fields(tool: &prism_js::ToolSchemaView) -> Vec<String> {
    tool.input_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

fn payload_required_fields(tool_name: &str, action: &str, tag: &str) -> Vec<String> {
    tool_schema_view(tool_name)
        .and_then(|tool| {
            tool.actions
                .into_iter()
                .find(|candidate| candidate.action == action)
        })
        .and_then(|schema| {
            schema
                .payload_variants
                .into_iter()
                .find(|variant| variant.tag == tag)
        })
        .map(|variant| variant.required_fields)
        .unwrap_or_default()
}

fn coordination_kind_tag(kind: &CoordinationMutationKindInput) -> &'static str {
    match kind {
        CoordinationMutationKindInput::PlanCreate => "plan_create",
        CoordinationMutationKindInput::PlanUpdate => "plan_update",
        CoordinationMutationKindInput::PlanArchive => "plan_archive",
        CoordinationMutationKindInput::TaskCreate => "task_create",
        CoordinationMutationKindInput::Update => "update",
        CoordinationMutationKindInput::PlanNodeCreate => "plan_node_create",
        CoordinationMutationKindInput::PlanEdgeCreate => "plan_edge_create",
        CoordinationMutationKindInput::PlanEdgeDelete => "plan_edge_delete",
        CoordinationMutationKindInput::Handoff => "handoff",
        CoordinationMutationKindInput::Resume => "resume",
        CoordinationMutationKindInput::Reclaim => "reclaim",
        CoordinationMutationKindInput::HandoffAccept => "handoff_accept",
    }
}

fn claim_action_tag(action: &ClaimActionInput) -> &'static str {
    match action {
        ClaimActionInput::Acquire => "acquire",
        ClaimActionInput::Renew => "renew",
        ClaimActionInput::Release => "release",
    }
}

fn artifact_action_tag(action: &ArtifactActionInput) -> &'static str {
    match action {
        ArtifactActionInput::Propose => "propose",
        ArtifactActionInput::Supersede => "supersede",
        ArtifactActionInput::Review => "review",
    }
}

fn format_tool_validation_error(validation: &ToolInputValidationView) -> String {
    let inspect_hint = match (
        validation.action.as_deref(),
        validation.action_schema_uri.as_deref(),
    ) {
        (Some(action), Some(action_schema_uri)) => format!(
            "Inspect via prism.tool(\"{}\")?.actions.find((action) => action.action === \"{}\") or prism.validateToolInput(\"{}\", <input>). Action schema: {}.",
            validation.tool_name, action, validation.tool_name, action_schema_uri
        ),
        _ => format!(
            "Inspect via prism.tool(\"{}\") or prism.validateToolInput(\"{}\", <input>).",
            validation.tool_name, validation.tool_name
        ),
    };
    let example_hint = validation
        .example_inputs
        .first()
        .map(format_inline_example_hint)
        .unwrap_or_default();
    format!("{} {}{}", validation.summary, inspect_hint, example_hint)
}

fn missing_field_name(parse_error: &str) -> Option<&str> {
    let (_, tail) = parse_error.split_once("missing field `")?;
    let (field, _) = tail.split_once('`')?;
    Some(field)
}

fn format_inline_example_hint(example: &Value) -> String {
    let rendered =
        serde_json::to_string(example).unwrap_or_else(|_| "<unserializable example>".to_string());
    let max_chars = 320;
    let compact = if rendered.chars().count() > max_chars {
        let mut truncated = rendered.chars().take(max_chars).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        rendered
    };
    format!(" Minimal valid example: {compact}")
}

fn normalize_vocab_token(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
        .collect()
}

macro_rules! impl_vocab_deserialize {
    ($name:ident, $key:literal, $label:literal, $example:literal, { $($normalized:literal => $variant:ident),+ $(,)? }) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                let normalized = normalize_vocab_token(&raw);
                match normalized.as_str() {
                    $(
                        $normalized => Ok(Self::$variant),
                    )+
                    _ => Err(de::Error::custom(vocabulary_error(
                        $key,
                        $label,
                        raw.trim(),
                        $example,
                    ))),
                }
            }
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum QueryLanguage {
    Ts,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PrismQueryArgs {
    #[schemars(description = "TypeScript snippet evaluated with a global `prism` object.")]
    pub(crate) code: String,
    #[schemars(description = "Query language. Only `ts` is currently supported.")]
    pub(crate) language: Option<QueryLanguage>,
}

#[derive(Debug, Clone, JsonSchema)]
pub(crate) enum PrismLocateTaskIntentInput {
    Inspect,
    Edit,
    Validate,
    Test,
    Explain,
}

impl_vocab_deserialize!(
    PrismLocateTaskIntentInput,
    "taskIntent",
    "task intent",
    r#"{"taskIntent":"edit"}"#,
    {
        "inspect" => Inspect,
        "read" => Inspect,
        "reader" => Inspect,
        "code" => Inspect,
        "implementation" => Inspect,
        "edit" => Edit,
        "write" => Edit,
        "modify" => Edit,
        "change" => Edit,
        "validate" => Validate,
        "validation" => Validate,
        "verify" => Validate,
        "test" => Test,
        "tests" => Test,
        "testing" => Test,
        "explain" => Explain,
        "explanation" => Explain,
        "doc" => Explain,
        "docs" => Explain,
        "document" => Explain,
        "documentation" => Explain,
        "spec" => Explain,
        "specs" => Explain,
        "design" => Explain
    }
);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismLocateArgs {
    #[schemars(description = "Search text for the compact target lookup.")]
    pub(crate) query: String,
    #[schemars(
        description = "Optional file path fragment to narrow compact locate results before ranking, for example `docs/SPEC.md` or `crates/prism-mcp/src/compact_tools.rs`."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Optional glob to narrow compact locate results before ranking, for example `docs/**` or `crates/prism-mcp/src/**`."
    )]
    pub(crate) glob: Option<String>,
    #[schemars(
        description = "Optional task intent that biases ranking toward code, docs, tests, or explanation targets. Accepts aliases such as `code` and `read` for `inspect`, plus docs-oriented aliases such as `docs` and `spec`; when omitted, docs-like `path` or `glob` filters automatically bias toward explanation targets."
    )]
    #[serde(
        alias = "task_intent",
        default,
        deserialize_with = "deserialize_optional_nonempty_enum"
    )]
    pub(crate) task_intent: Option<PrismLocateTaskIntentInput>,
    #[schemars(
        description = "Optional coordination task id that biases ranking and follow-through toward the task's anchors, intent bindings, and related work context."
    )]
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    #[schemars(description = "Optional compact candidate count from 1 to 3.")]
    pub(crate) limit: Option<usize>,
    #[schemars(
        description = "When true, also include one bounded preview for the top-ranked candidate."
    )]
    #[serde(alias = "include_top_preview")]
    pub(crate) include_top_preview: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismGatherArgs {
    #[schemars(description = "Exact text to gather as 1 to 3 bounded slices.")]
    pub(crate) query: String,
    #[schemars(
        description = "Optional file path fragment to narrow exact-text gather results, for example `benchmark_codex.py` or `docs/SPEC.md`."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Optional glob to narrow exact-text gather results, for example `benchmarks/**` or `docs/**`."
    )]
    pub(crate) glob: Option<String>,
    #[schemars(description = "Optional compact slice count from 1 to 3.")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PrismOpenModeInput {
    Focus,
    Edit,
    Raw,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOpenArgs {
    #[schemars(
        description = "Previously located compact handle to open. Exactly one of `handle` or `path` is required."
    )]
    pub(crate) handle: Option<String>,
    #[schemars(
        description = "Exact workspace file path to open directly without first minting a locate handle. Exactly one of `handle` or `path` is required."
    )]
    pub(crate) path: Option<String>,
    #[schemars(
        description = "Open mode: `focus` for a bounded local block, `edit` for an edit-oriented slice, or `raw` for the literal file window covering the target span. Path-based opens support `raw`, and also support `edit` when `line` is provided so PRISM can center an edit-ready window."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) mode: Option<PrismOpenModeInput>,
    #[schemars(
        description = "Optional 1-based focus line for exact-path opens. When present, PRISM returns a bounded window around this line."
    )]
    pub(crate) line: Option<usize>,
    #[schemars(
        description = "Optional context lines before `line` for exact-path opens. Ignored unless `line` is set."
    )]
    #[serde(alias = "before_lines")]
    pub(crate) before_lines: Option<usize>,
    #[schemars(
        description = "Optional context lines after `line` for exact-path opens. Ignored unless `line` is set."
    )]
    #[serde(alias = "after_lines")]
    pub(crate) after_lines: Option<usize>,
    #[schemars(description = "Optional character budget for the returned exact-path slice.")]
    #[serde(alias = "max_chars")]
    pub(crate) max_chars: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismWorksetArgs {
    pub(crate) handle: Option<String>,
    pub(crate) query: Option<String>,
    #[schemars(
        description = "Optional coordination task id that biases workset selection and broad-query resolution toward the task's active work context."
    )]
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismExpandKindInput {
    Diagnostics,
    Lineage,
    Neighbors,
    Diff,
    Health,
    Validation,
    Impact,
    Timeline,
    Memory,
    Drift,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismExpandArgs {
    #[schemars(description = "Opaque handle returned by compact locate/open/workset.")]
    pub(crate) handle: String,
    #[schemars(description = "Requested compact expansion kind.")]
    pub(crate) kind: PrismExpandKindInput,
    #[schemars(
        description = "When true and kind is `neighbors`, also include one bounded preview for the top neighbor."
    )]
    #[serde(alias = "include_top_preview")]
    pub(crate) include_top_preview: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismTaskBriefArgs {
    #[schemars(description = "Coordination task id to summarize through the compact task lens.")]
    #[serde(alias = "task_id")]
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismConceptLensInput {
    Open,
    Workset,
    Validation,
    Timeline,
    Memory,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismConceptVerbosityInput {
    Summary,
    Standard,
    Full,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptArgs {
    #[schemars(description = "Concept handle like `concept://validation_pipeline`.")]
    pub(crate) handle: Option<String>,
    #[schemars(description = "Broad repo noun or phrase to resolve into a concept packet.")]
    pub(crate) query: Option<String>,
    #[schemars(
        description = "Optional coordination task id that biases concept resolution toward task-related provenance and task intent context."
    )]
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    #[schemars(
        description = "Optional decode lens. When provided, also decode the concept into supporting context."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) lens: Option<PrismConceptLensInput>,
    #[schemars(
        description = "Optional concept-packet density. Use `summary` for the lightest packet, `standard` for compact orientation, or `full` for the complete concept payload."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) verbosity: Option<PrismConceptVerbosityInput>,
    #[schemars(
        description = "When true, include lineage-backed binding metadata aligned with the concept member lists."
    )]
    #[serde(alias = "include_binding_metadata")]
    pub(crate) include_binding_metadata: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptMutationOperationInput {
    Promote,
    Update,
    Retire,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContractMutationOperationInput {
    Promote,
    Update,
    Retire,
    AttachEvidence,
    AttachValidation,
    RecordConsumer,
    SetStatus,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContractKindInput {
    Interface,
    Behavioral,
    DataShape,
    DependencyBoundary,
    Lifecycle,
    Protocol,
    Operational,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContractStatusInput {
    Candidate,
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContractStabilityInput {
    Experimental,
    Internal,
    Public,
    Deprecated,
    Migrating,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContractGuaranteeStrengthInput {
    Hard,
    Soft,
    Conditional,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptScopeInput {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptRelationKindInput {
    DependsOn,
    Specializes,
    PartOf,
    ValidatedBy,
    OftenUsedWith,
    Supersedes,
    ConfusedWith,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptRelationMutationOperationInput {
    Upsert,
    Retire,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SparsePatchOpInput {
    Keep,
    Set,
    Clear,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SparsePatchObjectInput<T> {
    pub(crate) op: SparsePatchOpInput,
    pub(crate) value: Option<T>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum SparsePatchInput<T> {
    Value(T),
    Patch(SparsePatchObjectInput<T>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SparsePatch<T> {
    Keep,
    Set(T),
    Clear,
}

impl<T> SparsePatchInput<T> {
    pub(crate) fn into_patch(self, field: &str) -> Result<SparsePatch<T>, String> {
        match self {
            Self::Value(value) => Ok(SparsePatch::Set(value)),
            Self::Patch(SparsePatchObjectInput { op, value }) => match op {
                SparsePatchOpInput::Keep => Ok(SparsePatch::Keep),
                SparsePatchOpInput::Set => value
                    .map(SparsePatch::Set)
                    .ok_or_else(|| format!("`{field}` patch with op `set` requires `value`")),
                SparsePatchOpInput::Clear => Ok(SparsePatch::Clear),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptMutationArgs {
    #[schemars(
        description = "Whether to promote a new repo concept packet or update an existing one."
    )]
    pub(crate) operation: ConceptMutationOperationInput,
    #[schemars(
        description = "Stable concept handle like `concept://validation_pipeline`. Required for `update`. Optional for `promote`; Prism derives one from `canonicalName` when omitted."
    )]
    pub(crate) handle: Option<String>,
    #[schemars(description = "Canonical repo-native concept name. Required for `promote`.")]
    pub(crate) canonical_name: Option<String>,
    #[schemars(description = "Short repo-native summary. Required for `promote`.")]
    pub(crate) summary: Option<String>,
    #[schemars(description = "Common aliases for the concept.")]
    pub(crate) aliases: Option<Vec<String>>,
    #[schemars(
        description = "2 to 5 central member nodes for the concept. Required for `promote`."
    )]
    pub(crate) core_members: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional supporting member nodes.")]
    pub(crate) supporting_members: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional likely test nodes.")]
    pub(crate) likely_tests: Option<Vec<NodeIdInput>>,
    #[schemars(description = "Optional evidence lines explaining why this concept exists.")]
    pub(crate) evidence: Option<Vec<String>>,
    #[schemars(description = "Optional risk hint for the concept packet.")]
    pub(crate) risk_hint: Option<SparsePatchInput<String>>,
    #[schemars(description = "Optional confidence score from 0.0 to 1.0.")]
    pub(crate) confidence: Option<f32>,
    #[schemars(description = "Optional decode lenses Prism should expose for this concept.")]
    pub(crate) decode_lenses: Option<Vec<PrismConceptLensInput>>,
    #[schemars(
        description = "Concept persistence scope. `local` stays runtime-only, `session` persists in the workspace store, and `repo` exports to committed repo knowledge."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<ConceptScopeInput>,
    #[schemars(description = "Optional concept handles this published concept supersedes.")]
    pub(crate) supersedes: Option<Vec<String>>,
    #[schemars(description = "Reason for retiring a concept. Required for `retire`.")]
    pub(crate) retirement_reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractTargetInput {
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) concept_handles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractGuaranteeInput {
    pub(crate) id: Option<String>,
    pub(crate) statement: String,
    pub(crate) scope: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) strength: Option<ContractGuaranteeStrengthInput>,
    pub(crate) evidence_refs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractValidationInput {
    pub(crate) id: String,
    pub(crate) summary: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractCompatibilityInput {
    pub(crate) compatible: Option<Vec<String>>,
    pub(crate) additive: Option<Vec<String>>,
    pub(crate) risky: Option<Vec<String>>,
    pub(crate) breaking: Option<Vec<String>>,
    pub(crate) migrating: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismContractMutationArgs {
    #[schemars(
        description = "Contract lifecycle or maintenance operation such as promote, update, attach_evidence, or retire."
    )]
    pub(crate) operation: ContractMutationOperationInput,
    #[schemars(
        description = "Stable contract handle like `contract://query_runtime_surface`. Required for every operation except `promote`, where Prism derives one from `name` when omitted."
    )]
    pub(crate) handle: Option<String>,
    #[schemars(description = "Canonical contract name. Required for `promote`.")]
    pub(crate) name: Option<String>,
    #[schemars(description = "Short explanation of the promise. Required for `promote`.")]
    pub(crate) summary: Option<String>,
    #[schemars(description = "Optional aliases for the contract.")]
    pub(crate) aliases: Option<Vec<String>>,
    #[schemars(description = "Contract type such as `interface` or `dependency_boundary`.")]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) kind: Option<ContractKindInput>,
    #[schemars(description = "Provider or governed surface making the promise.")]
    pub(crate) subject: Option<ContractTargetInput>,
    #[schemars(description = "Structured guarantees consumers may rely on.")]
    pub(crate) guarantees: Option<Vec<ContractGuaranteeInput>>,
    #[schemars(description = "Conditional assumptions under which the contract holds.")]
    pub(crate) assumptions: Option<Vec<String>>,
    #[schemars(description = "Known consumers or dependent surfaces.")]
    pub(crate) consumers: Option<Vec<ContractTargetInput>>,
    #[schemars(description = "Validation links that support the contract.")]
    pub(crate) validations: Option<Vec<ContractValidationInput>>,
    #[schemars(description = "Stability signal such as internal, public, or migrating.")]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) stability: Option<ContractStabilityInput>,
    #[schemars(description = "Compatibility guidance for additive, risky, or breaking edits.")]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) compatibility: Option<ContractCompatibilityInput>,
    #[schemars(description = "Anchored supporting evidence lines or summaries.")]
    pub(crate) evidence: Option<Vec<String>>,
    #[schemars(description = "Contract lifecycle status.")]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<ContractStatusInput>,
    #[schemars(
        description = "Contract persistence scope. `local` stays runtime-only, `session` persists in the workspace store, and `repo` exports to committed repo knowledge."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<ConceptScopeInput>,
    #[schemars(description = "Optional handles this published contract supersedes.")]
    pub(crate) supersedes: Option<Vec<String>>,
    #[schemars(description = "Reason for retiring a contract.")]
    pub(crate) retirement_reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConceptRelationMutationArgs {
    pub(crate) operation: ConceptRelationMutationOperationInput,
    pub(crate) source_handle: String,
    pub(crate) target_handle: String,
    pub(crate) kind: ConceptRelationKindInput,
    pub(crate) confidence: Option<f32>,
    pub(crate) evidence: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<ConceptScopeInput>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeIdInput {
    #[serde(alias = "crate_name")]
    #[serde(alias = "crateName")]
    pub(crate) crate_name: String,
    pub(crate) path: String,
    pub(crate) kind: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnchorRefInput {
    Node {
        #[serde(rename = "crateName", alias = "crate_name")]
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        #[serde(rename = "lineageId", alias = "lineage_id")]
        lineage_id: String,
    },
    File {
        #[serde(rename = "fileId", alias = "file_id", default)]
        #[schemars(
            description = "Internal file id. Prefer `path` when calling MCP tools directly."
        )]
        file_id: Option<u32>,
        #[serde(default)]
        #[schemars(
            description = "Workspace-relative or absolute file path. Preferred over `fileId` for direct MCP tool calls."
        )]
        path: Option<String>,
    },
    Kind {
        kind: String,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKindInput {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeResultInput {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryKindInput {
    Episodic,
    Structural,
    Semantic,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemorySourceInput {
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OutcomeEvidenceInput {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Command { argv: Vec<String>, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InferredEdgeScopeInput {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOutcomeArgs {
    pub(crate) kind: OutcomeKindInput,
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) result: Option<OutcomeResultInput>,
    pub(crate) evidence: Option<Vec<OutcomeEvidenceInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryStorePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) kind: MemoryKindInput,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<MemoryScopeInput>,
    pub(crate) content: String,
    pub(crate) trust: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) source: Option<MemorySourceInput>,
    pub(crate) metadata: Option<Value>,
    pub(crate) promoted_from: Option<Vec<String>>,
    pub(crate) supersedes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryRetirePayload {
    #[serde(alias = "memory_id")]
    pub(crate) memory_id: String,
    #[serde(alias = "retirement_reason", alias = "reason")]
    pub(crate) retirement_reason: String,
}

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryMutationActionInput {
    Store,
    Retire,
}

impl_vocab_deserialize!(
    MemoryMutationActionInput,
    "memoryMutationAction",
    "memory mutation action",
    r#"{"action":"store"}"#,
    {
        "store" => Store,
        "retire" => Retire
    }
);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MemoryScopeInput {
    Local,
    Session,
    Repo,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationFeedbackCategoryInput {
    Structural,
    Lineage,
    Memory,
    Projection,
    Coordination,
    Freshness,
    Other,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationFeedbackVerdictInput {
    Wrong,
    Stale,
    Noisy,
    Helpful,
    Mixed,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "action", content = "payload")]
#[allow(dead_code)]
enum PrismMemoryArgsWirePayload {
    Store(MemoryStorePayload),
    Retire(MemoryRetirePayload),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismMemoryArgsWire {
    #[serde(flatten)]
    mutation: PrismMemoryArgsWirePayload,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PrismMemoryArgs {
    pub(crate) action: MemoryMutationActionInput,
    pub(crate) payload: Value,
    pub(crate) task_id: Option<String>,
}

impl_schema_from_wire!(PrismMemoryArgs, PrismMemoryArgsWire, "PrismMemoryArgs");

impl<'de> Deserialize<'de> for PrismMemoryArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let wire = serde_json::from_value::<PrismMemoryArgsWire>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| de::Error::custom("missing field `payload`"))?;
        let action = match wire.mutation {
            PrismMemoryArgsWirePayload::Store(_) => MemoryMutationActionInput::Store,
            PrismMemoryArgsWirePayload::Retire(_) => MemoryMutationActionInput::Retire,
        };
        Ok(Self {
            action,
            payload,
            task_id: wire.task_id,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismValidationFeedbackArgs {
    #[schemars(
        description = "Optional anchors for the feedback. Leave empty when reporting tool-level or workspace-level feedback that does not map cleanly to a semantic target."
    )]
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) context: String,
    #[serde(alias = "prism_said")]
    pub(crate) prism_said: String,
    #[serde(alias = "actually_true")]
    pub(crate) actually_true: String,
    pub(crate) category: ValidationFeedbackCategoryInput,
    pub(crate) verdict: ValidationFeedbackVerdictInput,
    #[serde(alias = "corrected_manually")]
    pub(crate) corrected_manually: Option<bool>,
    pub(crate) correction: Option<String>,
    pub(crate) metadata: Option<Value>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFinishTaskArgs {
    pub(crate) summary: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventMutationResult {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryMutationResult {
    pub(crate) memory_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationFeedbackMutationResult {
    pub(crate) entry_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptMutationResult {
    pub(crate) event_id: String,
    pub(crate) concept_handle: String,
    pub(crate) task_id: String,
    pub(crate) packet: ConceptPacketView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractMutationResult {
    pub(crate) event_id: String,
    pub(crate) contract_handle: String,
    pub(crate) task_id: String,
    pub(crate) packet: ContractPacketView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConceptRelationMutationResult {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
    pub(crate) relation: ConceptRelationView,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EdgeMutationResult {
    pub(crate) edge_id: String,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalDecisionResult {
    pub(crate) job_id: String,
    pub(crate) proposal_index: usize,
    pub(crate) kind: String,
    pub(crate) decision: CuratorProposalDecision,
    pub(crate) proposal: Value,
    pub(crate) created: CuratorProposalCreatedResources,
    pub(crate) detail: Option<String>,
    pub(crate) memory_id: Option<String>,
    pub(crate) edge_id: Option<String>,
    pub(crate) concept_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum CuratorProposalDecision {
    Applied,
    Rejected,
    NotApplicableYet,
}

#[derive(Debug, Clone, Default, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CuratorProposalCreatedResources {
    pub(crate) memory_id: Option<String>,
    pub(crate) edge_id: Option<String>,
    pub(crate) concept_handle: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryDiagnosticSchema {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) data: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
pub(crate) struct QueryEnvelopeSchema {
    pub(crate) result: Value,
    pub(crate) diagnostics: Vec<QueryDiagnosticSchema>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueryLimitsInput {
    pub(crate) max_result_nodes: Option<usize>,
    pub(crate) max_call_graph_depth: Option<usize>,
    pub(crate) max_output_json_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismConfigureSessionArgs {
    pub(crate) limits: Option<QueryLimitsInput>,
    #[serde(alias = "current_task_id")]
    pub(crate) current_task_id: Option<String>,
    #[serde(alias = "coordination_task_id")]
    pub(crate) coordination_task_id: Option<String>,
    #[serde(alias = "current_task_description")]
    pub(crate) current_task_description: Option<String>,
    #[serde(alias = "current_task_tags")]
    pub(crate) current_task_tags: Option<Vec<String>>,
    pub(crate) clear_current_task: Option<bool>,
    #[serde(alias = "current_agent")]
    pub(crate) current_agent: Option<String>,
    pub(crate) clear_current_agent: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismInferEdgeArgs {
    pub(crate) source: NodeIdInput,
    pub(crate) target: NodeIdInput,
    pub(crate) kind: String,
    pub(crate) confidence: f32,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) evidence: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionRepairOperationInput {
    ClearCurrentTask,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismSessionRepairArgs {
    pub(crate) operation: SessionRepairOperationInput,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionRepairOperationSchema {
    ClearCurrentTask,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionRepairMutationResult {
    pub(crate) operation: SessionRepairOperationSchema,
    pub(crate) cleared_task_id: Option<String>,
    pub(crate) session: SessionView,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMutationCredentialArgs {
    #[serde(alias = "credential_id")]
    pub(crate) credential_id: String,
    #[serde(alias = "principal_token")]
    pub(crate) principal_token: String,
}

#[derive(Debug)]
pub(crate) struct PrismMutationArgs {
    pub(crate) credential: PrismMutationCredentialArgs,
    pub(crate) mutation: PrismMutationKindArgs,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismHeartbeatLeaseArgs {
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    #[serde(alias = "claim_id")]
    pub(crate) claim_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkDeclarationKindInput {
    AdHoc,
    Coordination,
    Delegated,
}

#[derive(Debug)]
pub(crate) enum PrismMutationKindArgs {
    DeclareWork(PrismDeclareWorkArgs),
    Checkpoint(PrismCheckpointArgs),
    Outcome(PrismOutcomeArgs),
    Memory(PrismMemoryArgs),
    Concept(PrismConceptMutationArgs),
    Contract(PrismContractMutationArgs),
    ConceptRelation(PrismConceptRelationMutationArgs),
    ValidationFeedback(PrismValidationFeedbackArgs),
    SessionRepair(PrismSessionRepairArgs),
    InferEdge(PrismInferEdgeArgs),
    HeartbeatLease(PrismHeartbeatLeaseArgs),
    Coordination(PrismCoordinationArgs),
    Claim(PrismClaimArgs),
    Artifact(PrismArtifactArgs),
    TestRan(PrismTestRanArgs),
    FailureObserved(PrismFailureObservedArgs),
    FixValidated(PrismFixValidatedArgs),
    CuratorApplyProposal(PrismCuratorApplyProposalArgs),
    CuratorPromoteEdge(PrismCuratorPromoteEdgeArgs),
    CuratorPromoteConcept(PrismCuratorPromoteConceptArgs),
    CuratorPromoteMemory(PrismCuratorPromoteMemoryArgs),
    CuratorRejectProposal(PrismCuratorRejectProposalArgs),
}

impl PrismMutationKindArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = ensure_root_object_input_schema)]
#[serde(rename_all = "snake_case", tag = "action", content = "input")]
enum PrismMutationKindArgsWire {
    DeclareWork(PrismDeclareWorkArgs),
    Checkpoint(PrismCheckpointArgs),
    Outcome(PrismOutcomeArgs),
    Memory(PrismMemoryArgs),
    Concept(PrismConceptMutationArgs),
    Contract(PrismContractMutationArgs),
    ConceptRelation(PrismConceptRelationMutationArgs),
    ValidationFeedback(PrismValidationFeedbackArgs),
    SessionRepair(PrismSessionRepairArgs),
    InferEdge(PrismInferEdgeArgs),
    HeartbeatLease(PrismHeartbeatLeaseArgs),
    Coordination(PrismCoordinationArgs),
    Claim(PrismClaimArgs),
    Artifact(PrismArtifactArgs),
    TestRan(PrismTestRanArgs),
    FailureObserved(PrismFailureObservedArgs),
    FixValidated(PrismFixValidatedArgs),
    CuratorApplyProposal(PrismCuratorApplyProposalArgs),
    CuratorPromoteEdge(PrismCuratorPromoteEdgeArgs),
    CuratorPromoteConcept(PrismCuratorPromoteConceptArgs),
    CuratorPromoteMemory(PrismCuratorPromoteMemoryArgs),
    CuratorRejectProposal(PrismCuratorRejectProposalArgs),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrismMutationArgsWire {
    pub(crate) credential: PrismMutationCredentialArgs,
    #[serde(flatten)]
    pub(crate) mutation: PrismMutationKindArgsWire,
}

impl From<PrismMutationKindArgsWire> for PrismMutationKindArgs {
    fn from(value: PrismMutationKindArgsWire) -> Self {
        match value {
            PrismMutationKindArgsWire::DeclareWork(args) => Self::DeclareWork(args),
            PrismMutationKindArgsWire::Checkpoint(args) => Self::Checkpoint(args),
            PrismMutationKindArgsWire::Outcome(args) => Self::Outcome(args),
            PrismMutationKindArgsWire::Memory(args) => Self::Memory(args),
            PrismMutationKindArgsWire::Concept(args) => Self::Concept(args),
            PrismMutationKindArgsWire::Contract(args) => Self::Contract(args),
            PrismMutationKindArgsWire::ConceptRelation(args) => Self::ConceptRelation(args),
            PrismMutationKindArgsWire::ValidationFeedback(args) => Self::ValidationFeedback(args),
            PrismMutationKindArgsWire::SessionRepair(args) => Self::SessionRepair(args),
            PrismMutationKindArgsWire::InferEdge(args) => Self::InferEdge(args),
            PrismMutationKindArgsWire::HeartbeatLease(args) => Self::HeartbeatLease(args),
            PrismMutationKindArgsWire::Coordination(args) => Self::Coordination(args),
            PrismMutationKindArgsWire::Claim(args) => Self::Claim(args),
            PrismMutationKindArgsWire::Artifact(args) => Self::Artifact(args),
            PrismMutationKindArgsWire::TestRan(args) => Self::TestRan(args),
            PrismMutationKindArgsWire::FailureObserved(args) => Self::FailureObserved(args),
            PrismMutationKindArgsWire::FixValidated(args) => Self::FixValidated(args),
            PrismMutationKindArgsWire::CuratorApplyProposal(args) => {
                Self::CuratorApplyProposal(args)
            }
            PrismMutationKindArgsWire::CuratorPromoteEdge(args) => Self::CuratorPromoteEdge(args),
            PrismMutationKindArgsWire::CuratorPromoteConcept(args) => {
                Self::CuratorPromoteConcept(args)
            }
            PrismMutationKindArgsWire::CuratorPromoteMemory(args) => {
                Self::CuratorPromoteMemory(args)
            }
            PrismMutationKindArgsWire::CuratorRejectProposal(args) => {
                Self::CuratorRejectProposal(args)
            }
        }
    }
}

impl JsonSchema for PrismMutationArgs {
    fn inline_schema() -> bool {
        false
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PrismMutationArgs")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = PrismMutationKindArgsWire::json_schema(generator);
        ensure_root_object_input_schema(&mut schema);
        let credential_schema =
            serde_json::to_value(generator.subschema_for::<PrismMutationCredentialArgs>())
                .expect("credential schema should serialize");
        if let Some(variants) = schema.get_mut("oneOf").and_then(Value::as_array_mut) {
            for variant in variants {
                let Some(properties) = variant.get_mut("properties").and_then(Value::as_object_mut)
                else {
                    continue;
                };
                properties.insert("credential".to_string(), credential_schema.clone());
                let required = variant
                    .get_mut("required")
                    .and_then(Value::as_array_mut)
                    .expect("prism_mutate variants should declare required fields");
                if !required
                    .iter()
                    .any(|value| value.as_str() == Some("credential"))
                {
                    required.push(Value::String("credential".to_string()));
                }
            }
        }
        schema
    }
}

impl<'de> Deserialize<'de> for PrismMutationArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_tagged_tool_input::<PrismMutationArgsWire>("prism_mutate", value)
            .map(|wire| Self {
                credential: wire.credential,
                mutation: wire.mutation.into(),
            })
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrismMutationActionSchema {
    DeclareWork,
    Checkpoint,
    Outcome,
    Memory,
    Concept,
    Contract,
    ConceptRelation,
    ValidationFeedback,
    SessionRepair,
    InferEdge,
    HeartbeatLease,
    Coordination,
    Claim,
    Artifact,
    TestRan,
    FailureObserved,
    FixValidated,
    CuratorApplyProposal,
    CuratorPromoteEdge,
    CuratorPromoteConcept,
    CuratorPromoteMemory,
    CuratorRejectProposal,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismMutationResult {
    pub(crate) action: PrismMutationActionSchema,
    pub(crate) result: Value,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismDeclareWorkArgs {
    pub(crate) title: String,
    pub(crate) kind: Option<WorkDeclarationKindInput>,
    pub(crate) summary: Option<String>,
    #[serde(alias = "parent_work_id")]
    pub(crate) parent_work_id: Option<String>,
    #[serde(alias = "coordination_task_id")]
    pub(crate) coordination_task_id: Option<String>,
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkDeclarationResult {
    pub(crate) work_id: String,
    pub(crate) kind: prism_ir::WorkContextKind,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    pub(crate) parent_work_id: Option<String>,
    pub(crate) coordination_task_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) plan_title: Option<String>,
    pub(crate) session: SessionView,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCheckpointArgs {
    pub(crate) summary: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckpointMutationResult {
    pub(crate) event_ids: Vec<String>,
    pub(crate) task_id: String,
    pub(crate) summary: Option<String>,
    pub(crate) session: SessionView,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismTestRanArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) test: String,
    pub(crate) passed: bool,
    pub(crate) command: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFailureObservedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) trace: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismFixValidatedArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) summary: String,
    pub(crate) command: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinationMutationKindInput {
    PlanCreate,
    PlanUpdate,
    PlanArchive,
    TaskCreate,
    Update,
    PlanNodeCreate,
    PlanEdgeCreate,
    PlanEdgeDelete,
    Handoff,
    Resume,
    Reclaim,
    HandoffAccept,
}

impl_vocab_deserialize!(
    CoordinationMutationKindInput,
    "coordinationMutationKind",
    "coordination mutation kind",
    r#"{"kind":"task_create"}"#,
    {
        "plancreate" => PlanCreate,
        "planupdate" => PlanUpdate,
        "planarchive" => PlanArchive,
        "taskcreate" => TaskCreate,
        "update" => Update,
        "plannodecreate" => PlanNodeCreate,
        "planedgecreate" => PlanEdgeCreate,
        "planedgedelete" => PlanEdgeDelete,
        "handoff" => Handoff,
        "resume" => Resume,
        "reclaim" => Reclaim,
        "handoffaccept" => HandoffAccept
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimActionInput {
    Acquire,
    Renew,
    Release,
}

impl_vocab_deserialize!(
    ClaimActionInput,
    "claimAction",
    "claim action",
    r#"{"action":"acquire"}"#,
    {
        "acquire" => Acquire,
        "renew" => Renew,
        "release" => Release
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactActionInput {
    Propose,
    Supersede,
    Review,
}

impl_vocab_deserialize!(
    ArtifactActionInput,
    "artifactAction",
    "artifact action",
    r#"{"action":"propose"}"#,
    {
        "propose" => Propose,
        "supersede" => Supersede,
        "review" => Review
    }
);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
#[allow(dead_code)]
enum PrismCoordinationArgsWirePayload {
    PlanCreate(PlanCreatePayload),
    PlanUpdate(PlanUpdatePayload),
    PlanArchive(PlanArchivePayload),
    TaskCreate(TaskCreatePayload),
    Update(WorkflowUpdatePayload),
    PlanNodeCreate(PlanNodeCreatePayload),
    PlanEdgeCreate(PlanEdgeCreatePayload),
    PlanEdgeDelete(PlanEdgeDeletePayload),
    Handoff(HandoffPayload),
    Resume(TaskResumePayload),
    Reclaim(TaskReclaimPayload),
    HandoffAccept(HandoffAcceptPayload),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismCoordinationArgsWire {
    #[serde(flatten)]
    mutation: PrismCoordinationArgsWirePayload,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PrismCoordinationArgs {
    pub(crate) kind: CoordinationMutationKindInput,
    pub(crate) payload: Value,
    pub(crate) task_id: Option<String>,
}

impl_schema_from_wire!(
    PrismCoordinationArgs,
    PrismCoordinationArgsWire,
    "PrismCoordinationArgs"
);

impl<'de> Deserialize<'de> for PrismCoordinationArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let wire = serde_json::from_value::<PrismCoordinationArgsWire>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| de::Error::custom("missing field `payload`"))?;
        let kind = match wire.mutation {
            PrismCoordinationArgsWirePayload::PlanCreate(_) => {
                CoordinationMutationKindInput::PlanCreate
            }
            PrismCoordinationArgsWirePayload::PlanUpdate(_) => {
                CoordinationMutationKindInput::PlanUpdate
            }
            PrismCoordinationArgsWirePayload::PlanArchive(_) => {
                CoordinationMutationKindInput::PlanArchive
            }
            PrismCoordinationArgsWirePayload::TaskCreate(_) => {
                CoordinationMutationKindInput::TaskCreate
            }
            PrismCoordinationArgsWirePayload::Update(_) => CoordinationMutationKindInput::Update,
            PrismCoordinationArgsWirePayload::PlanNodeCreate(_) => {
                CoordinationMutationKindInput::PlanNodeCreate
            }
            PrismCoordinationArgsWirePayload::PlanEdgeCreate(_) => {
                CoordinationMutationKindInput::PlanEdgeCreate
            }
            PrismCoordinationArgsWirePayload::PlanEdgeDelete(_) => {
                CoordinationMutationKindInput::PlanEdgeDelete
            }
            PrismCoordinationArgsWirePayload::Handoff(_) => CoordinationMutationKindInput::Handoff,
            PrismCoordinationArgsWirePayload::Resume(_) => CoordinationMutationKindInput::Resume,
            PrismCoordinationArgsWirePayload::Reclaim(_) => CoordinationMutationKindInput::Reclaim,
            PrismCoordinationArgsWirePayload::HandoffAccept(_) => {
                CoordinationMutationKindInput::HandoffAccept
            }
        };
        Ok(Self {
            kind,
            payload,
            task_id: wire.task_id,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "action", content = "payload")]
#[allow(dead_code)]
enum PrismClaimArgsWirePayload {
    Acquire(ClaimAcquirePayload),
    Renew(ClaimRenewPayload),
    Release(ClaimReleasePayload),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismClaimArgsWire {
    #[serde(flatten)]
    mutation: PrismClaimArgsWirePayload,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PrismClaimArgs {
    pub(crate) action: ClaimActionInput,
    pub(crate) payload: Value,
    pub(crate) task_id: Option<String>,
}

impl_schema_from_wire!(PrismClaimArgs, PrismClaimArgsWire, "PrismClaimArgs");

impl<'de> Deserialize<'de> for PrismClaimArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let wire = serde_json::from_value::<PrismClaimArgsWire>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| de::Error::custom("missing field `payload`"))?;
        let action = match wire.mutation {
            PrismClaimArgsWirePayload::Acquire(_) => ClaimActionInput::Acquire,
            PrismClaimArgsWirePayload::Renew(_) => ClaimActionInput::Renew,
            PrismClaimArgsWirePayload::Release(_) => ClaimActionInput::Release,
        };
        Ok(Self {
            action,
            payload,
            task_id: wire.task_id,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "action", content = "payload")]
#[allow(dead_code)]
enum PrismArtifactArgsWirePayload {
    Propose(ArtifactProposePayload),
    Supersede(ArtifactSupersedePayload),
    Review(ArtifactReviewPayload),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismArtifactArgsWire {
    #[serde(flatten)]
    mutation: PrismArtifactArgsWirePayload,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct PrismArtifactArgs {
    pub(crate) action: ArtifactActionInput,
    pub(crate) payload: Value,
    pub(crate) task_id: Option<String>,
}

impl_schema_from_wire!(
    PrismArtifactArgs,
    PrismArtifactArgsWire,
    "PrismArtifactArgs"
);

impl<'de> Deserialize<'de> for PrismArtifactArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let wire = serde_json::from_value::<PrismArtifactArgsWire>(value.clone())
            .map_err(serde::de::Error::custom)?;
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| de::Error::custom("missing field `payload`"))?;
        let action = match wire.mutation {
            PrismArtifactArgsWirePayload::Propose(_) => ArtifactActionInput::Propose,
            PrismArtifactArgsWirePayload::Supersede(_) => ArtifactActionInput::Supersede,
            PrismArtifactArgsWirePayload::Review(_) => ArtifactActionInput::Review,
        };
        Ok(Self {
            action,
            payload,
            task_id: wire.task_id,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlansQueryArgs {
    pub(crate) status: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) contains: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractsQueryArgs {
    pub(crate) status: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) contains: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanTargetArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanProjectionAtArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    pub(crate) at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanProjectionDiffArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    pub(crate) from: u64,
    pub(crate) to: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNextArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNodeTargetArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: String,
    #[serde(alias = "node_id")]
    pub(crate) node_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationTaskTargetArgs {
    #[serde(alias = "task_id")]
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PolicyViolationQueryArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LimitArgs {
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnchorListArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingReviewsArgs {
    #[serde(alias = "plan_id")]
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulateClaimArgs {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: CapabilityInput,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) mode: Option<ClaimModeInput>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CapabilityInput {
    Observe,
    Edit,
    Review,
    Validate,
    Merge,
}

impl_vocab_deserialize!(
    CapabilityInput,
    "capability",
    "capability",
    r#"{"capability":"edit"}"#,
    {
        "observe" => Observe,
        "edit" => Edit,
        "review" => Review,
        "validate" => Validate,
        "merge" => Merge
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimModeInput {
    Advisory,
    SoftExclusive,
    HardExclusive,
}

impl_vocab_deserialize!(
    ClaimModeInput,
    "claimMode",
    "claim mode",
    r#"{"mode":"soft_exclusive"}"#,
    {
        "advisory" => Advisory,
        "softexclusive" => SoftExclusive,
        "hardexclusive" => HardExclusive
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LeaseRenewalModeInput {
    Strict,
    Assisted,
}

impl_vocab_deserialize!(
    LeaseRenewalModeInput,
    "leaseRenewalMode",
    "lease renewal mode",
    r#"{"leaseRenewalMode":"strict"}"#,
    {
        "strict" => Strict,
        "assisted" => Assisted
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GitExecutionStartModeInput {
    Off,
    Require,
}

impl_vocab_deserialize!(
    GitExecutionStartModeInput,
    "gitExecutionStartMode",
    "git execution start mode",
    r#"{"startMode":"require"}"#,
    {
        "off" => Off,
        "require" => Require
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GitExecutionCompletionModeInput {
    Off,
    Require,
}

impl_vocab_deserialize!(
    GitExecutionCompletionModeInput,
    "gitExecutionCompletionMode",
    "git execution completion mode",
    r#"{"completionMode":"require"}"#,
    {
        "off" => Off,
        "require" => Require
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GitIntegrationModeInput {
    ManualPr,
    AutoPr,
    DirectIntegrate,
    External,
}

impl_vocab_deserialize!(
    GitIntegrationModeInput,
    "gitIntegrationMode",
    "git integration mode",
    r#"{"integrationMode":"external"}"#,
    {
        "manualpr" => ManualPr,
        "autopr" => AutoPr,
        "directintegrate" => DirectIntegrate,
        "external" => External
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinationTaskStatusInput {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    CoordinationTaskStatusInput,
    "coordinationTaskStatus",
    "coordination task status",
    r#"{"status":"ready"}"#,
    {
        "proposed" => Proposed,
        "todo" => Ready,
        "ready" => Ready,
        "inprogress" => InProgress,
        "blocked" => Blocked,
        "inreview" => InReview,
        "validating" => Validating,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkflowStatusInput {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    Waiting,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    WorkflowStatusInput,
    "workflowStatus",
    "workflow status",
    r#"{"status":"in_progress"}"#,
    {
        "proposed" => Proposed,
        "todo" => Ready,
        "ready" => Ready,
        "inprogress" => InProgress,
        "blocked" => Blocked,
        "waiting" => Waiting,
        "inreview" => InReview,
        "validating" => Validating,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanStatusInput {
    Draft,
    Active,
    Blocked,
    Completed,
    Abandoned,
    Archived,
}

impl_vocab_deserialize!(
    PlanStatusInput,
    "planStatus",
    "coordination plan status",
    r#"{"status":"active"}"#,
    {
        "draft" => Draft,
        "active" => Active,
        "blocked" => Blocked,
        "completed" => Completed,
        "abandoned" => Abandoned,
        "archived" => Archived
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AcceptanceEvidencePolicyInput {
    Any,
    All,
    ReviewOnly,
    ValidationOnly,
    ReviewAndValidation,
}

impl_vocab_deserialize!(
    AcceptanceEvidencePolicyInput,
    "acceptanceEvidencePolicy",
    "acceptance evidence policy",
    r#"{"evidencePolicy":"any"}"#,
    {
        "any" => Any,
        "all" => All,
        "reviewonly" => ReviewOnly,
        "validationonly" => ValidationOnly,
        "reviewandvalidation" => ReviewAndValidation
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanNodeStatusInput {
    Proposed,
    Ready,
    InProgress,
    Blocked,
    Waiting,
    InReview,
    Validating,
    Completed,
    Abandoned,
}

impl_vocab_deserialize!(
    PlanNodeStatusInput,
    "planNodeStatus",
    "plan node status",
    r#"{"status":"ready"}"#,
    {
        "proposed" => Proposed,
        "todo" => Ready,
        "ready" => Ready,
        "inprogress" => InProgress,
        "blocked" => Blocked,
        "waiting" => Waiting,
        "inreview" => InReview,
        "validating" => Validating,
        "completed" => Completed,
        "abandoned" => Abandoned
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanNodeKindInput {
    Investigate,
    Decide,
    Edit,
    Validate,
    Review,
    Handoff,
    Merge,
    Release,
    Note,
}

impl_vocab_deserialize!(
    PlanNodeKindInput,
    "planNodeKind",
    "plan node kind",
    r#"{"kind":"edit"}"#,
    {
        "investigate" => Investigate,
        "decide" => Decide,
        "edit" => Edit,
        "validate" => Validate,
        "review" => Review,
        "handoff" => Handoff,
        "merge" => Merge,
        "release" => Release,
        "note" => Note
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanEdgeKindInput {
    DependsOn,
    Blocks,
    Informs,
    Validates,
    HandoffTo,
    ChildOf,
    RelatedTo,
}

impl_vocab_deserialize!(
    PlanEdgeKindInput,
    "planEdgeKind",
    "plan edge kind",
    r#"{"kind":"depends_on"}"#,
    {
        "dependson" => DependsOn,
        "blocks" => Blocks,
        "informs" => Informs,
        "validates" => Validates,
        "handoffto" => HandoffTo,
        "childof" => ChildOf,
        "relatedto" => RelatedTo
    }
);

#[derive(Debug, Clone, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewVerdictInput {
    Approved,
    ChangesRequested,
    Rejected,
}

impl_vocab_deserialize!(
    ReviewVerdictInput,
    "reviewVerdict",
    "review verdict",
    r#"{"verdict":"approved"}"#,
    {
        "approved" => Approved,
        "changesrequested" => ChangesRequested,
        "rejected" => Rejected
    }
);

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanCreatePayload {
    pub(crate) title: String,
    pub(crate) goal: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<PlanStatusInput>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
    pub(crate) scheduling: Option<PlanSchedulingPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanUpdatePayload {
    pub(crate) plan_id: String,
    pub(crate) title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<PlanStatusInput>,
    pub(crate) goal: Option<String>,
    pub(crate) policy: Option<CoordinationPolicyPayload>,
    pub(crate) scheduling: Option<PlanSchedulingPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanArchivePayload {
    pub(crate) plan_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationPolicyPayload {
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) default_claim_mode: Option<ClaimModeInput>,
    pub(crate) max_parallel_editors_per_anchor: Option<u16>,
    pub(crate) require_review_for_completion: Option<bool>,
    pub(crate) require_validation_for_completion: Option<bool>,
    pub(crate) stale_after_graph_change: Option<bool>,
    pub(crate) review_required_above_risk_score: Option<f32>,
    pub(crate) lease_stale_after_seconds: Option<u64>,
    pub(crate) lease_expires_after_seconds: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) lease_renewal_mode: Option<LeaseRenewalModeInput>,
    pub(crate) git_execution: Option<GitExecutionPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitExecutionPolicyPayload {
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) start_mode: Option<GitExecutionStartModeInput>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) completion_mode: Option<GitExecutionCompletionModeInput>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) integration_mode: Option<GitIntegrationModeInput>,
    pub(crate) target_ref: Option<String>,
    pub(crate) target_branch: Option<String>,
    pub(crate) require_task_branch: Option<bool>,
    pub(crate) max_commits_behind_target: Option<u32>,
    pub(crate) max_fetch_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanSchedulingPayload {
    pub(crate) importance: Option<u8>,
    pub(crate) urgency: Option<u8>,
    pub(crate) manual_boost: Option<i16>,
    pub(crate) due_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationRefPayload {
    pub(crate) id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanBindingPayload {
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) concept_handles: Option<Vec<String>>,
    pub(crate) artifact_refs: Option<Vec<String>>,
    pub(crate) memory_refs: Option<Vec<String>>,
    pub(crate) outcome_refs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcceptanceCriterionPayload {
    pub(crate) label: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) required_checks: Option<Vec<ValidationRefPayload>>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) evidence_policy: Option<AcceptanceEvidencePolicyInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) title: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<CoordinationTaskStatusInput>,
    pub(crate) assignee: Option<String>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowUpdatePayload {
    #[serde(
        alias = "taskId",
        alias = "nodeId",
        alias = "task_id",
        alias = "node_id"
    )]
    pub(crate) id: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) kind: Option<PlanNodeKindInput>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<WorkflowStatusInput>,
    pub(crate) assignee: Option<SparsePatchInput<String>>,
    pub(crate) is_abstract: Option<bool>,
    pub(crate) title: Option<String>,
    pub(crate) summary: Option<SparsePatchInput<String>>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) bindings: Option<PlanBindingPayload>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
    pub(crate) validation_refs: Option<Vec<ValidationRefPayload>>,
    pub(crate) priority: Option<SparsePatchInput<u8>>,
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) completion_context: Option<TaskCompletionContextPayload>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanNodeCreatePayload {
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) kind: Option<PlanNodeKindInput>,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) status: Option<PlanNodeStatusInput>,
    pub(crate) assignee: Option<String>,
    pub(crate) is_abstract: Option<bool>,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) bindings: Option<PlanBindingPayload>,
    pub(crate) depends_on: Option<Vec<String>>,
    pub(crate) acceptance: Option<Vec<AcceptanceCriterionPayload>>,
    pub(crate) validation_refs: Option<Vec<ValidationRefPayload>>,
    pub(crate) priority: Option<u8>,
    pub(crate) tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanEdgeCreatePayload {
    pub(crate) plan_id: String,
    pub(crate) from_node_id: String,
    pub(crate) to_node_id: String,
    pub(crate) kind: PlanEdgeKindInput,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanEdgeDeletePayload {
    pub(crate) plan_id: String,
    pub(crate) from_node_id: String,
    pub(crate) to_node_id: String,
    pub(crate) kind: PlanEdgeKindInput,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskCompletionContextPayload {
    pub(crate) risk_score: Option<f32>,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) review_artifact_ref: Option<String>,
    pub(crate) integration_commit: Option<String>,
    pub(crate) integration_evidence: Option<prism_ir::GitIntegrationEvidence>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffPayload {
    pub(crate) task_id: String,
    pub(crate) to_agent: Option<String>,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffAcceptPayload {
    pub(crate) task_id: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskResumePayload {
    pub(crate) task_id: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskReclaimPayload {
    pub(crate) task_id: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimAcquirePayload {
    pub(crate) anchors: Vec<AnchorRefInput>,
    pub(crate) capability: CapabilityInput,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) mode: Option<ClaimModeInput>,
    pub(crate) ttl_seconds: Option<u64>,
    pub(crate) agent: Option<String>,
    pub(crate) coordination_task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimRenewPayload {
    pub(crate) claim_id: String,
    pub(crate) ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimReleasePayload {
    pub(crate) claim_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactProposePayload {
    pub(crate) task_id: String,
    pub(crate) anchors: Option<Vec<AnchorRefInput>>,
    pub(crate) diff_ref: Option<String>,
    pub(crate) evidence: Option<Vec<String>>,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactSupersedePayload {
    pub(crate) artifact_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactReviewPayload {
    pub(crate) artifact_id: String,
    pub(crate) verdict: ReviewVerdictInput,
    pub(crate) summary: String,
    pub(crate) required_validations: Option<Vec<String>>,
    pub(crate) validated_checks: Option<Vec<String>>,
    pub(crate) risk_score: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationViolationView {
    pub(crate) code: String,
    pub(crate) summary: String,
    pub(crate) plan_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) claim_id: Option<String>,
    pub(crate) artifact_id: Option<String>,
    pub(crate) details: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CoordinationMutationResult {
    pub(crate) event_id: String,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaimMutationResult {
    pub(crate) claim_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) conflicts: Vec<Value>,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeartbeatLeaseMutationResult {
    pub(crate) task_id: Option<String>,
    pub(crate) claim_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactMutationResult {
    pub(crate) artifact_id: Option<String>,
    pub(crate) review_id: Option<String>,
    pub(crate) event_ids: Vec<String>,
    pub(crate) rejected: bool,
    pub(crate) violations: Vec<MutationViolationView>,
    pub(crate) state: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteEdgeArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<InferredEdgeScopeInput>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorApplyProposalOptionsArgs {
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) edge_scope: Option<InferredEdgeScopeInput>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) concept_scope: Option<ConceptScopeInput>,
    pub(crate) memory_trust: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorApplyProposalArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) note: Option<String>,
    pub(crate) options: Option<PrismCuratorApplyProposalOptionsArgs>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorRejectProposalArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) reason: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteConceptArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_enum")]
    pub(crate) scope: Option<ConceptScopeInput>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismCuratorPromoteMemoryArgs {
    #[serde(alias = "job_id")]
    pub(crate) job_id: String,
    #[serde(alias = "proposal_index")]
    pub(crate) proposal_index: usize,
    pub(crate) trust: Option<f32>,
    pub(crate) note: Option<String>,
    #[serde(alias = "task_id")]
    pub(crate) task_id: Option<String>,
}
