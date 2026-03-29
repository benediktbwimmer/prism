use std::collections::BTreeMap;

use prism_coordination::{
    Artifact, ArtifactProposeInput, CoordinationPolicy, CoordinationRuntimeState,
    CoordinationSnapshot, CoordinationStore, HandoffInput, PlanCreateInput, TaskCompletionContext,
    TaskCreateInput, TaskUpdateInput, WorkClaim,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language,
    Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, PlanEdge, PlanEdgeId, PlanEdgeKind,
    PlanExecutionOverlay, PlanGraph, PlanId, PlanKind, PlanNode, PlanNodeBlockerKind, PlanNodeId,
    PlanNodeKind, PlanNodeStatus, PlanScope, PlanStatus, SessionId, Span, TaskId,
    WorkspaceRevision,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeRecallQuery, OutcomeResult,
};
use prism_projections::{
    ConceptDecodeLens, ConceptPacket, ConceptProvenance, ConceptRelation, ConceptRelationKind,
    ConceptScope, ProjectionIndex,
};
use prism_store::{CoordinationPersistContext, Graph};

use super::Prism;

#[test]
fn finds_documents_by_file_stem_and_path_fragment() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
        name: "docs/SPEC.md".into(),
        kind: NodeKind::Document,
        file: FileId(1),
        span: Span::whole_file(1),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::document::docs::SPEC_md::overview",
            NodeKind::MarkdownHeading,
        ),
        name: "Overview".into(),
        kind: NodeKind::MarkdownHeading,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::document::docs::SPEC_md::spec_details",
            NodeKind::MarkdownHeading,
        ),
        name: "Spec Details".into(),
        kind: NodeKind::MarkdownHeading,
        file: FileId(1),
        span: Span::line(2),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::tests::search_respects_limit",
            NodeKind::Function,
        ),
        name: "search_respects_limit".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let symbol_matches = prism.symbol("SPEC");
    assert_eq!(symbol_matches.len(), 1);
    assert_eq!(symbol_matches[0].node().kind, NodeKind::Document);
    assert!(prism
        .symbol("docs/SPEC.md")
        .into_iter()
        .any(|symbol| symbol.node().kind == NodeKind::Document));
    assert!(prism
        .search("SPEC", 10, None, None)
        .into_iter()
        .any(|symbol| symbol.node().kind == NodeKind::MarkdownHeading));
    assert!(!prism
        .search("SPEC", 10, None, None)
        .into_iter()
        .any(|symbol| symbol.id().path == "demo::tests::search_respects_limit"));
}

#[test]
fn prefers_exact_name_matches_before_fuzzy_matches() {
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
        id: NodeId::new(
            "demo",
            "demo::document::notes::alpha_md",
            NodeKind::Document,
        ),
        name: "notes/alpha.md".into(),
        kind: NodeKind::Document,
        file: FileId(2),
        span: Span::whole_file(1),
        language: Language::Markdown,
    });

    let prism = Prism::new(graph);
    let symbols = prism.symbol("alpha");

    assert_eq!(symbols[0].node().kind, NodeKind::Function);
}

#[test]
fn search_respects_limit() {
    let mut graph = Graph::new();
    for index in 0..3 {
        graph.add_node(Node {
            id: NodeId::new(
                "demo",
                format!("demo::document::notes::alpha_{index}"),
                NodeKind::Document,
            ),
            name: format!("notes/alpha-{index}.md").into(),
            kind: NodeKind::Document,
            file: FileId(index + 1),
            span: Span::whole_file(1),
            language: Language::Markdown,
        });
    }

    let prism = Prism::new(graph);
    assert_eq!(prism.search("alpha", 2, None, None).len(), 2);
}

#[test]
fn symbol_by_id_returns_exact_symbol_without_searching() {
    let mut graph = Graph::new();
    let target = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: target.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let symbol = prism
        .symbol_by_id(&target)
        .expect("exact node id lookup should return a symbol");

    assert_eq!(symbol.id(), &target);
    assert_eq!(symbol.name(), "alpha");
}

#[test]
fn search_can_filter_by_kind_and_path() {
    use std::path::Path;

    let mut graph = Graph::new();
    let spec_file = graph.ensure_file(Path::new("/workspace/docs/SPEC.md"));
    let source_file = graph.ensure_file(Path::new("/workspace/src/spec.rs"));

    graph.add_node(Node {
        id: NodeId::new("demo", "demo::document::docs::SPEC_md", NodeKind::Document),
        name: "docs/SPEC.md".into(),
        kind: NodeKind::Document,
        file: spec_file,
        span: Span::whole_file(1),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::inspect_spec", NodeKind::Function),
        name: "inspect_spec".into(),
        kind: NodeKind::Function,
        file: source_file,
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);

    let documents = prism.search("spec", 10, Some(NodeKind::Document), Some("docs/"));
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].node().kind, NodeKind::Document);

    let functions = prism.search("spec", 10, Some(NodeKind::Function), Some("src/"));
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].node().kind, NodeKind::Function);
}

#[test]
fn concept_lookup_returns_curated_validation_packet() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::impact::Prism::validation_recipe",
            NodeKind::Method,
        ),
        name: "validation_recipe".into(),
        kind: NodeKind::Method,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::session_state::SessionState::start_task",
            NodeKind::Method,
        ),
        name: "start_task".into(),
        kind: NodeKind::Method,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::runtime::runtime_status", NodeKind::Function),
        name: "runtime_status".into(),
        kind: NodeKind::Function,
        file: FileId(3),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Curated validation concept.".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.95,
            core_members: vec![NodeId::new(
                "demo",
                "demo::impact::Prism::validation_recipe",
                NodeKind::Method,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: vec![NodeId::new(
                "demo",
                "demo::runtime::runtime_status",
                NodeKind::Function,
            )],
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Validation],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://session_lifecycle".to_string(),
            canonical_name: "session_lifecycle".to_string(),
            summary: "Curated session concept.".to_string(),
            aliases: vec!["session".to_string()],
            confidence: 0.9,
            core_members: vec![NodeId::new(
                "demo",
                "demo::session_state::SessionState::start_task",
                NodeKind::Method,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);
    let concept = prism.concept("validation").expect("concept should resolve");

    assert_eq!(concept.handle, "concept://validation_pipeline");
    assert!(concept
        .core_members
        .iter()
        .any(|node| node.path.contains("validation_recipe")));
    assert!(prism
        .concept_by_handle("concept://session_lifecycle")
        .is_some());
}

#[test]
fn concept_relation_lookup_returns_direct_neighbors() {
    let prism = Prism::new(Graph::new());
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Curated validation concept.".to_string(),
            aliases: vec!["validation".to_string()],
            confidence: 0.95,
            core_members: vec![
                NodeId::new("demo", "demo::validation_recipe", NodeKind::Function),
                NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
            ],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Validation],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://runtime_surface".to_string(),
            canonical_name: "runtime_surface".to_string(),
            summary: "Curated runtime concept.".to_string(),
            aliases: vec!["runtime".to_string()],
            confidence: 0.9,
            core_members: vec![
                NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
                NodeId::new("demo", "demo::start_task", NodeKind::Function),
            ],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);
    prism.upsert_concept_relation(ConceptRelation {
        source_handle: "concept://validation_pipeline".to_string(),
        target_handle: "concept://runtime_surface".to_string(),
        kind: ConceptRelationKind::OftenUsedWith,
        confidence: 0.83,
        evidence: vec!["Validation work often moves through runtime state.".to_string()],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "concept_relation".to_string(),
            task_id: None,
        },
    });

    let relations = prism.concept_relations_for_handle("concept://validation_pipeline");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].target_handle, "concept://runtime_surface");
    assert_eq!(relations[0].kind, ConceptRelationKind::OftenUsedWith);
}

#[test]
fn concept_health_flags_ambiguous_stale_validation_concepts() {
    let mut graph = Graph::new();
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_recipe", NodeKind::Function),
        name: "validation_recipe".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_healthcheck", NodeKind::Function),
        name: "validation_healthcheck".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::validation_recipe_test", NodeKind::Function),
        name: "validation_recipe_test".into(),
        kind: NodeKind::Function,
        file: FileId(3),
        span: Span::line(1),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    prism.replace_curated_concepts(vec![
        ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Validation checks and likely tests.".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.95,
            core_members: vec![NodeId::new(
                "demo",
                "demo::validation_recipe",
                NodeKind::Function,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: vec![NodeId::new(
                "demo",
                "demo::validation_recipe_test",
                NodeKind::Function,
            )],
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: Some("Validation drift is common here.".to_string()),
            decode_lenses: vec![ConceptDecodeLens::Validation, ConceptDecodeLens::Timeline],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
        ConceptPacket {
            handle: "concept://validation_health".to_string(),
            canonical_name: "validation_health".to_string(),
            summary: "Validation-oriented health probes.".to_string(),
            aliases: vec!["validation".to_string()],
            confidence: 0.9,
            core_members: vec![NodeId::new(
                "demo",
                "demo::validation_healthcheck",
                NodeKind::Function,
            )],
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: None,
            decode_lenses: vec![ConceptDecodeLens::Open],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept".to_string(),
                task_id: None,
            },
            publication: None,
        },
    ]);

    let health = prism
        .concept_health_by_handle("concept://validation_pipeline")
        .expect("health should resolve");

    assert_eq!(
        health.status,
        prism_projections::ConceptHealthStatus::Drifted
    );
    assert!(health.signals.ambiguity_ratio >= 0.6);
    assert!(health.signals.stale_validation_links);
    assert!(health
        .reasons
        .iter()
        .any(|reason| reason.contains("likely tests")));
}

#[test]
fn broad_identifier_search_prefers_code_over_replay_and_lockfile_noise() {
    use std::path::Path;

    let mut graph = Graph::new();
    let planner_file = graph.ensure_file(Path::new("/workspace/src/planner.rs"));
    let replay_file = graph.ensure_file(Path::new(
        "/workspace/crates/prism-mcp/src/query_replay_cases.rs",
    ));
    let lockfile = graph.ensure_file(Path::new("/workspace/www/dashboard/package-lock.json"));

    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::planner::build_helper_plan",
            NodeKind::Function,
        ),
        name: "build_helper_plan".into(),
        kind: NodeKind::Function,
        file: planner_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::query_replay_cases::assert_repo_helper_bundle",
            NodeKind::Function,
        ),
        name: "assert_repo_helper_bundle".into(),
        kind: NodeKind::Function,
        file: replay_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::document::package_lock_json::packages::node_modules/@babel/helper-globals",
            NodeKind::JsonKey,
        ),
        name: "node_modules/@babel/helper-globals".into(),
        kind: NodeKind::JsonKey,
        file: lockfile,
        span: Span::line(1),
        language: Language::Json,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::planner::build_helper_plan");
    assert!(!results
        .iter()
        .any(|symbol| symbol.id().path.contains("query_replay_cases")));
    assert!(!results
        .iter()
        .any(|symbol| symbol.id().path.contains("@babel/helper-globals")));
}

