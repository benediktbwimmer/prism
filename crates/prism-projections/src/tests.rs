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
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptPacket,
    ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationKind, ConceptScope,
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
fn projection_index_tracks_direct_concept_relations() {
    let history = HistorySnapshot {
        node_to_lineage: Vec::new(),
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 0,
        next_event: 0,
    };
    let outcomes = OutcomeMemorySnapshot { events: Vec::new() };
    let validation = ConceptPacket {
        handle: "concept://validation_pipeline".to_string(),
        canonical_name: "validation_pipeline".to_string(),
        summary: "Checks and likely tests for a change.".to_string(),
        aliases: vec!["validation".to_string()],
        confidence: 0.9,
        core_members: vec![
            NodeId::new("demo", "demo::validation_recipe", NodeKind::Function),
            NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
        ],
        core_member_lineages: vec![None, None],
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
            kind: "projection_test".to_string(),
            task_id: None,
        },
        publication: None,
    };
    let runtime = ConceptPacket {
        handle: "concept://runtime_surface".to_string(),
        canonical_name: "runtime_surface".to_string(),
        summary: "Entry points and runtime status.".to_string(),
        aliases: vec!["runtime".to_string()],
        confidence: 0.88,
        core_members: vec![
            NodeId::new("demo", "demo::runtime_status", NodeKind::Function),
            NodeId::new("demo", "demo::start_task", NodeKind::Function),
        ],
        core_member_lineages: vec![None, None],
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
            kind: "projection_test".to_string(),
            task_id: None,
        },
        publication: None,
    };
    let relation = ConceptRelation {
        source_handle: validation.handle.clone(),
        target_handle: runtime.handle.clone(),
        kind: ConceptRelationKind::OftenUsedWith,
        confidence: 0.82,
        evidence: vec!["Validation work routes through runtime status.".to_string()],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "projection_test".to_string(),
            task_id: None,
        },
    };

    let index = ProjectionIndex::derive_with_knowledge(
        &history,
        &outcomes,
        vec![validation.clone(), runtime.clone()],
        vec![relation.clone()],
    );

    let neighbors = index.concept_relations_for_handle(&validation.handle);
    assert_eq!(neighbors.len(), 1);
    assert_eq!(neighbors[0].target_handle, runtime.handle);
    assert_eq!(neighbors[0].kind, ConceptRelationKind::OftenUsedWith);
    assert_eq!(index.snapshot().concept_relations, vec![relation]);
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
fn projection_has_no_concepts_without_curated_packets() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let runtime = NodeId::new("demo", "demo::runtime::runtime_status", NodeKind::Function);
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (validation, LineageId::new("lineage:validation")),
            (runtime, LineageId::new("lineage:runtime")),
        ],
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 2,
        next_event: 0,
    };
    let outcomes = OutcomeMemorySnapshot { events: Vec::new() };

    let index = ProjectionIndex::derive(&history, &outcomes);

    assert!(index.concepts("validation", 3).is_empty());
    assert!(index
        .concept_by_handle("concept://validation_pipeline")
        .is_none());
}

#[test]
fn curated_concept_events_resolve_by_handle_and_query() {
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
        patch: None,
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
fn concept_resolution_handles_typo_tolerant_alias_queries() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let history = HistorySnapshot {
        node_to_lineage: vec![(validation.clone(), LineageId::new("lineage:validation"))],
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 0,
    };
    let mut index =
        ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    index.replace_curated_concepts_from_events(&[ConceptEvent {
        id: "concept-event:fuzzy".to_string(),
        recorded_at: 9,
        task_id: Some("task:fuzzy".to_string()),
        action: ConceptEventAction::Promote,
        patch: None,
        concept: ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Validation checks and likely tests.".to_string(),
            aliases: vec!["validation".to_string(), "checks".to_string()],
            confidence: 0.95,
            core_members: vec![validation],
            core_member_lineages: vec![Some(LineageId::new("lineage:validation"))],
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
                kind: "curated".to_string(),
                task_id: Some("task:fuzzy".to_string()),
            },
            publication: None,
        },
    }]);

    let resolutions = index.resolve_concepts("validaton", 2);

    assert_eq!(resolutions.len(), 1);
    assert_eq!(
        resolutions[0].packet.handle,
        "concept://validation_pipeline"
    );
    assert!(resolutions[0]
        .reasons
        .iter()
        .any(|reason| reason.contains("fuzzy alias")));
}

