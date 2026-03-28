use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use prism_agent::{EdgeId, InferenceSnapshot, InferredEdgeRecord, InferredEdgeScope};
use prism_history::{HistoryPersistDelta, HistorySnapshot, LineageTombstone};
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
use rusqlite::Connection;

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
    };

    graph.upsert_file_from_with_observed_without_rebuild(
        None,
        alpha_path,
        1,
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
fn memory_store_merges_episodic_snapshots_append_only() {
    let mut store = MemoryStore::default();
    let alpha = MemoryEntry {
        id: MemoryId("episodic:1".to_string()),
        anchors: Vec::new(),
        kind: MemoryKind::Episodic,
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
    assert_eq!(user_version, 12);
    assert!(indexed_tables.into_iter().all(|count| count == 1));

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
        .query_row("SELECT COUNT(*) FROM memory_entry_log", [], |row| {
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
    assert_eq!(user_version, 12);

    let logged_rows: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM memory_entry_log", [], |row| {
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
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            co_change_counts: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        history_delta: None,
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
        co_change_counts: vec![(lineage.clone(), neighbor.clone(), 1)],
        tombstones: Vec::new(),
        next_lineage: 1,
        next_event: 2,
    };
    let expected_snapshot = HistorySnapshot {
        node_to_lineage: vec![(beta.clone(), lineage.clone())],
        events: vec![born_event, renamed_event.clone()],
        co_change_counts: vec![(lineage.clone(), neighbor.clone(), 2)],
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
                removed_paths: Vec::new(),
                history_snapshot: initial_snapshot.clone(),
                history_delta: None,
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                co_change_deltas: vec![CoChangeDelta {
                    source_lineage: lineage.clone(),
                    target_lineage: neighbor.clone(),
                    count_delta: 1,
                }],
                validation_deltas: Vec::new(),
                projection_snapshot: None,
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
                removed_paths: Vec::new(),
                history_snapshot: expected_snapshot.clone(),
                history_delta: Some(HistoryPersistDelta {
                    removed_nodes: vec![alpha],
                    upserted_node_lineages: vec![(beta, lineage.clone())],
                    appended_events: vec![renamed_event],
                    co_change_deltas: vec![prism_history::HistoryCoChangeDelta {
                        source_lineage: lineage.clone(),
                        target_lineage: neighbor.clone(),
                        count_delta: 1,
                    }],
                    upserted_tombstones: Vec::<LineageTombstone>::new(),
                    removed_tombstone_lineages: Vec::new(),
                    next_lineage: 1,
                    next_event: 3,
                }),
                outcome_snapshot: OutcomeMemorySnapshot { events: Vec::new() },
                co_change_deltas: vec![CoChangeDelta {
                    source_lineage: lineage.clone(),
                    target_lineage: neighbor.clone(),
                    count_delta: 1,
                }],
                validation_deltas: Vec::new(),
                projection_snapshot: None,
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
        removed_paths: Vec::new(),
        history_snapshot: HistorySnapshot {
            node_to_lineage: Vec::new(),
            events: Vec::new(),
            co_change_counts: Vec::new(),
            tombstones: Vec::new(),
            next_lineage: 1,
            next_event: 2,
        },
        history_delta: None,
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
