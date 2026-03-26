use prism_agent::EdgeId;
use prism_ir::{AnchorRef, EventId, LineageId, NodeId, TaskId};
use prism_js::{NodeIdView, SymbolView};
use prism_memory::{MemoryId, OutcomeEvent};
use rmcp::{
    model::{Meta, RawResource, ResourceContents},
    schemars::JsonSchema,
    ErrorData as McpError,
};
use serde_json::{json, Value};

use crate::{
    parse_node_kind, ResourceLinkView, ResourcePageView, ResourceSchemaCatalogEntry,
    EDGE_RESOURCE_TEMPLATE_URI, ENTRYPOINTS_RESOURCE_TEMPLATE_URI, EVENT_RESOURCE_TEMPLATE_URI,
    LINEAGE_RESOURCE_TEMPLATE_URI, MEMORY_RESOURCE_TEMPLATE_URI, SCHEMAS_URI,
    SEARCH_RESOURCE_TEMPLATE_URI, SESSION_URI, SYMBOL_RESOURCE_TEMPLATE_URI,
    TASK_RESOURCE_TEMPLATE_URI,
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
    }
    json_resource_contents_with_meta(
        schema,
        schema_uri.to_string(),
        Some(resource_meta("schema", None, Some(target_resource_kind))),
    )
    .map(|contents| contents.with_mime_type("application/schema+json"))
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

pub(crate) fn parse_lineage_resource_uri(uri: &str) -> Option<LineageId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://lineage/")
        .map(percent_decode_lossy)
        .map(LineageId::new)
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

pub(crate) fn dedupe_resource_link_views(
    mut links: Vec<ResourceLinkView>,
) -> Vec<ResourceLinkView> {
    links.sort_by(|left, right| left.uri.cmp(&right.uri));
    links.dedup_by(|left, right| left.uri == right.uri);
    links
}

pub(crate) fn session_resource_uri() -> String {
    SESSION_URI.to_string()
}

pub(crate) fn schemas_resource_uri() -> String {
    SCHEMAS_URI.to_string()
}

pub(crate) fn schema_resource_uri(resource_kind: &str) -> String {
    format!("prism://schema/{}", percent_encode_component(resource_kind))
}

pub(crate) fn task_resource_uri(task_id: &str) -> String {
    format!("prism://task/{}", percent_encode_component(task_id))
}

pub(crate) fn search_resource_uri(query: &str) -> String {
    format!("prism://search/{}", percent_encode_component(query))
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

pub(crate) fn schema_resource_view_link(resource_kind: &str) -> ResourceLinkView {
    resource_link_view(
        schema_resource_uri(resource_kind),
        format!("PRISM Schema: {resource_kind}"),
        format!("JSON Schema for the `{resource_kind}` PRISM resource payload"),
    )
}

pub(crate) fn session_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        session_resource_uri(),
        "PRISM Session",
        "Active workspace root, current task context, and runtime query limits",
    )
}

pub(crate) fn task_resource_view_link(task_id: &str) -> ResourceLinkView {
    resource_link_view(
        task_resource_uri(task_id),
        "PRISM Task Replay",
        "Task-scoped outcome timeline and correlated events",
    )
}

pub(crate) fn search_resource_view_link(query: &str) -> ResourceLinkView {
    resource_link_view(
        search_resource_uri(query),
        format!("PRISM Search: {query}"),
        "Structured search results and diagnostics for this query",
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
    vec![
        ResourceSchemaCatalogEntry {
            resource_kind: "schemas".to_string(),
            schema_uri: schema_resource_uri("schemas"),
            resource_uri: Some(SCHEMAS_URI.to_string()),
            description: "Schema for the JSON Schema catalog resource itself.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "session".to_string(),
            schema_uri: schema_resource_uri("session"),
            resource_uri: Some(SESSION_URI.to_string()),
            description: "Schema for the active workspace, task context, and runtime limits."
                .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "entrypoints".to_string(),
            schema_uri: schema_resource_uri("entrypoints"),
            resource_uri: Some(ENTRYPOINTS_RESOURCE_TEMPLATE_URI.to_string()),
            description:
                "Schema for the workspace entrypoint overview and its pagination metadata."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "search".to_string(),
            schema_uri: schema_resource_uri("search"),
            resource_uri: Some(SEARCH_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for browseable search results and diagnostics.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "symbol".to_string(),
            schema_uri: schema_resource_uri("symbol"),
            resource_uri: Some(SYMBOL_RESOURCE_TEMPLATE_URI.to_string()),
            description:
                "Schema for exact symbol snapshots, including relations, lineage, and risk context."
                    .to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "lineage".to_string(),
            schema_uri: schema_resource_uri("lineage"),
            resource_uri: Some(LINEAGE_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for lineage history and current-node views.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "task".to_string(),
            schema_uri: schema_resource_uri("task"),
            resource_uri: Some(TASK_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for task replay pages and correlated outcome events.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "event".to_string(),
            schema_uri: schema_resource_uri("event"),
            resource_uri: Some(EVENT_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for a single recorded outcome event.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "memory".to_string(),
            schema_uri: schema_resource_uri("memory"),
            resource_uri: Some(MEMORY_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for a single episodic memory entry.".to_string(),
        },
        ResourceSchemaCatalogEntry {
            resource_kind: "edge".to_string(),
            schema_uri: schema_resource_uri("edge"),
            resource_uri: Some(EDGE_RESOURCE_TEMPLATE_URI.to_string()),
            description: "Schema for a single inferred-edge record.".to_string(),
        },
    ]
}

pub(crate) fn anchor_resource_view_links(anchors: &[AnchorRef]) -> Vec<ResourceLinkView> {
    let mut links = Vec::new();
    for anchor in anchors {
        match anchor {
            AnchorRef::Node(id) => links.push(symbol_resource_view_link_for_id(id)),
            AnchorRef::Lineage(lineage_id) => {
                links.push(lineage_resource_view_link(lineage_id.0.as_str()))
            }
            AnchorRef::File(_) | AnchorRef::Kind(_) => {}
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
