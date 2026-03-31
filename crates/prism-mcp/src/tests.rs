use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use rmcp::transport::{IntoTransport, Transport};

use super::query_replay_cases::{replay_cases, ReplayExpectation, ReplayHostProfile};
use super::*;
use crate::server_surface::{MutationDashboardMeta, MutationRefreshPolicy};
use crate::tests_support::*;
use prism_agent::{InferenceSnapshot, InferredEdgeScope};
use prism_coordination::{CoordinationPolicy, CoordinationStore, PlanCreateInput, TaskCreateInput};
use prism_core::{
    index_workspace_session, index_workspace_session_with_curator, ValidationFeedbackCategory,
    ValidationFeedbackRecord, ValidationFeedbackVerdict,
};
use prism_curator::{
    CandidateConcept, CandidateConceptOperation, CandidateEdge, CandidateMemory,
    CandidateMemoryEvidence, CandidateRiskSummary, CandidateValidationRecipe, CuratorBackend,
    CuratorContext, CuratorJob, CuratorProposal, CuratorRun,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language,
    Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, PlanEdgeKind, PlanId, Span,
    SymbolFingerprint, TaskId,
};
use prism_js::{AnchorRefView, ContractKindView, ContractStabilityView, ContractStatusView};
use prism_memory::{
    MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeMemory, OutcomeResult, RecallQuery,
};
use prism_query::{
    ConceptDecodeLens, ConceptPacket, ConceptProvenance, ConceptScope, ContractKind,
    ContractStability, ContractStatus,
};
use prism_store::{Graph, SqliteStore, Store};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use tracing::Level;

#[test]
fn cli_no_coordination_flag_disables_coordination_features() {
    let cli = PrismMcpCli::parse_from(["prism-mcp", "--no-coordination"]);
    let features = cli.features();
    assert_eq!(features.mode_label(), "simple");
    assert!(!features.coordination.workflow);
    assert!(!features.coordination.claims);
    assert!(!features.coordination.artifacts);
}

#[test]
fn executes_symbol_query() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result["path"], "demo::main");
    assert_eq!(result.result["kind"], "Function");
    assert!(result.diagnostics.is_empty());
}

#[test]
fn coordination_mutations_flow_through_query_runtime() {
    let host = host_with_node(demo_node());
    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship coordination" }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(plan.state["goal"], "Ship coordination");

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit main",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let claim = host
        .store_claim(
            test_session(&host).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(claim.claim_id.is_some());

    let artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:1"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(artifact.artifact_id.is_some());

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "dispatch plan",
        ),
    );
    let plan_id = plan.state["id"].as_str().unwrap();
    let plan_value = execution
        .dispatch("plan", &json!({ "planId": plan_id }).to_string())
        .unwrap();
    let ready_value = execution
        .dispatch("readyTasks", &json!({ "planId": plan_id }).to_string())
        .unwrap();
    let claims_value = execution
            .dispatch(
                "claims",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }] }"#,
            )
            .unwrap();
    let simulated_value = execution
            .dispatch(
                "simulateClaim",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }], "capability": "Edit", "mode": "HardExclusive" }"#,
            )
            .unwrap_or_else(|error| panic!("simulateClaim dispatch failed: {error:#}"));
    let artifacts_value = execution
        .dispatch("artifacts", &json!({ "taskId": task_id }).to_string())
        .unwrap();
    assert_eq!(plan_value["goal"], "Ship coordination");
    assert_eq!(ready_value.as_array().unwrap().len(), 1);
    assert_eq!(claims_value.as_array().unwrap().len(), 1);
    assert_eq!(artifacts_value.as_array().unwrap().len(), 1);
    assert!(simulated_value.as_array().unwrap().is_empty());
}

#[test]
fn plan_node_mutations_return_graph_native_views() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship first-class plans" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    host.current_prism()
        .replace_curated_concepts(vec![ConceptPacket {
            handle: "concept://native_plan_runtime".to_string(),
            canonical_name: "native_plan_runtime".to_string(),
            summary: "Native plan runtime concept.".to_string(),
            aliases: vec!["plan runtime".to_string()],
            confidence: 0.95,
            core_members: Vec::new(),
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Seeded for MCP native plan node mutation test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "seed".to_string(),
                task_id: None,
            },
            publication: None,
        }]);

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Track plan artifacts"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validation_artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:demo-main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let review_artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:review-main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validation_outcome = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::TestRan,
                anchors: Vec::new(),
                summary: "demo main validated".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();
    let review_outcome = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: Vec::new(),
                summary: "review main validated".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Review main"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "kind": "validate",
                    "title": "Edit main",
                    "summary": "Gather validation evidence",
                    "validationRefs": [{ "id": "validation:demo-main" }],
                    "isAbstract": true,
                    "bindings": {
                        "conceptHandles": ["concept://native_plan_runtime"],
                        "artifactRefs": [validation_artifact.artifact_id.as_deref().unwrap()],
                        "memoryRefs": ["memory:demo-main"],
                        "outcomeRefs": [validation_outcome.event_id.as_str()]
                    },
                    "acceptance": [{
                        "label": "main is updated",
                        "requiredChecks": [{ "id": "validation:demo-main" }],
                        "evidencePolicy": "review-and-validation"
                    }],
                    "priority": 3,
                    "tags": ["plans", "validation", "plans"]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();
    assert_eq!(node.state["title"], "Edit main");
    assert_eq!(node.state["kind"], "Validate");
    assert_eq!(node.state["summary"], "Gather validation evidence");
    assert_eq!(node.state["isAbstract"], true);
    assert_eq!(node.state["priority"], 3);
    assert_eq!(node.state["tags"], json!(["plans", "validation"]));
    assert_eq!(
        node.state["bindings"]["conceptHandles"][0],
        "concept://native_plan_runtime"
    );
    assert_eq!(
        node.state["acceptance"][0]["requiredChecks"][0]["id"],
        "validation:demo-main"
    );
    assert_eq!(
        node.state["validationRefs"][0]["id"],
        "validation:demo-main"
    );
    assert_eq!(
        node.state["acceptance"][0]["evidencePolicy"],
        "ReviewAndValidation"
    );

    let updated = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": node_id.clone(),
                    "kind": "review",
                    "title": "Edit main safely",
                    "summary": "Review the validation evidence",
                    "status": "in-progress",
                    "assignee": "agent:reviewer",
                    "isAbstract": false,
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "bindings": {
                        "conceptHandles": ["concept://native_plan_runtime"],
                        "artifactRefs": [review_artifact.artifact_id.as_deref().unwrap()],
                        "memoryRefs": ["memory:review-main"],
                        "outcomeRefs": [review_outcome.event_id.as_str()]
                    },
                    "dependsOn": [dependency_id.clone()],
                    "acceptance": [{
                        "label": "main still compiles",
                        "requiredChecks": [{ "id": "validation:cargo-test" }],
                        "evidencePolicy": "validation-only"
                    }],
                    "priority": 7,
                    "tags": ["review", "validation", "review"]
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(updated.state["title"], "Edit main safely");
    assert_eq!(updated.state["kind"], "Review");
    assert_eq!(updated.state["summary"], "Review the validation evidence");
    assert_eq!(updated.state["status"], "InProgress");
    assert_eq!(updated.state["assignee"], "agent:reviewer");
    assert_eq!(updated.state["acceptance"].as_array().unwrap().len(), 1);
    assert_eq!(updated.state["isAbstract"], false);
    assert_eq!(updated.state["priority"], 7);
    assert_eq!(updated.state["tags"], json!(["review", "validation"]));
    let binding_anchors = updated.state["bindings"]["anchors"].as_array().unwrap();
    assert_eq!(binding_anchors.len(), 2);
    assert!(binding_anchors.iter().any(
        |anchor| anchor["Node"]["path"] == "demo::main" && anchor["Node"]["kind"] == "Function"
    ));
    assert_eq!(
        updated.state["bindings"]["artifactRefs"][0],
        review_artifact.artifact_id.as_deref().unwrap()
    );
    assert_eq!(
        updated.state["acceptance"][0]["requiredChecks"][0]["id"],
        "validation:cargo-test"
    );
    assert_eq!(
        updated.state["acceptance"][0]["evidencePolicy"],
        "ValidationOnly"
    );

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(graph.root_nodes.iter().any(|root| root.0 == dependency_id));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == node_id));
    let graph_node = graph
        .nodes
        .iter()
        .find(|node| node.id.0 == node_id)
        .expect("graph node");
    assert_eq!(graph_node.kind, prism_ir::PlanNodeKind::Review);
    assert_eq!(
        graph_node.summary.as_deref(),
        Some("Review the validation evidence")
    );
    assert_eq!(graph_node.priority, Some(7));
    assert_eq!(graph_node.tags, vec!["review", "validation"]);
    assert_eq!(
        graph_node.bindings.concept_handles,
        vec!["concept://native_plan_runtime"]
    );

    let cleared = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": node_id,
                    "assignee": { "op": "clear" },
                    "summary": { "op": "clear" },
                    "priority": { "op": "clear" }
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(cleared.state["summary"], Value::Null);
    assert_eq!(cleared.state["priority"], Value::Null);
    assert_eq!(cleared.state["assignee"], Value::Null);
}

#[test]
fn native_plan_node_completion_rejects_missing_review_and_validation() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Require completion evidence",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Ship main",
                    "acceptance": [{
                        "label": "main is validated",
                        "requiredChecks": [{ "id": "validation:ci" }],
                        "evidencePolicy": "review-and-validation"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "native completion blockers",
        ),
    );
    let blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{node_id}" }}"#),
        )
        .unwrap();
    let kinds = blockers
        .as_array()
        .unwrap()
        .iter()
        .map(|blocker| blocker["kind"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"ReviewRequired".to_string()));
    assert!(kinds.contains(&"ValidationRequired".to_string()));

    let result = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": node_id,
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .expect("completion should return a structured rejection");
    assert!(result.rejected);
    assert!(result
        .violations
        .iter()
        .any(|violation| violation.code == "review_required"));
}

#[test]
fn coordination_update_routes_plain_ids_to_native_plan_nodes() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Unify workflow updates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Refine compact update semantics"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let updated = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": node_id.clone(),
                    "status": "waiting",
                    "summary": "Blocked on a follow-up schema tweak",
                    "priority": 5,
                    "tags": ["compact", "workflow", "compact"]
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(updated.state["id"], node_id);
    assert_eq!(updated.state["status"], "Waiting");
    assert_eq!(
        updated.state["summary"],
        "Blocked on a follow-up schema tweak"
    );
    assert_eq!(updated.state["priority"], 5);
    assert_eq!(updated.state["tags"], json!(["compact", "workflow"]));
}

#[test]
fn coordination_update_routes_plain_ids_to_coordination_tasks() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Unify workflow updates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Update task through unified mutation"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let updated = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task_id.clone(),
                    "status": "in_review",
                    "title": "Updated through unified mutation"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(updated.state["id"], task_id);
    assert_eq!(updated.state["status"], "InReview");
    assert_eq!(updated.state["title"], "Updated through unified mutation");
}

#[test]
fn coordination_update_routes_task_backed_ids_to_plan_nodes_for_node_only_fields() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Unify workflow updates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Update task through unified mutation"
                }),
                task_id: None,
            },
        )
        .unwrap();

    let updated = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task.state["id"].as_str().unwrap(),
                    "summary": "This should not be accepted for a task-backed id"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(
        updated.state["summary"],
        "This should not be accepted for a task-backed id"
    );
}

#[test]
fn native_plan_node_completion_accepts_current_task_validation_events_without_anchors() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Require validation evidence",
                    "policy": { "requireValidationForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let required_test =
        "test:cargo test -p prism-js api_reference_mentions_primary_tool -- --nocapture";
    let required_build = "build:cargo build --release -p prism-cli -p prism-mcp";

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "kind": "validate",
                    "title": "Validate migration",
                    "acceptance": [{
                        "label": "migration is validated",
                        "requiredChecks": [
                            { "id": required_test },
                            { "id": required_build }
                        ],
                        "evidencePolicy": "validation-only"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    host.configure_session(
        test_session(&host).as_ref(),
        PrismConfigureSessionArgs {
            limits: None,
            current_task_id: Some(node_id.clone()),
            coordination_task_id: None,
            current_task_description: Some("Validate migration".to_string()),
            current_task_tags: None,
            clear_current_task: None,
            current_agent: None,
            clear_current_agent: None,
        },
    )
    .unwrap();

    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::TestRan,
            anchors: Vec::new(),
            summary: "api reference validation passed".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: Some(vec![OutcomeEvidenceInput::Test {
                name: required_test.to_string(),
                passed: true,
            }]),
            task_id: None,
        },
    )
    .unwrap();
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: Vec::new(),
            summary: "release build passed".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: Some(vec![OutcomeEvidenceInput::Command {
                argv: vec![
                    "cargo".to_string(),
                    "build".to_string(),
                    "--release".to_string(),
                    "-p".to_string(),
                    "prism-cli".to_string(),
                    "-p".to_string(),
                    "prism-mcp".to_string(),
                ],
                passed: true,
            }]),
            task_id: None,
        },
    )
    .unwrap();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "native completion accepts current task evidence",
        ),
    );
    let blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{node_id}" }}"#),
        )
        .unwrap();
    assert!(blockers.as_array().unwrap().is_empty());

    let completed = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": node_id,
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(completed.state["status"], "Completed");
}

#[test]
fn plan_edge_mutations_update_projected_dependency_graph() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Shape execution edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Prepare change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Apply change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let created = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": node_id.clone(),
                    "toNodeId": dependency_id.clone(),
                    "kind": "depends_on"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(created.state["from"], node_id);
    assert_eq!(created.state["to"], dependency_id);
    assert_eq!(created.state["kind"], "DependsOn");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == node_id));

    let deleted = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeDelete,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": node_id.clone(),
                    "toNodeId": dependency_id.clone(),
                    "kind": "depends_on"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(deleted.state["from"], node_id);
    assert_eq!(deleted.state["to"], dependency_id);
    assert_eq!(deleted.state["kind"], "DependsOn");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(!graph.edges.iter().any(|edge| edge.from.0 == node_id
        && edge.to.0 == dependency_id
        && edge.kind == PlanEdgeKind::DependsOn));
    assert!(graph.root_nodes.iter().any(|root| root.0 == node_id));
}

#[test]
fn plan_edge_mutations_support_non_dependency_edge_kinds() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Shape native graph edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let source_id = source.state["id"].as_str().unwrap().to_string();

    let target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "kind": "validate",
                    "title": "Validate change",
                    "validationRefs": [{ "id": "validation:change" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let target_id = target.state["id"].as_str().unwrap().to_string();

    let created = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": source_id.clone(),
                    "toNodeId": target_id.clone(),
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(created.state["kind"], "Validates");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.edges.iter().any(|edge| edge.from.0 == source_id
        && edge.to.0 == target_id
        && edge.kind == PlanEdgeKind::Validates));

    let deleted = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeDelete,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": source_id.clone(),
                    "toNodeId": target_id.clone(),
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(deleted.state["kind"], "Validates");

    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id))
        .expect("plan graph");
    assert!(!graph.edges.iter().any(|edge| edge.from.0 == source_id
        && edge.to.0 == target_id
        && edge.kind == PlanEdgeKind::Validates));
}

#[test]
fn plan_edge_mutations_enforce_native_edge_semantics() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Enforce edge semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let parent = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Parent group",
                    "isAbstract": true
                }),
                task_id: None,
            },
        )
        .unwrap();
    let parent_id = parent.state["id"].as_str().unwrap().to_string();

    let child = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Child work"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let child_id = child.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": child_id.clone(),
                "toNodeId": parent_id.clone(),
                "kind": "child_of"
            }),
            task_id: None,
        },
    )
    .unwrap();
    let graph = host
        .current_prism()
        .plan_graph(&PlanId::new(plan_id.clone()))
        .expect("plan graph");
    assert!(graph.root_nodes.iter().any(|root| root.0 == parent_id));
    assert!(!graph.root_nodes.iter().any(|root| root.0 == child_id));

    let invalid_target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Plain work"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let invalid_target_id = invalid_target.state["id"].as_str().unwrap().to_string();

    let validates_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": child_id.clone(),
                    "toNodeId": invalid_target_id,
                    "kind": "validates"
                }),
                task_id: None,
            },
        )
        .expect_err("validates should require validate target");
    assert!(validates_error
        .to_string()
        .contains("must target a Validate node"));

    let handoff_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id,
                    "fromNodeId": child_id,
                    "toNodeId": parent_id,
                    "kind": "handoff_to"
                }),
                task_id: None,
            },
        )
        .expect_err("handoff should reject abstract target");
    assert!(handoff_error
        .to_string()
        .contains("must connect executable nodes"));
}

#[test]
fn plan_node_mutations_reject_runtime_only_binding_handles() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject runtime binding handles" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Bad binding node",
                    "bindings": {
                        "conceptHandles": ["handle:1"]
                    }
                }),
                task_id: None,
            },
        )
        .expect_err("runtime-only binding handle should reject");
    assert!(error
        .to_string()
        .contains("runtime-only handles like `handle:1`"));
}

#[test]
fn plan_node_mutations_reject_missing_published_binding_refs() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject missing published plan refs" }),
                task_id: None,
            },
        )
        .unwrap();
    let error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Bad binding node",
                    "bindings": {
                        "conceptHandles": ["concept://missing"]
                    }
                }),
                task_id: None,
            },
        )
        .expect_err("missing concept handle should reject");
    assert!(error
        .to_string()
        .contains("must reference an existing concept handle"));
}

#[test]
fn plan_query_reads_surface_native_ready_nodes_and_blockers() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Read native plan runtime semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let blocked = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Blocked" }),
                task_id: None,
            },
        )
        .unwrap();
    let blocked_id = blocked.state["id"].as_str().unwrap().to_string();

    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Dependency" }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();

    let validator = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Validator",
                    "kind": "Validate",
                    "validationRefs": [{ "id": "validation:validator" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let validator_id = validator.state["id"].as_str().unwrap().to_string();

    let handoff_source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Handoff source",
                    "status": "InProgress"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let handoff_source_id = handoff_source.state["id"].as_str().unwrap().to_string();

    let handoff_target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Handoff target" }),
                task_id: None,
            },
        )
        .unwrap();
    let handoff_target_id = handoff_target.state["id"].as_str().unwrap().to_string();

    let free = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id.clone(), "title": "Free" }),
                task_id: None,
            },
        )
        .unwrap();
    let free_id = free.state["id"].as_str().unwrap().to_string();

    for payload in [
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": blocked_id.clone(),
            "toNodeId": dependency_id.clone(),
            "kind": "depends_on"
        }),
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": blocked_id.clone(),
            "toNodeId": validator_id.clone(),
            "kind": "validates"
        }),
        json!({
            "planId": plan_id.clone(),
            "fromNodeId": handoff_source_id.clone(),
            "toNodeId": handoff_target_id.clone(),
            "kind": "handoff_to"
        }),
    ] {
        host.store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload,
                task_id: None,
            },
        )
        .unwrap();
    }

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "plan query reads",
        ),
    );
    let ready_nodes = execution
        .dispatch("planReadyNodes", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let blocked_node_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{blocked_id}" }}"#),
        )
        .unwrap();
    let handoff_target_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{handoff_target_id}" }}"#),
        )
        .unwrap();
    let execution_overlays = execution
        .dispatch("planExecution", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let summary = execution
        .dispatch("planSummary", &format!(r#"{{ "planId": "{plan_id}" }}"#))
        .unwrap();
    let plans = execution
        .dispatch(
            "plans",
            r#"{ "contains": "native plan runtime", "limit": 5 }"#,
        )
        .unwrap();
    let next = execution
        .dispatch(
            "planNext",
            &format!(r#"{{ "planId": "{plan_id}", "limit": 3 }}"#),
        )
        .unwrap();

    let ready_ids = ready_nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let mut ready_ids = ready_ids;
    ready_ids.sort();
    let mut expected_ready_ids = vec![
        dependency_id.clone(),
        validator_id.clone(),
        handoff_source_id.clone(),
        free_id,
    ];
    expected_ready_ids.sort();
    assert_eq!(ready_ids, expected_ready_ids);

    let blocked_kinds = blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .map(|blocker| blocker["kind"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        blocked_kinds,
        vec![
            "Dependency".to_string(),
            "ValidationGate".to_string(),
            "ValidationRequired".to_string()
        ]
    );
    assert!(blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| {
            blocker["kind"] == Value::String("ValidationGate".to_string())
                && blocker["validationChecks"] == json!(["validation:validator"])
        }));
    assert!(blocked_node_blockers
        .as_array()
        .unwrap()
        .iter()
        .any(|blocker| {
            blocker["kind"] == Value::String("ValidationRequired".to_string())
                && blocker["validationChecks"] == json!(["validation:validator"])
        }));
    assert_eq!(
        handoff_target_blockers[0]["kind"],
        Value::String("Handoff".to_string())
    );
    assert!(execution_overlays
        .as_array()
        .unwrap()
        .iter()
        .any(|overlay| {
            overlay["nodeId"] == Value::String(handoff_target_id.clone())
                && overlay["awaitingHandoffFrom"] == Value::String(handoff_source_id.clone())
        }));
    assert_eq!(summary["totalNodes"], Value::from(6));
    assert_eq!(summary["actionableNodes"], Value::from(4));
    assert_eq!(summary["executionBlockedNodes"], Value::from(2));
    assert_eq!(summary["completionGatedNodes"], Value::from(1));
    assert_eq!(summary["validationGatedNodes"], Value::from(1));
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(plans[0]["planId"], Value::String(plan_id.clone()));
    assert_eq!(plans[0]["summary"]["actionableNodes"], Value::from(4));
    let next_id = next[0]["node"]["id"].as_str().unwrap();
    assert!(matches!(
        next_id,
        id if id == dependency_id || id == validator_id || id == handoff_source_id
    ));
    assert_eq!(next[0]["actionable"], Value::Bool(true));
    assert_eq!(next[0]["unblocks"].as_array().unwrap().len(), 1);
}

#[test]
fn plan_query_reads_surface_child_hierarchy_completion_gates() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Read hierarchy completion semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let parent = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Parent",
                    "kind": "Note",
                    "isAbstract": true
                }),
                task_id: None,
            },
        )
        .unwrap();
    let parent_id = parent.state["id"].as_str().unwrap().to_string();

    let child = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Child",
                    "status": "InProgress"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let child_id = child.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": child_id,
                "toNodeId": parent_id.clone(),
                "kind": "child_of"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "child hierarchy semantics",
        ),
    );
    let parent_blockers = execution
        .dispatch(
            "planNodeBlockers",
            &format!(r#"{{ "planId": "{plan_id}", "nodeId": "{parent_id}" }}"#),
        )
        .unwrap();
    let next = execution
        .dispatch(
            "planNext",
            &format!(r#"{{ "planId": "{plan_id}", "limit": 3 }}"#),
        )
        .unwrap();

    assert_eq!(
        parent_blockers[0]["kind"],
        Value::String("ChildIncomplete".to_string())
    );
    assert!(next
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["node"]["id"] != parent_id)
        .and_then(|entry| entry["unblocks"].as_array())
        .is_some_and(|unblocks| {
            unblocks
                .iter()
                .any(|node| node == &Value::String(parent_id.clone()))
        }));
}

#[test]
fn mcp_returns_structured_coordination_rejections_and_persists_them() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Ship reviewed change",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();

    let rejected = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task.state["id"].as_str().unwrap(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected.rejected);
    assert!(!rejected.event_ids.is_empty());
    assert_eq!(rejected.state, Value::Null);
    assert!(rejected
        .violations
        .iter()
        .any(|violation| violation.code == "review_required"));

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let events = reloaded.current_prism().coordination_snapshot().events;
    assert_eq!(
        events.last().unwrap().kind,
        prism_ir::CoordinationEventKind::MutationRejected
    );
}

#[test]
fn mcp_exposes_policy_violations_through_prism_query() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Ship reviewed change",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let rejected = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected.rejected);

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "dispatch policy violations",
        ),
    );
    let violations = execution
        .dispatch(
            "policyViolations",
            &json!({
                "planId": plan_id,
                "taskId": task_id,
                "limit": 5
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(violations.as_array().unwrap().len(), 1);
    assert!(violations[0]["violations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|violation| violation["code"] == "review_required"));
}

#[test]
fn configure_session_binds_current_agent_and_task_create_inherits_it() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let session = host
        .configure_session(
            test_session(&host).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-a".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();
    assert_eq!(session.current_agent.as_deref(), Some("agent-a"));

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Bind agent identity" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert_eq!(task.state["assignee"], "agent-a");

    let claims = host.current_prism().coordination_snapshot().claims;
    assert!(claims.is_empty());
}

#[test]
fn configure_session_can_bind_coordination_task_without_current_task_id() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Bind a coordination task into session state" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let session = host
        .configure_session(
            test_session(&host).as_ref(),
            PrismConfigureSessionArgs {
                limits: None,
                current_task_id: None,
                coordination_task_id: Some(task_id.clone()),
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: None,
                clear_current_agent: None,
            },
        )
        .unwrap();

    let current_task = session.current_task.expect("current task should be set");
    assert_eq!(current_task.task_id, task_id);
    assert_eq!(current_task.description.as_deref(), Some("Edit alpha"));
    assert_eq!(
        current_task.coordination_task_id.as_deref(),
        Some(current_task.task_id.as_str())
    );
}

#[test]
fn plan_edge_mutations_reject_invalid_scheduling_graphs() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Reject invalid native graph edges" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let source = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement change"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let source_id = source.state["id"].as_str().unwrap().to_string();

    let target = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Validate change",
                    "kind": "Validate",
                    "validationRefs": [{ "id": "validation:change" }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let target_id = target.state["id"].as_str().unwrap().to_string();

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanEdgeCreate,
            payload: json!({
                "planId": plan_id.clone(),
                "fromNodeId": source_id.clone(),
                "toNodeId": target_id.clone(),
                "kind": "validates"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let cycle_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "fromNodeId": target_id.clone(),
                    "toNodeId": source_id.clone(),
                    "kind": "handoff_to"
                }),
                task_id: None,
            },
        )
        .expect_err("cross-kind cycle should be rejected");
    assert!(cycle_error.to_string().contains("introduce a cycle"));

    let self_error = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanEdgeCreate,
                payload: json!({
                    "planId": plan_id,
                    "fromNodeId": source_id.clone(),
                    "toNodeId": source_id,
                    "kind": "blocks"
                }),
                task_id: None,
            },
        )
        .expect_err("self edge should be rejected");
    assert!(self_error.to_string().contains("cannot target itself"));
}

