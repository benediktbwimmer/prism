use prism_ir::{
    ChangeTrigger, Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta, FileId, Language,
    Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode, Span, SymbolFingerprint,
};

use crate::resolver::last_path_segment;
use crate::HistoryStore;

fn function(path: &str, file_id: u32) -> Node {
    Node {
        id: NodeId::new("demo", path, NodeKind::Function),
        name: last_path_segment(path).into(),
        kind: NodeKind::Function,
        file: FileId(file_id),
        span: Span::line(1),
        language: Language::Rust,
    }
}

fn module(path: &str) -> NodeId {
    NodeId::new("demo", path, NodeKind::Module)
}

fn observed(node: Node, signature: u64, body: u64) -> ObservedNode {
    ObservedNode {
        node,
        fingerprint: SymbolFingerprint::with_parts(signature, Some(body), Some(body), None),
    }
}

fn change_set(added: Vec<ObservedNode>, removed: Vec<ObservedNode>) -> ObservedChangeSet {
    ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("change:1"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        trigger: ChangeTrigger::ManualReindex,
        files: vec![FileId(1)],
        previous_path: Some("/workspace/src/lib.rs".into()),
        current_path: Some("/workspace/src/lib.rs".into()),
        added,
        removed,
        updated: Vec::new(),
        edge_added: vec![Edge {
            kind: EdgeKind::Contains,
            source: NodeId::new("demo", "demo", NodeKind::Module),
            target: NodeId::new("demo", "demo::new_name", NodeKind::Function),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        }],
        edge_removed: Vec::new(),
    }
}

#[test]
fn matches_rename_by_fingerprint() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::old_name", NodeKind::Function)]);

    let events = history.apply(&change_set(
        vec![observed(function("demo::new_name", 1), 10, 20)],
        vec![observed(function("demo::old_name", 1), 10, 20)],
    ));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, prism_ir::LineageEventKind::Renamed);
    assert!(events[0]
        .evidence
        .contains(&prism_ir::LineageEvidence::FingerprintMatch));
    let lineage = history
        .lineage_of(&NodeId::new("demo", "demo::new_name", NodeKind::Function))
        .unwrap();
    assert_eq!(history.lineage_history(&lineage).len(), 1);
}

#[test]
fn allocates_born_events_without_false_fingerprint_evidence() {
    let mut history = HistoryStore::new();
    let events = history.apply(&change_set(
        vec![observed(function("demo::new_name", 1), 10, 20)],
        Vec::new(),
    ));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, prism_ir::LineageEventKind::Born);
    assert!(events[0].evidence.is_empty());
}

#[test]
fn allocates_died_events_without_false_fingerprint_evidence() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::old_name", NodeKind::Function)]);

    let events = history.apply(&change_set(
        Vec::new(),
        vec![observed(function("demo::old_name", 1), 10, 20)],
    ));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, prism_ir::LineageEventKind::Died);
    assert!(events[0].evidence.is_empty());
}

#[test]
fn records_split_when_one_symbol_matches_many_new_symbols() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::alpha", NodeKind::Function)]);

    let events = history.apply(&change_set(
        vec![
            observed(function("demo::alpha_fast", 1), 10, 20),
            observed(function("demo::alpha_safe", 1), 10, 20),
        ],
        vec![observed(function("demo::alpha", 1), 10, 20)],
    ));

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, prism_ir::LineageEventKind::Split);
    let left = history
        .lineage_of(&NodeId::new("demo", "demo::alpha_fast", NodeKind::Function))
        .unwrap();
    let right = history
        .lineage_of(&NodeId::new("demo", "demo::alpha_safe", NodeKind::Function))
        .unwrap();
    assert_eq!(left, right);
}

#[test]
fn records_merge_and_retires_noncanonical_lineages() {
    let mut history = HistoryStore::new();
    let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);
    history.seed_nodes([alpha.clone(), beta.clone()]);
    let alpha_lineage = history.lineage_of(&alpha).unwrap();
    let beta_lineage = history.lineage_of(&beta).unwrap();

    let events = history.apply(&change_set(
        vec![observed(function("demo::combined", 1), 10, 20)],
        vec![
            observed(function("demo::alpha", 1), 10, 20),
            observed(function("demo::beta", 1), 10, 20),
        ],
    ));

    assert!(events
        .iter()
        .any(|event| event.kind == prism_ir::LineageEventKind::Merged));
    assert!(events
        .iter()
        .any(|event| event.kind == prism_ir::LineageEventKind::Died));
    let combined = history
        .lineage_of(&NodeId::new("demo", "demo::combined", NodeKind::Function))
        .unwrap();
    assert!(combined == alpha_lineage || combined == beta_lineage);
}