#[test]
fn broad_identifier_search_suppresses_test_noise_when_non_test_code_exists() {
    use std::path::Path;

    let mut graph = Graph::new();
    let lib_file = graph.ensure_file(Path::new("/workspace/src/lib.rs"));
    let helpers_file = graph.ensure_file(Path::new("/workspace/src/query_helpers.rs"));

    graph.add_node(Node {
        id: NodeId::new("demo", "demo::build_helper_plan", NodeKind::Function),
        name: "build_helper_plan".into(),
        kind: NodeKind::Function,
        file: lib_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::query_helpers", NodeKind::Module),
        name: "query_helpers".into(),
        kind: NodeKind::Module,
        file: helpers_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::tests::helper", NodeKind::Function),
        name: "helper".into(),
        kind: NodeKind::Function,
        file: lib_file,
        span: Span::line(10),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::build_helper_plan");
    assert!(results
        .iter()
        .all(|symbol| !symbol.id().path.contains("::tests::")));
}

#[test]
fn broad_identifier_search_prefers_owner_module_over_path_inherited_functions() {
    use std::path::Path;

    let mut graph = Graph::new();
    let helpers_file = graph.ensure_file(Path::new("/workspace/src/helpers.rs"));

    graph.add_node(Node {
        id: NodeId::new("demo", "demo::helpers", NodeKind::Module),
        name: "helpers".into(),
        kind: NodeKind::Module,
        file: helpers_file,
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new("demo", "demo::helpers::anchor_sort_key", NodeKind::Function),
        name: "anchor_sort_key".into(),
        kind: NodeKind::Function,
        file: helpers_file,
        span: Span::line(3),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: NodeId::new(
            "demo",
            "demo::helpers::conflict_between",
            NodeKind::Function,
        ),
        name: "conflict_between".into(),
        kind: NodeKind::Function,
        file: helpers_file,
        span: Span::line(7),
        language: Language::Rust,
    });

    let prism = Prism::new(graph);
    let results = prism.search("helper", 5, None, None);

    assert_eq!(results[0].id().path, "demo::helpers");
    assert!(results
        .iter()
        .skip(1)
        .all(|symbol| !matches!(symbol.node().kind, NodeKind::Module)));
}

#[test]
fn exposes_lineage_queries_when_history_is_present() {
    let mut graph = Graph::new();
    let node_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: node_id.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([node_id.clone()]);
    let prism = Prism::with_history(graph, history);

    let lineage = prism.lineage_of(&node_id).unwrap();
    assert!(prism.lineage_history(&lineage).is_empty());
}

#[test]
fn outcome_queries_expand_node_to_lineage() {
    let mut graph = Graph::new();
    let old_id = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let new_id = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
    graph.add_node(Node {
        id: new_id.clone(),
        name: "renamed_alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    });

    let mut history = HistoryStore::new();
    history.seed_nodes([old_id.clone()]);
    let lineage = history.apply(&prism_ir::ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        trigger: prism_ir::ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added: vec![prism_ir::ObservedNode {
            node: Node {
                id: new_id.clone(),
                name: "renamed_alpha".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
        }],
        removed: vec![prism_ir::ObservedNode {
            node: Node {
                id: old_id.clone(),
                name: "alpha".into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            },
            fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(2), Some(2), None),
        }],
        updated: Vec::new(),
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    })[0]
        .lineage
        .clone();

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:1"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:rename")),
                causation: None,
            },
            anchors: vec![AnchorRef::Lineage(lineage)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "rename caused a failure".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "rename_flow".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let failures = prism.related_failures(&new_id);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].summary.contains("failure"));
}

#[test]
fn outcome_query_filters_expand_node_focus_with_additional_filters() {
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

    let outcomes = OutcomeMemory::new();
    let task = TaskId::new("task:alpha");
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:filter:1"),
                ts: 5,
                actor: EventActor::System,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "system failure".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:filter:2"),
                ts: 12,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "agent failure".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let events = prism.query_outcomes(&OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha)],
        task: Some(task),
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        actor: Some(EventActor::Agent),
        since: Some(10),
        limit: 10,
    });

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].summary, "agent failure");
}

#[test]
fn blast_radius_includes_validations_and_neighbors() {
    let mut graph = Graph::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
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
                id: EventId::new("outcome:2"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:beta")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::TestRan,
            result: OutcomeResult::Success,
            summary: "alpha requires unit test".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_unit".into(),
                passed: true,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let impact = prism.blast_radius(&alpha);
    assert!(impact.direct_nodes.contains(&beta));
    assert!(impact
        .likely_validations
        .iter()
        .any(|validation| validation == "test:alpha_unit"));
    assert!(impact
        .validation_checks
        .iter()
        .any(|check| check.label == "test:alpha_unit" && check.score > 0.0));
}

#[test]
fn blast_radius_uses_co_change_history_and_neighbor_validations() {
    let mut graph = Graph::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
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

    let mut history = HistoryStore::new();
    history.seed_nodes([alpha.clone(), beta.clone()]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:cochange"),
            ts: 10,
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
        updated: vec![
            (
                ObservedNode {
                    node: Node {
                        id: alpha.clone(),
                        name: "alpha".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(1),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(20), None, None),
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
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(10, Some(21), None, None),
                },
            ),
            (
                ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(30), None, None),
                },
                ObservedNode {
                    node: Node {
                        id: beta.clone(),
                        name: "beta".into(),
                        kind: NodeKind::Function,
                        file: FileId(1),
                        span: Span::line(2),
                        language: Language::Rust,
                    },
                    fingerprint: prism_ir::SymbolFingerprint::with_parts(11, Some(31), None, None),
                },
            ),
        ],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    });

    let beta_lineage = history.lineage_of(&beta).unwrap();
    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:cochange"),
                ts: 11,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:beta")),
                causation: None,
            },
            anchors: vec![AnchorRef::Lineage(beta_lineage)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "beta changes usually need the integration test".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "beta_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let impact = prism.blast_radius(&alpha);

    assert!(impact.direct_nodes.contains(&beta));
    assert!(impact
        .co_change_neighbors
        .iter()
        .any(|neighbor| neighbor.count == 1 && neighbor.nodes.contains(&beta)));
    assert!(impact
        .likely_validations
        .iter()
        .any(|validation| validation == "test:beta_integration"));
    assert!(impact
        .validation_checks
        .iter()
        .any(|check| check.label == "test:beta_integration" && check.score > 0.0));
    assert!(impact
        .risk_events
        .iter()
        .any(|event| event.summary.contains("integration test")));
}