#[test]
fn mcp_plan_update_completes_plan_and_closed_plan_rejects_new_claims() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Single pass coordination" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let rejected_plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected_plan.rejected);
    assert!(rejected_plan
        .violations
        .iter()
        .any(|violation| violation.code == "incomplete_plan_tasks"));

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::Update,
            payload: json!({
                "id": task_id.clone(),
                "status": "completed"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let completed_plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(!completed_plan.rejected);
    assert_eq!(completed_plan.state["status"], "Completed");

    let rejected_claim = host
        .store_claim(
            test_session(&host).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected_claim.rejected);
    assert!(rejected_claim
        .violations
        .iter()
        .any(|violation| violation.code == "plan_closed"));
}

#[test]
fn mcp_plan_update_rehydrates_stale_coordination_runtime_before_mutating() {
    let root = temp_workspace();
    let workspace = index_workspace_session(&root).unwrap();
    let plan_id = workspace
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:published-runtime-split"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-runtime-split")),
                    causation: None,
                },
                "Mutation should rehydrate current published plans".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let workspace = host.workspace.as_ref().expect("workspace host");
    let state = workspace
        .load_coordination_plan_state()
        .unwrap()
        .expect("coordination plan state");
    workspace
        .prism_arc()
        .replace_coordination_snapshot_and_plan_graphs(
            prism_coordination::CoordinationSnapshot::default(),
            state.plan_graphs.clone(),
            state.execution_overlays.clone(),
        );
    host.loaded_coordination_revision.store(
        workspace.coordination_revision().unwrap(),
        std::sync::atomic::Ordering::Relaxed,
    );

    let prism = host.current_prism();
    assert!(
        prism.coordination_plan(&plan_id).is_none(),
        "continuity runtime should be stale for this regression setup"
    );
    assert!(
        prism.plan_graph(&plan_id).is_some(),
        "plan runtime should still have the published plan graph"
    );

    let result = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanUpdate,
                payload: json!({
                    "planId": plan_id.0,
                    "status": "abandoned",
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(!result.rejected);
    assert_eq!(result.state["status"], "Abandoned");
}

#[test]
fn drift_candidates_and_task_intent_flow_through_prism_query_reads() {
    let spec = NodeId::new("demo", "docs::request_spec", NodeKind::Document);
    let implementation = NodeId::new("demo", "demo::handle_request", NodeKind::Function);
    let related = NodeId::new("demo", "demo::audit_request", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: spec.clone(),
        name: "request_spec".into(),
        kind: NodeKind::Document,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: implementation.clone(),
        name: "handle_request".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: related.clone(),
        name: "audit_request".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(3),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Specifies,
        source: spec.clone(),
        target: implementation.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::RelatedTo,
        source: spec.clone(),
        target: related.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let host = host_with_prism(Prism::new(graph));
    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Coordinate request handling" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Implement request flow",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::handle_request",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "dispatch drift candidates",
        ),
    );
    let drift_value = execution.dispatch("driftCandidates", r#"{}"#).unwrap();
    let intent_value = execution
        .dispatch("taskIntent", &format!(r#"{{ "taskId": "{task_id}" }}"#))
        .unwrap();

    assert_eq!(drift_value.as_array().unwrap().len(), 1);
    assert_eq!(drift_value[0]["spec"]["path"], "docs::request_spec");
    assert_eq!(drift_value[0]["reasons"][0], "no validation links");

    assert_eq!(intent_value["taskId"], task_id);
    assert_eq!(intent_value["specs"][0]["path"], "docs::request_spec");
    assert_eq!(
        intent_value["implementations"][0]["path"],
        "demo::handle_request"
    );
    assert_eq!(intent_value["related"][0]["path"], "demo::audit_request");
    assert_eq!(intent_value["driftCandidates"].as_array().unwrap().len(), 1);
}

#[test]
fn curator_reads_flow_through_prism_query_and_edge_promotion_is_explicit() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::InferredEdge(CandidateEdge {
                    edge: Edge {
                        kind: EdgeKind::Calls,
                        source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                        target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                        origin: prism_ir::EdgeOrigin::Inferred,
                        confidence: 0.82,
                    },
                    scope: InferredEdgeScope::SessionOnly,
                    evidence: vec!["observed repeated edits".into()],
                    rationale: "alpha usually routes to beta after validation".into(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validated"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let jobs = host
        .execute(
            test_session(&host),
            r#"
return prism.curator.jobs({ status: "completed", limit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(jobs.result.as_array().map(|items| items.len()), Some(1));
    assert_eq!(jobs.result[0]["id"], job_id);
    assert_eq!(jobs.result[0]["proposals"][0]["kind"], "inferred_edge");
    assert_eq!(jobs.result[0]["proposals"][0]["disposition"], "pending");

    let promoted = host
        .promote_curator_edge(
            test_session(&host).as_ref(),
            PrismCuratorPromoteEdgeArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                scope: Some(InferredEdgeScopeInput::Persisted),
                note: Some("accepted after review".into()),
                task_id: Some("task:promotion".into()),
            },
        )
        .unwrap();
    assert!(promoted.edge_id.is_some());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(proposal.result["disposition"], "applied");
    assert_eq!(proposal.result["output"], promoted.edge_id.unwrap());
}

#[test]
fn curator_memory_promotion_persists_and_recall_sees_promoted_entry() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::StructuralMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Structural,
                    content: "alpha owns request routing".to_string(),
                    trust: 0.82,
                    rationale: "Repeated successful fixes anchored on alpha".to_string(),
                    category: Some("ownership_rule".to_string()),
                    evidence: CandidateMemoryEvidence::default(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:routing-note"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_memory(
            test_session(&host).as_ref(),
            PrismCuratorPromoteMemoryArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                trust: None,
                note: Some("promote repeated routing knowledge".into()),
                task_id: Some("task:curator-memory".into()),
            },
        )
        .expect("memory promotion should succeed");
    assert!(promoted.memory_id.is_some());
    assert!(promoted.edge_id.is_none());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  memory: prism.memory.recall({{
    focus: sym ? [sym] : [],
    text: "routing",
    limit: 5,
  }}),
  structuralOnly: prism.memory.recall({{
    focus: sym ? [sym] : [],
    kinds: ["structural"],
    limit: 5,
  }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(
        proposal.result["proposal"]["output"],
        promoted.memory_id.unwrap()
    );
    assert_eq!(proposal.result["memory"][0]["entry"]["kind"], "Structural");
    assert_eq!(
        proposal.result["memory"][0]["entry"]["content"],
        "alpha owns request routing"
    );
    assert_eq!(
        proposal.result["structuralOnly"][0]["entry"]["kind"],
        "Structural"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["provenance"]["origin"],
        "curator"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["rationale"],
        "Repeated successful fixes anchored on alpha"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["structuralRule"]["kind"],
        "ownership_rule"
    );
    assert_eq!(
        proposal.result["memory"][0]["entry"]["metadata"]["structuralRule"]["promoted"],
        true
    );
}

#[test]
fn curator_concept_promotion_persists_session_concept_and_marks_proposal_applied() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha_route() {}
pub fn beta_route() {}
"#,
    )
    .unwrap();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::ConceptCandidate(CandidateConcept {
                    recommended_operation: CandidateConceptOperation::Promote,
                    canonical_name: "route_cluster".to_string(),
                    summary: "Routing hotspot cluster.".to_string(),
                    aliases: vec!["routing".to_string()],
                    core_members: vec![
                        NodeId::new("demo", "demo::alpha_route", NodeKind::Function),
                        NodeId::new("demo", "demo::beta_route", NodeKind::Function),
                    ],
                    supporting_members: Vec::new(),
                    likely_tests: Vec::new(),
                    evidence: vec!["Hotspot edit kept touching the same routing pair.".to_string()],
                    confidence: 0.79,
                    rationale: "Repeated co-change and hotspot edits justify a reusable concept."
                        .to_string(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha_route")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:route-fix"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-concept")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_concept(
            test_session(&host).as_ref(),
            PrismCuratorPromoteConceptArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                scope: None,
                note: Some("accept hotspot concept".into()),
                task_id: Some("task:curator-concept".into()),
            },
        )
        .expect("concept promotion should succeed");
    assert_eq!(promoted.memory_id, None);
    assert_eq!(promoted.edge_id, None);
    assert_eq!(
        promoted.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  concept: prism.conceptByHandle("concept://route_cluster", {{ includeBindingMetadata: true }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(proposal.result["proposal"]["kind"], "concept_candidate");
    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(
        proposal.result["proposal"]["output"],
        Value::String("concept://route_cluster".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["canonicalName"],
        Value::String("route_cluster".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["provenance"]["origin"],
        Value::String("curator".to_string())
    );
    assert_eq!(
        proposal.result["concept"]["provenance"]["kind"],
        Value::String("curator_concept_candidate".to_string())
    );
}

#[test]
fn curator_apply_proposal_promotes_repo_scoped_concept_with_override() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha_route() {}
pub fn beta_route() {}
"#,
    )
    .unwrap();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::ConceptCandidate(CandidateConcept {
                    recommended_operation: CandidateConceptOperation::Promote,
                    canonical_name: "route_cluster".to_string(),
                    summary: "Routing hotspot cluster.".to_string(),
                    aliases: vec!["routing".to_string()],
                    core_members: vec![
                        NodeId::new("demo", "demo::alpha_route", NodeKind::Function),
                        NodeId::new("demo", "demo::beta_route", NodeKind::Function),
                    ],
                    supporting_members: Vec::new(),
                    likely_tests: Vec::new(),
                    evidence: vec!["Hotspot edit kept touching the same routing pair.".to_string()],
                    confidence: 0.79,
                    rationale: "Repeated co-change and hotspot edits justify a reusable concept."
                        .to_string(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha_route")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:route-apply"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:curator-apply-concept")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let applied = host
        .apply_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorApplyProposalArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                note: Some("accept hotspot concept as published knowledge".into()),
                options: Some(PrismCuratorApplyProposalOptionsArgs {
                    edge_scope: None,
                    concept_scope: Some(ConceptScopeInput::Repo),
                    memory_trust: None,
                }),
                task_id: Some("task:curator-apply-concept".into()),
            },
        )
        .expect("generic curator apply should promote concept");
    assert_eq!(applied.kind, "concept_candidate");
    assert_eq!(applied.decision, CuratorProposalDecision::Applied);
    assert_eq!(
        applied.created.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );
    assert_eq!(
        applied.concept_handle.as_deref(),
        Some("concept://route_cluster")
    );

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return {{
  proposal: prism.curator.job("{job_id}")?.proposals[0],
  concept: prism.conceptByHandle("concept://route_cluster", {{ includeBindingMetadata: true }}),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(proposal.result["proposal"]["disposition"], "applied");
    assert_eq!(proposal.result["concept"]["scope"], "repo");
    assert_eq!(
        proposal.result["concept"]["publication"]["status"],
        "active"
    );
}

#[test]
fn curator_apply_proposal_routes_risk_summary_through_memory_promotion() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    summary: "alpha is a risky coordination hotspot".to_string(),
                    severity: "high".to_string(),
                    evidence_events: Vec::new(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic-fix"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-generic-repeat"),
                ts: 52,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk-generic")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed again under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let applied = host
        .apply_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorApplyProposalArgs {
                job_id,
                proposal_index: 0,
                note: Some("promote risk summary after review".into()),
                options: Some(PrismCuratorApplyProposalOptionsArgs {
                    edge_scope: None,
                    concept_scope: None,
                    memory_trust: Some(0.93),
                }),
                task_id: Some("task:alpha-risk-generic".into()),
            },
        )
        .expect("generic curator apply should route risk summary to memory promotion");
    assert_eq!(applied.kind, "risk_summary");
    assert_eq!(applied.decision, CuratorProposalDecision::Applied);
    assert_eq!(applied.edge_id, None);
    assert!(applied.memory_id.is_some());
    assert_eq!(applied.created.memory_id, applied.memory_id);

    let stored = test_session(&host)
        .notes
        .entry(&MemoryId(applied.memory_id.clone().unwrap()))
        .expect("promoted memory should exist");
    assert_eq!(stored.trust, 0.93);
    assert_eq!(stored.content, "alpha is a risky coordination hotspot");
}

#[test]
fn concept_relation_mutation_populates_query_and_compact_neighbor_views() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.9),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-relation".to_string()),
        },
    )
    .unwrap();
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://runtime_surface".to_string()),
            canonical_name: Some("runtime_surface".to_string()),
            summary: Some("Runtime status and entry points.".to_string()),
            aliases: Some(vec!["runtime".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::start_task".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.88),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-relation".to_string()),
        },
    )
    .unwrap();

    let stored = host
        .store_concept_relation(
            session.as_ref(),
            PrismConceptRelationMutationArgs {
                operation: ConceptRelationMutationOperationInput::Upsert,
                source_handle: "concept://custom_validation".to_string(),
                target_handle: "concept://runtime_surface".to_string(),
                kind: ConceptRelationKindInput::OftenUsedWith,
                confidence: Some(0.82),
                evidence: Some(vec![
                    "Validation work usually routes through runtime status.".to_string(),
                ]),
                scope: Some(ConceptScopeInput::Session),
                task_id: Some("task:concept-relation".to_string()),
            },
        )
        .expect("concept relation should store");
    assert_eq!(stored.relation.related_handle, "concept://runtime_surface");

    let query = host
        .execute(
            session,
            r#"
return {
  concept: prism.conceptByHandle("concept://custom_validation", { includeBindingMetadata: true }),
  relations: prism.conceptRelations("concept://custom_validation"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(
        query.result["concept"]["relations"][0]["relatedHandle"],
        Value::String("concept://runtime_surface".to_string())
    );
    assert_eq!(
        query.result["relations"][0]["kind"],
        Value::String("often_used_with".to_string())
    );

    let neighbors = host
        .compact_expand(
            test_session(&host),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: None,
            },
        )
        .expect("neighbor expand should accept concept handles");
    assert_eq!(neighbors.kind, prism_js::AgentExpandKind::Neighbors);
    assert_eq!(
        neighbors.result["relations"][0]["relatedHandle"],
        Value::String("concept://runtime_surface".to_string())
    );
}

#[test]
fn semantic_curator_memory_promotion_persists_and_is_recallable() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::SemanticMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Semantic,
                    content: "Recent outcome context: alpha failed under routing load; validated alpha routing follow-up".to_string(),
                    trust: 0.74,
                    rationale: "Repeated outcomes around alpha form reusable fuzzy context.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: CandidateMemoryEvidence {
                        event_ids: vec![EventId::new("outcome:alpha-risk")],
                        memory_ids: vec![MemoryId("memory:episodic-alpha".to_string())],
                        validation_checks: vec!["test:alpha_regression".to_string()],
                        co_change_lineages: Vec::new(),
                    },
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:semantic-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-fix"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:semantic-memory")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let promoted = host
        .promote_curator_memory(
            test_session(&host).as_ref(),
            PrismCuratorPromoteMemoryArgs {
                job_id,
                proposal_index: 0,
                trust: None,
                note: Some("promote semantic context".into()),
                task_id: Some("task:semantic-memory".into()),
            },
        )
        .expect("semantic memory promotion should succeed");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  kinds: ["semantic"],
  text: "routing load",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("semantic memory recall should succeed");

    assert_eq!(result.result[0]["entry"]["kind"], "Semantic");
    assert_eq!(
        result.result[0]["entry"]["metadata"]["category"],
        "risk_summary"
    );
    assert_eq!(
        result.result[0]["entry"]["metadata"]["evidence"]["validationChecks"][0],
        "test:alpha_regression"
    );
    assert_eq!(
        result.result[0]["entry"]["metadata"]["evidence"]["memoryIds"][0],
        "memory:episodic-alpha"
    );

    let events = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.memory.events({{
  memoryId: "{}",
  limit: 5,
}});
"#,
                promoted.memory_id.clone().unwrap()
            ),
            QueryLanguage::Ts,
        )
        .expect("memory event query should succeed");
    assert_eq!(events.result[0]["promotedFrom"][0], "memory:episodic-alpha");
}

#[test]
fn contract_mutation_and_contracts_resource_surface_packets() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let stored = host
        .store_contract(
            session.as_ref(),
            PrismContractMutationArgs {
                operation: ContractMutationOperationInput::Promote,
                handle: Some("contract://runtime_status_surface".to_string()),
                name: Some("runtime status surface".to_string()),
                summary: Some(
                    "The runtime status entry point remains available for internal diagnostics consumers."
                        .to_string(),
                ),
                aliases: Some(vec!["runtime status".to_string()]),
                kind: Some(ContractKindInput::Interface),
                subject: Some(ContractTargetInput {
                    anchors: Some(vec![AnchorRefInput::Node {
                        crate_name: "demo".to_string(),
                        path: "demo::main".to_string(),
                        kind: "function".to_string(),
                    }]),
                    concept_handles: Some(vec!["concept://runtime_surface".to_string()]),
                }),
                guarantees: Some(vec![ContractGuaranteeInput {
                    id: None,
                    statement:
                        "Internal diagnostics callers can query runtime status without reconstructing daemon state."
                            .to_string(),
                    scope: Some("internal".to_string()),
                    strength: Some(ContractGuaranteeStrengthInput::Hard),
                    evidence_refs: Some(vec!["runtime-status-tests".to_string()]),
                }]),
                assumptions: Some(vec!["The daemon is running.".to_string()]),
                consumers: Some(vec![ContractTargetInput {
                    anchors: None,
                    concept_handles: Some(vec!["concept://runtime_surface".to_string()]),
                }]),
                validations: Some(vec![ContractValidationInput {
                    id: "cargo test -p prism-mcp runtime_status".to_string(),
                    summary: Some("Covers the runtime status surface.".to_string()),
                    anchors: Some(vec![AnchorRefInput::Node {
                        crate_name: "demo".to_string(),
                        path: "demo::main".to_string(),
                        kind: "function".to_string(),
                    }]),
                }]),
                stability: Some(ContractStabilityInput::Internal),
                compatibility: Some(ContractCompatibilityInput {
                    compatible: Some(vec!["Internal implementation changes behind the same surface.".to_string()]),
                    additive: None,
                    risky: None,
                    breaking: Some(vec!["Removing the runtime status surface.".to_string()]),
                    migrating: None,
                }),
                evidence: Some(vec!["Promoted from repeated runtime-inspection work.".to_string()]),
                status: Some(ContractStatusInput::Active),
                scope: Some(ConceptScopeInput::Session),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:contract-surface".to_string()),
            },
        )
        .expect("contract should store");

    assert_eq!(stored.packet.kind, ContractKindView::Interface);
    assert_eq!(stored.packet.status, ContractStatusView::Active);
    assert_eq!(stored.packet.stability, ContractStabilityView::Internal);
    assert_eq!(
        stored.packet.subject.concept_handles,
        vec!["concept://runtime_surface"]
    );
    assert_eq!(
        host.current_prism()
            .contract_by_handle("contract://runtime_status_surface")
            .expect("contract should exist")
            .kind,
        ContractKind::Interface
    );
    assert_eq!(
        host.current_prism()
            .contract_by_handle("contract://runtime_status_surface")
            .expect("contract should exist")
            .status,
        ContractStatus::Active
    );
    assert_eq!(
        host.current_prism()
            .contract_by_handle("contract://runtime_status_surface")
            .expect("contract should exist")
            .stability,
        ContractStability::Internal
    );

    let payload = host
        .contracts_resource_value(
            session,
            "prism://contracts?contains=runtime&status=active&scope=session&kind=interface",
        )
        .expect("contracts resource should load");
    assert_eq!(payload.contracts.len(), 1);
    assert_eq!(
        payload.contracts[0].handle,
        "contract://runtime_status_surface"
    );
    assert_eq!(payload.contracts[0].guarantees[0].statement, "Internal diagnostics callers can query runtime status without reconstructing daemon state.");
    assert_eq!(payload.contracts[0].subject.anchors.len(), 1);
    assert_eq!(payload.kind.as_deref(), Some("interface"));
    assert!(payload.related_resources.iter().any(|resource| resource.uri
        == "prism://contracts?contains=runtime&status=active&scope=session&kind=interface"));
}

#[test]
fn contract_views_surface_file_anchor_paths_for_round_trip_safety() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let stored = host
        .store_contract(
            session.as_ref(),
            PrismContractMutationArgs {
                operation: ContractMutationOperationInput::Promote,
                handle: Some("contract://source_file_surface".to_string()),
                name: Some("source file surface".to_string()),
                summary: Some("The source file contract should round-trip through MCP without internal id guesswork.".to_string()),
                aliases: None,
                kind: Some(ContractKindInput::Behavioral),
                subject: Some(ContractTargetInput {
                    anchors: Some(vec![AnchorRefInput::File {
                        file_id: None,
                        path: Some("src/lib.rs".to_string()),
                    }]),
                    concept_handles: None,
                }),
                guarantees: Some(vec![ContractGuaranteeInput {
                    id: None,
                    statement: "Consumers can reason about the anchored file directly from the contract payload.".to_string(),
                    scope: None,
                    strength: Some(ContractGuaranteeStrengthInput::Hard),
                    evidence_refs: None,
                }]),
                assumptions: None,
                consumers: None,
                validations: Some(vec![ContractValidationInput {
                    id: "cargo test -p prism-mcp contract_views_surface_file_anchor_paths_for_round_trip_safety".to_string(),
                    summary: Some("Covers file-anchor output ergonomics.".to_string()),
                    anchors: Some(vec![AnchorRefInput::File {
                        file_id: None,
                        path: Some("src/lib.rs".to_string()),
                    }]),
                }]),
                stability: Some(ContractStabilityInput::Internal),
                compatibility: None,
                evidence: None,
                status: Some(ContractStatusInput::Active),
                scope: Some(ConceptScopeInput::Session),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:contract-file-anchor-view".to_string()),
            },
        )
        .expect("contract should store");

    let expected_path = "src/lib.rs";
    match &stored.packet.subject.anchors[0] {
        AnchorRefView::File { file_id, path } => {
            assert_eq!(path.as_deref(), Some(expected_path));
            assert!(file_id.is_some());
        }
        other => panic!("expected file anchor view, got {other:?}"),
    }
    match &stored.packet.validations[0].anchors[0] {
        AnchorRefView::File { file_id, path } => {
            assert_eq!(path.as_deref(), Some(expected_path));
            assert!(file_id.is_some());
        }
        other => panic!("expected file anchor view, got {other:?}"),
    }
    assert_eq!(
        stored.packet.guarantees[0].id,
        "consumers_can_reason_about_the_anchored_file_directly_from_the_contract_payload"
    );
    assert_eq!(
        stored
            .packet
            .health
            .as_ref()
            .and_then(|health| Some(&health.status)),
        Some(&prism_js::ContractHealthStatusView::Degraded)
    );
    assert!(stored
        .packet
        .health
        .as_ref()
        .and_then(|health| health.next_action.as_deref())
        .is_some_and(|value| value.contains("evidenceRefs")));
    assert!(stored
        .packet
        .health
        .as_ref()
        .and_then(|health| health.next_action.as_deref())
        .is_some_and(|value| value.contains(
            "consumers_can_reason_about_the_anchored_file_directly_from_the_contract_payload"
        )));

    let result = host
        .execute(
            session.clone(),
            r#"
return prism.contract("contract://source_file_surface")?.subject.anchors[0] ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result["type"], "file");
    assert_eq!(result.result["path"], expected_path);
    assert!(result.result["fileId"].as_u64().is_some());
}

#[test]
fn contract_query_helpers_and_read_context_surface_contract_packets() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn runtime_status() {}
pub fn inspect_runtime() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://runtime_status_surface".to_string()),
            name: Some("runtime status surface".to_string()),
            summary: Some(
                "The runtime status entry point remains available for diagnostics consumers."
                    .to_string(),
            ),
            aliases: Some(vec!["runtime status".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: None,
                statement: "Diagnostics callers can query runtime status.".to_string(),
                scope: Some("internal".to_string()),
                strength: Some(ContractGuaranteeStrengthInput::Hard),
                evidence_refs: Some(vec!["runtime-status-tests".to_string()]),
            }]),
            assumptions: Some(vec!["The daemon is running.".to_string()]),
            consumers: Some(vec![ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::inspect_runtime".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }]),
            validations: Some(vec![ContractValidationInput {
                id: "cargo test -p prism-mcp runtime_status".to_string(),
                summary: Some("Covers the runtime status surface.".to_string()),
                anchors: None,
            }]),
            stability: Some(ContractStabilityInput::Internal),
            compatibility: Some(ContractCompatibilityInput {
                compatible: None,
                additive: None,
                risky: None,
                breaking: Some(vec!["Removing the runtime status surface.".to_string()]),
                migrating: None,
            }),
            evidence: Some(vec![
                "Promoted from repeated runtime inspection work.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:contract-query".to_string()),
        },
    )
    .expect("contract should store");

    let envelope = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("runtime_status");
return {
  contract: prism.contract("runtime status surface"),
  contracts: prism.contracts({ scope: "session", status: "active", kind: "interface", contains: "runtime", limit: 1 }),
  contractsFor: sym ? prism.contractsFor(sym) : [],
  read: sym ? prism.readContext(sym) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("contract query should succeed");

    assert_eq!(
        envelope.result["contract"]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert_eq!(
        envelope.result["contracts"][0]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert_eq!(
        envelope.result["contractsFor"][0]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert_eq!(
        envelope.result["read"]["contracts"][0]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert!(envelope.result["read"]["suggestedQueries"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|query| query["label"] == Value::String("Contracts".to_string()))));
}

#[test]
fn impact_and_after_edit_surface_contract_guidance() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn runtime_status() {}
pub fn inspect_runtime() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full()
            .with_query_view(QueryViewFeatureFlag::Impact, true)
            .with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );
    let session = test_session(&host);

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://runtime_status_surface".to_string()),
            name: Some("runtime status surface".to_string()),
            summary: Some(
                "The runtime status entry point remains available for diagnostics consumers."
                    .to_string(),
            ),
            aliases: Some(vec!["runtime status".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: None,
                statement: "Diagnostics callers can query runtime status.".to_string(),
                scope: Some("internal".to_string()),
                strength: Some(ContractGuaranteeStrengthInput::Hard),
                evidence_refs: Some(vec!["runtime-status-tests".to_string()]),
            }]),
            assumptions: Some(vec!["The daemon is running.".to_string()]),
            consumers: Some(vec![ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::inspect_runtime".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }]),
            validations: Some(vec![ContractValidationInput {
                id: "cargo test -p prism-mcp runtime_status".to_string(),
                summary: Some("Covers the runtime status surface.".to_string()),
                anchors: None,
            }]),
            stability: Some(ContractStabilityInput::Internal),
            compatibility: Some(ContractCompatibilityInput {
                compatible: None,
                additive: None,
                risky: Some(vec!["Changing the status payload shape.".to_string()]),
                breaking: Some(vec!["Removing the runtime status surface.".to_string()]),
                migrating: Some(vec![
                    "Update diagnostics callers before widening the payload.".to_string(),
                ]),
            }),
            evidence: Some(vec![
                "Promoted from repeated runtime inspection work.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:contract-views".to_string()),
        },
    )
    .expect("contract should store");

    let envelope = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("runtime_status");
return {
  impact: sym ? prism.impact({ target: sym }) : null,
  afterEdit: sym ? prism.afterEdit({ target: sym }) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query views should succeed");

    assert_eq!(
        envelope.result["impact"]["contracts"][0]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert!(envelope.result["impact"]["downstream"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item["label"] == Value::String("demo::inspect_runtime".to_string()))));
    assert!(envelope.result["impact"]["recommendedChecks"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["label"]
            == Value::String("cargo test -p prism-mcp runtime_status".to_string()))));

    assert_eq!(
        envelope.result["afterEdit"]["contracts"][0]["handle"],
        Value::String("contract://runtime_status_surface".to_string())
    );
    assert!(envelope.result["afterEdit"]["nextReads"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item["label"] == Value::String("demo::inspect_runtime".to_string()))));
    assert!(envelope.result["afterEdit"]["tests"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["label"]
            == Value::String("cargo test -p prism-mcp runtime_status".to_string()))));
    assert!(envelope.result["afterEdit"]["notes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|value| value.contains("contract://runtime_status_surface")))));
}

#[test]
fn after_edit_path_set_collapses_duplicate_contract_notes() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn runtime_status() {}
pub fn inspect_runtime() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full().with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );
    let session = test_session(&host);

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://runtime_status_surface".to_string()),
            name: Some("runtime status surface".to_string()),
            summary: Some(
                "The runtime status entry point remains available for diagnostics consumers."
                    .to_string(),
            ),
            aliases: Some(vec!["runtime status".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: None,
                statement: "Diagnostics callers can query runtime status.".to_string(),
                scope: Some("internal".to_string()),
                strength: Some(ContractGuaranteeStrengthInput::Hard),
                evidence_refs: Some(vec!["runtime-status-tests".to_string()]),
            }]),
            assumptions: Some(vec!["The daemon is running.".to_string()]),
            consumers: Some(vec![ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::inspect_runtime".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }]),
            validations: Some(vec![ContractValidationInput {
                id: "cargo test -p prism-mcp runtime_status".to_string(),
                summary: Some("Covers the runtime status surface.".to_string()),
                anchors: None,
            }]),
            stability: Some(ContractStabilityInput::Internal),
            compatibility: Some(ContractCompatibilityInput {
                compatible: None,
                additive: None,
                risky: Some(vec!["Changing the status payload shape.".to_string()]),
                breaking: Some(vec!["Removing the runtime status surface.".to_string()]),
                migrating: Some(vec![
                    "Update diagnostics callers before widening the payload.".to_string(),
                ]),
            }),
            evidence: Some(vec![
                "Promoted from repeated runtime inspection work.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:contract-views".to_string()),
        },
    )
    .expect("contract should store");

    let after_edit = host
        .execute(
            test_session(&host),
            r#"return prism.afterEdit({ paths: ["src/lib.rs"] });"#,
            QueryLanguage::Ts,
        )
        .expect("afterEdit path-set should succeed");

    let matching_notes = after_edit.result["notes"]
        .as_array()
        .expect("notes should be array")
        .iter()
        .filter(|note| {
            note.as_str()
                .is_some_and(|value| value.contains("contract://runtime_status_surface"))
        })
        .count();
    assert_eq!(matching_notes, 1);
}

#[test]
fn validation_plan_surfaces_contract_validations_and_related_targets() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn runtime_status() {}
pub fn inspect_runtime() {}
pub fn runtime_status_contract_test() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full().with_query_view(QueryViewFeatureFlag::ValidationPlan, true),
    );
    let session = test_session(&host);

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://runtime_status_surface".to_string()),
            name: Some("runtime status surface".to_string()),
            summary: Some(
                "The runtime status entry point remains available for diagnostics consumers."
                    .to_string(),
            ),
            aliases: Some(vec!["runtime status".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: None,
                statement: "Diagnostics callers can query the runtime status entry point."
                    .to_string(),
                scope: None,
                strength: None,
                evidence_refs: None,
            }]),
            assumptions: None,
            consumers: Some(vec![ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::inspect_runtime".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }]),
            validations: Some(vec![ContractValidationInput {
                id: "cargo test -p prism-mcp runtime_status_contract".to_string(),
                summary: Some("Covers the runtime status contract.".to_string()),
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status_contract_test".to_string(),
                    kind: "function".to_string(),
                }]),
            }]),
            stability: Some(ContractStabilityInput::Internal),
            compatibility: None,
            evidence: Some(vec![
                "Promoted from repeated runtime inspection work.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:contract-validation-plan".to_string()),
        },
    )
    .expect("contract should store");

    let envelope = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("runtime_status");
return sym ? prism.validationPlan({ target: sym }) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("validationPlan should succeed");

    assert!(envelope.result["fast"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["label"]
                == Value::String("cargo test -p prism-mcp runtime_status_contract".to_string())
        })));
    assert!(envelope.result["relatedTargets"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| { item["path"] == Value::String("demo::inspect_runtime".to_string()) })));
    assert!(envelope.result["notes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("Contracts contributed")))));
}

#[test]
fn validation_plan_accepts_native_plan_node_task_ids() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Validate native milestone",
                    "policy": { "requireValidationForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let required_test =
        "test:cargo test -p prism-js api_reference_mentions_primary_tool -- --nocapture";
    let required_build = "build:cargo build --release -p prism-cli -p prism-mcp";
    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "kind": "validate",
                    "title": "Validate native milestone",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "acceptance": [{
                        "label": "native milestone is validated",
                        "requiredChecks": [
                            { "id": required_test },
                            { "id": required_build }
                        ],
                        "evidencePolicy": "validation-only"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let envelope = host
        .execute(
            test_session(&host),
            &format!(r#"return prism.validationPlan({{ taskId: "{node_id}" }});"#),
            QueryLanguage::Ts,
        )
        .expect("validationPlan should accept native plan node ids");

    assert_eq!(envelope.result["subject"]["taskId"], Value::String(node_id));
    assert!(envelope.result["fast"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            item["label"] == Value::String(required_test.to_string())
                || item["label"] == Value::String(required_build.to_string())
        })));
    assert!(envelope.result["notes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("native plan node requirements")))));
}

#[test]
fn task_surfaces_accept_native_plan_node_task_ids() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Route native task-shaped queries",
                    "policy": {
                        "requireValidationForCompletion": true,
                        "reviewRequiredAboveRiskScore": 0.0
                    }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let required_test = "test:cargo test -p prism-mcp native_task_surfaces_accept_plan_nodes";
    let required_build = "build:cargo build --release -p prism-cli -p prism-mcp";
    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "kind": "validate",
                    "title": "Validate native task-shaped queries",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "validationRefs": [{ "id": required_build }],
                    "acceptance": [{
                        "label": "native task-shaped queries are validated",
                        "requiredChecks": [{ "id": required_test }],
                        "evidencePolicy": "validation-only"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    let envelope = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return {{
  blastRadius: prism.taskBlastRadius("{node_id}"),
  taskRecipe: prism.taskValidationRecipe("{node_id}"),
  taskRisk: prism.taskRisk("{node_id}")
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("task-shaped query runtime should accept native plan node ids");

    assert!(envelope.result["blastRadius"]["likelyValidations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_test)));
    assert!(envelope.result["blastRadius"]["likelyValidations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_build)));
    assert_eq!(
        envelope.result["taskRecipe"]["taskId"],
        Value::String(node_id.clone())
    );
    assert!(envelope.result["taskRecipe"]["checks"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_test)));
    assert!(envelope.result["taskRecipe"]["checks"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_build)));
    assert_eq!(
        envelope.result["taskRisk"]["taskId"],
        Value::String(node_id)
    );
    assert!(envelope.result["taskRisk"]["likelyValidations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_test)));
    assert!(envelope.result["taskRisk"]["missingValidations"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == required_build)));
    assert_eq!(
        envelope.result["taskRisk"]["reviewRequired"],
        Value::Bool(true)
    );
}

#[test]
fn compact_workset_prioritizes_contract_consumers_and_validation_targets() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn runtime_status() {}
pub fn inspect_runtime() {}
pub fn runtime_status_contract_test() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://runtime_status_surface".to_string()),
            name: Some("runtime status surface".to_string()),
            summary: Some(
                "The runtime status entry point remains available for diagnostics consumers."
                    .to_string(),
            ),
            aliases: Some(vec!["runtime status".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: None,
                statement: "Diagnostics callers can query the runtime status entry point."
                    .to_string(),
                scope: None,
                strength: None,
                evidence_refs: None,
            }]),
            assumptions: None,
            consumers: Some(vec![ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::inspect_runtime".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }]),
            validations: Some(vec![ContractValidationInput {
                id: "cargo test -p prism-mcp runtime_status_contract".to_string(),
                summary: Some("Covers the runtime status contract.".to_string()),
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status_contract_test".to_string(),
                    kind: "function".to_string(),
                }]),
            }]),
            stability: Some(ContractStabilityInput::Internal),
            compatibility: None,
            evidence: Some(vec![
                "Promoted from repeated runtime inspection work.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:contract-workset".to_string()),
        },
    )
    .expect("contract should store");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: None,
                query: Some("runtime_status".to_string()),
            },
        )
        .expect("workset should succeed");

    assert_eq!(workset.supporting_reads[0].path, "demo::inspect_runtime");
    assert!(
        workset
            .likely_tests
            .iter()
            .chain(workset.supporting_reads.iter())
            .any(|target| target.path == "demo::runtime_status_contract_test")
            || workset
                .why
                .contains("cargo test -p prism-mcp runtime_status_contract")
            || workset.why.contains("anchors 1 validation target")
    );
}

#[test]
fn curator_proposals_query_flattens_pending_proposals_across_jobs() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::SemanticMemory(CandidateMemory {
                    anchors: vec![AnchorRef::Node(NodeId::new(
                        "demo",
                        "demo::alpha",
                        NodeKind::Function,
                    ))],
                    kind: MemoryKind::Semantic,
                    content: "Recent outcome context: alpha follow-up stayed green".to_string(),
                    trust: 0.72,
                    rationale: "Recent validated changes are reusable context.".to_string(),
                    category: Some("risk_summary".to_string()),
                    evidence: CandidateMemoryEvidence::default(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validated"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha change".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let proposals = host
        .execute(
            test_session(&host),
            r#"
return prism.curator.proposals({
  status: "completed",
  disposition: "pending",
  kind: "semantic_memory",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .unwrap();

    assert_eq!(
        proposals.result.as_array().map(|items| items.len()),
        Some(1)
    );
    assert_eq!(proposals.result[0]["jobId"], job_id);
    assert_eq!(proposals.result[0]["jobStatus"], "completed");
    assert_eq!(proposals.result[0]["kind"], "semantic_memory");
    assert_eq!(proposals.result[0]["disposition"], "pending");
}

#[test]
fn promoted_curator_knowledge_feeds_validation_and_risk_queries() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![
                    CuratorProposal::ValidationRecipe(CandidateValidationRecipe {
                        target: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                        checks: vec!["test:alpha_regression".to_string()],
                        rationale: "Repeated alpha regressions need a dedicated test".to_string(),
                        evidence: vec!["failure clusters around alpha routing".to_string()],
                    }),
                    CuratorProposal::RiskSummary(CandidateRiskSummary {
                        anchors: vec![AnchorRef::Node(NodeId::new(
                            "demo",
                            "demo::alpha",
                            NodeKind::Function,
                        ))],
                        summary: "alpha is a risky coordination hotspot".to_string(),
                        severity: "high".to_string(),
                        evidence_events: Vec::new(),
                    }),
                ],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-curator"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha routing follow-up".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha-risk-repeat"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha-risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed again under routing load".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Change alpha safely" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();
    let artifact = host
        .store_artifact(
            test_session(&host).as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task_id,
                    "diffRef": "patch:alpha-risk"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    let before = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  recipe: sym ? prism.validationRecipe(sym) : null,
  taskRecipe: prism.taskValidationRecipe("{task_id}"),
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("baseline query should succeed");

    assert!(!before.result["recipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert_eq!(
        before.result["taskRisk"]["promotedSummaries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        before.result["artifactRisk"]["promotedSummaries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    host.promote_curator_memory(
        test_session(&host).as_ref(),
        PrismCuratorPromoteMemoryArgs {
            job_id: job_id.clone(),
            proposal_index: 0,
            trust: None,
            note: Some("promote validation recipe".into()),
            task_id: Some(task_id.clone()),
        },
    )
    .expect("validation recipe promotion should succeed");
    host.promote_curator_memory(
        test_session(&host).as_ref(),
        PrismCuratorPromoteMemoryArgs {
            job_id,
            proposal_index: 1,
            trust: None,
            note: Some("promote risk summary".into()),
            task_id: Some(task_id.clone()),
        },
    )
    .expect("risk summary promotion should succeed");

    let after = host
        .execute(
            test_session(&host),
            &format!(
                r#"
const sym = prism.symbol("alpha");
return {{
  recipe: sym ? prism.validationRecipe(sym) : null,
  taskRecipe: prism.taskValidationRecipe("{task_id}"),
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}"),
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("post-promotion query should succeed");

    assert!(after.result["recipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert!(after.result["taskRecipe"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "test:alpha_regression"));
    assert_eq!(
        after.result["taskRisk"]["promotedSummaries"][0],
        "alpha is a risky coordination hotspot"
    );
    assert_eq!(
        after.result["artifactRisk"]["promotedSummaries"][0],
        "alpha is a risky coordination hotspot"
    );
    assert!(
        after.result["taskRisk"]["riskScore"].as_f64().unwrap()
            > before.result["taskRisk"]["riskScore"].as_f64().unwrap()
    );
    assert!(
        after.result["artifactRisk"]["riskScore"].as_f64().unwrap()
            > before.result["artifactRisk"]["riskScore"].as_f64().unwrap()
    );
}

#[test]
fn task_and_artifact_risk_surface_contract_review_guidance() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha() {}
pub fn alpha_consumer_one() {}
pub fn alpha_consumer_two() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full(),
    );
    let session = test_session(&host);

    let plan = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "title": "Contract-aware risk",
                    "goal": "Review contract-sensitive task risk",
                    "policy": {
                        "reviewRequiredAboveRiskScore": 0.2
                    }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();
    let artifact = host
        .store_artifact(
            session.as_ref(),
            PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task_id,
                    "diffRef": "patch:alpha-contract-risk"
                }),
                task_id: None,
            },
        )
        .unwrap();
    let artifact_id = artifact.artifact_id.clone().unwrap();

    host.store_contract(
        session.as_ref(),
        PrismContractMutationArgs {
            operation: ContractMutationOperationInput::Promote,
            handle: Some("contract://alpha_surface".to_string()),
            name: Some("alpha surface".to_string()),
            summary: Some("alpha remains callable for recorded consumers.".to_string()),
            aliases: Some(vec!["alpha contract".to_string()]),
            kind: Some(ContractKindInput::Interface),
            subject: Some(ContractTargetInput {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::alpha".to_string(),
                    kind: "function".to_string(),
                }]),
                concept_handles: None,
            }),
            guarantees: Some(vec![ContractGuaranteeInput {
                id: Some("alpha-callable".to_string()),
                statement: "alpha stays callable for downstream consumers.".to_string(),
                scope: Some("runtime".to_string()),
                strength: Some(ContractGuaranteeStrengthInput::Hard),
                evidence_refs: None,
            }]),
            assumptions: Some(vec!["consumers keep the expected call shape".to_string()]),
            consumers: Some(vec![
                ContractTargetInput {
                    anchors: Some(vec![AnchorRefInput::Node {
                        crate_name: "demo".to_string(),
                        path: "demo::alpha_consumer_one".to_string(),
                        kind: "function".to_string(),
                    }]),
                    concept_handles: None,
                },
                ContractTargetInput {
                    anchors: Some(vec![AnchorRefInput::Node {
                        crate_name: "demo".to_string(),
                        path: "demo::alpha_consumer_two".to_string(),
                        kind: "function".to_string(),
                    }]),
                    concept_handles: None,
                },
            ]),
            validations: None,
            stability: Some(ContractStabilityInput::Internal),
            compatibility: Some(ContractCompatibilityInput {
                compatible: None,
                additive: Some(vec!["Adding optional parameters is additive.".to_string()]),
                risky: Some(vec![
                    "Changing the return payload shape is risky.".to_string()
                ]),
                breaking: Some(vec!["Removing alpha is breaking for consumers.".to_string()]),
                migrating: None,
            }),
            evidence: Some(vec![
                "Captured during contract-aware risk review.".to_string()
            ]),
            status: Some(ContractStatusInput::Active),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some(task_id.clone()),
        },
    )
    .expect("contract should store");

    let envelope = host
        .execute(
            session,
            &format!(
                r#"
return {{
  taskRisk: prism.taskRisk("{task_id}"),
  artifactRisk: prism.artifactRisk("{artifact_id}")
}};
"#
            ),
            QueryLanguage::Ts,
        )
        .expect("contract-aware risk query should succeed");

    assert_eq!(
        envelope.result["taskRisk"]["contracts"][0]["handle"],
        Value::String("contract://alpha_surface".to_string())
    );
    assert_eq!(
        envelope.result["artifactRisk"]["contracts"][0]["handle"],
        Value::String("contract://alpha_surface".to_string())
    );
    assert!(envelope.result["taskRisk"]["contractReviewNotes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("review compatibility guidance")))));
    assert!(envelope.result["taskRisk"]["contractReviewNotes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("2 recorded consumers")))));
    assert!(envelope.result["artifactRisk"]["contractReviewNotes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|note| note
            .as_str()
            .is_some_and(|text| text.contains("health is stale")))));
}

#[test]
fn curator_rejection_is_a_distinct_mutation() {
    let root = temp_workspace();

    #[derive(Default)]
    struct FakeCurator;

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: Vec::new(),
                    summary: "alpha looks risky".into(),
                    severity: "medium".into(),
                    evidence_events: Vec::new(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:failure"),
                ts: 51,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "alpha follow-up validated".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let job_id = wait_for_completed_curator_job(&session);
    let host = QueryHost::with_session(session);

    let rejected = host
        .reject_curator_proposal(
            test_session(&host).as_ref(),
            PrismCuratorRejectProposalArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                reason: Some("not enough evidence".into()),
                task_id: Some("task:review".into()),
            },
        )
        .unwrap();
    assert!(rejected.edge_id.is_none());

    let proposal = host
        .execute(
            test_session(&host),
            &format!(
                r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
            ),
            QueryLanguage::Ts,
        )
        .unwrap();
    assert_eq!(proposal.result["disposition"], "rejected");
    assert_eq!(proposal.result["note"], "not enough evidence");
}

#[test]
fn prism_tool_queries_surface_schema_actions_and_examples() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(test_session(&host),
            r#"
const tools = prism.tools();
const mutate = prism.tool("prism_mutate");
const validationFeedback = mutate?.actions.find((action) => action.action === "validation_feedback");
const coordination = mutate?.actions.find((action) => action.action === "coordination");
const validation = prism.validateToolInput("prism_mutate", {
  action: "coordination",
  kind: "task_create",
  payload: { title: "Missing plan id" },
});
const missing = prism.tool("bogus_tool");
return {
  toolNames: tools.map((tool) => tool.toolName),
  mutateSummary: mutate ? {
    toolName: mutate.toolName,
    actionCount: mutate.actions.length,
    exampleAction: mutate.exampleInput?.action,
    examplePrismSaid: mutate.exampleInput?.input?.prismSaid,
  } : null,
  validationFeedback,
  coordination,
  validation,
  missing,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("tool schema query should succeed");

    let tool_names = result.result["toolNames"].as_array().expect("tool catalog");
    assert_eq!(tool_names.len(), 10);
    assert!(tool_names.iter().any(|tool| tool == "prism_locate"));
    assert!(tool_names.iter().any(|tool| tool == "prism_task_brief"));
    assert!(tool_names.iter().any(|tool| tool == "prism_concept"));
    assert!(tool_names.iter().any(|tool| tool == "prism_mutate"));

    let mutate = &result.result["mutateSummary"];
    assert_eq!(mutate["toolName"], "prism_mutate");
    assert_eq!(mutate["actionCount"], 18);
    assert_eq!(mutate["exampleAction"], "validation_feedback");
    assert_eq!(
        mutate["examplePrismSaid"],
        "Search result ordering was helpful."
    );

    let validation_feedback = &result.result["validationFeedback"];
    assert_eq!(validation_feedback["action"], "validation_feedback");
    assert_eq!(
        validation_feedback["requiredFields"]
            .as_array()
            .expect("required fields")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec![
            "context",
            "prismSaid",
            "actuallyTrue",
            "category",
            "verdict"
        ]
    );
    let verdict_field = validation_feedback["fields"]
        .as_array()
        .expect("field summaries")
        .iter()
        .find(|field| field["name"] == "verdict")
        .expect("verdict field");
    assert_eq!(
        verdict_field["enumValues"]
            .as_array()
            .expect("verdict enum values")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["wrong", "stale", "noisy", "helpful", "mixed"]
    );
    let anchors_field = validation_feedback["fields"]
        .as_array()
        .expect("field summaries")
        .iter()
        .find(|field| field["name"] == "anchors")
        .expect("anchors field");
    let nested_anchor_fields = anchors_field["nestedFields"]
        .as_array()
        .expect("nested anchor field summaries")
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect::<Vec<_>>();
    assert!(nested_anchor_fields.contains(&"type"));
    assert!(
        nested_anchor_fields.contains(&"crateName") || nested_anchor_fields.contains(&"crate_name")
    );
    assert!(nested_anchor_fields.contains(&"path"));
    assert!(nested_anchor_fields.contains(&"kind"));
    assert!(
        nested_anchor_fields.contains(&"lineageId") || nested_anchor_fields.contains(&"lineage_id")
    );
    assert!(anchors_field["schema"]
        .to_string()
        .contains("\"properties\""));

    assert_eq!(
        validation_feedback["schemaUri"],
        "prism://schema/tool/prism_mutate/action/validation_feedback"
    );

    let coordination = &result.result["coordination"];
    assert_eq!(
        coordination["schemaUri"],
        "prism://schema/tool/prism_mutate/action/coordination"
    );
    assert_eq!(coordination["payloadDiscriminator"], "kind");
    let payload_variants = coordination["payloadVariants"]
        .as_array()
        .expect("payload variants");
    assert_eq!(payload_variants.len(), 9);
    let task_create_variant = payload_variants
        .iter()
        .find(|variant| variant["tag"] == "task_create")
        .expect("task_create variant should exist");
    assert_eq!(
        task_create_variant["requiredFields"]
            .as_array()
            .expect("required fields")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["planId", "title"]
    );
    assert_eq!(
        task_create_variant["exampleInput"]["planId"],
        "plan:demo-main"
    );

    let validation = &result.result["validation"];
    assert_eq!(validation["toolName"], "prism_mutate");
    assert_eq!(validation["valid"], false);
    assert_eq!(validation["action"], "coordination");
    assert_eq!(
        validation["actionSchemaUri"],
        "prism://schema/tool/prism_mutate/action/coordination"
    );
    assert_eq!(
        validation["normalizedInput"],
        json!({
            "action": "coordination",
            "input": {
                "kind": "task_create",
                "payload": {
                    "title": "Missing plan id"
                }
            }
        })
    );
    assert!(validation["summary"]
        .as_str()
        .is_some_and(|summary| summary.contains("input.payload.planId")));
    assert_eq!(validation["issues"][0]["path"], "input.payload.planId");

    assert!(result.result["missing"].is_null());
}

#[test]
fn compact_agent_tools_keep_handles_stable_within_one_session() {
    let host = host_with_node(demo_node());
    let session = test_session(&host);

    let first = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "main".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("first locate should succeed");
    let second = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "main".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("second locate should succeed");

    assert_eq!(first.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(first.candidates.len(), 1);
    assert_eq!(second.candidates.len(), 1);
    assert_eq!(first.candidates[0].handle, second.candidates[0].handle);
}

#[test]
fn compact_locate_schema_surfaces_filters_and_preview_knobs() {
    let schema = crate::tool_schema_view("prism_locate").expect("locate schema should exist");
    let properties = schema.input_schema["properties"]
        .as_object()
        .expect("locate schema should expose object properties");

    assert!(properties["path"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("file path fragment")));
    assert!(properties["glob"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("glob")));
    assert!(properties["includeTopPreview"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("top-ranked candidate")));
}

#[test]
fn compact_gather_schema_surfaces_exact_text_knobs() {
    let schema = crate::tool_schema_view("prism_gather").expect("gather schema should exist");
    let properties = schema.input_schema["properties"]
        .as_object()
        .expect("gather schema should expose object properties");

    assert!(properties["query"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("Exact text")));
    assert!(properties["path"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("file path fragment")));
    assert!(properties["glob"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("glob")));
}

#[test]
fn prism_mutate_schema_surfaces_concept_action() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let concept = schema
        .actions
        .iter()
        .find(|action| action.action == "concept")
        .expect("concept action should exist");

    assert!(concept.required_fields.contains(&"operation".to_string()));
    assert!(concept
        .fields
        .iter()
        .any(|field| field.name == "canonicalName"));
    assert!(concept
        .fields
        .iter()
        .any(|field| field.name == "coreMembers"));
}

#[test]
fn prism_mutate_schema_surfaces_contract_action() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let contract = schema
        .actions
        .iter()
        .find(|action| action.action == "contract")
        .expect("contract action should exist");

    assert!(contract.required_fields.contains(&"operation".to_string()));
    assert!(contract.fields.iter().any(|field| field.name == "name"));
    assert!(contract.fields.iter().any(|field| field.name == "subject"));
    assert!(contract
        .fields
        .iter()
        .any(|field| field.name == "guarantees"));
}

#[test]
fn prism_mutate_schema_surfaces_action_specific_examples() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let missing_examples = schema
        .actions
        .iter()
        .filter(|action| action.example_input.is_none())
        .map(|action| action.action.as_str())
        .collect::<Vec<_>>();
    assert!(
        missing_examples.is_empty(),
        "missing examples for actions: {missing_examples:?}"
    );

    let validation_feedback = schema
        .actions
        .iter()
        .find(|action| action.action == "validation_feedback")
        .expect("validation feedback action should exist");
    let memory = schema
        .actions
        .iter()
        .find(|action| action.action == "memory")
        .expect("memory action should exist");
    let concept = schema
        .actions
        .iter()
        .find(|action| action.action == "concept")
        .expect("concept action should exist");
    let coordination = schema
        .actions
        .iter()
        .find(|action| action.action == "coordination")
        .expect("coordination action should exist");

    assert_eq!(
        validation_feedback
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("validation_feedback")
    );
    assert_eq!(
        memory
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("memory")
    );
    assert_eq!(
        concept
            .example_input
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str),
        Some("concept")
    );
    assert!(
        coordination.example_inputs.len() >= 3,
        "coordination should expose multiple action-specific examples"
    );
    assert!(coordination
        .example_inputs
        .iter()
        .any(|value| value["input"]["kind"] == "plan_create"));
    assert!(coordination
        .example_inputs
        .iter()
        .any(|value| value["input"]["kind"] == "plan_node_create"));

    let mutate_schema =
        crate::tool_input_schema_value("prism_mutate").expect("mutate schema value should exist");
    let mutate_examples = mutate_schema["examples"]
        .as_array()
        .expect("mutate examples should be an array")
        .iter()
        .filter_map(|value| value.get("action").and_then(Value::as_str))
        .collect::<Vec<_>>();
    for action in [
        "validation_feedback",
        "outcome",
        "memory",
        "concept",
        "contract",
        "concept_relation",
        "infer_edge",
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
    ] {
        assert!(
            mutate_examples.contains(&action),
            "missing mutate example for action `{action}`"
        );
    }
}

#[test]
fn coordination_schema_surfaces_closed_status_and_kind_enums() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");
    let coordination = schema
        .actions
        .iter()
        .find(|action| action.action == "coordination")
        .expect("coordination action should exist");
    let payload_variants = coordination.input_schema["properties"]["payload"]["oneOf"]
        .as_array()
        .expect("coordination payload should be a tagged union");
    let task_create = payload_variants
        .iter()
        .find(|variant| variant["title"] == "kind=task_create")
        .expect("task_create payload should exist");
    let plan_node_create = payload_variants
        .iter()
        .find(|variant| variant["title"] == "kind=plan_node_create")
        .expect("plan_node_create payload should exist");
    let plan_edge_create = payload_variants
        .iter()
        .find(|variant| variant["title"] == "kind=plan_edge_create")
        .expect("plan_edge_create payload should exist");

    let task_create_schema = task_create.to_string();
    assert!(task_create_schema.contains("\"status\""));
    assert!(task_create_schema.contains("\"ready\""));
    assert!(task_create_schema.contains("\"in_progress\""));

    let plan_node_create_schema = plan_node_create.to_string();
    assert!(plan_node_create_schema.contains("\"kind\""));
    assert!(plan_node_create_schema.contains("\"investigate\""));
    assert!(plan_node_create_schema.contains("\"edit\""));

    let plan_edge_create_schema = plan_edge_create.to_string();
    assert!(plan_edge_create_schema.contains("\"kind\""));
    assert!(plan_edge_create_schema.contains("\"depends_on\""));
    assert!(plan_edge_create_schema.contains("\"handoff_to\""));
}

#[test]
fn prism_mutate_schema_expands_payload_shapes_for_structured_actions() {
    let schema = crate::tool_schema_view("prism_mutate").expect("mutate schema should exist");

    let payload_fields = ["memory", "coordination", "claim", "artifact"]
        .into_iter()
        .map(|action| {
            let payload = schema
                .actions
                .iter()
                .find(|candidate| candidate.action == action)
                .and_then(|candidate| {
                    candidate
                        .fields
                        .iter()
                        .find(|field| field.name == "payload")
                })
                .expect("payload field should exist");
            (action, payload)
        })
        .collect::<Vec<_>>();

    for (action, payload) in &payload_fields {
        assert_ne!(
            payload.schema,
            Value::Bool(true),
            "{action} payload stayed opaque"
        );
        assert!(
            payload.schema.to_string().contains("\"properties\"")
                || payload.schema.to_string().contains("\"oneOf\""),
            "{action} payload schema should expose structure"
        );
    }

    let memory_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "memory")
        .expect("memory payload should exist")
        .1;
    let memory_nested = memory_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(memory_nested.contains(&"anchors"));
    assert!(memory_nested.contains(&"kind"));
    assert!(memory_nested.contains(&"content"));

    let coordination_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "coordination")
        .expect("coordination payload should exist")
        .1;
    assert_eq!(
        coordination_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(9)
    );
    let coordination_nested = coordination_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(coordination_nested.contains(&"id"));
    assert!(coordination_nested.contains(&"planId"));
    assert!(coordination_nested.contains(&"taskId"));
    assert!(coordination_nested.contains(&"title"));

    let claim_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "claim")
        .expect("claim payload should exist")
        .1;
    assert_eq!(
        claim_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(3)
    );
    let claim_nested = claim_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(claim_nested.contains(&"anchors"));
    assert!(claim_nested.contains(&"capability"));
    assert!(claim_nested.contains(&"claimId"));

    let artifact_payload = payload_fields
        .iter()
        .find(|(action, _)| *action == "artifact")
        .expect("artifact payload should exist")
        .1;
    assert_eq!(
        artifact_payload.schema["oneOf"]
            .as_array()
            .map(|variants| variants.len()),
        Some(3)
    );
    let artifact_nested = artifact_payload
        .nested_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert!(artifact_nested.contains(&"taskId"));
    assert!(artifact_nested.contains(&"artifactId"));
    assert!(artifact_nested.contains(&"verdict"));
}

#[test]
fn coordination_status_errors_are_self_repairing() {
    let host = host_with_node(demo_node());
    let session = test_session(&host);
    let plan = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Self-repairing coordination validation"
                }),
                task_id: None,
            },
        )
        .expect("plan should be created");
    let error = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Investigate refresh path",
                    "status": "not_a_real_status",
                }),
                task_id: None,
            },
        )
        .expect_err("invalid status should be rejected")
        .to_string();

    assert!(error.contains("Allowed values"));
    assert!(error.contains(r#"{"status":"ready"}"#));
    assert!(error.contains("prism://vocab"));
}

#[test]
fn prism_query_reports_complete_mutate_examples_and_payload_shapes() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const mutate = prism.tool("prism_mutate");
return {
  missingExamples: mutate?.actions.filter((action) => !action.exampleInput).map((action) => action.action) ?? [],
  opaquePayloadActions: mutate?.actions
    .filter((action) =>
      action.fields.some(
        (field) => field.name === "payload" && JSON.stringify(field.schema) === "true"
      )
    )
    .map((action) => action.action) ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("tool schema query should succeed");

    assert_eq!(result.result["missingExamples"], json!([]));
    assert_eq!(result.result["opaquePayloadActions"], json!([]));
}

#[test]
fn compact_locate_uses_intent_to_choose_between_code_and_docs() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
# Demo

## Event Journal

This section explains the event journal flow.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let edit = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("edit locate should succeed");
    assert_eq!(edit.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(edit.candidates[0].kind, NodeKind::Function);
    assert_eq!(edit.candidates[0].path, "demo::event_journal_snapshot");

    let explain = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("explain locate should succeed");
    assert_eq!(explain.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(explain.candidates[0].kind, NodeKind::MarkdownHeading);
    assert!(explain.candidates[0]
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("docs/SPEC.md")));
}

#[test]
fn compact_locate_defaults_to_docs_friendly_intent_for_docs_paths() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
# Demo

## Event Journal

This section explains the event journal flow.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: None,
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);
}

