use anyhow::Result;
use prism_ir::{
    AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageEvidence,
    LineageId, NodeId, NodeKind,
};
use serde_json::json;

use crate::{
    EpisodicMemory, MemoryComposite, MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource,
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult, RecallQuery,
    ScoredMemory,
};

fn node(name: &str) -> NodeId {
    NodeId::new("demo", format!("demo::{name}"), NodeKind::Function)
}

fn anchor_node(name: &str) -> AnchorRef {
    AnchorRef::Node(node(name))
}

fn lineage(name: &str) -> LineageId {
    LineageId::new(format!("lineage::{name}"))
}

fn lineage_event(
    lineage: LineageId,
    kind: LineageEventKind,
    before: Vec<NodeId>,
    after: Vec<NodeId>,
) -> LineageEvent {
    LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
        },
        lineage,
        kind,
        before,
        after,
        confidence: 1.0,
        evidence: vec![LineageEvidence::FingerprintMatch],
    }
}

#[test]
fn episodic_memory_generates_store_owned_ids() {
    let memory = EpisodicMemory::new();
    let mut entry = MemoryEntry::new(
        MemoryKind::Episodic,
        "Function alpha changed in commit abc123",
    );
    entry.anchors = vec![anchor_node("alpha")];
    entry.source = MemorySource::User;
    entry.trust = 1.0;

    let id = memory.store(entry).unwrap();

    assert_eq!(id.0, "memory:1");
}

#[test]
fn stored_memory_accepts_structural_and_semantic_kinds() {
    let memory = EpisodicMemory::new();

    let mut structural = MemoryEntry::new(MemoryKind::Structural, "alpha owns request routing");
    structural.anchors = vec![anchor_node("alpha")];
    let structural_id = memory.store(structural).unwrap();

    let mut semantic = MemoryEntry::new(MemoryKind::Semantic, "alpha tends to fail under load");
    semantic.anchors = vec![anchor_node("alpha")];
    let semantic_id = memory.store(semantic).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: None,
            limit: 10,
            kinds: Some(vec![MemoryKind::Structural, MemoryKind::Semantic]),
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 2);
    let ids = results
        .into_iter()
        .map(|memory| memory.id)
        .collect::<Vec<_>>();
    assert!(ids.contains(&structural_id));
    assert!(ids.contains(&semantic_id));
}

#[test]
fn episodic_snapshot_round_trip_preserves_ids() {
    let memory = EpisodicMemory::new();
    let mut entry = MemoryEntry::new(MemoryKind::Episodic, "alpha needed a follow-up fix");
    entry.anchors = vec![anchor_node("alpha")];
    entry.created_at = 42;
    let id = memory.store(entry).unwrap();

    let restored = EpisodicMemory::from_snapshot(memory.snapshot());
    let results = restored
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, id);
    assert_eq!(results[0].entry.created_at, 42);
}

#[test]
fn recall_uses_anchor_overlap_and_since_filter() {
    let memory = EpisodicMemory::new();

    let mut alpha = MemoryEntry::new(
        MemoryKind::Episodic,
        "Bug report mentioned alpha null handling",
    );
    alpha.anchors = vec![anchor_node("alpha"), anchor_node("beta")];
    alpha.created_at = 1_000;
    alpha.source = MemorySource::User;
    alpha.trust = 1.0;
    alpha.metadata = json!({"issue": "BUG-1"});
    memory.store(alpha).unwrap();

    let mut beta = MemoryEntry::new(
        MemoryKind::Episodic,
        "User noted beta is performance sensitive",
    );
    beta.anchors = vec![anchor_node("beta")];
    beta.created_at = 2_000;
    beta.source = MemorySource::System;
    beta.trust = 0.8;
    memory.store(beta).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("beta")],
            text: Some("performance".into()),
            limit: 10,
            kinds: Some(vec![MemoryKind::Episodic]),
            since: Some(1_500),
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].entry.content.contains("performance sensitive"));
}