#[test]
fn coordination_queries_expand_into_neighboring_symbols() {
    let mut graph = Graph::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
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
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Coordinate alpha".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task"),
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
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    coordination
        .acquire_claim(
            EventMeta {
                id: EventId::new("coord:claim"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            SessionId::new("session:a"),
            prism_coordination::ClaimAcquireInput {
                task_id: Some(task_id),
                anchors: vec![AnchorRef::Node(alpha.clone())],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: Some(120),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let claims = prism.claims(&[AnchorRef::Node(beta.clone())], 4);
    assert_eq!(claims.len(), 1);

    let simulated = prism.simulate_claim(
        &SessionId::new("session:b"),
        &[AnchorRef::Node(beta)],
        prism_ir::Capability::Edit,
        Some(prism_ir::ClaimMode::HardExclusive),
        None,
        4,
    );
    assert!(simulated
        .iter()
        .any(|conflict| conflict.severity == prism_ir::ConflictSeverity::Block));
    assert!(simulated.iter().any(|conflict| {
        conflict.overlap_kinds.iter().any(|kind| {
            matches!(
                kind,
                prism_ir::ConflictOverlapKind::Node
                    | prism_ir::ConflictOverlapKind::Lineage
                    | prism_ir::ConflictOverlapKind::File
            )
        })
    }));
}

#[test]
fn plan_graph_reads_native_runtime_state_before_coordination_projection() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Native plan graph".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_a, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:native:a"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_b, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:native:b"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task B".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_a = PlanNodeId::new(task_a.0.clone());
    let node_b = PlanNodeId::new(task_b.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Native graph".into(),
        goal: "Native graph".into(),
        status: PlanStatus::Active,
        revision: 7,
        root_nodes: vec![node_a.clone()],
        tags: vec!["native".into()],
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: node_a.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Native Task A".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_b.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Native Task B".into(),
                summary: None,
                status: PlanNodeStatus::Waiting,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: vec![PlanEdge {
            id: PlanEdgeId::new("plan-edge:native:validates"),
            plan_id: plan_id.clone(),
            from: node_b.clone(),
            to: node_a.clone(),
            kind: PlanEdgeKind::Validates,
            summary: None,
            metadata: serde_json::Value::Null,
        }],
    };
    let mut native_overlays = BTreeMap::new();
    native_overlays.insert(
        plan_id.0.to_string(),
        vec![PlanExecutionOverlay {
            node_id: node_b.clone(),
            pending_handoff_to: None,
            session: Some(SessionId::new("session:native")),
            worktree_id: None,
            branch_ref: None,
            effective_assignee: None,
            awaiting_handoff_from: None,
        }],
    );

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        native_overlays,
    );

    let runtime_graph = prism.plan_graph(&plan_id).unwrap();
    assert_eq!(runtime_graph.title, "Native graph");
    assert_eq!(runtime_graph.edges.len(), 1);
    assert_eq!(runtime_graph.edges[0].kind, PlanEdgeKind::Validates);
    let runtime_execution = prism.plan_execution(&plan_id);
    assert_eq!(runtime_execution.len(), 1);
    assert_eq!(
        runtime_execution[0].session,
        Some(SessionId::new("session:native"))
    );
    assert_eq!(runtime_execution[0].effective_assignee, None);
}

#[test]
fn continuity_reads_native_runtime_state_before_coordination_projection() {
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
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:runtime"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Continuity runtime".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:runtime"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let mut runtime_snapshot = prism.coordination_snapshot();
    runtime_snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("runtime task should exist")
        .title = "Task A runtime".into();
    runtime_snapshot
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("runtime task should exist")
        .depends_on = vec![prism_ir::CoordinationTaskId::new("coord-task:missing")];
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:runtime"),
        holder: SessionId::new("session:runtime"),
        agent: Some(prism_ir::AgentId::new("agent-runtime")),
        worktree_id: None,
        branch_ref: None,
        task: Some(task_id.clone()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::SoftExclusive,
        since: 3,
        expires_at: 30,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:runtime"),
        task: task_id.clone(),
        worktree_id: None,
        branch_ref: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: None,
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: Vec::new(),
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    *prism
        .continuity_runtime
        .write()
        .expect("continuity runtime lock poisoned") =
        CoordinationRuntimeState::from_snapshot(runtime_snapshot);

    assert_eq!(prism.coordination_snapshot().claims.len(), 1);
    assert_eq!(prism.coordination_snapshot().artifacts.len(), 1);
    assert_eq!(
        prism
            .coordination_snapshot()
            .tasks
            .into_iter()
            .find(|task| task.id == task_id)
            .expect("runtime task should exist")
            .title,
        "Task A runtime"
    );
    assert_eq!(
        prism
            .coordination_task(&task_id)
            .expect("runtime task should exist")
            .title,
        "Task A runtime"
    );
    assert_eq!(prism.claims(&[AnchorRef::Node(alpha.clone())], 10).len(), 1);
    assert_eq!(prism.artifacts(&task_id).len(), 1);
    assert_eq!(
        prism
            .coordination_artifact(&prism_ir::ArtifactId::new("artifact:runtime"))
            .expect("runtime artifact should exist")
            .task,
        task_id
    );
}

#[test]
fn native_task_mutations_preserve_non_dependency_plan_edges() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:preserve"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Preserve native edges".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_a, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:preserve:a"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_b, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:preserve:b"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task B".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_a = PlanNodeId::new(task_a.0.clone());
    let node_b = PlanNodeId::new(task_b.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Preserve native edges".into(),
        goal: "Preserve native edges".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![node_a.clone(), node_b.clone()],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: node_a.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Task A".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: vec![prism_ir::PlanAcceptanceCriterion {
                    label: "Task A is validated".into(),
                    anchors: Vec::new(),
                    required_checks: vec![prism_ir::ValidationRef {
                        id: "validation:ci".into(),
                    }],
                    evidence_policy: prism_ir::AcceptanceEvidencePolicy::ReviewAndValidation,
                }],
                validation_refs: vec![prism_ir::ValidationRef {
                    id: "validation:ci".into(),
                }],
                is_abstract: true,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_b.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Task B".into(),
                summary: None,
                status: PlanNodeStatus::Waiting,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: vec![PlanEdge {
            id: PlanEdgeId::new("plan-edge:preserve:validates"),
            plan_id: plan_id.clone(),
            from: node_b.clone(),
            to: node_a.clone(),
            kind: PlanEdgeKind::Validates,
            summary: None,
            metadata: serde_json::Value::Null,
        }],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    prism
        .update_native_task(
            EventMeta {
                id: EventId::new("coord:task:preserve:update"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskUpdateInput {
                task_id: prism_ir::CoordinationTaskId::new(task_a.0.clone()),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                anchors: None,
                depends_on: None,
                acceptance: None,
                base_revision: Some(WorkspaceRevision::default()),
                completion_context: None,
            },
            WorkspaceRevision::default(),
            4,
        )
        .unwrap();
    prism
        .request_native_handoff(
            EventMeta {
                id: EventId::new("coord:task:preserve:handoff"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            HandoffInput {
                task_id: prism_ir::CoordinationTaskId::new(task_a.0.clone()),
                to_agent: Some(prism_ir::AgentId::new("agent-b")),
                summary: "handoff".into(),
                base_revision: WorkspaceRevision::default(),
            },
            WorkspaceRevision::default(),
        )
        .unwrap();
    prism
        .create_native_task(
            EventMeta {
                id: EventId::new("coord:task:preserve:create"),
                ts: 6,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task C".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:native")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: vec![prism_ir::CoordinationTaskId::new(task_a.0.clone())],
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let runtime_graph = prism.plan_graph(&plan_id).unwrap();
    assert!(runtime_graph
        .edges
        .iter()
        .any(|edge| edge.kind == PlanEdgeKind::Validates
            && edge.from == node_b
            && edge.to == node_a));
    assert!(runtime_graph
        .edges
        .iter()
        .any(|edge| edge.kind == PlanEdgeKind::DependsOn && edge.to == node_a));
    let runtime_execution = prism.plan_execution(&plan_id);
    assert!(runtime_execution
        .iter()
        .any(|overlay| overlay.node_id == node_a
            && overlay
                .pending_handoff_to
                .as_ref()
                .is_some_and(|agent| agent.0 == "agent-b")));
    assert!(runtime_execution
        .iter()
        .any(|overlay| overlay.node_id == node_a
            && overlay
                .effective_assignee
                .as_ref()
                .is_some_and(|agent| agent.0 == "agent-b")));
    let task_a_node = runtime_graph
        .nodes
        .iter()
        .find(|node| node.id == node_a)
        .expect("task a node");
    assert!(task_a_node.is_abstract);
    assert_eq!(task_a_node.acceptance.len(), 1);
    assert_eq!(
        task_a_node.acceptance[0]
            .required_checks
            .iter()
            .map(|check| check.id.as_str())
            .collect::<Vec<_>>(),
        vec!["validation:ci"]
    );
    assert_eq!(
        task_a_node.acceptance[0].evidence_policy,
        prism_ir::AcceptanceEvidencePolicy::ReviewAndValidation
    );
}

#[test]
fn claim_reads_and_simulation_respect_worktree_scope() {
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
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));

    let mut runtime_snapshot = prism.coordination_snapshot();
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:a"),
        holder: SessionId::new("session:a"),
        agent: None,
        worktree_id: Some("worktree:a".into()),
        branch_ref: Some("refs/heads/a".into()),
        task: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::HardExclusive,
        since: 1,
        expires_at: 100,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    runtime_snapshot.claims.push(WorkClaim {
        id: prism_ir::ClaimId::new("claim:b"),
        holder: SessionId::new("session:b"),
        agent: None,
        worktree_id: Some("worktree:b".into()),
        branch_ref: Some("refs/heads/b".into()),
        task: None,
        anchors: vec![AnchorRef::Node(alpha.clone())],
        capability: prism_ir::Capability::Edit,
        mode: prism_ir::ClaimMode::HardExclusive,
        since: 1,
        expires_at: 100,
        status: prism_ir::ClaimStatus::Active,
        base_revision: WorkspaceRevision::default(),
    });
    *prism
        .continuity_runtime
        .write()
        .expect("continuity runtime lock poisoned") =
        CoordinationRuntimeState::from_snapshot(runtime_snapshot);

    let claims = prism.claims(&[AnchorRef::Node(alpha.clone())], 10);
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].id.0, "claim:a");

    let conflicts = prism.simulate_claim(
        &SessionId::new("session:new"),
        &[AnchorRef::Node(alpha)],
        prism_ir::Capability::Edit,
        Some(prism_ir::ClaimMode::HardExclusive),
        None,
        10,
    );
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].blocking_claims[0].0, "claim:a");
}

#[test]
fn artifact_reads_and_pending_reviews_respect_worktree_scope() {
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
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
    );
    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));

    let seeded = CoordinationStore::new();
    let (plan_id, _) = seeded
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:artifact-scope"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Scoped artifact reviews".into(),
                status: None,
                policy: Some(CoordinationPolicy::default()),
            },
        )
        .unwrap();
    let (task_id, _) = seeded
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:artifact-scope"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let mut runtime_snapshot = seeded.snapshot();
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:a"),
        task: task_id.clone(),
        worktree_id: Some("worktree:a".into()),
        branch_ref: Some("refs/heads/a".into()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: Some("patch:a".into()),
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: Vec::new(),
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    runtime_snapshot.artifacts.push(Artifact {
        id: prism_ir::ArtifactId::new("artifact:b"),
        task: task_id.clone(),
        worktree_id: Some("worktree:b".into()),
        branch_ref: Some("refs/heads/b".into()),
        anchors: vec![AnchorRef::Node(alpha.clone())],
        base_revision: WorkspaceRevision::default(),
        diff_ref: Some("patch:b".into()),
        status: prism_ir::ArtifactStatus::Proposed,
        evidence: Vec::new(),
        reviews: Vec::new(),
        required_validations: Vec::new(),
        validated_checks: Vec::new(),
        risk_score: None,
    });
    *prism
        .continuity_runtime
        .write()
        .expect("continuity runtime lock poisoned") =
        CoordinationRuntimeState::from_snapshot(runtime_snapshot);

    let artifacts = prism.artifacts(&task_id);
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, "artifact:a");
    assert_eq!(prism.pending_reviews(Some(&plan_id)).len(), 1);
    assert_eq!(
        prism
            .coordination_artifact(&prism_ir::ArtifactId::new("artifact:a"))
            .map(|artifact| artifact.id.0),
        Some("artifact:a".into())
    );
    assert!(prism
        .coordination_artifact(&prism_ir::ArtifactId::new("artifact:b"))
        .is_none());
}