#[test]
fn prism_locate_accepts_docs_alias_task_intent() {
    let args: PrismLocateArgs = serde_json::from_value(json!({
        "query": "event journal",
        "taskIntent": "docs",
    }))
    .expect("docs alias should deserialize");

    assert!(matches!(
        args.task_intent,
        Some(PrismLocateTaskIntentInput::Explain)
    ));
}

#[test]
fn prism_locate_accepts_code_and_read_alias_task_intent() {
    for alias in ["code", "read"] {
        let args: PrismLocateArgs = serde_json::from_value(json!({
            "query": "event journal",
            "taskIntent": alias,
        }))
        .expect("inspect alias should deserialize");

        assert!(matches!(
            args.task_intent,
            Some(PrismLocateTaskIntentInput::Inspect)
        ));
    }
}

#[test]
fn compact_locate_prefers_identifier_matches_over_test_helpers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
pub mod helpers;
pub mod tests;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn compact_open() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn cached_related_memory() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/tests.rs"),
        r#"
pub fn compact_open_returns_compact_related_handles() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open related_handles".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::Function);
    assert_eq!(
        locate.candidates[0].path,
        "demo::compact_tools::compact_open"
    );
}

#[test]
fn compact_locate_promotes_exact_identifier_candidates_before_fuzzy_matches() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
pub mod locate_helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn locate_intent_profile() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/locate_helpers.rs"),
        r#"
pub fn locate_query_tokens() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "locate_intent_profile compact_tools".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(
        locate.candidates[0].path,
        "demo::compact_tools::locate_intent_profile"
    );
}

#[test]
fn compact_locate_prefers_owner_like_boundaries_for_routing_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod app_shell;
pub mod operation_detail;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/app_shell.rs"),
        r#"
pub fn route_page_shell() {
    render_page_layout();
}

fn render_page_layout() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/operation_detail.rs"),
        r#"
pub fn render_operation_detail() {
    let note = "routing page shell";
    println!("{note}");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "routing page shell".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::Function);
    assert_eq!(
        locate.candidates[0].path,
        "demo::app_shell::route_page_shell"
    );
    assert!(locate.candidates[0]
        .why_short
        .to_ascii_lowercase()
        .contains("ownership-style query"));
}

#[test]
fn compact_locate_can_include_top_preview() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod compact_tools;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/compact_tools.rs"),
        r#"
pub fn compact_open() {
    println!("preview");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open".to_string(),
                path: Some("src/compact_tools.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    let preview = locate
        .top_preview
        .expect("locate should include a top preview");
    assert_eq!(preview.handle, locate.candidates[0].handle);
    assert!(preview.text.contains("pub fn compact_open"));
}

#[test]
fn compact_locate_can_return_text_fragment_handles_for_exact_script_metrics() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_query_calls = 0
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": prism_compact_tool_calls}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::Document);
    assert!(locate.candidates[0].path.contains("benchmark_codex.py:"));
    assert!(locate.candidates[0]
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with("benchmark_codex.py")));
    let preview = locate
        .top_preview
        .expect("text-fragment locate should include a preview");
    assert!(preview.text.contains("prism_compact_tool_calls"));

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    assert!(open.text.contains("prism_compact_tool_calls"));
}

#[test]
fn compact_open_remaps_stale_text_fragment_handles_after_file_edits() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        "def helper():\n    prism_query_calls = 0\n    prism_compact_tool_calls = 0\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::Document);

    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        "def helper():\n    prism_query_calls = 0\n    helper_value = 1\n    prism_compact_tool_calls = 0\n",
    )
    .unwrap();

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should remap the stale text-fragment handle");

    assert!(open.remapped);
    assert_eq!(open.start_line, 4);
    assert_eq!(open.end_line, 4);
    assert!(open.text.contains("prism_compact_tool_calls = 0"));
}

#[test]
fn compact_open_returns_compact_related_handles() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn event_journal_snapshot() {
    persist_event_journal();
}
fn persist_event_journal() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        r#"
## Event Journal

The event journal snapshot should persist journal entries.
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "event journal".to_string(),
                path: None,
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    let related = open
        .related_handles
        .expect("open should surface compact related handles");
    assert!(!related.is_empty());
    assert!(related.len() <= 2);
    assert!(related.iter().all(|target| target.file_path.is_none()));
    assert!(related
        .iter()
        .any(|target| target.kind == NodeKind::Function));
}

#[test]
fn compact_open_raw_reads_the_literal_symbol_span_instead_of_only_the_signature_line() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn compact_open() {
    let preview = "preview";
    println!("{preview}");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "compact_open".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("raw open should succeed");

    assert!(open.text.contains("let preview = \"preview\";"));
    assert!(open.text.contains("println!"));
    assert!(open.end_line > open.start_line);
}

#[test]
fn compact_open_deepens_shallow_semantic_files_on_first_touch() {
    let root = temp_workspace();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for index in 0..64 {
        fs::write(
            root.join(format!("src/helper_{index}.rs")),
            format!("pub fn helper_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let target_path = root.join("src/lib.rs");
    fs::write(
        &target_path,
        "pub fn alpha() {\n    beta();\n}\n\nfn beta() {}\n",
    )
    .unwrap();
    let target_path = fs::canonicalize(target_path).unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let initial_record = host
        .current_prism()
        .graph()
        .file_record(&target_path)
        .expect("lib file should be indexed")
        .clone();
    assert!(initial_record.unresolved_calls.is_empty());

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "alpha".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    assert!(open.text.contains("pub fn alpha()"));

    let deepened_prism = host.current_prism();
    let deepened_record = deepened_prism
        .graph()
        .file_record(&target_path)
        .expect("lib file should remain indexed");
    assert!(deepened_record
        .unresolved_calls
        .iter()
        .any(|call| call.caller.path == "demo::alpha" && call.name == "beta"));
}

#[test]
fn compact_open_edit_returns_enough_body_context_to_start_editing() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
fn helper_value(seed: i32) -> i32 {
    seed + 1
}

pub fn edit_target() {
    let alpha = helper_value(1);
    let beta = alpha + 1;
    let gamma = beta + 1;
    let delta = gamma + 1;
    let epsilon = delta + 1;
    let zeta = epsilon + 1;
    let eta = zeta + 1;
    let theta = eta + 1;
    let iota = theta + 1;
    println!("{iota}");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "edit_target".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Edit),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("edit open should succeed");

    assert!(open.text.contains("let alpha = helper_value(1);"));
    assert!(open.text.contains("let theta = eta + 1;"));
    assert!(open.text.contains("println!(\"{iota}\");"));
}

#[test]
fn compact_open_edit_surfaces_monolith_pressure_and_owner_followup_guidance() {
    let root = temp_workspace();
    let mut source = String::from(
        r#"
fn helper_region() -> i32 {
    7
}

pub fn giant_target() {
"#,
    );
    for idx in 0..48 {
        source.push_str(&format!(
            "    let segment_{idx} = helper_region() + {idx}; // padding padding padding padding padding padding padding padding\n"
        ));
    }
    source.push_str("}\n");
    fs::write(root.join("src/lib.rs"), source).unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "giant_target".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Edit),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("edit open should succeed");

    let next_action = open
        .next_action
        .as_deref()
        .expect("edit open should return follow-through guidance");
    assert!(open.truncated);
    assert!(next_action.contains("large or mixed-purpose target"));
    assert!(next_action.contains("prism_workset"));
    assert!(next_action.contains("prism_open") || next_action.contains("prism_expand `neighbors`"));
}

#[test]
fn compact_open_text_fragment_edit_mode_widens_beyond_raw_line_windows() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        concat!(
            "fn alpha() {\n",
            "    let one = 1;\n",
            "    let two = one + 1;\n",
            "    let three = two + 1;\n",
            "    let four = three + 1;\n",
            "    let five = four + 1;\n",
            "    println!(\"{five}\");\n",
            "}\n",
        ),
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let raw = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Raw),
                line: Some(4),
                before_lines: Some(0),
                after_lines: Some(0),
                max_chars: Some(160),
            },
        )
        .expect("raw path open should succeed");

    assert_eq!(raw.start_line, 4);
    assert_eq!(raw.end_line, 4);
    assert!(raw.text.contains("let three = two + 1;"));

    let edit = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(raw.handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Edit),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("text-fragment edit open should widen the window");

    assert!(edit.start_line <= 2);
    assert!(edit.end_line >= 7);
    assert!(edit.text.contains("let one = 1;"));
    assert!(edit.text.contains("let five = four + 1;"));
    assert!(edit.text.contains("println!(\"{five}\");"));
}

#[test]
fn compact_open_supports_exact_workspace_paths_without_locate() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn alpha() {}
pub fn beta() {
    let value = 42;
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Raw),
                line: Some(3),
                before_lines: Some(0),
                after_lines: Some(2),
                max_chars: Some(200),
            },
        )
        .expect("path open should succeed");

    assert_eq!(
        open.handle_category,
        prism_js::AgentHandleCategoryView::TextFragment
    );
    assert!(open.handle.starts_with("handle:"));
    assert!(open.text.contains("pub fn beta()"));
    assert!(open.text.contains("let value = 42;"));

    let reopened = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(open.handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("reopening the exact-path handle should succeed");
    assert_eq!(reopened.text, open.text);
    assert_eq!(reopened.start_line, open.start_line);
    assert_eq!(reopened.end_line, open.end_line);
}

#[test]
fn compact_open_supports_edit_mode_for_exact_paths_with_default_context() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        concat!(
            "pub fn alpha() {}\n",
            "pub fn beta() {\n",
            "    let value = 42;\n",
            "    let doubled = value * 2;\n",
            "    let tripled = doubled + value;\n",
            "    println!(\"{tripled}\");\n",
            "}\n",
        ),
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let open = host
        .compact_open(
            test_session(&host),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Edit),
                line: Some(4),
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("path open should support edit mode when a line is provided");

    assert_eq!(
        open.handle_category,
        prism_js::AgentHandleCategoryView::TextFragment
    );
    assert!(open.text.contains("pub fn beta() {"));
    assert!(open.text.contains("let value = 42;"));
    assert!(open.text.contains("println!(\"{tripled}\");"));
    assert_eq!(open.start_line, 2);
    assert_eq!(open.end_line, 7);
}

#[test]
fn compact_open_rejects_unsupported_exact_path_modes() {
    let root = temp_workspace();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let focus_error = host
        .compact_open(
            test_session(&host),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect_err("path open should reject focus mode");

    assert!(focus_error.to_string().contains(
        "path-based prism_open currently supports raw mode, or edit mode when `line` is set"
    ));

    let edit_error = host
        .compact_open(
            test_session(&host),
            PrismOpenArgs {
                handle: None,
                path: Some("src/lib.rs".to_string()),
                mode: Some(PrismOpenModeInput::Edit),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect_err("path edit open should require a line");

    assert!(edit_error
        .to_string()
        .contains("path-based prism_open edit mode requires `line`"));
}

#[test]
fn compact_open_rejects_prism_resource_uris_as_exact_paths() {
    let root = temp_workspace();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let error = host
        .compact_open(
            test_session(&host),
            PrismOpenArgs {
                handle: None,
                path: Some("prism://session".to_string()),
                mode: Some(PrismOpenModeInput::Raw),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect_err("resource URIs should not be treated as workspace file paths");

    assert!(error
        .to_string()
        .contains("PRISM resource URI, not a workspace file path"));
    assert!(error.to_string().contains("MCP resource surface"));
}

#[test]
fn compact_workset_for_text_fragment_handles_surfaces_related_slices() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": 1}
    print("prism_compact_tool_calls")
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.why.contains("Exact text hit"));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("reopen it in edit mode")));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.handle.is_some() && action.open_mode.is_some()
    }));
}

#[test]
fn compact_workset_for_code_symbols_prefers_same_file_graph_neighbors() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
fn parse_input(raw: &str) -> String {
    raw.trim().to_string()
}

fn persist_result(value: &str) {
    println!("{value}");
}

pub fn edit_target(raw: &str) {
    let parsed = parse_input(raw);
    persist_result(&parsed);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("tests/edit_target.rs"),
        r#"
#[test]
fn edit_target_smoke_test() {
    demo::edit_target("value");
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "edit_target".to_string(),
                path: Some("src/lib.rs".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Edit),
                limit: Some(1),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path == "demo::parse_input" || target.path == "demo::persist_result"));
    assert!(workset.supporting_reads[0]
        .why_short
        .contains("Direct callee from this symbol."));
    assert!(workset
        .likely_tests
        .iter()
        .all(|target| target.path != "demo::parse_input"));
    assert!(workset.why.contains("Start with `"));
    assert!(workset.why.contains("Likely test: `"));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("first supporting read")));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(workset.primary.handle.as_str())
            && action.open_mode == Some(prism_js::AgentOpenMode::Edit)
    }));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(workset.supporting_reads[0].handle.as_str())
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("likely test")));
}

#[test]
fn compact_concept_returns_validation_packet_and_decode() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
pub fn validation_recipe_test() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::start_task".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:compact-concept".to_string()),
        },
    )
    .expect("concept setup should succeed");

    let concept = host
        .compact_concept(
            Arc::clone(&session),
            PrismConceptArgs {
                handle: None,
                query: Some("validation".to_string()),
                lens: Some(PrismConceptLensInput::Validation),
                verbosity: None,
                include_binding_metadata: Some(true),
            },
        )
        .expect("concept tool should succeed");

    assert_eq!(concept.packet.handle, "concept://custom_validation");
    assert!(!concept.packet.core_members.is_empty());
    assert!(concept.packet.binding_metadata.is_some());
    assert!(concept
        .packet
        .resolution
        .as_ref()
        .is_some_and(|resolution| !resolution.reasons.is_empty()));
    assert!(concept
        .decode
        .as_ref()
        .and_then(|decode| decode.validation_recipe.as_ref())
        .is_some());
}

#[test]
fn compact_concept_summary_verbosity_trims_relations_and_evidence() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
pub fn read_context() {}
pub fn edit_context() {}
pub fn validation_context() {}
pub fn task_journal() {}
pub fn task_risk() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://summary_validation".to_string()),
            canonical_name: Some("summary_validation".to_string()),
            summary: Some("Validation concept used to test compact verbosity.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::start_task".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::read_context".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::edit_context".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_context".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::task_journal".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::task_risk".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            likely_tests: None,
            evidence: Some(vec![
                "Curated in test.".to_string(),
                "Second evidence string.".to_string(),
                "Third evidence string.".to_string(),
            ]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:compact-concept-summary".to_string()),
        },
    )
    .expect("concept setup should succeed");
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://runtime_surface".to_string()),
            canonical_name: Some("runtime_surface".to_string()),
            summary: Some("Runtime-facing status helpers.".to_string()),
            aliases: Some(vec!["runtime".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Seeded as the relation target in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.85),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:compact-concept-summary".to_string()),
        },
    )
    .expect("target concept setup should succeed");
    host.store_concept_relation(
        session.as_ref(),
        PrismConceptRelationMutationArgs {
            operation: ConceptRelationMutationOperationInput::Upsert,
            source_handle: "concept://summary_validation".to_string(),
            target_handle: "concept://runtime_surface".to_string(),
            kind: ConceptRelationKindInput::DependsOn,
            confidence: Some(0.9),
            evidence: Some(vec![
                "Depends on runtime status plumbing.".to_string(),
                "Second relation evidence.".to_string(),
            ]),
            scope: Some(ConceptScopeInput::Session),
            task_id: Some("task:compact-concept-summary".to_string()),
        },
    )
    .expect("relation setup should succeed");

    let concept = host
        .compact_concept(
            Arc::clone(&session),
            PrismConceptArgs {
                handle: None,
                query: Some("validation".to_string()),
                lens: Some(PrismConceptLensInput::Validation),
                verbosity: Some(PrismConceptVerbosityInput::Summary),
                include_binding_metadata: Some(false),
            },
        )
        .expect("concept tool should succeed");

    assert!(concept.packet.core_members.len() <= 3);
    assert!(concept.packet.supporting_members.len() <= 3);
    assert!(concept.packet.evidence.len() <= 2);
    assert!(concept.packet.relations.len() <= 1);
    assert_eq!(
        concept.packet.verbosity_applied,
        prism_js::ConceptPacketVerbosityView::Summary
    );
    assert!(concept.packet.truncation.is_some());
    assert!(concept.packet.relations[0].evidence.is_empty());
    assert!(concept
        .packet
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("trimmed for context")));
    assert!(concept.decode.as_ref().is_some_and(|decode| {
        decode.concept.relations[0].evidence.is_empty()
            && decode.concept.verbosity_applied == prism_js::ConceptPacketVerbosityView::Summary
            && decode.concept.truncation.is_some()
    }));
}

#[test]
fn prism_query_memory_flat_aliases_remain_compatible() {
    let host = host_with_node(demo_node());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::main",
                    "kind": "function"
                }],
                "kind": "episodic",
                "content": "main previously regressed on null handling",
                "trust": 0.9
            }),
            task_id: None,
        },
    )
    .expect("note should store");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return prism.memoryRecall({
  focus: sym ? [sym] : [],
  text: "null",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("flat memory alias should succeed");

    assert_eq!(
        result.result[0]["entry"]["content"],
        "main previously regressed on null handling"
    );
}

#[test]
fn query_concept_defaults_are_conservative_and_report_truncation() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
pub fn read_context() {}
pub fn edit_context() {}
pub fn validation_context() {}
pub fn task_journal() {}
pub fn task_risk() {}
pub fn plan_summary() {}
pub fn blockers() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://query_defaults_validation".to_string()),
            canonical_name: Some("query_defaults_validation".to_string()),
            summary: Some("Validation concept used to test query defaults.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::start_task".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::read_context".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::edit_context".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec![
                "Curated in test.".to_string(),
                "Second evidence string.".to_string(),
                "Third evidence string.".to_string(),
                "Fourth evidence string.".to_string(),
                "Fifth evidence string.".to_string(),
            ]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:query-concept-defaults".to_string()),
        },
    )
    .expect("concept setup should succeed");

    let envelope = host
        .execute(
            test_session(&host),
            r#"
return {
  list: prism.concepts("validation"),
  single: prism.concept("validation"),
  byHandle: prism.conceptByHandle("concept://query_defaults_validation"),
  decoded: prism.decodeConcept({ query: "validation", lens: "validation" }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");

    assert_eq!(envelope.result["list"][0]["verbosityApplied"], "summary");
    assert_eq!(envelope.result["single"]["verbosityApplied"], "standard");
    assert_eq!(envelope.result["byHandle"]["verbosityApplied"], "standard");
    assert_eq!(
        envelope.result["decoded"]["concept"]["verbosityApplied"],
        "standard"
    );
    assert_eq!(
        envelope.result["list"][0]["coreMembers"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        envelope.result["single"]["coreMembers"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        envelope.result["list"][0]["truncation"]["coreMembersOmitted"],
        Value::from(2)
    );
    assert_eq!(
        envelope.result["single"]["truncation"]["evidenceOmitted"],
        Value::from(1)
    );
}

#[test]
fn compact_concept_returns_alternates_for_ambiguous_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn healthcheck_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://validation_pipeline".to_string()),
            canonical_name: Some("validation_pipeline".to_string()),
            summary: Some("Validation checks and likely tests.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.92),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:ambiguous-validation".to_string()),
        },
    )
    .expect("validation concept setup should succeed");
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://validation_health_checks".to_string()),
            canonical_name: Some("validation_health_checks".to_string()),
            summary: Some(
                "Validation-oriented runtime health checks and status probes.".to_string(),
            ),
            aliases: Some(vec!["validation".to_string(), "health checks".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::healthcheck_status".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: None,
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.9),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:ambiguous-validation".to_string()),
        },
    )
    .expect("runtime concept setup should succeed");

    let concept = host
        .compact_concept(
            Arc::clone(&session),
            PrismConceptArgs {
                handle: None,
                query: Some("validation".to_string()),
                lens: None,
                verbosity: None,
                include_binding_metadata: Some(false),
            },
        )
        .expect("concept tool should succeed");

    assert!(!concept.alternates.is_empty());
    assert!(concept
        .alternates
        .iter()
        .any(|alternate| alternate.handle == "concept://validation_health_checks"));
}

#[test]
fn compact_tools_route_or_reject_concept_handles_with_clear_followups() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn validation_recipe_test() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![
                PrismConceptLensInput::Open,
                PrismConceptLensInput::Workset,
                PrismConceptLensInput::Validation,
            ]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:concept-followup".to_string()),
        },
    )
    .expect("concept setup should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some("concept://custom_validation".to_string()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should accept concept handles");
    assert_eq!(open.handle, "concept://custom_validation");
    assert_eq!(
        open.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert!(open.file_path.ends_with("/src/lib.rs"));
    assert!(open.text.contains("pub fn validation_recipe()"));
    assert_eq!(
        open.promoted_handle
            .as_ref()
            .expect("concept open should stage a primary member")
            .path,
        "demo::validation_recipe"
    );
    assert_eq!(
        open.related_handles
            .as_ref()
            .expect("concept open should expose related members")[0]
            .path,
        "demo::runtime_status"
    );
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset")));
    assert!(open
        .suggested_actions
        .iter()
        .any(|action| action.tool == "prism_expand"));

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some("concept://custom_validation".to_string()),
                query: None,
            },
        )
        .expect("workset should accept concept handles");
    assert_eq!(workset.primary.path, "demo::validation_recipe");
    assert_eq!(
        workset.primary.handle_category,
        prism_js::AgentHandleCategoryView::Symbol
    );
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_concept")));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation expand should accept concept handles");
    assert_eq!(
        validation.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(validation.kind, prism_js::AgentExpandKind::Validation);
    assert!(validation.result["likelyTests"].is_array());
    assert!(validation.result["why"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(validation
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset")));

    let health = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Health,
                include_top_preview: None,
            },
        )
        .expect("health expand should accept concept handles");
    assert_eq!(
        health.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(health.kind, prism_js::AgentExpandKind::Health);
    assert_eq!(health.result["status"], "drifted");
    assert!(health.result["repairTaskPayload"].is_object());
    assert!(health.result["signals"]["staleValidationLinks"]
        .as_bool()
        .unwrap());
    assert!(health
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("timeline")));

    host.store_outcome(
        session.as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FailureObserved,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validation failed".to_string(),
            result: Some(OutcomeResultInput::Failure),
            evidence: None,
            task_id: None,
        },
    )
    .expect("failure outcome should store");
    let mut memory = MemoryEntry::new(MemoryKind::Structural, "validation concept memory");
    memory.anchors = vec![prism_ir::AnchorRef::Node(NodeId::new(
        "demo",
        "demo::validation_recipe",
        NodeKind::Function,
    ))];
    session.notes.store(memory).expect("memory should store");

    let timeline = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Timeline,
                include_top_preview: None,
            },
        )
        .expect("timeline expand should accept concept handles");
    assert_eq!(
        timeline.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(timeline.kind, prism_js::AgentExpandKind::Timeline);
    assert!(timeline.result["recentEvents"].is_array());
    assert_eq!(
        timeline.result["lastFailure"]["summary"],
        "validation failed"
    );

    let memory = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Memory,
                include_top_preview: None,
            },
        )
        .expect("memory expand should accept concept handles");
    assert_eq!(
        memory.handle_category,
        prism_js::AgentHandleCategoryView::Concept
    );
    assert_eq!(memory.kind, prism_js::AgentExpandKind::Memory);
    assert!(memory.result["memories"].is_array());
    assert_eq!(
        memory.result["memories"][0]["summary"],
        "validation concept memory"
    );

    let unsupported_expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: "concept://custom_validation".to_string(),
                kind: PrismExpandKindInput::Lineage,
                include_top_preview: None,
            },
        )
        .expect_err("unsupported expand kind should still reject concept handles");
    assert!(
        unsupported_expand.to_string().contains("prism_concept"),
        "{unsupported_expand}"
    );
    assert!(
        !unsupported_expand
            .to_string()
            .contains("rerun prism_locate"),
        "{unsupported_expand}"
    );
}

