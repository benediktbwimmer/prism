use serde_json::{json, Value};

use crate::{
    capabilities_resource_uri, capabilities_section_resource_uri,
    contracts_resource_uri_with_options, edge_resource_uri, event_resource_uri,
    file_resource_uri_with_options, instruction_set_resource_uri, instructions_resource_uri,
    memory_resource_uri, plan_resource_uri, plans_resource_uri, protected_state_resource_uri,
    protected_state_resource_uri_with_options, resource_example_resource_uri,
    resource_shape_resource_uri, schema_resource_uri, self_description_audit_resource_uri,
    session_resource_uri, symbol_resource_uri_from_node_id, task_resource_uri,
    tool_action_example_resource_uri, tool_action_recipe_resource_uri,
    tool_action_schema_resource_uri, tool_action_shape_resource_uri, tool_example_resource_uri,
    tool_schema_resource_uri, tool_shape_resource_uri, tool_variant_example_resource_uri,
    tool_variant_recipe_resource_uri, tool_variant_schema_resource_uri,
    tool_variant_shape_resource_uri, vocab_entry_resource_uri, vocab_resource_uri,
    API_REFERENCE_URI,
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
        "plans" => Some("prism://plans?contains=persistence&limit=5".to_string()),
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
        "tool-example" => Some(tool_example_resource_uri("prism_mutate")),
        "tool-shape" => Some(tool_shape_resource_uri("prism_mutate")),
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
        "prism_query" => Some(json!({
            "code": "const peer = prism.from(\"runtime-demo\"); return { status: peer.runtime.status(), excerpt: peer.file(\"README.md\").read({ maxChars: 160 }) };",
            "language": "ts",
        })),
        "prism_mutate" => prism_mutate_action_example("validation_feedback"),
        _ => None,
    }
}

pub(crate) fn tool_input_examples(tool_name: &str) -> Option<Vec<Value>> {
    match tool_name {
        "prism_mutate" => Some(prism_mutate_examples()),
        _ => tool_input_example(tool_name).map(|example| vec![example]),
    }
}

pub(crate) fn tool_action_example(tool_name: &str, action: &str) -> Option<Value> {
    tool_action_examples(tool_name, action).into_iter().next()
}

pub(crate) fn tool_action_examples(tool_name: &str, action: &str) -> Vec<Value> {
    match tool_name {
        "prism_mutate" => prism_mutate_examples()
            .into_iter()
            .filter(|example| example.get("action").and_then(Value::as_str) == Some(action))
            .collect(),
        _ => tool_input_examples(tool_name)
            .unwrap_or_default()
            .into_iter()
            .filter(|example| example.get("action").and_then(Value::as_str) == Some(action))
            .collect(),
    }
}

fn prism_mutate_examples() -> Vec<Value> {
    let mut examples = [
        "validation_feedback",
        "declare_work",
        "checkpoint",
        "session_repair",
        "outcome",
        "memory",
        "concept",
        "contract",
        "concept_relation",
        "infer_edge",
        "heartbeat_lease",
        "coordination",
        "claim",
        "artifact",
        "test_ran",
        "failure_observed",
        "fix_validated",
        "curator_apply_proposal",
        "curator_promote_edge",
        "curator_promote_concept",
        "curator_promote_memory",
        "curator_reject_proposal",
    ]
    .into_iter()
    .filter_map(prism_mutate_action_example)
    .collect::<Vec<_>>();
    examples.extend(extra_prism_mutate_examples());
    examples.into_iter().map(with_mutation_credential).collect()
}