#[test]
fn ready_tasks_and_handoff_acceptance_respect_worktree_scope() {
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:worktree-ready"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Scoped ready work".into(),
                status: Some(prism_ir::PlanStatus::Active),
                policy: None,
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    let task = prism
        .create_native_task(
            EventMeta {
                id: EventId::new("coord:task:worktree-ready"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: Some(prism_ir::AgentId::new("agent-a")),
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    assert_eq!(task.worktree_id.as_deref(), Some("worktree:a"));
    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:b".into(),
        branch_ref: Some("refs/heads/b".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    assert!(prism.ready_tasks(&plan_id, 10).is_empty());

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    prism
        .request_native_handoff(
            EventMeta {
                id: EventId::new("coord:handoff:worktree-ready"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            HandoffInput {
                task_id: task.id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent-b")),
                summary: "handoff".into(),
                base_revision: WorkspaceRevision::default(),
            },
            WorkspaceRevision::default(),
        )
        .unwrap();

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:b".into(),
        branch_ref: Some("refs/heads/b".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    let accepted = prism
        .accept_native_handoff(
            EventMeta {
                id: EventId::new("coord:handoff-accept:worktree-ready"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            prism_coordination::HandoffAcceptInput {
                task_id: task.id.clone(),
                agent: Some(prism_ir::AgentId::new("agent-b")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert_eq!(accepted.worktree_id.as_deref(), Some("worktree:b"));
    let execution = prism.plan_execution(&plan_id);
    assert_eq!(execution.len(), 1);
    assert_eq!(execution[0].worktree_id.as_deref(), Some("worktree:b"));
    let projected = prism
        .coordination_task(&task.id)
        .expect("accepted task should remain queryable");
    assert_eq!(projected.worktree_id.as_deref(), Some("worktree:b"));
    assert_eq!(projected.status, prism_ir::CoordinationTaskStatus::Ready);
    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    prism.set_coordination_context(Some(CoordinationPersistContext {
        repo_id: "repo:test".into(),
        worktree_id: "worktree:a".into(),
        branch_ref: Some("refs/heads/a".into()),
        session_id: None,
        instance_id: Some("instance:test".into()),
    }));
    assert!(prism.ready_tasks(&plan_id, 10).is_empty());
}

#[test]
fn native_plan_node_mutations_preserve_authored_bindings_and_metadata() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validation-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            anchors: Vec::new(),
            kind: OutcomeKind::TestRan,
            result: OutcomeResult::Success,
            summary: "validation plan ran".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:review-plan"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            anchors: Vec::new(),
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "review plan validated".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-node-metadata"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Preserve authored node semantics".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:native-node-metadata"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Track artifacts".into(),
                status: None,
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    prism.replace_curated_concepts(vec![ConceptPacket {
        handle: "concept://validation_pipeline".to_string(),
        canonical_name: "validation_pipeline".to_string(),
        summary: "Validation pipeline concept.".to_string(),
        aliases: vec!["validation".to_string()],
        confidence: 0.95,
        core_members: Vec::new(),
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Seeded for native plan binding tests.".to_string()],
        risk_hint: None,
        decode_lenses: vec![ConceptDecodeLens::Validation],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "seed".to_string(),
            task_id: None,
        },
        publication: None,
    }]);
    let (validation_artifact_id, _) = prism
        .propose_native_artifact(
            EventMeta {
                id: EventId::new("coord:artifact:native-node-metadata:validation"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: Vec::new(),
                diff_ref: Some("patch:validation".into()),
                evidence: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                current_revision: WorkspaceRevision::default(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    let (review_artifact_id, _) = prism
        .propose_native_artifact(
            EventMeta {
                id: EventId::new("coord:artifact:native-node-metadata:review"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            ArtifactProposeInput {
                task_id,
                anchors: Vec::new(),
                diff_ref: Some("patch:review".into()),
                evidence: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                current_revision: WorkspaceRevision::default(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let node_id = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Validate,
            "Validate main".into(),
            Some("Collect validation evidence".into()),
            Some(PlanNodeStatus::Ready),
            None,
            false,
            prism_ir::PlanBinding {
                anchors: vec![AnchorRef::Kind(NodeKind::Function)],
                concept_handles: vec![
                    "concept://validation_pipeline".into(),
                    "concept://validation_pipeline".into(),
                ],
                artifact_refs: vec![validation_artifact_id.0.to_string()],
                memory_refs: vec!["memory:validation-note".into()],
                outcome_refs: vec!["outcome:validation-plan".into()],
            },
            Vec::new(),
            Vec::new(),
            vec![prism_ir::ValidationRef {
                id: "validation:demo-main".into(),
            }],
            WorkspaceRevision::default(),
            Some(3),
            vec!["release".into(), "validation".into(), "release".into()],
        )
        .unwrap();

    let plan_id = prism
        .update_native_plan_node(
            &node_id,
            Some(PlanNodeKind::Review),
            Some(PlanNodeStatus::InReview),
            None,
            Some(true),
            Some("Review validation evidence".into()),
            Some("Review the collected checks".into()),
            false,
            Some(prism_ir::PlanBinding {
                anchors: vec![AnchorRef::Kind(NodeKind::Method)],
                concept_handles: vec!["concept://validation_pipeline".into()],
                artifact_refs: vec![review_artifact_id.0.to_string()],
                memory_refs: vec!["memory:review-note".into()],
                outcome_refs: vec!["outcome:review-plan".into()],
            }),
            None,
            None,
            Some(vec![prism_ir::ValidationRef {
                id: "validation:review-main".into(),
            }]),
            Some(WorkspaceRevision::default()),
            Some(7),
            false,
            Some(vec!["review".into(), "validation".into(), "review".into()]),
        )
        .unwrap();

    let graph = prism.plan_graph(&plan_id).expect("plan graph");
    let node = graph
        .nodes
        .into_iter()
        .find(|node| node.id == node_id)
        .expect("native node");
    assert_eq!(node.kind, PlanNodeKind::Review);
    assert_eq!(node.summary.as_deref(), Some("Review the collected checks"));
    assert!(node.is_abstract);
    assert_eq!(node.priority, Some(7));
    assert_eq!(node.tags, vec!["review", "validation"]);
    assert_eq!(
        node.bindings.anchors,
        vec![AnchorRef::Kind(NodeKind::Method)]
    );
    assert_eq!(
        node.bindings.concept_handles,
        vec!["concept://validation_pipeline"]
    );
    assert_eq!(
        node.bindings.artifact_refs,
        vec![review_artifact_id.0.to_string()]
    );
    assert_eq!(node.bindings.memory_refs, vec!["memory:review-note"]);
    assert_eq!(node.bindings.outcome_refs, vec!["outcome:review-plan"]);
    assert_eq!(
        node.validation_refs
            .iter()
            .map(|check| check.id.as_str())
            .collect::<Vec<_>>(),
        vec!["validation:review-main"]
    );
}

#[test]
fn native_plan_node_bindings_reject_runtime_handles_and_unstable_refs() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-node-bindings"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Reject unstable binding refs".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let create_error = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Edit,
            "Bad binding".into(),
            None,
            None,
            None,
            false,
            prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: vec!["handle:1".into()],
                artifact_refs: Vec::new(),
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkspaceRevision::default(),
            None,
            Vec::new(),
        )
        .expect_err("runtime handle binding should reject");
    assert!(create_error
        .to_string()
        .contains("runtime-only handles like `handle:1`"));

    let node_id = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Edit,
            "Valid node".into(),
            None,
            None,
            None,
            false,
            prism_ir::PlanBinding::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkspaceRevision::default(),
            None,
            Vec::new(),
        )
        .unwrap();
    prism.replace_curated_concepts(vec![ConceptPacket {
        handle: "concept://validation_pipeline".to_string(),
        canonical_name: "validation_pipeline".to_string(),
        summary: "Validation pipeline concept.".to_string(),
        aliases: Vec::new(),
        confidence: 0.9,
        core_members: Vec::new(),
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Seeded for artifact ref shape validation.".to_string()],
        risk_hint: None,
        decode_lenses: vec![ConceptDecodeLens::Validation],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "seed".to_string(),
            task_id: None,
        },
        publication: None,
    }]);

    let update_error = prism
        .update_native_plan_node(
            &node_id,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            Some(prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: Vec::new(),
                artifact_refs: vec!["not-an-artifact-ref".into()],
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            }),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect_err("unstable published ref should reject");
    assert!(update_error
        .to_string()
        .contains("artifact_refs` must use stable `artifact:...` refs"));
}

#[test]
fn native_plan_node_bindings_reject_missing_published_refs() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-node-binding-resolution"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Reject missing published binding refs".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let concept_error = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Edit,
            "Missing concept".into(),
            None,
            None,
            None,
            false,
            prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: vec!["concept://missing".into()],
                artifact_refs: Vec::new(),
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkspaceRevision::default(),
            None,
            Vec::new(),
        )
        .expect_err("missing concept binding should reject");
    assert!(concept_error
        .to_string()
        .contains("must reference an existing concept handle"));

    prism.replace_curated_concepts(vec![ConceptPacket {
        handle: "concept://binding_resolution".to_string(),
        canonical_name: "binding_resolution".to_string(),
        summary: "Binding resolution concept.".to_string(),
        aliases: Vec::new(),
        confidence: 0.9,
        core_members: Vec::new(),
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Seeded for binding resolution test.".to_string()],
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

    let node_id = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Edit,
            "Valid concept".into(),
            None,
            None,
            None,
            false,
            prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: vec!["concept://binding_resolution".into()],
                artifact_refs: Vec::new(),
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkspaceRevision::default(),
            None,
            Vec::new(),
        )
        .unwrap();

    let artifact_error = prism
        .update_native_plan_node(
            &node_id,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            Some(prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: vec!["concept://binding_resolution".into()],
                artifact_refs: vec!["artifact:missing".into()],
                memory_refs: Vec::new(),
                outcome_refs: Vec::new(),
            }),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect_err("missing artifact binding should reject");
    assert!(artifact_error
        .to_string()
        .contains("must reference an existing published ref"));

    let outcome_error = prism
        .update_native_plan_node(
            &node_id,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            Some(prism_ir::PlanBinding {
                anchors: Vec::new(),
                concept_handles: vec!["concept://binding_resolution".into()],
                artifact_refs: Vec::new(),
                memory_refs: Vec::new(),
                outcome_refs: vec!["outcome:missing".into()],
            }),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect_err("missing outcome binding should reject");
    assert!(outcome_error
        .to_string()
        .contains("must reference an existing published ref"));
}

#[test]
fn hydrated_plan_graph_recovers_concept_bound_runtime_anchors() {
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

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
        vec![PlanGraph {
            id: PlanId::new("plan:concept-hydration"),
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            title: "Concept hydration".into(),
            goal: "Hydrate runtime anchors from concept bindings".into(),
            status: PlanStatus::Active,
            revision: 1,
            root_nodes: vec![PlanNodeId::new("plan-node:concept-hydration")],
            tags: Vec::new(),
            created_from: None,
            metadata: serde_json::Value::Null,
            nodes: vec![PlanNode {
                id: PlanNodeId::new("plan-node:concept-hydration"),
                plan_id: PlanId::new("plan:concept-hydration"),
                kind: PlanNodeKind::Edit,
                title: "Hydrate concept binding".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding {
                    anchors: Vec::new(),
                    concept_handles: vec!["concept://alpha_flow".into()],
                    artifact_refs: Vec::new(),
                    memory_refs: Vec::new(),
                    outcome_refs: Vec::new(),
                },
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            }],
            edges: Vec::new(),
        }],
        BTreeMap::new(),
    );
    prism.replace_curated_concepts(vec![ConceptPacket {
        handle: "concept://alpha_flow".to_string(),
        canonical_name: "alpha_flow".to_string(),
        summary: "Recover alpha through concept bindings.".to_string(),
        aliases: vec!["alpha".to_string()],
        confidence: 0.92,
        core_members: vec![alpha.clone()],
        core_member_lineages: vec![None],
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Seeded for hydration test.".to_string()],
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

    let hydrated = prism
        .plan_graph(&PlanId::new("plan:concept-hydration"))
        .expect("hydrated plan graph");
    let node = hydrated
        .nodes
        .iter()
        .find(|node| node.id == PlanNodeId::new("plan-node:concept-hydration"))
        .expect("hydrated node");
    assert!(node.bindings.anchors.contains(&AnchorRef::Node(alpha)));
    assert_eq!(
        node.bindings.concept_handles,
        vec!["concept://alpha_flow".to_string()]
    );
}

#[test]
fn native_plan_updates_validate_completion_and_preserve_non_dependency_edges() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Validate native plan writes".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_a, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:native-plan:a"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_b, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:native-plan:b"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task B".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_a = PlanNodeId::new(task_a.0.clone());
    let node_b = PlanNodeId::new(task_b.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Validate native plan writes".into(),
        goal: "Validate native plan writes".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![node_a.clone(), node_b.clone()],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: node_a.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Task A".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_b.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Task B".into(),
                summary: None,
                status: PlanNodeStatus::Waiting,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: vec![PlanEdge {
            id: PlanEdgeId::new("plan-edge:native-plan:validates"),
            plan_id: plan_id.clone(),
            from: node_b.clone(),
            to: node_a.clone(),
            kind: PlanEdgeKind::Validates,
            summary: None,
            metadata: serde_json::Value::Null,
        }],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    let error = prism
        .update_native_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-plan:complete"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            &plan_id,
            Some(PlanStatus::Completed),
            None,
            None,
        )
        .expect_err("incomplete plan should not complete");
    assert!(error.to_string().contains("cannot be completed"));

    let runtime_graph = prism.plan_graph(&plan_id).unwrap();
    assert_eq!(runtime_graph.status, PlanStatus::Active);
    assert!(runtime_graph.edges.iter().any(|edge| {
        edge.kind == PlanEdgeKind::Validates && edge.from == node_b && edge.to == node_a
    }));
}

#[test]
fn native_plan_edge_validation_rejects_self_cycles_and_multiple_child_parents() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:edge-validate"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Validate native plan edges".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_a, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:edge-validate:a"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_b, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:edge-validate:b"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task B".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_c, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:edge-validate:c"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task C".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_a = PlanNodeId::new(task_a.0.clone());
    let node_b = PlanNodeId::new(task_b.0.clone());
    let node_c = PlanNodeId::new(task_c.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Validate native plan edges".into(),
        goal: "Validate native plan edges".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![node_a.clone(), node_b.clone(), node_c.clone()],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: node_a.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Task A".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: vec![prism_ir::ValidationRef {
                    id: "validation:task-b".into(),
                }],
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_b.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Task B".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: vec![prism_ir::ValidationRef {
                    id: "validation:task-b".into(),
                }],
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_c.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Task C".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: Vec::new(),
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    prism
        .create_native_plan_edge(&plan_id, &node_a, &node_b, PlanEdgeKind::Validates)
        .unwrap();
    let cycle_error = prism
        .create_native_plan_edge(&plan_id, &node_b, &node_a, PlanEdgeKind::HandoffTo)
        .expect_err("mixed constrained edge cycle should be rejected");
    assert!(cycle_error.to_string().contains("introduce a cycle"));

    let self_error = prism
        .create_native_plan_edge(&plan_id, &node_a, &node_a, PlanEdgeKind::Blocks)
        .expect_err("self edges should be rejected");
    assert!(self_error.to_string().contains("cannot target itself"));

    prism
        .create_native_plan_edge(&plan_id, &node_c, &node_a, PlanEdgeKind::ChildOf)
        .unwrap();
    let parent_error = prism
        .create_native_plan_edge(&plan_id, &node_c, &node_b, PlanEdgeKind::ChildOf)
        .expect_err("child node should only have one authored parent");
    assert!(parent_error
        .to_string()
        .contains("already has an authored parent"));
}

#[test]
fn native_plan_edge_validation_enforces_kind_and_hierarchy_semantics() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let plan_id = PlanId::new("plan:native-edge-semantics");
    let parent = PlanNodeId::new("plan-node:parent");
    let child = PlanNodeId::new("plan-node:child");
    let work = PlanNodeId::new("plan-node:work");
    let validator = PlanNodeId::new("plan-node:validator");
    let non_validator = PlanNodeId::new("plan-node:non-validator");
    let abstract_target = PlanNodeId::new("plan-node:abstract-target");

    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Edge semantics".into(),
        goal: "Enforce edge semantics".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![
            parent.clone(),
            child.clone(),
            work.clone(),
            validator.clone(),
            non_validator.clone(),
            abstract_target.clone(),
        ],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: parent.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Note,
                title: "Parent".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: true,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: child.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Child".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: work.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Work".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: validator.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Validator".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: vec![prism_ir::ValidationRef {
                    id: "validation:validator".into(),
                }],
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: non_validator.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Not a validator".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: abstract_target.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Note,
                title: "Abstract target".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: true,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: Vec::new(),
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    prism
        .create_native_plan_edge(&plan_id, &child, &parent, PlanEdgeKind::ChildOf)
        .expect("child-of should succeed");
    let graph = prism.plan_graph(&plan_id).expect("plan graph");
    assert!(graph.root_nodes.iter().any(|node| node == &parent));
    assert!(!graph.root_nodes.iter().any(|node| node == &child));

    let validates_error = prism
        .create_native_plan_edge(&plan_id, &work, &non_validator, PlanEdgeKind::Validates)
        .expect_err("validates should require a validation node target");
    assert!(validates_error
        .to_string()
        .contains("must target a Validate node"));

    prism
        .create_native_plan_edge(&plan_id, &work, &validator, PlanEdgeKind::Validates)
        .expect("validate edge to validator should succeed");

    let handoff_error = prism
        .create_native_plan_edge(&plan_id, &work, &abstract_target, PlanEdgeKind::HandoffTo)
        .expect_err("handoff should reject abstract structure targets");
    assert!(handoff_error
        .to_string()
        .contains("must connect executable nodes"));
}

#[test]
fn native_plan_ready_nodes_and_blockers_follow_edge_semantics() {
    fn node(
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        title: &str,
        status: PlanNodeStatus,
    ) -> PlanNode {
        PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind: PlanNodeKind::Edit,
            title: title.into(),
            summary: None,
            status,
            bindings: prism_ir::PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            assignee: None,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn edge(plan_id: &PlanId, from: &PlanNodeId, to: &PlanNodeId, kind: PlanEdgeKind) -> PlanEdge {
        PlanEdge {
            id: PlanEdgeId::new(format!("{}:{:?}:{}", from.0, kind, to.0)),
            plan_id: plan_id.clone(),
            from: from.clone(),
            to: to.clone(),
            kind,
            summary: None,
            metadata: serde_json::Value::Null,
        }
    }

    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let plan_id = PlanId::new("plan:native-semantics");
    let blocked_by_dependency = PlanNodeId::new("plan-node:blocked-by-dependency");
    let dependency = PlanNodeId::new("plan-node:dependency");
    let blocked_by_authored_block = PlanNodeId::new("plan-node:blocked-by-authored-block");
    let authored_blocker = PlanNodeId::new("plan-node:authored-blocker");
    let blocked_by_validation = PlanNodeId::new("plan-node:blocked-by-validation");
    let validator = PlanNodeId::new("plan-node:validator");
    let handoff_source = PlanNodeId::new("plan-node:handoff-source");
    let handoff_target = PlanNodeId::new("plan-node:handoff-target");
    let free = PlanNodeId::new("plan-node:free");
    let pending_handoff = PlanNodeId::new("plan-node:pending-handoff");

    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Native plan semantics".into(),
        goal: "Enforce graph-native blocker rules".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![
            blocked_by_dependency.clone(),
            blocked_by_authored_block.clone(),
            blocked_by_validation.clone(),
            handoff_source.clone(),
            handoff_target.clone(),
            free.clone(),
            pending_handoff.clone(),
        ],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            node(
                &plan_id,
                &blocked_by_dependency,
                "Blocked by dependency",
                PlanNodeStatus::Ready,
            ),
            node(&plan_id, &dependency, "Dependency", PlanNodeStatus::Ready),
            node(
                &plan_id,
                &blocked_by_authored_block,
                "Blocked by authored block",
                PlanNodeStatus::Ready,
            ),
            node(
                &plan_id,
                &authored_blocker,
                "Authored blocker",
                PlanNodeStatus::Ready,
            ),
            node(
                &plan_id,
                &blocked_by_validation,
                "Blocked by validation",
                PlanNodeStatus::Ready,
            ),
            PlanNode {
                id: validator.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Validator".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: vec![prism_ir::ValidationRef {
                    id: "validation:validator".into(),
                }],
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            node(
                &plan_id,
                &handoff_source,
                "Handoff source",
                PlanNodeStatus::InProgress,
            ),
            node(
                &plan_id,
                &handoff_target,
                "Handoff target",
                PlanNodeStatus::Ready,
            ),
            node(&plan_id, &free, "Free", PlanNodeStatus::Ready),
            node(
                &plan_id,
                &pending_handoff,
                "Pending handoff",
                PlanNodeStatus::Ready,
            ),
        ],
        edges: vec![
            edge(
                &plan_id,
                &blocked_by_dependency,
                &dependency,
                PlanEdgeKind::DependsOn,
            ),
            edge(
                &plan_id,
                &blocked_by_authored_block,
                &authored_blocker,
                PlanEdgeKind::Blocks,
            ),
            edge(
                &plan_id,
                &blocked_by_validation,
                &validator,
                PlanEdgeKind::Validates,
            ),
            edge(
                &plan_id,
                &handoff_source,
                &handoff_target,
                PlanEdgeKind::HandoffTo,
            ),
        ],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::from([(
            plan_id.0.to_string(),
            vec![PlanExecutionOverlay {
                node_id: pending_handoff.clone(),
                pending_handoff_to: Some(prism_ir::AgentId::new("agent-b")),
                session: None,
                worktree_id: None,
                branch_ref: None,
                effective_assignee: None,
                awaiting_handoff_from: None,
            }],
        )]),
    );

    let ready_ids = prism
        .plan_ready_nodes(&plan_id)
        .into_iter()
        .map(|node| node.id.0)
        .collect::<Vec<_>>();
    assert_eq!(
        ready_ids,
        vec![
            authored_blocker.0.clone(),
            dependency.0.clone(),
            free.0.clone(),
            handoff_source.0.clone(),
            validator.0.clone(),
        ]
    );

    let dependency_blockers = prism.plan_node_blockers(&plan_id, &blocked_by_dependency);
    assert_eq!(dependency_blockers.len(), 1);
    assert_eq!(dependency_blockers[0].kind, PlanNodeBlockerKind::Dependency);
    assert_eq!(
        dependency_blockers[0].related_node_id,
        Some(dependency.clone())
    );

    let authored_blockers = prism.plan_node_blockers(&plan_id, &blocked_by_authored_block);
    assert_eq!(authored_blockers.len(), 1);
    assert_eq!(authored_blockers[0].kind, PlanNodeBlockerKind::BlockingNode);
    assert_eq!(
        authored_blockers[0].related_node_id,
        Some(authored_blocker.clone())
    );

    let validation_blockers = prism.plan_node_blockers(&plan_id, &blocked_by_validation);
    assert_eq!(validation_blockers.len(), 1);
    assert_eq!(
        validation_blockers[0].kind,
        PlanNodeBlockerKind::ValidationGate
    );
    assert_eq!(
        validation_blockers[0].related_node_id,
        Some(validator.clone())
    );
    assert_eq!(
        validation_blockers[0].validation_checks,
        vec!["validation:validator"]
    );

    let handoff_path_blockers = prism.plan_node_blockers(&plan_id, &handoff_target);
    assert_eq!(handoff_path_blockers.len(), 1);
    assert_eq!(handoff_path_blockers[0].kind, PlanNodeBlockerKind::Handoff);
    assert_eq!(
        handoff_path_blockers[0].related_node_id,
        Some(handoff_source.clone())
    );

    let pending_handoff_blockers = prism.plan_node_blockers(&plan_id, &pending_handoff);
    assert_eq!(pending_handoff_blockers.len(), 1);
    assert_eq!(
        pending_handoff_blockers[0].kind,
        PlanNodeBlockerKind::Handoff
    );
    assert!(pending_handoff_blockers[0]
        .summary
        .contains("pending handoff"));
    let execution = prism.plan_execution(&plan_id);
    assert!(execution.iter().any(|overlay| {
        overlay.node_id == handoff_target
            && overlay.awaiting_handoff_from.as_ref() == Some(&handoff_source)
    }));
    assert!(execution.iter().any(|overlay| {
        overlay.node_id == pending_handoff
            && overlay
                .effective_assignee
                .as_ref()
                .is_some_and(|agent| agent.0 == "agent-b")
    }));
}

#[test]
fn native_plan_child_hierarchy_gates_parent_completion_and_recommendations() {
    fn node(
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        title: &str,
        status: PlanNodeStatus,
        is_abstract: bool,
    ) -> PlanNode {
        PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind: if is_abstract {
                PlanNodeKind::Note
            } else {
                PlanNodeKind::Edit
            },
            title: title.into(),
            summary: None,
            status,
            bindings: prism_ir::PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract,
            assignee: None,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn edge(plan_id: &PlanId, from: &PlanNodeId, to: &PlanNodeId, kind: PlanEdgeKind) -> PlanEdge {
        PlanEdge {
            id: PlanEdgeId::new(format!("{}:{:?}:{}", from.0, kind, to.0)),
            plan_id: plan_id.clone(),
            from: from.clone(),
            to: to.clone(),
            kind,
            summary: None,
            metadata: serde_json::Value::Null,
        }
    }

    let plan_id = PlanId::new("plan:hierarchy");
    let parent_id = PlanNodeId::new("coord-task:parent");
    let child_id = PlanNodeId::new("coord-task:child");
    let sibling_id = PlanNodeId::new("coord-task:sibling");
    let graph = PlanGraph {
        id: plan_id.clone(),
        scope: prism_ir::PlanScope::Repo,
        kind: prism_ir::PlanKind::TaskExecution,
        title: "Hierarchy".into(),
        goal: "Hierarchy".into(),
        status: prism_ir::PlanStatus::Active,
        revision: 0,
        root_nodes: vec![parent_id.clone(), sibling_id.clone()],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            node(&plan_id, &parent_id, "Parent", PlanNodeStatus::Ready, true),
            node(
                &plan_id,
                &child_id,
                "Child",
                PlanNodeStatus::InProgress,
                false,
            ),
            node(
                &plan_id,
                &sibling_id,
                "Sibling",
                PlanNodeStatus::Ready,
                false,
            ),
        ],
        edges: vec![edge(&plan_id, &child_id, &parent_id, PlanEdgeKind::ChildOf)],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        CoordinationSnapshot::default(),
        ProjectionIndex::default(),
        vec![graph],
        BTreeMap::new(),
    );

    let blockers = prism.plan_node_blockers(&plan_id, &parent_id);
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].kind, PlanNodeBlockerKind::ChildIncomplete);
    assert_eq!(blockers[0].related_node_id, Some(child_id.clone()));

    let recommendations = prism.plan_next(&plan_id, 3);
    let child_recommendation = recommendations
        .iter()
        .find(|recommendation| recommendation.node.id == child_id)
        .expect("child recommendation");
    assert!(child_recommendation
        .unblocks
        .iter()
        .any(|node_id| node_id == &parent_id));
}

#[test]
fn native_plan_next_prefers_actionable_nodes_that_unblock_more_follow_up_work() {
    fn node(
        plan_id: &PlanId,
        node_id: &PlanNodeId,
        title: &str,
        status: PlanNodeStatus,
    ) -> PlanNode {
        PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind: PlanNodeKind::Edit,
            title: title.into(),
            summary: None,
            status,
            bindings: prism_ir::PlanBinding::default(),
            acceptance: Vec::new(),
            validation_refs: Vec::new(),
            is_abstract: false,
            assignee: None,
            base_revision: WorkspaceRevision::default(),
            priority: None,
            tags: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn edge(plan_id: &PlanId, from: &PlanNodeId, to: &PlanNodeId, kind: PlanEdgeKind) -> PlanEdge {
        PlanEdge {
            id: PlanEdgeId::new(format!("{}:{:?}:{}", from.0, kind, to.0)),
            plan_id: plan_id.clone(),
            from: from.clone(),
            to: to.clone(),
            kind,
            summary: None,
            metadata: serde_json::Value::Null,
        }
    }

    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let plan_id = PlanId::new("plan:native-next");
    let hub = PlanNodeId::new("plan-node:hub");
    let solo = PlanNodeId::new("plan-node:solo");
    let dependent_a = PlanNodeId::new("plan-node:dependent-a");
    let dependent_b = PlanNodeId::new("plan-node:dependent-b");

    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Native plan next".into(),
        goal: "Prefer actionable nodes that unlock more work".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![
            hub.clone(),
            solo.clone(),
            dependent_a.clone(),
            dependent_b.clone(),
        ],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            node(&plan_id, &hub, "Hub", PlanNodeStatus::Ready),
            node(&plan_id, &solo, "Solo", PlanNodeStatus::Ready),
            node(&plan_id, &dependent_a, "Dependent A", PlanNodeStatus::Ready),
            node(&plan_id, &dependent_b, "Dependent B", PlanNodeStatus::Ready),
        ],
        edges: vec![
            edge(&plan_id, &dependent_a, &hub, PlanEdgeKind::DependsOn),
            edge(&plan_id, &dependent_b, &hub, PlanEdgeKind::DependsOn),
        ],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    let next = prism.plan_next(&plan_id, 3);
    assert_eq!(next.len(), 3);
    assert_eq!(next[0].node.id, hub);
    assert!(next[0].actionable);
    assert_eq!(next[0].unblocks.len(), 2);
    assert!(next[0]
        .reasons
        .iter()
        .any(|reason| reason.contains("unblock 2 node")));
    assert_eq!(next[1].node.id, solo);
}

#[test]
fn native_plan_node_completion_rejects_missing_review_and_acceptance_validation() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:native-complete"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Require completion evidence".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    let node_id = prism
        .create_native_plan_node(
            &plan_id,
            PlanNodeKind::Edit,
            "Ship main".into(),
            None,
            Some(PlanNodeStatus::Ready),
            None,
            false,
            prism_ir::PlanBinding::default(),
            Vec::new(),
            vec![prism_ir::PlanAcceptanceCriterion {
                label: "main is validated".into(),
                anchors: Vec::new(),
                required_checks: vec![prism_ir::ValidationRef {
                    id: "validation:ci".into(),
                }],
                evidence_policy: prism_ir::AcceptanceEvidencePolicy::ReviewAndValidation,
            }],
            vec![prism_ir::ValidationRef {
                id: "validation:ci".into(),
            }],
            WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            None,
            Vec::new(),
        )
        .unwrap();

    let blockers = prism.plan_node_blockers(&plan_id, &node_id);
    let summary = prism
        .plan_summary(&plan_id)
        .expect("plan summary should exist");
    assert_eq!(summary.total_nodes, 1);
    assert!(prism.plan_ready_nodes(&plan_id).is_empty());
    assert_eq!(summary.actionable_nodes, 0);
    assert_eq!(summary.execution_blocked_nodes, 1);
    assert_eq!(summary.completion_gated_nodes, 1);
    assert_eq!(summary.review_gated_nodes, 1);
    assert_eq!(summary.validation_gated_nodes, 1);
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == PlanNodeBlockerKind::ReviewRequired));
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == PlanNodeBlockerKind::ValidationRequired));

    let error = prism
        .update_native_plan_node(
            &node_id,
            None,
            Some(PlanNodeStatus::Completed),
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect_err("native node completion should reject missing evidence");
    assert!(error.to_string().contains("cannot complete"));
    assert!(error.to_string().contains("approved review artifact"));
}

#[test]
fn published_plan_unbound_tasks_stay_actionable_across_unrelated_graph_drift() {
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
    history.seed_nodes([alpha]);
    history.apply(&ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("observed:stale-ready"),
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
                    id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
            },
            ObservedNode {
                node: Node {
                    id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                },
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
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
                id: EventId::new("coord:plan:stale-ready"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Keep published readiness aligned".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    stale_after_graph_change: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:stale-ready"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Unbound task".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    assert_eq!(prism.plan_ready_nodes(&plan_id).len(), 1);
    assert_eq!(prism.ready_tasks(&plan_id, 10).len(), 1);

    let summary = prism
        .plan_summary(&plan_id)
        .expect("plan summary should exist");
    assert_eq!(summary.total_nodes, 1);
    assert_eq!(summary.actionable_nodes, 1);
    assert_eq!(summary.execution_blocked_nodes, 0);
    assert_eq!(summary.stale_nodes, 0);
}

#[test]
fn replace_coordination_snapshot_and_plan_graphs_preserves_stale_policy() {
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
            id: EventId::new("observed:replace-stale-ready"),
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
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
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
                fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(1), None, None),
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
                id: EventId::new("coord:plan:replace-stale-ready"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Preserve stale policy on replacement".into(),
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
                id: EventId::new("coord:task:replace-stale-ready"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Stale task".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let snapshot = coordination.snapshot();
    let plan_graph = coordination.plan_graph(&plan_id).expect("plan graph");

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    prism.replace_coordination_snapshot_and_plan_graphs(
        snapshot,
        vec![plan_graph],
        BTreeMap::new(),
    );

    let blockers = prism.plan_node_blockers(&plan_id, &PlanNodeId::new(task_id.0.clone()));
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == PlanNodeBlockerKind::StaleRevision));
    let summary = prism
        .plan_summary(&plan_id)
        .expect("plan summary should exist");
    assert_eq!(summary.actionable_nodes, 0);
    assert_eq!(summary.execution_blocked_nodes, 1);
    assert_eq!(summary.stale_nodes, 1);
    assert!(prism
        .plan_next(&plan_id, 5)
        .into_iter()
        .all(|recommendation| !recommendation.actionable));
}

#[test]
fn task_backed_native_plan_node_completion_uses_continuity_review_state() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:task-backed-native"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Complete through native node update".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:task-backed-native"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:native")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    let node_id = PlanNodeId::new(task_id.0.clone());

    let before = prism.plan_node_blockers(&plan_id, &node_id);
    assert!(before
        .iter()
        .any(|blocker| blocker.kind == PlanNodeBlockerKind::ReviewRequired));
    assert!(prism
        .update_native_plan_node(
            &node_id,
            None,
            Some(PlanNodeStatus::Completed),
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .is_err());

    let (artifact_id, _) = prism
        .propose_native_artifact(
            EventMeta {
                id: EventId::new("coord:artifact:task-backed-native"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            prism_coordination::ArtifactProposeInput {
                task_id: prism_ir::CoordinationTaskId::new(task_id.0.clone()),
                anchors: Vec::new(),
                diff_ref: Some("patch:alpha".into()),
                evidence: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    prism
        .review_native_artifact(
            EventMeta {
                id: EventId::new("coord:review:task-backed-native"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            prism_coordination::ArtifactReviewInput {
                artifact_id,
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "approved".into(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
            WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();

    let after = prism.plan_node_blockers(&plan_id, &node_id);
    assert!(!after
        .iter()
        .any(|blocker| blocker.kind == PlanNodeBlockerKind::ReviewRequired));
    prism
        .update_native_plan_node(
            &node_id,
            None,
            Some(PlanNodeStatus::Completed),
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect("approved artifact should satisfy native completion gate");
}

#[test]
fn native_claim_and_artifact_mutations_preserve_non_dependency_plan_edges() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:compat"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Preserve graph under compatibility writes".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_a, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:compat:a"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (task_b, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:compat:b"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task B".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_a = PlanNodeId::new(task_a.0.clone());
    let node_b = PlanNodeId::new(task_b.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::TaskExecution,
        title: "Compatibility writes".into(),
        goal: "Compatibility writes".into(),
        status: PlanStatus::Active,
        revision: 1,
        root_nodes: vec![node_a.clone(), node_b.clone()],
        tags: Vec::new(),
        created_from: None,
        metadata: serde_json::Value::Null,
        nodes: vec![
            PlanNode {
                id: node_a.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Edit,
                title: "Task A".into(),
                summary: None,
                status: PlanNodeStatus::Ready,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
            PlanNode {
                id: node_b.clone(),
                plan_id: plan_id.clone(),
                kind: PlanNodeKind::Validate,
                title: "Task B".into(),
                summary: None,
                status: PlanNodeStatus::Waiting,
                bindings: prism_ir::PlanBinding::default(),
                acceptance: Vec::new(),
                validation_refs: Vec::new(),
                is_abstract: false,
                assignee: None,
                base_revision: WorkspaceRevision::default(),
                priority: None,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
            },
        ],
        edges: vec![PlanEdge {
            id: PlanEdgeId::new("plan-edge:compat:validates"),
            plan_id: plan_id.clone(),
            from: node_b.clone(),
            to: node_a.clone(),
            kind: PlanEdgeKind::Validates,
            summary: None,
            metadata: serde_json::Value::Null,
        }],
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );

    let (claim_id, _conflicts, state) = prism
        .acquire_native_claim(
            EventMeta {
                id: EventId::new("coord:claim:compat"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            SessionId::new("session:compat"),
            prism_coordination::ClaimAcquireInput {
                task_id: Some(prism_ir::CoordinationTaskId::new(task_a.0.clone())),
                anchors: vec![AnchorRef::Node(NodeId::new(
                    "demo",
                    "demo::alpha",
                    NodeKind::Function,
                ))],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: None,
                base_revision: WorkspaceRevision::default(),
                current_revision: WorkspaceRevision::default(),
                agent: Some(prism_ir::AgentId::new("agent-a")),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert!(claim_id.is_some());
    assert!(state.is_some());

    let (_artifact_id, artifact) = prism
        .propose_native_artifact(
            EventMeta {
                id: EventId::new("coord:artifact:compat"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            prism_coordination::ArtifactProposeInput {
                task_id: prism_ir::CoordinationTaskId::new(task_a.0.clone()),
                anchors: Vec::new(),
                diff_ref: None,
                evidence: Vec::new(),
                base_revision: WorkspaceRevision::default(),
                current_revision: WorkspaceRevision::default(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();
    assert_eq!(artifact.task.0, task_a.0);

    let runtime_graph = prism.plan_graph(&plan_id).unwrap();
    assert!(runtime_graph
        .edges
        .iter()
        .any(|edge| edge.kind == PlanEdgeKind::Validates
            && edge.from == node_b
            && edge.to == node_a));
    assert_eq!(prism.coordination_snapshot().claims.len(), 1);
    assert_eq!(
        prism
            .artifacts(&prism_ir::CoordinationTaskId::new(task_a.0.clone()))
            .len(),
        1
    );
}

#[test]
fn native_plan_metadata_survives_compatibility_write_and_reload() {
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
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:metadata-reload"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Persist native metadata".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:metadata-reload"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Task A".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let node_id = PlanNodeId::new(task_id.0.clone());
    let native_graph = PlanGraph {
        id: plan_id.clone(),
        scope: PlanScope::Repo,
        kind: PlanKind::Migration,
        title: "Native persistence migration".into(),
        goal: "Persist native metadata".into(),
        status: PlanStatus::Active,
        revision: 9,
        root_nodes: vec![node_id.clone()],
        tags: vec!["persistence".into(), "ux".into()],
        created_from: Some("concept://coordination_ux".into()),
        metadata: serde_json::json!({ "source": "native-graph" }),
        nodes: vec![PlanNode {
            id: node_id.clone(),
            plan_id: plan_id.clone(),
            kind: PlanNodeKind::Validate,
            title: "Task A".into(),
            summary: Some("Preserve authored metadata".into()),
            status: PlanNodeStatus::Ready,
            bindings: prism_ir::PlanBinding {
                anchors: vec![AnchorRef::Node(alpha.clone())],
                concept_handles: vec!["concept://coordination_ux".into()],
                artifact_refs: vec!["artifact:coordination".into()],
                memory_refs: vec!["memory:coordination".into()],
                outcome_refs: vec!["outcome:coordination".into()],
            },
            acceptance: Vec::new(),
            validation_refs: vec![prism_ir::ValidationRef {
                id: "validation:coordination".into(),
            }],
            is_abstract: true,
            assignee: None,
            base_revision: WorkspaceRevision::default(),
            priority: Some(5),
            tags: vec!["native".into(), "metadata".into()],
            metadata: serde_json::json!({ "source": "native-node" }),
        }],
        edges: Vec::new(),
    };

    let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        ProjectionIndex::default(),
        vec![native_graph],
        BTreeMap::new(),
    );
    prism
        .acquire_native_claim(
            EventMeta {
                id: EventId::new("coord:claim:metadata-reload"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            SessionId::new("session:metadata"),
            prism_coordination::ClaimAcquireInput {
                task_id: Some(prism_ir::CoordinationTaskId::new(task_id.0.clone())),
                anchors: vec![AnchorRef::Node(alpha)],
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::Advisory),
                ttl_seconds: Some(60),
                base_revision: WorkspaceRevision::default(),
                current_revision: WorkspaceRevision::default(),
                agent: None,
                worktree_id: None,
                branch_ref: None,
            },
        )
        .expect("claim should succeed");

    let snapshot = prism.coordination_snapshot();
    let reloaded = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        snapshot,
        ProjectionIndex::default(),
    );

    let persisted = reloaded.plan_graph(&plan_id).expect("persisted graph");
    assert_eq!(persisted.title, "Native persistence migration");
    assert_eq!(persisted.kind, PlanKind::Migration);
    assert_eq!(persisted.revision, 9);
    assert_eq!(persisted.tags, vec!["persistence", "ux"]);
    assert_eq!(
        persisted.created_from.as_deref(),
        Some("concept://coordination_ux")
    );
    assert_eq!(persisted.metadata["source"], "native-graph");
    let node = persisted
        .nodes
        .into_iter()
        .find(|node| node.id == node_id)
        .expect("node should persist");
    assert_eq!(node.kind, PlanNodeKind::Validate);
    assert_eq!(node.summary.as_deref(), Some("Preserve authored metadata"));
    assert_eq!(node.priority, Some(5));
    assert_eq!(node.tags, vec!["native", "metadata"]);
    assert_eq!(
        node.bindings.concept_handles,
        vec!["concept://coordination_ux"]
    );
    assert_eq!(
        node.validation_refs
            .iter()
            .map(|value| value.id.as_str())
            .collect::<Vec<_>>(),
        vec!["validation:coordination"]
    );
    assert!(node.is_abstract);
    assert_eq!(node.metadata["source"], "native-node");
}

#[test]
fn validation_recipe_reuses_blast_radius_signal() {
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

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:5"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:validate")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha broke an integration test".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let recipe = prism.validation_recipe(&alpha);
    assert_eq!(recipe.target, alpha);
    assert_eq!(recipe.checks, vec!["test:alpha_integration"]);
    assert_eq!(recipe.scored_checks.len(), 1);
    assert_eq!(recipe.scored_checks[0].label, "test:alpha_integration");
    assert_eq!(recipe.recent_failures.len(), 1);
    assert_eq!(
        recipe.recent_failures[0].summary,
        "alpha broke an integration test"
    );
}

#[test]
fn resume_task_returns_correlated_events() {
    let graph = Graph::new();
    let history = HistoryStore::new();
    let outcomes = OutcomeMemory::new();
    let task = TaskId::new("task:fix");
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:3"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: Vec::new(),
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "applied patch".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:4"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: Some(EventId::new("outcome:3")),
            },
            anchors: Vec::new(),
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "validated patch".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let prism = Prism::with_history_and_outcomes(graph, history, outcomes);
    let replay = prism.resume_task(&task);
    assert_eq!(replay.events.len(), 2);
    assert_eq!(replay.events[0].summary, "validated patch");
}

#[test]
fn task_and_artifact_risk_join_coordination_with_change_intelligence() {
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

    let outcomes = OutcomeMemory::new();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:risk"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:risk")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha changes usually break integration".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Risky edit".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    review_required_above_risk_score: Some(0.2),
                    require_validation_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Edit alpha".into(),
                status: None,
                assignee: None,
                session: Some(SessionId::new("session:a")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    let (artifact_id, _) = coordination
        .propose_artifact(
            EventMeta {
                id: EventId::new("coord:artifact"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: vec![AnchorRef::Node(alpha.clone())],
                diff_ref: Some("patch:1".into()),
                evidence: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: vec!["test:alpha_integration".into()],
                validated_checks: Vec::new(),
                risk_score: Some(0.7),
                worktree_id: None,
                branch_ref: None,
            },
        )
        .unwrap();

    let projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination.snapshot(),
        projections,
    );

    let task_risk = prism.task_risk(&task_id, 5).unwrap();
    assert!(task_risk.review_required);
    assert_eq!(task_risk.likely_validations, vec!["test:alpha_integration"]);
    assert_eq!(
        task_risk.missing_validations,
        vec!["test:alpha_integration"]
    );

    let artifact_risk = prism.artifact_risk(&artifact_id, 5).unwrap();
    assert!(artifact_risk.review_required);
    assert_eq!(
        artifact_risk.missing_validations,
        vec!["test:alpha_integration"]
    );

    let blockers = prism.blockers(&task_id, 5);
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::RiskReviewRequired));
    assert!(blockers
        .iter()
        .any(|blocker| blocker.kind == prism_coordination::BlockerKind::ValidationRequired));
}

#[test]
fn exposes_intent_links_and_task_intent() {
    let mut graph = Graph::new();
    let spec = NodeId::new(
        "demo",
        "demo::document::docs::spec_md::behavior",
        NodeKind::MarkdownHeading,
    );
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let alpha_test = NodeId::new("demo", "demo::alpha_test", NodeKind::Function);
    graph.add_node(Node {
        id: spec.clone(),
        name: "Behavior".into(),
        kind: NodeKind::MarkdownHeading,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_node(Node {
        id: alpha_test.clone(),
        name: "alpha_test".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(2),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Specifies,
        source: spec.clone(),
        target: alpha.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 0.8,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Validates,
        source: spec.clone(),
        target: alpha_test.clone(),
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 0.8,
    });

    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:intent"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Ship alpha".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:intent"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Update alpha".into(),
                status: None,
                assignee: None,
                session: Some(SessionId::new("session:intent")),
                worktree_id: None,
                branch_ref: None,
                anchors: vec![AnchorRef::Node(alpha.clone())],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision::default(),
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );

    assert_eq!(prism.spec_for(&alpha), vec![spec.clone()]);
    assert_eq!(prism.implementation_for(&spec), vec![alpha.clone()]);

    let task_intent = prism.task_intent(&task_id).unwrap();
    assert_eq!(task_intent.specs, vec![spec.clone()]);
    assert_eq!(task_intent.implementations, vec![alpha.clone()]);
    assert_eq!(task_intent.validations, vec![alpha_test.clone()]);
    assert!(task_intent.drift_candidates.is_empty());
}

#[test]
fn drift_candidates_flag_specs_without_validations() {
    let mut graph = Graph::new();
    let spec = NodeId::new(
        "demo",
        "demo::document::docs::spec_md::contract",
        NodeKind::MarkdownHeading,
    );
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    graph.add_node(Node {
        id: spec.clone(),
        name: "Contract".into(),
        kind: NodeKind::MarkdownHeading,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Markdown,
    });
    graph.add_node(Node {
        id: alpha.clone(),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(2),
        span: Span::line(1),
        language: Language::Rust,
    });
    graph.add_edge(Edge {
        kind: EdgeKind::Specifies,
        source: spec.clone(),
        target: alpha,
        origin: prism_ir::EdgeOrigin::Static,
        confidence: 0.8,
    });

    let prism = Prism::new(graph);
    let drift = prism.drift_candidates(10);
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].spec, spec);
    assert!(drift[0]
        .reasons
        .iter()
        .any(|reason| reason == "no validation links"));
}

#[test]
fn policy_violations_expose_rejected_coordination_mutations() {
    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coord:plan:audit"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            PlanCreateInput {
                goal: "Require review".into(),
                status: None,
                policy: Some(CoordinationPolicy {
                    require_review_for_completion: true,
                    ..CoordinationPolicy::default()
                }),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coord:task:audit"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Edit alpha".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:audit")),
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();
    coordination
        .update_task(
            EventMeta {
                id: EventId::new("coord:reject:audit"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some(prism_ir::CoordinationTaskStatus::Completed),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                anchors: None,
                depends_on: None,
                acceptance: None,
                base_revision: None,
                completion_context: Some(TaskCompletionContext::default()),
            },
            WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .unwrap_err();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        Graph::new(),
        HistoryStore::new(),
        OutcomeMemory::new(),
        coordination.snapshot(),
        ProjectionIndex::default(),
    );
    let violations = prism.policy_violations(Some(&plan_id), Some(&task_id), 10);
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .violations
            .iter()
            .any(|violation| violation.code
                == prism_coordination::PolicyViolationCode::ReviewRequired)
    );
}