#[test]
fn records_ambiguous_when_multiple_old_symbols_compete_for_one_new_symbol() {
    let mut history = HistoryStore::new();
    history.seed_nodes([
        NodeId::new("demo", "demo::alpha", NodeKind::Function),
        NodeId::new("demo", "demo::beta", NodeKind::Function),
    ]);

    let events = history.apply(&change_set(
        vec![
            observed(function("demo::combined_a", 1), 10, 20),
            observed(function("demo::combined_b", 1), 10, 20),
        ],
        vec![
            observed(function("demo::alpha", 1), 10, 20),
            observed(function("demo::beta", 1), 10, 20),
        ],
    ));

    assert!(events
        .iter()
        .any(|event| event.kind == prism_ir::LineageEventKind::Ambiguous));
}

#[test]
fn revives_dead_lineage_when_symbol_returns() {
    let mut history = HistoryStore::new();
    let old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    history.seed_nodes([old.clone()]);
    let original_lineage = history.lineage_of(&old).unwrap();

    history.apply(&change_set(
        Vec::new(),
        vec![observed(function("demo::alpha", 1), 10, 20)],
    ));
    let revive = history.apply(&change_set(
        vec![observed(function("demo::alpha_v2", 1), 10, 20)],
        Vec::new(),
    ));

    assert!(revive
        .iter()
        .any(|event| event.kind == prism_ir::LineageEventKind::Revived));
    let revived_lineage = history
        .lineage_of(&NodeId::new("demo", "demo::alpha_v2", NodeKind::Function))
        .unwrap();
    assert_eq!(revived_lineage, original_lineage);
}

#[test]
fn adds_same_container_evidence_when_parent_lineage_is_stable() {
    let mut history = HistoryStore::new();
    history.seed_nodes([
        module("demo::parent"),
        NodeId::new("demo", "demo::parent::old_name", NodeKind::Function),
    ]);

    let events = history.apply(&change_set(
        vec![observed(function("demo::parent::new_name", 1), 10, 20)],
        vec![observed(function("demo::parent::old_name", 1), 10, 20)],
    ));

    assert!(events[0]
        .evidence
        .contains(&prism_ir::LineageEvidence::SameContainerLineage));
}

#[test]
fn adds_file_move_hint_when_path_changes() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new(
        "demo",
        "demo::feature::alpha",
        NodeKind::Function,
    )]);

    let mut moved = change_set(
        vec![observed(function("demo::renamed::alpha", 1), 10, 20)],
        vec![observed(function("demo::feature::alpha", 1), 10, 20)],
    );
    moved.previous_path = Some("/workspace/src/feature.rs".into());
    moved.current_path = Some("/workspace/src/renamed.rs".into());

    let events = history.apply(&moved);
    assert_eq!(events[0].kind, prism_ir::LineageEventKind::Moved);
    assert!(events[0]
        .evidence
        .contains(&prism_ir::LineageEvidence::FileMoveHint));
}

#[test]
fn adds_git_rename_hint_for_git_triggered_moves() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new(
        "demo",
        "demo::feature::alpha",
        NodeKind::Function,
    )]);

    let mut moved = change_set(
        vec![observed(function("demo::renamed::alpha", 1), 10, 20)],
        vec![observed(function("demo::feature::alpha", 1), 10, 20)],
    );
    moved.trigger = ChangeTrigger::GitCheckout;
    moved.previous_path = Some("/workspace/src/feature.rs".into());
    moved.current_path = Some("/workspace/src/renamed.rs".into());

    let events = history.apply(&moved);
    assert!(events[0]
        .evidence
        .contains(&prism_ir::LineageEvidence::GitRenameHint));
}

#[test]
fn snapshot_round_trip_preserves_tombstones() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::alpha", NodeKind::Function)]);
    history.apply(&change_set(
        Vec::new(),
        vec![observed(function("demo::alpha", 1), 10, 20)],
    ));

    let snapshot = history.snapshot();
    let restored = HistoryStore::from_snapshot(snapshot);
    assert_eq!(restored.snapshot().tombstones.len(), 1);
}

#[test]
fn snapshot_round_trip_preserves_lineage_node_index() {
    let mut history = HistoryStore::new();
    history.seed_nodes([NodeId::new("demo", "demo::alpha", NodeKind::Function)]);
    history.apply(&change_set(
        vec![observed(function("demo::alpha_v2", 1), 10, 20)],
        vec![observed(function("demo::alpha", 1), 10, 20)],
    ));

    let lineage = history
        .lineage_of(&NodeId::new("demo", "demo::alpha_v2", NodeKind::Function))
        .unwrap();
    let expected = history.current_nodes_for_lineage(&lineage);

    let restored = HistoryStore::from_snapshot(history.snapshot());
    assert_eq!(restored.current_nodes_for_lineage(&lineage), expected);
}