#[test]
fn query_runtime_exposes_concept_packets_and_decode() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    host.store_concept(
        test_session(&host).as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://custom_validation".to_string()),
            canonical_name: Some("custom_validation".to_string()),
            summary: Some("Custom curated validation concept.".to_string()),
            aliases: Some(vec!["validation".to_string(), "checks".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::start_task".to_string(),
                kind: "function".to_string(),
            }]),
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::runtime_status".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec!["Curated in test.".to_string()]),
            risk_hint: None,
            confidence: Some(0.91),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:query-concept".to_string()),
        },
    )
    .expect("concept setup should succeed");
    let envelope = host
        .execute(
            test_session(&host),
            r#"
return {
  concept: prism.concept("validation", { includeBindingMetadata: true }),
  byHandle: prism.conceptByHandle("concept://custom_validation", { includeBindingMetadata: true }),
  decoded: prism.decodeConcept({ query: "validation", lens: "validation", includeBindingMetadata: true }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");

    assert_eq!(
        envelope.result["concept"]["handle"],
        Value::String("concept://custom_validation".to_string())
    );
    assert_eq!(
        envelope.result["concept"]["curationHints"]["inspectFirst"]["path"],
        Value::String("demo::validation_recipe".to_string())
    );
    assert_eq!(
        envelope.result["concept"]["curationHints"]["supportingRead"]["path"],
        Value::String("demo::start_task".to_string())
    );
    assert_eq!(
        envelope.result["concept"]["curationHints"]["likelyTest"]["path"],
        Value::String("demo::runtime_status".to_string())
    );
    assert!(envelope.result["concept"]["curationHints"]["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("runtime_status")));
    assert!(envelope.result["concept"]["resolution"].is_object());
    assert!(envelope.result["concept"]["bindingMetadata"].is_object());
    assert!(envelope.result["byHandle"]["bindingMetadata"].is_object());
    assert!(envelope.result["decoded"]["concept"]["bindingMetadata"].is_object());
    assert!(envelope.result["decoded"]["validationRecipe"].is_object());
}

#[test]
fn concept_mutation_promotes_updates_and_reloads_repo_concepts() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
pub fn start_task() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let promoted = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: Some("custom_validation".to_string()),
                summary: Some("Custom curated validation concept.".to_string()),
                aliases: Some(vec!["validation".to_string(), "custom checks".to_string()]),
                core_members: Some(vec![
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::validation_recipe".to_string(),
                        kind: "function".to_string(),
                    },
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::runtime_status".to_string(),
                        kind: "function".to_string(),
                    },
                ]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Promoted from live repo work.".to_string()]),
                risk_hint: Some(SparsePatchInput::Value("Keep checks aligned.".to_string())),
                confidence: Some(0.91),
                decode_lenses: Some(vec![
                    PrismConceptLensInput::Validation,
                    PrismConceptLensInput::Memory,
                ]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://legacy_validation".to_string()]),
                retirement_reason: None,
                task_id: Some("task:concept-promote".to_string()),
            },
        )
        .expect("concept promote should succeed");

    assert!(promoted.event_id.starts_with("concept-event:"));
    assert_eq!(promoted.concept_handle, "concept://custom_validation");
    assert_eq!(
        promoted.packet.summary,
        "Custom curated validation concept."
    );
    assert_eq!(promoted.packet.provenance.origin, "repo_mutation");
    assert_eq!(
        promoted
            .packet
            .publication
            .as_ref()
            .map(|value| value.status),
        Some(prism_js::ConceptPublicationStatusView::Active)
    );
    assert_eq!(
        promoted
            .packet
            .publication
            .as_ref()
            .map(|value| value.supersedes.clone()),
        Some(vec!["concept://legacy_validation".to_string()])
    );

    let updated = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Update,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: Some("Updated curated validation concept.".to_string()),
                aliases: Some(vec!["validation".to_string(), "updated checks".to_string()]),
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Updated after more repo work.".to_string()]),
                risk_hint: Some(SparsePatchInput::Value(
                    "Config drift is common.".to_string(),
                )),
                confidence: Some(0.95),
                decode_lenses: Some(vec![
                    PrismConceptLensInput::Open,
                    PrismConceptLensInput::Validation,
                ]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://older_validation_flow".to_string()]),
                retirement_reason: None,
                task_id: Some("task:concept-update".to_string()),
            },
        )
        .expect("concept update should succeed");

    assert_eq!(
        updated.packet.summary,
        "Updated curated validation concept."
    );
    assert_eq!(updated.packet.aliases[1], "updated checks");
    assert_eq!(
        updated
            .packet
            .publication
            .as_ref()
            .map(|value| value.supersedes.clone()),
        Some(vec!["concept://older_validation_flow".to_string()])
    );

    let cleared = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Update,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: None,
                aliases: None,
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: None,
                risk_hint: Some(SparsePatchInput::Patch(SparsePatchObjectInput {
                    op: SparsePatchOpInput::Clear,
                    value: None,
                })),
                confidence: None,
                decode_lenses: None,
                scope: None,
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:concept-clear-risk-hint".to_string()),
            },
        )
        .expect("concept riskHint clear should succeed");
    assert_eq!(cleared.packet.risk_hint, None);

    let retired = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Retire,
                handle: Some("concept://custom_validation".to_string()),
                canonical_name: None,
                summary: None,
                aliases: None,
                core_members: None,
                supporting_members: None,
                likely_tests: None,
                evidence: None,
                risk_hint: None,
                confidence: None,
                decode_lenses: None,
                scope: Some(ConceptScopeInput::Repo),
                supersedes: Some(vec!["concept://validation_pipeline".to_string()]),
                retirement_reason: Some(
                    "Replaced by the canonical validation pipeline concept.".to_string(),
                ),
                task_id: Some("task:concept-retire".to_string()),
            },
        )
        .expect("concept retire should succeed");
    assert_eq!(
        retired
            .packet
            .publication
            .as_ref()
            .map(|value| value.status),
        Some(prism_js::ConceptPublicationStatusView::Retired)
    );

    let queried = host
        .execute(
            test_session(&host),
            r#"
return prism.conceptByHandle("concept://custom_validation");
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");
    assert_eq!(queried.result, Value::Null);

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let persisted = reloaded
        .execute(
            test_session(&reloaded),
            r#"
return prism.conceptByHandle("concept://custom_validation");
"#,
            QueryLanguage::Ts,
        )
        .expect("reloaded concept query should succeed");
    assert_eq!(persisted.result, Value::Null);
}

#[test]
fn concept_mutation_rejects_weak_repo_concepts() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let error = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://weak_validation".to_string()),
                canonical_name: Some("weak_validation".to_string()),
                summary: Some("Too weak".to_string()),
                aliases: Some(vec!["validation".to_string()]),
                core_members: Some(vec![NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                }]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["thin".to_string()]),
                risk_hint: None,
                confidence: Some(0.55),
                decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
                scope: Some(ConceptScopeInput::Repo),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:weak-concept".to_string()),
            },
        )
        .expect_err("weak repo concept should be rejected");

    assert!(error
        .to_string()
        .contains("concept coreMembers must contain at least 2"));
}

#[test]
fn concept_mutation_persists_session_scope_but_not_local_scope() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let session_concept = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://workspace_validation".to_string()),
                canonical_name: Some("workspace_validation".to_string()),
                summary: Some("Workspace-scoped validation concept for local reuse.".to_string()),
                aliases: Some(vec![
                    "validation".to_string(),
                    "workspace checks".to_string(),
                ]),
                core_members: Some(vec![
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::validation_recipe".to_string(),
                        kind: "function".to_string(),
                    },
                    NodeIdInput {
                        crate_name: "demo".to_string(),
                        path: "demo::runtime_status".to_string(),
                        kind: "function".to_string(),
                    },
                ]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Promoted for workspace reuse.".to_string()]),
                risk_hint: None,
                confidence: Some(0.86),
                decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
                scope: Some(ConceptScopeInput::Session),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:session-concept".to_string()),
            },
        )
        .expect("session concept should store");
    assert_eq!(
        session_concept.packet.scope,
        prism_js::ConceptScopeView::Session
    );
    assert!(session_concept.packet.publication.is_none());

    let local_concept = host
        .store_concept(
            session.as_ref(),
            PrismConceptMutationArgs {
                operation: ConceptMutationOperationInput::Promote,
                handle: Some("concept://local_validation_probe".to_string()),
                canonical_name: Some("local_validation_probe".to_string()),
                summary: Some(
                    "Runtime-only validation cluster for the current debugging pass.".to_string(),
                ),
                aliases: Some(vec!["validation probe".to_string()]),
                core_members: Some(vec![NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                }]),
                supporting_members: None,
                likely_tests: None,
                evidence: Some(vec!["Temporary local concept.".to_string()]),
                risk_hint: None,
                confidence: Some(0.6),
                decode_lenses: Some(vec![PrismConceptLensInput::Open]),
                scope: Some(ConceptScopeInput::Local),
                supersedes: None,
                retirement_reason: None,
                task_id: Some("task:local-concept".to_string()),
            },
        )
        .expect("local concept should store");
    assert_eq!(
        local_concept.packet.scope,
        prism_js::ConceptScopeView::Local
    );

    let visible_now = host
        .execute(
            test_session(&host),
            r#"
return {
  session: prism.conceptByHandle("concept://workspace_validation"),
  local: prism.conceptByHandle("concept://local_validation_probe"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("concept query should succeed");
    assert_eq!(visible_now.result["session"]["scope"], "session");
    assert_eq!(visible_now.result["local"]["scope"], "local");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let persisted = reloaded
        .execute(
            test_session(&reloaded),
            r#"
return {
  session: prism.conceptByHandle("concept://workspace_validation"),
  local: prism.conceptByHandle("concept://local_validation_probe"),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("reloaded concept query should succeed");
    assert_eq!(persisted.result["session"]["scope"], "session");
    assert_eq!(persisted.result["local"], Value::Null);
}

#[test]
fn session_memory_persists_locally_while_local_memory_does_not_reload() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "semantic",
                "scope": "session",
                "content": "alpha keeps a workspace-scoped validation hint",
                "trust": 0.8
            }),
            task_id: Some("task:session-memory".to_string()),
        },
    )
    .expect("session memory should persist");

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "episodic",
                "scope": "local",
                "content": "temporary alpha debugging note that should stay runtime-only",
                "trust": 0.6
            }),
            task_id: Some("task:local-memory".to_string()),
        },
    )
    .expect("local memory should store");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let recalled = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  limit: 10,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    let rendered = recalled.result.to_string();
    assert!(rendered.contains("workspace-scoped validation hint"));
    assert!(!rendered.contains("runtime-only"));

    let events = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  limit: 10,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory events query should succeed");
    let event_json = events.result.to_string();
    assert!(event_json.contains("\"scope\":\"Session\""));
    assert!(!event_json.contains("\"scope\":\"Local\""));
}

#[test]
fn compact_gather_returns_multiple_exact_slices() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("benchmarks/scripts")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("benchmarks/scripts/benchmark_codex.py"),
        r#"
def helper():
    prism_compact_tool_calls = 0
    payload = {"prism_compact_tool_calls": 1}
    print("prism_compact_tool_calls")
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "prism_compact_tool_calls".to_string(),
                path: Some("benchmarks/scripts/benchmark_codex.py".to_string()),
                glob: None,
                limit: Some(3),
            },
        )
        .expect("gather should succeed");

    assert_eq!(gather.matches.len(), 3);
    assert!(!gather.truncated);
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.text.contains("prism_compact_tool_calls")));
    assert!(gather.matches.iter().all(|matched| {
        matched
            .next_action
            .as_deref()
            .is_some_and(|next| next.contains("prism_gather"))
    }));
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.promoted_handle.is_none()));
    assert!(gather
        .matches
        .iter()
        .all(|matched| matched.suggested_actions.iter().any(|action| {
            action.tool == "prism_workset"
                && action.handle.as_deref() == Some(matched.handle.as_str())
        })));
}

#[test]
fn compact_locate_promotes_numbered_markdown_headings_to_semantic_handles() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Memory\n\n## 9.10 Integration Points\n\nPRISM should enrich memory recall with lineage and prior outcomes.\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn memory_recall() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: Some(true),
            },
        )
        .expect("locate should succeed");

    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);
    assert_eq!(
        locate
            .top_preview
            .as_ref()
            .map(|preview| preview.start_line),
        Some(3)
    );
}

#[test]
fn compact_fragment_followups_surface_semantic_config_targets() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("crates/member-a")).unwrap();
    fs::create_dir_all(root.join("crates/member-b")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/prism\"]\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/member-a/Cargo.toml"),
        "[package]\nname = \"member-a\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/member-b/Cargo.toml"),
        "[package]\nname = \"member-b\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1.0\"\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| !targets.is_empty()));
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().all(|target| {
            target.kind == NodeKind::TomlKey
                && !target.path.contains("member_a")
                && !target.path.contains("member_b")
                && !target.path.contains("crates/")
                && target
                    .file_path
                    .as_deref()
                    .is_none_or(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(gather.matches[0]
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets
            .iter()
            .any(|target| target.path.contains("::workspace"))));
    assert!(gather.matches[0]
        .next_action
        .as_deref()
        .is_some_and(|next| next.contains("strongest semantic related handle")));
    assert!(gather.matches[0]
        .next_action
        .as_deref()
        .is_some_and(|next| next.contains("prism_open on it")));
    let promoted_handle = gather.matches[0]
        .promoted_handle
        .as_ref()
        .expect("semantic gather should lift a promoted handle");
    assert_eq!(promoted_handle.kind, NodeKind::TomlKey);
    assert!(promoted_handle.path.contains("::workspace::dependencies"));
    assert!(gather.matches[0].suggested_actions.iter().any(|action| {
        action.tool == "prism_workset"
            && action.handle.as_deref() == Some(promoted_handle.handle.as_str())
    }));
    assert!(gather.matches[0].suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(promoted_handle.handle.as_str())
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    let handle = gather.matches[0].handle.clone();

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(!workset.supporting_reads.is_empty());
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.kind == NodeKind::TomlKey));
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("::workspace")));
    assert!(workset.supporting_reads.iter().all(|target| {
        !target.path.contains("member_a")
            && !target.path.contains("member_b")
            && !target.path.contains("crates/")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path.ends_with("Cargo.toml"))
    }));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(workset.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let neighbors = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("neighbors should succeed");
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "TomlKey")));
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| item["filePath"] == "Cargo.toml")));
    assert!(neighbors.top_preview.is_some());
    let first_neighbor_handle = neighbors.result["neighbors"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["handle"].as_str())
        .expect("neighbors should include a top handle");
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(first_neighbor_handle)
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation should succeed");
    assert!(validation.result["checks"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    let first_next_read = validation.result["nextReads"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["handle"].as_str())
        .expect("validation should include nextReads");
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_open"
            && action.handle.as_deref() == Some(first_next_read)
            && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Neighbors)
    }));
}

#[test]
fn compact_gather_prefers_root_workspace_dependencies_with_many_child_manifests() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/member-0\"]\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    for index in 0..12 {
        let crate_dir = root.join(format!("crates/member-{index}"));
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"member-{index}\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n"
            ),
        )
        .unwrap();
    }

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");

    let related = gather.matches[0]
        .related_handles
        .as_ref()
        .expect("related handles");
    assert!(related.iter().any(|target| {
        target.kind == NodeKind::TomlKey
            && target.path.contains("::workspace::dependencies")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path == "Cargo.toml")
    }));
    assert!(related.iter().all(|target| {
        !target.path.contains("member_")
            && !target.path.contains("member-")
            && !target.path.contains("crates/")
    }));
}

#[test]
fn compact_structured_config_handles_prefer_same_file_family_over_tests() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/member-0\"]\nresolver = \"2\"\n\n[workspace.package]\nedition = \"2021\"\n\n[workspace.dependencies]\nanyhow = \"1.0\"\nserde = \"1.0\"\n",
    )
    .unwrap();
    for index in 0..4 {
        let crate_dir = root.join(format!("crates/member-{index}"));
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"member-{index}\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n"
            ),
        )
        .unwrap();
    }

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    let gather = host
        .compact_gather(
            Arc::clone(&session),
            PrismGatherArgs {
                query: "workspace.dependencies".to_string(),
                path: Some("Cargo.toml".to_string()),
                glob: None,
                limit: Some(1),
            },
        )
        .expect("gather should succeed");
    let semantic_handle = gather.matches[0]
        .related_handles
        .as_ref()
        .and_then(|targets| {
            targets
                .iter()
                .find(|target| target.path.contains("::workspace::dependencies"))
        })
        .map(|target| target.handle.clone())
        .expect("semantic handle");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(semantic_handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.supporting_reads.iter().all(|target| {
        target.kind == NodeKind::TomlKey
            && !target.path.contains("tests::")
            && target
                .file_path
                .as_deref()
                .is_none_or(|path| path.ends_with("Cargo.toml"))
    }));
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("::workspace")));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("validation")));

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(semantic_handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");
    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| !targets.is_empty()));
    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().all(|target| {
            target.kind == NodeKind::TomlKey
                && !target.path.contains("tests::")
                && target
                    .file_path
                    .as_deref()
                    .is_none_or(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(open.text.contains("[workspace]"));
    assert!(open.text.contains("[workspace.dependencies]"));
    assert!(open.text.contains("anyhow = \"1.0\""));
    assert!(open.text.contains("serde = \"1.0\""));
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("validation") && text.contains("neighbors")));
    assert!(open.promoted_handle.is_none());
    assert!(open.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(open.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let neighbors = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: semantic_handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("neighbors should succeed");
    assert!(neighbors.result["neighbors"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            item["kind"] == "TomlKey"
                && item["path"]
                    .as_str()
                    .is_some_and(|path| !path.contains("tests::"))
                && item["filePath"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("Cargo.toml"))
        })));
    let preview = neighbors
        .top_preview
        .expect("structured config neighbors should include a top preview");
    let first_neighbor = neighbors.result["neighbors"]
        .as_array()
        .and_then(|items| items.first())
        .expect("neighbors should contain at least one item");
    assert_eq!(
        preview.handle,
        first_neighbor["handle"].as_str().unwrap_or_default()
    );
    assert!(preview.text.contains("[workspace]"));
    assert!(preview.text.contains("[workspace.dependencies]"));
    assert!(neighbors
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("validation")));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(neighbors.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Validation)
    }));

    let validation = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: semantic_handle.clone(),
                kind: PrismExpandKindInput::Validation,
                include_top_preview: None,
            },
        )
        .expect("validation should succeed");
    assert!(validation.result["nextReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(validation.result["likelyTests"]
        .as_array()
        .is_some_and(|items| items.is_empty()));
    assert!(validation.result["checks"]
        .as_array()
        .is_some_and(|items| items.len() >= 2));
    assert!(validation.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            item["kind"] == "TomlKey"
                && item["path"]
                    .as_str()
                    .is_some_and(|path| !path.contains("tests::"))
                && item["filePath"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("Cargo.toml"))
        })));
    assert!(validation
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("neighbors")));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_open" && action.open_mode == Some(prism_js::AgentOpenMode::Focus)
    }));
    assert!(validation.suggested_actions.iter().any(|action| {
        action.tool == "prism_expand"
            && action.handle.as_deref() == Some(semantic_handle.as_str())
            && action.expand_kind == Some(prism_js::AgentExpandKind::Neighbors)
    }));
}

#[test]
fn compact_workset_query_prefers_strong_concept_resolution_for_broad_subsystems() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn session_memory() {}
pub fn outcome_memory() {}
pub fn memory_system_test() {}
"#,
    )
    .unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://memory_system".to_string()),
            canonical_name: Some("memory system".to_string()),
            summary: Some(
                "Session memory recall and outcome history form the repo memory subsystem."
                    .to_string(),
            ),
            aliases: Some(vec![
                "memory layer".to_string(),
                "recall system".to_string(),
            ]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::session_memory".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::outcome_memory".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::memory_system_test".to_string(),
                kind: "function".to_string(),
            }]),
            evidence: Some(vec![
                "Promoted from repeated memory-system dogfooding.".to_string()
            ]),
            risk_hint: None,
            decode_lenses: None,
            scope: None,
            confidence: Some(0.94),
            task_id: None,
            supersedes: None,
            retirement_reason: None,
        },
    )
    .expect("concept store should succeed");

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: None,
                query: Some("memory system".to_string()),
            },
        )
        .expect("workset should succeed");

    assert_eq!(workset.primary.path, "demo::session_memory");
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path == "demo::outcome_memory"));
    assert!(workset.remapped);
    assert!(workset
        .why
        .contains("Session memory recall and outcome history"));
}

#[test]
fn compact_expand_drift_surfaces_spec_gap_summary() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Drift);
    let drift_reasons = expand.result["driftReasons"]
        .as_array()
        .expect("driftReasons should be an array");
    assert!(!drift_reasons.is_empty());
    let next_reads = expand.result["nextReads"]
        .as_array()
        .expect("nextReads should be an array");
    assert!(!next_reads.is_empty());
    assert!(next_reads.iter().all(|item| item["filePath"].is_null()));
    assert!(matches!(
        expand.result["confidence"].as_str(),
        Some("medium" | "high")
    ));
}

#[test]
fn compact_expand_neighbors_can_include_top_preview() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Neighbors,
                include_top_preview: Some(true),
            },
        )
        .expect("expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Neighbors);
    let neighbors = expand.result["neighbors"]
        .as_array()
        .expect("neighbors should be an array");
    assert!(!neighbors.is_empty());
    let preview = expand
        .top_preview
        .expect("neighbors expand should include top preview");
    assert_eq!(
        preview.handle,
        neighbors[0]["handle"].as_str().unwrap_or_default()
    );
    assert!(!preview.text.is_empty());
}

#[test]
fn compact_tool_query_trace_records_refresh_and_handler_phases() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.compact_locate(
        Arc::clone(&session),
        PrismLocateArgs {
            query: "Integration Points".to_string(),
            path: Some("docs/SPEC.md".to_string()),
            glob: None,
            task_intent: Some(PrismLocateTaskIntentInput::Explain),
            limit: Some(3),
            include_top_preview: None,
        },
    )
    .expect("locate should succeed");

    let recent = host.query_log_entries(QueryLogArgs {
        limit: Some(5),
        since: None,
        target: None,
        operation: None,
        task_id: None,
        min_duration_ms: None,
    });
    let entry = recent
        .iter()
        .find(|entry| entry.kind == "prism_locate")
        .expect("compact locate query log entry");
    let trace = host
        .query_trace_view(&entry.id)
        .expect("compact locate query trace");
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"compact.refreshWorkspace"));
    assert!(operations.contains(&"compact.handler"));
    let refresh_args = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "compact.refreshWorkspace")
        .and_then(|phase| phase.args_summary.as_ref())
        .and_then(Value::as_object)
        .expect("compact refresh args");
    let refresh_metrics = refresh_args
        .get("metrics")
        .and_then(Value::as_object)
        .expect("compact refresh metrics");
    for key in [
        "lockWaitMs",
        "lockHoldMs",
        "fsRefreshMs",
        "snapshotRevisionsMs",
        "loadEpisodicMs",
        "loadInferenceMs",
        "loadCoordinationMs",
        "reloadWork",
    ] {
        assert!(
            refresh_metrics.contains_key(key),
            "expected compact refresh args to include `{key}`"
        );
    }
    let reload_work = refresh_metrics
        .get("reloadWork")
        .and_then(Value::as_object)
        .expect("compact refresh reload-work metrics");
    for key in [
        "loadedBytes",
        "replayVolume",
        "fullRebuildCount",
        "workspaceReloaded",
    ] {
        assert!(
            reload_work.contains_key(key),
            "expected compact refresh reload-work metrics to include `{key}`"
        );
    }
    assert!(trace
        .phases
        .iter()
        .find(|phase| phase.operation == "compact.handler")
        .is_some_and(|phase| phase.success));
}

#[test]
fn compact_expand_lineage_returns_compact_recent_history() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    fs::write(&source_path, "pub fn latest_name() {}\n").unwrap();

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let current_id = NodeId::new("demo", "demo::latest_name", NodeKind::Function);
    graph.add_node(Node {
        id: current_id.clone(),
        name: "latest_name".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    let rename_chain = [
        "demo::old_name",
        "demo::legacy_name",
        "demo::older_name",
        "demo::previous_name",
        "demo::latest_name",
    ];
    history.seed_nodes([NodeId::new("demo", rename_chain[0], NodeKind::Function)]);
    for (index, names) in rename_chain.windows(2).enumerate() {
        history.apply(&ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new(format!("change:rename:{index}")),
                ts: (index + 1) as u64,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: ChangeTrigger::ManualReindex,
            files: vec![file_id],
            previous_path: Some(source_path.to_string_lossy().into_owned().into()),
            current_path: Some(source_path.to_string_lossy().into_owned().into()),
            added: vec![ObservedNode {
                node: Node {
                    id: NodeId::new("demo", names[1], NodeKind::Function),
                    name: names[1].rsplit("::").next().unwrap().into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
            }],
            removed: vec![ObservedNode {
                node: Node {
                    id: NodeId::new("demo", names[0], NodeKind::Function),
                    name: names[0].rsplit("::").next().unwrap().into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(10, Some(20), Some(20), None),
            }],
            updated: Vec::new(),
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        });
    }

    let host = host_with_prism(Prism::with_history(graph, history));
    let session = test_session(&host);
    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: current_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&current_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "latest_name".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("latest_name".into()),
        why_short: "exact symbol".into(),
        start_line: Some(1),
        end_line: Some(1),
        start_column: None,
        end_column: None,
    });

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Lineage,
                include_top_preview: None,
            },
        )
        .expect("lineage expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Lineage);
    assert_eq!(expand.result["currentPath"], "demo::latest_name");
    assert_eq!(expand.result["status"], "active");
    assert!(expand.result.get("history").is_none());
    let recent_history = expand.result["recentHistory"]
        .as_array()
        .expect("recentHistory should be an array");
    assert_eq!(recent_history.len(), 3);
    assert!(expand.result["truncated"].as_bool().unwrap_or(false));
    assert!(expand.result["nextAction"]
        .as_str()
        .is_some_and(|text| text.contains("prism_query")));
    assert!(recent_history.iter().all(|event| {
        event.get("before").is_none()
            && event.get("after").is_none()
            && event.get("evidenceDetails").is_none()
            && event["summary"].as_str().is_some()
            && event["evidence"]
                .as_array()
                .is_some_and(|evidence| evidence.len() <= 2)
    }));
}

#[test]
fn compact_expand_diff_returns_compact_recent_patch_summaries() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn alpha() {}\n";
    fs::write(&source_path, source).unwrap();
    let alpha_span = {
        let start = source.find("alpha").expect("alpha span");
        Span::new(start, start + "alpha".len())
    };

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: alpha_span,
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone()]);

    let outcomes = OutcomeMemory::new();
    for index in 0..4 {
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(format!("outcome:patch:{index}")),
                    ts: (index + 1) as u64,
                    actor: EventActor::System,
                    correlation: None,
                    causation: None,
                },
                anchors: vec![AnchorRef::File(file_id), AnchorRef::Node(alpha_id.clone())],
                kind: OutcomeKind::PatchApplied,
                result: OutcomeResult::Success,
                summary: format!("patched alpha {index}"),
                evidence: Vec::new(),
                metadata: json!({
                    "trigger": "ManualReindex",
                    "filePaths": [source_path.to_string_lossy().into_owned()],
                    "changedSymbols": [
                        {
                            "status": "updated_after",
                            "id": alpha_id,
                            "name": "alpha",
                            "kind": NodeKind::Function,
                            "filePath": source_path.to_string_lossy().into_owned(),
                            "span": alpha_span,
                        }
                    ],
                }),
            })
            .unwrap();
    }

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let session = test_session(&host);
    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: alpha_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&alpha_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "alpha".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("alpha".into()),
        why_short: "exact symbol".into(),
        start_line: Some(1),
        end_line: Some(1),
        start_column: None,
        end_column: None,
    });

    let expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Diff,
                include_top_preview: None,
            },
        )
        .expect("diff expand should succeed");

    assert_eq!(expand.kind, prism_js::AgentExpandKind::Diff);
    assert!(expand.result.get("diff").is_none());
    let recent_diffs = expand.result["recentDiffs"]
        .as_array()
        .expect("recentDiffs should be an array");
    assert_eq!(recent_diffs.len(), 3);
    assert!(expand.result["truncated"].as_bool().unwrap_or(false));
    assert!(expand.result["nextAction"]
        .as_str()
        .is_some_and(|text| text.contains("prism_query")));
    assert!(recent_diffs.iter().all(|diff| {
        diff["symbolPath"] == "demo::alpha"
            && diff["summary"]
                .as_str()
                .is_some_and(|summary| summary.contains("patched alpha"))
            && diff["filePath"]
                .as_str()
                .is_some_and(|path| path.ends_with("src/lib.rs"))
    }));
}

#[test]
fn compact_expand_perception_lenses_surface_impact_timeline_and_memory() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn beta() {}\n\npub fn alpha() { beta(); }\n";
    fs::write(&source_path, source).unwrap();
    let test_path = root.join("tests/alpha.rs");
    fs::write(&test_path, "#[test]\nfn alpha_test() { super::alpha(); }\n").unwrap();

    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };
    let alpha_start = source.rfind("alpha").expect("alpha span");
    let alpha_span = Span::new(alpha_start, alpha_start + "alpha".len());

    let mut graph = Graph::new();
    let source_file = graph.ensure_file(&source_path);
    let test_file = graph.ensure_file(&test_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta_id = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_test_id = NodeId::new("demo", "demo::alpha_test", NodeKind::Function);
    graph.add_node(Node {
        id: beta_id.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: beta_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: alpha_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_test_id.clone(),
        name: "alpha_test".into(),
        kind: NodeKind::Function,
        file: test_file,
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha_id.clone(),
        target: beta_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Validates,
        source: alpha_test_id.clone(),
        target: alpha_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone(), beta_id.clone(), alpha_test_id.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:failure"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha regression".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:patch"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![
                AnchorRef::File(source_file),
                AnchorRef::Node(alpha_id.clone()),
            ],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "patched alpha".into(),
            evidence: Vec::new(),
            metadata: json!({
                "trigger": "ManualReindex",
                "filePaths": [source_path.to_string_lossy().into_owned()],
                "changedSymbols": [{
                    "status": "updated_after",
                    "id": alpha_id.clone(),
                    "name": "alpha",
                    "kind": NodeKind::Function,
                    "filePath": source_path.to_string_lossy().into_owned(),
                    "span": alpha_span,
                }],
            }),
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:validated"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated alpha".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let session = test_session(&host);
    let mut memory = MemoryEntry::new(
        MemoryKind::Structural,
        "alpha edits usually require checking beta and alpha_test together",
    );
    memory.anchors = vec![AnchorRef::Node(alpha_id.clone())];
    memory.trust = 0.9;
    session.notes.store(memory).unwrap();

    let handle = session.intern_target_handle(crate::session_state::SessionHandleTarget {
        id: alpha_id.clone(),
        lineage_id: host
            .current_prism()
            .lineage_of(&alpha_id)
            .map(|lineage| lineage.0.to_string()),
        handle_category: crate::session_state::SessionHandleCategory::Symbol,
        name: "alpha".into(),
        kind: NodeKind::Function,
        file_path: Some(source_path.to_string_lossy().into_owned()),
        query: Some("alpha".into()),
        why_short: "exact symbol".into(),
        start_line: Some(3),
        end_line: Some(3),
        start_column: None,
        end_column: None,
    });

    let impact = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Impact,
                include_top_preview: None,
            },
        )
        .expect("impact expand should succeed");
    assert_eq!(impact.kind, prism_js::AgentExpandKind::Impact);
    assert!(impact.result["likelyTouch"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["path"] == "demo::beta")));
    assert!(impact.result["recentFailures"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(impact.result["riskHint"].as_str().is_some());

    let timeline = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: handle.clone(),
                kind: PrismExpandKindInput::Timeline,
                include_top_preview: None,
            },
        )
        .expect("timeline expand should succeed");
    assert_eq!(timeline.kind, prism_js::AgentExpandKind::Timeline);
    assert!(timeline.result["recentEvents"]
        .as_array()
        .is_some_and(|items| items.len() >= 2));
    assert!(timeline.result["recentPatches"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert_eq!(
        timeline.result["lastFailure"]["summary"],
        "alpha regression"
    );
    assert_eq!(
        timeline.result["lastValidation"]["summary"],
        "validated alpha"
    );

    let memory_expand = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle,
                kind: PrismExpandKindInput::Memory,
                include_top_preview: None,
            },
        )
        .expect("memory expand should succeed");
    assert_eq!(memory_expand.kind, prism_js::AgentExpandKind::Memory);
    assert!(memory_expand.result["memories"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["summary"]
            == "alpha edits usually require checking beta and alpha_test together")));
}

#[test]
fn compact_task_brief_summarizes_coordination_outcomes_and_next_reads() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn gamma() {}\n\npub fn beta() {}\n\npub fn alpha() { beta(); }\n";
    fs::write(&source_path, source).unwrap();
    let gamma_span = {
        let start = source.find("gamma").expect("gamma span");
        Span::new(start, start + "gamma".len())
    };
    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };
    let alpha_start = source.rfind("alpha").expect("alpha span");
    let alpha_span = Span::new(alpha_start, alpha_start + "alpha".len());

    let mut graph = Graph::new();
    let source_file = graph.ensure_file(&source_path);
    let gamma_id = NodeId::new("demo", "demo::gamma", NodeKind::Function);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta_id = NodeId::new("demo", "demo::beta", NodeKind::Function);
    graph.add_node(Node {
        id: gamma_id.clone(),
        name: "gamma".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: gamma_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta_id.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: beta_span,
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: alpha_span,
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha_id.clone(),
        target: beta_id.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone(), beta_id.clone(), gamma_id.clone()]);
    let host = host_with_prism(Prism::with_history(graph, history));

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Coordinate alpha",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let dependency = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Review gamma",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::gamma",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let dependency_id = dependency.state["id"].as_str().unwrap().to_string();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id.clone(),
                    "title": "Edit alpha",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "dependsOn": [dependency_id]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    host.store_claim(
        test_session(&host).as_ref(),
        PrismClaimArgs {
            action: ClaimActionInput::Acquire,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "capability": "Edit",
                "mode": "SoftExclusive",
                "coordinationTaskId": task_id.clone()
            }),
            task_id: None,
        },
    )
    .unwrap();
    host.store_artifact(
        test_session(&host).as_ref(),
        PrismArtifactArgs {
            action: ArtifactActionInput::Propose,
            payload: json!({
                "taskId": task_id.clone(),
                "diffRef": "patch:alpha"
            }),
            task_id: None,
        },
    )
    .unwrap();
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated alpha".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: Some(task_id.clone()),
        },
    )
    .unwrap();

    let brief = host
        .compact_task_brief(
            test_session(&host),
            PrismTaskBriefArgs {
                task_id: task_id.clone(),
            },
        )
        .expect("task brief should succeed");

    assert_eq!(brief.task_id, task_id);
    assert_eq!(brief.title, "Edit alpha");
    assert!(!brief.blockers.is_empty());
    assert_eq!(brief.claim_holders.len(), 1);
    assert!(brief
        .recent_outcomes
        .iter()
        .any(|event| event.summary == "validated alpha"));
    assert!(brief
        .next_reads
        .iter()
        .any(|target| target.path == "demo::beta"));
    assert!(brief
        .next_reads
        .iter()
        .any(|target| target.path == "demo::gamma"));
    assert_eq!(brief.next_reads[0].path, "demo::gamma");
    assert!(brief.next_action.as_deref().is_some_and(|value| {
        value.contains("current task blockers") && value.contains("prism.blockers(taskId)")
    }));
}

#[test]
fn compact_task_brief_prefers_refresh_for_stale_current_task() {
    let mut graph = Graph::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:task-brief-stale"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![(
            ObservedNode {
                node: Node {
                    id: alpha.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
            ObservedNode {
                node: Node {
                    id: alpha.clone(),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:task-brief-stale"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Refresh stale task".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:task-brief-stale"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    prism.replace_coordination_snapshot(coordination.snapshot());
    let host = host_with_prism(prism);

    let brief = host
        .compact_task_brief(
            test_session(&host),
            PrismTaskBriefArgs {
                task_id: task_id.0.to_string(),
            },
        )
        .expect("task brief should succeed");

    assert!(brief
        .blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision));
    assert!(brief
        .next_action
        .as_deref()
        .is_some_and(|value| value.contains("Refresh this task")));
}

#[test]
fn compact_task_brief_accepts_native_plan_node_current_task_ids() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Track a native milestone node",
                    "policy": { "requireValidationForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let required_test =
        "test:cargo test -p prism-js api_reference_mentions_primary_tool -- --nocapture";
    let required_build = "build:cargo build --release -p prism-cli -p prism-mcp";
    let node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({
                    "planId": plan_id,
                    "kind": "validate",
                    "title": "Validate migration milestone",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "acceptance": [{
                        "label": "migration is validated",
                        "requiredChecks": [
                            { "id": required_test },
                            { "id": required_build }
                        ],
                        "evidencePolicy": "validation-only"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let node_id = node.state["id"].as_str().unwrap().to_string();

    host.configure_session(
        test_session(&host).as_ref(),
        PrismConfigureSessionArgs {
            limits: None,
            current_task_id: Some(node_id.clone()),
            coordination_task_id: None,
            current_task_description: Some("Validate migration milestone".to_string()),
            current_task_tags: Some(vec!["milestone".to_string()]),
            clear_current_task: None,
            current_agent: None,
            clear_current_agent: None,
        },
    )
    .unwrap();
    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::NoteAdded,
            anchors: Vec::new(),
            summary: "Started milestone validation".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: None,
        },
    )
    .unwrap();

    let brief = host
        .compact_task_brief(
            test_session(&host),
            PrismTaskBriefArgs {
                task_id: node_id.clone(),
            },
        )
        .expect("native current-task plan node should resolve in task brief");

    assert_eq!(brief.task_id, node_id);
    assert_eq!(brief.title, "Validate migration milestone");
    assert_eq!(brief.status, prism_ir::CoordinationTaskStatus::Ready);
    assert!(brief
        .recent_outcomes
        .iter()
        .any(|event| event.summary == "Started milestone validation"));
    assert!(brief.next_action.is_some());
}

#[test]
fn compact_workset_for_spec_targets_surfaces_drift_reads_and_gap_summary() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Integration Points".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.status, prism_js::AgentLocateStatus::Ok);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset.supporting_reads.iter().any(|target| {
        target.path.contains("memory_recall")
            || target.path.contains("reanchor_persisted_memory_snapshot")
    }));
    assert!(workset.why.contains("Gap summary:") || workset.why.contains("gap summary"));
    assert!(workset
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("drift")));
}

#[test]
fn compact_workset_for_spec_targets_prefers_owner_paths_over_text_adjacent_helpers() {
    let root = temp_workspace();
    write_dashboard_validation_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Validation view".to_string(),
                path: Some("docs/DASHBOARD_IMPLEMENTATION_SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");

    assert!(!workset.supporting_reads.is_empty());
    assert!(workset
        .supporting_reads
        .iter()
        .any(|target| target.path.contains("validation_feedback_view")
            || target.path.contains("store_validation_feedback")));
    assert!(workset.supporting_reads.iter().all(|target| {
        !target
            .path
            .contains("strip_internal_developer_api_reference")
    }));

    let drift = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("drift should succeed");
    assert!(drift.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().all(|item| {
            !item["path"]
                .as_str()
                .unwrap_or_default()
                .contains("strip_internal_developer_api_reference")
        })));
    assert!(drift
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_open") && text.contains("prism_workset")));
}