fn prism_mutate_action_example(action: &str) -> Option<Value> {
    match action {
        "validation_feedback" => Some(json!({
            "action": "validation_feedback",
            "input": {
                "context": "Search for `session` while curating concept packs.",
                "prismSaid": "Search result ordering was helpful.",
                "actuallyTrue": "The top result matched the active session-state owner quickly.",
                "category": "projection",
                "verdict": "helpful",
                "correctedManually": false
            }
        })),
        "declare_work" => Some(json!({
            "action": "declare_work",
            "input": {
                "title": "Curate principal identity concepts",
                "kind": "ad_hoc",
                "summary": "Bootstrap durable work attribution before later mutations.",
                "parentWorkId": "work:parent-demo",
                "planId": "plan:demo-main"
            }
        })),
        "checkpoint" => Some(json!({
            "action": "checkpoint",
            "input": {
                "summary": "Checkpoint the current implementation milestone.",
                "taskId": "work:demo-main"
            }
        })),
        "session_repair" => Some(json!({
            "action": "session_repair",
            "input": {
                "operation": "clear_current_task"
            }
        })),
        "outcome" => Some(json!({
            "action": "outcome",
            "input": {
                "kind": "plan_created",
                "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                "summary": "Outlined the validation follow-up plan.",
                "result": "success",
                "taskId": "task:demo-main"
            }
        })),
        "memory" => Some(json!({
            "action": "memory",
            "input": {
                "action": "store",
                "payload": {
                    "kind": "episodic",
                    "scope": "session",
                    "content": "Concept handles need concept-aware compact follow-through.",
                    "anchors": [sample_node_anchor(
                        "prism_mcp",
                        "prism_mcp::compact_tools::concept::QueryHost::compact_concept",
                        "method"
                    )],
                    "trust": 0.82,
                    "metadata": {
                        "provenance": {
                            "kind": "manual_memory",
                            "origin": "schema_example"
                        }
                    }
                }
            }
        })),
        "concept" => Some(json!({
            "action": "concept",
            "input": {
                "operation": "promote",
                "canonicalName": "validation_pipeline",
                "summary": "Checks, likely tests, and recent failures behind a change.",
                "coreMembers": [
                    sample_node_id_input("demo", "demo::validation_recipe", "function"),
                    sample_node_id_input("demo", "demo::runtime_status", "function")
                ],
                "aliases": ["validation", "checks"],
                "evidence": ["Promoted from repeated validation worksets."]
            }
        })),
        "contract" => Some(json!({
            "action": "contract",
            "input": {
                "operation": "promote",
                "name": "runtime status surface",
                "summary": "The runtime status entry point remains available for internal diagnostics consumers.",
                "kind": "interface",
                "subject": {
                    "anchors": [sample_node_anchor("demo", "demo::runtime_status", "function")]
                },
                "guarantees": [{
                    "statement": "Internal diagnostics callers can query runtime status without reconstructing daemon state.",
                    "strength": "hard",
                    "evidenceRefs": ["runtime-status-tests"]
                }],
                "consumers": [{
                    "conceptHandles": ["concept://runtime_surface"]
                }],
                "validations": [{
                    "id": "cargo test -p prism-mcp runtime_status",
                    "summary": "Covers the runtime status surface."
                }],
                "evidence": ["Promoted from repeated runtime-inspection work."],
                "scope": "session"
            }
        })),
        "concept_relation" => Some(json!({
            "action": "concept_relation",
            "input": {
                "operation": "upsert",
                "sourceHandle": "concept://validation_pipeline",
                "targetHandle": "concept://runtime_surface",
                "kind": "often_used_with",
                "confidence": 0.82,
                "scope": "session",
                "evidence": ["Validation work usually routes through the runtime surface."],
                "taskId": "task:demo-main"
            }
        })),
        "curator_promote_concept" => Some(json!({
            "action": "curator_promote_concept",
            "input": {
                "jobId": "curator-job:demo",
                "proposalIndex": 0,
                "scope": "session",
                "note": "Accept the hotspot concept proposal after review.",
                "taskId": "task:demo-main"
            }
        })),
        "infer_edge" => Some(json!({
            "action": "infer_edge",
            "input": {
                "source": sample_node_id_input("demo", "demo::validation_recipe", "function"),
                "target": sample_node_id_input("demo", "demo::runtime_status", "function"),
                "kind": "related_to",
                "confidence": 0.74,
                "scope": "persisted",
                "evidence": ["Observed together in repeated validation worksets."],
                "taskId": "task:demo-main"
            }
        })),
        "heartbeat_lease" => Some(json!({
            "action": "heartbeat_lease",
            "input": {
                "taskId": "coord-task:1"
            }
        })),
        "coordination" => Some(json!({
            "action": "coordination",
            "input": {
                "kind": "task_create",
                "payload": {
                    "planId": "plan:demo-main",
                    "title": "Validate compact mutate examples",
                    "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                    "acceptance": [{
                        "label": "Schema examples cover every mutate action"
                    }]
                },
                "taskId": "task:demo-main"
            }
        })),
        "claim" => Some(json!({
            "action": "claim",
            "input": {
                "action": "acquire",
                "payload": {
                    "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                    "capability": "edit",
                    "mode": "soft_exclusive",
                    "ttlSeconds": 1800,
                    "coordinationTaskId": "coord-task:1"
                },
                "taskId": "task:demo-main"
            }
        })),
        "artifact" => Some(json!({
            "action": "artifact",
            "input": {
                "action": "propose",
                "payload": {
                    "taskId": "coord-task:1",
                    "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                    "diffRef": "patch:demo-validation",
                    "requiredValidations": ["cargo test -p prism-mcp"],
                    "riskScore": 0.35
                },
                "taskId": "task:demo-main"
            }
        })),
        "test_ran" => Some(json!({
            "action": "test_ran",
            "input": {
                "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                "test": "cargo test -p prism-mcp prism_mutate_schema_surfaces_action_specific_examples",
                "passed": true,
                "command": ["cargo", "test", "-p", "prism-mcp", "prism_mutate_schema_surfaces_action_specific_examples"],
                "taskId": "task:demo-main"
            }
        })),
        "failure_observed" => Some(json!({
            "action": "failure_observed",
            "input": {
                "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                "summary": "Schema example was missing the mutate action payload shape.",
                "trace": "trace:schema-example-missing-payload-shape",
                "taskId": "task:demo-main"
            }
        })),
        "fix_validated" => Some(json!({
            "action": "fix_validated",
            "input": {
                "anchors": [sample_node_anchor("demo", "demo::validation_recipe", "function")],
                "summary": "Mutation schema now exposes concrete payload shapes and examples.",
                "command": ["cargo", "test", "-p", "prism-mcp", "prism_mutate_schema_surfaces_action_specific_examples"],
                "taskId": "task:demo-main"
            }
        })),
        "curator_apply_proposal" => Some(json!({
            "action": "curator_apply_proposal",
            "input": {
                "jobId": "curator:1",
                "proposalIndex": 0,
                "note": "Apply the reviewed proposal.",
                "options": {
                    "conceptScope": "repo",
                    "memoryTrust": 0.86,
                    "edgeScope": "persisted"
                },
                "taskId": "task:demo-main"
            }
        })),
        "curator_promote_edge" => Some(json!({
            "action": "curator_promote_edge",
            "input": {
                "jobId": "curator:1",
                "proposalIndex": 0,
                "scope": "persisted",
                "note": "Promote the inferred validation edge.",
                "taskId": "task:demo-main"
            }
        })),
        "curator_promote_memory" => Some(json!({
            "action": "curator_promote_memory",
            "input": {
                "jobId": "curator:1",
                "proposalIndex": 1,
                "trust": 0.86,
                "note": "Promote the repeated validation lesson.",
                "taskId": "task:demo-main"
            }
        })),
        "curator_reject_proposal" => Some(json!({
            "action": "curator_reject_proposal",
            "input": {
                "jobId": "curator:1",
                "proposalIndex": 2,
                "reason": "The proposed memory was still too task-specific.",
                "taskId": "task:demo-main"
            }
        })),
        _ => None,
    }
}