#[test]
fn lineage_reanchoring_moves_memory_to_new_node_id_and_adds_lineage_anchor() {
    let memory = EpisodicMemory::new();
    let old = node("alpha");
    let new = node("renamed_alpha");
    let symbol_lineage = lineage("alpha");

    let mut entry = MemoryEntry::new(
        MemoryKind::Episodic,
        "Function alpha changed in commit abc123",
    );
    entry.anchors = vec![AnchorRef::Node(old.clone())];
    memory.store(entry).unwrap();

    memory
        .apply_lineage(&[lineage_event(
            symbol_lineage.clone(),
            LineageEventKind::Renamed,
            vec![old.clone()],
            vec![new.clone()],
        )])
        .unwrap();

    let old_results = memory
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Node(old)],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();
    let new_results = memory
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Node(new.clone())],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();
    let lineage_results = memory
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Lineage(symbol_lineage)],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();

    assert!(old_results.is_empty());
    assert_eq!(new_results.len(), 1);
    assert_eq!(lineage_results.len(), 1);
    assert!(lineage_results[0]
        .entry
        .anchors
        .contains(&AnchorRef::Node(new)));
}

#[test]
fn died_lineage_preserves_memory_via_lineage_anchor() {
    let memory = EpisodicMemory::new();
    let alpha = node("alpha");
    let symbol_lineage = lineage("alpha");

    let mut entry = MemoryEntry::new(MemoryKind::Episodic, "User noted alpha is sensitive");
    entry.anchors = vec![AnchorRef::Node(alpha.clone())];
    memory.store(entry).unwrap();

    memory
        .apply_lineage(&[lineage_event(
            symbol_lineage.clone(),
            LineageEventKind::Died,
            vec![alpha.clone()],
            Vec::new(),
        )])
        .unwrap();

    let removed_results = memory
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Node(alpha)],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();
    let lineage_results = memory
        .recall(&RecallQuery {
            focus: vec![AnchorRef::Lineage(symbol_lineage)],
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();

    assert!(removed_results.is_empty());
    assert_eq!(lineage_results.len(), 1);
}

struct StaticModule {
    name: &'static str,
    score: f32,
    id: &'static str,
}

impl MemoryModule for StaticModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        kind == MemoryKind::Episodic
    }

    fn store(&self, _entry: MemoryEntry) -> Result<MemoryId> {
        Ok(MemoryId(self.id.to_string()))
    }

    fn recall(&self, _query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        Ok(vec![ScoredMemory {
            id: MemoryId(self.id.to_string()),
            entry: MemoryEntry::new(MemoryKind::Episodic, format!("from {}", self.name)),
            score: self.score,
            source_module: self.name.to_string(),
            explanation: Some("static test result".to_string()),
        }])
    }

    fn apply_lineage(&self, _events: &[LineageEvent]) -> Result<()> {
        Ok(())
    }
}

#[test]
fn composite_clamps_weights_and_dedupes_ids() {
    let composite = MemoryComposite::new()
        .with_module(
            StaticModule {
                name: "first",
                score: 1.4,
                id: "shared",
            },
            0.25,
        )
        .with_module(
            StaticModule {
                name: "second",
                score: 0.8,
                id: "shared",
            },
            1.0,
        );

    let results = composite
        .recall(&RecallQuery {
            focus: Vec::new(),
            text: None,
            limit: 10,
            kinds: None,
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_module, "second");
    assert!((results[0].score - 0.8).abs() < f32::EPSILON);
}

#[test]
fn outcome_queries_and_resume_task_work() {
    let outcomes = OutcomeMemory::new();
    let task = prism_ir::TaskId::new("task:alpha");
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:1"),
                ts: 10,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: vec![anchor_node("alpha")],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha failed".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_test".into(),
                passed: false,
            }],
            metadata: json!({}),
        })
        .unwrap();

    let failures = outcomes.related_failures(&[anchor_node("alpha")], 10);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].summary, "alpha failed");

    let replay = outcomes.resume_task(&task);
    assert_eq!(replay.events.len(), 1);
}

#[test]
fn outcome_lineage_reanchor_moves_event_anchor() {
    let outcomes = OutcomeMemory::new();
    let old = node("alpha");
    let new = node("renamed_alpha");
    let symbol_lineage = lineage("alpha");

    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:1"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            anchors: vec![AnchorRef::Node(old.clone())],
            kind: OutcomeKind::PatchApplied,
            result: OutcomeResult::Success,
            summary: "patched alpha".into(),
            evidence: vec![],
            metadata: json!({}),
        })
        .unwrap();

    outcomes
        .apply_lineage(&[lineage_event(
            symbol_lineage.clone(),
            LineageEventKind::Renamed,
            vec![old],
            vec![new.clone()],
        )])
        .unwrap();

    let by_new = outcomes.outcomes_for(&[AnchorRef::Node(new)], 10);
    let by_lineage = outcomes.outcomes_for(&[AnchorRef::Lineage(symbol_lineage)], 10);
    assert_eq!(by_new.len(), 1);
    assert_eq!(by_lineage.len(), 1);
}
