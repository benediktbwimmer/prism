use serde_json::{json, Value};

use crate::{
    capabilities_resource_uri, edge_resource_uri, event_resource_uri, memory_resource_uri,
    schema_resource_uri, search_resource_uri_with_options, session_resource_uri,
    symbol_resource_uri_from_node_id, task_resource_uri, tool_schema_resource_uri,
    API_REFERENCE_URI,
};
use prism_ir::{EdgeKind, NodeId};

pub(crate) fn schema_examples(target_resource_kind: &str) -> Option<Vec<Value>> {
    if let Some(tool_name) = target_resource_kind.strip_prefix("tool:") {
        return tool_input_example(tool_name).map(|example| vec![example]);
    }
    resource_payload_example(target_resource_kind).map(|example| vec![example])
}

pub(crate) fn resource_example_uri(resource_kind: &str) -> Option<String> {
    match resource_kind {
        "capabilities" => Some(capabilities_resource_uri()),
        "schemas" => Some("prism://schemas".to_string()),
        "session" => Some(session_resource_uri()),
        "tool-schemas" => Some("prism://tool-schemas".to_string()),
        "entrypoints" => Some("prism://entrypoints?limit=5".to_string()),
        "search" => Some(search_resource_uri_with_options(
            "read context",
            Some("behavioral"),
            Some("read"),
            Some("function"),
            Some("src"),
            Some(true),
        )),
        "symbol" => Some(symbol_resource_uri_from_node_id(&sample_node_id())),
        "lineage" => Some("prism://lineage/lineage%3Ademo%3A%3Amain?limit=5".to_string()),
        "task" => Some(task_resource_uri("task:demo-main")),
        "event" => Some(event_resource_uri("event:demo-main")),
        "memory" => Some(memory_resource_uri("memory:demo-main")),
        "edge" => Some(edge_resource_uri("edge:demo-main-calls-helper")),
        _ => None,
    }
}

pub(crate) fn tool_input_example(tool_name: &str) -> Option<Value> {
    match tool_name {
        "prism_query" => Some(json!({
            "code": "return prism.search(\"read context\", { limit: 5, strategy: \"behavioral\", ownerKind: \"read\" });",
            "language": "ts",
        })),
        "prism_session" => Some(json!({
            "action": "start_task",
            "input": {
                "description": "Investigate the read-context path.",
                "tags": ["mcp", "examples"],
            }
        })),
        "prism_mutate" => Some(json!({
            "action": "validation_feedback",
            "input": {
                "subsystem": "projection",
                "context": "Search resource example slice",
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::main",
                    "kind": "function"
                }],
                "whatPrismSaid": "Search result ordering was helpful.",
                "whatWasTrue": "The top candidate matched the intended read path.",
                "helpful": true,
                "correctedManually": false
            }
        })),
        _ => None,
    }
}

fn resource_payload_example(resource_kind: &str) -> Option<Value> {
    match resource_kind {
        "capabilities" => Some(capabilities_payload_example()),
        "schemas" => Some(resource_schema_catalog_payload_example()),
        "session" => Some(session_payload_example()),
        "tool-schemas" => Some(tool_schema_catalog_payload_example()),
        "entrypoints" => Some(entrypoints_payload_example()),
        "search" => Some(search_payload_example()),
        "symbol" => Some(symbol_payload_example()),
        "lineage" => Some(lineage_payload_example()),
        "task" => Some(task_payload_example()),
        "event" => Some(event_payload_example()),
        "memory" => Some(memory_payload_example()),
        "edge" => Some(edge_payload_example()),
        _ => None,
    }
}

fn session_payload_example() -> Value {
    json!({
        "uri": session_resource_uri(),
        "schemaUri": schema_resource_uri("session"),
        "workspaceRoot": "/workspace/demo",
        "currentTask": {
            "taskId": "task:demo-main",
            "description": "Inspect the read-context flow.",
            "tags": ["mcp", "examples"]
        },
        "currentAgent": "codex",
        "limits": sample_limits(),
        "features": sample_features(),
        "relatedResources": sample_related_resources(),
    })
}