#[test]
fn compact_workset_for_product_surface_spec_headings_lifts_body_identifiers() {
    let root = temp_workspace();
    write_compact_default_tools_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Compact Default Tools".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");
    assert_eq!(locate.candidates[0].kind, NodeKind::MarkdownHeading);

    let workset = host
        .compact_workset(
            Arc::clone(&session),
            PrismWorksetArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                query: None,
            },
        )
        .expect("workset should succeed");
    assert!(workset.supporting_reads.iter().any(|target| {
        target.path.contains("prism_locate")
            || target.path.contains("prism_open")
            || target.path.contains("prism_workset")
            || target.path.contains("prism_expand")
    }));
    assert!(workset
        .supporting_reads
        .iter()
        .all(|target| !target.path.contains("tests::")));

    let drift = host
        .compact_expand(
            Arc::clone(&session),
            PrismExpandArgs {
                handle: locate.candidates[0].handle.clone(),
                kind: PrismExpandKindInput::Drift,
                include_top_preview: None,
            },
        )
        .expect("drift should succeed");
    assert!(drift.result["nextReads"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| {
            let path = item["path"].as_str().unwrap_or_default();
            path.contains("prism_locate")
                || path.contains("prism_open")
                || path.contains("prism_workset")
                || path.contains("prism_expand")
        })));
}

#[test]
fn compact_open_for_product_surface_spec_headings_prefers_identifier_owners() {
    let root = temp_workspace();
    write_compact_default_tools_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let locate = host
        .compact_locate(
            Arc::clone(&session),
            PrismLocateArgs {
                query: "Compact Default Tools".to_string(),
                path: Some("docs/SPEC.md".to_string()),
                glob: None,
                task_intent: Some(PrismLocateTaskIntentInput::Explain),
                limit: Some(3),
                include_top_preview: None,
            },
        )
        .expect("locate should succeed");

    let open = host
        .compact_open(
            Arc::clone(&session),
            PrismOpenArgs {
                handle: Some(locate.candidates[0].handle.clone()),
                path: None,
                mode: Some(PrismOpenModeInput::Focus),
                line: None,
                before_lines: None,
                after_lines: None,
                max_chars: None,
            },
        )
        .expect("open should succeed");

    assert!(open
        .related_handles
        .as_ref()
        .is_some_and(|targets| targets.iter().any(|target| {
            target.path.contains("prism_locate")
                || target.path.contains("prism_open")
                || target.path.contains("prism_workset")
                || target.path.contains("prism_expand")
        })));
    assert!(open.related_handles.as_ref().is_some_and(|targets| {
        targets
            .iter()
            .all(|target| !target.path.contains("tests::"))
    }));
    assert!(open
        .next_action
        .as_deref()
        .is_some_and(|text| text.contains("prism_workset") && text.contains("drift")));
}

