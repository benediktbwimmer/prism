use serde_json::{json, Value};

use crate::{
    capabilities_resource_uri, capabilities_section_resource_uri,
    contracts_resource_uri_with_options, edge_resource_uri, event_resource_uri,
    file_resource_uri_with_options, instruction_set_resource_uri, instructions_resource_uri,
    memory_resource_uri, plan_resource_uri, plans_resource_uri, protected_state_resource_uri,
    protected_state_resource_uri_with_options, resource_example_resource_uri,
    resource_shape_resource_uri, schema_resource_uri, self_description_audit_resource_uri,
    session_resource_uri, symbol_resource_uri_from_node_id, task_resource_uri,
    tool_example_resource_uri, tool_schema_resource_uri, tool_shape_resource_uri,
    vocab_entry_resource_uri, vocab_resource_uri, API_REFERENCE_URI,
};
use prism_ir::{EdgeKind, NodeId};

pub(crate) fn schema_examples(target_resource_kind: &str) -> Option<Vec<Value>> {
    if let Some(tool_name) = target_resource_kind.strip_prefix("tool:") {
        return tool_input_examples(tool_name);
    }
    resource_payload_example(target_resource_kind).map(|example| vec![example])
}

pub(crate) fn resource_example_uri(resource_kind: &str) -> Option<String> {
    match resource_kind {
        "instructions" => Some(instructions_resource_uri()),
        "capabilities" => Some(capabilities_resource_uri()),
        "schemas" => Some("prism://schemas".to_string()),
        "session" => Some(session_resource_uri()),
        "protected-state" => Some(protected_state_resource_uri_with_options(Some("concepts:events"))),
        "vocab" => Some(vocab_resource_uri()),
        "tool-schemas" => Some("prism://tool-schemas".to_string()),
        "plans" => Some("prism://plans?contains=persistence&sort=last_updated_desc&limit=5".to_string()),
        "plan" => Some(plan_resource_uri("plan:1")),
        "contracts" => Some(contracts_resource_uri_with_options(
            Some("runtime"),
            Some("active"),
            Some("repo"),
            Some("interface"),
        )),
        "entrypoints" => Some("prism://entrypoints?limit=5".to_string()),
        "search" => Some(
            "prism://search/read%20context?strategy=behavioral&ownerKind=read&kind=function&path=src&pathMode=exact&structuredPath=workspace&topLevelOnly=true&includeInferred=true".to_string(),
        ),
        "file" => Some(file_resource_uri_with_options(
            "crates/prism-mcp/src/views.rs",
            Some(420),
            Some(445),
            Some(1400),
        )),
        "symbol" => Some(symbol_resource_uri_from_node_id(&sample_node_id())),
        "lineage" => Some("prism://lineage/lineage%3Ademo%3A%3Amain?limit=5".to_string()),
        "task" => Some(task_resource_uri("task:demo-main")),
        "event" => Some(event_resource_uri("event:demo-main")),
        "memory" => Some(memory_resource_uri("memory:demo-main")),
        "edge" => Some(edge_resource_uri("edge:demo-main-calls-helper")),
        "tool-example" => Some(tool_example_resource_uri("prism_code")),
        "tool-shape" => Some(tool_shape_resource_uri("prism_code")),
        "resource-example" => Some(resource_example_resource_uri("search")),
        "resource-shape" => Some(resource_shape_resource_uri("search")),
        "capabilities-section" => Some(capabilities_section_resource_uri("tools")),
        "vocab-entry" => Some(vocab_entry_resource_uri("coordinationMutationKind")),
        "self-description-audit" => Some(self_description_audit_resource_uri()),
        _ => None,
    }
}

pub(crate) fn tool_input_example(tool_name: &str) -> Option<Value> {
    match tool_name {
        "prism_locate" => Some(json!({
            "query": "session",
            "taskIntent": "edit",
            "taskId": "coord-task:12",
            "limit": 3,
        })),
        "prism_gather" => Some(json!({
            "query": "prism_compact_tool_calls",
            "path": "benchmarks/scripts/benchmark_codex.py",
            "limit": 3,
        })),
        "prism_open" => Some(json!({
            "path": "crates/prism-mcp/src/query_runtime.rs",
            "mode": "raw",
            "line": 686,
            "beforeLines": 2,
            "afterLines": 6,
        })),
        "prism_workset" => Some(json!({
            "handle": "handle:1",
            "taskId": "coord-task:12",
        })),
        "prism_expand" => Some(json!({
            "handle": "handle:1",
            "kind": "validation",
        })),
        "prism_task_brief" => Some(json!({
            "taskId": "coord-task:1",
        })),
        "prism_concept" => Some(json!({
            "query": "validation pipeline",
            "taskId": "coord-task:12",
            "lens": "validation",
            "verbosity": "summary",
        })),
        "prism_code" => Some(json!({
            "code": "const peer = prism.from(\"runtime-demo\"); return { status: peer.runtime.status(), excerpt: peer.file(\"README.md\").read({ maxChars: 160 }) };",
            "language": "ts",
        })),
        _ => None,
    }
}

