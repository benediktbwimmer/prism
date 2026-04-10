use std::{path::Path, sync::OnceLock};

use prism_agent::EdgeId;
use prism_ir::{AnchorRef, EventId, LineageId, NodeId, PlanId, TaskId};
use prism_js::{NodeIdView, SymbolView};
use prism_memory::{MemoryId, OutcomeEvent};
use rmcp::{
    model::{Meta, RawResource, ResourceContents},
    schemars::JsonSchema,
    ErrorData as McpError,
};
use serde_json::{json, Value};

use crate::{
    capabilities_section_resource_uri, compact_followups::workspace_display_path, parse_node_kind,
    resource_example_resource_uri, resource_example_uri, resource_shape_resource_uri,
    schema_examples, vocab_entry_resource_uri, ResourceLinkView, ResourcePageView,
    ResourceSchemaCatalogEntry, CAPABILITIES_SECTION_RESOURCE_TEMPLATE_URI, CAPABILITIES_URI,
    CONTRACTS_RESOURCE_TEMPLATE_URI, CONTRACTS_URI, EDGE_RESOURCE_TEMPLATE_URI,
    ENTRYPOINTS_RESOURCE_TEMPLATE_URI, EVENT_RESOURCE_TEMPLATE_URI, FILE_RESOURCE_TEMPLATE_URI,
    LINEAGE_RESOURCE_TEMPLATE_URI, MEMORY_RESOURCE_TEMPLATE_URI, PLANS_RESOURCE_TEMPLATE_URI,
    PLANS_URI, PLAN_RESOURCE_TEMPLATE_URI, PROTECTED_STATE_URI,
    RESOURCE_EXAMPLE_RESOURCE_TEMPLATE_URI, RESOURCE_SHAPE_RESOURCE_TEMPLATE_URI, SCHEMAS_URI,
    SEARCH_RESOURCE_TEMPLATE_URI, SELF_DESCRIPTION_AUDIT_URI, SESSION_URI,
    SYMBOL_RESOURCE_TEMPLATE_URI, TASK_RESOURCE_TEMPLATE_URI, TOOL_SCHEMAS_URI,
    VOCAB_ENTRY_RESOURCE_TEMPLATE_URI, VOCAB_URI,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResourcePageRequest {
    offset: usize,
    limit: usize,
    limit_capped: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PageSlice<T> {
    pub(crate) items: Vec<T>,
    pub(crate) page: ResourcePageView,
    pub(crate) truncated: bool,
}

pub(crate) fn json_resource_contents_with_meta<T: serde::Serialize>(
    value: T,
    uri: impl Into<String>,
    meta: Option<Meta>,
) -> Result<ResourceContents, McpError> {
    let text = serde_json::to_string_pretty(&value).map_err(|err| {
        McpError::internal_error(
            "failed to serialize resource payload",
            Some(json!({ "error": err.to_string() })),
        )
    })?;
    let contents = ResourceContents::text(text, uri).with_mime_type("application/json");
    Ok(match meta {
        Some(meta) => contents.with_meta(meta),
        None => contents,
    })
}

pub(crate) fn schema_resource_contents<T: JsonSchema + std::any::Any>(
    schema_uri: &str,
    title: &str,
    description: &str,
    target_resource_kind: &str,
) -> Result<ResourceContents, McpError> {
    let schema = schema_resource_value::<T>(schema_uri, title, description, target_resource_kind);
    json_resource_contents_with_meta(
        schema,
        schema_uri.to_string(),
        Some(resource_meta("schema", None, Some(target_resource_kind))),
    )
    .map(|contents| contents.with_mime_type("application/schema+json"))
}

pub(crate) fn schema_resource_value<T: JsonSchema + std::any::Any>(
    schema_uri: &str,
    title: &str,
    description: &str,
    target_resource_kind: &str,
) -> Value {
    let mut schema = Value::Object(
        rmcp::handler::server::tool::schema_for_type::<T>()
            .as_ref()
            .clone(),
    );
    if let Value::Object(object) = &mut schema {
        object.insert(
            "$schema".to_string(),
            Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
        );
        object.insert("$id".to_string(), Value::String(schema_uri.to_string()));
        object.insert("title".to_string(), Value::String(title.to_string()));
        object.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
        if let Some(examples) = schema_examples(target_resource_kind) {
            object.insert("examples".to_string(), Value::Array(examples));
        }
    }
    schema
}

pub(crate) fn split_resource_uri(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (uri, None),
    }
}

pub(crate) fn parse_resource_page(
    uri: &str,
    default_limit: usize,
    max_limit: usize,
) -> Result<ResourcePageRequest, McpError> {
    let (_, query) = split_resource_uri(uri);
    let mut requested_limit = None;
    let mut offset = None;

    if let Some(query) = query {
        for part in query.split('&').filter(|part| !part.is_empty()) {
            let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
            let key = percent_decode_lossy(raw_key);
            let value = percent_decode_lossy(raw_value);
            match key.as_str() {
                "limit" => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid pagination limit",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?;
                    requested_limit = Some(parsed);
                }
                "cursor" | "offset" => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid pagination cursor",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?;
                    offset = Some(parsed);
                }
                _ => {}
            }
        }
    }

    let requested = requested_limit.unwrap_or(default_limit);
    let limit = requested.min(max_limit).max(1);
    Ok(ResourcePageRequest {
        offset: offset.unwrap_or(0),
        limit,
        limit_capped: requested > max_limit,
    })
}

