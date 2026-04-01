use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use prism_agent::{EdgeId, InferenceSnapshot, InferredEdgeRecord, InferredEdgeScope};
use prism_coordination::{CoordinationEvent, CoordinationSnapshot};
use prism_history::{HistoryPersistDelta, HistorySnapshot, LineageTombstone};
use prism_ir::{
    CoordinationEventKind, CredentialCapability, CredentialId, CredentialRecord, CredentialStatus,
    Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta, FileId, GraphChange, Language,
    LineageEvent, LineageId, Node, NodeId, NodeKind, PrincipalAuthorityId, PrincipalId,
    PrincipalKind, PrincipalProfile, PrincipalRegistrySnapshot, PrincipalStatus, Span, TaskId,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind, MemoryId, MemoryKind,
    MemorySource, OutcomeMemorySnapshot, OutcomeRecallQuery,
};
use prism_parser::ParseDepth;
use prism_projections::{
    CoChangeDelta, CoChangeRecord, ConceptPacket, ConceptProvenance, ConceptRelation,
    ConceptRelationKind, ConceptScope, ProjectionSnapshot, ValidationCheck, ValidationDelta,
    MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE,
};
use rusqlite::Connection;

use crate::{
    migrate_worktree_cache_from_shared_runtime, AuxiliaryPersistBatch, CoordinationPersistBatch,
    CoordinationPersistContext, Graph, IndexPersistBatch, MemoryStore, SqliteStore, Store,
    WorkspaceTreeDirectoryFingerprint, WorkspaceTreeFileFingerprint, WorkspaceTreeSnapshot,
};

fn node(name: &str) -> Node {
    Node {
        id: NodeId::new("demo", format!("demo::{name}"), NodeKind::Function),
        name: name.into(),
        kind: NodeKind::Function,
        file: FileId(0),
        span: Span::line(1),
        language: Language::Rust,
    }
}

fn coordination_context() -> CoordinationPersistContext {
    CoordinationPersistContext {
        repo_id: "repo:test".to_string(),
        worktree_id: "worktree:test".to_string(),
        branch_ref: Some("refs/heads/test".to_string()),
        session_id: Some("session:test".to_string()),
        instance_id: Some("instance:test".to_string()),
    }
}

