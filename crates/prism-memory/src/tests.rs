use anyhow::Result;
use prism_ir::{
    AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageEvidence,
    LineageId, NodeId, NodeKind,
};
use serde_json::json;

use crate::{
    common::compare_scored_memory, EpisodicMemory, MemoryComposite, MemoryEntry, MemoryId,
    MemoryKind, MemoryModule, MemorySource, OutcomeEvent, OutcomeEvidence, OutcomeKind,
    OutcomeMemory, OutcomeRecallQuery, OutcomeResult, RecallQuery, ScoredMemory,
    SemanticBackendKind, SemanticMemory, SemanticMemoryConfig, SessionMemory, StructuralMemory,
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
            execution_context: None,
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

    assert!(id.0.starts_with("memory:"));
}

#[test]
fn session_memory_routes_structural_and_semantic_entries() {
    let memory = SessionMemory::new();

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
fn dedicated_modules_only_accept_their_own_kind() {
    let structural = StructuralMemory::new();
    let semantic = SemanticMemory::new();

    let mut structural_entry =
        MemoryEntry::new(MemoryKind::Structural, "alpha owns request routing");
    structural_entry.anchors = vec![anchor_node("alpha")];
    assert!(structural.store(structural_entry).is_ok());

    let mut semantic_entry = MemoryEntry::new(MemoryKind::Semantic, "alpha tends to fail");
    semantic_entry.anchors = vec![anchor_node("alpha")];
    assert!(semantic.store(semantic_entry).is_ok());

    let mut wrong_kind = MemoryEntry::new(MemoryKind::Semantic, "wrong");
    wrong_kind.anchors = vec![anchor_node("alpha")];
    assert!(structural.store(wrong_kind).is_err());
}

#[test]
fn structural_memory_recalls_rule_like_knowledge_by_tags() {
    let memory = StructuralMemory::new();

    let mut invariant = MemoryEntry::new(
        MemoryKind::Structural,
        "Billing migration must preserve the ledger invariant during backfill",
    );
    invariant.anchors = vec![anchor_node("alpha")];
    memory.store(invariant).unwrap();

    let mut review = MemoryEntry::new(
        MemoryKind::Structural,
        "Review auth and session changes together before rollout",
    );
    review.anchors = vec![anchor_node("alpha")];
    memory.store(review).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("migration invariant".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Structural]),
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0]
        .entry
        .content
        .contains("preserve the ledger invariant"));
    assert!(results[0]
        .explanation
        .as_deref()
        .is_some_and(|text| text.contains("structural tags")));
}

#[test]
fn structural_memory_prefers_promoted_rules_over_generic_notes() {
    let memory = StructuralMemory::new();

    let mut note = MemoryEntry::new(
        MemoryKind::Structural,
        "Alpha routing needed another follow-up after a failed rollout.",
    );
    note.anchors = vec![anchor_node("alpha")];
    memory.store(note).unwrap();

    let mut promoted = MemoryEntry::new(
        MemoryKind::Structural,
        "Changes in this area should run validation: test:alpha_regression",
    );
    promoted.anchors = vec![anchor_node("alpha")];
    promoted.trust = 0.82;
    promoted.metadata = json!({
        "category": "validation_rule",
        "provenance": {
            "origin": "curator",
            "kind": "structural_memory",
        },
        "evidence": {
            "eventIds": ["outcome:1", "outcome:2"],
            "validationChecks": ["test:alpha_regression"],
            "coChangeLineages": [],
        },
        "structuralRule": {
            "kind": "validation_rule",
            "promoted": true,
            "signalCount": 3,
        }
    });
    memory.store(promoted).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("what validation should run for alpha routing".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Structural]),
            since: None,
        })
        .unwrap();

    assert!(!results.is_empty());
    assert!(results[0]
        .entry
        .content
        .contains("should run validation: test:alpha_regression"));
    assert_eq!(
        results[0].entry.metadata["structuralRule"]["kind"],
        "validation_rule"
    );
    assert!(results[0]
        .explanation
        .as_deref()
        .is_some_and(|text| text.contains("rule kinds") && text.contains("promoted rule")));
}

#[test]
fn semantic_memory_recalls_metadata_backed_context() {
    let memory = SemanticMemory::new();

    let mut flaky = MemoryEntry::new(
        MemoryKind::Semantic,
        "Payments worker stalls when the upstream gateway slows down",
    );
    flaky.anchors = vec![anchor_node("alpha")];
    flaky.metadata = json!({
        "symptoms": ["timeout", "retry"],
        "surface": "payments",
    });
    memory.store(flaky).unwrap();

    let mut unrelated = MemoryEntry::new(
        MemoryKind::Semantic,
        "Search indexing gets noisy when filesystem watchers duplicate events",
    );
    unrelated.anchors = vec![anchor_node("alpha")];
    memory.store(unrelated).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("payments timeout retry".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Semantic]),
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].entry.content.contains("Payments worker stalls"));
    assert!(results[0]
        .explanation
        .as_deref()
        .is_some_and(|text| text.contains("semantic")));
}

#[test]
fn semantic_memory_alias_bridge_connects_login_to_auth_terms() {
    let memory = SemanticMemory::new();

    let mut auth = MemoryEntry::new(
        MemoryKind::Semantic,
        "Authentication state breaks when the credential cache races the session refresh.",
    );
    auth.anchors = vec![anchor_node("alpha")];
    memory.store(auth).unwrap();

    let mut unrelated = MemoryEntry::new(
        MemoryKind::Semantic,
        "Filesystem watchers duplicate events during search indexing refreshes.",
    );
    unrelated.anchors = vec![anchor_node("alpha")];
    memory.store(unrelated).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("login session credential issue".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Semantic]),
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0]
        .entry
        .content
        .contains("Authentication state breaks"));
    assert!(results[0]
        .explanation
        .as_deref()
        .is_some_and(|text| text.contains("alias")));
}