pub(crate) fn parse_schema_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://schema/")
        .map(percent_decode_lossy)
}

pub(crate) fn parse_tool_schema_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://schema/tool/")
        .map(percent_decode_lossy)
        .filter(|tool_name| !tool_name.trim().is_empty() && !tool_name.contains("/action/"))
}

pub(crate) fn parse_tool_action_schema_resource_uri(uri: &str) -> Option<(String, String)> {
    let (base, _) = split_resource_uri(uri);
    let rest = base.strip_prefix("prism://schema/tool/")?;
    let (tool_name, action) = rest.split_once("/action/")?;
    let tool_name = percent_decode_lossy(tool_name);
    let action = percent_decode_lossy(action);
    if tool_name.trim().is_empty() || action.trim().is_empty() || action.contains("/variant/") {
        return None;
    }
    Some((tool_name, action))
}

pub(crate) fn paginate_items<T>(items: Vec<T>, request: ResourcePageRequest) -> PageSlice<T> {
    let total = items.len();
    let start = request.offset.min(total);
    let end = start.saturating_add(request.limit).min(total);
    let has_more = end < total;
    let next_cursor = has_more.then(|| end.to_string());
    let items = items.into_iter().skip(start).take(request.limit).collect();
    let page = ResourcePageView {
        cursor: (request.offset > 0).then(|| request.offset.to_string()),
        next_cursor,
        limit: request.limit,
        returned: end.saturating_sub(start),
        total,
        has_more,
        limit_capped: request.limit_capped,
    };
    PageSlice {
        truncated: page.has_more || page.limit_capped,
        items,
        page,
    }
}

pub(crate) fn parse_symbol_resource_uri(uri: &str) -> Result<Option<NodeId>, McpError> {
    let (base, _) = split_resource_uri(uri);
    let Some(rest) = base.strip_prefix("prism://symbol/") else {
        return Ok(None);
    };
    let mut segments = rest.splitn(3, '/');
    let Some(crate_name) = segments.next() else {
        return Ok(None);
    };
    let Some(kind) = segments.next() else {
        return Ok(None);
    };
    let Some(path) = segments.next() else {
        return Ok(None);
    };
    let crate_name = percent_decode_lossy(crate_name);
    let kind = percent_decode_lossy(kind);
    let path = percent_decode_lossy(path);
    let kind = parse_node_kind(&kind).map_err(|err| {
        McpError::invalid_params(
            "invalid symbol resource uri",
            Some(json!({
                "uri": uri,
                "error": err.to_string(),
            })),
        )
    })?;
    Ok(Some(NodeId::new(crate_name, path, kind)))
}

pub(crate) fn parse_search_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://search/")
        .map(percent_decode_lossy)
        .filter(|query| !query.trim().is_empty())
}

pub(crate) fn parse_file_resource_uri(uri: &str) -> Result<Option<crate::FileReadArgs>, McpError> {
    let (base, query) = split_resource_uri(uri);
    let Some(path) = base.strip_prefix("prism://file/") else {
        return Ok(None);
    };
    let path = percent_decode_lossy(path);
    if path.trim().is_empty() {
        return Ok(None);
    }

    let mut args = crate::FileReadArgs {
        path,
        start_line: None,
        end_line: None,
        max_chars: None,
    };

    if let Some(query) = query {
        for part in query.split('&').filter(|part| !part.is_empty()) {
            let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
            let key = percent_decode_lossy(raw_key);
            let value = percent_decode_lossy(raw_value);
            match key.as_str() {
                "startLine" => {
                    args.start_line = Some(value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid file resource startLine",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?);
                }
                "endLine" => {
                    args.end_line = Some(value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid file resource endLine",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?);
                }
                "maxChars" => {
                    args.max_chars = Some(value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid file resource maxChars",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?);
                }
                _ => {}
            }
        }
    }

    Ok(Some(args))
}

pub(crate) fn parse_resource_query_param(uri: &str, name: &str) -> Option<String> {
    let (_, query) = split_resource_uri(uri);
    query.and_then(|query| {
        query
            .split('&')
            .filter(|part| !part.is_empty())
            .find_map(|part| {
                let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
                (percent_decode_lossy(raw_key) == name).then(|| percent_decode_lossy(raw_value))
            })
    })
}

pub(crate) fn parse_lineage_resource_uri(uri: &str) -> Option<LineageId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://lineage/")
        .map(percent_decode_lossy)
        .map(LineageId::new)
}

pub(crate) fn parse_plan_resource_uri(uri: &str) -> Option<PlanId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://plan/")
        .map(percent_decode_lossy)
        .map(PlanId::new)
}

pub(crate) fn parse_task_resource_uri(uri: &str) -> Option<TaskId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://task/")
        .map(percent_decode_lossy)
        .map(TaskId::new)
}