#[tokio::test]
async fn mcp_server_executes_compact_agent_tool_round_trip() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn main() {
    println!("hello");
}
"#,
    )
    .unwrap();
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_locate",
            json!({
                "query": "main",
                "taskIntent": "edit",
                "limit": 3,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let locate = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(locate["status"], "ok");

    client
        .send(call_tool_request(
            3,
            "prism_open",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "mode": "focus",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let open = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(open["handle"], locate["candidates"][0]["handle"]);
    assert!(open["text"]
        .as_str()
        .expect("open text should be a string")
        .contains("fn main"));
    assert!(open["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            4,
            "prism_workset",
            json!({
                "handle": locate["candidates"][0]["handle"],
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let workset = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(
        workset["primary"]["handle"],
        locate["candidates"][0]["handle"]
    );
    assert!(workset["why"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(workset["truncated"], false);
    assert!(workset["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism_open")));
    assert!(workset["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            5,
            "prism_expand",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "kind": "diagnostics",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let expand = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(expand["kind"], "diagnostics");
    assert_eq!(
        expand["result"]["whyShort"],
        locate["candidates"][0]["whyShort"]
    );
    assert!(expand["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    client
        .send(call_tool_request(
            6,
            "prism_gather",
            json!({
                "query": "println!(\"hello\")",
                "path": "src/lib.rs",
                "limit": 2,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let gather = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(gather["truncated"], false);
    assert_eq!(gather["matches"].as_array().map(Vec::len), Some(1));
    assert_eq!(gather["matches"][0]["filePath"], "src/lib.rs");
    assert!(gather["matches"][0]["text"]
        .as_str()
        .is_some_and(|value| value.contains("println!(\"hello\")")));
    assert!(gather["matches"][0]["suggestedActions"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_executes_prism_task_brief_round_trip() {
    let server = server_with_node(demo_node());
    let plan = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Coordinate main" }),
                task_id: None,
            },
        )
        .expect("plan create should succeed");
    let plan_id = plan.state["id"].as_str().unwrap().to_string();
    let task = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Inspect main",
                    "status": "Ready",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .expect("task create should succeed");
    let task_id = task.state["id"].as_str().unwrap().to_string();
    server
        .host
        .store_outcome(
            test_session(&server.host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                summary: "validated main".to_string(),
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: Some(task_id.clone()),
            },
        )
        .expect("outcome should store");

    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_task_brief",
            json!({
                "taskId": task_id,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let brief = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(brief["title"], "Inspect main");
    assert!(brief["recentOutcomes"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["summary"] == "validated main")));
    assert!(brief["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism_open")));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_keeps_compact_handles_stable_across_parallel_follow_up_calls() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_locate",
            json!({
                "query": "Integration Points",
                "path": "docs/SPEC.md",
                "taskIntent": "explain",
                "limit": 3,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let locate = first_tool_content_json(client.receive().await.unwrap());
    let handle = locate["candidates"][0]["handle"].clone();

    for (id, tool, args) in [
        (
            3,
            "prism_open",
            json!({
                "handle": handle,
                "mode": "focus",
            }),
        ),
        (
            4,
            "prism_workset",
            json!({
                "handle": locate["candidates"][0]["handle"],
            }),
        ),
        (
            5,
            "prism_expand",
            json!({
                "handle": locate["candidates"][0]["handle"],
                "kind": "drift",
            }),
        ),
    ] {
        client
            .send(call_tool_request(
                id,
                tool,
                args.as_object()
                    .expect("tool args should be an object")
                    .clone(),
            ))
            .await
            .unwrap();
    }

    let first = first_tool_content_json(client.receive().await.unwrap());
    let second = first_tool_content_json(client.receive().await.unwrap());
    let third = first_tool_content_json(client.receive().await.unwrap());
    let payloads = [first, second, third];

    assert!(payloads
        .iter()
        .any(|payload| payload["handle"] == locate["candidates"][0]["handle"]));
    assert!(payloads
        .iter()
        .any(|payload| payload["primary"]["handle"] == locate["candidates"][0]["handle"]));
    assert!(payloads
        .iter()
        .any(|payload| payload["kind"] == "drift" && payload["result"]["nextReads"].is_array()));

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_maps_prism_query_user_errors_to_invalid_params_like_compact_tools() {
    let server = server_with_node(demo_node());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_query",
            json!({
                "code": "return prism.runtimeStatus();",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let feature_disabled = response_json(client.receive().await.unwrap());
    assert_eq!(feature_disabled["error"]["code"], -32602);
    assert_eq!(
        feature_disabled["error"]["data"]["code"],
        "query_feature_disabled"
    );
    assert!(feature_disabled["error"]["message"]
        .as_str()
        .is_some_and(|value| value.contains("internal developer queries are disabled")));

    client
        .send(call_tool_request(
            3,
            "prism_query",
            json!({
                "code": "return prism.search(\"main\", { pathMode: \"sideways\" });",
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();
    let invalid_query_args = response_json(client.receive().await.unwrap());
    assert_eq!(invalid_query_args["error"]["code"], -32602);
    assert_eq!(
        invalid_query_args["error"]["data"]["code"],
        "query_invalid_argument"
    );
    assert!(invalid_query_args["error"]["message"]
        .as_str()
        .is_some_and(|value| value.contains("unsupported search pathMode `sideways`")));

    client
        .send(call_tool_request(
            4,
            "prism_open",
            json!({})
                .as_object()
                .expect("tool args should be an object")
                .clone(),
        ))
        .await
        .unwrap();
    let compact_invalid_args = response_json(client.receive().await.unwrap());
    assert_eq!(compact_invalid_args["error"]["code"], -32602);
    assert_eq!(
        compact_invalid_args["error"]["message"],
        "exactly one of `handle` or `path` is required"
    );
    assert_eq!(
        compact_invalid_args["error"]["data"]["fields"],
        json!(["handle", "path"])
    );

    running.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_server_allows_path_based_prism_open_edit_mode_when_line_is_provided() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        concat!(
            "pub fn alpha() {}\n",
            "pub fn beta() {\n",
            "    let value = 42;\n",
            "    let doubled = value * 2;\n",
            "    let tripled = doubled + value;\n",
            "    println!(\"{tripled}\");\n",
            "}\n",
        ),
    )
    .unwrap();
    let server = PrismMcpServer::with_session(index_workspace_session(&root).unwrap());
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_open",
            json!({
                "path": "src/lib.rs",
                "mode": "edit",
                "line": 4,
            })
            .as_object()
            .expect("tool args should be an object")
            .clone(),
        ))
        .await
        .unwrap();

    let payload = first_tool_content_json(client.receive().await.unwrap());
    assert!(payload["filePath"]
        .as_str()
        .is_some_and(|path| path.ends_with("/src/lib.rs")));
    assert_eq!(payload["startLine"], 2);
    assert_eq!(payload["endLine"], 7);
    assert!(payload["text"]
        .as_str()
        .is_some_and(|text| text.contains("pub fn beta() {")));
    assert!(payload["text"]
        .as_str()
        .is_some_and(|text| text.contains("println!(\"{tripled}\");")));

    running.cancel().await.unwrap();
}

#[test]
fn bundle_helpers_collapse_search_and_target_context_into_one_query() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const symbol = prism.symbolBundle("helper", { includeDiscovery: true });
const search = prism.searchBundle("helper", { limit: 5 });
const target = prism.targetBundle(search);
const richSearch = prism.searchBundle("helper", { limit: 5, includeDiscovery: true });
const richTarget = prism.targetBundle(richSearch, { includeDiscovery: true });
return {
  symbol: {
    query: symbol?.query ?? null,
    resultPath: symbol?.result?.id?.path ?? null,
    candidateCount: symbol?.candidates?.length ?? 0,
    discoveryPath: symbol?.discovery?.target?.id?.path ?? null,
    readContextPath: symbol?.readContext?.target?.id?.path ?? null,
    suggestedReadsCount: symbol?.suggestedReads?.length ?? 0,
    summary: symbol?.summary ?? null,
  },
  search: {
    query: search.query,
    resultCount: search.results.length,
    topResultPath: search.topResult?.id?.path ?? null,
    focusedPath: search.focusedBlock?.symbol?.id?.path ?? null,
    readContextPath: search.readContext?.target?.id?.path ?? null,
    validationPath: search.validationContext?.target?.id?.path ?? null,
    recentChangePath: search.recentChangeContext?.target?.id?.path ?? null,
    hasDiscovery: search.discovery != null,
    suggestedReadsCount: search.suggestedReads.length,
    summary: search.summary,
    diagnosticCodes: search.diagnostics.map((diagnostic) => diagnostic.code),
  },
  target: target == null ? null : {
    targetPath: target.target.id.path,
    editContextPath: target.editContext.target.id.path,
    readContextPath: target.readContext.target.id.path,
    hasDiscovery: target.discovery != null,
    suggestedReadsCount: target.suggestedReads.length,
    diffCount: target.diff.length,
    likelyTestsCount: target.likelyTests.length,
    summary: target.summary,
  },
  richSearch: {
    topResultPath: richSearch.topResult?.id?.path ?? null,
    discoveryPath: richSearch.discovery?.target?.id?.path ?? null,
    readContextPath: richSearch.readContext?.target?.id?.path ?? null,
    suggestedReadsCount: richSearch.suggestedReads.length,
    summary: richSearch.summary,
  },
  richTarget: richTarget == null ? null : {
    targetPath: richTarget.target.id.path,
    discoveryPath: richTarget.discovery?.target?.id?.path ?? null,
    editContextPath: richTarget.editContext.target.id.path,
    readContextPath: richTarget.readContext.target.id.path,
    suggestedReadsCount: richTarget.suggestedReads.length,
    summary: richTarget.summary,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("bundle helpers query should succeed");

    let symbol = &result.result["symbol"];
    assert_eq!(symbol["query"], "helper");
    assert_eq!(symbol["resultPath"], "demo::alpha::helper");
    assert_eq!(symbol["candidateCount"], 2);
    assert_eq!(symbol["discoveryPath"], symbol["resultPath"]);
    assert_eq!(symbol["readContextPath"], symbol["resultPath"]);
    assert!(symbol["suggestedReadsCount"].as_u64().unwrap_or_default() > 0);
    assert_eq!(symbol["summary"]["kind"], "symbol");
    assert_eq!(symbol["summary"]["resultCount"], 2);
    assert_eq!(symbol["summary"]["empty"], false);
    assert_eq!(symbol["summary"]["ambiguous"], true);

    let search = &result.result["search"];
    assert_eq!(search["query"], "helper");
    assert_eq!(search["resultCount"], 2);
    assert_eq!(search["focusedPath"], search["topResultPath"]);
    assert_eq!(search["readContextPath"], search["topResultPath"]);
    assert_eq!(search["validationPath"], search["topResultPath"]);
    assert_eq!(search["recentChangePath"], search["topResultPath"]);
    assert_eq!(search["hasDiscovery"], false);
    assert!(search["suggestedReadsCount"].as_u64().is_some());
    assert_eq!(search["summary"]["kind"], "search");
    assert_eq!(search["summary"]["resultCount"], 2);
    assert_eq!(search["summary"]["ambiguous"], true);
    assert!(search["diagnosticCodes"]
        .as_array()
        .expect("bundle diagnostics")
        .iter()
        .any(|diagnostic| diagnostic == "ambiguous_search"));

    let target = &result.result["target"];
    assert_eq!(target["targetPath"], search["topResultPath"]);
    assert_eq!(target["hasDiscovery"], false);
    assert_eq!(target["editContextPath"], target["targetPath"]);
    assert_eq!(target["readContextPath"], target["targetPath"]);
    assert!(target["suggestedReadsCount"].as_u64().is_some());
    assert_eq!(target["summary"]["kind"], "target");
    assert_eq!(target["summary"]["resultCount"], 1);
    assert!(target["diffCount"].as_u64().is_some());
    assert!(target["likelyTestsCount"].as_u64().is_some());

    let rich_search = &result.result["richSearch"];
    assert_eq!(rich_search["discoveryPath"], rich_search["topResultPath"]);
    assert_eq!(rich_search["readContextPath"], rich_search["topResultPath"]);
    assert!(
        rich_search["suggestedReadsCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(rich_search["summary"]["kind"], "search");

    let rich_target = &result.result["richTarget"];
    assert_eq!(rich_target["targetPath"], rich_search["topResultPath"]);
    assert_eq!(rich_target["discoveryPath"], rich_target["targetPath"]);
    assert_eq!(rich_target["editContextPath"], rich_target["targetPath"]);
    assert_eq!(rich_target["readContextPath"], rich_target["targetPath"]);
    assert!(
        rich_target["suggestedReadsCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
    assert_eq!(rich_target["summary"]["kind"], "target");
}

#[test]
fn restart_restores_persisted_session_seed_for_new_host_sessions() {
    let root = temp_workspace();
    let first = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let started = first
        .start_task(
            test_session(&first).as_ref(),
            Some("Resume milestone one".to_string()),
            vec!["milestone-1".to_string(), "restart".to_string()],
            None,
        )
        .expect("task should start");
    first
        .configure_session_without_refresh(
            test_session(&first).as_ref(),
            PrismConfigureSessionArgs {
                limits: Some(QueryLimitsInput {
                    max_result_nodes: Some(3),
                    max_call_graph_depth: Some(2),
                    max_output_json_bytes: Some(2048),
                }),
                current_task_id: Some(started.0.to_string()),
                coordination_task_id: None,
                current_task_description: Some("Resume milestone one".to_string()),
                current_task_tags: Some(vec!["milestone-1".to_string(), "restart".to_string()]),
                clear_current_task: None,
                current_agent: Some("agent-restart".to_string()),
                clear_current_agent: None,
            },
        )
        .expect("session configure should succeed");

    let restarted = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let restored = restarted
        .session_resource_value(test_session(&restarted).as_ref())
        .expect("restored session should load");

    let current_task = restored.current_task.expect("current task should restore");
    assert_eq!(current_task.task_id, started.0);
    assert_eq!(
        current_task.description.as_deref(),
        Some("Resume milestone one")
    );
    assert_eq!(
        current_task.tags,
        vec!["milestone-1".to_string(), "restart".to_string()]
    );
    assert_eq!(restored.current_agent.as_deref(), Some("agent-restart"));
    assert_eq!(restored.limits.max_result_nodes, 3);
    assert_eq!(restored.limits.max_call_graph_depth, 2);
    assert_eq!(restored.limits.max_output_json_bytes, 2048);
}

#[test]
fn bundle_helpers_keep_diagnostics_local_to_each_helper_call() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const broad = prism.searchBundle("helper", { limit: 5 });
const exact = prism.symbolBundle("core");
const text = prism.textSearchBundle("core", {
  path: "src/alpha.rs",
  semanticLimit: 2,
  aroundBefore: 1,
  aroundAfter: 2,
});
return {
  broad: {
    diagnosticCodes: broad.diagnostics.map((diagnostic) => diagnostic.code),
    summary: broad.summary,
  },
  exact: {
    resultPath: exact.result?.id?.path ?? null,
    diagnosticCodes: exact.diagnostics.map((diagnostic) => diagnostic.code),
    summary: exact.summary,
  },
  text: {
    topMatchPath: text.topMatch?.path ?? null,
    diagnosticCodes: text.diagnostics.map((diagnostic) => diagnostic.code),
    summary: text.summary,
  },
};
"#,
            QueryLanguage::Ts,
        )
        .expect("bundle-local diagnostics query should succeed");

    let broad = &result.result["broad"];
    assert!(broad["diagnosticCodes"]
        .as_array()
        .expect("broad diagnostics")
        .iter()
        .any(|diagnostic| diagnostic == "ambiguous_search"));
    assert_eq!(broad["summary"]["ambiguous"], true);

    let exact = &result.result["exact"];
    assert_eq!(exact["resultPath"], "demo::alpha::core");
    assert_eq!(exact["summary"]["kind"], "symbol");
    assert_eq!(exact["summary"]["resultCount"], 1);
    assert_eq!(exact["summary"]["ambiguous"], false);
    assert_eq!(exact["summary"]["truncated"], false);
    assert_eq!(exact["diagnosticCodes"], json!([]));
    assert_eq!(exact["summary"]["diagnosticCodes"], json!([]));

    let text = &result.result["text"];
    assert_eq!(text["topMatchPath"], "src/alpha.rs");
    assert_eq!(text["summary"]["kind"], "text_search");
    assert_eq!(text["summary"]["ambiguous"], false);
    assert_eq!(text["summary"]["truncated"], false);
    assert_eq!(text["diagnosticCodes"], json!([]));
    assert_eq!(text["summary"]["diagnosticCodes"], json!([]));
}

#[test]
fn text_search_bundle_collapses_raw_match_and_semantic_context_into_one_query() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/alpha.rs"),
        r#"
pub fn helper() {
    core();
}

pub fn core() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const bundle = prism.textSearchBundle("helper", {
  path: "src/alpha.rs",
  semanticLimit: 3,
  aroundBefore: 1,
  aroundAfter: 4,
});
const regexBundle = prism.textSearchBundle("helper\\(", {
  regex: true,
  path: "src/alpha.rs",
  semanticQuery: "helper",
  semanticLimit: 3,
  includeDiscovery: true,
});
return { bundle, regexBundle };
"#,
            QueryLanguage::Ts,
        )
        .expect("text search bundle query should succeed");

    let bundle = &result.result["bundle"];
    assert_eq!(bundle["query"], "helper");
    assert_eq!(bundle["topMatch"]["path"], "src/alpha.rs");
    assert_eq!(
        bundle["rawContext"]["focus"]["startLine"],
        bundle["topMatch"]["location"]["startLine"]
    );
    assert_eq!(bundle["semanticQuery"], "helper");
    assert_eq!(
        bundle["focusedBlock"]["symbol"]["id"]["path"],
        bundle["topSymbol"]["id"]["path"]
    );
    assert_eq!(
        bundle["readContext"]["target"]["id"]["path"],
        bundle["topSymbol"]["id"]["path"]
    );
    assert!(bundle["suggestedReads"].is_array());
    assert_eq!(bundle["summary"]["kind"], "text_search");
    assert_eq!(bundle["summary"]["resultCount"], 1);
    assert!(bundle["discovery"].is_null());
    assert!(bundle["diagnostics"].is_array());

    let regex_bundle = &result.result["regexBundle"];
    assert_eq!(regex_bundle["query"], "helper\\(");
    assert_eq!(regex_bundle["semanticQuery"], "helper");
    assert_eq!(
        regex_bundle["discovery"]["target"]["id"]["path"],
        regex_bundle["topSymbol"]["id"]["path"]
    );
    assert_eq!(
        regex_bundle["readContext"]["target"]["id"]["path"],
        regex_bundle["topSymbol"]["id"]["path"]
    );
    assert!(regex_bundle["suggestedReads"].is_array());
    assert_eq!(regex_bundle["summary"]["kind"], "text_search");
}

#[test]
fn lineage_targets_remap_stale_symbol_ids_to_current_edit_slices() {
    let root = temp_workspace();
    let source = "pub fn alpha_v2() { beta(); }\npub fn beta() {}\n";
    fs::write(root.join("src/lib.rs"), source).unwrap();

    let alpha_old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let alpha_new = NodeId::new("demo", "demo::alpha_v2", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let alpha_start = source.find("pub fn alpha_v2").expect("alpha_v2 source");
    let alpha_end = source.find('\n').expect("alpha_v2 line end");
    let beta_start = source.find("pub fn beta").expect("beta source");
    let beta_end = source[source.len() - 1..]
        .find('\n')
        .map(|index| source.len() - 1 + index)
        .unwrap_or(source.len());

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&root.join("src/lib.rs"));
    graph.add_node(Node {
        id: alpha_new.clone(),
        name: "alpha_v2".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::new(alpha_start, alpha_end),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: Span::new(beta_start, beta_end),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_old.clone(), beta.clone()]);
    history.apply(&prism_ir::ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:rename-alpha"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![file_id],
        previous_path: Some(
            root.join("src/lib.rs")
                .to_string_lossy()
                .into_owned()
                .into(),
        ),
        current_path: Some(
            root.join("src/lib.rs")
                .to_string_lossy()
                .into_owned()
                .into(),
        ),
        added: vec![prism_ir::ObservedNode {
            node: Node {
                id: alpha_new.clone(),
                name: "alpha_v2".into(),
                kind: NodeKind::Function,
                file: file_id,
                span: Span::new(alpha_start, alpha_end),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
        }],
        removed: vec![prism_ir::ObservedNode {
            node: Node {
                id: alpha_old.clone(),
                name: "alpha".into(),
                kind: NodeKind::Function,
                file: file_id,
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
        }],
        updated: vec![(
            prism_ir::ObservedNode {
                node: Node {
                    id: beta.clone(),
                    name: "beta".into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::line(2),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(20), None, None),
            },
            prism_ir::ObservedNode {
                node: Node {
                    id: beta.clone(),
                    name: "beta".into(),
                    kind: NodeKind::Function,
                    file: file_id,
                    span: Span::new(beta_start, beta_end),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(21), None, None),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let lineage_id = history
        .lineage_of(&alpha_new)
        .expect("renamed node should keep lineage")
        .0
        .to_string();
    let host = host_with_prism(Prism::with_history(graph, history));

    let lineage = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.lineage(stale);
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded lineage query should succeed: {error:#}"),
    };
    let slice = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.editSlice(stale, {{ maxLines: 2, maxChars: 120 }});
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded edit slice query should succeed: {error:#}"),
    };
    let full = match host.execute(
        test_session(&host),
        &format!(
            r#"
const stale = {{ id: {}, lineageId: "{}" }};
return prism.full(stale);
"#,
            serde_json::to_string(&serde_json::json!({
                "crateName": "demo",
                "path": "demo::alpha",
                "kind": "Function"
            }))
            .expect("old id should serialize"),
            lineage_id
        ),
        QueryLanguage::Ts,
    ) {
        Ok(result) => result,
        Err(error) => panic!("reloaded full query should succeed: {error:#}"),
    };

    assert!(lineage
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "target_remapped_via_lineage"));
    assert!(slice.result["text"]
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha_v2()"));
    assert!(full
        .result
        .as_str()
        .unwrap_or_default()
        .contains("pub fn alpha_v2()"));
    assert_eq!(lineage.result["current"]["id"]["path"], "demo::alpha_v2");
}

#[test]
fn mutation_trace_records_internal_phases_for_persisted_only_mutations() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    let result = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::alpha".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated alpha".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect("mutation should succeed");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mcp.executeHandler"));
    assert!(operations.contains(&"mcp.encodeResponse"));
    assert!(operations.contains(&"runtimeSync.waitLock"));
    assert!(operations.contains(&"runtimeSync.refreshFs"));
    assert!(operations.contains(&"runtimeSync.snapshotRevisions"));
    assert!(operations.contains(&"mutation.refreshWorkspace"));
    assert!(operations.contains(&"mutation.operation"));
    assert!(operations.contains(&"mutation.encodeResult"));
    assert!(operations.contains(&"mutation.publishTaskUpdate"));
    assert!(operations.contains(&"mutation.publishTaskUpdate.buildSnapshot"));
    assert!(operations.contains(&"mutation.publishTaskUpdate.encode"));
    assert!(operations.contains(&"mutation.publishTaskUpdate.publishEvent"));
    assert!(trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .and_then(|phase| phase.args_summary.as_ref())
        .is_some_and(|args| args["refreshPath"] != Value::String("skipped".to_string())));
}

#[test]
fn dropped_query_run_persists_aborted_call_record() {
    let host = host_with_node(demo_node());
    let session = test_session(&host);

    {
        let query_run = host.begin_query_run(
            session.as_ref(),
            "prism_query",
            "typescript",
            "return { ok: true };",
        );
        query_run.record_phase(
            "typescript.statement_body.prepare",
            &json!({ "mode": "statement_body" }),
            Duration::from_millis(3),
            true,
            None,
        );
    }

    let records = host.mcp_call_log_store.records();
    let record = records
        .iter()
        .find(|record| record.entry.call_type == "tool" && record.entry.name == "prism_query")
        .expect("dropped query record should exist");
    assert!(!record.entry.success);
    assert_eq!(
        record.entry.error.as_deref(),
        Some("request dropped before query completed")
    );
    let operations = record
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"typescript.statement_body.prepare"));
}

#[test]
fn dropped_mutation_run_persists_aborted_call_record() {
    let host = host_with_node(demo_node());
    let session = test_session(&host);

    {
        let run = host.begin_mutation_run(session.as_ref(), "session.finish_task");
        run.record_phase(
            "mutation.operation",
            &json!({ "action": "session.finish_task" }),
            Duration::from_millis(4),
            false,
            Some("request dropped before mutation completed".to_string()),
        );
    }

    let records = host.mcp_call_log_store.records();
    let record = records
        .iter()
        .find(|record| record.entry.call_type == "tool" && record.entry.name == "prism_session")
        .expect("dropped mutation record should exist");
    assert!(!record.entry.success);
    assert_eq!(
        record.entry.error.as_deref(),
        Some("request dropped before mutation completed")
    );
    let detail = host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    assert_eq!(
        trace.entry.error.as_deref(),
        Some("request dropped before mutation completed")
    );
}

#[test]
fn mutation_trace_surfaces_lock_waits_for_finish_task() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    server
        .host
        .start_task(
            test_session(&server.host).as_ref(),
            Some("Trace finish_task lock waits".to_string()),
            Vec::new(),
            None,
        )
        .expect("task should start");

    let result = server
        .execute_logged_mutation(
            "session.finish_task",
            MutationRefreshPolicy::None,
            || {
                server.host.finish_task_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismFinishTaskArgs {
                        summary: "Finished the traced task".to_string(),
                        anchors: Some(vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::alpha".to_string(),
                            kind: "function".to_string(),
                        }]),
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![
                        result.task_id.clone(),
                        result.event_id.clone(),
                        result.memory_id.clone(),
                    ],
                    0,
                )
            },
        )
        .expect("finish task mutation should succeed");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mutation.waitRefreshLock"));
    assert!(operations.contains(&"mutation.waitWorkspaceStoreLock"));
    assert!(operations.contains(&"mutation.appendOutcomePersist"));
}

#[test]
fn mutation_trace_surfaces_lock_waits_for_memory_store() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    let result = server
        .execute_logged_mutation(
            "mutate.memory",
            MutationRefreshPolicy::None,
            || {
                server.host.store_memory_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismMemoryArgs {
                        action: MemoryMutationActionInput::Store,
                        payload: json!({
                            "anchors": [{
                                "type": "node",
                                "crateName": "demo",
                                "path": "demo::alpha",
                                "kind": "function"
                            }],
                            "kind": "episodic",
                            "scope": "session",
                            "content": "Store memory while tracing lock waits"
                        }),
                        task_id: Some("task:trace-memory-locks".to_string()),
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.memory_id.clone()],
                    0,
                )
            },
        )
        .expect("memory mutation should succeed");

    assert!(result.memory_id.starts_with("memory:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mutation.waitWorkspaceStoreLock"));
    assert!(operations.contains(&"mutation.appendMemoryEvent"));
    assert!(operations.contains(&"mutation.waitRefreshLock"));
    assert!(operations.contains(&"mutation.appendOutcomePersist"));
}

#[test]
fn coordination_mutation_trace_records_persistence_subphases() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    let result = server
        .execute_logged_mutation_with_run(
            "mutate.coordination",
            MutationRefreshPolicy::None,
            |run| {
                server.host.store_coordination_traced(
                    test_session(&server.host).as_ref(),
                    PrismCoordinationArgs {
                        kind: CoordinationMutationKindInput::PlanCreate,
                        payload: json!({ "goal": "Trace coordination persistence" }),
                        task_id: None,
                    },
                    run,
                )
            },
            |result| {
                MutationDashboardMeta::coordination(
                    result.event_ids.clone(),
                    result.violations.len(),
                )
            },
        )
        .expect("coordination mutation should succeed");

    assert!(result.event_id.starts_with("coordination:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let operations = trace
        .phases
        .iter()
        .map(|phase| phase.operation.as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"mutation.coordination.refreshWorkspace"));
    assert!(operations.contains(&"mutation.coordination.syncLoadedRevisionBefore"));
    assert!(operations.contains(&"mutation.coordination.waitRefreshLock"));
    assert!(operations.contains(&"mutation.coordination.readRevision"));
    assert!(operations.contains(&"mutation.coordination.applyMutation"));
    assert!(operations.contains(&"mutation.coordination.captureDelta"));
    assert!(operations.contains(&"mutation.coordination.commitPersistBatch"));
    assert!(operations.contains(&"mutation.coordination.syncPublishedPlans"));
    assert!(operations.contains(&"mutation.coordination.publishedPlans.writeLogs"));
    assert!(operations.contains(&"mutation.coordination.publishedPlans.writeIndex"));
    assert!(operations.contains(&"mutation.coordination.syncLoadedRevisionAfter"));
}

#[tokio::test]
async fn validation_feedback_tool_mutation_skips_request_path_refresh() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );
    let server_handle = server.clone();
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server_task = tokio::spawn(async move { server.serve(server_transport).await });
    let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

    let _ = initialize_client(&mut client).await;
    client.send(initialized_notification()).await.unwrap();
    let running = server_task
        .await
        .expect("server join should succeed")
        .expect("server should initialize");

    client
        .send(call_tool_request(
            2,
            "prism_mutate",
            json!({
                "action": "validation_feedback",
                "input": {
                    "context": "Dogfooding refresh-runtime mutation policy.",
                    "prismSaid": "Mutation refresh should run before validation feedback.",
                    "actuallyTrue": "Validation feedback should append directly without request-path refresh.",
                    "category": "freshness",
                    "verdict": "helpful",
                    "correctedManually": true
                }
            })
            .as_object()
            .unwrap()
            .clone(),
        ))
        .await
        .unwrap();

    let envelope = first_tool_content_json(client.receive().await.unwrap());
    assert_eq!(envelope["action"], "validation_feedback");

    let detail = server_handle
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("skipped".to_string()))
    );

    running.cancel().await.unwrap();
}

#[test]
fn follow_up_queries_skip_persisted_refresh_after_local_outcome_write() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.store_outcome_without_refresh(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated alpha".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: None,
        },
    )
    .expect("local outcome write should succeed");

    host.execute(
        test_session(&host),
        r#"
return prism.runtimeStatus();
"#,
        QueryLanguage::Ts,
    )
    .expect("follow-up query should succeed");

    let entry = host
        .query_log_entries(QueryLogArgs {
            limit: Some(5),
            since: None,
            target: Some("runtimeStatus".to_string()),
            operation: None,
            task_id: None,
            min_duration_ms: None,
        })
        .into_iter()
        .find(|entry| entry.kind == "typescript")
        .expect("runtime status query log entry");
    let trace = host
        .query_trace_view(&entry.id)
        .expect("runtime status query trace");
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "typescript.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    let refresh_path = args
        .get("refreshPath")
        .and_then(Value::as_str)
        .expect("refreshPath should be a string");
    assert!(matches!(refresh_path, "none" | "deferred"));
    assert_eq!(args.get("episodicReloaded"), Some(&Value::Bool(false)));
    assert_eq!(args.get("inferenceReloaded"), Some(&Value::Bool(false)));
    assert_eq!(args.get("coordinationReloaded"), Some(&Value::Bool(false)));
    let metrics = args
        .get("metrics")
        .and_then(Value::as_object)
        .expect("refresh metrics");
    for key in [
        "lockWaitMs",
        "lockHoldMs",
        "fsRefreshMs",
        "snapshotRevisionsMs",
        "loadEpisodicMs",
        "loadInferenceMs",
        "loadCoordinationMs",
        "reloadWork",
    ] {
        assert!(
            metrics.contains_key(key),
            "expected typescript refresh args to include `{key}`"
        );
    }
    let reload_work = metrics
        .get("reloadWork")
        .and_then(Value::as_object)
        .expect("typescript refresh reload-work metrics");
    assert_eq!(
        reload_work.get("loadedBytes"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("replayVolume"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("fullRebuildCount"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(metrics.get("fsRefreshMs"), Some(&Value::Number(0.into())));
    assert_eq!(
        reload_work.get("workspaceReloaded"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn first_mutation_after_workspace_refresh_skips_persisted_reload() {
    let root = temp_workspace();
    let session = index_workspace_session(&root).unwrap();
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn gamma() { delta(); }\npub fn delta() {}\n",
    )
    .expect("workspace edit should succeed");
    server
        .host
        .workspace
        .as_ref()
        .expect("workspace session")
        .refresh_fs()
        .expect("workspace refresh should succeed");

    let result = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::gamma".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated gamma".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect("mutation should succeed");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("none".to_string()))
    );
    assert_eq!(args.get("deferred"), Some(&Value::Bool(false)));
}

#[test]
fn mutation_on_dirty_workspace_defers_refresh_instead_of_reloading_runtime() {
    let root = temp_workspace();
    let session = index_workspace_session(&root).unwrap();
    let server = PrismMcpServer::with_session_and_features(
        session,
        PrismMcpFeatures::full().with_internal_developer(true),
    );

    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    )
    .expect("workspace edit should succeed");

    let result = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::alpha".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated alpha".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect("mutation should succeed on live runtime state");

    assert!(result.event_id.starts_with("outcome:"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    let refresh_path = args
        .get("refreshPath")
        .and_then(Value::as_str)
        .expect("refreshPath should be a string");
    assert!(matches!(refresh_path, "none" | "deferred"));
    let metrics = args
        .get("metrics")
        .and_then(Value::as_object)
        .expect("refresh metrics");
    assert_eq!(metrics.get("fsRefreshMs"), Some(&Value::Number(0.into())));
    let reload_work = metrics
        .get("reloadWork")
        .and_then(Value::as_object)
        .expect("refresh reload-work metrics");
    assert_eq!(
        reload_work.get("fullRebuildCount"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("workspaceReloaded"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn prism_query_errors_include_js_message_and_stack() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
throw new Error("boom");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query runtime failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
    assert!(
        !message.contains("Exception generated by QuickJS"),
        "{message}"
    );
}

#[test]
fn prism_query_parse_errors_map_back_to_user_snippet_locations() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const broken = ;
return broken;
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query parse failed"), "{message}");
    assert!(
        message.contains("user snippet line 2, column 16"),
        "{message}"
    );
    assert!(message.contains("Statement-body mode"), "{message}");
    assert!(message.contains("Implicit-expression mode"), "{message}");
    assert!(
        message.contains("single expression such as `({ ... })`"),
        "{message}"
    );
    assert!(
        message.contains("statement-style snippet with an explicit `return ...`"),
        "{message}"
    );
}

#[test]
fn prism_query_runtime_errors_map_back_to_user_snippet_locations() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const value = 1;
throw new Error("boom");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(message.contains("prism_query runtime failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(
        message.contains("Inspect the referenced user-snippet line"),
        "{message}"
    );
}

#[test]
fn prism_query_misspelled_method_names_suggest_repair() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
return prism.seach("alpha");
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");
    let error = error.downcast::<crate::QueryExecutionError>().unwrap();
    assert_eq!(error.data()["code"], "query_typecheck_failed");
    assert!(error.data()["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism.search")));
    assert!(error.data()["nextAction"]
        .as_str()
        .is_some_and(|value| value.contains("prism://api-reference")));
}

#[test]
fn prism_query_rejects_unknown_option_keys_before_host_dispatch() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
return prism.search("alpha", { limt: 1 });
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");
    let error = error.downcast::<crate::QueryExecutionError>().unwrap();
    assert_eq!(error.data()["code"], "query_typecheck_failed");
    assert_eq!(error.data()["invalidKeys"][0], "limt");
    assert_eq!(error.data()["didYouMean"]["limt"], "limit");
    assert!(error.to_string().contains("unknown key `limt`"));
    assert!(error.to_string().contains("prism.search"));
}

#[test]
fn prism_query_dynamic_views_reject_unknown_input_keys() {
    let root = temp_workspace();
    let host = QueryHost::with_session_and_limits_and_features(
        index_workspace_session(&root).unwrap(),
        QueryLimits::default(),
        PrismMcpFeatures::full().with_query_view(QueryViewFeatureFlag::AfterEdit, true),
    );

    let error = host
        .execute(
            test_session(&host),
            r#"
return prism.afterEdit({ taret: [] });
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");
    let error = error.downcast::<crate::QueryExecutionError>().unwrap();
    assert_eq!(error.data()["code"], "query_typecheck_failed");
    assert_eq!(error.data()["method"], "prism.afterEdit");
    assert_eq!(error.data()["invalidKeys"][0], "taret");
    assert_eq!(error.data()["didYouMean"]["taret"], "target");
    assert!(error.to_string().contains("unknown key"));
    assert!(error.to_string().contains("taret"));
}

#[test]
fn prism_query_rejects_unknown_result_properties_before_execution() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
return prism.search("alpha", { limit: 1 })[0].idd;
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");
    let error = error.downcast::<crate::QueryExecutionError>().unwrap();
    assert_eq!(error.data()["code"], "query_typecheck_failed");
    assert_eq!(error.data()["property"], "idd");
    assert_eq!(error.data()["didYouMean"], "id");
    assert!(error.to_string().contains("unknown property `idd`"));
}

#[test]
fn prism_query_allows_valid_nested_result_properties() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.search("alpha", { limit: 1 })[0].id.path;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
}

#[test]
fn prism_query_serialization_failures_have_actionable_hints() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let error = host
        .execute(
            test_session(&host),
            r#"
const value = {};
value.self = value;
return value;
"#,
            QueryLanguage::Ts,
        )
        .expect_err("query should fail");

    let message = error.to_string();
    assert!(
        message.contains("prism_query result is not JSON-serializable"),
        "{message}"
    );
    assert!(message.contains("circular reference"), "{message}");
    assert!(message.contains("statement-body query"), "{message}");
    assert!(message.contains("JSON-serializable values"), "{message}");
}

#[test]
fn prism_query_missing_return_emits_actionable_diagnostic() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result, Value::Null);
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "query_return_missing"));
}

#[test]
fn prism_query_supports_async_style_multi_statement_snippets() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const results = await prism.search("alpha", { limit: 2, kind: "function" });
const sym = await prism.symbol("alpha");
return {
  top: results[0]?.id.path ?? null,
  exact: sym?.id.path ?? null,
  count: results.length,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("async-style query should succeed");

    assert_eq!(result.result["top"], "demo::alpha");
    assert_eq!(result.result["exact"], "demo::alpha");
    assert_eq!(result.result["count"], 1);
}

#[test]
fn prism_query_supports_implicit_expression_object_results() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
({
  top: (await prism.search("alpha", { limit: 1, kind: "function" }))[0]?.id.path ?? null,
  exact: (await prism.symbol("alpha"))?.id.path ?? null,
  count: (await prism.search("alpha", { limit: 2, kind: "function" })).length,
})
"#,
            QueryLanguage::Ts,
        )
        .expect("implicit expression query should succeed");

    assert_eq!(result.result["top"], "demo::alpha");
    assert_eq!(result.result["exact"], "demo::alpha");
    assert_eq!(result.result["count"], 1);
    assert!(!result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "query_return_missing"));
}

#[test]
fn prism_query_supports_implicit_expression_values() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
(await prism.symbol("alpha"))?.id.path ?? null
"#,
            QueryLanguage::Ts,
        )
        .expect("implicit expression value query should succeed");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert!(result.diagnostics.is_empty());
}

#[test]
#[ignore = "heavy end-to-end replay case; run explicitly when validating replay coverage"]
fn query_replay_cases_cover_real_failures_and_repo_queries() {
    let fixture_root = temp_workspace();
    let repo_root = repo_workspace_root();

    let mut fixture_default_host = None;
    let mut fixture_tiny_output_cap_host = None;
    let mut repo_default_host = None;

    for case in replay_cases() {
        let host = match case.profile {
            ReplayHostProfile::FixtureDefault => fixture_default_host.get_or_insert_with(|| {
                host_with_session_internal(index_workspace_session(&fixture_root).unwrap())
            }),
            ReplayHostProfile::FixtureTinyOutputCap => fixture_tiny_output_cap_host
                .get_or_insert_with(|| {
                    let mut limits = QueryLimits::default();
                    limits.max_output_json_bytes = 64;
                    host_with_session_internal_and_limits(
                        index_workspace_session(&fixture_root).unwrap(),
                        limits,
                    )
                }),
            ReplayHostProfile::RepoDefault => repo_default_host.get_or_insert_with(|| {
                host_with_session_internal(index_workspace_session(&repo_root).unwrap())
            }),
        };

        match case.expectation {
            ReplayExpectation::Success(assertion) => {
                let envelope = host
                    .execute(test_session(&host), case.code, QueryLanguage::Ts)
                    .unwrap_or_else(|error| {
                        panic!("replay case `{}` should succeed: {error}", case.name)
                    });
                assertion(&envelope);
            }
            ReplayExpectation::Error(assertion) => {
                let error = host
                    .execute(test_session(&host), case.code, QueryLanguage::Ts)
                    .expect_err(&format!("replay case `{}` should fail", case.name));
                assertion(&error.to_string());
            }
        }
    }
}

#[test]
fn prism_runtime_views_surface_status_logs_and_timeline() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("health listener");
    let addr = listener.local_addr().expect("listener addr");
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(
        prism_dir.join("prism-mcp-daemon.log"),
        [
            json!({
                "timestamp": "2026-03-26T15:12:35Z",
                "level": "INFO",
                "message": "starting prism-mcp",
                "target": "prism_mcp::logging",
                "filename": "crates/prism-mcp/src/logging.rs",
                "line_number": 53,
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:12:36Z",
                "level": "INFO",
                "message": "completed prism workspace indexing",
                "target": "prism_core::indexer",
                "filename": "crates/prism-core/src/indexer.rs",
                "line_number": 435,
                "total_ms": "6227",
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:12:42Z",
                "level": "INFO",
                "message": "prism-mcp daemon ready",
                "target": "prism_mcp::daemon_mode",
                "filename": "crates/prism-mcp/src/daemon_mode.rs",
                "line_number": 57,
                "startup_ms": "6534",
            })
            .to_string(),
            json!({
                "timestamp": "2026-03-26T15:16:23Z",
                "level": "WARN",
                "message": "response error",
                "target": "rmcp::service",
                "filename": "service.rs",
                "line_number": 873,
                "error": "query_execution_failed",
            })
            .to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  status: prism.runtimeStatus(),
  warnings: prism.runtimeLogs({ level: "WARN", limit: 5 }),
  timeline: prism.runtimeTimeline({ limit: 10 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("runtime views query should succeed");

    let status = &result.result["status"];
    assert_eq!(status["health"]["ok"], true);
    assert_eq!(status["daemonCount"], 0);
    assert_eq!(status["bridgeCount"], 0);
    assert_eq!(status["healthPath"], "/healthz");
    assert_eq!(status["connection"]["mode"], "direct-daemon");
    assert_eq!(status["connection"]["transport"], "streamable-http");
    assert_eq!(
        status["connection"]["bridgeRole"],
        "stdio-compatibility-only"
    );
    assert_eq!(
        status["connection"]["healthUri"]
            .as_str()
            .unwrap_or_default(),
        format!("http://{addr}/healthz")
    );
    assert_eq!(
        status["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    assert!(status["logPath"]
        .as_str()
        .unwrap_or_default()
        .ends_with(".prism/prism-mcp-daemon.log"));
    assert!(status["cachePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with(".prism/cache.db"));
    assert_eq!(status["freshness"]["fsDirty"], false);
    assert!(
        status["freshness"]["materialization"]["workspace"]["status"]
            .as_str()
            .is_some()
    );
    assert!(status["freshness"]["status"].as_str().is_some());

    let warnings = result.result["warnings"]
        .as_array()
        .expect("runtime warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["message"], "response error");
    assert_eq!(warnings[0]["target"], "rmcp::service");
    assert_eq!(warnings[0]["fields"]["error"], "query_execution_failed");

    let timeline = result.result["timeline"]
        .as_array()
        .expect("runtime timeline");
    assert_eq!(timeline.len(), 3);
    assert_eq!(timeline[0]["message"], "starting prism-mcp");
    assert_eq!(timeline[1]["message"], "completed prism workspace indexing");
    assert_eq!(timeline[2]["message"], "prism-mcp daemon ready");

    server.join().expect("health server should exit cleanly");
}

#[test]
fn prism_connection_info_surfaces_direct_daemon_endpoint_without_internal_mode() {
    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();
    let addr = "127.0.0.1:9";

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(prism_dir.join("prism-mcp-daemon.log"), "").unwrap();

    let host = host_with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            "return prism.connectionInfo();",
            QueryLanguage::Ts,
        )
        .expect("connection info query should succeed");

    assert_eq!(result.result["mode"], "direct-daemon");
    assert_eq!(result.result["transport"], "streamable-http");
    assert_eq!(result.result["bridgeRole"], "stdio-compatibility-only");
    assert_eq!(
        result.result["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    assert_eq!(
        result.result["healthUri"].as_str().unwrap_or_default(),
        format!("http://{addr}/healthz")
    );
}

#[test]
fn prism_runtime_views_prefer_structured_runtime_state() {
    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();
    let addr = "127.0.0.1:9";

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(prism_dir.join("prism-mcp-daemon.log"), "").unwrap();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let workspace = host.workspace.as_ref().expect("workspace-backed host");
    let source_path = root.join("src/lib.rs");
    fs::write(&source_path, "pub fn runtime_status_refresh() {}\n").unwrap();
    thread::sleep(Duration::from_millis(300));
    workspace.refresh_fs().unwrap();
    host.sync_workspace_revision(workspace).unwrap();
    let last_refresh = workspace
        .last_refresh()
        .expect("workspace refresh should record live refresh metadata");
    fs::write(
        prism_dir.join("prism-mcp-runtime.json"),
        json!({
            "processes": [
                {
                    "pid": std::process::id(),
                    "kind": "daemon",
                    "started_at": current_timestamp(),
                    "health_path": "/healthz",
                    "http_uri": format!("http://{addr}/mcp"),
                    "upstream_uri": null,
                }
            ],
            "events": [
                {
                    "ts": 10,
                    "timestamp": "10",
                    "level": "INFO",
                    "message": "starting prism-mcp",
                    "target": "prism_mcp::logging",
                    "file": "crates/prism-mcp/src/logging.rs",
                    "line_number": null,
                    "fields": { "mode": "daemon" }
                },
                {
                    "ts": 11,
                    "timestamp": "11",
                    "level": "INFO",
                    "message": "built prism-mcp workspace server",
                    "target": "prism_mcp::lib",
                    "file": "crates/prism-mcp/src/lib.rs",
                    "line_number": null,
                    "fields": { "fileCount": 12, "buildMs": 4321 }
                },
                {
                    "ts": 12,
                    "timestamp": "12",
                    "level": "INFO",
                    "message": "prism-mcp workspace refresh",
                    "target": "prism_mcp::lib",
                    "file": "crates/prism-mcp/src/lib.rs",
                    "line_number": null,
                    "fields": {
                        "refreshPath": "auxiliary",
                        "durationMs": 87
                    }
                },
                {
                    "ts": 13,
                    "timestamp": "13",
                    "level": "INFO",
                    "message": "prism-mcp daemon ready",
                    "target": "prism_mcp::daemon_mode",
                    "file": "crates/prism-mcp/src/daemon_mode.rs",
                    "line_number": null,
                    "fields": { "httpUri": format!("http://{addr}/mcp"), "startupMs": 6534 }
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  status: prism.runtimeStatus(),
  timeline: prism.runtimeTimeline({ limit: 10 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("runtime state query should succeed");

    let status = &result.result["status"];
    assert_eq!(status["health"]["ok"], true);
    assert_eq!(status["daemonCount"], 1);
    assert_eq!(status["bridgeCount"], 0);
    assert_eq!(status["healthPath"], "/healthz");
    assert_eq!(status["connection"]["mode"], "direct-daemon");
    assert_eq!(
        status["connection"]["uri"].as_str().unwrap_or_default(),
        format!("http://{addr}/mcp")
    );
    assert_eq!(status["freshness"]["lastWorkspaceBuildMs"], 4321);
    assert_eq!(status["freshness"]["lastDaemonReadyMs"], 6534);
    assert_eq!(status["freshness"]["lastRefreshPath"], last_refresh.path);
    assert_eq!(
        status["freshness"]["lastRefreshDurationMs"],
        last_refresh.duration_ms
    );
    assert_eq!(
        status["freshness"]["lastRefreshTimestamp"],
        last_refresh.timestamp
    );
    assert_eq!(status["freshness"]["fsDirty"], false);
    assert_eq!(
        status["freshness"]["materialization"]["workspace"]["status"],
        "current"
    );
    let processes = status["processes"].as_array().expect("runtime processes");
    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0]["kind"], "daemon");

    let timeline = result.result["timeline"]
        .as_array()
        .expect("runtime timeline");
    assert_eq!(timeline.len(), 4);
    assert_eq!(timeline[0]["message"], "starting prism-mcp");
    assert_eq!(timeline[1]["message"], "built prism-mcp workspace server");
    assert_eq!(timeline[2]["message"], "prism-mcp workspace refresh");
    assert_eq!(timeline[3]["message"], "prism-mcp daemon ready");
}

#[test]
fn prism_runtime_views_do_not_source_freshness_from_runtime_state_refresh_events() {
    let root = temp_workspace();
    let prism_dir = root.join(".prism");
    fs::create_dir_all(&prism_dir).unwrap();
    let addr = "127.0.0.1:9";

    fs::write(
        prism_dir.join("prism-mcp-http-uri"),
        format!("http://{addr}/mcp\n"),
    )
    .unwrap();
    fs::write(prism_dir.join("prism-mcp-daemon.log"), "").unwrap();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    fs::write(
        prism_dir.join("prism-mcp-runtime.json"),
        json!({
            "processes": [],
            "events": [
                {
                    "ts": 12,
                    "timestamp": "12",
                    "level": "INFO",
                    "message": "prism-mcp workspace refresh",
                    "target": "prism_mcp::lib",
                    "file": "crates/prism-mcp/src/lib.rs",
                    "line_number": null,
                    "fields": {
                        "refreshPath": "auxiliary",
                        "durationMs": 87
                    }
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let result = host
        .execute(
            test_session(&host),
            "return prism.runtimeStatus().freshness;",
            QueryLanguage::Ts,
        )
        .expect("runtime status query should succeed");
    let last_refresh = host
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.last_refresh());

    assert_eq!(
        result.result["lastRefreshPath"],
        last_refresh
            .as_ref()
            .map(|refresh| Value::String(refresh.path.clone()))
            .unwrap_or(Value::Null)
    );
    assert_eq!(
        result.result["lastRefreshDurationMs"],
        last_refresh
            .as_ref()
            .map(|refresh| Value::Number(refresh.duration_ms.into()))
            .unwrap_or(Value::Null)
    );
    assert_eq!(
        result.result["lastRefreshTimestamp"],
        last_refresh
            .as_ref()
            .map(|refresh| Value::String(refresh.timestamp.clone()))
            .unwrap_or(Value::Null)
    );
    assert_ne!(
        result.result["lastRefreshPath"],
        Value::String("auxiliary".to_string())
    );
}

#[test]
fn prism_runtime_views_ignore_invalid_runtime_state_sidecar() {
    let root = temp_workspace();
    fs::write(root.join(".gitignore"), ".prism/\n").unwrap();
    fs::create_dir_all(root.join(".prism")).unwrap();
    fs::write(
        root.join(".prism").join("prism-mcp-runtime.json"),
        "{ invalid",
    )
    .unwrap();
    fs::write(root.join(".prism").join("prism-mcp-daemon.log"), "").unwrap();
    fs::write(
        root.join(".prism").join("prism-mcp-http-uri"),
        "http://127.0.0.1:9/mcp",
    )
    .unwrap();

    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
        )
        .expect("invalid runtime state should not break runtime status");

    assert_eq!(result.result["health"]["ok"], false);
    assert_eq!(result.result["daemonCount"], 0);
    assert_eq!(result.result["bridgeCount"], 0);
    assert!(result.result["freshness"]["status"].as_str().is_some());
}

#[test]
fn workspace_refresh_policy_records_incremental_refresh_events() {
    assert!(should_record_workspace_refresh_event(
        "incremental",
        false,
        false,
        false,
        42,
    ));
}

#[test]
fn workspace_refresh_policy_logs_slow_refreshes_at_info_by_default() {
    assert_eq!(
        workspace_refresh_log_level("none", SLOW_WORKSPACE_REFRESH_LOG_MS, false),
        Some(Level::INFO)
    );
    assert_eq!(workspace_refresh_log_level("none", 42, false), None);
}

#[test]
fn workspace_refresh_policy_uses_debug_level_when_env_logging_is_enabled() {
    assert_eq!(
        workspace_refresh_log_level("none", 42, true),
        Some(Level::DEBUG)
    );
}

#[test]
fn prism_change_views_surface_recent_files_symbols_and_task_changes() {
    let root = temp_workspace();
    let source_path = root.join("src/lib.rs");
    let source = "pub fn alpha() {}\npub fn beta() {}\n";
    fs::write(&source_path, source).unwrap();

    let alpha_span = {
        let start = source.find("alpha").expect("alpha span");
        Span::new(start, start + "alpha".len())
    };
    let beta_span = {
        let start = source.find("beta").expect("beta span");
        Span::new(start, start + "beta".len())
    };

    let mut graph = Graph::new();
    let file_id = graph.ensure_file(&source_path);
    let alpha_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: alpha_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: file_id,
        span: alpha_span,
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha_id.clone()]);

    let task_id = TaskId::new("task:change-view");
    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:change-view"),
                ts: 10,
                actor: EventActor::System,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::File(file_id), AnchorRef::Node(alpha_id.clone())],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "patched src/lib.rs".into(),
            evidence: Vec::new(),
            metadata: json!({
                "trigger": "ManualReindex",
                "filePaths": [source_path.to_string_lossy().into_owned()],
                "changedSymbols": [
                    {
                        "status": "updated_after",
                        "id": alpha_id,
                        "name": "alpha",
                        "kind": NodeKind::Function,
                        "filePath": source_path.to_string_lossy().into_owned(),
                        "span": alpha_span,
                    },
                    {
                        "status": "removed",
                        "id": NodeId::new("demo", "demo::beta", NodeKind::Function),
                        "name": "beta",
                        "kind": NodeKind::Function,
                        "filePath": source_path.to_string_lossy().into_owned(),
                        "span": beta_span,
                    }
                ],
            }),
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  files: prism.changedFiles({ limit: 5, path: "src/lib.rs" }),
  symbols: prism.changedSymbols("src/lib.rs", { limit: 5 }),
  patches: prism.recentPatches({ path: "src/lib.rs", limit: 5 }),
  diff: (() => {
    const sym = prism.symbol("alpha");
    return sym ? prism.diffFor(sym, { limit: 5 }) : [];
  })(),
  lineageDiff: (() => {
    const sym = prism.symbol("alpha");
    return sym?.lineageId ? prism.diffFor({ lineageId: sym.lineageId }, { limit: 5 }) : [];
  })(),
  task: prism.taskChanges("task:change-view", { limit: 5 }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("change-view query should succeed");

    let changed_file = &result.result["files"][0];
    assert!(changed_file["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("src/lib.rs"));
    assert_eq!(changed_file["changedSymbolCount"], 2);
    assert_eq!(changed_file["removedCount"], 1);
    assert_eq!(changed_file["updatedCount"], 1);

    let symbols = result.result["symbols"]
        .as_array()
        .expect("changed symbols");
    assert_eq!(symbols.len(), 2);
    assert!(symbols.iter().any(|symbol| {
        symbol["status"] == "updated_after"
            && symbol["location"]["startLine"] == 1
            && symbol["excerpt"]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("alpha")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol["status"] == "removed"
            && symbol["location"]["startLine"] == 2
            && symbol["excerpt"]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("beta")
    }));

    let patch = &result.result["patches"][0];
    assert_eq!(patch["trigger"], "ManualReindex");
    assert_eq!(patch["taskId"], "task:change-view");
    assert_eq!(patch["changedSymbols"].as_array().unwrap().len(), 2);
    assert!(patch["files"][0]
        .as_str()
        .unwrap_or_default()
        .ends_with("src/lib.rs"));

    let diff = result.result["diff"].as_array().expect("target diff");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0]["eventId"], "outcome:change-view");
    assert_eq!(diff[0]["symbol"]["name"], "alpha");
    assert_eq!(diff[0]["symbol"]["location"]["startLine"], 1);
    assert!(diff[0]["symbol"]["excerpt"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("alpha"));
    assert!(diff[0]["symbol"]["lineageId"].as_str().is_some());

    let lineage_diff = result.result["lineageDiff"]
        .as_array()
        .expect("lineage diff");
    assert_eq!(lineage_diff.len(), 1);
    assert_eq!(lineage_diff[0]["symbol"]["name"], "alpha");
    assert_eq!(
        lineage_diff[0]["symbol"]["lineageId"],
        diff[0]["symbol"]["lineageId"]
    );

    let task_patch = &result.result["task"][0];
    assert_eq!(task_patch["eventId"], "outcome:change-view");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prism_search_surfaces_toml_config_keys_through_normal_queries() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"

[workspace]
members = ["crates/alpha"]

[dependencies]
serde = "1.0"
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const workspaceKey = prism.search("workspace", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
const membersKey = prism.search("members", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
const serdeKey = prism.search("serde", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 1,
})[0];
return {
  workspaceKey,
  membersKey,
  serdeKey,
  workspaceContains: workspaceKey?.relations().contains ?? [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("toml query should succeed");

    assert_eq!(result.result["workspaceKey"]["name"], "workspace");
    assert!(result.result["workspaceKey"]["filePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with("/Cargo.toml"));
    assert_eq!(result.result["membersKey"]["name"], "members");
    assert_eq!(result.result["serdeKey"]["name"], "serde");
    let workspace_contains = result.result["workspaceContains"]
        .as_array()
        .expect("workspace contains");
    assert!(workspace_contains
        .iter()
        .any(|value| value["name"] == "members"));
}

#[test]
fn prism_search_supports_exact_path_and_structured_key_narrowing() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("crates/alpha/src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/alpha"]

[package]
name = "demo"
version.workspace = true
"#,
    )
    .unwrap();
    fs::write(
        root.join("crates/alpha/Cargo.toml"),
        r#"[package]
name = "alpha"
version.workspace = true
"#,
    )
    .unwrap();
    fs::write(root.join("crates/alpha/src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(
            test_session(&host),
            r#"
return {
  topLevel: prism.search("workspace", {
    path: "Cargo.toml",
    pathMode: "exact",
    kind: "toml-key",
    topLevelOnly: true,
    limit: 5,
  }),
  nested: prism.search("workspace", {
    path: "Cargo.toml",
    pathMode: "exact",
    kind: "toml-key",
    structuredPath: "package.version.workspace",
    limit: 5,
  }),
};
"#,
            QueryLanguage::Ts,
        )
        .expect("exact path search query should succeed");

    let top_level = result.result["topLevel"]
        .as_array()
        .expect("top-level results");
    assert_eq!(top_level.len(), 1);
    assert_eq!(top_level[0]["name"], "workspace");
    assert!(top_level[0]["id"]["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("::workspace"));
    assert!(top_level[0]["filePath"]
        .as_str()
        .unwrap_or_default()
        .ends_with("/Cargo.toml"));

    let nested = result.result["nested"].as_array().expect("nested results");
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0]["name"], "workspace");
    assert!(nested[0]["id"]["path"]
        .as_str()
        .unwrap_or_default()
        .ends_with("::package::version::workspace"));
}

#[test]
fn markdown_heading_symbols_cover_their_section_body() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Top\nalpha\n## Child\nbeta\n# Next\ngamma\n",
    )
    .unwrap();

    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = host
        .execute(test_session(&host),
            r#"
const heading = prism.search("Top", { path: "docs/SPEC.md", kind: "markdown-heading", limit: 1 })[0];
return {
  full: heading?.full() ?? null,
  excerpt: heading?.excerpt() ?? null,
  location: heading?.location ?? null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let full = result.result["full"].as_str().unwrap_or_default();
    assert!(full.contains("## Child"));
    assert!(!full.contains("# Next"));
    assert_eq!(result.result["location"]["startLine"], 1);
    assert_eq!(result.result["location"]["endLine"], 4);
    assert_eq!(result.result["excerpt"]["startLine"], 1);
}

#[test]
fn spec_cluster_and_drift_surface_behavioral_owners() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return {
  cluster: spec ? prism.specCluster(spec) : null,
  drift: spec ? prism.explainDrift(spec) : null,
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let read_paths = result.result["cluster"]["readPath"]
        .as_array()
        .expect("cluster readPath should be an array");
    assert!(read_paths.iter().any(|candidate| {
        candidate["symbol"]["id"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("memory_recall")
    }));

    let persistence_paths = result.result["cluster"]["persistencePath"]
        .as_array()
        .expect("cluster persistencePath should be an array");
    assert!(persistence_paths.iter().any(|candidate| {
        candidate["symbol"]["id"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("reanchor_persisted_memory_snapshot")
    }));

    let tests = result.result["cluster"]["tests"]
        .as_array()
        .expect("cluster tests should be an array");
    assert!(tests.iter().any(|candidate| {
        candidate["symbol"]["filePath"]
            .as_str()
            .unwrap_or_default()
            .contains("/tests/")
    }));

    let next_reads = result.result["drift"]["nextReads"]
        .as_array()
        .expect("drift nextReads should be an array");
    assert!(!next_reads.is_empty());
    assert!(matches!(
        result.result["drift"]["trustSignals"]["confidenceLabel"].as_str(),
        Some("medium" | "high")
    ));
    assert!(result.result["drift"]["trustSignals"]["evidenceSources"]
        .as_array()
        .is_some_and(|items| items.iter().any(|value| value == "inferred")));
    assert!(result.result["drift"]["expectations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value
            .as_str()
            .unwrap_or_default()
            .contains("prior outcomes")));

    let spec_id = host
        .current_prism()
        .search(
            "Integration Points",
            1,
            Some(NodeKind::MarkdownHeading),
            Some("docs/SPEC.md"),
        )
        .first()
        .expect("spec heading should be indexed")
        .id()
        .clone();
    let symbol_resource = host
        .symbol_resource_value(test_session(&host), &spec_id)
        .unwrap();
    assert!(symbol_resource.workspace_revision.graph_version > 0);
    assert!(symbol_resource.spec_cluster.is_some());
    assert!(symbol_resource.spec_drift.is_some());
    assert!(!symbol_resource.suggested_reads.is_empty());
    assert!(!symbol_resource.read_context.suggested_reads.is_empty());
    assert!(!symbol_resource.edit_context.suggested_queries.is_empty());
    assert!(!symbol_resource.discovery.suggested_reads.is_empty());
    assert!(!symbol_resource
        .discovery
        .validation_context
        .suggested_queries
        .is_empty());
    assert!(
        symbol_resource
            .discovery
            .recent_change_context
            .suggested_queries
            .len()
            >= 3
    );
    assert!(symbol_resource
        .discovery
        .trust_signals
        .evidence_sources
        .iter()
        .any(|source| matches!(
            source,
            prism_js::EvidenceSourceKind::DirectGraph | prism_js::EvidenceSourceKind::Inferred
        )));
    assert!(!symbol_resource.discovery.where_used_behavioral.is_empty());
    assert!(!symbol_resource.discovery.why.is_empty());
    for expected in [
        "Read Context",
        "Focused Block",
        "Next Reads",
        "Where Used",
        "Validation Recipe",
        "Edit Context",
    ] {
        assert!(symbol_resource
            .suggested_queries
            .iter()
            .any(|query| query.label == expected));
    }
    assert_eq!(
        symbol_resource.related_resources[0].uri,
        symbol_resource.uri
    );
}

#[test]
fn owner_lookup_and_behavioral_search_prefer_behavioral_owners() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return {
  behavioral: prism.search("memory recall", {
    strategy: "behavioral",
    ownerKind: "read",
    limit: 5,
  }),
  owners: spec ? prism.owners(spec, { kind: "read", limit: 5 }) : [],
  implementationOwners: spec
    ? prism.implementationFor(spec, { mode: "owners", ownerKind: "read" })
    : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    let behavioral = result.result["behavioral"]
        .as_array()
        .expect("behavioral search should return an array");
    assert!(!behavioral.is_empty());
    assert!(behavioral.iter().any(|symbol| {
        symbol["ownerHint"]["kind"].as_str() == Some("read")
            && symbol["id"]["path"]
                .as_str()
                .unwrap_or_default()
                .contains("memory_recall")
    }));
    assert!(behavioral.iter().any(|symbol| {
        symbol["ownerHint"]["trustSignals"]["evidenceSources"]
            .as_array()
            .is_some_and(|items| items.iter().any(|value| value == "inferred"))
    }));

    let owners = result.result["owners"]
        .as_array()
        .expect("owners should return an array");
    assert!(owners.iter().any(|candidate| {
        candidate["kind"].as_str() == Some("read")
            && candidate["why"]
                .as_str()
                .unwrap_or_default()
                .contains("read-oriented")
    }));
    assert!(owners.iter().any(|candidate| {
        matches!(
            candidate["trustSignals"]["confidenceLabel"].as_str(),
            Some("medium" | "high")
        ) && candidate["trustSignals"]["evidenceSources"]
            .as_array()
            .is_some_and(|items| items.iter().any(|value| value == "inferred"))
    }));

    let implementation_owners = result.result["implementationOwners"]
        .as_array()
        .expect("owner-mode implementationFor should return an array");
    assert!(implementation_owners.iter().any(|symbol| {
        symbol["ownerHint"]["kind"].as_str() == Some("read")
            && symbol["id"]["path"]
                .as_str()
                .unwrap_or_default()
                .contains("memory_recall")
    }));
}

#[test]
fn search_resource_payload_surfaces_suggested_reads() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(
            test_session(&host),
            "prism://search/memory%20recall?strategy=behavioral&ownerKind=read",
            "memory recall",
        )
        .unwrap();

    assert_eq!(payload.strategy, "behavioral");
    assert_eq!(payload.owner_kind.as_deref(), Some("read"));
    assert!(payload.workspace_revision.graph_version > 0);
    assert!(!payload.suggested_reads.is_empty());
    assert!(payload.discovery.is_some());
    assert!(payload
        .discovery
        .as_ref()
        .is_some_and(|bundle| !bundle.suggested_reads.is_empty()));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .trust_signals
            .evidence_sources
            .iter()
            .any(|source| matches!(source, prism_js::EvidenceSourceKind::Inferred))
    }));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .validation_context
            .suggested_queries
            .iter()
            .any(|query| query.label == "Validation Context")
    }));
    assert!(payload
        .discovery
        .as_ref()
        .is_some_and(|bundle| !bundle.why.is_empty()));
    assert!(payload.discovery.as_ref().is_some_and(|bundle| {
        bundle
            .recent_change_context
            .suggested_queries
            .iter()
            .any(|query| query.label == "Recent Change Context")
    }));
    assert!(payload.suggested_reads.iter().any(|candidate| {
        candidate.kind == "read" && candidate.symbol.id.path.contains("memory_recall")
    }));
    assert!(payload.results.iter().any(|symbol| {
        symbol
            .owner_hint
            .as_ref()
            .is_some_and(|hint| hint.kind == "read")
    }));
    assert!(payload.top_read_context.is_some());
    assert!(!payload.suggested_queries.is_empty());
    assert_eq!(payload.suggested_queries[0].label, "Direct Search");
    assert_eq!(payload.suggested_queries[1].label, "Behavioral Search");
    assert_eq!(payload.suggested_queries[2].label, "Read Context");
    assert!(payload.related_resources[0]
        .uri
        .starts_with("prism://search/memory%20recall"));
    assert!(payload.related_resources[1]
        .uri
        .starts_with("prism://symbol/"));
}

#[test]
fn search_resource_payload_echoes_applied_uri_options() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crates/alpha"]

[package]
name = "demo"
version.workspace = true
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(test_session(&host),
            "prism://search/workspace?strategy=direct&kind=toml-key&path=Cargo.toml&pathMode=exact&structuredPath=workspace&topLevelOnly=true&preferCallableCode=false&preferEditableTargets=true&preferBehavioralOwners=true&includeInferred=false",
            "workspace",
        )
        .unwrap();

    assert_eq!(payload.strategy, "direct");
    assert_eq!(payload.owner_kind, None);
    assert_eq!(payload.kind.as_deref(), Some("toml-key"));
    assert_eq!(payload.path.as_deref(), Some("Cargo.toml"));
    assert_eq!(payload.path_mode.as_deref(), Some("exact"));
    assert_eq!(payload.structured_path.as_deref(), Some("workspace"));
    assert_eq!(payload.top_level_only, Some(true));
    assert_eq!(payload.prefer_callable_code, Some(false));
    assert_eq!(payload.prefer_editable_targets, Some(true));
    assert_eq!(payload.prefer_behavioral_owners, Some(true));
    assert!(!payload.include_inferred);
    assert_eq!(payload.results.len(), 1);
    assert_eq!(
        payload.results[0].id.path,
        "demo::document::Cargo_toml::workspace"
    );
}

#[test]
fn resource_suggested_candidates_use_compact_default_excerpts() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let id = host
        .current_prism()
        .search(
            "memory_recall",
            1,
            Some(NodeKind::Function),
            Some("src/recall.rs"),
        )
        .first()
        .expect("memory_recall should be indexed")
        .id()
        .clone();

    let symbol_payload = host
        .symbol_resource_value(test_session(&host), &id)
        .unwrap();
    let search_payload = host
        .search_resource_value(
            test_session(&host),
            "prism://search/memory_recall?strategy=behavioral&ownerKind=read",
            "memory_recall",
        )
        .unwrap();

    let symbol_candidate_excerpt = symbol_payload
        .suggested_reads
        .iter()
        .filter_map(|candidate| candidate.symbol.source_excerpt.as_ref())
        .next()
        .expect("symbol resource suggested candidate should include excerpt");
    assert!(symbol_candidate_excerpt.text.chars().count() <= 240);

    let search_excerpt = search_payload
        .suggested_reads
        .iter()
        .find(|candidate| candidate.symbol.id.path.contains("memory_recall"))
        .and_then(|candidate| candidate.symbol.source_excerpt.as_ref())
        .expect("search resource suggested candidate should include excerpt");
    assert!(search_excerpt.text.chars().count() <= 240);
    assert!(search_excerpt.truncated);
}

#[test]
fn read_and_edit_context_queries_return_semantic_bundles() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let session = index_workspace_session(&root).unwrap();
    let spec_id = session
        .prism()
        .search(
            "Integration Points",
            1,
            Some(NodeKind::MarkdownHeading),
            Some("docs/SPEC.md"),
        )
        .first()
        .expect("spec heading should be indexed")
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validation-context"),
                ts: 50,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:validation-context")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(spec_id)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "integration-point regression surfaced during validation".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();
    let host = QueryHost::with_session(session);

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec
  ? {
      read: prism.readContext(spec),
      edit: prism.editContext(spec),
      validation: prism.validationContext(spec),
      recentChange: prism.recentChangeContext(spec),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result.result["read"]["directLinks"].is_array());
    assert_eq!(
        result.result["read"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["read"]["directLinkBlocks"].is_array());
    assert!(result.result["read"]["suggestedReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["read"]["testBlocks"].is_array());
    assert!(result.result["edit"]["writePaths"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert_eq!(
        result.result["edit"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["edit"]["writePathBlocks"].is_array());
    assert!(result.result["edit"]["checklist"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["validation"]["tests"].is_array());
    assert_eq!(
        result.result["validation"]["targetBlock"]["symbol"]["name"],
        "Integration Points"
    );
    assert!(result.result["validation"]["testBlocks"].is_array());
    assert!(result.result["validation"]["recentFailures"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["recentChange"]["recentEvents"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["recentChange"]["suggestedQueries"].is_array());
}

#[test]
fn discovery_bundle_query_trace_records_internal_subphases() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());

    host.execute(
        test_session(&host),
        r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.discovery(spec) : null;
"#,
        QueryLanguage::Ts,
    )
    .expect("discovery query should succeed");

    let trace = host
        .execute(
            test_session(&host),
            r#"
const recent = prism.queryLog({ limit: 5 });
const discovery = recent.find((entry) => entry.operations.includes("discoveryBundle"));
return discovery ? prism.queryTrace(discovery.id) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query trace lookup should succeed");

    let phases = trace.result["phases"].as_array().expect("trace phases");
    let operations = phases
        .iter()
        .filter_map(|phase| phase["operation"].as_str())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"discoveryBundle.prefetch"));
    assert!(operations.contains(&"discoveryBundle.entrypointsFor"));
    assert!(operations.contains(&"discoveryBundle.whereUsedBehavioral"));
    assert!(operations.contains(&"discoveryBundle.sharedContext"));
    assert!(operations.contains(&"discoveryBundle"));
}

#[test]
fn discovery_helpers_surface_next_reads_and_behavioral_where_used() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec
  ? {
      nextReads: prism.nextReads(spec, { limit: 5 }),
      whereUsed: prism.whereUsed(spec, { mode: "behavioral", limit: 5 }),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result.result["nextReads"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(result.result["whereUsed"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[test]
fn discovery_helpers_surface_direct_where_used_and_entrypoints() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
const beta = prism.symbol("beta");
return beta
  ? {
      whereUsed: prism.whereUsed(beta, { mode: "direct", limit: 5 }).map((sym) => sym.id.path),
      entrypoints: prism.entrypointsFor(beta, { limit: 5 }).map((sym) => sym.id.path),
    }
  : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["whereUsed"], json!(["demo::alpha"]));
    assert_eq!(result.result["entrypoints"], json!(["demo::alpha"]));
}

#[test]
fn custom_query_limits_apply_per_host() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::beta", NodeKind::Function),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        target: NodeId::new("demo", "demo::beta", NodeKind::Function),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let host = QueryHost::new_with_limits(
        Prism::new(graph),
        QueryLimits {
            max_result_nodes: 1,
            max_call_graph_depth: 1,
            max_output_json_bytes: 512,
        },
    );

    let search = host
        .execute(
            test_session(&host),
            r#"
return prism.search("a", { limit: 10 }).map((sym) => sym.id.path);
"#,
            QueryLanguage::Ts,
        )
        .expect("search should succeed");
    assert_eq!(search.result.as_array().map(Vec::len), Some(1));
    assert_eq!(search.diagnostics[0].code, "result_truncated");

    let depth = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym?.callGraph(9);
"#,
            QueryLanguage::Ts,
        )
        .expect("call graph should succeed");
    assert_eq!(depth.result["maxDepthReached"], 1);
    assert!(depth
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "depth_limited"));

    let capped_host = QueryHost::new_with_limits(
        Prism::new(Graph::new()),
        QueryLimits {
            max_result_nodes: 1,
            max_call_graph_depth: 1,
            max_output_json_bytes: 32,
        },
    );
    let capped = capped_host
        .execute(
            test_session(&capped_host),
            r#"
return "abcdefghijklmnopqrstuvwxyz0123456789";
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(capped.result, Value::Null);
    assert!(capped
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "result_truncated"));
}

#[test]
fn search_kind_filter_uses_cli_style_names() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.search("main", { kind: "function" });
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
}

#[test]
fn reports_diagnostics_for_overbroad_searches() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
prism.search("main", { limit: 1000 });
return prism.diagnostics();
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
    assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "result_truncated");
    assert_eq!(
        result.diagnostics[0]
            .data
            .as_ref()
            .and_then(|data| data["nextAction"].as_str()),
        Some(
            "Use prism.search(query, { path: ..., module: ..., kind: ..., taskId: ..., limit: ... }) to narrow the result set."
        )
    );
}

#[test]
fn unknown_host_operations_return_actionable_diagnostics() {
    let host = host_with_node(demo_node());
    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "dispatch unknown operation",
        ),
    );

    let error = execution
        .dispatch("bogusOperation", r#"{}"#)
        .expect_err("unknown operation should fail");

    assert!(error.to_string().contains("unsupported host operation"));
    assert_eq!(execution.diagnostics().len(), 1);
    assert_eq!(execution.diagnostics()[0].code, "unknown_method");
    assert!(execution.diagnostics()[0]
        .data
        .as_ref()
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism://capabilities")));
}

#[test]
fn unknown_host_operations_suggest_closest_known_operation() {
    let host = host_with_node(demo_node());
    let execution = QueryExecution::new(
        host.clone(),
        test_session(&host),
        host.current_prism(),
        host.begin_query_run(
            test_session(&host).as_ref(),
            "test",
            "test",
            "dispatch misspelled operation",
        ),
    );

    execution
        .dispatch("serach", r#"{}"#)
        .expect_err("misspelled operation should fail");

    assert_eq!(execution.diagnostics().len(), 1);
    assert_eq!(
        execution.diagnostics()[0]
            .data
            .as_ref()
            .and_then(|data| data["didYouMean"].as_str()),
        Some("search")
    );
}

#[test]
fn reuses_warm_runtime_across_queries() {
    let host = host_with_node(demo_node());

    let first = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return sym?.id.path;
"#,
            QueryLanguage::Ts,
        )
        .expect("first query should succeed");
    let second = host
        .execute(
            test_session(&host),
            r#"
return prism.entrypoints().map((sym) => sym.id.path);
"#,
            QueryLanguage::Ts,
        )
        .expect("second query should succeed");

    assert_eq!(first.result, Value::String("demo::main".to_owned()));
    assert_eq!(second.result.as_array().map(|items| items.len()), Some(1));
}

#[test]
fn cleans_up_user_globals_between_queries() {
    let host = host_with_node(demo_node());

    host.execute(
        test_session(&host),
        r#"
globalThis.__prismLeaked = 1;
return true;
"#,
        QueryLanguage::Ts,
    )
    .expect("first query should succeed");

    let second = host
        .execute(
            test_session(&host),
            r#"
return typeof globalThis.__prismLeaked;
"#,
            QueryLanguage::Ts,
        )
        .expect("second query should succeed");

    assert_eq!(second.result, Value::String("undefined".to_owned()));
}

#[test]
fn exposes_blast_radius_and_related_failures() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: alpha.clone(),
        target: beta.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 1.0,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone(), beta.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:test"),
                ts: 10,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha previously failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_unit".into(),
                passed: false,
            }],
            metadata: Value::Null,
        })
        .expect("outcome event should store");

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return {
  blast: sym ? prism.blastRadius(sym) : null,
  failures: sym ? prism.relatedFailures(sym) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(
        result.result["blast"]["directNodes"][0]["path"],
        "demo::beta"
    );
    assert_eq!(
        result.result["failures"][0]["summary"],
        "alpha previously failed"
    );
}

#[test]
fn exposes_validation_recipe() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:9"),
                ts: 9,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha broke validation".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_validation".into(),
                passed: false,
            }],
            metadata: Value::Null,
        })
        .expect("outcome event should store");

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? prism.validationRecipe(sym) : null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["target"]["path"], "demo::alpha");
    assert_eq!(
        result.result["checks"][0],
        Value::String("test:alpha_validation".to_string())
    );
    assert_eq!(
        result.result["scoredChecks"][0]["label"],
        Value::String("test:alpha_validation".to_string())
    );
    assert_eq!(
        result.result["recentFailures"][0]["summary"],
        "alpha broke validation"
    );
}

#[test]
fn exposes_co_change_neighbors() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    for (id, line) in [(&alpha, 1), (&beta, 2)] {
        graph.add_node(Node {
            id: id.clone(),
            name: id.path.rsplit("::").next().unwrap().into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(line),
            language: Language::Rust,
        });
    }

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone(), beta.clone()]);
    history.apply(&prism_ir::ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: None,
        current_path: None,
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![
            (
                prism_ir::ObservedNode {
                    node: Node {
                        id: alpha.clone(),
                        name: "alpha".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(1),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
                },
                prism_ir::ObservedNode {
                    node: Node {
                        id: alpha.clone(),
                        name: "alpha".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(1),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(11), None, None),
                },
            ),
            (
                prism_ir::ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(20), None, None),
                },
                prism_ir::ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(21), None, None),
                },
            ),
        ],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let host = host_with_prism(Prism::with_history(graph, history));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? prism.coChangeNeighbors(sym) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result[0]["count"], 1);
    assert_eq!(result.result[0]["nodes"][0]["path"], "demo::beta");
}

#[test]
fn inferred_edge_overlay_affects_relations_queries() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: beta.clone(),
        name: "beta".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Rust,
    });

    let host = host_with_prism(Prism::new(graph));
    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.9,
            scope: Some(InferredEdgeScopeInput::SessionOnly),
            evidence: Some(vec!["task-local inference".to_string()]),
            task_id: None,
        },
    )
    .expect("inferred edge should store");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result[0], "demo::beta");
}