pub(crate) fn tool_input_examples(tool_name: &str) -> Option<Vec<Value>> {
    tool_input_example(tool_name).map(|example| vec![example])
}

pub(crate) fn tool_action_example(tool_name: &str, action: &str) -> Option<Value> {
    tool_action_examples(tool_name, action).into_iter().next()
}

pub(crate) fn tool_action_examples(tool_name: &str, action: &str) -> Vec<Value> {
    tool_input_examples(tool_name)
        .unwrap_or_default()
        .into_iter()
        .filter(|example| example.get("action").and_then(Value::as_str) == Some(action))
        .collect()
}

fn sample_node_anchor(crate_name: &str, path: &str, kind: &str) -> Value {
    json!({
        "type": "node",
        "crateName": crate_name,
        "path": path,
        "kind": kind,
    })
}

fn sample_node_id_input(crate_name: &str, path: &str, kind: &str) -> Value {
    json!({
        "crateName": crate_name,
        "path": path,
        "kind": kind,
    })
}

pub(crate) fn resource_payload_example(resource_kind: &str) -> Option<Value> {
    match resource_kind {
        "capabilities" => Some(capabilities_payload_example()),
        "schemas" => Some(resource_schema_catalog_payload_example()),
        "session" => Some(session_payload_example()),
        "protected-state" => Some(protected_state_payload_example()),
        "vocab" => Some(vocab_payload_example()),
        "tool-schemas" => Some(tool_schema_catalog_payload_example()),
        "plans" => Some(plans_payload_example()),
        "plan" => Some(plan_payload_example()),
        "contracts" => Some(contracts_payload_example()),
        "entrypoints" => Some(entrypoints_payload_example()),
        "search" => Some(search_payload_example()),
        "file" => Some(file_payload_example()),
        "symbol" => Some(symbol_payload_example()),
        "lineage" => Some(lineage_payload_example()),
        "task" => Some(task_payload_example()),
        "event" => Some(event_payload_example()),
        "memory" => Some(memory_payload_example()),
        "edge" => Some(edge_payload_example()),
        "tool-example" => Some(tool_example_payload_example()),
        "tool-shape" => Some(tool_shape_payload_example()),
        "resource-example" => Some(resource_example_payload_example()),
        "resource-shape" => Some(resource_shape_payload_example()),
        "capabilities-section" => Some(capabilities_section_payload_example()),
        "vocab-entry" => Some(vocab_entry_payload_example()),
        "self-description-audit" => Some(self_description_audit_payload_example()),
        _ => None,
    }
}