pub(crate) fn parse_event_resource_uri(uri: &str) -> Option<EventId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://event/")
        .map(percent_decode_lossy)
        .map(EventId::new)
}

pub(crate) fn parse_memory_resource_uri(uri: &str) -> Option<MemoryId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://memory/")
        .map(percent_decode_lossy)
        .map(MemoryId)
}

pub(crate) fn parse_edge_resource_uri(uri: &str) -> Option<EdgeId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://edge/")
        .map(percent_decode_lossy)
        .map(EdgeId)
}

pub(crate) fn resource_link_view(
    uri: String,
    name: impl Into<String>,
    description: impl Into<String>,
) -> ResourceLinkView {
    ResourceLinkView {
        uri,
        name: name.into(),
        description: Some(description.into()),
    }
}

pub(crate) fn dedupe_resource_link_views(links: Vec<ResourceLinkView>) -> Vec<ResourceLinkView> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(links.len());
    for link in links {
        if seen.insert(link.uri.clone()) {
            deduped.push(link);
        }
    }
    deduped
}

pub(crate) fn session_resource_uri() -> String {
    SESSION_URI.to_string()
}

pub(crate) fn capabilities_resource_uri() -> String {
    CAPABILITIES_URI.to_string()
}

pub(crate) fn protected_state_resource_uri() -> String {
    PROTECTED_STATE_URI.to_string()
}

pub(crate) fn protected_state_resource_uri_with_options(stream: Option<&str>) -> String {
    let mut uri = protected_state_resource_uri();
    if let Some(stream) = stream.filter(|value| !value.is_empty()) {
        uri.push_str("?stream=");
        uri.push_str(&percent_encode_component(stream));
    }
    uri
}

pub(crate) fn plans_resource_uri() -> String {
    PLANS_URI.to_string()
}

pub(crate) fn contracts_resource_uri() -> String {
    CONTRACTS_URI.to_string()
}

pub(crate) fn vocab_resource_uri() -> String {
    VOCAB_URI.to_string()
}

pub(crate) fn schemas_resource_uri() -> String {
    SCHEMAS_URI.to_string()
}

pub(crate) fn tool_schemas_resource_uri() -> String {
    TOOL_SCHEMAS_URI.to_string()
}

pub(crate) fn schema_resource_uri(resource_kind: &str) -> String {
    format!("prism://schema/{}", percent_encode_component(resource_kind))
}

pub(crate) fn plans_resource_uri_with_options(
    status: Option<&str>,
    scope: Option<&str>,
    contains: Option<&str>,
    sort: Option<&str>,
) -> String {
    let mut uri = plans_resource_uri();
    let mut params = Vec::new();
    if let Some(status) = status.filter(|value| !value.is_empty()) {
        params.push(format!("status={}", percent_encode_component(status)));
    }
    if let Some(scope) = scope.filter(|value| !value.is_empty()) {
        params.push(format!("scope={}", percent_encode_component(scope)));
    }
    if let Some(contains) = contains.filter(|value| !value.is_empty()) {
        params.push(format!("contains={}", percent_encode_component(contains)));
    }
    if let Some(sort) = sort.filter(|value| !value.is_empty()) {
        params.push(format!("sort={}", percent_encode_component(sort)));
    }
    if !params.is_empty() {
        uri.push('?');
        uri.push_str(&params.join("&"));
    }
    uri
}

pub(crate) fn contracts_resource_uri_with_options(
    contains: Option<&str>,
    status: Option<&str>,
    scope: Option<&str>,
    kind: Option<&str>,
) -> String {
    let mut uri = contracts_resource_uri();
    let mut params = Vec::new();
    if let Some(contains) = contains.filter(|value| !value.is_empty()) {
        params.push(format!("contains={}", percent_encode_component(contains)));
    }
    if let Some(status) = status.filter(|value| !value.is_empty()) {
        params.push(format!("status={}", percent_encode_component(status)));
    }
    if let Some(scope) = scope.filter(|value| !value.is_empty()) {
        params.push(format!("scope={}", percent_encode_component(scope)));
    }
    if let Some(kind) = kind.filter(|value| !value.is_empty()) {
        params.push(format!("kind={}", percent_encode_component(kind)));
    }
    if !params.is_empty() {
        uri.push('?');
        uri.push_str(&params.join("&"));
    }
    uri
}

pub(crate) fn tool_schema_resource_uri(tool_name: &str) -> String {
    format!(
        "prism://schema/tool/{}",
        percent_encode_component(tool_name)
    )
}

pub(crate) fn tool_action_schema_resource_uri(tool_name: &str, action: &str) -> String {
    format!(
        "prism://schema/tool/{}/action/{}",
        percent_encode_component(tool_name),
        percent_encode_component(action)
    )
}

pub(crate) fn plan_resource_uri(plan_id: &str) -> String {
    format!("prism://plan/{}", percent_encode_component(plan_id))
}

pub(crate) fn task_resource_uri(task_id: &str) -> String {
    format!("prism://task/{}", percent_encode_component(task_id))
}

pub(crate) fn search_resource_uri(query: &str) -> String {
    format!("prism://search/{}", percent_encode_component(query))
}