#[test]
fn upsert_file_with_reanchors_emits_reanchored_change() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();

    graph.upsert_file(
        path,
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let new = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
    let update = graph.upsert_file_with_reanchors(
        path,
        2,
        vec![node("renamed_alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[(old.clone(), new.clone())],
    );

    assert_eq!(update.changes, vec![GraphChange::Reanchored { old, new }]);
    assert_eq!(update.observed.removed.len(), 1);
    assert_eq!(update.observed.added.len(), 1);
    assert!(update.observed.updated.is_empty());
}

#[test]
fn clear_derived_edges_for_nodes_skips_full_rebuild_when_scope_has_no_derived_edges() {
    let mut graph = Graph::new();
    let alpha_path = Path::new("src/alpha.rs");
    let beta_path = Path::new("src/beta.rs");
    let caller_path = Path::new("src/caller.rs");

    let alpha_file = graph.ensure_file(alpha_path);
    let beta_file = graph.ensure_file(beta_path);
    let caller_file = graph.ensure_file(caller_path);

    let alpha = Node {
        file: alpha_file,
        ..node("alpha")
    };
    let beta = Node {
        file: beta_file,
        ..node("beta")
    };
    let caller = Node {
        file: caller_file,
        ..node("caller")
    };

    graph.upsert_file(
        alpha_path,
        1,
        vec![alpha.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        beta_path,
        1,
        vec![beta.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        caller_path,
        1,
        vec![caller.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.add_edge(Edge {
        kind: EdgeKind::Calls,
        source: caller.id.clone(),
        target: beta.id.clone(),
        origin: EdgeOrigin::Inferred,
        confidence: 1.0,
    });

    let removed = graph.clear_derived_edges_for_nodes(&HashSet::from([alpha.id.clone()]));
    assert_eq!(removed, 0);
    assert!(graph
        .edges_from(&caller.id, Some(EdgeKind::Calls))
        .iter()
        .any(|edge| edge.target == beta.id));
}

#[test]
fn extend_edges_updates_adjacency_and_derived_incidence_incrementally() {
    let mut graph = Graph::new();
    let alpha_path = Path::new("src/alpha.rs");
    let caller_path = Path::new("src/caller.rs");

    let alpha_file = graph.ensure_file(alpha_path);
    let caller_file = graph.ensure_file(caller_path);

    let alpha = Node {
        file: alpha_file,
        ..node("alpha")
    };
    let caller = Node {
        file: caller_file,
        ..node("caller")
    };

    graph.upsert_file(
        alpha_path,
        1,
        vec![alpha.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        caller_path,
        1,
        vec![caller.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    graph.extend_edges(std::iter::once(Edge {
        kind: EdgeKind::Calls,
        source: caller.id.clone(),
        target: alpha.id.clone(),
        origin: EdgeOrigin::Inferred,
        confidence: 1.0,
    }));

    assert_eq!(graph.edges_from(&caller.id, Some(EdgeKind::Calls)).len(), 1);
    assert_eq!(
        graph.clear_derived_edges_for_nodes(&HashSet::from([alpha.id.clone()])),
        1
    );
    assert!(graph
        .edges_from(&caller.id, Some(EdgeKind::Calls))
        .is_empty());
    assert_eq!(graph.nodes_by_name("alpha").len(), 1);
}

#[test]
fn extend_edges_updates_file_reverse_dependency_indexes_incrementally() {
    let mut graph = Graph::new();
    let callee_path = Path::new("src/callee.rs");
    let caller_path = Path::new("src/caller.rs");

    let callee_file = graph.ensure_file(callee_path);
    let caller_file = graph.ensure_file(caller_path);
    let callee = Node {
        file: callee_file,
        ..node("callee")
    };
    let caller = Node {
        file: caller_file,
        ..node("caller")
    };

    graph.upsert_file(
        callee_path,
        1,
        vec![callee.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        caller_path,
        1,
        vec![caller.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    graph.extend_edges(std::iter::once(Edge {
        kind: EdgeKind::Calls,
        source: caller.id.clone(),
        target: callee.id.clone(),
        origin: EdgeOrigin::Inferred,
        confidence: 1.0,
    }));

    assert!(graph
        .neighboring_files_for_path(caller_path)
        .contains(&callee_path.to_path_buf()));
    assert!(graph
        .reverse_dependent_files_for_path(callee_path)
        .contains(&caller_path.to_path_buf()));
}

#[test]
fn file_update_emits_dependency_invalidation_keys_for_symbol_renames() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();
    let alpha = node("alpha");

    graph.upsert_file(
        path,
        1,
        vec![alpha.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let update = graph.upsert_file_from_with_observed_without_rebuild(
        None,
        path,
        2,
        ParseDepth::Deep,
        vec![Node {
            id: NodeId::new("demo", "demo::beta", NodeKind::Function),
            name: "beta".into(),
            ..alpha.clone()
        }],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        EventMeta {
            id: EventId::new("observed:rename".to_string()),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        prism_ir::ChangeTrigger::ManualReindex,
    );

    assert!(update.requires_edge_resolution);
    assert!(update
        .dependency_invalidation_keys
        .symbol_names
        .contains("alpha"));
    assert!(update
        .dependency_invalidation_keys
        .symbol_names
        .contains("beta"));
    assert!(update
        .dependency_invalidation_keys
        .symbol_paths
        .contains("demo::alpha"));
    assert!(update
        .dependency_invalidation_keys
        .symbol_paths
        .contains("demo::beta"));
}

#[test]
fn structurally_unchanged_file_update_does_not_require_index_rebuild() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();
    let alpha = node("alpha");

    graph.upsert_file(
        path,
        1,
        vec![alpha.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let update = graph.upsert_file_from_with_observed_without_rebuild(
        None,
        path,
        2,
        ParseDepth::Deep,
        vec![Node {
            span: Span::line(3),
            ..alpha.clone()
        }],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        EventMeta {
            id: EventId::new("observed:test".to_string()),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        prism_ir::ChangeTrigger::ManualReindex,
    );

    assert!(!update.requires_index_rebuild);
    assert!(!update.requires_edge_resolution);
}

#[test]
fn structurally_unchanged_file_update_ignores_edge_order_for_in_place_fast_path() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();
    let alpha = node("alpha");
    let beta = node("beta");
    let edges = vec![
        Edge {
            kind: EdgeKind::Calls,
            source: alpha.id.clone(),
            target: beta.id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        },
        Edge {
            kind: EdgeKind::References,
            source: beta.id.clone(),
            target: alpha.id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        },
    ];

    graph.upsert_file(
        path,
        1,
        vec![alpha.clone(), beta.clone()],
        edges.clone(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let mut reversed_edges = edges;
    reversed_edges.reverse();
    let update = graph.upsert_file_from_with_observed_without_rebuild(
        None,
        path,
        2,
        ParseDepth::Deep,
        vec![
            Node {
                span: Span::line(3),
                ..alpha
            },
            Node {
                span: Span::line(7),
                ..beta
            },
        ],
        reversed_edges,
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        EventMeta {
            id: EventId::new("observed:test".to_string()),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        prism_ir::ChangeTrigger::ManualReindex,
    );

    assert!(update.persist_in_place);
    assert!(!update.requires_index_rebuild);
    assert!(!update.requires_edge_resolution);
}

#[test]
fn structural_file_update_maintains_indexes_incrementally() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();

    graph.upsert_file(
        path,
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let update = graph.upsert_file_from_with_observed_without_rebuild(
        None,
        path,
        2,
        ParseDepth::Deep,
        vec![node("beta")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        prism_ir::EventMeta {
            id: EventId::new("event:test"),
            ts: 0,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        prism_ir::ChangeTrigger::ManualReindex,
    );

    assert!(!update.requires_index_rebuild);
    assert!(graph.nodes_by_name("alpha").is_empty());
    assert_eq!(graph.nodes_by_name("beta").len(), 1);
}

#[test]
fn remove_file_with_changes_emits_removed_nodes() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();

    graph.upsert_file(
        path,
        1,
        vec![node("alpha"), node("beta")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let removed = graph.remove_file_with_changes(path);

    assert_eq!(removed.len(), 2);
    assert!(removed.contains(&GraphChange::Removed(NodeId::new(
        "demo",
        "demo::alpha",
        NodeKind::Function,
    ))));
    assert!(removed.contains(&GraphChange::Removed(NodeId::new(
        "demo",
        "demo::beta",
        NodeKind::Function,
    ))));
}

#[test]
fn remove_file_update_emits_observed_removed_nodes() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();

    graph.upsert_file(
        path,
        1,
        vec![node("alpha"), node("beta")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let update = graph.remove_file_with_update(path);

    assert_eq!(update.observed.removed.len(), 2);
    assert!(update.observed.added.is_empty());
    assert!(update.observed.updated.is_empty());
}

#[test]
fn file_state_retains_file_edges_without_global_edge_scan() {
    let path = Path::new("src/lib.rs");
    let mut graph = Graph::new();
    let alpha = node("alpha");
    let beta = node("beta");
    let edge = Edge {
        kind: EdgeKind::Contains,
        source: alpha.id.clone(),
        target: beta.id.clone(),
        origin: EdgeOrigin::Static,
        confidence: 1.0,
    };

    graph.upsert_file(
        path,
        1,
        vec![alpha, beta],
        vec![edge.clone()],
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let state = graph.file_state(path).expect("file state exists");
    assert_eq!(state.edges, vec![edge]);
    assert_eq!(state.record.edges.len(), 1);
}

#[test]
fn deferred_file_updates_rebuild_indexes_once_at_batch_end() {
    let alpha_path = Path::new("src/alpha.rs");
    let beta_path = Path::new("src/beta.rs");
    let mut graph = Graph::new();
    let meta = EventMeta {
        id: EventId::new("observed:test".to_string()),
        ts: 1,
        actor: EventActor::System,
        correlation: None,
        causation: None,
        execution_context: None,
    };

    graph.upsert_file_from_with_observed_without_rebuild(
        None,
        alpha_path,
        1,
        ParseDepth::Deep,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        meta.clone(),
        prism_ir::ChangeTrigger::ManualReindex,
    );
    graph.upsert_file_from_with_observed_without_rebuild(
        None,
        beta_path,
        2,
        ParseDepth::Deep,
        vec![node("beta")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
        meta.clone(),
        prism_ir::ChangeTrigger::ManualReindex,
    );
    graph.rebuild_indexes();

    assert_eq!(graph.nodes_by_name("alpha").len(), 1);
    assert_eq!(graph.nodes_by_name("beta").len(), 1);

    let update = graph.remove_file_with_observed_without_rebuild(
        alpha_path,
        meta,
        prism_ir::ChangeTrigger::ManualReindex,
    );
    graph.rebuild_indexes();

    assert_eq!(update.observed.removed.len(), 1);
    assert!(graph.nodes_by_name("alpha").is_empty());
    assert_eq!(graph.nodes_by_name("beta").len(), 1);
}

#[test]
fn memory_store_round_trips_auxiliary_snapshots() {
    let mut store = MemoryStore::default();
    let history = HistorySnapshot {
        node_to_lineage: Vec::new(),
        events: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 0,
        next_event: 0,
    };
    let episodic = EpisodicMemorySnapshot {
        entries: vec![MemoryEntry {
            id: MemoryId("episodic:7".to_string()),
            anchors: Vec::new(),
            kind: MemoryKind::Episodic,
            scope: prism_memory::MemoryScope::Local,
            content: "remember alpha".to_string(),
            metadata: serde_json::Value::Null,
            created_at: 7,
            source: MemorySource::Agent,
            trust: 0.7,
        }],
    };
    let inference = InferenceSnapshot {
        records: vec![InferredEdgeRecord {
            id: EdgeId("edge:5".to_string()),
            edge: Edge {
                kind: EdgeKind::Calls,
                source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                origin: EdgeOrigin::Inferred,
                confidence: 0.8,
            },
            scope: InferredEdgeScope::Persisted,
            task: None,
            evidence: vec!["stored for reuse".to_string()],
        }],
    };
    let projections = ProjectionSnapshot {
        co_change_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:1"),
            vec![CoChangeRecord {
                lineage: prism_ir::LineageId::new("lineage:2"),
                count: 3,
            }],
        )],
        validation_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:1"),
            vec![ValidationCheck {
                label: "test:alpha_integration".to_string(),
                score: 5.0,
                last_seen: 42,
            }],
        )],
        curated_concepts: Vec::new(),
        concept_relations: Vec::new(),
    };
    let workspace_tree = WorkspaceTreeSnapshot {
        root_hash: 17,
        files: vec![(
            PathBuf::from("src/lib.rs"),
            WorkspaceTreeFileFingerprint {
                len: 128,
                modified_ns: Some(11),
                changed_ns: Some(13),
                content_hash: 23,
            },
        )]
        .into_iter()
        .collect(),
        directories: vec![(
            PathBuf::from("src"),
            WorkspaceTreeDirectoryFingerprint {
                aggregate_hash: 29,
                file_count: 1,
                modified_ns: Some(31),
                changed_ns: Some(37),
            },
        )]
        .into_iter()
        .collect(),
    };

    store.save_history_snapshot(&history).unwrap();
    store.save_episodic_snapshot(&episodic).unwrap();
    store.save_inference_snapshot(&inference).unwrap();
    store.save_projection_snapshot(&projections).unwrap();
    store.save_workspace_tree_snapshot(&workspace_tree).unwrap();

    let loaded_history = store.load_history_snapshot().unwrap().unwrap();
    assert!(loaded_history.node_to_lineage.is_empty());
    assert!(loaded_history.events.is_empty());
    assert_eq!(loaded_history.next_lineage, history.next_lineage);
    assert_eq!(loaded_history.next_event, history.next_event);
    let shallow_history = store
        .load_history_snapshot_with_options(false)
        .unwrap()
        .unwrap();
    assert_eq!(shallow_history.events, history.events);
    assert_eq!(shallow_history.next_lineage, history.next_lineage);
    assert_eq!(shallow_history.next_event, history.next_event);
    assert_eq!(store.load_episodic_snapshot().unwrap(), Some(episodic));
    assert_eq!(store.load_inference_snapshot().unwrap(), Some(inference));
    assert_eq!(store.load_projection_snapshot().unwrap(), Some(projections));
    assert_eq!(
        store.load_workspace_tree_snapshot().unwrap(),
        Some(workspace_tree)
    );
}

#[test]
fn sqlite_store_round_trips_principal_registry_snapshot() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-principal-registry-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut store = SqliteStore::open(&path).unwrap();
    let snapshot = PrincipalRegistrySnapshot {
        principals: vec![PrincipalProfile {
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:test".into()),
            kind: PrincipalKind::Agent,
            name: "Test Agent".to_string(),
            role: Some("worker".to_string()),
            status: PrincipalStatus::Active,
            created_at: 11,
            updated_at: 12,
            parent_principal_id: Some(PrincipalId("principal:parent".into())),
            profile: serde_json::json!({
                "team": "runtime",
            }),
        }],
        credentials: vec![CredentialRecord {
            credential_id: CredentialId("credential:test".into()),
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:test".into()),
            token_verifier: "verifier:test".to_string(),
            capabilities: vec![
                CredentialCapability::MutateCoordination,
                CredentialCapability::MintChildPrincipal,
            ],
            status: CredentialStatus::Active,
            created_at: 13,
            last_used_at: Some(14),
            revoked_at: None,
        }],
    };

    store.save_principal_registry_snapshot(&snapshot).unwrap();

    assert_eq!(
        store.load_principal_registry_snapshot().unwrap(),
        Some(snapshot)
    );

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn coordination_persist_batch_is_revisioned_and_idempotent() {
    let event = CoordinationEvent {
        meta: EventMeta {
            id: EventId::new("coordination:event:1"),
            ts: 1,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        kind: CoordinationEventKind::PlanCreated,
        summary: "create plan".to_string(),
        plan: None,
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: serde_json::Value::Null,
    };
    let _snapshot = CoordinationSnapshot {
        events: vec![event.clone()],
        ..CoordinationSnapshot::default()
    };

    let mut memory = MemoryStore::default();
    let first = memory
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(first.inserted_events, 1);
    assert!(first.applied);

    let retry = memory
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    assert_eq!(retry.revision, 1);
    assert_eq!(retry.inserted_events, 0);
    assert!(!retry.applied);
    assert_eq!(
        memory.load_latest_coordination_persist_context().unwrap(),
        Some(coordination_context())
    );

    let err = memory
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: Vec::new(),
        })
        .unwrap_err();
    assert!(err.to_string().contains("coordination revision mismatch"));
}

#[test]
fn sqlite_store_load_lineage_history_reads_persisted_events_by_lineage() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-lineage-history-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();

    let node = NodeId::new("demo", "demo::alpha", prism_ir::NodeKind::Function);
    let lineage = LineageId::new("lineage:alpha");
    let event = LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:lineage:alpha"),
            ts: 7,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Updated,
        before: vec![node.clone()],
        after: vec![node.clone()],
        confidence: 0.9,
        evidence: vec![prism_ir::LineageEvidence::ExactNodeId],
    };
    store
        .save_history_snapshot(&HistorySnapshot {
            node_to_lineage: vec![(node, lineage.clone())],
            events: vec![event.clone()],
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 1,
        })
        .unwrap();

    let loaded = store.load_lineage_history(&lineage).unwrap();
    assert_eq!(loaded, vec![event]);
}

#[test]
fn sqlite_store_load_task_replay_reads_persisted_events_by_task() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-task-replay-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();

    let task = TaskId::new("task:lazy-replay");
    let event = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:task:lazy-replay"),
            ts: 9,
            actor: EventActor::Agent,
            correlation: Some(task.clone()),
            causation: None,
            execution_context: None,
        },
        anchors: Vec::new(),
        kind: prism_memory::OutcomeKind::PlanCreated,
        result: prism_memory::OutcomeResult::Success,
        summary: "Investigate replay".into(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    store
        .save_outcome_snapshot(&OutcomeMemorySnapshot {
            events: vec![event.clone()],
        })
        .unwrap();

    let loaded = store.load_task_replay(&task).unwrap();
    assert_eq!(loaded.task, task);
    assert_eq!(loaded.events, vec![event]);
}

#[test]
fn sqlite_store_append_outcome_events_persists_authoritative_events_and_validation_deltas() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-outcome-journal-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();

    let lineage = LineageId::new("lineage:alpha");
    let event = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:journal"),
            ts: 10,
            actor: EventActor::Agent,
            correlation: Some(TaskId::new("task:journal")),
            causation: None,
            execution_context: None,
        },
        anchors: vec![prism_ir::AnchorRef::Lineage(lineage.clone())],
        kind: prism_memory::OutcomeKind::FailureObserved,
        result: prism_memory::OutcomeResult::Failure,
        summary: "journaled failure".into(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };

    let inserted = store
        .append_outcome_events(
            &[event.clone()],
            &[ValidationDelta {
                lineage: lineage.clone(),
                label: "test:journal".to_string(),
                score_delta: 1.0,
                last_seen: 10,
            }],
        )
        .unwrap();
    assert_eq!(inserted, 1);

    let loaded = store
        .load_task_replay(&TaskId::new("task:journal"))
        .unwrap();
    assert_eq!(loaded.events, vec![event]);

    let projection = store.load_projection_snapshot().unwrap().unwrap();
    assert_eq!(
        projection.validation_by_lineage,
        vec![(
            lineage,
            vec![ValidationCheck {
                label: "test:journal".to_string(),
                score: 1.0,
                last_seen: 10,
            }],
        )]
    );
}

#[test]
fn sqlite_store_load_outcomes_reads_anchored_events_from_sqlite_index() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-outcome-query-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();

    let alpha = NodeId::new("demo", "demo::alpha", prism_ir::NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", prism_ir::NodeKind::Function);
    let alpha_lineage = LineageId::new("lineage:alpha");
    let beta_task = TaskId::new("task:beta");
    let events = vec![
        prism_memory::OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:alpha"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            anchors: vec![
                prism_ir::AnchorRef::Node(alpha),
                prism_ir::AnchorRef::Lineage(alpha_lineage.clone()),
            ],
            kind: prism_memory::OutcomeKind::FailureObserved,
            result: prism_memory::OutcomeResult::Failure,
            summary: "alpha failed".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        },
        prism_memory::OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:beta"),
                ts: 6,
                actor: EventActor::Agent,
                correlation: Some(beta_task.clone()),
                causation: None,
                execution_context: None,
            },
            anchors: vec![prism_ir::AnchorRef::Node(beta)],
            kind: prism_memory::OutcomeKind::TestRan,
            result: prism_memory::OutcomeResult::Success,
            summary: "beta passed".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        },
    ];
    store
        .save_outcome_snapshot(&OutcomeMemorySnapshot {
            events: events.clone(),
        })
        .unwrap();

    let loaded = store
        .load_outcomes(&OutcomeRecallQuery {
            anchors: vec![prism_ir::AnchorRef::Lineage(alpha_lineage)],
            kinds: Some(vec![prism_memory::OutcomeKind::FailureObserved]),
            result: Some(prism_memory::OutcomeResult::Failure),
            limit: 10,
            ..OutcomeRecallQuery::default()
        })
        .unwrap();
    assert_eq!(loaded, vec![events[0].clone()]);

    let task_loaded = store
        .load_outcomes(&OutcomeRecallQuery {
            task: Some(beta_task),
            kinds: Some(vec![prism_memory::OutcomeKind::TestRan]),
            limit: 10,
            ..OutcomeRecallQuery::default()
        })
        .unwrap();
    assert_eq!(task_loaded, vec![events[1].clone()]);
}

#[test]
fn memory_store_apply_history_delta_updates_authoritative_history_state() {
    let mut store = MemoryStore::default();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let lineage = LineageId::new("lineage:alpha");
    let born = LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:born"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Born,
        before: Vec::new(),
        after: vec![alpha.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    };
    store
        .save_history_snapshot(&HistorySnapshot {
            node_to_lineage: vec![(alpha.clone(), lineage.clone())],
            events: vec![born.clone()],
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        })
        .unwrap();

    let updated = LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:updated"),
            ts: 2,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Updated,
        before: vec![alpha],
        after: vec![beta.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    };
    store
        .apply_history_delta(&HistoryPersistDelta {
            removed_nodes: vec![NodeId::new("demo", "demo::alpha", NodeKind::Function)],
            upserted_node_lineages: vec![(beta.clone(), lineage.clone())],
            appended_events: vec![updated.clone()],
            upserted_tombstones: Vec::<LineageTombstone>::new(),
            removed_tombstone_lineages: Vec::new(),
            next_lineage: 1,
            next_event: 3,
        })
        .unwrap();

    let snapshot = store.load_history_snapshot().unwrap().unwrap();
    assert_eq!(snapshot.node_to_lineage, vec![(beta, lineage)]);
    assert_eq!(snapshot.events, vec![born, updated]);
    assert_eq!(snapshot.next_event, 3);
}

#[test]
fn memory_store_coordination_compaction_replays_from_fallback_snapshot_and_suffix() {
    let event = CoordinationEvent {
        meta: EventMeta {
            id: EventId::new("coordination:event:1"),
            ts: 1,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        kind: CoordinationEventKind::PlanCreated,
        summary: "create plan".to_string(),
        plan: None,
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: serde_json::Value::Null,
    };
    let mut store = MemoryStore::default();
    store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    let snapshot = CoordinationSnapshot {
        events: vec![event.clone()],
        ..CoordinationSnapshot::default()
    };
    store.save_coordination_compaction(&snapshot).unwrap();

    let stream = store.load_coordination_event_stream().unwrap();
    assert_eq!(
        stream.fallback_snapshot.unwrap().events,
        Vec::<CoordinationEvent>::new()
    );
    assert!(stream.suffix_events.is_empty());
}

#[test]
fn memory_store_merges_episodic_snapshots_append_only() {
    let mut store = MemoryStore::default();
    let alpha = MemoryEntry {
        id: MemoryId("episodic:1".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember alpha".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 1,
        source: MemorySource::Agent,
        trust: 0.7,
    };
    let beta = MemoryEntry {
        id: MemoryId("episodic:2".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember beta".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 2,
        source: MemorySource::Agent,
        trust: 0.8,
    };

    store
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: vec![alpha.clone()],
        })
        .unwrap();
    store
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: vec![beta.clone()],
        })
        .unwrap();

    assert_eq!(
        store.load_episodic_snapshot().unwrap(),
        Some(EpisodicMemorySnapshot {
            entries: vec![alpha, beta],
        })
    );
}

#[test]
fn sqlite_store_persists_projections_in_dedicated_tables() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let projections = ProjectionSnapshot {
        co_change_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:10"),
            vec![CoChangeRecord {
                lineage: prism_ir::LineageId::new("lineage:20"),
                count: 2,
            }],
        )],
        validation_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:10"),
            vec![ValidationCheck {
                label: "test:smoke".to_string(),
                score: 3.5,
                last_seen: 99,
            }],
        )],
        curated_concepts: Vec::new(),
        concept_relations: vec![ConceptRelation {
            source_handle: "concept://validation_pipeline".to_string(),
            target_handle: "concept://runtime_surface".to_string(),
            kind: ConceptRelationKind::OftenUsedWith,
            confidence: 0.82,
            evidence: vec!["Validation work often routes through runtime state.".to_string()],
            scope: ConceptScope::Session,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "store_test".to_string(),
                task_id: None,
            },
        }],
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.save_projection_snapshot(&projections).unwrap();
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(projections.clone())
    );

    let snapshot_rows: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM snapshots WHERE key = 'projections'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(snapshot_rows, 0);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn migrate_worktree_cache_moves_local_state_out_of_shared_runtime_db() {
    let shared_path = temp_sqlite_path("prism-store-shared-runtime-migration");
    let local_path = temp_sqlite_path("prism-store-worktree-cache-migration");
    let mut shared = SqliteStore::open(&shared_path).unwrap();

    let source_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();
    let alpha = node("alpha");
    let lineage = LineageId::new("lineage:alpha");
    graph.upsert_file_from(
        None,
        &source_path,
        1,
        ParseDepth::Deep,
        vec![alpha.clone()],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    );
    shared
        .commit_index_persist_batch(
            &graph,
            &IndexPersistBatch {
                upserted_paths: vec![source_path.clone()],
                in_place_upserted_paths: Vec::new(),
                removed_paths: Vec::new(),
                history_snapshot: HistorySnapshot {
                    node_to_lineage: vec![(alpha.id.clone(), lineage.clone())],
                    events: vec![LineageEvent {
                        meta: EventMeta {
                            id: EventId::new("event:lineage:migration"),
                            ts: 7,
                            actor: EventActor::Agent,
                            correlation: None,
                            causation: None,
                            execution_context: None,
                        },
                        lineage: lineage.clone(),
                        kind: prism_ir::LineageEventKind::Updated,
                        before: vec![alpha.id.clone()],
                        after: vec![alpha.id.clone()],
                        confidence: 1.0,
                        evidence: vec![prism_ir::LineageEvidence::ExactNodeId],
                    }],
                    tombstones: Vec::new(),
                    next_lineage: 2,
                    next_event: 8,
                },
                history_delta: None,
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                outcome_events: Vec::new(),
                defer_graph_materialization: false,
                co_change_deltas: vec![CoChangeDelta {
                    source_lineage: lineage.clone(),
                    target_lineage: LineageId::new("lineage:beta"),
                    count_delta: 1,
                }],
                validation_deltas: vec![ValidationDelta {
                    lineage: lineage.clone(),
                    label: "test:migration".to_string(),
                    score_delta: 1.0,
                    last_seen: 7,
                }],
                projection_snapshot: Some(ProjectionSnapshot {
                    co_change_by_lineage: Vec::new(),
                    validation_by_lineage: Vec::new(),
                    curated_concepts: vec![
                        ConceptPacket {
                            handle: "concept://local-alpha".to_string(),
                            canonical_name: "local-alpha".to_string(),
                            summary: "local concept".to_string(),
                            aliases: Vec::new(),
                            confidence: 0.9,
                            core_members: Vec::new(),
                            core_member_lineages: Vec::new(),
                            supporting_members: Vec::new(),
                            supporting_member_lineages: Vec::new(),
                            likely_tests: Vec::new(),
                            likely_test_lineages: Vec::new(),
                            evidence: Vec::new(),
                            risk_hint: None,
                            decode_lenses: Vec::new(),
                            scope: ConceptScope::Local,
                            provenance: ConceptProvenance {
                                origin: "test".to_string(),
                                kind: "migration".to_string(),
                                task_id: None,
                            },
                            publication: None,
                        },
                        ConceptPacket {
                            handle: "concept://session-alpha".to_string(),
                            canonical_name: "session-alpha".to_string(),
                            summary: "session concept".to_string(),
                            aliases: Vec::new(),
                            confidence: 0.8,
                            core_members: Vec::new(),
                            core_member_lineages: Vec::new(),
                            supporting_members: Vec::new(),
                            supporting_member_lineages: Vec::new(),
                            likely_tests: Vec::new(),
                            likely_test_lineages: Vec::new(),
                            evidence: Vec::new(),
                            risk_hint: None,
                            decode_lenses: Vec::new(),
                            scope: ConceptScope::Session,
                            provenance: ConceptProvenance {
                                origin: "test".to_string(),
                                kind: "migration".to_string(),
                                task_id: None,
                            },
                            publication: None,
                        },
                    ],
                    concept_relations: vec![
                        ConceptRelation {
                            source_handle: "concept://local-alpha".to_string(),
                            target_handle: "concept://local-beta".to_string(),
                            kind: ConceptRelationKind::OftenUsedWith,
                            scope: ConceptScope::Local,
                            evidence: Vec::new(),
                            confidence: 0.8,
                            provenance: ConceptProvenance {
                                origin: "test".to_string(),
                                kind: "migration".to_string(),
                                task_id: None,
                            },
                        },
                        ConceptRelation {
                            source_handle: "concept://session-alpha".to_string(),
                            target_handle: "concept://session-beta".to_string(),
                            kind: ConceptRelationKind::OftenUsedWith,
                            scope: ConceptScope::Session,
                            evidence: Vec::new(),
                            confidence: 0.7,
                            provenance: ConceptProvenance {
                                origin: "test".to_string(),
                                kind: "migration".to_string(),
                                task_id: None,
                            },
                        },
                    ],
                }),
                workspace_tree_snapshot: Some(WorkspaceTreeSnapshot {
                    root_hash: 9,
                    files: HashMap::from([(
                        source_path.clone(),
                        WorkspaceTreeFileFingerprint {
                            len: 12,
                            modified_ns: Some(1),
                            changed_ns: Some(2),
                            content_hash: 3,
                        },
                    )])
                    .into_iter()
                    .collect(),
                    directories: HashMap::from([(
                        PathBuf::from("src"),
                        WorkspaceTreeDirectoryFingerprint {
                            aggregate_hash: 4,
                            file_count: 1,
                            modified_ns: Some(5),
                            changed_ns: Some(6),
                        },
                    )])
                    .into_iter()
                    .collect(),
                }),
            },
        )
        .unwrap();
    shared
        .save_curator_snapshot(&prism_curator::CuratorSnapshot {
            records: Vec::new(),
        })
        .unwrap();
    shared
        .save_inference_snapshot(&InferenceSnapshot {
            records: vec![InferredEdgeRecord {
                id: EdgeId("edge:migration".to_string()),
                edge: Edge {
                    kind: EdgeKind::Calls,
                    source: alpha.id.clone(),
                    target: alpha.id.clone(),
                    origin: EdgeOrigin::Inferred,
                    confidence: 0.8,
                },
                scope: InferredEdgeScope::Persisted,
                task: None,
                evidence: vec!["test".to_string()],
            }],
        })
        .unwrap();

    let local_entry = MemoryEntry {
        id: MemoryId("episodic:local".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "local memory".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 11,
        source: MemorySource::Agent,
        trust: 0.7,
    };
    let session_entry = MemoryEntry {
        id: MemoryId("episodic:session".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Session,
        content: "session memory".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 12,
        source: MemorySource::Agent,
        trust: 0.8,
    };
    let local_event = MemoryEvent::from_entry(
        MemoryEventKind::Stored,
        local_entry.clone(),
        None,
        Vec::new(),
        Vec::new(),
    );
    let session_event = MemoryEvent::from_entry(
        MemoryEventKind::Stored,
        session_entry.clone(),
        None,
        Vec::new(),
        Vec::new(),
    );
    shared
        .append_memory_events(&[local_event.clone(), session_event.clone()])
        .unwrap();
    shared
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: vec![local_entry.clone(), session_entry.clone()],
        })
        .unwrap();
    drop(shared);

    migrate_worktree_cache_from_shared_runtime(&local_path, &shared_path).unwrap();

    let mut local = SqliteStore::open(&local_path).unwrap();
    let mut shared = SqliteStore::open(&shared_path).unwrap();

    assert!(local.load_graph().unwrap().is_some());
    assert!(local.load_history_snapshot().unwrap().is_some());
    assert!(local.load_workspace_tree_snapshot().unwrap().is_some());
    assert!(local.load_curator_snapshot().unwrap().is_some());
    assert!(local.load_inference_snapshot().unwrap().is_some());
    assert_eq!(
        local.load_memory_events().unwrap(),
        vec![local_event]
    );
    assert_eq!(
        local.load_episodic_snapshot().unwrap().unwrap().entries,
        vec![local_entry]
    );
    let local_projection = local.load_projection_snapshot().unwrap().unwrap();
    assert_eq!(local_projection.curated_concepts.len(), 1);
    assert_eq!(local_projection.curated_concepts[0].scope, ConceptScope::Local);
    assert_eq!(local_projection.concept_relations.len(), 1);
    assert_eq!(local_projection.concept_relations[0].scope, ConceptScope::Local);

    assert!(shared.load_graph().unwrap().is_none());
    assert!(shared.load_history_snapshot().unwrap().is_none());
    assert!(shared.load_workspace_tree_snapshot().unwrap().is_none());
    assert!(shared.load_curator_snapshot().unwrap().is_none());
    assert!(shared.load_inference_snapshot().unwrap().is_none());
    assert_eq!(
        shared.load_memory_events().unwrap(),
        vec![session_event]
    );
    assert_eq!(
        shared.load_episodic_snapshot().unwrap().unwrap().entries,
        vec![session_entry]
    );
    let shared_projection = shared.load_projection_knowledge_snapshot().unwrap().unwrap();
    assert_eq!(shared_projection.curated_concepts.len(), 1);
    assert_eq!(shared_projection.curated_concepts[0].scope, ConceptScope::Session);
    assert_eq!(shared_projection.concept_relations.len(), 1);
    assert_eq!(shared_projection.concept_relations[0].scope, ConceptScope::Session);

    let _ = std::fs::remove_file(local_path);
    let _ = std::fs::remove_file(shared_path);
}

#[test]
fn migrate_worktree_cache_copies_outcome_compat_state_from_shared_runtime() {
    let shared_path = temp_sqlite_path("prism-store-shared-outcome-compat");
    let local_path = temp_sqlite_path("prism-store-local-outcome-compat");
    let mut shared = SqliteStore::open(&shared_path).unwrap();
    let event = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:migration"),
            ts: 17,
            actor: EventActor::Agent,
            correlation: Some(TaskId::new("task:migration")),
            causation: None,
            execution_context: None,
        },
        anchors: vec![prism_ir::AnchorRef::Lineage(LineageId::new("lineage:outcome"))],
        kind: prism_memory::OutcomeKind::FailureObserved,
        result: prism_memory::OutcomeResult::Failure,
        summary: "migrated outcome".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    shared.append_outcome_events(&[event.clone()], &[]).unwrap();
    drop(shared);

    migrate_worktree_cache_from_shared_runtime(&local_path, &shared_path).unwrap();

    let local = SqliteStore::open(&local_path).unwrap();
    let shared = SqliteStore::open(&shared_path).unwrap();
    assert_eq!(
        local.load_task_replay(&TaskId::new("task:migration")).unwrap().events,
        vec![event.clone()]
    );
    assert_eq!(
        shared.load_task_replay(&TaskId::new("task:migration")).unwrap().events,
        vec![event]
    );

    let _ = std::fs::remove_file(local_path);
    let _ = std::fs::remove_file(shared_path);
}

fn temp_sqlite_path(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.db"))
}


#[test]
fn sqlite_store_round_trips_workspace_tree_snapshot() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-tree-snapshot-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let workspace_tree = WorkspaceTreeSnapshot {
        root_hash: 41,
        files: vec![(
            PathBuf::from("crates/prism-core/src/lib.rs"),
            WorkspaceTreeFileFingerprint {
                len: 256,
                modified_ns: Some(101),
                changed_ns: Some(103),
                content_hash: 107,
            },
        )]
        .into_iter()
        .collect(),
        directories: vec![(
            PathBuf::from("crates/prism-core/src"),
            WorkspaceTreeDirectoryFingerprint {
                aggregate_hash: 109,
                file_count: 1,
                modified_ns: Some(113),
                changed_ns: Some(127),
            },
        )]
        .into_iter()
        .collect(),
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.save_workspace_tree_snapshot(&workspace_tree).unwrap();
    assert_eq!(
        store.load_workspace_tree_snapshot().unwrap(),
        Some(workspace_tree)
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_configures_connection_pragmas() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-pragmas-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let store = SqliteStore::open(&path).unwrap();
    let journal_mode: String = store
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    let synchronous: i64 = store
        .conn
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();
    let temp_store: i64 = store
        .conn
        .pragma_query_value(None, "temp_store", |row| row.get(0))
        .unwrap();
    let wal_autocheckpoint: i64 = store
        .conn
        .pragma_query_value(None, "wal_autocheckpoint", |row| row.get(0))
        .unwrap();
    let user_version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let indexed_tables = [
        "idx_edges_file_path_kind",
        "idx_file_nodes_file_path_node",
        "idx_node_fingerprints_file_path",
        "idx_unresolved_calls_file_path",
        "idx_unresolved_imports_file_path",
        "idx_unresolved_impls_file_path",
        "idx_unresolved_intents_file_path",
    ]
    .into_iter()
    .map(|name| {
        store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    })
    .collect::<Vec<_>>();

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(synchronous, 1);
    assert_eq!(temp_store, 2);
    assert_eq!(wal_autocheckpoint, 1000);
    assert_eq!(user_version, 20);
    assert!(indexed_tables.into_iter().all(|count| count == 1));

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_coordination_persist_batch_appends_events_and_enforces_revision() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-coordination-persist-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let event = CoordinationEvent {
        meta: EventMeta {
            id: EventId::new("coordination:event:sqlite"),
            ts: 7,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        kind: CoordinationEventKind::PlanCreated,
        summary: "create persisted plan".to_string(),
        plan: None,
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: serde_json::Value::Null,
    };
    let _snapshot = CoordinationSnapshot {
        events: vec![event.clone()],
        ..CoordinationSnapshot::default()
    };

    let mut store = SqliteStore::open(&path).unwrap();
    let first = store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(first.inserted_events, 1);
    assert!(first.applied);

    let event_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM coordination_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(event_rows, 1);
    let logged_context = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .unwrap();
    assert_eq!(logged_context, coordination_context());
    let mutation_row: (String, String, Option<String>, Option<String>, Option<String>, i64, i64) =
        store
            .conn
            .query_row(
                "SELECT repo_id, worktree_id, branch_ref, session_id, instance_id, inserted_events, applied
                 FROM coordination_mutation_log
                 ORDER BY sequence DESC
                 LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(mutation_row.0, "repo:test");
    assert_eq!(mutation_row.1, "worktree:test");
    assert_eq!(mutation_row.2.as_deref(), Some("refs/heads/test"));
    assert_eq!(mutation_row.3.as_deref(), Some("session:test"));
    assert_eq!(mutation_row.4.as_deref(), Some("instance:test"));
    assert_eq!(mutation_row.5, 1);
    assert_eq!(mutation_row.6, 1);

    let retry = store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    assert_eq!(retry.revision, 1);
    assert_eq!(retry.inserted_events, 0);
    assert!(!retry.applied);

    let err = store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: Vec::new(),
        })
        .unwrap_err();
    assert!(err.to_string().contains("coordination revision mismatch"));

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_coordination_compaction_loads_suffix_events_after_compacted_sequence() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-coordination-compaction-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let event = CoordinationEvent {
        meta: EventMeta {
            id: EventId::new("coordination:event:1"),
            ts: 1,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        kind: CoordinationEventKind::PlanCreated,
        summary: "create plan".to_string(),
        plan: None,
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: serde_json::Value::Null,
    };
    let next_event = CoordinationEvent {
        meta: EventMeta {
            id: EventId::new("coordination:event:2"),
            ts: 2,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        kind: CoordinationEventKind::PlanUpdated,
        summary: "update plan".to_string(),
        plan: None,
        task: None,
        claim: None,
        artifact: None,
        review: None,
        metadata: serde_json::json!({}),
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(0),
            appended_events: vec![event.clone()],
        })
        .unwrap();
    let snapshot = CoordinationSnapshot {
        events: vec![event.clone()],
        ..CoordinationSnapshot::default()
    };
    store.save_coordination_compaction(&snapshot).unwrap();
    store
        .commit_coordination_persist_batch(&CoordinationPersistBatch {
            context: coordination_context(),
            expected_revision: Some(1),
            appended_events: vec![next_event.clone()],
        })
        .unwrap();

    let stream = store.load_coordination_event_stream().unwrap();
    assert_eq!(
        stream.fallback_snapshot.expect("fallback snapshot").events,
        Vec::<CoordinationEvent>::new()
    );
    assert_eq!(stream.suffix_events, vec![next_event]);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_prunes_co_change_neighbors_to_top_k() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-projection-prune-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = prism_ir::LineageId::new("lineage:source");
    let projections = ProjectionSnapshot {
        co_change_by_lineage: vec![(
            source.clone(),
            (0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8))
                .map(|index| CoChangeRecord {
                    lineage: prism_ir::LineageId::new(format!("lineage:{index:03}")),
                    count: (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8 - index) as u32,
                })
                .collect(),
        )],
        validation_by_lineage: Vec::new(),
        curated_concepts: Vec::new(),
        concept_relations: Vec::new(),
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.save_projection_snapshot(&projections).unwrap();
    let loaded = store.load_projection_snapshot().unwrap().unwrap();
    let neighbors = loaded
        .co_change_by_lineage
        .into_iter()
        .find(|(lineage, _)| lineage == &source)
        .map(|(_, neighbors)| neighbors)
        .unwrap();

    assert_eq!(neighbors.len(), MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);
    assert_eq!(
        neighbors.first().unwrap().count,
        (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) as u32
    );
    assert_eq!(neighbors.last().unwrap().count, 9);

    let row_count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM projection_co_change WHERE source_lineage = ?1",
            [source.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row_count as usize, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_prunes_legacy_co_change_rows_on_open() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-legacy-prune-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    {
        let mut store = SqliteStore::open(&path).unwrap();
        let tx = store.conn.transaction().unwrap();
        for index in 0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) {
            tx.execute(
                "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "lineage:source",
                    format!("lineage:{index:03}"),
                    (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8 - index) as i64
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();
    let row_count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM projection_co_change WHERE source_lineage = 'lineage:source'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row_count as usize, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_co_change_delta_prunes_only_touched_sources() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-touched-projection-prune-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let touched = prism_ir::LineageId::new("lineage:touched");
    let untouched = prism_ir::LineageId::new("lineage:untouched");

    let mut store = SqliteStore::open(&path).unwrap();
    {
        let tx = store.conn.transaction().unwrap();
        for source in [&touched, &untouched] {
            for index in 0..(MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) {
                tx.execute(
                    "INSERT INTO projection_co_change(source_lineage, target_lineage, count)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![
                        source.0.as_str(),
                        format!("lineage:{index:03}"),
                        (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8 - index) as i64
                    ],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }

    store
        .save_history_snapshot_with_co_change_deltas(
            &HistorySnapshot {
                node_to_lineage: Vec::new(),
                events: Vec::new(),
                tombstones: Vec::new(),
                next_lineage: 1,
                next_event: 1,
            },
            &[CoChangeDelta {
                source_lineage: touched.clone(),
                target_lineage: prism_ir::LineageId::new("lineage:extra"),
                count_delta: 1,
            }],
        )
        .unwrap();

    let touched_rows: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM projection_co_change WHERE source_lineage = ?1",
            [touched.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let untouched_rows: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM projection_co_change WHERE source_lineage = ?1",
            [untouched.0.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(touched_rows as usize, MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE);
    assert_eq!(
        untouched_rows as usize,
        MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_rewrites_only_touched_derived_edges() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-derived-edge-scope-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let touched_source = NodeId::new("demo", "demo::touched_source", NodeKind::Function);
    let touched_target = NodeId::new("demo", "demo::touched_target", NodeKind::Function);
    let untouched_source = NodeId::new("demo", "demo::untouched_source", NodeKind::Function);
    let untouched_target = NodeId::new("demo", "demo::untouched_target", NodeKind::Function);

    let mut initial_graph = Graph::new();
    initial_graph.edges = vec![
        Edge {
            kind: EdgeKind::Calls,
            source: touched_source.clone(),
            target: touched_target.clone(),
            origin: EdgeOrigin::Inferred,
            confidence: 0.2,
        },
        Edge {
            kind: EdgeKind::Calls,
            source: untouched_source.clone(),
            target: untouched_target.clone(),
            origin: EdgeOrigin::Inferred,
            confidence: 0.9,
        },
    ];

    let mut store = SqliteStore::open(&path).unwrap();
    {
        let tx = store.conn.transaction().unwrap();
        crate::sqlite::test_replace_derived_edges_tx(&tx, &initial_graph).unwrap();
        tx.commit().unwrap();
    }

    let mut updated_graph = Graph::new();
    updated_graph.edges = vec![Edge {
        kind: EdgeKind::Calls,
        source: touched_source.clone(),
        target: touched_target.clone(),
        origin: EdgeOrigin::Inferred,
        confidence: 0.8,
    }];

    {
        let tx = store.conn.transaction().unwrap();
        let touched_nodes = HashSet::from([touched_source.clone(), touched_target.clone()]);
        crate::sqlite::test_replace_derived_edges_touching_nodes_tx(
            &tx,
            &updated_graph,
            &touched_nodes,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    let edges = {
        let mut stmt = store
            .conn
            .prepare(
                "SELECT source_path, target_path, confidence
                 FROM edges
                 WHERE file_path IS NULL AND kind = ?1
                 ORDER BY source_path, target_path",
            )
            .unwrap();
        let rows = stmt
            .query_map([1_i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .unwrap();
        rows.map(|row| row.unwrap()).collect::<Vec<_>>()
    };

    assert_eq!(edges.len(), 2);
    assert_eq!(edges[0].0, touched_source.path.to_string());
    assert_eq!(edges[0].1, touched_target.path.to_string());
    assert!((edges[0].2 - 0.8).abs() < 1e-6);
    assert_eq!(edges[1].0, untouched_source.path.to_string());
    assert_eq!(edges[1].1, untouched_target.path.to_string());
    assert!((edges[1].2 - 0.9).abs() < 1e-6);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_retires_legacy_history_co_change_rows_on_open() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-legacy-history-co-change-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE history_co_change (
                source_lineage TEXT NOT NULL,
                target_lineage TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (source_lineage, target_lineage)
            );
            INSERT INTO history_co_change(source_lineage, target_lineage, count)
            VALUES ('lineage:alpha', 'lineage:beta', 7);
            ",
        )
        .unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();
    let row_count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM history_co_change", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(row_count, 0);
    let retired: i64 = store
        .conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'history:legacy_co_change_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retired, 1);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_commits_auxiliary_snapshots_with_projection_deltas() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-aux-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let history = HistorySnapshot {
        node_to_lineage: Vec::new(),
        events: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 4,
        next_event: 9,
    };
    let outcomes = OutcomeMemorySnapshot { events: Vec::new() };
    let co_change_delta = CoChangeDelta {
        source_lineage: prism_ir::LineageId::new("lineage:10"),
        target_lineage: prism_ir::LineageId::new("lineage:20"),
        count_delta: 2,
    };
    let validation_delta = ValidationDelta {
        lineage: prism_ir::LineageId::new("lineage:10"),
        label: "test:smoke".to_string(),
        score_delta: 3.5,
        last_seen: 99,
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .save_history_snapshot_with_co_change_deltas(
            &history,
            std::slice::from_ref(&co_change_delta),
        )
        .unwrap();
    store
        .save_outcome_snapshot_with_validation_deltas(
            &outcomes,
            std::slice::from_ref(&validation_delta),
        )
        .unwrap();

    let loaded_history = store.load_history_snapshot().unwrap().unwrap();
    assert!(loaded_history.node_to_lineage.is_empty());
    assert!(loaded_history.events.is_empty());
    assert_eq!(loaded_history.next_lineage, history.next_lineage);
    assert_eq!(loaded_history.next_event, history.next_event);

    assert!(store.load_outcome_snapshot().unwrap().is_none());
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:10"),
                vec![CoChangeRecord {
                    lineage: prism_ir::LineageId::new("lineage:20"),
                    count: 2,
                }],
            )],
            validation_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:10"),
                vec![ValidationCheck {
                    label: "test:smoke".to_string(),
                    score: 3.5,
                    last_seen: 99,
                }],
            )],
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_apply_validation_deltas_materializes_without_bumping_workspace_revision() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-apply-validation-deltas-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();
    let base_revision = store.workspace_revision().unwrap();

    store
        .apply_validation_deltas(&[ValidationDelta {
            lineage: prism_ir::LineageId::new("lineage:alpha"),
            label: "test:smoke".to_string(),
            score_delta: 2.5,
            last_seen: 41,
        }])
        .unwrap();

    assert_eq!(store.workspace_revision().unwrap(), base_revision);
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: Vec::new(),
            validation_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:alpha"),
                vec![ValidationCheck {
                    label: "test:smoke".to_string(),
                    score: 2.5,
                    last_seen: 41,
                }],
            )],
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
    );
}

#[test]
fn sqlite_store_apply_projection_deltas_materializes_without_bumping_workspace_revision() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-apply-projection-deltas-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();
    let base_revision = store.workspace_revision().unwrap();

    store
        .apply_projection_deltas(
            &[CoChangeDelta {
                source_lineage: prism_ir::LineageId::new("lineage:alpha"),
                target_lineage: prism_ir::LineageId::new("lineage:beta"),
                count_delta: 3,
            }],
            &[ValidationDelta {
                lineage: prism_ir::LineageId::new("lineage:alpha"),
                label: "test:smoke".to_string(),
                score_delta: 2.5,
                last_seen: 41,
            }],
        )
        .unwrap();

    assert_eq!(store.workspace_revision().unwrap(), base_revision);
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:alpha"),
                vec![CoChangeRecord {
                    lineage: prism_ir::LineageId::new("lineage:beta"),
                    count: 3,
                }],
            )],
            validation_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:alpha"),
                vec![ValidationCheck {
                    label: "test:smoke".to_string(),
                    score: 2.5,
                    last_seen: 41,
                }],
            )],
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
    );
}

#[test]
fn sqlite_store_load_projection_knowledge_snapshot_omits_co_change_and_validation_rows() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("prism-store-load-projection-knowledge-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();
    let snapshot = ProjectionSnapshot {
        co_change_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:alpha"),
            vec![CoChangeRecord {
                lineage: prism_ir::LineageId::new("lineage:beta"),
                count: 7,
            }],
        )],
        validation_by_lineage: vec![(
            prism_ir::LineageId::new("lineage:alpha"),
            vec![ValidationCheck {
                label: "test:smoke".to_string(),
                score: 4.0,
                last_seen: 99,
            }],
        )],
        curated_concepts: vec![prism_projections::ConceptPacket {
            handle: "concept://alpha".to_string(),
            canonical_name: "alpha".to_string(),
            summary: "alpha concept".to_string(),
            aliases: Vec::new(),
            confidence: 0.9,
            core_members: Vec::new(),
            core_member_lineages: Vec::new(),
            supporting_members: Vec::new(),
            supporting_member_lineages: Vec::new(),
            evidence: Vec::new(),
            likely_tests: Vec::new(),
            likely_test_lineages: Vec::new(),
            risk_hint: None,
            decode_lenses: Vec::new(),
            scope: ConceptScope::Local,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "projection_knowledge_snapshot".to_string(),
                task_id: None,
            },
            publication: None,
        }],
        concept_relations: vec![ConceptRelation {
            source_handle: "concept://alpha".to_string(),
            target_handle: "concept://beta".to_string(),
            kind: ConceptRelationKind::OftenUsedWith,
            scope: ConceptScope::Local,
            evidence: Vec::new(),
            confidence: 0.8,
            provenance: ConceptProvenance {
                origin: "test".to_string(),
                kind: "projection_knowledge_snapshot".to_string(),
                task_id: None,
            },
        }],
    };
    store.save_projection_snapshot(&snapshot).unwrap();

    assert_eq!(
        store.load_projection_knowledge_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: Vec::new(),
            validation_by_lineage: Vec::new(),
            curated_concepts: snapshot.curated_concepts,
            concept_relations: snapshot.concept_relations,
        })
    );
}