#[test]
fn semantic_memory_openai_preference_falls_back_to_local_without_runtime_support() {
    let memory = SemanticMemory::with_config(SemanticMemoryConfig {
        preferred_backend: SemanticBackendKind::OpenAi,
        openai: None,
    });

    let mut entry = MemoryEntry::new(
        MemoryKind::Semantic,
        "Ownership stays with the routing owner during migration review.",
    );
    entry.anchors = vec![anchor_node("alpha")];
    memory.store(entry).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("who owns the routing migration".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Semantic]),
            since: None,
        })
        .unwrap();

    assert_eq!(memory.configured_backend(), SemanticBackendKind::OpenAi);
    assert_eq!(results.len(), 1);
    assert!(results[0]
        .explanation
        .as_deref()
        .is_some_and(|text| text.contains("backend local-fallback")));
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
                execution_context: None,
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
fn outcome_query_filters_by_task_actor_result_and_since() {
    let outcomes = OutcomeMemory::new();
    let task = prism_ir::TaskId::new("task:alpha");
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:1"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
                execution_context: None,
            },
            anchors: vec![anchor_node("alpha")],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "agent failure".into(),
            evidence: vec![],
            metadata: json!({}),
        })
        .unwrap();
    outcomes
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:2"),
                ts: 15,
                actor: EventActor::User,
                correlation: Some(task.clone()),
                causation: None,
                execution_context: None,
            },
            anchors: vec![anchor_node("alpha")],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "user failure".into(),
            evidence: vec![],
            metadata: json!({}),
        })
        .unwrap();

    let events = outcomes.query_events(&OutcomeRecallQuery {
        anchors: vec![anchor_node("alpha")],
        task: Some(task),
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        actor: Some(EventActor::User),
        since: Some(10),
        limit: 10,
    });

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].summary, "user failure");
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
                execution_context: None,
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

#[test]
fn equal_score_tie_break_prefers_trust_then_source() {
    let mut lower_trust = MemoryEntry::new(MemoryKind::Episodic, "lower trust");
    lower_trust.id = MemoryId("memory:1".to_string());
    lower_trust.trust = 0.4;
    lower_trust.source = MemorySource::User;
    lower_trust.created_at = 100;

    let mut higher_trust = MemoryEntry::new(MemoryKind::Episodic, "higher trust");
    higher_trust.id = MemoryId("memory:2".to_string());
    higher_trust.trust = 0.9;
    higher_trust.source = MemorySource::Agent;
    higher_trust.created_at = 1;

    let mut system = MemoryEntry::new(MemoryKind::Episodic, "system");
    system.id = MemoryId("memory:3".to_string());
    system.trust = 0.8;
    system.source = MemorySource::System;
    system.created_at = 100;

    let mut user = MemoryEntry::new(MemoryKind::Episodic, "user");
    user.id = MemoryId("memory:4".to_string());
    user.trust = 0.8;
    user.source = MemorySource::User;
    user.created_at = 1;

    let mut scored = vec![
        ScoredMemory {
            id: lower_trust.id.clone(),
            entry: lower_trust,
            score: 0.75,
            source_module: "test".to_string(),
            explanation: None,
        },
        ScoredMemory {
            id: higher_trust.id.clone(),
            entry: higher_trust,
            score: 0.75,
            source_module: "test".to_string(),
            explanation: None,
        },
        ScoredMemory {
            id: system.id.clone(),
            entry: system,
            score: 0.70,
            source_module: "test".to_string(),
            explanation: None,
        },
        ScoredMemory {
            id: user.id.clone(),
            entry: user,
            score: 0.70,
            source_module: "test".to_string(),
            explanation: None,
        },
    ];
    scored.sort_by(compare_scored_memory);

    assert_eq!(scored[0].id.0, "memory:2");
    assert_eq!(scored[1].id.0, "memory:1");
    assert_eq!(scored[2].id.0, "memory:4");
    assert_eq!(scored[3].id.0, "memory:3");
}

#[test]
fn source_bias_does_not_override_more_relevant_recall() {
    let memory = EpisodicMemory::new();

    let mut relevant_agent = MemoryEntry::new(
        MemoryKind::Episodic,
        "load shedding regression in routing requires careful follow-up",
    );
    relevant_agent.anchors = vec![anchor_node("alpha")];
    relevant_agent.source = MemorySource::Agent;
    relevant_agent.trust = 0.8;
    relevant_agent.created_at = 100;
    memory.store(relevant_agent).unwrap();

    let mut less_relevant_user = MemoryEntry::new(
        MemoryKind::Episodic,
        "routing regression note with weaker overlap",
    );
    less_relevant_user.anchors = vec![anchor_node("alpha")];
    less_relevant_user.source = MemorySource::User;
    less_relevant_user.trust = 0.9;
    less_relevant_user.created_at = 100;
    memory.store(less_relevant_user).unwrap();

    let results = memory
        .recall(&RecallQuery {
            focus: vec![anchor_node("alpha")],
            text: Some("load shedding regression".into()),
            limit: 5,
            kinds: Some(vec![MemoryKind::Episodic]),
            since: None,
        })
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results[0]
        .entry
        .content
        .contains("load shedding regression in routing"));
}