pub(crate) fn search_resource_uri_with_options(
    query: &str,
    strategy: Option<&str>,
    owner_kind: Option<&str>,
    kind: Option<&str>,
    path: Option<&str>,
    module: Option<&str>,
    task_id: Option<&str>,
    path_mode: Option<&str>,
    structured_path: Option<&str>,
    top_level_only: Option<bool>,
    prefer_callable_code: Option<bool>,
    prefer_editable_targets: Option<bool>,
    prefer_behavioral_owners: Option<bool>,
    include_inferred: Option<bool>,
) -> String {
    let mut uri = search_resource_uri(query);
    let mut params = Vec::new();
    if let Some(strategy) = strategy.filter(|value| !value.is_empty()) {
        params.push(format!("strategy={}", percent_encode_component(strategy)));
    }
    if let Some(owner_kind) = owner_kind.filter(|value| !value.is_empty()) {
        params.push(format!(
            "ownerKind={}",
            percent_encode_component(owner_kind)
        ));
    }
    if let Some(kind) = kind.filter(|value| !value.is_empty()) {
        params.push(format!("kind={}", percent_encode_component(kind)));
    }
    if let Some(path) = path.filter(|value| !value.is_empty()) {
        params.push(format!("path={}", percent_encode_component(path)));
    }
    if let Some(module) = module.filter(|value| !value.is_empty()) {
        params.push(format!("module={}", percent_encode_component(module)));
    }
    if let Some(task_id) = task_id.filter(|value| !value.is_empty()) {
        params.push(format!("taskId={}", percent_encode_component(task_id)));
    }
    if let Some(path_mode) = path_mode.filter(|value| !value.is_empty()) {
        params.push(format!("pathMode={}", percent_encode_component(path_mode)));
    }
    if let Some(structured_path) = structured_path.filter(|value| !value.is_empty()) {
        params.push(format!(
            "structuredPath={}",
            percent_encode_component(structured_path)
        ));
    }
    if let Some(top_level_only) = top_level_only {
        params.push(format!("topLevelOnly={top_level_only}"));
    }
    if let Some(prefer_callable_code) = prefer_callable_code {
        params.push(format!("preferCallableCode={prefer_callable_code}"));
    }
    if let Some(prefer_editable_targets) = prefer_editable_targets {
        params.push(format!("preferEditableTargets={prefer_editable_targets}"));
    }
    if let Some(prefer_behavioral_owners) = prefer_behavioral_owners {
        params.push(format!("preferBehavioralOwners={prefer_behavioral_owners}"));
    }
    if let Some(include_inferred) = include_inferred {
        params.push(format!("includeInferred={include_inferred}"));
    }
    if !params.is_empty() {
        uri.push('?');
        uri.push_str(&params.join("&"));
    }
    uri
}

pub(crate) fn lineage_resource_uri(lineage_id: &str) -> String {
    format!("prism://lineage/{}", percent_encode_component(lineage_id))
}

pub(crate) fn event_resource_uri(event_id: &str) -> String {
    format!("prism://event/{}", percent_encode_component(event_id))
}

pub(crate) fn memory_resource_uri(memory_id: &str) -> String {
    format!("prism://memory/{}", percent_encode_component(memory_id))
}

pub(crate) fn edge_resource_uri(edge_id: &str) -> String {
    format!("prism://edge/{}", percent_encode_component(edge_id))
}

pub(crate) fn resource_meta(
    resource_kind: &str,
    schema_uri: Option<String>,
    target_resource_kind: Option<&str>,
) -> Meta {
    let mut prism_meta = serde_json::Map::new();
    prism_meta.insert(
        "resourceKind".to_string(),
        Value::String(resource_kind.to_string()),
    );
    if let Some(schema_uri) = schema_uri {
        prism_meta.insert("schemaUri".to_string(), Value::String(schema_uri));
    }
    if let Some(target_resource_kind) = target_resource_kind {
        prism_meta.insert(
            "targetResourceKind".to_string(),
            Value::String(target_resource_kind.to_string()),
        );
    }
    let mut meta = serde_json::Map::new();
    meta.insert("prism".to_string(), Value::Object(prism_meta));
    Meta(meta)
}

pub(crate) fn session_resource_link() -> RawResource {
    RawResource::new(session_resource_uri(), "PRISM Session")
        .with_description("Active workspace root, current task context, and runtime query limits")
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "session",
            Some(schema_resource_uri("session")),
            None,
        ))
}

pub(crate) fn instructions_resource_link() -> RawResource {
    crate::instructions::instructions_resource_link()
}

pub(crate) fn instruction_set_resource_links(
    runtime_mode: prism_core::PrismRuntimeMode,
) -> Vec<RawResource> {
    crate::instructions::instruction_set_resource_links(runtime_mode)
}

pub(crate) fn capabilities_resource_link() -> RawResource {
    RawResource::new(capabilities_resource_uri(), "PRISM Capabilities")
        .with_description(
            "Canonical capability map for query methods, resources, features, and build info",
        )
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "capabilities",
            Some(schema_resource_uri("capabilities")),
            None,
        ))
}