#[test]
fn sqlite_store_checkpoint_snapshots_do_not_bump_workspace_revision() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "prism-store-checkpoint-snapshot-revision-{nanos}.db"
    ));
    let mut store = SqliteStore::open(&path).unwrap();
    let base_revision = store.workspace_revision().unwrap();

    store
        .save_projection_snapshot(&ProjectionSnapshot {
            co_change_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:alpha"),
                vec![CoChangeRecord {
                    lineage: prism_ir::LineageId::new("lineage:beta"),
                    count: 1,
                }],
            )],
            validation_by_lineage: Vec::new(),
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
        .unwrap();
    store
        .save_workspace_tree_snapshot(&WorkspaceTreeSnapshot {
            root_hash: 7,
            files: vec![(
                PathBuf::from("src/lib.rs"),
                WorkspaceTreeFileFingerprint {
                    len: 12,
                    modified_ns: Some(1),
                    changed_ns: Some(1),
                    content_hash: 42,
                },
            )]
            .into_iter()
            .collect(),
            directories: vec![(
                PathBuf::from("src"),
                WorkspaceTreeDirectoryFingerprint {
                    aggregate_hash: 43,
                    file_count: 1,
                    modified_ns: Some(1),
                    changed_ns: Some(1),
                },
            )]
            .into_iter()
            .collect(),
        })
        .unwrap();

    assert_eq!(store.workspace_revision().unwrap(), base_revision);
}