fn tool_example_payload_example() -> Value {
    json!({
        "uri": tool_example_resource_uri("prism_code"),
        "schemaUri": schema_resource_uri("tool-example"),
        "toolName": "prism_code",
        "targetSchemaUri": tool_schema_resource_uri("prism_code"),
        "shapeUri": tool_shape_resource_uri("prism_code"),
        "example": {
            "code": "return prism.plans().limit(5).map(plan => ({ id: plan.id, status: plan.status }));",
            "language": "ts"
        },
        "examples": [{
            "code": "return prism.plans().limit(5).map(plan => ({ id: plan.id, status: plan.status }));",
            "language": "ts"
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn tool_shape_payload_example() -> Value {
    json!({
        "uri": tool_shape_resource_uri("prism_code"),
        "schemaUri": schema_resource_uri("tool-shape"),
        "toolName": "prism_code",
        "toolSchemaUri": tool_schema_resource_uri("prism_code"),
        "exampleUri": tool_example_resource_uri("prism_code"),
        "description": "Compact shape summary for the canonical programmable PRISM code surface.",
        "requiredFields": ["code"],
        "optionalFields": ["language", "dryRun", "credential", "bridgeExecution"],
        "fields": [{
            "name": "code",
            "required": true,
            "description": "JavaScript or TypeScript source evaluated against the live PRISM runtime.",
            "types": ["string"],
            "enumValues": [],
            "nestedFields": []
        }],
        "actions": [{
            "action": "default",
            "schemaUri": tool_schema_resource_uri("prism_code"),
            "exampleUri": tool_example_resource_uri("prism_code"),
            "shapeUri": tool_shape_resource_uri("prism_code"),
            "description": "Exact input shape for the canonical programmable PRISM code surface.",
            "requiredFields": ["code"],
            "optionalFields": ["language", "dryRun", "credential", "bridgeExecution"],
            "fields": [{
                "name": "code",
                "required": true,
                "description": "JavaScript or TypeScript source evaluated against the live PRISM runtime.",
                "types": ["string"],
                "enumValues": [],
                "nestedFields": []
            }]
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn resource_example_payload_example() -> Value {
    json!({
        "uri": resource_example_resource_uri("search"),
        "schemaUri": schema_resource_uri("resource-example"),
        "resourceKind": "search",
        "resourceSchemaUri": schema_resource_uri("search"),
        "shapeUri": resource_shape_resource_uri("search"),
        "example": search_payload_example(),
        "relatedResources": sample_related_resources(),
    })
}

fn resource_shape_payload_example() -> Value {
    json!({
        "uri": resource_shape_resource_uri("search"),
        "schemaUri": schema_resource_uri("resource-shape"),
        "resourceKind": "search",
        "resourceSchemaUri": schema_resource_uri("search"),
        "exampleUri": resource_example_resource_uri("search"),
        "description": "Compact shape summary for the search resource payload.",
        "requiredFields": ["uri", "schemaUri", "query"],
        "optionalFields": ["results", "page", "truncated"],
        "fields": [{
            "name": "query",
            "required": true,
            "description": "Search query string",
            "types": ["string"],
            "enumValues": [],
            "nestedFields": []
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn capabilities_section_payload_example() -> Value {
    json!({
        "uri": capabilities_section_resource_uri("tools"),
        "schemaUri": schema_resource_uri("capabilities-section"),
        "section": "tools",
        "value": [{
            "name": "prism_code",
            "description": "Input schema for the canonical programmable PRISM code surface.",
            "schemaUri": tool_schema_resource_uri("prism_code"),
            "exampleUri": tool_example_resource_uri("prism_code"),
            "shapeUri": tool_shape_resource_uri("prism_code")
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn vocab_entry_payload_example() -> Value {
    json!({
        "uri": vocab_entry_resource_uri("coordinationMutationKind"),
        "schemaUri": schema_resource_uri("vocab-entry"),
        "key": "coordinationMutationKind",
        "vocabulary": {
            "key": "coordinationMutationKind",
            "title": "Coordination Mutation Kinds",
            "description": "Coordination mutation kinds accepted by the native PRISM coordination surface.",
            "values": [{
                "value": "coordination_transaction",
                "aliases": ["transaction"],
                "description": "Apply one ordered coordination transaction over the canonical plan/task model."
            }, {
                "value": "plan_bootstrap",
                "aliases": [],
                "description": "Create a plan and its initial tasks in one authoritative write."
            }]
        },
        "relatedResources": sample_related_resources(),
    })
}

fn self_description_audit_payload_example() -> Value {
    json!({
        "uri": self_description_audit_resource_uri(),
        "schemaUri": schema_resource_uri("self-description-audit"),
        "budgetBytes": 12288,
        "totalEntries": 3,
        "oversizeEntries": 0,
        "missingCompanionEntries": 0,
        "entries": [{
            "surfaceKind": "tool",
            "name": "prism_code",
            "fullUri": tool_schema_resource_uri("prism_code"),
            "schemaUri": tool_schema_resource_uri("prism_code"),
            "exampleUri": tool_example_resource_uri("prism_code"),
            "shapeUri": tool_shape_resource_uri("prism_code"),
            "recipeUri": null,
            "fullBytes": 4096,
            "schemaBytes": 4096,
            "exampleBytes": 1024,
            "shapeBytes": 1024,
            "recipeBytes": null,
            "exampleValid": true,
            "exampleValidationIssueCodes": [],
            "sourceFreeOperable": true,
            "issues": []
        }],
        "missingRecipeEntries": 0,
        "invalidExampleEntries": 0,
        "nonOperableEntries": 0,
        "relatedResources": sample_related_resources(),
    })
}

fn session_payload_example() -> Value {
    json!({
        "uri": session_resource_uri(),
        "schemaUri": schema_resource_uri("session"),
        "workspaceRoot": "workspace/demo",
        "currentTask": {
            "taskId": "task:demo-main",
            "description": "Inspect the read-context flow.",
            "tags": ["mcp", "examples"]
        },
        "currentWork": {
            "workId": "work:demo-main",
            "kind": "ad_hoc",
            "title": "Inspect the read-context flow",
            "summary": "Bootstrap work attribution before storing outcomes.",
            "parentWorkId": "work:parent-demo",
            "coordinationTaskId": null,
            "planId": null,
            "planTitle": null
        },
        "currentAgent": "codex",
        "bridgeIdentity": {
            "status": "bound",
            "profile": "codex-c",
            "principalId": "principal:codex-c",
            "credentialId": "credential:codex-c",
            "error": null,
            "nextAction": "Proceed with authenticated `prism_code` calls without supplying `credential` on this bridge."
        },
        "limits": sample_limits(),
        "features": sample_features(),
        "relatedResources": sample_related_resources(),
    })
}

fn protected_state_payload_example() -> Value {
    json!({
        "uri": protected_state_resource_uri_with_options(Some("concepts:events")),
        "schemaUri": schema_resource_uri("protected-state"),
        "workspaceRoot": "workspace/demo",
        "streamSelector": "concepts:events",
        "streams": [{
            "stream": "repo_concept_events",
            "streamId": "concepts:events",
            "protectedPath": ".prism/concepts/events.jsonl",
            "verificationStatus": "LegacyUnsigned",
            "lastVerifiedEventId": null,
            "lastVerifiedEntryHash": null,
            "trustBundleId": null,
            "diagnosticCode": "protected_stream_legacy_unsigned",
            "diagnosticSummary": "protected stream .prism/concepts/events.jsonl still uses unsigned legacy records at line 1",
            "repairHint": "run an explicit migrate-sign flow before using this stream authoritatively"
        }],
        "allVerified": false,
        "nonVerifiedStreamCount": 1,
        "relatedResources": sample_related_resources(),
    })
}

fn vocab_payload_example() -> Value {
    json!({
        "uri": vocab_resource_uri(),
        "schemaUri": schema_resource_uri("vocab"),
        "vocabularies": [{
            "key": "coordinationTaskStatus",
            "title": "Coordination Task Statuses",
            "description": "Canonical coordination task status values.",
            "values": [{
                "value": "ready",
                "aliases": ["todo"],
                "description": "Actionable task waiting to be worked."
            }, {
                "value": "in_progress",
                "aliases": ["in-progress", "inprogress"],
                "description": "Task actively being worked."
            }]
        }, {
            "key": "coordinationMutationKind",
            "title": "Coordination Mutation Kinds",
            "description": "Coordination mutation kinds accepted by the native PRISM coordination surface.",
            "values": [{
                "value": "coordination_transaction",
                "aliases": ["transaction"],
                "description": "Apply one ordered coordination transaction."
            }, {
                "value": "plan_bootstrap",
                "aliases": [],
                "description": "Create a plan and its initial tasks in one authoritative write."
            }, {
                "value": "task_create",
                "aliases": [],
                "description": "Create a coordination task."
            }]
        }],
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
        "queryViews": [{
            "name": "repoPlaybook",
            "enabled": false,
            "featureFlag": "repo_playbook",
            "stability": "experimental",
            "owner": "prism-mcp",
            "description": "Summarize repo-specific build, test, lint, format, and workflow guidance."
        }],
        "resources": [{
            "name": "PRISM Session",
            "uri": session_resource_uri(),
            "mimeType": "application/json",
            "description": "Active workspace root, task context, limits, and feature flags.",
            "schemaUri": schema_resource_uri("session"),
            "exampleUri": session_resource_uri(),
        }, {
            "name": "PRISM Protected State",
            "uri": protected_state_resource_uri(),
            "mimeType": "application/json",
            "description": "Protected .prism stream verification status, trust diagnostics, and repair guidance.",
            "schemaUri": schema_resource_uri("protected-state"),
            "exampleUri": resource_example_uri("protected-state"),
        }, {
            "name": "PRISM Vocabulary",
            "uri": vocab_resource_uri(),
            "mimeType": "application/json",
            "description": "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads.",
            "schemaUri": schema_resource_uri("vocab"),
            "exampleUri": vocab_resource_uri(),
        }],
        "resourceTemplates": [{
            "name": "PRISM Search",
            "uriTemplate": "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}",
            "mimeType": "application/json",
            "description": "Read structured search results and diagnostics for a query.",
            "exampleUri": resource_example_uri("search"),
        }],
        "tools": [{
            "name": "prism_locate",
            "description": "Input schema for the compact first-hop target locator.",
            "schemaUri": tool_schema_resource_uri("prism_locate"),
            "exampleInput": tool_input_example("prism_locate"),
        }, {
            "name": "prism_code",
            "description": "Input schema for the canonical programmable PRISM code surface.",
            "schemaUri": tool_schema_resource_uri("prism_code"),
            "exampleInput": tool_input_example("prism_code"),
        }, {
            "name": "prism_task_brief",
            "description": "Input schema for the compact coordination task brief tool.",
            "schemaUri": tool_schema_resource_uri("prism_task_brief"),
            "exampleInput": tool_input_example("prism_task_brief"),
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

fn plans_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("plans"),
        "schemaUri": schema_resource_uri("plans"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "status": null,
        "scope": null,
        "contains": "persistence",
        "sort": "last_updated_desc",
        "plans": [{
            "planId": "plan:1",
            "title": "Migrate persistence to DB-native runtime storage",
            "goal": "Move persistence away from snapshot-authoritative writes.",
            "status": "active",
            "scope": "repo",
            "kind": "migration",
            "scheduling": {
                "importance": 3,
                "urgency": 2,
                "manualBoost": 0,
                "dueAt": null
            },
            "gitExecutionPolicy": {
                "startMode": "auto",
                "completionMode": "auto",
                "integrationMode": "branch",
                "targetRef": null,
                "targetBranch": "main",
                "requireTaskBranch": false,
                "maxCommitsBehindTarget": 0,
                "maxFetchAgeSeconds": null
            },
            "createdAt": 1712304000u64,
            "lastUpdatedAt": 1712307600u64,
            "nodeStatusCounts": {
                "proposed": 0,
                "ready": 1,
                "inProgress": 0,
                "blocked": 0,
                "waiting": 0,
                "inReview": 0,
                "validating": 0,
                "completed": 0,
                "abandoned": 0,
                "abstractNodes": 0
            },
            "summary": "1 actionable of 6 nodes",
            "planSummary": {
                "planId": "plan:1",
                "status": "active",
                "totalNodes": 6,
                "completedNodes": 0,
                "abandonedNodes": 0,
                "inProgressNodes": 0,
                "actionableNodes": 1,
                "executionBlockedNodes": 0,
                "completionGatedNodes": 0,
                "reviewGatedNodes": 0,
                "validationGatedNodes": 0,
                "staleNodes": 0,
                "claimConflictedNodes": 0
            },
            "activity": {
                "createdAt": 1712304000u64,
                "lastUpdatedAt": 1712307600u64,
                "lastEventKind": "plan_updated",
                "lastEventSummary": "Prioritized persistence work.",
                "lastEventTaskId": "coord-task:1"
            }
        }],
        "page": sample_page(),
        "truncated": false,
        "diagnostics": [],
        "relatedResources": [{
            "uri": plans_resource_uri(),
            "name": "PRISM Plans",
            "description": "Browse published and runtime-hydrated plans with compact progress summaries"
        }, {
            "uri": session_resource_uri(),
            "name": "PRISM Session",
            "description": "Active workspace root, current work and task focus, and runtime query limits"
        }],
    })
}

fn plan_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("plan"),
        "schemaUri": schema_resource_uri("plan"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "plan": {
            "id": "plan:1",
            "title": "Migrate persistence to DB-native runtime storage",
            "goal": "Move persistence away from snapshot-authoritative writes.",
            "status": "active",
            "scope": "repo",
            "kind": "migration",
            "revision": 3,
            "tags": ["persistence", "storage"],
            "createdFrom": "manual"
        },
        "summary": {
            "planId": "plan:1",
            "status": "active",
            "totalNodes": 6,
            "completedNodes": 0,
            "abandonedNodes": 0,
            "inProgressNodes": 0,
            "actionableNodes": 1,
            "executionBlockedNodes": 0,
            "completionGatedNodes": 0,
            "reviewGatedNodes": 0,
            "validationGatedNodes": 0,
            "staleNodes": 0,
            "claimConflictedNodes": 0
        },
        "relatedResources": [{
            "uri": plan_resource_uri("plan:1"),
            "name": "PRISM Plan: plan:1",
            "description": "Coordination plan detail with root nodes and progress summary"
        }, {
            "uri": plans_resource_uri(),
            "name": "PRISM Plans",
            "description": "Browse published and runtime-hydrated plans with compact progress summaries"
        }, {
            "uri": session_resource_uri(),
            "name": "PRISM Session",
            "description": "Active workspace root, current work and task focus, and runtime query limits"
        }],
    })
}

fn contracts_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("contracts"),
        "schemaUri": schema_resource_uri("contracts"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "contains": "runtime",
        "status": "active",
        "scope": "repo",
        "kind": "interface",
        "contracts": [{
            "handle": "contract://runtime_status_surface",
            "name": "runtime status surface",
            "summary": "The runtime status entry point remains available for internal diagnostics consumers.",
            "aliases": ["runtime status"],
            "kind": "interface",
            "subject": {
                "anchors": [sample_node_anchor("demo", "demo::runtime_status", "function")],
                "conceptHandles": ["concept://runtime_surface"]
            },
            "guarantees": [{
                "statement": "Internal diagnostics callers can query runtime status without reconstructing daemon state.",
                "scope": "internal",
                "strength": "hard",
                "evidenceRefs": ["runtime-status-tests"]
            }],
            "assumptions": ["The daemon is running."],
            "consumers": [{
                "conceptHandles": ["concept://runtime_surface"]
            }],
            "validations": [{
                "id": "cargo test -p prism-mcp runtime_status",
                "summary": "Covers the runtime status surface.",
                "anchors": [sample_node_anchor("demo", "demo::runtime_status", "function")]
            }],
            "stability": "internal",
            "compatibility": {
                "compatible": ["Internal implementation changes behind the same surface."],
                "breaking": ["Removing the runtime status surface."]
            },
            "evidence": ["Promoted from repeated runtime-inspection work."],
            "status": "active",
            "scope": "repo",
            "provenance": {
                "origin": "repo_mutation",
                "kind": "manual_contract_promote",
                "taskId": "task:demo-main"
            },
            "publication": {
                "publishedAt": 1700000200,
                "lastReviewedAt": 1700000200,
                "status": "active",
                "supersedes": []
            },
            "resolution": {
                "score": 260,
                "reasons": ["canonical name match", "linked to current task context"]
            }
        }],
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
        "module": null,
        "taskId": null,
        "pathMode": "exact",
        "structuredPath": "workspace",
        "topLevelOnly": true,
        "includeInferred": true,
        "results": [sample_symbol()],
        "suggestedQueries": [sample_suggested_query()],
        "page": sample_page(),
        "truncated": false,
        "diagnostics": [sample_diagnostic()],
        "relatedResources": sample_related_resources(),
    })
}

fn file_payload_example() -> Value {
    json!({
        "uri": resource_example_uri("file"),
        "schemaUri": schema_resource_uri("file"),
        "workspaceRevision": {
            "graphVersion": 42,
            "gitCommit": "abc123def456"
        },
        "path": "crates/prism-mcp/src/views.rs",
        "excerpt": {
            "text": "pub(crate) fn anchor_ref_view(prism: &Prism, anchor: AnchorRef) -> AnchorRefView {",
            "startLine": 420,
            "endLine": 420,
            "truncated": false
        },
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
        "symbol": {
            "id": sample_node_id_value(),
            "name": "main",
            "kind": "function",
            "filePath": "src/main.rs",
            "location": {
                "startLine": 1,
                "startColumn": 1,
                "endLine": 3,
                "endColumn": 2
            }
        },
        "diagnostics": [],
        "relatedResources": [{
            "uri": schema_resource_uri("symbol"),
            "name": "PRISM Schema: symbol",
            "description": "JSON Schema for the `symbol` PRISM resource payload"
        }],
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
            "resourceKind": "vocab",
            "schemaUri": schema_resource_uri("vocab"),
            "resourceUri": vocab_resource_uri(),
            "exampleUri": resource_example_uri("vocab"),
            "description": "Schema for the canonical PRISM vocabulary catalog."
        }, {
            "resourceKind": "search",
            "schemaUri": schema_resource_uri("search"),
            "resourceUri": "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}",
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
            "toolName": "prism_locate",
            "schemaUri": tool_schema_resource_uri("prism_locate"),
            "description": "Input schema for the compact first-hop target locator.",
            "exampleInput": tool_input_example("prism_locate"),
        }, {
            "toolName": "prism_open",
            "schemaUri": tool_schema_resource_uri("prism_open"),
            "description": "Input schema for opening one compact handle as a bounded code slice.",
            "exampleInput": tool_input_example("prism_open"),
        }, {
            "toolName": "prism_workset",
            "schemaUri": tool_schema_resource_uri("prism_workset"),
            "description": "Input schema for building a compact implementation workset.",
            "exampleInput": tool_input_example("prism_workset"),
        }, {
            "toolName": "prism_expand",
            "schemaUri": tool_schema_resource_uri("prism_expand"),
            "description": "Input schema for explicit depth-on-demand handle expansion.",
            "exampleInput": tool_input_example("prism_expand"),
        }, {
            "toolName": "prism_task_brief",
            "schemaUri": tool_schema_resource_uri("prism_task_brief"),
            "description": "Input schema for the compact coordination task brief tool.",
            "exampleInput": tool_input_example("prism_task_brief"),
        }, {
            "toolName": "prism_concept",
            "schemaUri": tool_schema_resource_uri("prism_concept"),
            "description": "Input schema for resolving a broad repo concept into a compact concept packet.",
            "exampleInput": tool_input_example("prism_concept"),
        }, {
            "toolName": "prism_code",
            "schemaUri": tool_schema_resource_uri("prism_code"),
            "description": "Input schema for the canonical programmable PRISM code surface.",
            "exampleInput": tool_input_example("prism_code"),
        }, {
            "toolName": "prism_task_brief",
            "schemaUri": tool_schema_resource_uri("prism_task_brief"),
            "description": "Input schema for the compact coordination task brief tool.",
            "exampleInput": tool_input_example("prism_task_brief"),
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
        "contracts": [sample_contract_packet()],
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

fn sample_contract_packet() -> Value {
    json!({
        "handle": "contract://runtime_status_surface",
        "name": "runtime status surface",
        "summary": "Internal diagnostics callers can query runtime status without reconstructing daemon state.",
        "aliases": ["runtime status"],
        "kind": "interface",
        "subject": {
            "anchors": [{
                "type": "node",
                "crateName": "demo",
                "path": "demo::runtime_status",
                "kind": "function"
            }],
            "conceptHandles": ["concept://runtime_surface"]
        },
        "guarantees": [{
            "statement": "Internal diagnostics callers can query runtime status without reconstructing daemon state.",
            "scope": "internal",
            "strength": "hard",
            "evidenceRefs": ["runtime-status-tests"]
        }],
        "assumptions": ["The daemon is running."],
        "consumers": [{
            "anchors": [{
                "type": "node",
                "crateName": "demo",
                "path": "demo::inspect_runtime",
                "kind": "function"
            }],
            "conceptHandles": []
        }],
        "validations": [{
            "id": "cargo test -p prism-mcp runtime_status",
            "summary": "Covers the runtime status surface.",
            "anchors": []
        }],
        "stability": "internal",
        "compatibility": {
            "compatible": ["Internal implementation changes behind the same surface."],
            "additive": [],
            "risky": [],
            "breaking": ["Removing the runtime status surface."],
            "migrating": []
        },
        "evidence": ["Promoted from repeated runtime-inspection work."],
        "status": "active",
        "scope": "session",
        "provenance": {
            "origin": "manual_store",
            "kind": "manual_contract_promote",
            "taskId": "task:contract-surface"
        }
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
        "internalDeveloper": false,
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
            "uri": instructions_resource_uri(),
            "name": "PRISM Instruction Sets",
            "description": "Overview of the available PRISM role-specific instruction resources"
        },
        {
            "uri": instruction_set_resource_uri("execution"),
            "name": "PRISM Instructions: Execution",
            "description": "Task execution guidance for actionable nodes, implementation, validation, and completion"
        },
        {
            "uri": capabilities_resource_uri(),
            "name": "PRISM Capabilities",
            "description": "Canonical capability map for query methods, resources, features, and build info"
        },
        {
            "uri": vocab_resource_uri(),
            "name": "PRISM Vocabulary",
            "description": "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads"
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