pub(crate) fn protected_state_resource_link() -> RawResource {
    RawResource::new(protected_state_resource_uri(), "PRISM Protected State")
        .with_description(
            "Protected .prism stream verification status, trust diagnostics, and repair guidance",
        )
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "protected-state",
            Some(schema_resource_uri("protected-state")),
            None,
        ))
}

pub(crate) fn plans_resource_link() -> RawResource {
    RawResource::new(plans_resource_uri(), "PRISM Plans")
        .with_description(
            "Browse published and runtime-hydrated plans with compact progress summaries",
        )
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "plans",
            Some(schema_resource_uri("plans")),
            None,
        ))
}

pub(crate) fn contracts_resource_link() -> RawResource {
    RawResource::new(contracts_resource_uri(), "PRISM Contracts")
        .with_description("Browse contract packets with compact status and promise metadata")
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "contracts",
            Some(schema_resource_uri("contracts")),
            None,
        ))
}

pub(crate) fn task_resource_link(task_id: &str) -> RawResource {
    RawResource::new(task_resource_uri(task_id), "PRISM Task Replay")
        .with_description("Task-scoped outcome timeline and correlated events")
        .with_mime_type("application/json")
        .with_meta(resource_meta(
            "task",
            Some(schema_resource_uri("task")),
            None,
        ))
}

pub(crate) fn event_resource_link(event_id: &str) -> RawResource {
    RawResource::new(
        event_resource_uri(event_id),
        format!("PRISM Event: {event_id}"),
    )
    .with_description("Recorded outcome event and associated task metadata")
    .with_mime_type("application/json")
    .with_meta(resource_meta(
        "event",
        Some(schema_resource_uri("event")),
        None,
    ))
}

pub(crate) fn memory_resource_link(memory_id: &str) -> RawResource {
    RawResource::new(
        memory_resource_uri(memory_id),
        format!("PRISM Memory: {memory_id}"),
    )
    .with_description("Stored episodic memory entry and associated task metadata")
    .with_mime_type("application/json")
    .with_meta(resource_meta(
        "memory",
        Some(schema_resource_uri("memory")),
        None,
    ))
}

pub(crate) fn edge_resource_link(edge_id: &str) -> RawResource {
    RawResource::new(
        edge_resource_uri(edge_id),
        format!("PRISM Inferred Edge: {edge_id}"),
    )
    .with_description("Inferred-edge record with scope, evidence, and task metadata")
    .with_mime_type("application/json")
    .with_meta(resource_meta(
        "edge",
        Some(schema_resource_uri("edge")),
        None,
    ))
}

pub(crate) fn schemas_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        schemas_resource_uri(),
        "PRISM Resource Schemas",
        "Catalog of JSON Schemas for every structured PRISM resource payload",
    )
}

pub(crate) fn instructions_resource_view_link() -> ResourceLinkView {
    crate::instructions::instructions_resource_view_link()
}

pub(crate) fn capabilities_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        capabilities_resource_uri(),
        "PRISM Capabilities",
        "Canonical capability map for query methods, resources, features, and build info",
    )
}

pub(crate) fn protected_state_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        protected_state_resource_uri(),
        "PRISM Protected State",
        "Protected .prism stream verification status, trust diagnostics, and repair guidance",
    )
}

pub(crate) fn plans_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        plans_resource_uri(),
        "PRISM Plans",
        "Browse published and runtime-hydrated plans with compact progress summaries",
    )
}

pub(crate) fn vocab_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        vocab_resource_uri(),
        "PRISM Vocabulary",
        "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads",
    )
}

pub(crate) fn tool_schemas_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        tool_schemas_resource_uri(),
        "PRISM Tool Schemas",
        "Catalog of JSON Schemas for PRISM MCP tool input payloads",
    )
}

pub(crate) fn schema_resource_view_link(resource_kind: &str) -> ResourceLinkView {
    resource_link_view(
        schema_resource_uri(resource_kind),
        format!("PRISM Schema: {resource_kind}"),
        format!("JSON Schema for the `{resource_kind}` PRISM resource payload"),
    )
}

pub(crate) fn tool_schema_resource_view_link(tool_name: &str) -> ResourceLinkView {
    resource_link_view(
        tool_schema_resource_uri(tool_name),
        format!("PRISM Tool Schema: {tool_name}"),
        format!("JSON Schema for the `{tool_name}` tool input payload"),
    )
}

pub(crate) fn tool_action_schema_resource_view_link(
    tool_name: &str,
    action: &str,
) -> ResourceLinkView {
    resource_link_view(
        tool_action_schema_resource_uri(tool_name, action),
        format!("PRISM Tool Action Schema: {tool_name}.{action}"),
        format!("Exact action schema for `{tool_name}` action `{action}`"),
    )
}

pub(crate) fn session_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        session_resource_uri(),
        "PRISM Session",
        "Active workspace root, current task context, and runtime query limits",
    )
}

pub(crate) fn instructions_resource_uri() -> String {
    crate::instructions::instructions_resource_uri()
}

pub(crate) fn instruction_set_resource_uri(id: &str) -> String {
    crate::instructions::instruction_set_resource_uri(id)
}