#[test]
fn persisted_inferred_edges_reload_with_workspace_session() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.95,
            scope: Some(InferredEdgeScopeInput::Persisted),
            evidence: Some(vec!["persisted inference".to_string()]),
            task_id: Some("task:persist".to_string()),
        },
    )
    .expect("inferred edge should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let result = reloaded
        .execute(
            test_session(&reloaded),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert!(result
        .result
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::beta"));
}

#[test]
fn persisted_notes_reload_with_workspace_session() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::alpha",
                    "kind": "function"
                }],
                "kind": "episodic",
                "content": "alpha previously regressed",
                "trust": 0.9
            }),
            task_id: Some("task:note".to_string()),
        },
    )
    .expect("note should persist");

    let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let replay = reloaded
        .current_prism()
        .resume_task(&TaskId::new("task:note"));
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);

    let recalled = test_session(&reloaded)
        .notes
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            text: Some("regressed".to_string()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Episodic]),
            since: None,
        })
        .expect("recall should succeed");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entry.content, "alpha previously regressed");
}

#[test]
fn repo_memory_events_are_queryable_and_visible_in_memory_resource_history() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha ownership belongs in committed shared memory",
                    "promotedFrom": ["memory:seed"],
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory".to_string()),
            },
        )
        .expect("repo memory should persist");

    let queried = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  scope: "repo",
  actions: ["promoted"],
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory events query should succeed");

    assert_eq!(queried.result[0]["scope"], "Repo");
    assert_eq!(queried.result[0]["action"], "Promoted");
    assert_eq!(queried.result[0]["promotedFrom"][0], "memory:seed");

    let payload = host
        .memory_resource_value(
            test_session(&host).as_ref(),
            &MemoryId(result.memory_id.clone()),
        )
        .expect("memory resource should load");
    assert_eq!(payload.memory.scope, "Repo");
    assert_eq!(
        payload.memory.metadata["provenance"]["origin"],
        "manual_store"
    );
    assert_eq!(payload.memory.metadata["publication"]["status"], "active");
    assert_eq!(payload.history.len(), 1);
    assert_eq!(payload.history[0].memory_id, result.memory_id);
}

#[test]
fn repo_memory_events_surface_superseded_publications() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership supersedes the older shared note",
                    "supersedes": ["memory:legacy-routing-note"],
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory-supersede".to_string()),
            },
        )
        .expect("repo memory supersede should persist");

    let queried = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  scope: "repo",
  actions: ["superseded"],
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("superseded memory events query should succeed");

    assert_eq!(queried.result[0]["scope"], "Repo");
    assert_eq!(queried.result[0]["action"], "Superseded");
    assert_eq!(
        queried.result[0]["supersedes"][0],
        "memory:legacy-routing-note"
    );

    let payload = host
        .memory_resource_value(
            test_session(&host).as_ref(),
            &MemoryId(result.memory_id.clone()),
        )
        .expect("memory resource should load");
    assert_eq!(payload.history.len(), 1);
    assert_eq!(payload.history[0].action, "Superseded");
    assert_eq!(
        payload.history[0].supersedes[0],
        "memory:legacy-routing-note"
    );
}

#[test]
fn repo_memory_store_rejects_duplicate_active_publication_without_supersedes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let first = host
        .store_memory(
            session.as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership belongs in committed repo knowledge",
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory-duplicate".to_string()),
            },
        )
        .expect("first repo memory should persist");

    let error = host
        .store_memory(
            session.as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership belongs in committed repo knowledge",
                    "trust": 0.92
                }),
                task_id: Some("task:repo-memory-duplicate".to_string()),
            },
        )
        .expect_err("duplicate active repo memory should be rejected");

    assert!(error
        .to_string()
        .contains("duplicates active published memory"));
    assert!(error.to_string().contains(&first.memory_id));
    assert!(error.to_string().contains("supersedes"));
}

#[test]
fn repo_memory_supersede_reloads_live_snapshot() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let original = host
        .store_memory(
            session.as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership follows the old contract wording",
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory-supersede".to_string()),
            },
        )
        .expect("original repo memory should persist");

    let replacement = host
        .store_memory(
            session.as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership follows the reviewed contract wording",
                    "supersedes": [original.memory_id.clone()],
                    "trust": 0.93
                }),
                task_id: Some("task:repo-memory-supersede".to_string()),
            },
        )
        .expect("replacement repo memory should persist");

    assert!(session
        .notes
        .entry(&MemoryId(original.memory_id.clone()))
        .is_none());
    assert!(session
        .notes
        .entry(&MemoryId(replacement.memory_id.clone()))
        .is_some());

    let original_payload = host
        .memory_resource_value(session.as_ref(), &MemoryId(original.memory_id.clone()))
        .expect("superseded memory resource should still load from history");
    assert_eq!(original_payload.memory.id, original.memory_id);
    assert_eq!(original_payload.history.len(), 1);
}

#[test]
fn repo_memory_retire_removes_live_entry_and_keeps_history_resource() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    let stored = host
        .store_memory(
            session.as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }],
                    "kind": "structural",
                    "scope": "repo",
                    "content": "alpha routing ownership was published before the boundary rewrite",
                    "trust": 0.9
                }),
                task_id: Some("task:repo-memory-retire".to_string()),
            },
        )
        .expect("repo memory should persist");

    host.store_memory(
        session.as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Retire,
            payload: json!({
                "memoryId": stored.memory_id.clone(),
                "retirementReason": "Boundary rewrite replaced this published routing rule."
            }),
            task_id: Some("task:repo-memory-retire".to_string()),
        },
    )
    .expect("repo memory retire should succeed");

    assert!(session
        .notes
        .entry(&MemoryId(stored.memory_id.clone()))
        .is_none());

    let queried = host
        .execute(
            session.clone(),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.events({
  focus: sym ? [sym] : [],
  scope: "repo",
  actions: ["retired"],
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("retired memory events query should succeed");
    assert_eq!(queried.result[0]["action"], "Retired");

    let payload = host
        .memory_resource_value(session.as_ref(), &MemoryId(stored.memory_id.clone()))
        .expect("retired memory resource should load from history");
    assert_eq!(payload.memory.id, stored.memory_id);
    assert_eq!(payload.memory.metadata["publication"]["status"], "retired");
    assert_eq!(payload.history[0].action, "Retired");
    assert_eq!(
        payload.history[0]
            .entry
            .as_ref()
            .expect("retired event should keep the entry payload")
            .metadata["publication"]["retirementReason"],
        "Boundary rewrite replaced this published routing rule."
    );
}

#[test]
fn repo_memory_store_rejects_weak_published_memory() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let error = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [],
                    "kind": "episodic",
                    "scope": "repo",
                    "content": "short memory",
                    "trust": 0.5
                }),
                task_id: Some("task:weak-memory".to_string()),
            },
        )
        .expect_err("weak repo memory should be rejected");

    assert!(error.to_string().contains("repo-published memory"));
}

#[test]
fn memory_resource_related_resources_include_file_anchor_links() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "file",
                        "path": "src/lib.rs"
                    }],
                    "kind": "episodic",
                    "scope": "session",
                    "content": "File-anchored memory for resource-link safety.",
                    "trust": 0.82
                }),
                task_id: Some("task:file-anchor-memory".to_string()),
            },
        )
        .expect("memory should persist");

    let payload = host
        .memory_resource_value(
            test_session(&host).as_ref(),
            &MemoryId(result.memory_id.clone()),
        )
        .expect("memory resource should load");
    let expected_uri = file_resource_uri("src/lib.rs");
    assert!(payload
        .related_resources
        .iter()
        .any(|resource| resource.uri == expected_uri));
}

#[test]
fn validation_feedback_mutation_persists_to_workspace_log() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::alpha".to_string(),
                    kind: "function".to_string(),
                }]),
                context: "blast-radius check for alpha".to_string(),
                prism_said: "Prism only reported alpha".to_string(),
                actually_true: "beta was also affected through the call graph".to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Wrong,
                corrected_manually: Some(true),
                correction: Some("checked callers directly and updated the plan".to_string()),
                metadata: Some(json!({
                    "query": "prism.blastRadius(alpha)",
                })),
                task_id: Some("task:feedback".to_string()),
            },
        )
        .expect("validation feedback should persist");

    assert!(result.entry_id.starts_with("feedback:"));
    assert_eq!(result.task_id, "task:feedback");

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].context, "blast-radius check for alpha");
    assert_eq!(entries[0].metadata["query"], "prism.blastRadius(alpha)");
}

#[test]
fn validation_feedback_mutation_allows_workspace_level_feedback_without_anchors() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: None,
                context: "compact locate docs intent dogfood".to_string(),
                prism_said: "taskIntent `docs` was rejected".to_string(),
                actually_true:
                    "docs paths should be easy to inspect without fabricating a symbol anchor"
                        .to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Mixed,
                corrected_manually: Some(true),
                correction: Some("reran locate with inspect semantics".to_string()),
                metadata: Some(json!({
                    "tool": "prism_locate",
                    "scope": "workspace",
                })),
                task_id: Some("task:feedback".to_string()),
            },
        )
        .expect("anchorless validation feedback should persist");

    assert!(result.entry_id.starts_with("feedback:"));

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].anchors.is_empty());
    assert_eq!(entries[0].metadata["tool"], "prism_locate");
}

#[test]
fn validation_feedback_mutation_accepts_file_anchor_paths() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: Some(vec![AnchorRefInput::File {
                    file_id: None,
                    path: Some("src/lib.rs".to_string()),
                }]),
                context: "direct file-anchor dogfood".to_string(),
                prism_said: "file anchors require internal ids".to_string(),
                actually_true: "workspace-relative file paths should resolve safely".to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Wrong,
                corrected_manually: Some(true),
                correction: Some("resolved the file through the workspace index".to_string()),
                metadata: Some(json!({
                    "tool": "prism_mutate",
                    "action": "validation_feedback",
                })),
                task_id: Some("task:file-anchor-feedback".to_string()),
            },
        )
        .expect("file path anchors should persist");

    assert!(result.entry_id.starts_with("feedback:"));

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].anchors, vec![AnchorRef::File(FileId(1))]);
    assert_eq!(entries[0].metadata["action"], "validation_feedback");
}

#[test]
fn validation_feedback_mutation_accepts_unsupported_text_file_anchor_paths() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("www/dashboard/src")).unwrap();
    fs::write(
        root.join("www/dashboard/src/App.tsx"),
        "export const app = 1;\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .store_validation_feedback(
            test_session(&host).as_ref(),
            PrismValidationFeedbackArgs {
                anchors: Some(vec![AnchorRefInput::File {
                    file_id: None,
                    path: Some("www/dashboard/src/App.tsx".to_string()),
                }]),
                context: "frontend file-anchor dogfood".to_string(),
                prism_said: "frontend files must already be parser-indexed".to_string(),
                actually_true:
                    "unsupported text files should still resolve as workspace file anchors"
                        .to_string(),
                category: ValidationFeedbackCategoryInput::Projection,
                verdict: ValidationFeedbackVerdictInput::Wrong,
                corrected_manually: Some(false),
                correction: None,
                metadata: Some(json!({
                    "tool": "prism_mutate",
                    "action": "validation_feedback",
                    "surface": "frontend",
                })),
                task_id: Some("task:frontend-file-anchor-feedback".to_string()),
            },
        )
        .expect("unsupported text file path anchors should persist");

    assert!(result.entry_id.starts_with("feedback:"));

    let reloaded = index_workspace_session(&root).unwrap();
    let app_path = root
        .join("www/dashboard/src/App.tsx")
        .canonicalize()
        .unwrap();
    let entries = reloaded.validation_feedback(Some(5)).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0].anchors[0],
        AnchorRef::File(file_id)
            if reloaded
                .prism()
                .graph()
                .file_path(*file_id)
                .is_some_and(|path| path == &app_path)
    ));
}

#[test]
fn validation_feedback_query_reads_internal_feedback_stream() {
    let root = temp_workspace();
    let workspace = index_workspace_session(&root).unwrap();
    workspace
        .append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some("task:feedback".to_string()),
            context: "session behavioral-owner dogfood".to_string(),
            anchors: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            prism_said: "dashboard and schema helpers ranked too high".to_string(),
            actually_true: "deeper runtime owners should rank first".to_string(),
            category: ValidationFeedbackCategory::Projection,
            verdict: ValidationFeedbackVerdict::Helpful,
            corrected_manually: true,
            correction: Some("tightened behavioral penalties".to_string()),
            metadata: json!({ "query": "prism.search(\"session\")" }),
        })
        .expect("feedback append should succeed");
    let host = host_with_session_internal(workspace);

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.validationFeedback({
  limit: 5,
  category: "projection",
  verdict: "helpful",
  contains: "session",
  correctedManually: true,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("validation feedback query should succeed");

    let entries = result
        .result
        .as_array()
        .expect("feedback results should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "projection");
    assert_eq!(entries[0]["verdict"], "helpful");
    assert_eq!(entries[0]["taskId"], "task:feedback");
    assert_eq!(entries[0]["correctedManually"], true);
    assert_eq!(entries[0]["metadata"]["query"], "prism.search(\"session\")");
    assert!(entries[0]["anchors"][0].to_string().contains("demo::alpha"));
}

#[test]
fn auto_refreshes_workspace_and_records_patch_events() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
    )
    .unwrap();

    host.observe_workspace_for_read().unwrap();
    wait_until("background workspace refresh after external edit", || {
        host.execute(
            test_session(&host),
            r#"
const sym = prism.symbol("gamma");
const alpha = prism.symbol("alpha");
return {
  path: sym?.id.path,
  callers: alpha ? alpha.relations().callees.map((node) => node.id.path) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .map(|result| result.result["path"] == Value::String("demo::gamma".to_string()))
        .unwrap_or(false)
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("gamma");
const alpha = prism.symbol("alpha");
return {
  path: sym?.id.path,
  callers: alpha ? alpha.relations().callees.map((node) => node.id.path) : [],
};
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed after background refresh");

    assert_eq!(result.result["path"], "demo::gamma");
    assert!(result.result["callers"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::gamma"));

    let patch_events = host
        .current_prism()
        .outcome_memory()
        .outcomes_for(
            &[AnchorRef::Node(NodeId::new(
                "demo",
                "demo::gamma",
                NodeKind::Function,
            ))],
            10,
        )
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .collect::<Vec<_>>();
    assert_eq!(patch_events.len(), 1);
}

#[test]
fn unchanged_query_skips_workspace_refresh() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");

    assert_eq!(workspace.observed_fs_revision(), 0);
    assert_eq!(workspace.applied_fs_revision(), 0);

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.symbol("alpha")?.id.path ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed without a refresh");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert_eq!(workspace.observed_fs_revision(), 0);
    assert_eq!(workspace.applied_fs_revision(), 0);
}

#[test]
fn queries_skip_request_path_persisted_reload_when_runtime_is_current() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let started = Instant::now();
    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.symbol("alpha")?.id.path ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed while refresh lock is held");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "query spent too long in request-path refresh work"
    );

    let trace = host
        .query_trace_view(
            &host.query_log_entries(QueryLogArgs {
                limit: Some(1),
                since: None,
                target: None,
                operation: Some("typescript.refreshWorkspace".to_string()),
                task_id: None,
                min_duration_ms: None,
            })[0]
                .id,
        )
        .expect("query trace should exist");
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "typescript.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    let refresh_path = args
        .get("refreshPath")
        .and_then(Value::as_str)
        .expect("refreshPath should be a string");
    assert!(matches!(refresh_path, "none" | "deferred"));
    let metrics = args
        .get("metrics")
        .and_then(Value::as_object)
        .expect("refresh metrics");
    let reload_work = metrics
        .get("reloadWork")
        .and_then(Value::as_object)
        .expect("refresh reload-work metrics");
    assert_eq!(
        reload_work.get("loadedBytes"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("replayVolume"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("fullRebuildCount"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(metrics.get("fsRefreshMs"), Some(&Value::Number(0.into())));
    assert_eq!(
        reload_work.get("workspaceReloaded"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn queries_defer_request_path_refresh_when_runtime_sync_is_busy() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let _sync_guard = host
        .workspace_runtime_sync_lock
        .lock()
        .expect("workspace runtime sync lock should be available");

    let started = Instant::now();
    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.symbol("alpha")?.id.path ?? null;
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed while workspace sync is busy");

    assert_eq!(result.result, Value::String("demo::alpha".to_string()));
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "query spent too long waiting on the workspace runtime sync lock"
    );

    let trace = host
        .query_trace_view(
            &host.query_log_entries(QueryLogArgs {
                limit: Some(1),
                since: None,
                target: None,
                operation: Some("typescript.refreshWorkspace".to_string()),
                task_id: None,
                min_duration_ms: None,
            })[0]
                .id,
        )
        .expect("query trace should exist");
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "typescript.refreshWorkspace")
        .expect("refresh phase should exist");
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("deferred".to_string()))
    );
    let metrics = args
        .get("metrics")
        .and_then(Value::as_object)
        .expect("refresh metrics");
    let reload_work = metrics
        .get("reloadWork")
        .and_then(Value::as_object)
        .expect("refresh reload-work metrics");
    assert_eq!(metrics.get("lockWaitMs"), Some(&Value::Number(0.into())));
    assert_eq!(metrics.get("lockHoldMs"), Some(&Value::Number(0.into())));
    assert_eq!(
        reload_work.get("loadedBytes"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("replayVolume"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(
        reload_work.get("fullRebuildCount"),
        Some(&Value::Number(0.into()))
    );
    assert_eq!(metrics.get("fsRefreshMs"), Some(&Value::Number(0.into())));
    assert_eq!(
        reload_work.get("workspaceReloaded"),
        Some(&Value::Bool(false))
    );

    let freshness = crate::runtime_views::runtime_status(&host)
        .expect("runtime status should succeed while refresh is deferred")
        .freshness;
    assert_eq!(freshness.status, "deferred");
    assert_eq!(freshness.last_refresh_path.as_deref(), Some("deferred"));
}

#[test]
fn persisted_only_mutations_fail_fast_when_runtime_sync_is_busy() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );
    let _sync_guard = server
        .host
        .workspace_runtime_sync_lock
        .lock()
        .expect("workspace runtime sync lock should be available");

    let started = Instant::now();
    let error = server
        .execute_logged_mutation(
            "mutate.outcome",
            MutationRefreshPolicy::PersistedOnly,
            || {
                server.host.store_outcome_without_refresh(
                    test_session(&server.host).as_ref(),
                    PrismOutcomeArgs {
                        kind: OutcomeKindInput::FixValidated,
                        anchors: vec![AnchorRefInput::Node {
                            crate_name: "demo".to_string(),
                            path: "demo::alpha".to_string(),
                            kind: "function".to_string(),
                        }],
                        summary: "validated alpha".to_string(),
                        result: Some(OutcomeResultInput::Success),
                        evidence: None,
                        task_id: None,
                    },
                )
            },
            |result| {
                MutationDashboardMeta::task(
                    Some(result.task_id.clone()),
                    vec![result.task_id.clone(), result.event_id.clone()],
                    0,
                )
            },
        )
        .expect_err("persisted-only mutation should fail fast while runtime sync is busy");

    assert!(
        started.elapsed() < Duration::from_millis(200),
        "persisted-only mutation spent too long waiting on the runtime sync lock"
    );
    assert!(error.to_string().contains("request admission busy"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.refreshWorkspace")
        .expect("refresh phase should exist");
    assert!(!refresh_phase.success);
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args should exist");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("busy".to_string()))
    );
}

#[test]
fn coordination_mutations_fail_fast_when_runtime_sync_is_busy() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );
    let _sync_guard = server
        .host
        .workspace_runtime_sync_lock
        .lock()
        .expect("workspace runtime sync lock should be available");

    let started = Instant::now();
    let error = server
        .execute_logged_mutation_with_run(
            "mutate.coordination",
            MutationRefreshPolicy::None,
            |run| {
                server.host.store_coordination_traced(
                    test_session(&server.host).as_ref(),
                    PrismCoordinationArgs {
                        kind: CoordinationMutationKindInput::PlanCreate,
                        payload: json!({ "goal": "Fail fast when runtime sync is busy" }),
                        task_id: None,
                    },
                    run,
                )
            },
            |result| MutationDashboardMeta::coordination(result.event_ids.clone(), 0),
        )
        .expect_err("coordination mutation should fail fast while runtime sync is busy");

    assert!(
        started.elapsed() < Duration::from_millis(200),
        "coordination mutation spent too long waiting on the runtime sync lock"
    );
    assert!(error.to_string().contains("request admission busy"));

    let detail = server
        .host
        .dashboard_operation_detail("mutation:1")
        .expect("mutation detail should exist");
    let crate::dashboard_types::DashboardOperationDetailView::Mutation { trace } = detail else {
        panic!("expected mutation trace");
    };
    let refresh_phase = trace
        .phases
        .iter()
        .find(|phase| phase.operation == "mutation.coordination.refreshWorkspace")
        .expect("coordination refresh phase should exist");
    assert!(!refresh_phase.success);
    let args = refresh_phase
        .args_summary
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh args should exist");
    assert_eq!(
        args.get("refreshPath"),
        Some(&Value::String("busy".to_string()))
    );
}

#[test]
fn claim_mutations_fail_fast_when_runtime_sync_is_busy() {
    let root = temp_workspace();
    let server = PrismMcpServer::with_session_and_features(
        index_workspace_session(&root).unwrap(),
        PrismMcpFeatures::full().with_internal_developer(true),
    );
    let plan = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Claim admission busy coverage" }),
                task_id: None,
            },
        )
        .expect("plan creation should succeed");
    let task = server
        .host
        .store_coordination(
            test_session(&server.host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Acquire edit claim",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .expect("task creation should succeed");

    let _sync_guard = server
        .host
        .workspace_runtime_sync_lock
        .lock()
        .expect("workspace runtime sync lock should be available");
    let started = Instant::now();
    let error = server
        .host
        .store_claim(
            test_session(&server.host).as_ref(),
            PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task.state["id"].as_str().unwrap()
                }),
                task_id: None,
            },
        )
        .expect_err("claim mutation should fail fast while runtime sync is busy");

    assert!(
        started.elapsed() < Duration::from_millis(200),
        "claim mutation spent too long waiting on the runtime sync lock"
    );
    assert!(error.to_string().contains("request admission busy"));
}

#[test]
fn runtime_status_reports_workspace_materialization_depth_and_coverage() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("web")).unwrap();
    fs::write(root.join("web/app.js"), "export const alpha = 1;\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let freshness = crate::runtime_views::runtime_status(&host)
        .expect("runtime status should succeed")
        .freshness;
    let workspace = freshness.materialization.workspace;

    assert_eq!(workspace.status, "current");
    assert_eq!(workspace.depth, "medium");

    let coverage = workspace.coverage.expect("workspace coverage should exist");
    assert!(coverage.known_files >= coverage.materialized_files);
    assert!(coverage.known_directories > 0);
    assert!(coverage.materialized_files > 0);
    assert!(coverage.materialized_nodes > 0);
    let boundary = workspace
        .boundaries
        .iter()
        .find(|boundary| boundary.id == "boundary:web:out_of_scope")
        .expect("out-of-scope boundary should exist");
    assert_eq!(boundary.path, "web");
    assert_eq!(boundary.provenance, "workspace_walk");
    assert_eq!(boundary.materialization_state, "out_of_scope");
    assert_eq!(boundary.scope_state, "out_of_scope");
}

#[test]
fn runtime_status_reports_projection_and_overlay_scopes() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn validation_recipe() {}
pub fn runtime_status() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());
    let session = test_session(&host);

    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://repo_runtime_scope".to_string()),
            canonical_name: Some("repo_runtime_scope".to_string()),
            summary: Some("Published runtime scope concept.".to_string()),
            aliases: Some(vec!["runtime scope".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Published for runtime scope reporting.".to_string()]),
            risk_hint: None,
            confidence: Some(0.93),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Repo),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:repo-runtime-scope".to_string()),
        },
    )
    .unwrap();
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://session_runtime_scope".to_string()),
            canonical_name: Some("session_runtime_scope".to_string()),
            summary: Some("Session runtime scope concept.".to_string()),
            aliases: Some(vec!["session runtime".to_string()]),
            core_members: Some(vec![
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::validation_recipe".to_string(),
                    kind: "function".to_string(),
                },
                NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::runtime_status".to_string(),
                    kind: "function".to_string(),
                },
            ]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Persisted for session scope reporting.".to_string()]),
            risk_hint: None,
            confidence: Some(0.86),
            decode_lenses: Some(vec![PrismConceptLensInput::Validation]),
            scope: Some(ConceptScopeInput::Session),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:session-runtime-scope".to_string()),
        },
    )
    .unwrap();
    host.store_concept(
        session.as_ref(),
        PrismConceptMutationArgs {
            operation: ConceptMutationOperationInput::Promote,
            handle: Some("concept://local_runtime_scope".to_string()),
            canonical_name: Some("local_runtime_scope".to_string()),
            summary: Some("Worktree runtime scope concept.".to_string()),
            aliases: Some(vec!["local runtime".to_string()]),
            core_members: Some(vec![NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::validation_recipe".to_string(),
                kind: "function".to_string(),
            }]),
            supporting_members: None,
            likely_tests: None,
            evidence: Some(vec!["Retained only for the current worktree.".to_string()]),
            risk_hint: None,
            confidence: Some(0.6),
            decode_lenses: Some(vec![PrismConceptLensInput::Open]),
            scope: Some(ConceptScopeInput::Local),
            supersedes: None,
            retirement_reason: None,
            task_id: Some("task:local-runtime-scope".to_string()),
        },
    )
    .unwrap();

    let plan = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Track scoped runtime overlays" }),
                task_id: None,
            },
        )
        .unwrap();
    host.store_coordination(
        session.as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::TaskCreate,
            payload: json!({
                "planId": plan.state["id"].as_str().unwrap(),
                "title": "Inspect runtime scope overlays",
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::runtime_status",
                    "kind": "function"
                }]
            }),
            task_id: None,
        },
    )
    .unwrap();

    let status =
        crate::runtime_views::runtime_status(&host).expect("runtime status should succeed");
    let repo_projection = status
        .scopes
        .projections
        .iter()
        .find(|scope| scope.scope == "repo")
        .expect("repo projection scope should exist");
    let worktree_projection = status
        .scopes
        .projections
        .iter()
        .find(|scope| scope.scope == "worktree")
        .expect("worktree projection scope should exist");
    let session_projection = status
        .scopes
        .projections
        .iter()
        .find(|scope| scope.scope == "session")
        .expect("session projection scope should exist");
    assert_eq!(repo_projection.concept_count, 1);
    assert_eq!(worktree_projection.concept_count, 1);
    assert_eq!(session_projection.concept_count, 1);
    assert!(worktree_projection.co_change_lineage_count > 0);

    let repo_overlay = status
        .scopes
        .overlays
        .iter()
        .find(|scope| scope.scope == "repo")
        .expect("repo overlay scope should exist");
    let worktree_overlay = status
        .scopes
        .overlays
        .iter()
        .find(|scope| scope.scope == "worktree")
        .expect("worktree overlay scope should exist");
    let session_overlay = status
        .scopes
        .overlays
        .iter()
        .find(|scope| scope.scope == "session")
        .expect("session overlay scope should exist");
    assert_eq!(repo_overlay.plan_count, 1);
    assert!(repo_overlay.plan_node_count > 0);
    assert_eq!(worktree_overlay.overlay_count, 1);
    assert_eq!(session_overlay.overlay_count, 1);
}

#[test]
fn refresh_workspace_reloads_updated_persisted_notes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let stored = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::feature::alpha",
                        "kind": "function"
                    }],
                    "kind": "episodic",
                    "content": "alpha needs care during routing changes",
                    "trust": 0.8
                }),
                task_id: None,
            },
        )
        .expect("note should store");

    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");
    let initial_applied_fs_revision = workspace.applied_fs_revision();
    let initial_observed_fs_revision = workspace.observed_fs_revision();
    let mut snapshot = workspace
        .load_episodic_snapshot()
        .unwrap()
        .expect("persisted snapshot should exist");
    let entry = snapshot
        .entries
        .iter_mut()
        .find(|candidate| candidate.id.0 == stored.memory_id)
        .expect("stored note should be persisted");
    entry.anchors = vec![AnchorRef::Node(beta.clone())];
    workspace.persist_episodic(&snapshot).unwrap();

    host.refresh_workspace().unwrap();
    wait_until("persisted notes background reload", || {
        host.refresh_workspace().unwrap();
        test_session(&host)
            .notes
            .entry(&MemoryId(stored.memory_id.clone()))
            .is_some_and(|entry| entry.anchors.contains(&AnchorRef::Node(beta.clone())))
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("beta");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "routing changes",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed after snapshot reload");

    let entry = test_session(&host)
        .notes
        .entry(&MemoryId(stored.memory_id.clone()))
        .expect("stored note should remain in session memory");
    assert!(entry.anchors.contains(&AnchorRef::Node(beta)));

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(
        result.result[0]["entry"]["content"],
        "alpha needs care during routing changes"
    );
    assert_eq!(workspace.applied_fs_revision(), initial_applied_fs_revision);
    assert_eq!(
        workspace.observed_fs_revision(),
        initial_observed_fs_revision
    );

    let timeline = crate::runtime_views::runtime_timeline(
        &host,
        RuntimeTimelineArgs {
            limit: Some(10),
            contains: Some("workspace refresh".to_string()),
        },
    )
    .expect("runtime timeline should include refresh events");
    let refresh_fields = timeline[0]
        .fields
        .as_ref()
        .and_then(Value::as_object)
        .expect("refresh event fields");
    for key in [
        "lockWaitMs",
        "lockHoldMs",
        "fsRefreshMs",
        "snapshotRevisionsMs",
        "loadEpisodicMs",
        "loadInferenceMs",
        "loadCoordinationMs",
        "loadedBytes",
        "replayVolume",
        "fullRebuildCount",
        "workspaceReloaded",
    ] {
        assert!(
            refresh_fields.contains_key(key),
            "expected runtime refresh event to include `{key}`"
        );
    }
    assert_eq!(
        refresh_fields.get("workspaceReloaded"),
        Some(&Value::Bool(false))
    );
}

#[test]
fn refresh_workspace_reloads_updated_persisted_inference_without_fs_refresh() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    host.store_inferred_edge(
        test_session(&host).as_ref(),
        PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.95,
            scope: Some(InferredEdgeScopeInput::Persisted),
            evidence: Some(vec!["persisted inference".to_string()]),
            task_id: Some("task:persist".to_string()),
        },
    )
    .expect("inferred edge should persist");

    let workspace = host
        .workspace
        .as_ref()
        .expect("workspace-backed host expected");
    let initial_applied_fs_revision = workspace.applied_fs_revision();
    let initial_observed_fs_revision = workspace.observed_fs_revision();
    let mut snapshot = workspace
        .load_inference_snapshot()
        .unwrap()
        .unwrap_or(InferenceSnapshot {
            records: Vec::new(),
        });
    assert_eq!(snapshot.records.len(), 1);
    snapshot.records[0].edge.target = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    workspace.persist_inference(&snapshot).unwrap();

    host.refresh_workspace().unwrap();
    wait_until("persisted inference background reload", || {
        host.refresh_workspace().unwrap();
        host.execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .map(|result| {
            result
                .result
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .any(|value| value == "demo::alpha")
        })
        .unwrap_or(false)
    });

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed after inference reload");

    assert!(result
        .result
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .any(|value| value == "demo::alpha"));
    assert_eq!(workspace.applied_fs_revision(), initial_applied_fs_revision);
    assert_eq!(
        workspace.observed_fs_revision(),
        initial_observed_fs_revision
    );
}

