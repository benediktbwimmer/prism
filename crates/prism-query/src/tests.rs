use prism_coordination::{
    ArtifactProposeInput, CoordinationPolicy, CoordinationStore, PlanCreateInput, TaskCreateInput,
};
use prism_history::HistoryStore;
use prism_ir::{
    AnchorRef, ChangeTrigger, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language,
    Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, SessionId, Span, TaskId,
    WorkspaceRevision,
};
use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult};
use prism_projections::ProjectionIndex;
use prism_store::Graph;

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
            },
        )
        .unwrap();

    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        OutcomeMemory::new(),
        coordination,
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
            },
        )
        .unwrap();

    let projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
    let prism = Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination,
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
        coordination,
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