pub(crate) fn plans_resource_view_link_with_options(
    status: Option<&str>,
    scope: Option<&str>,
    contains: Option<&str>,
    sort: Option<&str>,
) -> ResourceLinkView {
    resource_link_view(
        plans_resource_uri_with_options(status, scope, contains, sort),
        "PRISM Plans",
        "Browse published and runtime-hydrated plans with compact progress summaries",
    )
}

pub(crate) fn contracts_resource_view_link_with_options(
    contains: Option<&str>,
    status: Option<&str>,
    scope: Option<&str>,
    kind: Option<&str>,
) -> ResourceLinkView {
    resource_link_view(
        contracts_resource_uri_with_options(contains, status, scope, kind),
        "PRISM Contracts",
        "Browse contract packets with compact status and promise metadata",
    )
}

pub(crate) fn task_resource_view_link(task_id: &str) -> ResourceLinkView {
    resource_link_view(
        task_resource_uri(task_id),
        "PRISM Task Replay",
        "Task-scoped outcome timeline and correlated events",
    )
}

pub(crate) fn plan_resource_view_link(plan_id: &str) -> ResourceLinkView {
    resource_link_view(
        plan_resource_uri(plan_id),
        format!("PRISM Plan: {plan_id}"),
        "Coordination plan detail with root nodes and progress summary",
    )
}

pub(crate) fn search_resource_view_link_with_options(
    query: &str,
    strategy: Option<&str>,
    owner_kind: Option<&str>,
    kind: Option<&str>,
    path: Option<&str>,
    module: Option<&str>,
    task_id: Option<&str>,
    path_mode: Option<&str>,
    structured_path: Option<&str>,
    top_level_only: Option<bool>,
    prefer_callable_code: Option<bool>,
    prefer_editable_targets: Option<bool>,
    prefer_behavioral_owners: Option<bool>,
    include_inferred: Option<bool>,
) -> ResourceLinkView {
    resource_link_view(
        search_resource_uri_with_options(
            query,
            strategy,
            owner_kind,
            kind,
            path,
            module,
            task_id,
            path_mode,
            structured_path,
            top_level_only,
            prefer_callable_code,
            prefer_editable_targets,
            prefer_behavioral_owners,
            include_inferred,
        ),
        format!("PRISM Search: {query}"),
        "Structured search results and diagnostics for this query",
    )
}

pub(crate) fn file_resource_uri(path: &str) -> String {
    format!("prism://file/{}", percent_encode_component(path))
}

pub(crate) fn file_resource_uri_with_options(
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
    max_chars: Option<usize>,
) -> String {
    let mut uri = file_resource_uri(path);
    let mut params = Vec::new();
    if let Some(start_line) = start_line {
        params.push(format!("startLine={start_line}"));
    }
    if let Some(end_line) = end_line {
        params.push(format!("endLine={end_line}"));
    }
    if let Some(max_chars) = max_chars {
        params.push(format!("maxChars={max_chars}"));
    }
    if !params.is_empty() {
        uri.push('?');
        uri.push_str(&params.join("&"));
    }
    uri
}

pub(crate) fn file_resource_view_link(path: &str) -> ResourceLinkView {
    resource_link_view(
        file_resource_uri(path),
        format!("PRISM File: {path}"),
        "Workspace file excerpt with optional line-range narrowing",
    )
}

pub(crate) fn symbol_resource_view_link(symbol: &SymbolView) -> ResourceLinkView {
    resource_link_view(
        symbol_resource_uri(&symbol.id),
        format!("PRISM Symbol: {}", symbol.id.path),
        "Exact symbol snapshot with relations, lineage, and risk context",
    )
}

pub(crate) fn symbol_resource_view_link_for_id(id: &NodeId) -> ResourceLinkView {
    resource_link_view(
        symbol_resource_uri_from_node_id(id),
        format!("PRISM Symbol: {}", id.path),
        "Exact symbol snapshot with relations, lineage, and risk context",
    )
}

pub(crate) fn lineage_resource_view_link(lineage_id: &str) -> ResourceLinkView {
    resource_link_view(
        lineage_resource_uri(lineage_id),
        format!("PRISM Lineage: {lineage_id}"),
        "Structured lineage history and current nodes",
    )
}

pub(crate) fn event_resource_view_link(event_id: &str) -> ResourceLinkView {
    resource_link_view(
        event_resource_uri(event_id),
        format!("PRISM Event: {event_id}"),
        "Recorded outcome event and associated task metadata",
    )
}

pub(crate) fn memory_resource_view_link(memory_id: &str) -> ResourceLinkView {
    resource_link_view(
        memory_resource_uri(memory_id),
        format!("PRISM Memory: {memory_id}"),
        "Stored episodic memory entry and associated task metadata",
    )
}

pub(crate) fn edge_resource_view_link(edge_id: &str) -> ResourceLinkView {
    resource_link_view(
        edge_resource_uri(edge_id),
        format!("PRISM Inferred Edge: {edge_id}"),
        "Inferred-edge record with scope, evidence, and task metadata",
    )
}