#[test]
fn convenience_symbol_query_returns_diagnostics() {
    let host = host_with_node(demo_node());

    let envelope = host
        .symbol_query(test_session(&host), "missing")
        .expect("symbol query should succeed");
    assert!(envelope.result.is_object() || envelope.result.is_null());
    assert!(envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "anchor_unresolved"));
    assert!(envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "anchor_unresolved")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["suggestedQueries"].as_array())
        .is_some_and(|queries| !queries.is_empty()));
    assert!(envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "anchor_unresolved")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism.search")));
}

#[test]
fn convenience_search_query_returns_structured_envelope() {
    let host = host_with_node(demo_node());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "main".to_string(),
                limit: Some(1),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");
    assert!(envelope.result.is_array());
    assert!(envelope.diagnostics.is_empty());
}

#[test]
fn ambiguous_symbol_queries_surface_ranked_narrowing_context() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .symbol_query(test_session(&host), "helper")
        .expect("symbol query should succeed");
    let diagnostic = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ambiguous_symbol")
        .expect("ambiguity diagnostic should be present");
    let ambiguity = diagnostic
        .data
        .as_ref()
        .and_then(|data| data["ambiguity"].as_object())
        .expect("ambiguity payload should be present");

    assert_eq!(ambiguity["candidateCount"].as_u64(), Some(2));
    assert_eq!(
        ambiguity["returned"]["id"]["path"].as_str(),
        envelope.result["id"]["path"].as_str()
    );
    assert!(envelope.result["id"]["path"]
        .as_str()
        .is_some_and(|path| !path.contains("::tests::")));
    assert!(ambiguity["candidates"][0]["suggestedQueries"]
        .as_array()
        .is_some_and(|queries| !queries.is_empty()));
    assert!(ambiguity["suggestedQueries"]
        .as_array()
        .is_some_and(|queries| queries.iter().any(|query| {
            query["label"]
                .as_str()
                .is_some_and(|label| label == "Focused Block")
        })));
}

#[test]
fn search_supports_module_and_task_scope_narrowing() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(root.join("src/alpha.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let module_envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: Some("demo::beta".to_string()),
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("module search should succeed");
    assert_eq!(
        module_envelope
            .result
            .as_array()
            .map(|results| results.len()),
        Some(1)
    );
    assert_eq!(
        module_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
    );

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Investigate helper collision" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Inspect beta helper",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::beta::helper",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let task_envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: Some(task_id),
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("task-scoped search should succeed");
    assert_eq!(
        task_envelope.result.as_array().map(|results| results.len()),
        Some(1)
    );
    assert_eq!(
        task_envelope.result[0]["id"]["path"].as_str(),
        Some("demo::beta::helper")
    );
}

#[test]
fn broad_noun_queries_prefer_callable_code_over_module_collisions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
pub mod planner;
"#,
    )
    .unwrap();
    fs::write(root.join("src/helpers.rs"), "pub fn bootstrap() {}\n").unwrap();
    fs::write(
        root.join("src/planner.rs"),
        "pub fn build_helper_plan() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::planner::build_helper_plan")
    );
    assert!(
        envelope.diagnostics.is_empty()
            || envelope
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "ambiguous_search")
    );
}

#[test]
fn explicit_search_modes_prefer_callable_code_over_exact_module_collisions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod session;

pub fn session() {}
"#,
    )
    .unwrap();
    fs::write(root.join("src/session.rs"), "pub fn load() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Function");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session")
    );
}

#[test]
fn broad_noun_queries_prefer_non_test_code_over_test_name_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper_registry() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helper_registry")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results
            .iter()
            .any(|symbol| symbol["id"]["path"] == "demo::tests::helper")
    }));
}

#[test]
fn broad_noun_queries_overfetch_past_test_and_module_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod query_helpers;
pub mod discovery_helpers;

pub fn build_helper_plan() {}

#[cfg(test)]
mod tests {
    pub fn helper() {}
    pub fn bundle_helpers_keep_diagnostics_local_to_each_helper_call() {}
    pub fn discovery_helpers_surface_direct_where_used_and_entrypoints() {}
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_helpers.rs"),
        "pub fn hydrate_owner() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/discovery_helpers.rs"),
        "pub fn collect_related_owner() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(3),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::build_helper_plan")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("::tests::"))
        })
    }));
}

#[test]
fn broad_noun_queries_prefer_owner_module_over_path_inherited_helper_functions() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn anchor_sort_key() {}
pub fn conflict_between() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Module");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers")
    );
}

#[test]
fn broad_noun_queries_deprioritize_replay_fixture_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod query_replay_cases;

pub fn build_helper_plan() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_replay_cases.rs"),
        "pub fn helper() {}\n",
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::build_helper_plan")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results
            .iter()
            .any(|symbol| symbol["id"]["path"] == "demo::query_replay_cases::helper")
    }));
}

#[test]
fn broad_noun_queries_deprioritize_dependency_lockfile_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn helper() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{
  "packages": {
    "node_modules/@babel/helper-globals": {
      "version": "7.28.0"
    }
  }
}"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helper")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("@babel/helper-globals"))
        })
    }));
}

#[test]
fn broad_noun_queries_prefer_exact_modules_over_exact_field_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod session;

pub struct QueryHost {
    pub session: usize,
}
"#,
    )
    .unwrap();
    fs::write(root.join("src/session.rs"), "pub fn load() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: None,
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: None,
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Module");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session")
    );
}

#[test]
fn explicit_search_modes_can_prefer_behavioral_owners_without_behavioral_strategy() {
    let root = temp_workspace();
    write_memory_insight_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "memory recall".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert!(envelope
        .result
        .as_array()
        .is_some_and(|results| results.iter().take(3).any(|symbol| {
            symbol["ownerHint"]["kind"].as_str() == Some("read")
                && symbol["id"]["path"]
                    .as_str()
                    .is_some_and(|path| path.contains("memory_recall"))
        })));
}

#[test]
fn direct_search_merges_behavioral_owner_hints_for_same_symbol() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub fn session_view() {
    let current_session = read_session_state();
    assert!(!current_session.is_empty());
}

fn read_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let session_view = envelope
        .result
        .as_array()
        .and_then(|results| {
            results
                .iter()
                .find(|symbol| symbol["id"]["path"].as_str() == Some("demo::session_view"))
        })
        .expect("session_view should be returned");
    assert_eq!(session_view["ownerHint"]["kind"].as_str(), Some("read"));
}

#[test]
fn behavioral_owner_search_prefers_deeper_session_implementations_over_surface_wrappers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod dashboard;
mod resources;
mod runtime;

pub use dashboard::dashboard_session_view;
pub use resources::session_resource_link;
pub use runtime::load_session_memory;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

fn fetch_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/dashboard.rs"),
        r#"
pub fn dashboard_session_view() -> &'static str {
    "session"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let top_paths = envelope
        .result
        .as_array()
        .expect("behavioral search should return an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        top_paths
            .iter()
            .all(|path| path.starts_with("demo::runtime::")),
        "expected deeper runtime implementations to outrank surface wrappers, got {top_paths:?}"
    );
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::resources::session_resource_link"
                || path == &"demo::dashboard::dashboard_session_view"),
        "expected surface wrappers to rank below runtime implementations, got {top_paths:?}"
    );
}

#[test]
fn behavioral_owner_search_deprioritizes_schema_examples_for_broad_read_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod resources;
mod runtime;
mod schema_examples;

pub use resources::session_resource_link;
pub use runtime::{fetch_session_state, load_session_history, load_session_memory};
pub use schema_examples::session_payload_example;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

pub fn fetch_session_state() -> &'static str {
    "ready"
}

pub fn load_session_history() -> &'static str {
    fetch_session_state()
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r#"
pub fn session_payload_example() -> &'static str {
    "{\"currentTask\":\"demo-session\"}"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(6),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let top_paths = envelope
        .result
        .as_array()
        .expect("behavioral search should return an array")
        .iter()
        .take(3)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        top_paths
            .iter()
            .all(|path| path.starts_with("demo::runtime::")),
        "expected runtime implementations to outrank schema/resource surfaces, got {top_paths:?}"
    );
}

#[test]
fn ambiguity_payload_exposes_buckets_for_broad_first_hop_queries() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod dashboard;
mod resources;
mod runtime;
mod schema_examples;

pub use dashboard::dashboard_session_view;
pub use resources::session_resource_link;
pub use runtime::load_session_memory;
pub use schema_examples::session_payload_example;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn load_session_memory() {
    let session_state = fetch_session_state();
    assert!(!session_state.is_empty());
}

fn fetch_session_state() -> &'static str {
    "ready"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/resources.rs"),
        r#"
pub fn session_resource_link() -> &'static str {
    "/session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/dashboard.rs"),
        r#"
pub fn dashboard_session_view() -> &'static str {
    "session"
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r#"
pub fn session_payload_example() -> &'static str {
    "{\"currentTask\":\"demo-session\"}"
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(8),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("behavioral".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: None,
                prefer_editable_targets: None,
                prefer_behavioral_owners: None,
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    let ambiguity = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ambiguous_search")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["ambiguity"].as_object())
        .expect("ambiguous search payload should be present");

    let why = ambiguity["why"].as_array().expect("why should be an array");
    assert!(why.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|line| line.contains("Broad-query bucketing grouped candidates"))
    }));

    let candidates = ambiguity["candidates"]
        .as_array()
        .expect("ambiguity candidates should be an array");
    assert_eq!(candidates[0]["bucket"], "implementation");
    assert!(candidates
        .iter()
        .any(|candidate| candidate["bucket"] == "surface"));
    assert!(candidates
        .iter()
        .any(|candidate| candidate["bucket"] != "implementation"));
}

#[test]
fn behavioral_owner_search_ignores_excerpt_only_schema_example_noise() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod runtime;
pub mod schema_examples;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/runtime.rs"),
        r#"
pub fn lookup_session_state() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/schema_examples.rs"),
        r##"
pub fn read_payload_example() {
    let _payload = r#"{"helper":true}"#;
}
"##,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: None,
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::runtime::lookup_session_state")
    );
    assert!(envelope.result.as_array().is_some_and(|results| {
        !results.iter().any(|symbol| {
            symbol["id"]["path"]
                .as_str()
                .is_some_and(|path| path.contains("schema_examples"))
        })
    }));
}

#[test]
fn explicit_callable_behavioral_owner_search_prefers_functions_over_exact_module_nouns() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(envelope.result[0]["kind"], "Function");
    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
}

#[test]
fn broad_helper_queries_deprioritize_generic_query_helpers_utilities() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
pub mod query_helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/query_helpers.rs"),
        r#"
pub fn compact_owner_candidate_excerpts() {}
pub fn helper_candidate_summary() {}
pub fn next_reads() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path.starts_with("demo::query_helpers::")),
        "expected generic query_helpers utilities to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_deprioritize_low_level_helpers_module_utilities() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}

pub fn anchor_sort_key() {}
pub fn conflict_between() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::helpers::anchor_sort_key"
                || path == &"demo::helpers::conflict_between"),
        "expected low-level helpers.rs utilities to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_deprioritize_internal_helpers_module_plumbing() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub fn lookup_registry() {
    let helper_mode = true;
    assert!(helper_mode);
}

pub(crate) fn derived_event_meta() {}
fn expire_claims_locked() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::helpers::lookup_registry")
    );
    let top_paths = envelope
        .result
        .as_array()
        .expect("search results should be an array")
        .iter()
        .take(2)
        .filter_map(|value| value["id"]["path"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !top_paths
            .iter()
            .any(|path| path == &"demo::helpers::derived_event_meta"
                || path == &"demo::helpers::expire_claims_locked"),
        "expected internal helpers.rs plumbing to rank below task-facing helper code, got {top_paths:?}"
    );
}

#[test]
fn broad_helper_queries_surface_weak_match_diagnostics_for_internal_modules() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod helpers;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/helpers.rs"),
        r#"
pub(crate) fn derived_event_meta() {}
fn expire_claims_locked() {}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "helper".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert!(envelope
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "weak_search_match"));
    let weak = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "weak_search_match")
        .expect("weak_search_match diagnostic should be present");
    assert!(weak
        .data
        .as_ref()
        .and_then(|data| data["reason"].as_str())
        .is_some_and(|reason| !reason.trim().is_empty()));
}

#[test]
fn broad_implementation_search_deprioritizes_lib_rs_facade_wrappers() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod session;

pub fn index_workspace_session() {
    session::load_session_state();
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/session.rs"),
        r#"
pub fn load_session_state() {
    let session_state = "ready";
    assert!(!session_state.is_empty());
}
"#,
    )
    .unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let envelope = host
        .search_query(
            test_session(&host),
            SearchArgs {
                query: "session".to_string(),
                limit: Some(5),
                kind: None,
                path: None,
                module: None,
                task_id: None,
                path_mode: None,
                strategy: Some("direct".to_string()),
                structured_path: None,
                top_level_only: None,
                prefer_callable_code: Some(true),
                prefer_editable_targets: Some(true),
                prefer_behavioral_owners: Some(true),
                owner_kind: Some("read".to_string()),
                include_inferred: None,
            },
        )
        .expect("search query should succeed");

    assert_eq!(
        envelope.result[0]["id"]["path"].as_str(),
        Some("demo::session::load_session_state")
    );
}

#[test]
fn file_around_truncation_emits_actionable_diagnostic() {
    let root = temp_workspace();
    write_long_excerpt_workspace(&root);
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.file("src/recall.rs").around({
  line: 8,
  before: 0,
  after: 20,
  maxChars: 80,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");

    assert_eq!(result.result["truncated"], true);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "result_truncated"
                && diagnostic
                    .data
                    .as_ref()
                    .and_then(|data| data.get("operation"))
                    .and_then(Value::as_str)
                    == Some("fileAround")
        })
        .expect("fileAround truncation diagnostic");
    assert!(diagnostic.message.contains("raise `maxChars`"));
    assert_eq!(
        diagnostic
            .data
            .as_ref()
            .and_then(|data| data.get("maxChars"))
            .and_then(Value::as_u64),
        Some(80)
    );
}

#[test]
fn search_resource_payload_surfaces_ambiguity_context() {
    let root = temp_workspace();
    fs::write(
        root.join("src/lib.rs"),
        r#"
pub mod alpha;
pub mod beta;
"#,
    )
    .unwrap();
    fs::write(root.join("src/alpha.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(root.join("src/beta.rs"), "pub fn helper() {}\n").unwrap();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let payload = host
        .search_resource_value(test_session(&host), "prism://search/helper", "helper")
        .expect("search resource should succeed");

    assert!(payload.ambiguity.is_some());
    assert_eq!(
        payload
            .ambiguity
            .as_ref()
            .and_then(|ambiguity| ambiguity.returned.as_ref())
            .map(|symbol| symbol.id.path.as_str()),
        payload
            .results
            .first()
            .map(|symbol| symbol.id.path.as_str())
    );
    assert!(payload
        .suggested_queries
        .iter()
        .any(|query| query.label == "Focused Block"));
}

#[test]
fn plans_resource_payload_surfaces_filters_and_root_nodes() {
    let host = host_with_node(demo_node());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Migrate persistence storage semantics" }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let root_node = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanNodeCreate,
                payload: json!({ "planId": plan_id, "title": "Classify authoritative tables" }),
                task_id: None,
            },
        )
        .unwrap();

    let payload = host
        .plans_resource_value(
            test_session(&host),
            "prism://plans?contains=persistence&limit=1",
        )
        .expect("plans resource should succeed");

    assert_eq!(payload.contains.as_deref(), Some("persistence"));
    assert_eq!(payload.page.returned, 1);
    assert_eq!(payload.plans.len(), 1);
    assert_eq!(payload.plans[0].summary.actionable_nodes, 1);
    assert!(payload
        .related_resources
        .iter()
        .any(|link| link.uri == "prism://plans?contains=persistence"));
    assert_eq!(
        payload.plans[0].root_node_ids,
        vec![root_node.state["id"].as_str().unwrap()]
    );
}

#[test]
fn plans_resource_contains_filter_matches_singular_and_plural_terms() {
    let host = host_with_node(demo_node());

    host.store_coordination(
        test_session(&host).as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({ "goal": "Burn down the last refresh bottleneck" }),
            task_id: None,
        },
    )
    .unwrap();

    let payload = host
        .plans_resource_value(
            test_session(&host),
            "prism://plans?contains=bottlenecks&limit=5",
        )
        .expect("plans resource should succeed");

    assert_eq!(payload.contains.as_deref(), Some("bottlenecks"));
    assert_eq!(payload.plans.len(), 1);
    assert!(payload.plans[0]
        .title
        .to_ascii_lowercase()
        .contains("bottleneck"));
}

#[test]
fn first_mutation_auto_creates_session_task() {
    let host = host_with_node(demo_node());

    let memory = host
        .store_memory(
            test_session(&host).as_ref(),
            PrismMemoryArgs {
                action: MemoryMutationActionInput::Store,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "kind": "episodic",
                    "content": "remember this",
                    "trust": 0.8
                }),
                task_id: None,
            },
        )
        .expect("note should store");

    assert!(memory.memory_id.starts_with("memory:"));
    let task = test_session(&host)
        .current_task()
        .expect("task should be created");
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.task, task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);
}

#[test]
fn recalls_session_memory_for_symbol_focus() {
    let host = host_with_node(demo_node());

    host.store_memory(
        test_session(&host).as_ref(),
        PrismMemoryArgs {
            action: MemoryMutationActionInput::Store,
            payload: json!({
                "anchors": [{
                    "type": "node",
                    "crateName": "demo",
                    "path": "demo::main",
                    "kind": "function"
                }],
                "kind": "episodic",
                "content": "main previously regressed on null handling",
                "trust": 0.9
            }),
            task_id: None,
        },
    )
    .expect("note should store");

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "null",
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    assert_eq!(
        result.result[0]["entry"]["content"],
        "main previously regressed on null handling"
    );
    assert_eq!(result.result[0]["entry"]["kind"], "Episodic");
}

#[test]
fn memory_recall_respects_kinds_and_since_filters() {
    let host = host_with_node(demo_node());

    let mut old_structural =
        MemoryEntry::new(MemoryKind::Structural, "main changes require a migration");
    old_structural.anchors = vec![AnchorRef::Node(NodeId::new(
        "demo",
        "demo::main",
        NodeKind::Function,
    ))];
    old_structural.created_at = 10;
    old_structural.source = MemorySource::User;
    old_structural.trust = 1.0;
    test_session(&host).notes.store(old_structural).unwrap();

    let mut fresh_semantic =
        MemoryEntry::new(MemoryKind::Semantic, "main often flakes during retries");
    fresh_semantic.anchors = vec![AnchorRef::Node(NodeId::new(
        "demo",
        "demo::main",
        NodeKind::Function,
    ))];
    fresh_semantic.created_at = 50;
    fresh_semantic.source = MemorySource::System;
    fresh_semantic.trust = 0.8;
    test_session(&host).notes.store(fresh_semantic).unwrap();

    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  kinds: ["semantic"],
  since: 20,
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory recall should succeed");

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(result.result[0]["entry"]["kind"], "Semantic");
    assert_eq!(
        result.result[0]["entry"]["content"],
        "main often flakes during retries"
    );
}

#[test]
fn memory_outcomes_support_filtered_history_queries() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);

    let mut graph = Graph::new();
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone()]);

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:1"),
                ts: 5,
                actor: EventActor::System,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "system saw alpha fail".into(),
            evidence: vec![],
            metadata: Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha:2"),
                ts: 15,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "agent saw alpha fail".into(),
            evidence: vec![],
            metadata: Value::Null,
        })
        .unwrap();

    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("alpha");
return prism.memory.outcomes({
  focus: sym ? [sym] : [],
  taskId: "task:alpha",
  kinds: ["failure"],
  result: "failure",
  actor: "agent",
  since: 10,
  limit: 5,
});
"#,
            QueryLanguage::Ts,
        )
        .expect("memory outcome query should succeed");

    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(result.result[0]["summary"], "agent saw alpha fail");
}

#[test]
fn finish_task_writes_summary_memory_clears_session_task_and_updates_task_resource() {
    let host = host_with_node(demo_node());
    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .expect("task should start");

    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FixValidated,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            summary: "validated main behavior".to_string(),
            result: Some(OutcomeResultInput::Success),
            evidence: None,
            task_id: None,
        },
    )
    .expect("validation outcome should store");

    let result = host
        .finish_task(
            test_session(&host).as_ref(),
            PrismFinishTaskArgs {
                summary: "Closed out main investigation with validation coverage".to_string(),
                anchors: Some(vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }]),
                task_id: None,
            },
        )
        .expect("finish task should succeed");

    assert_eq!(result.task_id, task.0);
    assert_eq!(test_session(&host).current_task(), None);
    assert_eq!(result.journal.disposition, "completed");
    assert_eq!(
        result.journal.summary.final_summary.as_deref(),
        Some("Closed out main investigation with validation coverage")
    );

    let replay = host.current_prism().resume_task(&task);
    let closing = replay
        .events
        .iter()
        .find(|event| event.meta.id.0 == result.event_id)
        .expect("closing outcome should be present");
    assert_eq!(closing.kind, OutcomeKind::NoteAdded);
    assert_eq!(
        closing.metadata["taskLifecycle"]["disposition"],
        "completed"
    );
    assert_eq!(
        closing.metadata["taskLifecycle"]["memoryId"],
        result.memory_id
    );

    let memory = test_session(&host)
        .notes
        .entry(&MemoryId(result.memory_id.clone()))
        .expect("summary memory should exist");
    assert_eq!(
        memory.content,
        "Closed out main investigation with validation coverage"
    );
    assert_eq!(memory.metadata["taskLifecycle"]["disposition"], "completed");

    let resource = host
        .task_resource_value(
            test_session(&host).as_ref(),
            &task_resource_uri(&result.task_id),
            &task,
        )
        .expect("task resource should load");
    assert_eq!(resource.journal.disposition, "completed");
    assert_eq!(
        resource.journal.summary.final_summary.as_deref(),
        Some("Closed out main investigation with validation coverage")
    );
}

#[test]
fn task_journal_query_reports_missing_validation_and_related_memory() {
    let main = demo_node();
    let main_id = main.id.clone();
    let task_id = TaskId::new("task:journal");

    let mut graph = Graph::default();
    graph.nodes.insert(main_id.clone(), main);
    graph.adjacency = HashMap::new();
    graph.reverse_adjacency = HashMap::new();

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:journal:plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(main_id.clone())],
            kind: OutcomeKind::PlanCreated,
            result: OutcomeResult::Success,
            summary: "Investigate main".into(),
            evidence: Vec::new(),
            metadata: json!({ "tags": ["bug"] }),
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:journal:patch"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(task_id.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(main_id.clone())],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Partial,
            summary: "patched main".into(),
            evidence: Vec::new(),
            metadata: Value::Null,
        })
        .unwrap();

    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::main", NodeKind::Function)]);
    let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
    let mut memory = MemoryEntry::new(
        MemoryKind::Structural,
        "main changes should always get a regression check",
    );
    memory.anchors = vec![AnchorRef::Node(main_id)];
    memory.trust = 0.9;
    test_session(&host).notes.store(memory).unwrap();

    let result = host
        .execute(
            test_session(&host),
            r#"
return prism.taskJournal("task:journal", { eventLimit: 10, memoryLimit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(result.result["taskId"], "task:journal");
    assert_eq!(result.result["disposition"], "open");
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "missing_validation"));
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic["code"] == "missing_close_summary"));
    assert!(result.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["data"]["nextAction"].as_str().is_some()));
    assert_eq!(
        result.result["relatedMemory"][0]["entry"]["content"],
        "main changes should always get a regression check"
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missing_validation"));
    assert!(result.diagnostics.iter().all(|diagnostic| {
        diagnostic
            .data
            .as_ref()
            .and_then(|data| data["nextAction"].as_str())
            .is_some()
    }));
}

#[test]
fn call_graph_depth_limit_diagnostic_includes_next_action() {
    let host = host_with_node(demo_node());
    let result = host
        .execute(
            test_session(&host),
            r#"
const sym = prism.symbol("main");
return sym?.callGraph(50);
"#,
            QueryLanguage::Ts,
        )
        .expect("call graph query should succeed");

    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "depth_limited"));
    assert!(result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "depth_limited")
        .and_then(|diagnostic| diagnostic.data.as_ref())
        .and_then(|data| data["nextAction"].as_str())
        .is_some_and(|value| value.contains("prism.callGraph")));
}

#[test]
fn abandon_task_suppresses_unresolved_failure_diagnostic() {
    let host = host_with_node(demo_node());
    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            Vec::new(),
            None,
        )
        .expect("task should start");

    host.store_outcome(
        test_session(&host).as_ref(),
        PrismOutcomeArgs {
            kind: OutcomeKindInput::FailureObserved,
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            summary: "main is blocked by upstream".to_string(),
            result: Some(OutcomeResultInput::Failure),
            evidence: None,
            task_id: None,
        },
    )
    .expect("failure outcome should store");

    let result = host
        .abandon_task(
            test_session(&host).as_ref(),
            PrismFinishTaskArgs {
                summary: "Stopped after upstream dependency failure".to_string(),
                anchors: None,
                task_id: None,
            },
        )
        .expect("abandon task should succeed");

    assert_eq!(result.task_id, task.0);
    assert_eq!(result.journal.disposition, "abandoned");
    assert!(result
        .journal
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code != "unresolved_failure"));
    assert_eq!(
        result.journal.summary.final_summary.as_deref(),
        Some("Stopped after upstream dependency failure")
    );
}

#[test]
fn explicit_start_task_sets_session_default_and_logs_plan() {
    let host = host_with_node(demo_node());

    let task = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .expect("task should start");

    assert_eq!(test_session(&host).current_task(), Some(task.clone()));
    let replay = host.current_prism().resume_task(&task);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, OutcomeKind::PlanCreated);
    assert_eq!(replay.events[0].summary, "Investigate main");
    assert_eq!(replay.events[0].metadata["tags"][0], "bug");

    let journal = host
        .execute(
            test_session(&host),
            &format!(
                "return prism.taskJournal(\"{}\", {{ eventLimit: 10, memoryLimit: 5 }});",
                task.0
            ),
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn start_task_can_bind_directly_to_coordination_task() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Dogfood coordination task session binding" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit alpha",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap().to_string();

    let started = host
        .start_task(
            test_session(&host).as_ref(),
            None,
            vec!["ux".to_string()],
            Some(task_id.clone()),
        )
        .expect("task should bind to coordination task");

    assert_eq!(started.0.as_str(), task_id);
    let current_task = test_session(&host)
        .current_task_state()
        .expect("current task should be set");
    assert_eq!(current_task.id, started);
    assert_eq!(current_task.description.as_deref(), Some("Edit alpha"));
    assert_eq!(current_task.tags, vec!["ux".to_string()]);
    assert_eq!(
        current_task.coordination_task_id.as_deref(),
        Some(task_id.as_str())
    );

    let replay = host.current_prism().resume_task(&started);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].summary, "Edit alpha");
    assert_eq!(replay.events[0].metadata["coordinationTaskId"], task_id);

    let session = host
        .session_resource_value(test_session(&host).as_ref())
        .expect("session resource should load");
    assert_eq!(
        session
            .current_task
            .as_ref()
            .and_then(|task| task.coordination_task_id.as_deref()),
        Some(task_id.as_str())
    );
}

#[test]
fn coordination_task_journal_falls_back_to_task_title_without_outcomes() {
    let root = temp_workspace();
    let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

    let plan = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Dogfood coordination task journal metadata fallback" }),
                task_id: None,
            },
        )
        .unwrap();
    let task = host
        .store_coordination(
            test_session(&host).as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Validate persistence task",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::alpha",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            },
        )
        .unwrap();
    let task_id = task.state["id"].as_str().unwrap();

    let journal = host
        .execute(
            test_session(&host),
            &format!(
                "return prism.taskJournal(\"{task_id}\", {{ eventLimit: 10, memoryLimit: 5 }});"
            ),
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(journal.result["taskId"], task_id);
    assert_eq!(journal.result["description"], "Validate persistence task");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn task_journal_without_outcome_history_does_not_claim_missing_plan() {
    let host = host_with_node(demo_node());
    let task = TaskId::new("task:empty");
    test_session(&host).set_current_task(
        task.clone(),
        Some("Investigate empty task".to_string()),
        Vec::new(),
        None,
    );

    let journal = host
        .execute(
            test_session(&host),
            r#"
return prism.taskJournal("task:empty", { eventLimit: 10, memoryLimit: 5 });
"#,
            QueryLanguage::Ts,
        )
        .expect("task journal query should succeed");

    assert_eq!(journal.result["taskId"], "task:empty");
    assert_eq!(journal.result["disposition"], "active");
    assert!(journal.result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic["code"] != "missing_plan"));
}

#[test]
fn explicit_task_override_does_not_replace_session_default() {
    let host = host_with_node(demo_node());
    let active = host
        .start_task(
            test_session(&host).as_ref(),
            Some("Primary task".to_string()),
            Vec::new(),
            None,
        )
        .expect("task should start");

    let explicit = TaskId::new("task:secondary:99");
    let event = host
        .store_outcome(
            test_session(&host).as_ref(),
            PrismOutcomeArgs {
                kind: OutcomeKindInput::FailureObserved,
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                summary: "secondary failure".to_string(),
                result: Some(OutcomeResultInput::Failure),
                evidence: None,
                task_id: Some(explicit.0.to_string()),
            },
        )
        .expect("outcome should store");

    assert_eq!(test_session(&host).current_task(), Some(active));
    let replay = host.current_prism().resume_task(&explicit);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].meta.id.0, event.event_id);
}

#[test]
fn cloned_servers_isolate_session_state_but_share_persisted_state() {
    let server = server_with_node(demo_node());
    let client_a = server.clone();
    let client_b = server.clone();

    assert!(Arc::ptr_eq(&client_a.host, &client_b.host));
    assert_ne!(
        client_a.session.session_id().0,
        client_b.session.session_id().0
    );

    client_a
        .host
        .configure_session_without_refresh(
            client_a.session.as_ref(),
            PrismConfigureSessionArgs {
                limits: Some(QueryLimitsInput {
                    max_result_nodes: Some(3),
                    max_call_graph_depth: None,
                    max_output_json_bytes: None,
                }),
                current_task_id: None,
                coordination_task_id: None,
                current_task_description: None,
                current_task_tags: None,
                clear_current_task: None,
                current_agent: Some("agent-a".to_string()),
                clear_current_agent: None,
            },
        )
        .unwrap();
    let task_a = client_a
        .host
        .start_task(
            client_a.session.as_ref(),
            Some("Investigate main".to_string()),
            vec!["bug".to_string()],
            None,
        )
        .unwrap();

    let session_a = client_a
        .host
        .session_resource_value(client_a.session.as_ref())
        .unwrap();
    let session_b = client_b
        .host
        .session_resource_value(client_b.session.as_ref())
        .unwrap();
    assert_eq!(session_a.current_agent.as_deref(), Some("agent-a"));
    assert_eq!(
        session_a
            .current_task
            .as_ref()
            .map(|task| task.task_id.as_str()),
        Some(task_a.0.as_str())
    );
    assert_eq!(session_a.limits.max_result_nodes, 3);
    assert_eq!(session_b.current_agent, None);
    assert!(session_b.current_task.is_none());
    assert_eq!(
        session_b.limits.max_result_nodes,
        QueryLimits::default().max_result_nodes
    );

    let session_edge = client_a
        .host
        .store_inferred_edge(
            client_a.session.as_ref(),
            PrismInferEdgeArgs {
                kind: "calls".to_string(),
                source: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                target: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                confidence: 0.8,
                scope: Some(InferredEdgeScopeInput::SessionOnly),
                evidence: None,
                task_id: None,
            },
        )
        .unwrap();
    assert!(client_a
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(session_edge.edge_id.clone()))
        .is_some());
    assert!(client_b
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(session_edge.edge_id.clone()))
        .is_none());

    let persisted_edge = client_a
        .host
        .store_inferred_edge(
            client_a.session.as_ref(),
            PrismInferEdgeArgs {
                kind: "calls".to_string(),
                source: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                target: NodeIdInput {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                },
                confidence: 0.9,
                scope: Some(InferredEdgeScopeInput::Persisted),
                evidence: Some(vec!["shared persisted inference".to_string()]),
                task_id: None,
            },
        )
        .unwrap();
    assert!(client_b
        .session
        .inferred_edges
        .record(&prism_agent::EdgeId(persisted_edge.edge_id.clone()))
        .is_some());

    client_a
        .host
        .execute(
            Arc::clone(&client_a.session),
            r#"return prism.symbol("main")?.id.path;"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    client_b
        .host
        .execute(
            Arc::clone(&client_b.session),
            r#"return prism.symbol("main")?.id.path;"#,
            QueryLanguage::Ts,
        )
        .unwrap();
    let query_log = client_a.host.query_log_entries(QueryLogArgs {
        limit: Some(10),
        since: None,
        target: None,
        operation: None,
        task_id: None,
        min_duration_ms: None,
    });
    assert!(query_log
        .iter()
        .any(|entry| entry.session_id == client_a.session.session_id().0));
    assert!(query_log
        .iter()
        .any(|entry| entry.session_id == client_b.session.session_id().0));
}

#[test]
fn workspace_coordination_persistence_records_mcp_session_scope() {
    let root = temp_workspace();
    let workspace = index_workspace_session(&root).unwrap();
    let host = host_with_session(workspace);
    let session_a = host.new_session_state();
    let session_b = host.new_session_state();

    host.store_coordination(
        session_a.as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Track session-scoped persistence context"
            }),
            task_id: None,
        },
    )
    .unwrap();
    let cache = root.join(".prism").join("cache.db");
    let mut store = SqliteStore::open(&cache).unwrap();
    let context_after_a = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("coordination mutation log should retain a latest context");
    assert_eq!(
        context_after_a.session_id.as_deref(),
        Some(session_a.session_id().0.as_str())
    );
    drop(store);

    host.store_coordination(
        session_b.as_ref(),
        PrismCoordinationArgs {
            kind: CoordinationMutationKindInput::PlanCreate,
            payload: json!({
                "goal": "Track a second session-scoped persistence context"
            }),
            task_id: None,
        },
    )
    .unwrap();

    let mut store = SqliteStore::open(&cache).unwrap();
    let logged_context = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("coordination mutation log should retain a latest context");
    assert_eq!(
        logged_context.session_id.as_deref(),
        Some(session_b.session_id().0.as_str())
    );
    assert_eq!(store.load_coordination_events().unwrap().len(), 2);

    drop(store);
    drop(host);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_coordination_mutations_keep_mcp_session_scope_in_authoritative_persistence() {
    let root = temp_workspace();
    let host = host_with_session_internal(index_workspace_session(&root).unwrap());
    let session = host.new_session_state();

    let plan = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({
                    "goal": "Track rejected mutation persistence",
                    "policy": { "requireReviewForCompletion": true }
                }),
                task_id: None,
            },
        )
        .unwrap();
    let plan_id = plan.state["id"].as_str().unwrap().to_string();

    let task = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan_id,
                    "title": "Complete without review"
                }),
                task_id: None,
            },
        )
        .unwrap();

    let rejected = host
        .store_coordination(
            session.as_ref(),
            PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::Update,
                payload: json!({
                    "id": task.state["id"].as_str().unwrap(),
                    "status": "completed"
                }),
                task_id: None,
            },
        )
        .unwrap();
    assert!(rejected.rejected);
    assert!(!rejected.event_ids.is_empty());

    let cache = root.join(".prism").join("cache.db");
    let mut store = SqliteStore::open(&cache).unwrap();
    let context = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("rejected coordination mutation should retain latest context");
    assert_eq!(
        context.session_id.as_deref(),
        Some(session.session_id().0.as_str())
    );
    let events = store.load_coordination_events().unwrap();
    assert_eq!(
        events.last().map(|event| &event.kind),
        Some(&prism_ir::CoordinationEventKind::MutationRejected)
    );

    drop(store);
    drop(host);
    let _ = fs::remove_dir_all(root);
}
