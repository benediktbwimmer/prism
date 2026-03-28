use prism_history::HistorySnapshot;
use prism_ir::{
    AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageId, NodeId,
    NodeKind, TaskId,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult,
};

use crate::projections::{ProjectionIndex, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE};
use crate::{
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptScope,
};

#[test]
fn derives_validation_and_co_change_indexes() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:1");
    let beta_lineage = LineageId::new("lineage:2");
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        events: Vec::new(),
        co_change_counts: vec![(alpha_lineage.clone(), beta_lineage.clone(), 3)],
        tombstones: Vec::new(),
        next_lineage: 2,
        next_event: 0,
    };
    let outcomes = OutcomeMemorySnapshot {
        events: vec![OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:1"),
                ts: 10,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:1")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        }],
    };

    let index = ProjectionIndex::derive(&history, &outcomes);
    let checks = index.validation_checks_for_lineages(&[alpha_lineage.clone()], 10);
    assert_eq!(checks[0].label, "test:alpha_integration");
    assert_eq!(checks[0].last_seen, 10);

    let neighbors = index.co_change_neighbors(&alpha_lineage, 10);
    assert_eq!(neighbors[0].lineage, beta_lineage);
    assert_eq!(neighbors[0].count, 3);
}

#[test]
fn incremental_updates_match_derived_index() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:1");
    let beta_lineage = LineageId::new("lineage:2");
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        events: Vec::new(),
        co_change_counts: vec![(alpha_lineage.clone(), beta_lineage.clone(), 1)],
        tombstones: Vec::new(),
        next_lineage: 2,
        next_event: 0,
    };
    let event = OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:2"),
            ts: 11,
            actor: EventActor::Agent,
            correlation: Some(TaskId::new("task:2")),
            causation: None,
        },
        anchors: vec![AnchorRef::Node(alpha.clone())],
        kind: OutcomeKind::FailureObserved,
        result: OutcomeResult::Failure,
        summary: "alpha failed".into(),
        evidence: vec![OutcomeEvidence::Test {
            name: "alpha_unit".into(),
            passed: false,
        }],
        metadata: serde_json::Value::Null,
    };
    let derived = ProjectionIndex::derive(
        &history,
        &OutcomeMemorySnapshot {
            events: vec![event.clone()],
        },
    );

    let mut incremental = ProjectionIndex::new();
    incremental.apply_lineage_events(&[
        LineageEvent {
            meta: EventMeta {
                id: EventId::new("lineage:1"),
                ts: 10,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            lineage: alpha_lineage.clone(),
            kind: LineageEventKind::Updated,
            before: vec![alpha.clone()],
            after: vec![alpha.clone()],
            confidence: 1.0,
            evidence: Vec::new(),
        },
        LineageEvent {
            meta: EventMeta {
                id: EventId::new("lineage:2"),
                ts: 10,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            lineage: beta_lineage.clone(),
            kind: LineageEventKind::Updated,
            before: vec![beta.clone()],
            after: vec![beta.clone()],
            confidence: 1.0,
            evidence: Vec::new(),
        },
    ]);
    incremental.apply_outcome_event(&event, |node| {
        if node == &alpha {
            Some(alpha_lineage.clone())
        } else if node == &beta {
            Some(beta_lineage.clone())
        } else {
            None
        }
    });

    assert_eq!(incremental.snapshot(), derived.snapshot());
}

#[test]
fn co_change_neighbors_are_pruned_to_top_k() {
    let source = LineageId::new("lineage:source");
    let history = HistorySnapshot {
        node_to_lineage: Vec::new(),
        events: Vec::new(),
        co_change_counts: (0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8))
            .map(|index| {
                (
                    source.clone(),
                    LineageId::new(format!("lineage:{index:03}")),
                    (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8 - index) as u32,
                )
            })
            .collect(),
        tombstones: Vec::new(),
        next_lineage: 0,
        next_event: 0,
    };

    let index = ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    let neighbors = index.co_change_neighbors(&source, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 16);

    assert_eq!(neighbors.len(), MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);
    assert_eq!(
        neighbors.first().unwrap().count,
        (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) as u32
    );
    assert_eq!(neighbors.last().unwrap().count, 9);
}