fn extra_prism_mutate_examples() -> Vec<Value> {
    vec![
        json!({
            "action": "memory",
            "input": {
                "action": "retire",
                "payload": {
                    "memoryId": "memory:demo-main",
                    "retirementReason": "Superseded by a newer validated routing rule."
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_bootstrap",
                "payload": {
                    "plan": {
                        "title": "Investigate refresh path latency",
                        "goal": "Investigate refresh path latency"
                    },
                    "tasks": [{
                        "clientId": "t0",
                        "title": "Capture baseline timings"
                    }, {
                        "clientId": "t1",
                        "title": "Compare the slow phases",
                        "dependsOn": ["t0"]
                    }],
                    "nodes": [{
                        "clientId": "n0",
                        "kind": "validate",
                        "title": "Verify the fix",
                        "validationRefs": [{ "id": "bench:refresh-hot-path" }],
                        "dependsOn": ["t1"]
                    }],
                    "edges": [{
                        "fromClientId": "t1",
                        "toClientId": "n0",
                        "kind": "validates"
                    }]
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_create",
                "payload": {
                    "title": "Investigate refresh path latency",
                    "goal": "Investigate refresh path latency",
                    "status": "active",
                    "policy": {
                        "defaultClaimMode": "soft_exclusive",
                        "staleAfterGraphChange": false
                    }
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_archive",
                "payload": {
                    "planId": "plan:demo-main"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_node_create",
                "payload": {
                    "planId": "plan:demo-main",
                    "kind": "investigate",
                    "title": "Break down refresh latency",
                    "status": "ready",
                    "validationRefs": [{ "id": "bench:refresh-hot-path" }],
                    "acceptance": [{
                        "label": "Captures no-op and one-file timings",
                        "evidencePolicy": "any"
                    }]
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "update",
                "payload": {
                    "id": "coord-task:1",
                    "status": "in_progress",
                    "title": "Validate compact mutate examples"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_edge_create",
                "payload": {
                    "planId": "plan:demo-main",
                    "fromNodeId": "coord-task:1",
                    "toNodeId": "coord-task:2",
                    "kind": "depends_on"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_update",
                "payload": {
                    "planId": "plan:demo-main",
                    "title": "Investigate refresh path latency deeply",
                    "scheduling": {
                        "importance": 80,
                        "urgency": 70
                    }
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "plan_edge_delete",
                "payload": {
                    "planId": "plan:demo-main",
                    "fromNodeId": "coord-task:1",
                    "toNodeId": "coord-task:2",
                    "kind": "depends_on"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "handoff",
                "payload": {
                    "taskId": "coord-task:1",
                    "toAgent": "agent:reviewer",
                    "summary": "Hand off the reviewable artifact workflow."
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "resume",
                "payload": {
                    "taskId": "coord-task:1",
                    "agent": "agent:demo"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "reclaim",
                "payload": {
                    "taskId": "coord-task:1",
                    "agent": "agent:demo"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "coordination",
            "input": {
                "kind": "handoff_accept",
                "payload": {
                    "taskId": "coord-task:1",
                    "agent": "agent:reviewer"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "claim",
            "input": {
                "action": "renew",
                "payload": {
                    "claimId": "claim:demo-main",
                    "ttlSeconds": 1800
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "claim",
            "input": {
                "action": "release",
                "payload": {
                    "claimId": "claim:demo-main"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "artifact",
            "input": {
                "action": "supersede",
                "payload": {
                    "artifactId": "artifact:demo-main"
                },
                "taskId": "task:demo-main"
            }
        }),
        json!({
            "action": "artifact",
            "input": {
                "action": "review",
                "payload": {
                    "artifactId": "artifact:demo-main",
                    "verdict": "approved",
                    "summary": "Refresh hot path change looks safe.",
                    "validatedChecks": ["cargo test -p prism-mcp"]
                },
                "taskId": "task:demo-main"
            }
        }),
    ]
}

fn with_mutation_credential(mut example: Value) -> Value {
    let Some(object) = example.as_object_mut() else {
        return example;
    };
    object.insert(
        "credential".to_string(),
        json!({
            "credentialId": "credential:demo-main",
            "principalToken": "prism_ptok_demo_main_example"
        }),
    );
    example
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
        "uri": tool_variant_example_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
        "schemaUri": schema_resource_uri("tool-example"),
        "toolName": "prism_mutate",
        "action": "coordination",
        "variant": "plan_bootstrap",
        "discriminator": "kind",
        "targetSchemaUri": tool_variant_schema_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
        "shapeUri": tool_variant_shape_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
        "recipeUri": tool_variant_recipe_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
        "example": {
            "plan": {
                "title": "Improve self-description",
                "goal": "Make PRISM MCP self-describing under truncation"
            }
        },
        "examples": [{
            "plan": {
                "title": "Improve self-description",
                "goal": "Make PRISM MCP self-describing under truncation"
            }
        }],
        "relatedResources": sample_related_resources(),
    })
}

fn tool_shape_payload_example() -> Value {
    json!({
        "uri": tool_action_shape_resource_uri("prism_mutate", "coordination"),
        "schemaUri": schema_resource_uri("tool-shape"),
        "toolName": "prism_mutate",
        "toolSchemaUri": tool_action_schema_resource_uri("prism_mutate", "coordination"),
        "exampleUri": tool_action_example_resource_uri("prism_mutate", "coordination"),
        "description": "Compact shape summary for `prism_mutate` action `coordination`",
        "requiredFields": ["payload"],
        "optionalFields": ["taskId", "planId", "kind"],
        "fields": [{
            "name": "payload",
            "required": true,
            "description": "Tagged coordination payload",
            "types": ["object"],
            "enumValues": [],
            "nestedFields": []
        }],
        "actions": [{
            "action": "coordination",
            "schemaUri": tool_action_schema_resource_uri("prism_mutate", "coordination"),
            "exampleUri": tool_action_example_resource_uri("prism_mutate", "coordination"),
            "shapeUri": tool_action_shape_resource_uri("prism_mutate", "coordination"),
            "recipeUri": tool_action_recipe_resource_uri("prism_mutate", "coordination"),
            "description": "Exact input schema for `prism_mutate` action `coordination`.",
            "requiredFields": ["payload"],
            "optionalFields": ["taskId", "planId", "kind"],
            "fields": [{
                "name": "payload",
                "required": true,
                "description": "Tagged coordination payload",
                "types": ["object"],
                "enumValues": [],
                "nestedFields": []
            }],
            "payloadDiscriminator": "kind",
            "variants": [{
                "tag": "plan_bootstrap",
                "discriminator": "kind",
                "schemaUri": tool_variant_schema_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
                "exampleUri": tool_variant_example_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
                "shapeUri": tool_variant_shape_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
                "recipeUri": tool_variant_recipe_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
                "requiredFields": ["plan", "tasks"],
                "optionalFields": ["nodes", "edges"],
                "fields": []
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
            "name": "prism_mutate",
            "description": "Input schema for coarse PRISM state mutations and tagged action unions.",
            "schemaUri": tool_schema_resource_uri("prism_mutate"),
            "exampleUri": tool_example_resource_uri("prism_mutate"),
            "shapeUri": tool_shape_resource_uri("prism_mutate")
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
            "description": "Nested kind values accepted by prism_mutate action coordination.",
            "values": [{
                "value": "plan_bootstrap",
                "aliases": [],
                "description": "Create a plan and its initial graph in one authoritative write."
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
            "surfaceKind": "tool_variant",
            "name": "prism_mutate.coordination.plan_bootstrap",
            "fullUri": tool_variant_schema_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
            "schemaUri": tool_variant_schema_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
            "exampleUri": tool_variant_example_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
            "shapeUri": tool_variant_shape_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
            "recipeUri": tool_variant_recipe_resource_uri("prism_mutate", "coordination", "plan_bootstrap"),
            "fullBytes": 4096,
            "schemaBytes": 4096,
            "exampleBytes": 1024,
            "shapeBytes": 2048,
            "recipeBytes": 512,
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
        "workspaceRoot": "/workspace/demo",
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
            "nextAction": "Proceed with authoritative `prism_mutate` calls without supplying `credential` on this bridge."
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
        "workspaceRoot": "/workspace/demo",
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
            "description": "Nested kind values accepted by prism_mutate action coordination.",
            "values": [{
                "value": "plan_bootstrap",
                "aliases": [],
                "description": "Create a plan and its initial graph in one authoritative write."
            }, {
                "value": "task_create",
                "aliases": [],
                "description": "Create a coordination task."
            }, {
                "value": "plan_node_create",
                "aliases": [],
                "description": "Create a first-class plan node."
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
        "plans": [{
            "planId": "plan:1",
            "title": "Migrate persistence to DB-native runtime storage",
            "goal": "Move persistence away from snapshot-authoritative writes.",
            "status": "active",
            "scope": "repo",
            "kind": "migration",
            "rootNodeIds": ["coord-task:1"],
            "summary": {
                "planId": "plan:1",
                "totalNodes": 6,
                "completedNodes": 0,
                "runningNodes": 0,
                "readyNodes": 1,
                "blockedNodes": 0,
                "pendingNodes": 5,
                "actionableNodes": 1,
                "executionBlockedNodes": 0,
                "completionGatedNodes": 0,
                "validationGatedNodes": 0,
                "staleNodes": 0,
                "reviewRequiredNodes": 0,
                "claimBlockedNodes": 0,
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
            "createdFrom": "manual",
            "rootNodeIds": ["coord-task:1"]
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
            "toolName": "prism_query",
            "schemaUri": tool_schema_resource_uri("prism_query"),
            "description": "Input schema for programmable read-only TypeScript PRISM queries.",
            "exampleInput": tool_input_example("prism_query"),
        }, {
            "toolName": "prism_mutate",
            "schemaUri": tool_schema_resource_uri("prism_mutate"),
            "description": "Input schema for coarse PRISM state mutations and tagged action unions.",
            "exampleInput": tool_input_example("prism_mutate"),
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
