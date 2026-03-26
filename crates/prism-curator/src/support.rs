use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use toml::Value as TomlValue;

use crate::types::{CuratorBudget, CuratorContext};

pub(crate) fn curator_run_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["proposals", "diagnostics"],
        "properties": {
            "proposals": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["kind"],
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": [
                                "inferred_edge",
                                "structural_memory",
                                "semantic_memory",
                                "risk_summary",
                                "validation_recipe"
                            ]
                        }
                    },
                    "additionalProperties": true
                }
            },
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["code", "message"],
                    "properties": {
                        "code": { "type": "string" },
                        "message": { "type": "string" },
                        "data": {}
                    }
                }
            }
        }
    })
}

pub(crate) fn bounded_context(ctx: &CuratorContext, budget: &CuratorBudget) -> CuratorContext {
    let mut bounded = ctx.clone();
    if bounded.graph.nodes.len() > budget.max_context_nodes {
        bounded.graph.nodes.truncate(budget.max_context_nodes);
    }
    let max_edges = budget.max_context_nodes.saturating_mul(4).max(1);
    if bounded.graph.edges.len() > max_edges {
        bounded.graph.edges.truncate(max_edges);
    }
    if bounded.outcomes.len() > budget.max_outcomes {
        bounded.outcomes.truncate(budget.max_outcomes);
    }
    if bounded.memories.len() > budget.max_memories {
        bounded.memories.truncate(budget.max_memories);
    }
    if bounded.projections.co_change.len() > budget.max_context_nodes {
        bounded
            .projections
            .co_change
            .truncate(budget.max_context_nodes);
    }
    if bounded.projections.validation_checks.len() > budget.max_context_nodes {
        bounded
            .projections
            .validation_checks
            .truncate(budget.max_context_nodes);
    }
    bounded
}

pub(crate) fn render_toml_value(value: &TomlValue) -> Result<String> {
    match value {
        TomlValue::String(text) => Ok(TomlValue::String(text.clone()).to_string()),
        TomlValue::Integer(number) => Ok(number.to_string()),
        TomlValue::Float(number) => Ok(number.to_string()),
        TomlValue::Boolean(value) => Ok(value.to_string()),
        TomlValue::Datetime(value) => Ok(value.to_string()),
        TomlValue::Array(_) | TomlValue::Table(_) => {
            let rendered = toml::to_string(value)?.trim().to_string();
            if rendered.is_empty() {
                return Err(anyhow!("failed to render TOML override"));
            }
            Ok(rendered)
        }
    }
}

pub(crate) fn unique_temp_dir(prefix: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