#[test]
fn derives_seeded_repo_concepts_from_nodes_and_signals() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let session = NodeId::new(
        "demo",
        "demo::session_state::SessionState::start_task",
        NodeKind::Method,
    );
    let runtime = NodeId::new("demo", "demo::runtime::runtime_status", NodeKind::Function);
    let validation_lineage = LineageId::new("lineage:validation");
    let session_lineage = LineageId::new("lineage:session");
    let runtime_lineage = LineageId::new("lineage:runtime");
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (validation.clone(), validation_lineage.clone()),
            (session.clone(), session_lineage.clone()),
            (runtime.clone(), runtime_lineage.clone()),
        ],
        events: Vec::new(),
        co_change_counts: vec![(validation_lineage.clone(), session_lineage, 2)],
        tombstones: Vec::new(),
        next_lineage: 3,
        next_event: 0,
    };
    let outcomes = OutcomeMemorySnapshot {
        events: vec![OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:concept"),
                ts: 42,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:concept")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(validation.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "validation failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "validation_concept".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        }],
    };

    let index = ProjectionIndex::derive(&history, &outcomes);
    let concepts = index.concepts("validation", 3);

    assert_eq!(concepts[0].handle, "concept://validation_pipeline");
    assert!(concepts[0]
        .core_members
        .iter()
        .any(|node| node.path == validation.path));
    assert!(index
        .concept_by_handle("concept://session_lifecycle")
        .is_some());
    assert!(index
        .concept_by_handle("concept://runtime_surface")
        .is_some());
}

#[test]
fn curated_concept_events_override_seeded_packets_by_handle() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let runtime = NodeId::new("demo", "demo::runtime_status", NodeKind::Function);
    let validation_lineage = LineageId::new("lineage:validation");
    let runtime_lineage = LineageId::new("lineage:runtime");
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (validation.clone(), validation_lineage),
            (runtime.clone(), runtime_lineage),
        ],
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 0,
    };
    let mut index =
        ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    index.replace_curated_concepts_from_events(&[ConceptEvent {
        id: "concept-event:1".to_string(),
        recorded_at: 7,
        task_id: Some("task:concept".to_string()),
        action: ConceptEventAction::Promote,
        concept: ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Curated validation concept".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.97,
            core_members: vec![validation.clone(), runtime.clone()],
            core_member_lineages: vec![
                Some(LineageId::new("lineage:validation")),
                Some(LineageId::new("lineage:runtime")),
            ],
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            evidence: vec!["Curated from repo work.".to_string()],
            risk_hint: Some("Config drift common".to_string()),
            decode_lenses: vec![ConceptDecodeLens::Validation, ConceptDecodeLens::Memory],
            scope: ConceptScope::Repo,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated_concept_event".to_string(),
                task_id: Some("task:concept".to_string()),
            },
            publication: Some(ConceptPublication {
                published_at: 7,
                last_reviewed_at: Some(7),
                status: ConceptPublicationStatus::Active,
                supersedes: Vec::new(),
                retired_at: None,
                retirement_reason: None,
            }),
        },
    }]);

    let concept = index
        .concept_by_handle("concept://validation_pipeline")
        .expect("curated concept should resolve");
    assert_eq!(concept.summary, "Curated validation concept");
    assert_eq!(concept.confidence, 0.97);
    assert_eq!(
        index.concepts("validation", 1)[0].summary,
        "Curated validation concept"
    );
}

#[test]
fn curated_concepts_rebind_members_from_lineage_after_rename() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let renamed_alpha = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:alpha");
    let beta_lineage = LineageId::new("lineage:beta");
    let rename_event = LineageEvent {
        meta: EventMeta {
            id: EventId::new("lineage:rename-alpha"),
            ts: 12,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        lineage: alpha_lineage.clone(),
        kind: LineageEventKind::Renamed,
        before: vec![alpha.clone()],
        after: vec![renamed_alpha.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    };
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (renamed_alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        events: vec![rename_event],
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 2,
        next_event: 1,
    };
    let concept = ConceptPacket {
        handle: "concept://alpha_flow".to_string(),
        canonical_name: "alpha_flow".to_string(),
        summary: "Curated alpha concept".to_string(),
        aliases: vec!["alpha".to_string()],
        confidence: 0.94,
        core_members: vec![alpha.clone(), beta.clone()],
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Observed in repo work.".to_string()],
        risk_hint: None,
        decode_lenses: vec![ConceptDecodeLens::Open, ConceptDecodeLens::Workset],
        scope: ConceptScope::Repo,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "curated_concept_event".to_string(),
            task_id: Some("task:concept".to_string()),
        },
        publication: Some(ConceptPublication {
            published_at: 12,
            last_reviewed_at: Some(12),
            status: ConceptPublicationStatus::Active,
            supersedes: Vec::new(),
            retired_at: None,
            retirement_reason: None,
        }),
    };

    let index = ProjectionIndex::derive_with_curated(
        &history,
        &OutcomeMemorySnapshot { events: Vec::new() },
        vec![concept],
    );
    let rebound = index
        .concept_by_handle("concept://alpha_flow")
        .expect("curated concept should resolve");

    assert!(rebound
        .core_members
        .iter()
        .any(|node| node.path == renamed_alpha.path));
    assert!(!rebound
        .core_members
        .iter()
        .any(|node| node.path == alpha.path));
    assert_eq!(
        rebound.core_member_lineages.first().cloned().flatten(),
        Some(alpha_lineage)
    );
}