#[test]
fn concept_health_reports_drift_for_unvalidated_test_heavy_concepts() {
    let validation = NodeId::new("demo", "demo::validation_recipe", NodeKind::Function);
    let validation_test = NodeId::new("demo", "demo::validation_recipe_test", NodeKind::Function);
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (validation.clone(), LineageId::new("lineage:validation")),
            (
                validation_test.clone(),
                LineageId::new("lineage:validation_test"),
            ),
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
        id: "concept-event:health".to_string(),
        recorded_at: 11,
        task_id: Some("task:health".to_string()),
        action: ConceptEventAction::Promote,
        patch: Some(ConceptEventPatch {
            set_fields: vec!["riskHint".to_string()],
            cleared_fields: Vec::new(),
            ..ConceptEventPatch::default()
        }),
        concept: ConceptPacket {
            handle: "concept://validation_pipeline".to_string(),
            canonical_name: "validation_pipeline".to_string(),
            summary: "Validation checks and likely tests.".to_string(),
            aliases: vec!["validation".to_string()],
            confidence: 0.95,
            core_members: vec![validation],
            core_member_lineages: vec![Some(LineageId::new("lineage:validation"))],
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            likely_tests: vec![validation_test],
            likely_test_lineages: vec![Some(LineageId::new("lineage:validation_test"))],
            evidence: vec!["Curated in test.".to_string()],
            risk_hint: Some("Validation drift is common.".to_string()),
            decode_lenses: vec![ConceptDecodeLens::Validation],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "curated".to_string(),
                task_id: Some("task:health".to_string()),
            },
            publication: None,
        },
    }]);

    let health = index
        .concept_health("concept://validation_pipeline")
        .expect("health should resolve");

    assert_eq!(health.status, crate::ConceptHealthStatus::Drifted);
    assert!(health.signals.stale_validation_links);
    assert_eq!(health.signals.live_core_member_ratio, 1.0);
}

#[test]
fn concept_updates_replay_from_typed_patch_payload() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let history = HistorySnapshot {
        node_to_lineage: vec![
            (alpha.clone(), LineageId::new("lineage:alpha")),
            (beta.clone(), LineageId::new("lineage:beta")),
        ],
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 0,
    };
    let mut index =
        ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    index.replace_curated_concepts_from_events(&[
        ConceptEvent {
            id: "concept-event:base".to_string(),
            recorded_at: 13,
            task_id: Some("task:concept-base".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://validation_pipeline".to_string(),
                canonical_name: "validation_pipeline".to_string(),
                summary: "Original validation concept summary.".to_string(),
                aliases: vec!["validation".to_string()],
                confidence: 0.9,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: vec![
                    Some(LineageId::new("lineage:alpha")),
                    Some(LineageId::new("lineage:beta")),
                ],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Curated in test.".to_string()],
                risk_hint: Some("Old risk hint".to_string()),
                decode_lenses: vec![ConceptDecodeLens::Validation],
                scope: ConceptScope::Session,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "curated".to_string(),
                    task_id: Some("task:concept-base".to_string()),
                },
                publication: None,
            },
        },
        ConceptEvent {
            id: "concept-event:update".to_string(),
            recorded_at: 14,
            task_id: Some("task:concept-update".to_string()),
            action: ConceptEventAction::Update,
            patch: Some(ConceptEventPatch {
                set_fields: vec!["summary".to_string()],
                cleared_fields: vec!["riskHint".to_string()],
                summary: Some("Patched validation concept summary.".to_string()),
                ..ConceptEventPatch::default()
            }),
            concept: ConceptPacket {
                handle: "concept://validation_pipeline".to_string(),
                canonical_name: "validation_pipeline".to_string(),
                summary: "Stale full post image should be ignored.".to_string(),
                aliases: vec!["validation".to_string()],
                confidence: 0.9,
                core_members: vec![alpha, beta],
                core_member_lineages: vec![
                    Some(LineageId::new("lineage:alpha")),
                    Some(LineageId::new("lineage:beta")),
                ],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Curated in test.".to_string()],
                risk_hint: Some("Stale risk hint should be ignored".to_string()),
                decode_lenses: vec![ConceptDecodeLens::Validation],
                scope: ConceptScope::Session,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "curated".to_string(),
                    task_id: Some("task:concept-update".to_string()),
                },
                publication: None,
            },
        },
    ]);

    let concept = index
        .concept_by_handle("concept://validation_pipeline")
        .expect("curated concept should resolve");
    assert_eq!(concept.summary, "Patched validation concept summary.");
    assert_eq!(concept.risk_hint, None);
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