pub(crate) fn symbol_resource_uri(id: &NodeIdView) -> String {
    format!(
        "prism://symbol/{}/{}/{}",
        percent_encode_component(&id.crate_name),
        percent_encode_component(&id.kind.to_string()),
        percent_encode_component(&id.path),
    )
}

pub(crate) fn symbol_resource_uri_from_node_id(id: &NodeId) -> String {
    format!(
        "prism://symbol/{}/{}/{}",
        percent_encode_component(id.crate_name.as_str()),
        percent_encode_component(&id.kind.to_string()),
        percent_encode_component(id.path.as_str()),
    )
}

pub(crate) fn resource_schema_catalog_entries() -> Vec<ResourceSchemaCatalogEntry> {
    static RESOURCE_SCHEMA_CATALOG_CACHE: OnceLock<Vec<ResourceSchemaCatalogEntry>> =
        OnceLock::new();
    RESOURCE_SCHEMA_CATALOG_CACHE
        .get_or_init(|| {
            vec![
        ResourceSchemaCatalogEntry {
            resource_kind: "capabilities".to_string(),
            schema_uri: schema_resource_uri("capabilities"),
            resource_uri: Some(CAPABILITIES_URI.to_string()),
            example_uri: resource_example_uri("capabilities"),
            shape_uri: Some(resource_shape_resource_uri("capabilities")),
            description:
                "Schema for the canonical PRISM capability map, including methods, resources, and build info."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "schemas".to_string(),
            schema_uri: schema_resource_uri("schemas"),
            resource_uri: Some(SCHEMAS_URI.to_string()),
            example_uri: resource_example_uri("schemas"),
            shape_uri: Some(resource_shape_resource_uri("schemas")),
            description: "Schema for the JSON Schema catalog resource itself.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "vocab".to_string(),
            schema_uri: schema_resource_uri("vocab"),
            resource_uri: Some(VOCAB_URI.to_string()),
            example_uri: resource_example_uri("vocab"),
            shape_uri: Some(resource_shape_resource_uri("vocab")),
            description:
                "Schema for the canonical PRISM vocabulary catalog, including enums, actions, and allowed values."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "session".to_string(),
            schema_uri: schema_resource_uri("session"),
            resource_uri: Some(SESSION_URI.to_string()),
            example_uri: resource_example_uri("session"),
            shape_uri: Some(resource_shape_resource_uri("session")),
            description: "Schema for the active workspace, task context, and runtime limits."
                .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "protected-state".to_string(),
            schema_uri: schema_resource_uri("protected-state"),
            resource_uri: Some(PROTECTED_STATE_URI.to_string()),
            example_uri: resource_example_uri("protected-state"),
            shape_uri: Some(resource_shape_resource_uri("protected-state")),
            description:
                "Schema for protected .prism stream verification status, trust diagnostics, and repair hints."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "tool-schemas".to_string(),
            schema_uri: schema_resource_uri("tool-schemas"),
            resource_uri: Some(TOOL_SCHEMAS_URI.to_string()),
            example_uri: resource_example_uri("tool-schemas"),
            shape_uri: Some(resource_shape_resource_uri("tool-schemas")),
            description: "Schema for the tool-schema catalog resource.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "plans".to_string(),
            schema_uri: schema_resource_uri("plans"),
            resource_uri: Some(PLANS_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("plans"),
            shape_uri: Some(resource_shape_resource_uri("plans")),
            description:
                "Schema for compact plan discovery results, filters, and pagination metadata."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "plan".to_string(),
            schema_uri: schema_resource_uri("plan"),
            resource_uri: Some(PLAN_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("plan"),
            shape_uri: Some(resource_shape_resource_uri("plan")),
            description: "Schema for a single coordination plan detail resource.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "contracts".to_string(),
            schema_uri: schema_resource_uri("contracts"),
            resource_uri: Some(CONTRACTS_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("contracts"),
            shape_uri: Some(resource_shape_resource_uri("contracts")),
            description:
                "Schema for contract discovery results, promise metadata, and pagination."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "entrypoints".to_string(),
            schema_uri: schema_resource_uri("entrypoints"),
            resource_uri: Some(ENTRYPOINTS_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("entrypoints"),
            shape_uri: Some(resource_shape_resource_uri("entrypoints")),
            description:
                "Schema for the workspace entrypoint overview and its pagination metadata."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "search".to_string(),
            schema_uri: schema_resource_uri("search"),
            resource_uri: Some(SEARCH_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("search"),
            shape_uri: Some(resource_shape_resource_uri("search")),
            description: "Schema for browseable search results and diagnostics.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "file".to_string(),
            schema_uri: schema_resource_uri("file"),
            resource_uri: Some(FILE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("file"),
            shape_uri: Some(resource_shape_resource_uri("file")),
            description: "Schema for read-only workspace file excerpts addressed by path."
                .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "symbol".to_string(),
            schema_uri: schema_resource_uri("symbol"),
            resource_uri: Some(SYMBOL_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("symbol"),
            shape_uri: Some(resource_shape_resource_uri("symbol")),
            description:
                "Schema for exact symbol snapshots, including relations, lineage, and risk context."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "lineage".to_string(),
            schema_uri: schema_resource_uri("lineage"),
            resource_uri: Some(LINEAGE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("lineage"),
            shape_uri: Some(resource_shape_resource_uri("lineage")),
            description: "Schema for lineage history and current-node views.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "task".to_string(),
            schema_uri: schema_resource_uri("task"),
            resource_uri: Some(TASK_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("task"),
            shape_uri: Some(resource_shape_resource_uri("task")),
            description: "Schema for task replay pages and correlated outcome events.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "event".to_string(),
            schema_uri: schema_resource_uri("event"),
            resource_uri: Some(EVENT_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("event"),
            shape_uri: Some(resource_shape_resource_uri("event")),
            description: "Schema for a single recorded outcome event.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "memory".to_string(),
            schema_uri: schema_resource_uri("memory"),
            resource_uri: Some(MEMORY_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("memory"),
            shape_uri: Some(resource_shape_resource_uri("memory")),
            description: "Schema for a single episodic memory entry.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "edge".to_string(),
            schema_uri: schema_resource_uri("edge"),
            resource_uri: Some(EDGE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: resource_example_uri("edge"),
            shape_uri: Some(resource_shape_resource_uri("edge")),
            description: "Schema for a single inferred-edge record.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "tool-example".to_string(),
            schema_uri: schema_resource_uri("tool-example"),
            resource_uri: Some(crate::TOOL_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(crate::tool_example_resource_uri("prism_code")),
            shape_uri: Some(resource_shape_resource_uri("tool-example")),
            description: "Schema for compact tool example companion resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "tool-shape".to_string(),
            schema_uri: schema_resource_uri("tool-shape"),
            resource_uri: Some(crate::TOOL_SHAPE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(crate::tool_shape_resource_uri("prism_code")),
            shape_uri: Some(resource_shape_resource_uri("tool-shape")),
            description: "Schema for compact tool shape companion resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "resource-example".to_string(),
            schema_uri: schema_resource_uri("resource-example"),
            resource_uri: Some(RESOURCE_EXAMPLE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(resource_example_resource_uri("search")),
            shape_uri: Some(resource_shape_resource_uri("resource-example")),
            description: "Schema for compact resource example companion resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "resource-shape".to_string(),
            schema_uri: schema_resource_uri("resource-shape"),
            resource_uri: Some(RESOURCE_SHAPE_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(resource_shape_resource_uri("search")),
            shape_uri: Some(resource_shape_resource_uri("resource-shape")),
            description: "Schema for compact resource shape companion resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "capabilities-section".to_string(),
            schema_uri: schema_resource_uri("capabilities-section"),
            resource_uri: Some(CAPABILITIES_SECTION_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(capabilities_section_resource_uri("tools")),
            shape_uri: Some(resource_shape_resource_uri("capabilities-section")),
            description: "Schema for segmented capabilities section resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "vocab-entry".to_string(),
            schema_uri: schema_resource_uri("vocab-entry"),
            resource_uri: Some(VOCAB_ENTRY_RESOURCE_TEMPLATE_URI.to_string()),
            example_uri: Some(vocab_entry_resource_uri("coordinationMutationKind")),
            shape_uri: Some(resource_shape_resource_uri("vocab-entry")),
            description: "Schema for segmented vocabulary entry resources.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "self-description-audit".to_string(),
            schema_uri: schema_resource_uri("self-description-audit"),
            resource_uri: Some(SELF_DESCRIPTION_AUDIT_URI.to_string()),
            example_uri: Some(SELF_DESCRIPTION_AUDIT_URI.to_string()),
            shape_uri: Some(resource_shape_resource_uri("self-description-audit")),
            description: "Schema for the self-description audit resource.".to_string(),
        },
            ]
        })
        .clone()
}

pub(crate) fn anchor_resource_view_links(
    prism: &prism_query::Prism,
    workspace_root: Option<&Path>,
    anchors: &[AnchorRef],
) -> Vec<ResourceLinkView> {
    let mut links = Vec::new();
    for anchor in anchors {
        match anchor {
            AnchorRef::Node(id) => links.push(symbol_resource_view_link_for_id(id)),
            AnchorRef::Lineage(lineage_id) => {
                links.push(lineage_resource_view_link(lineage_id.0.as_str()))
            }
            AnchorRef::File(file_id) => {
                if let Some(path) = prism.graph().file_path(*file_id) {
                    links.push(file_resource_view_link(&workspace_display_path(
                        workspace_root,
                        path,
                    )));
                }
            }
            AnchorRef::WorkspacePath(path) => links.push(file_resource_view_link(path)),
            AnchorRef::Kind(_) => {}
        }
    }
    dedupe_resource_link_views(links)
}

pub(crate) fn task_resource_view_links_from_events(
    events: &[OutcomeEvent],
) -> Vec<ResourceLinkView> {
    dedupe_resource_link_views(
        events
            .iter()
            .filter_map(|event| event.meta.correlation.as_ref())
            .map(|task_id| task_resource_view_link(task_id.0.as_str()))
            .collect(),
    )
}

pub(crate) fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            decoded.push(b' ');
        } else {
            decoded.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

pub(crate) fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}
