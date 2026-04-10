use std::time::{SystemTime, UNIX_EPOCH};

use prism_core::AdmissionBusyError;
use prism_ir::NodeId;
use rmcp::{model::*, ErrorData as McpError};
use serde_json::json;

use crate::QueryExecutionError;

pub(crate) fn map_query_error(error: anyhow::Error) -> McpError {
    if let Some(admission_error) = error.downcast_ref::<AdmissionBusyError>() {
        return McpError::internal_error(
            admission_error.to_string(),
            Some(json!({
                "code": admission_error.code(),
                "category": "busy",
                "operation": admission_error.operation(),
                "resource": admission_error.resource(),
                "retryable": true,
                "nextAction": admission_error.next_action(),
            })),
        );
    }
    if let Some(query_error) = error.downcast_ref::<QueryExecutionError>() {
        if matches!(
            query_error.code(),
            Some("query_feature_disabled" | "query_invalid_argument")
        ) {
            return McpError::invalid_params(
                query_error.to_string(),
                Some(query_error.data().clone()),
            );
        }
        return McpError::internal_error(query_error.summary(), Some(query_error.data().clone()));
    }
    if let Some(json_error) = error.downcast_ref::<serde_json::Error>() {
        return McpError::invalid_params(
            "prism_code arguments invalid",
            Some(json!({
                "code": "query_invalid_argument",
                "category": "invalid_argument",
                "error": json_error.to_string(),
                "nextAction": "Check the query method argument names, required fields, and value types, then retry.",
            })),
        );
    }
    McpError::internal_error(
        "prism code failed",
        Some(json!({
            "code": "query_execution_failed",
            "error": error.to_string(),
        })),
    )
}

pub(crate) fn map_code_error(error: anyhow::Error) -> McpError {
    if let Some(admission_error) = error.downcast_ref::<AdmissionBusyError>() {
        return McpError::internal_error(
            admission_error.to_string(),
            Some(json!({
                "code": admission_error.code(),
                "category": "busy",
                "operation": admission_error.operation(),
                "resource": admission_error.resource(),
                "retryable": true,
                "nextAction": admission_error.next_action(),
            })),
        );
    }
    if let Some(query_error) = error.downcast_ref::<QueryExecutionError>() {
        if matches!(
            query_error.code(),
            Some("query_feature_disabled" | "query_invalid_argument")
        ) {
            return McpError::invalid_params(query_error.to_string(), Some(query_error.data().clone()));
        }
        return McpError::internal_error(query_error.summary(), Some(query_error.data().clone()));
    }
    if let Some(json_error) = error.downcast_ref::<serde_json::Error>() {
        return McpError::invalid_params(
            "prism_code arguments invalid",
            Some(json!({
                "code": "query_invalid_argument",
                "category": "invalid_argument",
                "error": json_error.to_string(),
                "nextAction": "Check the prism_code argument names, required fields, and value types, then retry.",
            })),
        );
    }
    McpError::internal_error(
        "prism code failed",
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
