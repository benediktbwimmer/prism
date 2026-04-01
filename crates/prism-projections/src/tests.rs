use prism_history::HistorySnapshot;
use prism_ir::{
    AnchorRef, EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageId, NodeId,
    NodeKind, TaskId,
};
use prism_memory::{
    OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult,
};

use crate::projections::{
    co_change_delta_batch_for_events, co_change_deltas_for_events, ProjectionIndex,
    MAX_CO_CHANGE_LINEAGES_PER_CHANGESET, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE,
};
use crate::{
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptPacket,
    ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationKind, ConceptScope, ContractCompatibility, ContractEvent, ContractEventAction,
    ContractGuarantee, ContractKind, ContractPacket, ContractStatus, ContractTarget,
};

fn history_snapshot(
    node_to_lineage: Vec<(NodeId, LineageId)>,
    events: Vec<LineageEvent>,
    next_lineage: u64,
    next_event: u64,
) -> HistorySnapshot {
    HistorySnapshot {
        node_to_lineage,
        events,
        tombstones: Vec::new(),
        next_lineage,
        next_event,
    }
}

fn updated_event(
    change_set_id: &str,
    event_id: &str,
    ts: u64,
    lineage: &LineageId,
    node: &NodeId,
) -> LineageEvent {
    LineageEvent {
        meta: EventMeta {
            id: EventId::new(event_id),
            ts,
            actor: EventActor::System,
            correlation: None,
            causation: Some(EventId::new(change_set_id)),
        },
        lineage: lineage.clone(),
        kind: LineageEventKind::Updated,
        before: vec![node.clone()],
        after: vec![node.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    }
}

#[test]
fn derives_validation_and_co_change_indexes() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:1");
    let beta_lineage = LineageId::new("lineage:2");
    let history = history_snapshot(
        vec![
            (alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        vec![
            updated_event("change-set:1", "lineage:1", 10, &alpha_lineage, &alpha),
            updated_event("change-set:1", "lineage:2", 10, &beta_lineage, &beta),
            updated_event("change-set:2", "lineage:3", 11, &alpha_lineage, &alpha),
            updated_event("change-set:2", "lineage:4", 11, &beta_lineage, &beta),
            updated_event("change-set:3", "lineage:5", 12, &alpha_lineage, &alpha),
            updated_event("change-set:3", "lineage:6", 12, &beta_lineage, &beta),
        ],
        2,
        6,
    );
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
    let history = history_snapshot(Vec::new(), Vec::new(), 0, 0);
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
    let history = history_snapshot(
        vec![
            (alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        vec![
            updated_event("change-set:1", "lineage:1", 10, &alpha_lineage, &alpha),
            updated_event("change-set:1", "lineage:2", 10, &beta_lineage, &beta),
        ],
        2,
        2,
    );
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
    let source_node = NodeId::new("demo", "demo::source", NodeKind::Function);
    let mut node_to_lineage = vec![(source_node.clone(), source.clone())];
    let mut events = Vec::new();
    let mut ts = 0u64;
    for index in 0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) {
        let target = LineageId::new(format!("lineage:{index:03}"));
        let target_node = NodeId::new(
            "demo",
            format!("demo::target_{index:03}"),
            NodeKind::Function,
        );
        node_to_lineage.push((target_node.clone(), target.clone()));
        for repeat in 0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8 - index) {
            let change_set_id = format!("change-set:{index}:{repeat}");
            events.push(updated_event(
                &change_set_id,
                &format!("lineage:source:{index}:{repeat}"),
                ts,
                &source,
                &source_node,
            ));
            events.push(updated_event(
                &change_set_id,
                &format!("lineage:target:{index}:{repeat}"),
                ts,
                &target,
                &target_node,
            ));
            ts += 1;
        }
    }
    let history = history_snapshot(node_to_lineage, events, 0, ts);

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
fn co_change_deltas_sample_bulk_changesets_above_guardrail() {
    let events = (0..(MAX_CO_CHANGE_LINEAGES_PER_CHANGESET + 1))
        .map(|index| LineageEvent {
            meta: EventMeta {
                id: EventId::new(format!("lineage:{index}")),
                ts: index as u64,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            lineage: LineageId::new(format!("lineage:{index}")),
            kind: LineageEventKind::Updated,
            before: Vec::new(),
            after: Vec::new(),
            confidence: 1.0,
            evidence: Vec::new(),
        })
        .collect::<Vec<_>>();

    let batch = co_change_delta_batch_for_events(&events);
    assert!(batch.truncated);
    assert_eq!(
        batch.distinct_lineage_count,
        MAX_CO_CHANGE_LINEAGES_PER_CHANGESET + 1
    );
    assert_eq!(
        batch.sampled_lineage_count,
        MAX_CO_CHANGE_LINEAGES_PER_CHANGESET
    );
    assert_eq!(
        batch.deltas.len(),
        MAX_CO_CHANGE_LINEAGES_PER_CHANGESET * (MAX_CO_CHANGE_LINEAGES_PER_CHANGESET - 1)
    );
    assert!(!co_change_deltas_for_events(&events).is_empty());
}

#[test]
fn apply_lineage_events_with_precomputed_co_change_deltas_matches_default_path() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let gamma = NodeId::new("demo", "demo::gamma", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:alpha");
    let beta_lineage = LineageId::new("lineage:beta");
    let gamma_lineage = LineageId::new("lineage:gamma");
    let events = vec![
        updated_event(
            "change-set:1",
            "lineage:event:1",
            10,
            &alpha_lineage,
            &alpha,
        ),
        updated_event("change-set:1", "lineage:event:2", 10, &beta_lineage, &beta),
        updated_event(
            "change-set:1",
            "lineage:event:3",
            10,
            &gamma_lineage,
            &gamma,
        ),
    ];
    let deltas = co_change_deltas_for_events(&events);

    let mut default_index = ProjectionIndex::new();
    default_index.apply_lineage_events(&events);

    let mut precomputed_index = ProjectionIndex::new();
    precomputed_index.apply_lineage_events_with_co_change_deltas(&events, &deltas);

    assert_eq!(default_index.snapshot(), precomputed_index.snapshot());
}

#[test]
fn projection_has_no_concepts_without_curated_packets() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let runtime = NodeId::new("demo", "demo::runtime::runtime_status", NodeKind::Function);
    let history = history_snapshot(
        vec![
            (validation, LineageId::new("lineage:validation")),
            (runtime, LineageId::new("lineage:runtime")),
        ],
        Vec::new(),
        2,
        0,
    );
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
    let history = history_snapshot(
        vec![
            (validation.clone(), validation_lineage),
            (runtime.clone(), runtime_lineage),
        ],
        Vec::new(),
        1,
        0,
    );
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
fn curated_contract_events_resolve_by_handle_and_query() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let history = history_snapshot(
        vec![(alpha.clone(), LineageId::new("lineage:alpha"))],
        Vec::new(),
        1,
        0,
    );
    let mut index =
        ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    index.replace_curated_contracts_from_events(&[ContractEvent {
        id: "contract-event:1".to_string(),
        recorded_at: 7,
        task_id: Some("task:contract".to_string()),
        action: ContractEventAction::Promote,
        patch: None,
        contract: ContractPacket {
            handle: "contract://alpha_api".to_string(),
            name: "alpha_api".to_string(),
            summary: "The alpha surface preserves a stable internal callable contract.".to_string(),
            aliases: vec!["alpha api".to_string(), "alpha contract".to_string()],
            kind: ContractKind::Interface,
            subject: ContractTarget {
                anchors: vec![AnchorRef::Node(alpha)],
                concept_handles: Vec::new(),
            },
            guarantees: vec![ContractGuarantee {
                id: "alpha_name_stable".to_string(),
                statement: "Callers may rely on the alpha function name staying stable."
                    .to_string(),
                scope: Some("internal callers".to_string()),
                strength: None,
                evidence_refs: vec!["validation:test-alpha".to_string()],
            }],
            assumptions: vec!["Only internal callers rely on this surface.".to_string()],
            consumers: Vec::new(),
            validations: Vec::new(),
            stability: crate::ContractStability::Internal,
            compatibility: ContractCompatibility {
                additive: vec!["Adding optional parameters is additive.".to_string()],
                breaking: vec!["Renaming alpha is breaking.".to_string()],
                ..ContractCompatibility::default()
            },
            evidence: vec!["Seeded from a contract test.".to_string()],
            status: ContractStatus::Active,
            scope: crate::ContractScope::Repo,
            provenance: crate::ContractProvenance {
                origin: "test".to_string(),
                kind: "curated_contract_event".to_string(),
                task_id: Some("task:contract".to_string()),
            },
            publication: Some(crate::ContractPublication {
                published_at: 7,
                last_reviewed_at: Some(7),
                status: crate::ContractPublicationStatus::Active,
                supersedes: Vec::new(),
                retired_at: None,
                retirement_reason: None,
            }),
        },
    }]);

    let contract = index
        .contract_by_handle("contract://alpha_api")
        .expect("curated contract should resolve");
    assert_eq!(contract.kind, ContractKind::Interface);
    assert_eq!(contract.guarantees.len(), 1);
    assert_eq!(contract.guarantees[0].id, "alpha_name_stable");
    let health = index
        .contract_health("contract://alpha_api")
        .expect("contract health should resolve");
    assert_eq!(health.status, crate::ContractHealthStatus::Stale);
    assert_eq!(health.signals.validation_count, 0);
    assert_eq!(
        index.contracts("alpha contract", 1)[0].handle,
        "contract://alpha_api"
    );
}

#[test]
fn concept_resolution_handles_typo_tolerant_alias_queries() {
    let validation = NodeId::new(
        "demo",
        "demo::impact::Prism::validation_recipe",
        NodeKind::Method,
    );
    let history = history_snapshot(
        vec![(validation.clone(), LineageId::new("lineage:validation"))],
        Vec::new(),
        1,
        0,
    );
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
    let history = history_snapshot(
        vec![
            (validation.clone(), LineageId::new("lineage:validation")),
            (
                validation_test.clone(),
                LineageId::new("lineage:validation_test"),
            ),
        ],
        Vec::new(),
        1,
        0,
    );
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
fn lineage_and_validation_updates_only_refresh_affected_concepts() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let alpha_renamed = NodeId::new("demo", "demo::alpha_renamed", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:alpha");
    let beta_lineage = LineageId::new("lineage:beta");
    let history = history_snapshot(
        vec![
            (alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        Vec::new(),
        2,
        0,
    );
    let mut index =
        ProjectionIndex::derive(&history, &OutcomeMemorySnapshot { events: Vec::new() });
    index.replace_curated_concepts_from_events(&[
        ConceptEvent {
            id: "concept-event:alpha".to_string(),
            recorded_at: 10,
            task_id: Some("task:alpha".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://alpha".to_string(),
                canonical_name: "alpha".to_string(),
                summary: "Alpha concept.".to_string(),
                aliases: vec!["alpha".to_string()],
                confidence: 0.9,
                core_members: vec![alpha.clone()],
                core_member_lineages: vec![Some(alpha_lineage.clone())],
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
                    task_id: Some("task:alpha".to_string()),
                },
                publication: None,
            },
        },
        ConceptEvent {
            id: "concept-event:beta".to_string(),
            recorded_at: 10,
            task_id: Some("task:beta".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://beta".to_string(),
                canonical_name: "beta".to_string(),
                summary: "Beta concept.".to_string(),
                aliases: vec!["beta".to_string()],
                confidence: 0.9,
                core_members: vec![beta.clone()],
                core_member_lineages: vec![Some(beta_lineage.clone())],
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
                    task_id: Some("task:beta".to_string()),
                },
                publication: None,
            },
        },
    ]);

    index.apply_lineage_events(&[LineageEvent {
        meta: EventMeta {
            id: EventId::new("lineage:event:rename"),
            ts: 11,
            actor: EventActor::System,
            correlation: None,
            causation: Some(EventId::new("change-set:rename")),
        },
        lineage: alpha_lineage.clone(),
        kind: LineageEventKind::Updated,
        before: vec![alpha.clone()],
        after: vec![alpha_renamed.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    }]);

    let alpha_packet = index
        .concept_by_handle("concept://alpha")
        .expect("alpha concept should still resolve");
    let beta_packet = index
        .concept_by_handle("concept://beta")
        .expect("beta concept should still resolve");
    assert_eq!(alpha_packet.core_members, vec![alpha_renamed.clone()]);
    assert_eq!(beta_packet.core_members, vec![beta.clone()]);

    let outcome = OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:event:beta"),
            ts: 12,
            actor: EventActor::Agent,
            correlation: Some(TaskId::new("task:beta-validate")),
            causation: None,
        },
        kind: OutcomeKind::TestRan,
        result: OutcomeResult::Success,
        summary: "Validated beta.".to_string(),
        anchors: vec![AnchorRef::Node(beta.clone())],
        evidence: vec![OutcomeEvidence::Test {
            name: "beta validation".to_string(),
            passed: true,
        }],
        metadata: serde_json::Value::Null,
    };
    index.apply_outcome_event(&outcome, |node| match node {
        current if current == &alpha_renamed => Some(alpha_lineage.clone()),
        current if current == &beta => Some(beta_lineage.clone()),
        _ => None,
    });

    let alpha_after_validation = index
        .concept_by_handle("concept://alpha")
        .expect("alpha concept should still resolve");
    let beta_after_validation = index
        .concept_by_handle("concept://beta")
        .expect("beta concept should still resolve");
    assert_eq!(alpha_after_validation.core_members, vec![alpha_renamed]);
    assert_eq!(beta_after_validation.core_members, vec![beta]);
}

#[test]
fn concept_updates_replay_from_typed_patch_payload() {
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let history = history_snapshot(
        vec![
            (alpha.clone(), LineageId::new("lineage:alpha")),
            (beta.clone(), LineageId::new("lineage:beta")),
        ],
        Vec::new(),
        1,
        0,
    );
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
    let history = history_snapshot(
        vec![
            (renamed_alpha.clone(), alpha_lineage.clone()),
            (beta.clone(), beta_lineage.clone()),
        ],
        vec![rename_event],
        2,
        1,
    );
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
