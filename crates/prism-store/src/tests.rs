use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use prism_agent::{EdgeId, InferenceSnapshot, InferredEdgeRecord, InferredEdgeScope};
use prism_history::HistorySnapshot;
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta, FileId, GraphChange, Language,
    Node, NodeId, NodeKind, Span,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemorySource, OutcomeMemorySnapshot,
};
use prism_projections::{
    CoChangeDelta, CoChangeRecord, ProjectionSnapshot, ValidationCheck, ValidationDelta,
    MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE,
};

use crate::{AuxiliaryPersistBatch, Graph, IndexPersistBatch, MemoryStore, SqliteStore, Store};

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
fn memory_store_round_trips_auxiliary_snapshots() {
    let mut store = MemoryStore::default();
    let history = HistorySnapshot {
        node_to_lineage: Vec::new(),
        events: Vec::new(),
        co_change_counts: Vec::new(),
        tombstones: Vec::new(),
        next_lineage: 0,
        next_event: 0,
    };
    let episodic = EpisodicMemorySnapshot {
        entries: vec![MemoryEntry {
            id: MemoryId("episodic:7".to_string()),
            anchors: Vec::new(),
            kind: MemoryKind::Episodic,
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
    };

    store.save_history_snapshot(&history).unwrap();
    store.save_episodic_snapshot(&episodic).unwrap();
    store.save_inference_snapshot(&inference).unwrap();
    store.save_projection_snapshot(&projections).unwrap();

    let loaded_history = store.load_history_snapshot().unwrap().unwrap();
    assert!(loaded_history.node_to_lineage.is_empty());
    assert!(loaded_history.events.is_empty());
    assert_eq!(loaded_history.next_lineage, history.next_lineage);
    assert_eq!(loaded_history.next_event, history.next_event);
    assert_eq!(store.load_episodic_snapshot().unwrap(), Some(episodic));
    assert_eq!(store.load_inference_snapshot().unwrap(), Some(inference));
    assert_eq!(store.load_projection_snapshot().unwrap(), Some(projections));
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
    assert_eq!(neighbors.first().unwrap().count, (MAX_CO_CHANGE_NEIGHBORS_PER_LINEAGE + 8) as u32);
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
        co_change_counts: Vec::new(),
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
    assert_eq!(loaded_history.co_change_counts, history.co_change_counts);
    assert_eq!(loaded_history.next_lineage, history.next_lineage);
    assert_eq!(loaded_history.next_event, history.next_event);

    let loaded_outcomes = store.load_outcome_snapshot().unwrap().unwrap();
    assert!(loaded_outcomes.events.is_empty());
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
        })
    );

    drop(store);
    let _ = std::fs::remove_file(path);
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
            outcome_snapshot: Some(outcome),
            validation_deltas: vec![ValidationDelta {
                lineage: prism_ir::LineageId::new("lineage:10"),
                label: "test:smoke".to_string(),
                score_delta: 2.0,
                last_seen: 11,
            }],
            episodic_snapshot: Some(episodic.clone()),
            inference_snapshot: Some(inference.clone()),
            curator_snapshot: None,
            coordination_snapshot: None,
        })
        .unwrap();

    let loaded_outcomes = store.load_outcome_snapshot().unwrap().unwrap();
    assert_eq!(loaded_outcomes.events.len(), 1);
    assert_eq!(loaded_outcomes.events[0].summary, "stored with note");
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
        })
    );

    drop(store);
    let _ = std::fs::remove_file(path);
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
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            co_change_counts: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
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
        })
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
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            co_change_counts: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
        co_change_deltas: Vec::new(),
        validation_deltas: Vec::new(),
        projection_snapshot: None,
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
