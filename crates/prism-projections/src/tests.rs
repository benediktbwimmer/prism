use prism_history::HistorySnapshot;
use prism_ir::{
    AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageId, NodeId,
    NodeKind, TaskId,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult,
};

use crate::projections::ProjectionIndex;

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
