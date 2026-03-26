use std::time::{SystemTime, UNIX_EPOCH};

use prism_ir::NodeId;
use prism_query::Prism;
use rmcp::{model::*, ErrorData as McpError};
use serde_json::json;

pub(crate) fn map_query_error(error: anyhow::Error) -> McpError {
    McpError::internal_error(
        "prism query failed",
        Some(json!({
            "code": "query_execution_failed",
            "error": error.to_string(),
        })),
    )
}

pub(crate) fn structured_tool_result<T: serde::Serialize>(
    value: T,
) -> Result<CallToolResult, McpError> {
    structured_tool_result_with_links(value, Vec::new())
}

pub(crate) fn structured_tool_result_with_links<T: serde::Serialize>(
    value: T,
    links: Vec<RawResource>,
) -> Result<CallToolResult, McpError> {
    let value = serde_json::to_value(value).map_err(|err| {
        McpError::internal_error(
            "failed to serialize structured tool result",
            Some(json!({ "error": err.to_string() })),
        )
    })?;
    let mut result = CallToolResult::structured(value);
    result
        .content
        .extend(links.into_iter().map(Content::resource_link));
    Ok(result)
}

pub(crate) fn merge_node_ids<I>(mut base: Vec<NodeId>, extra: I) -> Vec<NodeId>
where
    I: IntoIterator<Item = NodeId>,
{
    base.extend(extra);
    base.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    base.dedup();
    base
}

pub(crate) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn max_event_sequence(prism: &Prism) -> u64 {
    prism
        .outcome_snapshot()
        .events
        .into_iter()
        .filter_map(|event| event.meta.id.0.rsplit(':').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

pub(crate) fn max_task_sequence(prism: &Prism) -> u64 {
    prism
        .outcome_snapshot()
        .events
        .into_iter()
        .filter_map(|event| event.meta.correlation)
        .map(|task| task.0.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64
}