fn capabilities_payload_example() -> Value {
    json!({
        "uri": capabilities_resource_uri(),
        "schemaUri": schema_resource_uri("capabilities"),
        "build": {
            "serverName": "prism-mcp",
            "serverVersion": "0.1.0",
            "protocolVersion": "2025-06-18",
            "workspaceRevision": {
                "graphVersion": 42,
                "gitCommit": "abc123def456"
            },
            "apiReferenceUri": API_REFERENCE_URI,
        },
        "features": sample_features(),
        "queryMethods": [{
            "name": "readContext",
            "enabled": true,
            "group": "core",
            "featureGate": null,
            "description": "Build a semantic read bundle for a target."
        }, {
            "name": "validationContext",
            "enabled": true,
            "group": "core",
            "featureGate": null,
            "description": "Build a validation-focused bundle for a target."
        }],
        "resources": [{
            "name": "PRISM Session",
            "uri": session_resource_uri(),
            "mimeType": "application/json",
            "description": "Active workspace root, task context, limits, and feature flags.",
            "schemaUri": schema_resource_uri("session"),
            "exampleUri": session_resource_uri(),
        }],
        "resourceTemplates": [{
            "name": "PRISM Search",
            "uriTemplate": "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&includeInferred={includeInferred}",
            "mimeType": "application/json",
            "description": "Read structured search results and diagnostics for a query.",
            "exampleUri": resource_example_uri("search"),
        }],
        "tools": [{
            "name": "prism_query",
            "description": "Input schema for programmable read-only TypeScript PRISM queries.",
            "schemaUri": tool_schema_resource_uri("prism_query"),
            "exampleInput": tool_input_example("prism_query"),
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn entrypoints_payload_example() -> Value {
    json!({
        "uri": "prism://entrypoints?limit=5",
        "schemaUri": schema_resource_uri("entrypoints"),
        "entrypoints": [sample_symbol()],
        "page": sample_page(),
        "truncated": false,
        "diagnostics": [],
        "relatedResources": sample_related_resources(),
    })
}

fn search_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("search"),
        "schemaUri": schema_resource_uri("search"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "query": "read context",
        "strategy": "behavioral",
        "ownerKind": "read",
        "kind": "function",
        "path": "src",
        "includeInferred": true,
        "suggestedReads": [sample_owner_candidate()],
        "results": [sample_symbol()],
        "discovery": sample_discovery_bundle(),
        "topReadContext": sample_read_context(),
        "suggestedQueries": [sample_suggested_query()],
        "page": sample_page(),
        "truncated": false,
        "diagnostics": [sample_diagnostic()],
        "relatedResources": sample_related_resources(),
    })
}

fn symbol_payload_example() -> Value {
    json!({
        "uri": symbol_resource_uri_from_node_id(&sample_node_id()),
        "schemaUri": schema_resource_uri("symbol"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "symbol": sample_symbol(),
        "discovery": sample_discovery_bundle(),
        "suggestedReads": [sample_owner_candidate()],
        "readContext": sample_read_context(),
        "editContext": sample_edit_context(),
        "suggestedQueries": [sample_suggested_query()],
        "relations": sample_relations(),
        "specCluster": null,
        "specDrift": null,
        "lineage": sample_lineage(),
        "coChangeNeighbors": [sample_co_change()],
        "relatedFailures": [sample_outcome_event()],
        "blastRadius": sample_change_impact(),
        "validationRecipe": sample_validation_recipe(),
        "diagnostics": [],
        "relatedResources": sample_related_resources(),
    })
}

fn sample_discovery_bundle() -> Value {
    json!({
        "target": sample_symbol(),
        "suggestedReads": [sample_owner_candidate()],
        "readContext": sample_read_context(),
        "editContext": sample_edit_context(),
        "validationContext": sample_validation_context(),
        "recentChangeContext": sample_recent_change_context(),
        "entrypoints": [sample_symbol()],
        "whereUsedDirect": [sample_symbol()],
        "whereUsedBehavioral": [sample_symbol()],
        "suggestedQueries": [sample_suggested_query()],
        "relations": sample_relations(),
        "specCluster": null,
        "specDrift": null,
        "lineage": sample_lineage(),
        "coChangeNeighbors": [sample_co_change()],
        "relatedFailures": [sample_outcome_event()],
        "blastRadius": sample_change_impact(),
        "validationRecipe": sample_validation_recipe(),
        "trustSignals": sample_discovery_trust_signals(),
        "why": [
            "Suggested reads prioritize read-oriented owner paths for the target.",
            "Entrypoints and where-used views show how the target is reached from the outside."
        ]
    })
}

fn lineage_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("lineage"),
        "schemaUri": schema_resource_uri("lineage"),
        "lineageId": "lineage:demo::main",
        "status": "ambiguous",
        "currentNodes": [sample_symbol()],
        "currentNodesTruncated": false,
        "history": [{
            "eventId": "lineage-event:1",
            "ts": 1700000100,
            "kind": "Rename",
            "confidence": 0.92,
            "before": [{
                "crateName": "demo",
                "path": "demo::old_main",
                "kind": "function"
            }],
            "after": [sample_node_id_value()],
            "evidence": ["git mv src/lib.rs src/main.rs"]
        }],
        "historyPage": sample_page(),
        "truncated": false,
        "coChangeNeighbors": [sample_co_change()],
        "diagnostics": [sample_lineage_diagnostic()],
        "relatedResources": sample_related_resources(),
    })
}

fn task_payload_example() -> Value {
    json!({
        "uri": task_resource_uri("task:demo-main"),
        "schemaUri": schema_resource_uri("task"),
        "taskId": "task:demo-main",
        "journal": sample_task_journal(),
        "events": [sample_outcome_event()],
        "page": sample_page(),
        "truncated": false,
        "relatedResources": sample_related_resources(),
    })
}

fn event_payload_example() -> Value {
    json!({
        "uri": event_resource_uri("event:demo-main"),
        "schemaUri": schema_resource_uri("event"),
        "event": sample_outcome_event(),
        "relatedResources": sample_related_resources(),
    })
}

fn memory_payload_example() -> Value {
    json!({
        "uri": memory_resource_uri("memory:demo-main"),
        "schemaUri": schema_resource_uri("memory"),
        "memory": sample_memory_entry(),
        "taskId": "task:demo-main",
        "relatedResources": sample_related_resources(),
    })
}

fn edge_payload_example() -> Value {
    json!({
        "uri": edge_resource_uri("edge:demo-main-calls-helper"),
        "schemaUri": schema_resource_uri("edge"),
        "edge": {
            "id": "edge:demo-main-calls-helper",
            "edge": {
                "kind": EdgeKind::Calls,
                "source": sample_node_id_value(),
                "target": {
                    "crateName": "demo",
                    "path": "demo::helper",
                    "kind": "function"
                },
                "origin": "inferred",
                "confidence": 0.74
            },
            "scope": "session",
            "taskId": "task:demo-main",
            "evidence": ["Observed in the read-context walkthrough."]
        },
        "relatedResources": sample_related_resources(),
    })
}

fn resource_schema_catalog_payload_example() -> Value {
    json!({
        "uri": "prism://schemas",
        "schemaUri": schema_resource_uri("schemas"),
        "schemas": [{
            "resourceKind": "search",
            "schemaUri": schema_resource_uri("search"),
            "resourceUri": "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&includeInferred={includeInferred}",
            "exampleUri": resource_example_uri("search"),
            "description": "Schema for browseable search results and diagnostics."
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn tool_schema_catalog_payload_example() -> Value {
    json!({
        "uri": "prism://tool-schemas",
        "schemaUri": schema_resource_uri("tool-schemas"),
        "tools": [{
            "toolName": "prism_query",
            "schemaUri": tool_schema_resource_uri("prism_query"),
            "description": "Input schema for programmable read-only TypeScript PRISM queries.",
            "exampleInput": tool_input_example("prism_query"),
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn sample_symbol() -> Value {
    json!({
        "id": sample_node_id_value(),
        "name": "main",
        "kind": "function",
        "signature": "fn main()",
        "filePath": "src/main.rs",
        "span": { "start": 0, "end": 42 },
        "location": {
            "startLine": 1,
            "startColumn": 1,
            "endLine": 3,
            "endColumn": 2
        },
        "language": "rust",
        "lineageId": "lineage:demo::main",
        "sourceExcerpt": {
            "text": "fn main() {\\n    helper();\\n}",
            "startLine": 1,
            "endLine": 3,
            "truncated": false
        },
        "ownerHint": {
            "kind": "read",
            "score": 93,
            "matchedTerms": ["read", "context"],
            "why": "Matches the dominant read-path terms.",
            "trustSignals": sample_inferred_trust_signals("high")
        }
    })
}

fn sample_owner_candidate() -> Value {
    json!({
        "symbol": sample_symbol(),
        "kind": "read",
        "score": 93,
        "matchedTerms": ["read", "context"],
        "why": "Touches the same read path and exposes the next likely file to inspect.",
        "trustSignals": sample_inferred_trust_signals("high")
    })
}

fn sample_read_context() -> Value {
    json!({
        "target": sample_symbol(),
        "directLinks": [sample_symbol()],
        "suggestedReads": [sample_owner_candidate()],
        "tests": [sample_owner_candidate()],
        "relatedMemory": [sample_scored_memory()],
        "recentFailures": [sample_outcome_event()],
        "validationRecipe": sample_validation_recipe(),
        "why": ["This symbol is the top behavioral read owner for the query."],
        "suggestedQueries": [sample_suggested_query()],
    })
}

fn sample_edit_context() -> Value {
    json!({
        "target": sample_symbol(),
        "directLinks": [sample_symbol()],
        "suggestedReads": [sample_owner_candidate()],
        "writePaths": [sample_owner_candidate()],
        "tests": [sample_owner_candidate()],
        "relatedMemory": [sample_scored_memory()],
        "recentFailures": [sample_outcome_event()],
        "blastRadius": sample_change_impact(),
        "validationRecipe": sample_validation_recipe(),
        "checklist": ["Inspect the read path.", "Run the regression check."],
        "suggestedQueries": [sample_suggested_query()],
    })
}

fn sample_validation_context() -> Value {
    json!({
        "target": sample_symbol(),
        "tests": [sample_owner_candidate()],
        "relatedMemory": [sample_scored_memory()],
        "recentFailures": [sample_outcome_event()],
        "blastRadius": sample_change_impact(),
        "validationRecipe": sample_validation_recipe(),
        "why": ["Validation context keeps tests, failures, and blast radius in one bundle."],
        "suggestedQueries": [sample_suggested_query()],
    })
}

fn sample_recent_change_context() -> Value {
    json!({
        "target": sample_symbol(),
        "recentEvents": [sample_outcome_event()],
        "recentFailures": [sample_outcome_event()],
        "coChangeNeighbors": [sample_co_change()],
        "relatedMemory": [sample_scored_memory()],
        "promotedSummaries": ["Recent changes clustered around read-context indexing."],
        "lineage": sample_lineage(),
        "why": ["Recent change context keeps outcomes, co-change signals, and lineage together."],
        "suggestedQueries": [sample_suggested_query()],
    })
}

fn sample_inferred_trust_signals(label: &str) -> Value {
    json!({
        "confidenceLabel": label,
        "evidenceSources": ["inferred"],
        "why": ["This result comes from inferred behavioral ranking over names, paths, and excerpts."]
    })
}

fn sample_discovery_trust_signals() -> Value {
    json!({
        "confidenceLabel": "high",
        "evidenceSources": ["direct_graph", "inferred", "memory", "outcome"],
        "why": [
            "Direct graph links anchor the discovery bundle to indexed structural relations.",
            "Behavioral owner ranking contributes inferred follow-up reads and usage paths.",
            "Anchored memory contributes recalled notes and promoted summaries.",
            "Outcome history contributes recent failures, validations, or recorded events."
        ]
    })
}

fn sample_relations() -> Value {
    json!({
        "contains": [],
        "callers": [],
        "callees": [sample_symbol()],
        "references": [],
        "imports": [],
        "implements": [],
        "specifies": [],
        "specifiedBy": [],
        "validates": [],
        "validatedBy": [],
        "related": [],
        "relatedBy": []
    })
}

fn sample_lineage() -> Value {
    json!({
        "lineageId": "lineage:demo::main",
        "current": sample_symbol(),
        "status": "ambiguous",
        "history": [{
            "eventId": "lineage-event:1",
            "ts": 1700000100,
            "kind": "Rename",
            "confidence": 0.92,
            "before": [{
                "crateName": "demo",
                "path": "demo::old_main",
                "kind": "function"
            }],
            "after": [sample_node_id_value()],
            "evidence": ["git mv src/lib.rs src/main.rs"]
        }]
    })
}

fn sample_co_change() -> Value {
    json!({
        "lineage": "lineage:demo::main",
        "count": 3,
        "nodes": [sample_node_id_value()]
    })
}

fn sample_change_impact() -> Value {
    json!({
        "directNodes": [sample_node_id_value()],
        "lineages": ["lineage:demo::main"],
        "likelyValidations": ["cargo test -p prism-mcp"],
        "validationChecks": [{
            "label": "cargo test -p prism-mcp",
            "score": 0.91,
            "lastSeen": 1700000200
        }],
        "coChangeNeighbors": [sample_co_change()],
        "riskEvents": [sample_outcome_event()],
        "promotedSummaries": ["Recent failures clustered around the read-context path."]
    })
}

fn sample_validation_recipe() -> Value {
    json!({
        "target": sample_node_id_value(),
        "checks": ["cargo test -p prism-mcp"],
        "scoredChecks": [{
            "label": "cargo test -p prism-mcp",
            "score": 0.91,
            "lastSeen": 1700000200
        }],
        "relatedNodes": [sample_node_id_value()],
        "coChangeNeighbors": [sample_co_change()],
        "recentFailures": [sample_outcome_event()]
    })
}

fn sample_task_journal() -> Value {
    json!({
        "taskId": "task:demo-main",
        "description": "Inspect the read-context flow.",
        "tags": ["mcp", "examples"],
        "disposition": "open",
        "active": true,
        "anchors": [],
        "summary": {
            "planCount": 1,
            "patchCount": 1,
            "buildCount": 0,
            "testCount": 1,
            "failureCount": 0,
            "validationCount": 0,
            "noteCount": 0,
            "startedAt": 1700000000,
            "lastUpdatedAt": 1700000300,
            "finalSummary": null
        },
        "diagnostics": [sample_diagnostic()],
        "relatedMemory": [sample_scored_memory()],
        "recentEvents": [sample_outcome_event()]
    })
}

fn sample_scored_memory() -> Value {
    json!({
        "id": "memory:demo-main",
        "entry": sample_memory_entry(),
        "score": 0.84,
        "sourceModule": "session",
        "explanation": "Anchored to the same lineage and recent task history."
    })
}

fn sample_memory_entry() -> Value {
    json!({
        "id": "memory:demo-main",
        "anchors": [],
        "kind": "semantic",
        "content": "Main changes usually require the prism-mcp regression suite.",
        "metadata": { "source": "example" },
        "createdAt": 1700000000,
        "source": "session",
        "trust": 0.82
    })
}

fn sample_outcome_event() -> Value {
    json!({
        "meta": {
            "id": "event:demo-main",
            "ts": 1700000200,
            "actor": "Agent",
            "correlation": "task:demo-main",
            "causation": null
        },
        "anchors": [],
        "kind": "TestRan",
        "result": "Success",
        "summary": "Validated the read-context workflow.",
        "evidence": [{
            "Test": {
                "name": "cargo test -p prism-mcp",
                "passed": true
            }
        }],
        "metadata": { "suite": "prism-mcp" }
    })
}

fn sample_diagnostic() -> Value {
    json!({
        "code": "result_truncated",
        "message": "Search results for `read context` were truncated at 5 entries.",
        "data": {
            "query": "read context",
            "applied": 5,
            "nextAction": "Use a narrower prism.search(...) call and then inspect prism.readContext(...) on one candidate."
        }
    })
}

fn sample_lineage_diagnostic() -> Value {
    json!({
        "code": "lineage_uncertain",
        "message": "Lineage for `demo::main` contains ambiguous history.",
        "data": {
            "id": "demo::main",
            "nextAction": "Inspect `prism.lineage(target)` history and compare the candidate symbols with `prism.readContext(target)` before editing."
        }
    })
}

fn sample_suggested_query() -> Value {
    json!({
        "label": "Read Context",
        "query": "const sym = prism.search(\"read context\", { limit: 1 })[0]; return sym ? prism.readContext(sym) : null;",
        "why": "Inspect the strongest candidate before editing."
    })
}

fn sample_page() -> Value {
    json!({
        "cursor": null,
        "nextCursor": null,
        "limit": 5,
        "returned": 1,
        "total": 1,
        "hasMore": false,
        "limitCapped": false
    })
}

fn sample_features() -> Value {
    json!({
        "mode": "simple",
        "coordination": {
            "workflow": false,
            "claims": false,
            "artifacts": false
        }
    })
}

fn sample_limits() -> Value {
    json!({
        "maxResultNodes": 500,
        "maxCallGraphDepth": 10,
        "maxOutputJsonBytes": 262144
    })
}

fn sample_related_resources() -> Value {
    json!([
        {
            "uri": capabilities_resource_uri(),
            "name": "PRISM Capabilities",
            "description": "Canonical capability map for query methods, resources, features, and build info"
        },
        {
            "uri": schema_resource_uri("search"),
            "name": "PRISM Schema: search",
            "description": "JSON Schema for the `search` PRISM resource payload"
        }
    ])
}

fn sample_node_id() -> NodeId {
    NodeId::new("demo", "demo::main", prism_ir::NodeKind::Function)
}

fn sample_node_id_value() -> Value {
    json!({
        "crateName": "demo",
        "path": "demo::main",
        "kind": "function"
    })
}