#[test]
fn sqlite_store_save_graph_snapshot_materializes_without_bumping_workspace_revision() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("prism-store-graph-snapshot-revision-{nanos}.db"));
    let mut store = SqliteStore::open(&path).unwrap();
    let base_revision = store.workspace_revision().unwrap();

    let mut graph = Graph::new();
    graph.upsert_file(
        Path::new("src/lib.rs"),
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    store.save_graph_snapshot(&graph).unwrap();

    assert_eq!(store.workspace_revision().unwrap(), base_revision);
    let loaded_graph = store.load_graph().unwrap().unwrap();
    assert!(loaded_graph.nodes.contains_key(&NodeId::new(
        "demo",
        "demo::alpha",
        NodeKind::Function
    )));
}

#[test]
fn sqlite_store_commits_auxiliary_batches_atomically() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-aux-batch-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let episodic = EpisodicMemorySnapshot {
        entries: vec![MemoryEntry {
            id: MemoryId("episodic:9".to_string()),
            anchors: Vec::new(),
            kind: MemoryKind::Episodic,
            scope: prism_memory::MemoryScope::Local,
            content: "remember this".to_string(),
            metadata: serde_json::Value::Null,
            created_at: 9,
            source: MemorySource::Agent,
            trust: 0.6,
        }],
    };
    let inference = InferenceSnapshot {
        records: vec![InferredEdgeRecord {
            id: EdgeId("edge:9".to_string()),
            edge: Edge {
                kind: EdgeKind::Calls,
                source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                origin: EdgeOrigin::Inferred,
                confidence: 0.7,
            },
            scope: InferredEdgeScope::Persisted,
            task: None,
            evidence: vec!["batched".to_string()],
        }],
    };
    let outcome = OutcomeMemorySnapshot {
        events: vec![prism_memory::OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:batch"),
                ts: 11,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::NoteAdded,
            result: prism_memory::OutcomeResult::Success,
            summary: "stored with note".to_string(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        }],
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: None,
            outcome_events: outcome.events.clone(),
            validation_deltas: vec![ValidationDelta {
                lineage: prism_ir::LineageId::new("lineage:10"),
                label: "test:smoke".to_string(),
                score_delta: 2.0,
                last_seen: 11,
            }],
            memory_events: Vec::new(),
            episodic_snapshot: Some(episodic.clone()),
            inference_records: Vec::new(),
            inference_snapshot: Some(inference.clone()),
            curator_snapshot: None,
        })
        .unwrap();

    let loaded_outcomes = store.load_outcome_snapshot().unwrap().unwrap();
    assert_eq!(loaded_outcomes.events.len(), 1);
    assert_eq!(loaded_outcomes.events[0].summary, "stored with note");
    let stored_outcome_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM outcome_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored_outcome_rows, 1);
    let cached_outcome_rows: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM snapshots WHERE key = 'outcomes'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cached_outcome_rows, 0);
    assert_eq!(store.load_episodic_snapshot().unwrap(), Some(episodic));
    assert_eq!(store.load_inference_snapshot().unwrap(), Some(inference));
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: Vec::new(),
            validation_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:10"),
                vec![ValidationCheck {
                    label: "test:smoke".to_string(),
                    score: 2.0,
                    last_seen: 11,
                }],
            )],
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_merges_episodic_snapshots_append_only() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-episodic-append-only-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let alpha = MemoryEntry {
        id: MemoryId("episodic:1".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember alpha".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 1,
        source: MemorySource::Agent,
        trust: 0.7,
    };
    let beta = MemoryEntry {
        id: MemoryId("episodic:2".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember beta".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 2,
        source: MemorySource::Agent,
        trust: 0.8,
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: vec![alpha.clone()],
        })
        .unwrap();
    store
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: vec![beta.clone()],
        })
        .unwrap();

    assert_eq!(
        store.load_episodic_snapshot().unwrap(),
        Some(EpisodicMemorySnapshot {
            entries: vec![alpha.clone(), beta.clone()],
        })
    );

    let logged_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM memory_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(logged_rows, 2);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_auxiliary_outcome_snapshot_reload_preserves_external_updates() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-aux-outcome-cache-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let alpha = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:alpha"),
            ts: 1,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        anchors: Vec::new(),
        kind: prism_memory::OutcomeKind::NoteAdded,
        result: prism_memory::OutcomeResult::Success,
        summary: "alpha".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    let beta = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:beta"),
            ts: 2,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        anchors: Vec::new(),
        kind: prism_memory::OutcomeKind::NoteAdded,
        result: prism_memory::OutcomeResult::Success,
        summary: "beta".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    let gamma = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:gamma"),
            ts: 3,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        anchors: Vec::new(),
        kind: prism_memory::OutcomeKind::NoteAdded,
        result: prism_memory::OutcomeResult::Success,
        summary: "gamma".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };

    let mut store_a = SqliteStore::open(&path).unwrap();
    store_a
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(OutcomeMemorySnapshot {
                events: vec![alpha.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    let mut store_b = SqliteStore::open(&path).unwrap();
    store_b
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(OutcomeMemorySnapshot {
                events: vec![beta.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    store_a
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            outcome_snapshot: Some(OutcomeMemorySnapshot {
                events: vec![gamma.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    let loaded = store_a.load_outcome_snapshot().unwrap().unwrap();
    let loaded_ids = loaded
        .events
        .into_iter()
        .map(|event| event.meta.id.0)
        .collect::<Vec<_>>();
    assert_eq!(
        loaded_ids,
        vec![
            gamma.meta.id.0.to_string(),
            beta.meta.id.0.to_string(),
            alpha.meta.id.0.to_string()
        ]
    );

    drop(store_b);
    drop(store_a);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn runtime_reader_opens_while_writer_holds_immediate_transaction() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-runtime-reader-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let writer = SqliteStore::open(&path).unwrap();
    writer.conn.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let reader = writer.reopen_runtime_reader().unwrap();
    assert_eq!(reader.workspace_revision().unwrap(), 0);

    writer.conn.execute_batch("ROLLBACK;").unwrap();
    drop(reader);
    drop(writer);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_auxiliary_episodic_snapshot_reload_preserves_external_updates() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-aux-episodic-cache-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let alpha = MemoryEntry {
        id: MemoryId("episodic:alpha".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember alpha".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 1,
        source: MemorySource::Agent,
        trust: 0.7,
    };
    let beta = MemoryEntry {
        id: MemoryId("episodic:beta".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember beta".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 2,
        source: MemorySource::Agent,
        trust: 0.8,
    };
    let gamma = MemoryEntry {
        id: MemoryId("episodic:gamma".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
        scope: prism_memory::MemoryScope::Local,
        content: "remember gamma".to_string(),
        metadata: serde_json::Value::Null,
        created_at: 3,
        source: MemorySource::Agent,
        trust: 0.9,
    };

    let mut store_a = SqliteStore::open(&path).unwrap();
    store_a
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            episodic_snapshot: Some(EpisodicMemorySnapshot {
                entries: vec![alpha.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    let mut store_b = SqliteStore::open(&path).unwrap();
    store_b
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            episodic_snapshot: Some(EpisodicMemorySnapshot {
                entries: vec![beta.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    store_a
        .commit_auxiliary_persist_batch(&AuxiliaryPersistBatch {
            episodic_snapshot: Some(EpisodicMemorySnapshot {
                entries: vec![gamma.clone()],
            }),
            ..AuxiliaryPersistBatch::default()
        })
        .unwrap();

    assert_eq!(
        store_a.load_episodic_snapshot().unwrap(),
        Some(EpisodicMemorySnapshot {
            entries: vec![alpha, beta, gamma],
        })
    );

    drop(store_b);
    drop(store_a);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_migrates_snapshot_backed_outcomes_to_append_only_log() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-outcome-migration-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let outcomes = OutcomeMemorySnapshot {
        events: vec![prism_memory::OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:migrated"),
                ts: 7,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::NoteAdded,
            result: prism_memory::OutcomeResult::Success,
            summary: "migrated from snapshot".to_string(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        }],
    };

    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE snapshots (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 16;
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO snapshots(key, value) VALUES (?1, ?2)",
            ("outcomes", serde_json::to_string(&outcomes).unwrap()),
        )
        .unwrap();
    }

    let mut store = SqliteStore::open(&path).unwrap();
    let loaded = store.load_outcome_snapshot().unwrap().unwrap();
    assert_eq!(loaded.events, outcomes.events);
    let logged_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM outcome_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(logged_rows, 1);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_compacts_hot_patch_outcomes_when_loading_event_log() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-hot-patch-compaction-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let changed_symbols = (0..400_u32)
        .map(|index| {
            serde_json::json!({
                "status": "updated_after",
                "id": {
                    "crate_name": "demo",
                    "path": format!("demo::symbol_{index}"),
                    "kind": "Function",
                },
                "name": format!("symbol_{index}"),
                "kind": "Function",
                "filePath": "src/lib.rs",
                "span": {
                    "start": index * 8,
                    "end": index * 8 + 7,
                },
            })
        })
        .collect::<Vec<_>>();
    let outcomes = OutcomeMemorySnapshot {
        events: vec![prism_memory::OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:hot-patch"),
                ts: 7,
                actor: EventActor::System,
                correlation: None,
                causation: None,
                execution_context: None,
            },
            anchors: Vec::new(),
            kind: prism_memory::OutcomeKind::PatchApplied,
            result: prism_memory::OutcomeResult::Success,
            summary: "large patch".to_string(),
            evidence: Vec::new(),
            metadata: serde_json::json!({
                "trigger": "FsWatch",
                "filePaths": ["src/lib.rs"],
                "changedSymbols": changed_symbols,
            }),
        }],
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.save_outcome_snapshot(&outcomes).unwrap();

    let loaded = store.load_outcome_snapshot().unwrap().unwrap();
    let metadata = loaded.events[0].metadata.as_object().unwrap();
    assert_eq!(
        metadata["changedSymbols"].as_array().unwrap().len(),
        256,
        "hot patch payload should be capped in memory"
    );
    assert_eq!(metadata["changedSymbolsTotalCount"].as_u64().unwrap(), 400);
    assert!(metadata["changedSymbolsTruncated"].as_bool().unwrap());
    let file_summary = metadata["changedFilesSummary"]
        .as_array()
        .unwrap()
        .first()
        .unwrap();
    assert_eq!(file_summary["filePath"].as_str().unwrap(), "src/lib.rs");
    assert_eq!(file_summary["changedSymbolCount"].as_u64().unwrap(), 400);
    assert_eq!(file_summary["updatedCount"].as_u64().unwrap(), 400);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_compacts_hot_patch_outcomes_on_open_and_rewrites_payload() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-hot-patch-open-compaction-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let changed_symbols = (0..400_u32)
        .map(|index| {
            serde_json::json!({
                "status": "updated_after",
                "id": {
                    "crate_name": "demo",
                    "path": format!("demo::symbol_{index}"),
                    "kind": "Function",
                },
                "name": format!("symbol_{index}"),
                "kind": "Function",
                "filePath": "src/lib.rs",
                "span": {
                    "start": index * 8,
                    "end": index * 8 + 7,
                },
            })
        })
        .collect::<Vec<_>>();
    let raw_event = serde_json::json!({
        "meta": {
            "id": "outcome:legacy-hot-patch",
            "ts": 7,
            "actor": "System",
            "correlation": null,
            "causation": null
        },
        "anchors": [],
        "kind": "PatchApplied",
        "result": "Success",
        "summary": "large patch",
        "evidence": [],
        "metadata": {
            "trigger": "FsWatch",
            "filePaths": ["src/lib.rs"],
            "changedSymbols": changed_symbols
        }
    });

    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            CREATE TABLE outcome_event_log (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL UNIQUE,
                ts INTEGER NOT NULL,
                payload TEXT NOT NULL
            );
            PRAGMA user_version = 16;
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO outcome_event_log(event_id, ts, payload) VALUES (?1, ?2, ?3)",
            (
                "outcome:legacy-hot-patch",
                7_i64,
                serde_json::to_string(&raw_event).unwrap(),
            ),
        )
        .unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();
    let payload: String = store
        .conn
        .query_row(
            "SELECT payload FROM outcome_event_log WHERE event_id = 'outcome:legacy-hot-patch'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let metadata = value["metadata"].as_object().unwrap();
    assert_eq!(metadata["changedSymbols"].as_array().unwrap().len(), 256);
    assert_eq!(metadata["changedSymbolsTotalCount"].as_u64().unwrap(), 400);
    assert!(metadata["changedSymbolsTruncated"].as_bool().unwrap());
    let compacted: i64 = store
        .conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'outcomes:hot_patch_payloads_compacted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(compacted, 1);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_reconciles_inference_snapshot_updates_and_removals() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-inference-reconcile-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    let gamma = NodeId::new("demo", "demo::gamma", NodeKind::Function);

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .save_inference_snapshot(&InferenceSnapshot {
            records: vec![InferredEdgeRecord {
                id: EdgeId("edge:1".to_string()),
                edge: Edge {
                    kind: EdgeKind::Calls,
                    source: alpha.clone(),
                    target: beta,
                    origin: EdgeOrigin::Inferred,
                    confidence: 0.9,
                },
                scope: InferredEdgeScope::Persisted,
                task: None,
                evidence: vec!["initial".to_string()],
            }],
        })
        .unwrap();

    store
        .save_inference_snapshot(&InferenceSnapshot {
            records: vec![InferredEdgeRecord {
                id: EdgeId("edge:1".to_string()),
                edge: Edge {
                    kind: EdgeKind::Calls,
                    source: alpha,
                    target: gamma.clone(),
                    origin: EdgeOrigin::Inferred,
                    confidence: 0.95,
                },
                scope: InferredEdgeScope::Persisted,
                task: None,
                evidence: vec!["updated".to_string()],
            }],
        })
        .unwrap();

    let loaded = store.load_inference_snapshot().unwrap().unwrap();
    assert_eq!(loaded.records.len(), 1);
    assert_eq!(loaded.records[0].edge.target, gamma);
    assert_eq!(loaded.records[0].evidence, vec!["updated".to_string()]);

    store
        .save_inference_snapshot(&InferenceSnapshot {
            records: Vec::new(),
        })
        .unwrap();
    assert_eq!(store.load_inference_snapshot().unwrap(), None);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_migrates_snapshot_backed_episodic_memory_to_append_only_log() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-episodic-migration-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let episodic = EpisodicMemorySnapshot {
        entries: vec![MemoryEntry {
            id: MemoryId("episodic:7".to_string()),
            anchors: Vec::new(),
            kind: MemoryKind::Episodic,
            scope: prism_memory::MemoryScope::Local,
            content: "remember alpha".to_string(),
            metadata: serde_json::Value::Null,
            created_at: 7,
            source: MemorySource::Agent,
            trust: 0.7,
        }],
    };

    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            CREATE TABLE snapshots (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA user_version = 11;
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO snapshots(key, value) VALUES (?1, ?2)",
            ("episodic", serde_json::to_string(&episodic).unwrap()),
        )
        .unwrap();
    }

    let mut store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.load_episodic_snapshot().unwrap(), Some(episodic));

    let user_version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 20);

    let logged_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM memory_event_log", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(logged_rows, 1);

    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

#[test]
fn sqlite_store_commits_index_batches_atomically() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-batch-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();
    graph.upsert_file(
        &source_path,
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let batch = IndexPersistBatch {
        upserted_paths: vec![source_path.clone()],
        in_place_upserted_paths: Vec::new(),
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        history_delta: None,
        outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
        outcome_events: Vec::new(),
        defer_graph_materialization: false,
        co_change_deltas: vec![CoChangeDelta {
            source_lineage: prism_ir::LineageId::new("lineage:1"),
            target_lineage: prism_ir::LineageId::new("lineage:2"),
            count_delta: 1,
        }],
        validation_deltas: vec![ValidationDelta {
            lineage: prism_ir::LineageId::new("lineage:1"),
            label: "test:smoke".to_string(),
            score_delta: 1.5,
            last_seen: 7,
        }],
        projection_snapshot: None,
        workspace_tree_snapshot: None,
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.commit_index_persist_batch(&graph, &batch).unwrap();

    let loaded_graph = store.load_graph().unwrap().unwrap();
    assert!(loaded_graph.file_state(&source_path).is_some());
    assert_eq!(
        store.load_projection_snapshot().unwrap(),
        Some(ProjectionSnapshot {
            co_change_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:1"),
                vec![CoChangeRecord {
                    lineage: prism_ir::LineageId::new("lineage:2"),
                    count: 1,
                }],
            )],
            validation_by_lineage: vec![(
                prism_ir::LineageId::new("lineage:1"),
                vec![ValidationCheck {
                    label: "test:smoke".to_string(),
                    score: 1.5,
                    last_seen: 7,
                }],
            )],
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_applies_incremental_history_delta() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-history-delta-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source_path = PathBuf::from("src/lib.rs");
    let lineage = prism_ir::LineageId::new("lineage:1");
    let neighbor = prism_ir::LineageId::new("lineage:2");
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

    let born_event = prism_ir::LineageEvent {
        meta: EventMeta {
            id: EventId::new("evt:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Born,
        before: Vec::new(),
        after: vec![alpha.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    };
    let renamed_event = prism_ir::LineageEvent {
        meta: EventMeta {
            id: EventId::new("evt:2"),
            ts: 2,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Updated,
        before: vec![alpha.clone()],
        after: vec![beta.clone()],
        confidence: 1.0,
        evidence: Vec::new(),
    };

    let initial_snapshot = HistorySnapshot {
        node_to_lineage: vec![(alpha.clone(), lineage.clone())],
        events: vec![born_event.clone()],
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 2,
    };
    let expected_snapshot = HistorySnapshot {
        node_to_lineage: vec![(beta.clone(), lineage.clone())],
        events: vec![born_event, renamed_event.clone()],
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 3,
    };

    let mut initial_graph = Graph::new();
    initial_graph.upsert_file(
        &source_path,
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .commit_index_persist_batch(
            &initial_graph,
            &IndexPersistBatch {
                upserted_paths: vec![source_path.clone()],
                in_place_upserted_paths: Vec::new(),
                removed_paths: Vec::new(),
                history_snapshot: initial_snapshot.clone(),
                history_delta: None,
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                outcome_events: Vec::new(),
                defer_graph_materialization: false,
                co_change_deltas: vec![CoChangeDelta {
                    source_lineage: lineage.clone(),
                    target_lineage: neighbor.clone(),
                    count_delta: 1,
                }],
                validation_deltas: Vec::new(),
                projection_snapshot: None,
                workspace_tree_snapshot: None,
            },
        )
        .unwrap();

    let mut renamed_graph = Graph::new();
    renamed_graph.upsert_file(
        &source_path,
        2,
        vec![node("beta")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    store
        .commit_index_persist_batch(
            &renamed_graph,
            &IndexPersistBatch {
                upserted_paths: vec![source_path],
                in_place_upserted_paths: Vec::new(),
                removed_paths: Vec::new(),
                history_snapshot: expected_snapshot.clone(),
                history_delta: Some(HistoryPersistDelta {
                    removed_nodes: vec![alpha],
                    upserted_node_lineages: vec![(beta, lineage.clone())],
                    appended_events: vec![renamed_event],
                    upserted_tombstones: Vec::<LineageTombstone>::new(),
                    removed_tombstone_lineages: Vec::new(),
                    next_lineage: 1,
                    next_event: 3,
                }),
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                outcome_events: Vec::new(),
                defer_graph_materialization: false,
                co_change_deltas: vec![CoChangeDelta {
                    source_lineage: lineage.clone(),
                    target_lineage: neighbor.clone(),
                    count_delta: 1,
                }],
                validation_deltas: Vec::new(),
                projection_snapshot: None,
                workspace_tree_snapshot: None,
            },
        )
        .unwrap();

    assert_eq!(
        store.load_history_snapshot().unwrap(),
        Some(expected_snapshot)
    );

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_tolerates_duplicate_node_ids_in_single_file_state() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-duplicate-node-test-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();
    graph.upsert_file(
        &source_path,
        1,
        vec![node("alpha"), node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let batch = IndexPersistBatch {
        upserted_paths: vec![source_path],
        in_place_upserted_paths: Vec::new(),
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        history_delta: None,
        outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
        outcome_events: Vec::new(),
        defer_graph_materialization: false,
        co_change_deltas: Vec::new(),
        validation_deltas: Vec::new(),
        projection_snapshot: None,
        workspace_tree_snapshot: None,
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.commit_index_persist_batch(&graph, &batch).unwrap();

    let node_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(node_rows, 1);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_index_batch_appends_outcome_events_without_snapshot_reload() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-index-outcome-events-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();
    graph.upsert_file(
        &source_path,
        1,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let event = prism_memory::OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:index-batch"),
            ts: 42,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        anchors: Vec::new(),
        kind: prism_memory::OutcomeKind::PatchApplied,
        result: prism_memory::OutcomeResult::Success,
        summary: "index batch persisted direct outcome event".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };

    let batch = IndexPersistBatch {
        upserted_paths: vec![source_path],
        in_place_upserted_paths: Vec::new(),
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        history_delta: None,
        outcome_snapshot: OutcomeMemorySnapshot {
            events: vec![event.clone()],
        },
        outcome_events: vec![event.clone()],
        defer_graph_materialization: false,
        co_change_deltas: Vec::new(),
        validation_deltas: Vec::new(),
        projection_snapshot: None,
        workspace_tree_snapshot: None,
    };

    let mut store = SqliteStore::open(&path).unwrap();
    store.commit_index_persist_batch(&graph, &batch).unwrap();

    let loaded_outcomes = store.load_outcome_snapshot().unwrap().unwrap();
    assert_eq!(loaded_outcomes.events, vec![event]);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_store_index_batch_updates_structurally_unchanged_file_state_in_place() {
    let path = std::env::temp_dir().join(format!(
        "prism-store-index-in-place-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();
    graph.upsert_file_from(
        None,
        &source_path,
        1,
        prism_parser::ParseDepth::Deep,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    );

    let mut store = SqliteStore::open(&path).unwrap();
    store
        .commit_index_persist_batch(
            &graph,
            &IndexPersistBatch {
                upserted_paths: vec![source_path.clone()],
                in_place_upserted_paths: Vec::new(),
                removed_paths: Vec::new(),
                history_snapshot: HistorySnapshot {
                    node_to_lineage: Vec::new(),
                    events: Vec::new(),
                    tombstones: Vec::new(),
                    next_lineage: 1,
                    next_event: 2,
                },
                history_delta: None,
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                outcome_events: Vec::new(),
                defer_graph_materialization: false,
                co_change_deltas: Vec::new(),
                validation_deltas: Vec::new(),
                projection_snapshot: None,
                workspace_tree_snapshot: None,
            },
        )
        .unwrap();

    let update = graph.upsert_file_from(
        None,
        &source_path,
        2,
        prism_parser::ParseDepth::Deep,
        vec![node("alpha")],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    );
    assert!(update.persist_in_place);

    store
        .commit_index_persist_batch(
            &graph,
            &IndexPersistBatch {
                upserted_paths: Vec::new(),
                in_place_upserted_paths: vec![source_path.clone()],
                removed_paths: Vec::new(),
                history_snapshot: HistorySnapshot {
                    node_to_lineage: Vec::new(),
                    events: Vec::new(),
                    tombstones: Vec::new(),
                    next_lineage: 1,
                    next_event: 2,
                },
                history_delta: None,
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                outcome_events: Vec::new(),
                defer_graph_materialization: false,
                co_change_deltas: Vec::new(),
                validation_deltas: Vec::new(),
                projection_snapshot: None,
                workspace_tree_snapshot: None,
            },
        )
        .unwrap();

    let reloaded = store.load_graph().unwrap().unwrap();
    let state = reloaded.file_state(&source_path).unwrap();
    assert_eq!(state.record.hash, 2);
    assert_eq!(state.nodes.len(), 1);

    drop(store);
    let _ = std::fs::remove_file(path);
}
