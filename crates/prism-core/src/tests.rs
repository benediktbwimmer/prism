use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use prism_coordination::{
    ArtifactProposeInput, ClaimAcquireInput, CoordinationEvent, CoordinationSnapshot,
    CoordinationStore, HandoffInput, PlanCreateInput, TaskCreateInput, TaskUpdateInput,
};
use prism_curator::{
    CandidateRiskSummary, CuratorBackend, CuratorBudget, CuratorContext, CuratorJob,
    CuratorProposal, CuratorRun,
};
use prism_ir::{
    AnchorRef, ChangeTrigger, CoordinationEventKind, EdgeKind, EventActor, EventId, EventMeta,
    GraphChange, LineageEvent, LineageEventKind, LineageEvidence, LineageId, NodeId, NodeKind,
    SessionId, TaskId,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind, MemoryEventQuery, MemoryId,
    MemoryKind, MemoryModule, MemoryScope, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeRecallQuery, OutcomeResult, SessionMemory,
};
use prism_parser::ParseDepth;
use prism_projections::ProjectionSnapshot;
use prism_query::{
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptPacket,
    ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationEvent, ConceptRelationEventAction, ConceptRelationKind, ConceptScope,
    ContractCompatibility, ContractEvent, ContractEventAction, ContractGuarantee, ContractKind,
    ContractPacket, ContractStatus, ContractTarget, OutcomeReadBackend, Prism,
};
use prism_store::{Graph, MemoryStore, SqliteStore, Store};
use serde_json::json;

use super::{
    hydrate_workspace_session_with_options, index_workspace, index_workspace_session,
    index_workspace_session_with_curator, index_workspace_session_with_options, PrismDocSyncStatus,
    SharedRuntimeBackend, ValidationFeedbackCategory, ValidationFeedbackRecord,
    ValidationFeedbackVerdict, WorkspaceIndexer, WorkspaceSessionOptions,
};
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator_support::build_curator_context;
use crate::materialization::summarize_workspace_materialization;
use crate::memory_refresh::reanchor_persisted_memory_snapshot;
use crate::workspace_tree::build_workspace_tree_snapshot;

static NEXT_TEMP_WORKSPACE: AtomicU64 = AtomicU64::new(0);

#[test]
fn reindexes_incrementally_across_file_changes() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { beta(); }\nfn beta() {}\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();
    assert!(indexer.outcomes.snapshot().events.is_empty());

    let initial_calls = indexer
        .graph()
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .count();
    assert_eq!(initial_calls, 1);

    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { gamma(); }\nfn gamma() {}\n",
    )
    .unwrap();
    indexer.index().unwrap();

    let patch_events = indexer
        .outcomes
        .outcomes_for(
            &[AnchorRef::Node(NodeId::new(
                "demo",
                "demo::gamma",
                NodeKind::Function,
            ))],
            10,
        )
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .collect::<Vec<_>>();
    assert_eq!(patch_events.len(), 1);

    assert!(indexer
        .graph()
        .nodes_by_name("gamma")
        .into_iter()
        .any(|node| node.id.path == "prism::gamma" || node.id.path.ends_with("::gamma")));
    assert_eq!(
        indexer
            .graph()
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Calls)
            .count(),
        1
    );

    fs::remove_file(root.join("src/lib.rs")).unwrap();
    indexer.index().unwrap();

    let removal_patch_events = indexer
        .outcomes
        .snapshot()
        .events
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .count();
    assert_eq!(removal_patch_events, 2);

    assert!(indexer.graph().nodes_by_name("alpha").is_empty());
    assert!(indexer
        .graph()
        .edges
        .iter()
        .all(|edge| edge.kind != EdgeKind::Calls));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn hydrated_workspace_session_marks_background_refresh_pending() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let _ =
        index_workspace_session_with_options(&root, WorkspaceSessionOptions::default()).unwrap();

    fs::write(root.join("src/lib.rs"), "pub fn beta() {}\n").unwrap();

    let session =
        hydrate_workspace_session_with_options(&root, WorkspaceSessionOptions::default()).unwrap();
    assert!(session.needs_refresh());
}

#[test]
fn reanchors_persisted_memory_snapshot_from_lineage_events() {
    let old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
    let new = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
    let lineage = LineageId::new("lineage:alpha");

    let memory = SessionMemory::new();
    let mut entry = MemoryEntry::new(MemoryKind::Episodic, "alpha needs care during edits");
    entry.anchors = vec![AnchorRef::Node(old.clone())];
    memory.store(entry).unwrap();

    let mut store = MemoryStore::default();
    store
        .save_episodic_snapshot(&EpisodicMemorySnapshot {
            entries: memory.snapshot().entries,
        })
        .unwrap();

    reanchor_persisted_memory_snapshot(
        &mut store,
        &[LineageEvent {
            meta: EventMeta {
                id: EventId::new("lineage:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            lineage: lineage.clone(),
            kind: LineageEventKind::Renamed,
            before: vec![old],
            after: vec![new.clone()],
            confidence: 1.0,
            evidence: vec![LineageEvidence::BodyHashMatch],
        }],
    )
    .unwrap();

    let snapshot = store.load_episodic_snapshot().unwrap().unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    assert!(snapshot.entries[0]
        .anchors
        .contains(&AnchorRef::Node(new.clone())));
    assert!(snapshot.entries[0]
        .anchors
        .contains(&AnchorRef::Lineage(lineage)));
}

#[test]
fn reloads_graph_from_disk_cache() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

    let mut first = WorkspaceIndexer::new(&root).unwrap();
    first.index().unwrap();
    drop(first);

    assert!(root.join(".prism/cache.db").exists());

    let second = WorkspaceIndexer::new(&root).unwrap();
    assert!(second
        .graph()
        .nodes_by_name("alpha")
        .into_iter()
        .any(|node| node.id.path.ends_with("::alpha")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn ignores_gitignored_paths_during_indexing() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(
        root.join("node_modules/pkg/ignored.json"),
        "{\"ignoredConfig\":{\"enabled\":true}}\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    assert!(indexer
        .graph()
        .tracked_files()
        .into_iter()
        .all(|path| !path.starts_with(root.join("node_modules"))));
    assert!(indexer.graph().nodes_by_name("ignoredConfig").is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn validation_feedback_persists_across_workspace_reloads() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();

    let entry = session
        .append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some("task:feedback".to_string()),
            context: "blast-radius check for alpha".to_string(),
            anchors: vec![AnchorRef::Node(alpha.clone())],
            prism_said: "Prism only surfaced alpha".to_string(),
            actually_true: "beta and gamma were also impacted through callers".to_string(),
            category: ValidationFeedbackCategory::Projection,
            verdict: ValidationFeedbackVerdict::Wrong,
            corrected_manually: true,
            correction: Some("verified callers directly and expanded the edit set".to_string()),
            metadata: serde_json::json!({
                "query": "prism.blastRadius(alpha)",
            }),
        })
        .unwrap();
    assert!(entry.id.starts_with("feedback:"));
    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let entries = reloaded.validation_feedback(Some(10)).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].task_id.as_deref(), Some("task:feedback"));
    assert_eq!(entries[0].category, ValidationFeedbackCategory::Projection);
    assert_eq!(entries[0].verdict, ValidationFeedbackVerdict::Wrong);
    assert_eq!(entries[0].anchors, vec![AnchorRef::Node(alpha)]);
    assert_eq!(
        entries[0].metadata["query"].as_str(),
        Some("prism.blastRadius(alpha)")
    );

    drop(reloaded);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn validation_feedback_writes_do_not_wait_for_refresh_lock() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let entry = session
        .append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some("task:feedback".to_string()),
            context: "feedback should not block on refresh".to_string(),
            anchors: Vec::new(),
            prism_said: "mutation blocked behind refresh".to_string(),
            actually_true: "validation feedback can append independently".to_string(),
            category: ValidationFeedbackCategory::Memory,
            verdict: ValidationFeedbackVerdict::Helpful,
            corrected_manually: false,
            correction: None,
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    assert!(entry.id.starts_with("feedback:"));
    assert_eq!(session.validation_feedback(Some(5)).unwrap().len(), 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn try_append_outcome_defers_when_refresh_is_in_progress() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let event = OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:test:busy".to_string()),
            ts: 1,
            actor: EventActor::Agent,
            correlation: Some(TaskId::new("task:busy".to_string())),
            causation: None,
        },
        anchors: Vec::new(),
        kind: OutcomeKind::PlanCreated,
        result: OutcomeResult::Success,
        summary: "busy refresh".to_string(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };

    assert!(session.try_append_outcome(event).unwrap().is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn try_mutate_coordination_defers_when_refresh_is_in_progress() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    assert!(session
        .try_mutate_coordination_with_session(None, |_| Ok::<_, anyhow::Error>(()))
        .unwrap()
        .is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn try_ensure_paths_deep_defers_when_refresh_is_in_progress() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    assert!(session
        .try_ensure_paths_deep([root.join("src/lib.rs")])
        .unwrap()
        .is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn uses_member_package_identity_and_attaches_workspace_docs() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("crates/alpha/src")).unwrap();
    fs::create_dir_all(root.join("crates/beta/src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/alpha/Cargo.toml"),
        "[package]\nname = \"alpha-pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/beta/Cargo.toml"),
        "[package]\nname = \"beta-pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("crates/alpha/src/lib.rs"), "fn alpha() {}\n").unwrap();
    fs::write(
        root.join("crates/beta/src/lib.rs"),
        "mod outer { mod inner {} }\n",
    )
    .unwrap();
    fs::write(root.join("docs/SPEC.md"), "# Spec\n").unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    assert!(indexer
        .graph()
        .nodes_by_name("alpha")
        .into_iter()
        .any(|node| node.id.crate_name == "alpha_pkg" && node.id.path == "alpha_pkg::alpha"));
    assert!(indexer
        .graph()
        .nodes_by_name("inner")
        .into_iter()
        .any(|node| node.id.crate_name == "beta_pkg" && node.id.path == "beta_pkg::outer::inner"));

    let inner_module = indexer
        .graph()
        .nodes_by_name("inner")
        .into_iter()
        .find(|node| node.kind == NodeKind::Module)
        .unwrap();
    assert!(!indexer
        .graph()
        .edges_to(&inner_module.id, Some(EdgeKind::Contains))
        .iter()
        .any(|edge| edge.source.kind == NodeKind::Package));

    let spec = indexer
        .graph()
        .nodes_by_name("Spec")
        .into_iter()
        .find(|node| node.kind == NodeKind::MarkdownHeading)
        .unwrap();
    let spec_document = indexer
        .graph()
        .nodes_by_name("docs/SPEC.md")
        .into_iter()
        .find(|node| node.kind == NodeKind::Document)
        .unwrap();
    assert!(indexer
        .graph()
        .edges_to(&spec_document.id, Some(EdgeKind::Contains))
        .iter()
        .any(|edge| edge.source.kind == NodeKind::Package));
    assert!(indexer
        .graph()
        .edges_to(&spec.id, Some(EdgeKind::Contains))
        .iter()
        .any(|edge| edge.source == spec_document.id));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolves_intent_edges_from_markdown_docs() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() {}\npub fn alpha_test() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/SPEC.md"),
        "# Behavior `alpha`\nRun `alpha_test`\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    let spec = indexer
        .graph()
        .nodes_by_name("Behavior `alpha`")
        .into_iter()
        .find(|node| node.kind == NodeKind::MarkdownHeading)
        .unwrap();
    let alpha = indexer
        .graph()
        .nodes_by_name("alpha")
        .into_iter()
        .find(|node| node.kind == NodeKind::Function)
        .unwrap();
    let alpha_test = indexer
        .graph()
        .nodes_by_name("alpha_test")
        .into_iter()
        .find(|node| node.kind == NodeKind::Function)
        .unwrap();

    assert!(indexer
        .graph()
        .edges_from(&spec.id, Some(EdgeKind::Specifies))
        .into_iter()
        .any(|edge| edge.target == alpha.id));
    assert!(indexer
        .graph()
        .edges_from(&spec.id, Some(EdgeKind::Validates))
        .into_iter()
        .any(|edge| edge.target == alpha_test.id));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn emits_reanchored_change_for_symbol_rename() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    fs::write(
        root.join("src/lib.rs"),
        "fn renamed_alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let changes = indexer.index_with_changes().unwrap();

    assert!(changes.contains(&GraphChange::Reanchored {
        old: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        new: NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function),
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn emits_reanchored_changes_for_file_move_with_same_content() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/feature.rs"),
        "pub fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    fs::rename(root.join("src/feature.rs"), root.join("src/renamed.rs")).unwrap();

    let changes = indexer.index_with_changes().unwrap();

    assert!(changes.contains(&GraphChange::Reanchored {
        old: NodeId::new("demo", "demo::feature", NodeKind::Module),
        new: NodeId::new("demo", "demo::renamed", NodeKind::Module),
    }));
    assert!(changes.contains(&GraphChange::Reanchored {
        old: NodeId::new("demo", "demo::feature::alpha", NodeKind::Function),
        new: NodeId::new("demo", "demo::renamed::alpha", NodeKind::Function),
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn fs_watch_refreshes_session_after_external_edit() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
    )
    .unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([root.join("src/lib.rs")]);
    let observed = session.refresh_fs().unwrap();
    assert!(!observed.is_empty());

    assert!(session
        .prism()
        .symbol("gamma")
        .iter()
        .any(|symbol| symbol.id().path == "demo::gamma"));
    let patch_events = session
        .prism()
        .outcome_memory()
        .outcomes_for(
            &[AnchorRef::Node(NodeId::new(
                "demo",
                "demo::gamma",
                NodeKind::Function,
            ))],
            10,
        )
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .count();
    assert_eq!(patch_events, 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn fs_watch_refresh_enqueues_curator_with_patch_outcomes_and_projection_context() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { helper(); }\nfn helper() {}\nfn beta() {}\n",
    )
    .unwrap();

    #[derive(Clone, Debug, PartialEq)]
    struct CapturedCuratorRun {
        trigger: prism_curator::CuratorTrigger,
        focus: Vec<AnchorRef>,
        outcome_kinds: Vec<OutcomeKind>,
        co_change_count: usize,
    }

    #[derive(Clone, Default)]
    struct FakeCurator {
        seen: Arc<Mutex<Vec<CapturedCuratorRun>>>,
    }

    impl CuratorBackend for FakeCurator {
        fn run(&self, job: &CuratorJob, ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            self.seen.lock().unwrap().push(CapturedCuratorRun {
                trigger: job.trigger.clone(),
                focus: job.focus.clone(),
                outcome_kinds: ctx
                    .outcomes
                    .iter()
                    .map(|event| event.kind.clone())
                    .collect(),
                co_change_count: ctx.projections.co_change.len(),
            });
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: job.focus.clone(),
                    summary: "watcher refresh needs review".into(),
                    severity: "medium".into(),
                    evidence_events: ctx
                        .outcomes
                        .iter()
                        .map(|event| event.meta.id.clone())
                        .collect(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let backend = FakeCurator::default();
    let session = index_workspace_session_with_curator(&root, Arc::new(backend.clone())).unwrap();
    let initial_runs = backend.seen.lock().unwrap().len();
    fs::write(
        root.join("src/lib.rs"),
        "fn gamma() { delta(); }\nfn delta() {}\nfn beta() {}\n",
    )
    .unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([root.join("src/lib.rs")]);
    let observed = session.refresh_fs().unwrap();
    assert!(!observed.is_empty());

    let mut gamma = None;
    let mut delta = None;
    let mut completed = false;
    for _ in 0..60 {
        let prism = session.prism();
        gamma = prism
            .symbol("gamma")
            .into_iter()
            .find(|symbol| symbol.id().path == "demo::gamma")
            .map(|symbol| symbol.id().clone());
        delta = prism
            .symbol("delta")
            .into_iter()
            .find(|symbol| symbol.id().path == "demo::delta")
            .map(|symbol| symbol.id().clone());
        completed = session
            .curator_snapshot()
            .unwrap()
            .records
            .iter()
            .any(|record| {
                record.status == prism_curator::CuratorJobStatus::Completed
                    && record.job.trigger == prism_curator::CuratorTrigger::HotspotChanged
            });
        if gamma.is_some() && delta.is_some() && completed {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let gamma = gamma.expect("watcher refresh should index gamma");
    let delta = delta.expect("watcher refresh should index delta");
    assert!(completed);

    let patch_events = session
        .prism()
        .outcome_memory()
        .outcomes_for(&[AnchorRef::Node(gamma.clone())], 10)
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .count();
    assert_eq!(patch_events, 1);

    let neighbors = session.prism().co_change_neighbors(&gamma, 8);
    assert!(neighbors
        .iter()
        .any(|neighbor| neighbor.nodes.iter().any(|node| node.path == delta.path)));

    let seen = backend.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), initial_runs + 1);
    let captured = seen.last().unwrap();
    assert_eq!(
        captured.trigger,
        prism_curator::CuratorTrigger::HotspotChanged
    );
    assert!(captured
        .focus
        .iter()
        .any(|anchor| matches!(anchor, AnchorRef::Node(node) if node.path == "demo::gamma")));
    assert!(captured.outcome_kinds.contains(&OutcomeKind::PatchApplied));
    assert!(captured.co_change_count > 0);

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let snapshot = reloaded.curator_snapshot().unwrap();
    assert!(snapshot.records.iter().any(|record| {
        record.job.trigger == prism_curator::CuratorTrigger::HotspotChanged
            && matches!(
                record.run.as_ref().and_then(|run| run.proposals.first()),
                Some(CuratorProposal::RiskSummary(summary))
                    if summary.summary == "watcher refresh needs review"
            )
    }));

    let reloaded_gamma = reloaded
        .prism()
        .symbol("gamma")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::gamma")
        .expect("gamma should survive reload")
        .id()
        .clone();
    let reloaded_neighbors = reloaded.prism().co_change_neighbors(&reloaded_gamma, 8);
    assert!(reloaded_neighbors
        .iter()
        .any(|neighbor| neighbor.nodes.iter().any(|node| node.path == "demo::delta")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reload_preserves_lineage_patch_outcomes_memory_and_projections_after_rename() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();

    let mut note = MemoryEntry::new(MemoryKind::Episodic, "alpha previously regressed");
    note.anchors = vec![AnchorRef::Node(alpha.clone())];
    session
        .persist_episodic(&EpisodicMemorySnapshot {
            entries: vec![note],
        })
        .unwrap();
    session.flush_materializations().unwrap();

    fs::write(
        root.join("src/lib.rs"),
        "fn renamed_alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let observed = session.refresh_fs().unwrap();
    assert!(observed.iter().any(|change| {
        let saw_updated_rename = change.updated.iter().any(|(before, after)| {
            before.node.id.path == "demo::alpha" && after.node.id.path == "demo::renamed_alpha"
        });
        let saw_split_add_remove = change
            .removed
            .iter()
            .any(|node| node.node.id.path == "demo::alpha")
            && change
                .added
                .iter()
                .any(|node| node.node.id.path == "demo::renamed_alpha");
        saw_updated_rename || saw_split_add_remove
    }));

    let renamed_alpha = session
        .prism()
        .symbol("renamed_alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::renamed_alpha")
        .expect("renamed alpha should be indexed after refresh")
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:renamed-alpha:test"),
                ts: 20,
                actor: EventActor::User,
                correlation: Some(TaskId::new("task:renamed-alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(renamed_alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "renamed alpha needs integration coverage".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "renamed_alpha_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    session.flush_materializations().unwrap();

    let reloaded = index_workspace_session(&root).unwrap();
    let reloaded_prism = reloaded.prism();
    let renamed_alpha = reloaded_prism
        .symbol("renamed_alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::renamed_alpha")
        .expect("renamed alpha should survive reload")
        .id()
        .clone();

    assert!(reloaded_prism
        .symbol("alpha")
        .into_iter()
        .all(|symbol| symbol.id().path != "demo::alpha"));

    let lineage = reloaded_prism
        .lineage_of(&renamed_alpha)
        .expect("renamed alpha should keep a lineage");
    let history = reloaded_prism.lineage_history(&lineage);
    assert!(history.iter().any(|event| {
        event.kind == LineageEventKind::Renamed
            && event.before.iter().any(|node| node.path == "demo::alpha")
            && event
                .after
                .iter()
                .any(|node| node.path == "demo::renamed_alpha")
    }));

    let patch_events = reloaded_prism
        .outcome_memory()
        .outcomes_for(&[AnchorRef::Node(renamed_alpha.clone())], 10)
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .collect::<Vec<_>>();
    assert_eq!(patch_events.len(), 1);

    let snapshot = reloaded
        .load_episodic_snapshot()
        .unwrap()
        .expect("reanchored note should persist");
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.content == "alpha previously regressed")
        .expect("reanchored note should be present");
    assert!(entry
        .anchors
        .contains(&AnchorRef::Node(renamed_alpha.clone())));
    assert!(entry.anchors.contains(&AnchorRef::Lineage(lineage.clone())));
    assert!(!entry.anchors.contains(&AnchorRef::Node(alpha.clone())));

    let recipe = reloaded_prism.validation_recipe(&renamed_alpha);
    assert!(recipe
        .scored_checks
        .iter()
        .any(|check| check.label == "test:renamed_alpha_integration" && check.score > 0.0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reload_bounds_hot_outcomes_but_queries_cold_outcomes_from_store() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();

    for idx in 0..(crate::session::HOT_OUTCOME_HYDRATION_LIMIT + 32) {
        session
            .prism()
            .outcome_memory()
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(format!("outcome:cold:{idx}")),
                    ts: u64::try_from(idx + 1).unwrap(),
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: if idx == 0 {
                    OutcomeKind::FailureObserved
                } else {
                    OutcomeKind::NoteAdded
                },
                result: if idx == 0 {
                    OutcomeResult::Failure
                } else {
                    OutcomeResult::Success
                },
                summary: format!("event {idx}"),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();
    }
    session
        .store
        .lock()
        .unwrap()
        .save_outcome_snapshot(&session.prism().outcome_snapshot())
        .unwrap();

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    assert!(
        reloaded.prism().outcome_snapshot().events.len()
            <= crate::session::HOT_OUTCOME_HYDRATION_LIMIT
    );

    let failures = reloaded.prism().query_outcomes(&OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha)],
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        limit: 10,
        ..OutcomeRecallQuery::default()
    });
    assert!(failures
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:cold:0")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn persist_outcomes_flushes_checkpoint_materialization() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();
    session
        .prism()
        .outcome_memory()
        .store_event(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:checkpoint:test"),
                ts: 33,
                actor: EventActor::Agent,
                correlation: None,
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "checkpointed outcome".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    session.persist_outcomes().unwrap();
    session.flush_materializations().unwrap();

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let events = reloaded.prism().query_outcomes(&OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha)],
        limit: 10,
        ..OutcomeRecallQuery::default()
    });
    assert!(events
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:checkpoint:test")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_session_load_methods_prefer_hot_outcomes_over_unpersisted_store_state() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();
    let task_id = TaskId::new("task:hot-session");
    let event_id = EventId::new("outcome:hot-session");
    let event = OutcomeEvent {
        meta: EventMeta {
            id: event_id.clone(),
            ts: 1,
            actor: EventActor::Agent,
            correlation: Some(task_id.clone()),
            causation: None,
        },
        anchors: vec![AnchorRef::Node(alpha.clone())],
        kind: OutcomeKind::FailureObserved,
        result: OutcomeResult::Failure,
        summary: "hot only failure".into(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    session
        .prism()
        .outcome_memory()
        .store_event(event.clone())
        .unwrap();

    let replay = session.load_task_replay(&task_id).unwrap();
    assert_eq!(replay.task, task_id);
    assert_eq!(replay.events, vec![event.clone()]);

    let loaded = session
        .load_outcomes(&OutcomeRecallQuery {
            anchors: vec![AnchorRef::Node(alpha)],
            kinds: Some(vec![OutcomeKind::FailureObserved]),
            result: Some(OutcomeResult::Failure),
            limit: 10,
            ..OutcomeRecallQuery::default()
        })
        .unwrap();
    assert_eq!(loaded, vec![event.clone()]);

    assert_eq!(session.load_outcome_event(&event_id).unwrap(), Some(event));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reload_queries_cold_lineage_history_from_store() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();
    let lineage = session
        .prism()
        .lineage_of(&alpha)
        .expect("alpha should have a lineage");

    let mut persisted_history = session.prism().history_snapshot();
    let persisted_event = LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:lineage:cold"),
            ts: 11,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
        },
        lineage: lineage.clone(),
        kind: prism_ir::LineageEventKind::Updated,
        before: vec![alpha.clone()],
        after: vec![alpha.clone()],
        confidence: 0.9,
        evidence: vec![prism_ir::LineageEvidence::ExactNodeId],
    };
    persisted_history.events = vec![persisted_event.clone()];
    session
        .store
        .lock()
        .unwrap()
        .save_history_snapshot(&persisted_history)
        .unwrap();
    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let events = reloaded.prism().lineage_history(&lineage);
    assert_eq!(events, vec![persisted_event]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_memory_events_round_trip_through_committed_jsonl_and_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "alpha ownership is shared memory");
    entry.id = MemoryId("structural:repo-test".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Repo;
    entry.source = MemorySource::User;
    entry.trust = 0.9;
    entry.metadata = json!({
        "provenance": {
            "origin": "test",
            "kind": "repo_memory_round_trip",
        },
        "publication": {
            "publishedAt": 17,
            "lastReviewedAt": 17,
            "status": "active",
        }
    });
    session
        .append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            entry.clone(),
            Some("task:repo-memory".to_string()),
            vec![MemoryId("memory:source".to_string())],
            Vec::new(),
        ))
        .unwrap();

    let repo_log = root.join(".prism").join("memory").join("events.jsonl");
    assert!(repo_log.exists());

    let reloaded = index_workspace_session(&root).unwrap();
    let snapshot = reloaded
        .load_episodic_snapshot()
        .unwrap()
        .expect("repo memory should reload");
    assert!(snapshot.entries.iter().any(|candidate| {
        candidate.id == entry.id
            && candidate.scope == MemoryScope::Repo
            && candidate.content == "alpha ownership is shared memory"
    }));

    let events = reloaded
        .memory_events(&MemoryEventQuery {
            memory_id: Some(MemoryId("structural:repo-test".to_string())),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Promoted]),
            scope: Some(MemoryScope::Repo),
            task_id: Some("task:repo-memory".to_string()),
            since: None,
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].promoted_from,
        vec![MemoryId("memory:source".to_string())]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_concept_events_round_trip_through_committed_jsonl_and_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();
    let beta = session
        .prism()
        .symbol("beta")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::beta")
        .expect("beta should be indexed")
        .id()
        .clone();
    session
        .append_concept_event(ConceptEvent {
            id: "concept-event:repo-test".to_string(),
            recorded_at: 17,
            task_id: Some("task:repo-concept".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://alpha_flow".to_string(),
                canonical_name: "alpha_flow".to_string(),
                summary: "Curated alpha concept shared through the repo.".to_string(),
                aliases: vec!["alpha".to_string(), "alpha flow".to_string()],
                confidence: 0.93,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: vec![
                    session.prism().lineage_of(&alpha),
                    session.prism().lineage_of(&beta),
                ],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Promoted from repo task work.".to_string()],
                risk_hint: Some("Alpha changes tend to need a quick smoke test.".to_string()),
                decode_lenses: vec![ConceptDecodeLens::Open, ConceptDecodeLens::Workset],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "repo_concept_round_trip".to_string(),
                    task_id: Some("task:repo-concept".to_string()),
                },
                publication: Some(ConceptPublication {
                    published_at: 17,
                    last_reviewed_at: Some(17),
                    status: ConceptPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    let repo_log = root.join(".prism").join("concepts").join("events.jsonl");
    assert!(repo_log.exists());

    let reloaded = index_workspace_session(&root).unwrap();
    let concept = reloaded
        .prism()
        .concept_by_handle("concept://alpha_flow")
        .expect("repo concept should reload");
    assert_eq!(
        concept.summary,
        "Curated alpha concept shared through the repo."
    );
    assert_eq!(
        concept.aliases,
        vec!["alpha".to_string(), "alpha flow".to_string()]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn shared_runtime_sqlite_shares_session_memory_and_concepts_across_workspaces() {
    let shared_runtime_root = temp_workspace();
    let shared_runtime_sqlite = shared_runtime_root.join("shared-runtime.db");
    let root_one = temp_workspace();
    let root_two = temp_workspace();
    for root in [&root_one, &root_two] {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();
    }

    let options = WorkspaceSessionOptions {
        coordination: true,
        shared_runtime: SharedRuntimeBackend::Sqlite {
            path: shared_runtime_sqlite.clone(),
        },
        hydrate_persisted_projections: false,
    };
    let session_one = index_workspace_session_with_options(&root_one, options.clone()).unwrap();
    let alpha = session_one
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "shared session memory");
    entry.id = MemoryId("memory:shared-session".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha.clone())];
    entry.scope = MemoryScope::Session;
    entry.source = MemorySource::User;
    entry.trust = 0.9;
    session_one
        .append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Stored,
            entry.clone(),
            Some("task:shared-runtime".to_string()),
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();
    session_one
        .persist_episodic(&EpisodicMemorySnapshot {
            entries: vec![entry.clone()],
        })
        .unwrap();

    session_one
        .append_concept_event(ConceptEvent {
            id: "concept-event:shared-runtime".to_string(),
            recorded_at: 23,
            task_id: Some("task:shared-runtime".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://shared_alpha".to_string(),
                canonical_name: "shared_alpha".to_string(),
                summary: "Session concept persisted through the shared runtime sqlite.".to_string(),
                aliases: vec!["shared alpha".to_string()],
                confidence: 0.87,
                core_members: vec![alpha.clone()],
                core_member_lineages: vec![session_one.prism().lineage_of(&alpha)],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Session-scoped concept".to_string()],
                risk_hint: None,
                decode_lenses: vec![ConceptDecodeLens::Open],
                scope: ConceptScope::Session,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "shared_runtime_sqlite".to_string(),
                    task_id: Some("task:shared-runtime".to_string()),
                },
                publication: None,
            },
        })
        .unwrap();
    drop(session_one);

    let session_two = index_workspace_session_with_options(&root_two, options).unwrap();
    let snapshot = session_two
        .load_episodic_snapshot()
        .unwrap()
        .expect("shared session memory should reload");
    assert!(snapshot
        .entries
        .iter()
        .any(|candidate| candidate.id == entry.id));

    let concept = session_two
        .prism()
        .concept_by_handle("concept://shared_alpha")
        .expect("shared concept should reload");
    assert_eq!(concept.scope, ConceptScope::Session);
    assert_eq!(
        concept.summary,
        "Session concept persisted through the shared runtime sqlite."
    );

    let _ = fs::remove_dir_all(root_one);
    let _ = fs::remove_dir_all(root_two);
    let _ = fs::remove_dir_all(shared_runtime_root);
}

#[test]
fn repo_concept_event_patch_trace_round_trips_through_jsonl() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();
    let beta = session
        .prism()
        .symbol("beta")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::beta")
        .expect("beta should be indexed")
        .id()
        .clone();
    session
        .append_concept_event(ConceptEvent {
            id: "concept-event:repo-patch".to_string(),
            recorded_at: 19,
            task_id: Some("task:repo-concept-patch".to_string()),
            action: ConceptEventAction::Update,
            patch: Some(ConceptEventPatch {
                set_fields: vec!["summary".to_string()],
                cleared_fields: vec!["riskHint".to_string()],
                summary: Some("Updated alpha concept with cleared risk guidance.".to_string()),
                ..ConceptEventPatch::default()
            }),
            concept: ConceptPacket {
                handle: "concept://alpha_flow".to_string(),
                canonical_name: "alpha_flow".to_string(),
                summary: "Updated alpha concept with cleared risk guidance.".to_string(),
                aliases: vec!["alpha".to_string()],
                confidence: 0.91,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: vec![
                    session.prism().lineage_of(&alpha),
                    session.prism().lineage_of(&beta),
                ],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Updated from repo task work.".to_string()],
                risk_hint: None,
                decode_lenses: vec![ConceptDecodeLens::Open],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "repo_concept_patch_round_trip".to_string(),
                    task_id: Some("task:repo-concept-patch".to_string()),
                },
                publication: Some(ConceptPublication {
                    published_at: 19,
                    last_reviewed_at: Some(19),
                    status: ConceptPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    let events = crate::concept_events::load_repo_concept_events(&root).unwrap();
    assert_eq!(events.len(), 1);
    let patch = events[0]
        .patch
        .as_ref()
        .expect("patch trace should persist");
    assert_eq!(patch.set_fields, vec!["summary".to_string()]);
    assert_eq!(patch.cleared_fields, vec!["riskHint".to_string()]);
    assert_eq!(
        patch.summary.as_deref(),
        Some("Updated alpha concept with cleared risk guidance.")
    );
    assert_eq!(patch.risk_hint, None);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_concept_events_auto_sync_prism_doc() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { beta(); gamma(); }\npub fn beta() {}\npub fn gamma() {}\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();
    let beta = session
        .prism()
        .symbol("beta")
        .into_iter()
        .next()
        .expect("beta should be indexed")
        .id()
        .clone();
    let gamma = session
        .prism()
        .symbol("gamma")
        .into_iter()
        .next()
        .expect("gamma should be indexed")
        .id()
        .clone();

    session
        .append_concept_event(ConceptEvent {
            id: "concept-event:repo-prism-doc".to_string(),
            recorded_at: 31,
            task_id: Some("task:repo-prism-doc".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://alpha_flow".to_string(),
                canonical_name: "alpha_flow".to_string(),
                summary: "Explains how alpha delegates work into beta.".to_string(),
                aliases: vec!["alpha flow".to_string()],
                confidence: 0.92,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: Vec::new(),
                supporting_members: vec![gamma],
                supporting_member_lineages: Vec::new(),
                likely_tests: vec![beta],
                likely_test_lineages: Vec::new(),
                evidence: vec!["Promoted from repo curation.".to_string()],
                risk_hint: Some("Touch beta when changing alpha.".to_string()),
                decode_lenses: vec![ConceptDecodeLens::Open, ConceptDecodeLens::Workset],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "manual".to_string(),
                    kind: "manual_concept".to_string(),
                    task_id: Some("task:repo-prism-doc".to_string()),
                },
                publication: Some(ConceptPublication {
                    published_at: 31,
                    last_reviewed_at: Some(31),
                    status: ConceptPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    let prism_doc = fs::read_to_string(root.join("PRISM.md")).unwrap();
    let concepts_doc = fs::read_to_string(root.join("docs/prism/concepts.md")).unwrap();
    let relations_doc = fs::read_to_string(root.join("docs/prism/relations.md")).unwrap();
    let contracts_doc = fs::read_to_string(root.join("docs/prism/contracts.md")).unwrap();
    assert!(prism_doc.contains("# PRISM"));
    assert!(prism_doc.contains("## How to Read This Repo"));
    assert!(prism_doc.contains("docs/prism/concepts.md"));
    assert!(prism_doc.contains("docs/prism/relations.md"));
    assert!(prism_doc.contains("docs/prism/contracts.md"));
    assert!(prism_doc.contains("- Active repo concepts: 1"));
    assert!(concepts_doc.contains("# PRISM Concepts"));
    assert!(concepts_doc.contains("`alpha_flow` (`concept://alpha_flow`)"));
    assert!(concepts_doc.contains("Explains how alpha delegates work into beta."));
    assert!(concepts_doc.contains("### Core Members"));
    assert!(concepts_doc.contains("demo::alpha"));
    assert!(concepts_doc.contains("### Supporting Members"));
    assert!(concepts_doc.contains("demo::gamma"));
    assert!(concepts_doc.contains("### Risk Hint"));
    assert!(relations_doc.contains("# PRISM Relations"));
    assert!(contracts_doc.contains("# PRISM Contracts"));
    assert!(contracts_doc.contains("No active repo-scoped contracts are currently published."));

    let sync = session.sync_prism_doc().unwrap();
    assert_eq!(sync.status, PrismDocSyncStatus::Unchanged);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_concept_relations_auto_sync_prism_doc() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { beta(); gamma(); }\npub fn beta() {}\npub fn gamma() {}\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();
    let beta = session
        .prism()
        .symbol("beta")
        .into_iter()
        .next()
        .expect("beta should be indexed")
        .id()
        .clone();
    let gamma = session
        .prism()
        .symbol("gamma")
        .into_iter()
        .next()
        .expect("gamma should be indexed")
        .id()
        .clone();

    for (handle, canonical_name, members) in [
        (
            "concept://alpha_flow",
            "alpha_flow",
            vec![alpha, beta.clone()],
        ),
        ("concept://beta_system", "beta_system", vec![beta, gamma]),
    ] {
        session
            .append_concept_event(ConceptEvent {
                id: format!("concept-event:{canonical_name}"),
                recorded_at: 37,
                task_id: Some("task:repo-prism-relations".to_string()),
                action: ConceptEventAction::Promote,
                patch: None,
                concept: ConceptPacket {
                    handle: handle.to_string(),
                    canonical_name: canonical_name.to_string(),
                    summary: format!("Published concept for {canonical_name}."),
                    aliases: Vec::new(),
                    confidence: 0.9,
                    core_members: members,
                    core_member_lineages: Vec::new(),
                    supporting_members: Vec::new(),
                    supporting_member_lineages: Vec::new(),
                    likely_tests: Vec::new(),
                    likely_test_lineages: Vec::new(),
                    evidence: vec![format!("Published concept for {canonical_name}.")],
                    risk_hint: None,
                    decode_lenses: vec![ConceptDecodeLens::Open],
                    scope: ConceptScope::Repo,
                    provenance: ConceptProvenance {
                        origin: "manual".to_string(),
                        kind: "manual_concept".to_string(),
                        task_id: Some("task:repo-prism-relations".to_string()),
                    },
                    publication: Some(ConceptPublication {
                        published_at: 37,
                        last_reviewed_at: Some(37),
                        status: ConceptPublicationStatus::Active,
                        supersedes: Vec::new(),
                        retired_at: None,
                        retirement_reason: None,
                    }),
                },
            })
            .unwrap();
    }

    session
        .append_concept_relation_event(ConceptRelationEvent {
            id: "concept-relation:alpha-beta".to_string(),
            recorded_at: 41,
            task_id: Some("task:repo-prism-relations".to_string()),
            action: ConceptRelationEventAction::Upsert,
            relation: ConceptRelation {
                source_handle: "concept://alpha_flow".to_string(),
                target_handle: "concept://beta_system".to_string(),
                kind: ConceptRelationKind::DependsOn,
                confidence: 0.88,
                evidence: vec!["Observed through repo curation.".to_string()],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "manual".to_string(),
                    kind: "manual_concept_relation".to_string(),
                    task_id: Some("task:repo-prism-relations".to_string()),
                },
            },
        })
        .unwrap();

    let prism_doc = fs::read_to_string(root.join("PRISM.md")).unwrap();
    let relations_doc = fs::read_to_string(root.join("docs/prism/relations.md")).unwrap();
    assert!(prism_doc.contains("- Active repo concepts: 2"));
    assert!(prism_doc.contains("- Active repo relations: 1"));
    assert!(prism_doc.contains("## Generated Docs"));
    assert!(relations_doc.contains("# PRISM Relations"));
    assert!(relations_doc.contains("depends on: `beta_system` (`concept://beta_system`)"));
    assert!(relations_doc.contains("confidence 0.88"));

    let sync = session.sync_prism_doc().unwrap();
    assert_eq!(sync.status, PrismDocSyncStatus::Unchanged);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_contract_events_auto_sync_prism_doc() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();

    session
        .append_contract_event(ContractEvent {
            id: "contract-event:repo-prism-doc".to_string(),
            recorded_at: 43,
            task_id: Some("task:repo-contract-prism-doc".to_string()),
            action: ContractEventAction::Promote,
            patch: None,
            contract: ContractPacket {
                handle: "contract://alpha_api".to_string(),
                name: "alpha_api".to_string(),
                summary: "Preserves a stable callable surface for alpha consumers.".to_string(),
                aliases: vec!["alpha api".to_string()],
                kind: ContractKind::Interface,
                subject: ContractTarget {
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    concept_handles: vec!["concept://alpha_flow".to_string()],
                },
                guarantees: vec![ContractGuarantee {
                    id: "alpha_name_stable".to_string(),
                    statement: "Internal callers may rely on the alpha function name.".to_string(),
                    scope: Some("internal callers".to_string()),
                    strength: Some(prism_query::ContractGuaranteeStrength::Hard),
                    evidence_refs: vec!["validation:test-alpha".to_string()],
                }],
                assumptions: vec!["The alpha surface remains internal-only.".to_string()],
                consumers: vec![ContractTarget {
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    concept_handles: vec!["concept://alpha_flow".to_string()],
                }],
                validations: vec![prism_query::ContractValidation {
                    id: "alpha-smoke".to_string(),
                    summary: Some("Run the alpha smoke path after interface changes.".to_string()),
                    anchors: vec![AnchorRef::Node(alpha)],
                }],
                stability: prism_query::ContractStability::Internal,
                compatibility: ContractCompatibility {
                    additive: vec!["Adding optional behavior is safe.".to_string()],
                    breaking: vec!["Renaming alpha is breaking.".to_string()],
                    ..ContractCompatibility::default()
                },
                evidence: vec!["Promoted from repo curation.".to_string()],
                status: ContractStatus::Active,
                scope: prism_query::ContractScope::Repo,
                provenance: prism_query::ContractProvenance {
                    origin: "manual".to_string(),
                    kind: "manual_contract".to_string(),
                    task_id: Some("task:repo-contract-prism-doc".to_string()),
                },
                publication: Some(prism_query::ContractPublication {
                    published_at: 43,
                    last_reviewed_at: Some(43),
                    status: prism_query::ContractPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    let prism_doc = fs::read_to_string(root.join("PRISM.md")).unwrap();
    let contracts_doc = fs::read_to_string(root.join("docs/prism/contracts.md")).unwrap();
    assert!(prism_doc.contains("- Active repo contracts: 1"));
    assert!(prism_doc.contains("docs/prism/contracts.md"));
    assert!(contracts_doc.contains("# PRISM Contracts"));
    assert!(contracts_doc.contains("`alpha_api` (`contract://alpha_api`)"));
    assert!(contracts_doc.contains("Preserves a stable callable surface for alpha consumers."));
    assert!(contracts_doc.contains("Kind: interface"));
    assert!(contracts_doc.contains("Status: active"));
    assert!(contracts_doc.contains("Stability: internal"));
    assert!(contracts_doc.contains("### Subject"));
    assert!(contracts_doc.contains("node:demo:demo::alpha:function"));
    assert!(contracts_doc.contains("`concept://alpha_flow`"));
    assert!(contracts_doc.contains("### Guarantees"));
    assert!(contracts_doc.contains("alpha_name_stable"));
    assert!(contracts_doc.contains("validation:test-alpha"));
    assert!(contracts_doc.contains("### Assumptions"));
    assert!(contracts_doc.contains("The alpha surface remains internal-only."));
    assert!(contracts_doc.contains("### Consumers"));
    assert!(contracts_doc.contains("### Validations"));
    assert!(contracts_doc.contains("alpha-smoke"));
    assert!(contracts_doc.contains("### Compatibility"));
    assert!(contracts_doc.contains("Renaming alpha is breaking."));
    assert!(contracts_doc.contains("### Evidence"));
    assert!(contracts_doc.contains("Promoted from repo curation."));

    let sync = session.sync_prism_doc().unwrap();
    assert_eq!(sync.status, PrismDocSyncStatus::Unchanged);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_contract_events_round_trip_through_committed_jsonl_and_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();

    session
        .append_contract_event(ContractEvent {
            id: "contract-event:repo-test".to_string(),
            recorded_at: 29,
            task_id: Some("task:repo-contract".to_string()),
            action: ContractEventAction::Promote,
            patch: None,
            contract: ContractPacket {
                handle: "contract://alpha_api".to_string(),
                name: "alpha_api".to_string(),
                summary:
                    "The alpha surface preserves a stable callable contract for internal users."
                        .to_string(),
                aliases: vec!["alpha api".to_string()],
                kind: ContractKind::Interface,
                subject: ContractTarget {
                    anchors: vec![AnchorRef::Node(alpha)],
                    concept_handles: Vec::new(),
                },
                guarantees: vec![ContractGuarantee {
                    id: "alpha_name_stable".to_string(),
                    statement: "Internal callers may rely on the alpha function name.".to_string(),
                    scope: Some("internal callers".to_string()),
                    strength: None,
                    evidence_refs: vec!["validation:test-alpha".to_string()],
                }],
                assumptions: vec!["The surface remains internal-only.".to_string()],
                consumers: Vec::new(),
                validations: Vec::new(),
                stability: prism_query::ContractStability::Internal,
                compatibility: ContractCompatibility {
                    breaking: vec!["Renaming alpha is breaking.".to_string()],
                    ..ContractCompatibility::default()
                },
                evidence: vec!["Promoted from repo task work.".to_string()],
                status: ContractStatus::Active,
                scope: prism_query::ContractScope::Repo,
                provenance: prism_query::ContractProvenance {
                    origin: "test".to_string(),
                    kind: "repo_contract_round_trip".to_string(),
                    task_id: Some("task:repo-contract".to_string()),
                },
                publication: Some(prism_query::ContractPublication {
                    published_at: 29,
                    last_reviewed_at: Some(29),
                    status: prism_query::ContractPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    let repo_log = root.join(".prism").join("contracts").join("events.jsonl");
    assert!(repo_log.exists());

    let reloaded = index_workspace_session(&root).unwrap();
    let contract = reloaded
        .prism()
        .contract_by_handle("contract://alpha_api")
        .expect("repo contract should reload");
    assert_eq!(contract.kind, ContractKind::Interface);
    assert_eq!(contract.guarantees.len(), 1);
    assert_eq!(contract.status, ContractStatus::Active);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_concepts_rebind_members_through_lineage_after_rename_and_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let prism = session.prism();
    let alpha = prism
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();
    let beta = prism
        .symbol("beta")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::beta")
        .expect("beta should be indexed")
        .id()
        .clone();
    let alpha_lineage = prism
        .lineage_of(&alpha)
        .expect("alpha should have a lineage before rename");
    let beta_lineage = prism
        .lineage_of(&beta)
        .expect("beta should have a lineage before rename");
    drop(prism);

    session
        .append_concept_event(ConceptEvent {
            id: "concept-event:repo-rebind".to_string(),
            recorded_at: 21,
            task_id: Some("task:repo-concept-rebind".to_string()),
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://alpha_flow".to_string(),
                canonical_name: "alpha_flow".to_string(),
                summary: "Curated alpha concept shared through the repo.".to_string(),
                aliases: vec!["alpha".to_string(), "alpha flow".to_string()],
                confidence: 0.93,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: vec![Some(alpha_lineage.clone()), Some(beta_lineage.clone())],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Promoted from repo task work.".to_string()],
                risk_hint: Some("Alpha changes tend to need a quick smoke test.".to_string()),
                decode_lenses: vec![ConceptDecodeLens::Open, ConceptDecodeLens::Workset],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "repo_concept_rebind".to_string(),
                    task_id: Some("task:repo-concept-rebind".to_string()),
                },
                publication: Some(ConceptPublication {
                    published_at: 21,
                    last_reviewed_at: Some(21),
                    status: ConceptPublicationStatus::Active,
                    supersedes: Vec::new(),
                    retired_at: None,
                    retirement_reason: None,
                }),
            },
        })
        .unwrap();

    fs::write(
        root.join("src/lib.rs"),
        "fn renamed_alpha() {}\nfn beta() {}\n",
    )
    .unwrap();

    let observed = session.refresh_fs().unwrap();
    assert!(observed.iter().any(|change| {
        let saw_updated_rename = change.updated.iter().any(|(before, after)| {
            before.node.id.path == "demo::alpha" && after.node.id.path == "demo::renamed_alpha"
        });
        let saw_split_add_remove = change
            .removed
            .iter()
            .any(|node| node.node.id.path == "demo::alpha")
            && change
                .added
                .iter()
                .any(|node| node.node.id.path == "demo::renamed_alpha");
        saw_updated_rename || saw_split_add_remove
    }));

    let concept_after_refresh = session
        .prism()
        .concept_by_handle("concept://alpha_flow")
        .expect("repo concept should stay available after refresh");
    assert!(concept_after_refresh
        .core_members
        .iter()
        .any(|node| node.path == "demo::renamed_alpha"));
    assert!(!concept_after_refresh
        .core_members
        .iter()
        .any(|node| node.path == "demo::alpha"));

    let reloaded = index_workspace_session(&root).unwrap();
    let reloaded_concept = reloaded
        .prism()
        .concept_by_handle("concept://alpha_flow")
        .expect("repo concept should reload after rename");
    assert!(reloaded_concept
        .core_members
        .iter()
        .any(|node| node.path == "demo::renamed_alpha"));
    assert!(!reloaded_concept
        .core_members
        .iter()
        .any(|node| node.path == "demo::alpha"));
    assert_eq!(
        reloaded_concept
            .core_member_lineages
            .first()
            .cloned()
            .flatten(),
        Some(alpha_lineage)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reload_preserves_coordination_claim_resolution_through_rename() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();

    let (plan_id, task_id, holder) = session
        .mutate_coordination(|prism| {
            let scoped_anchors =
                prism.coordination_scope_anchors(&[AnchorRef::Node(alpha.clone())]);
            let base_revision = prism.workspace_revision();
            let lineage = prism
                .lineage_of(&alpha)
                .expect("alpha should have a lineage before rename");
            assert!(scoped_anchors.contains(&AnchorRef::Lineage(lineage)));

            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:rename-plan"),
                    ts: 1,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:coordination-rename")),
                    causation: None,
                },
                "Coordinate rename follow-up".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:rename-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:coordination-rename")),
                    causation: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Rename alpha safely".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(prism_ir::SessionId::new("session:rename-owner")),
                    worktree_id: None,
                    branch_ref: None,
                    anchors: scoped_anchors.clone(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: base_revision.clone(),
                },
            )?;
            let task_id = task.id.clone();
            let holder = prism_ir::SessionId::new("session:rename-owner");
            prism.acquire_native_claim(
                EventMeta {
                    id: EventId::new("coordination:rename-claim"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:coordination-rename")),
                    causation: None,
                },
                holder.clone(),
                prism_coordination::ClaimAcquireInput {
                    task_id: Some(task_id.clone()),
                    anchors: scoped_anchors,
                    capability: prism_ir::Capability::Edit,
                    mode: Some(prism_ir::ClaimMode::HardExclusive),
                    ttl_seconds: Some(300),
                    base_revision: base_revision.clone(),
                    current_revision: base_revision,
                    agent: None,
                    worktree_id: None,
                    branch_ref: None,
                },
            )?;
            Ok((plan_id, task_id, holder))
        })
        .unwrap();

    fs::write(
        root.join("src/lib.rs"),
        "fn renamed_alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let observed = session.refresh_fs().unwrap();
    assert!(observed.iter().any(|change| {
        let saw_updated_rename = change.updated.iter().any(|(before, after)| {
            before.node.id.path == "demo::alpha" && after.node.id.path == "demo::renamed_alpha"
        });
        let saw_split_add_remove = change
            .removed
            .iter()
            .any(|node| node.node.id.path == "demo::alpha")
            && change
                .added
                .iter()
                .any(|node| node.node.id.path == "demo::renamed_alpha");
        saw_updated_rename || saw_split_add_remove
    }));
    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let reloaded_prism = reloaded.prism();
    let renamed_alpha = reloaded_prism
        .symbol("renamed_alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::renamed_alpha")
        .expect("renamed alpha should survive reload")
        .id()
        .clone();
    let lineage = reloaded_prism
        .lineage_of(&renamed_alpha)
        .expect("renamed alpha should keep its lineage");

    let task = reloaded_prism
        .coordination_task(&task_id)
        .expect("coordination task should persist across reload");
    assert_eq!(task.plan, plan_id);
    assert!(task.anchors.contains(&AnchorRef::Lineage(lineage.clone())));

    let claims = reloaded_prism.claims(&[AnchorRef::Node(renamed_alpha.clone())], 10);
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].holder, holder);
    assert_eq!(claims[0].task.as_ref(), Some(&task_id));
    assert!(claims[0]
        .anchors
        .contains(&AnchorRef::Lineage(lineage.clone())));

    let conflicts = reloaded_prism.simulate_claim(
        &prism_ir::SessionId::new("session:rename-contender"),
        &[AnchorRef::Node(renamed_alpha.clone())],
        prism_ir::Capability::Edit,
        Some(prism_ir::ClaimMode::HardExclusive),
        None,
        10,
    );
    assert!(conflicts.iter().any(|conflict| {
        conflict.severity == prism_ir::ConflictSeverity::Block
            && conflict.overlap_kinds.iter().any(|kind| {
                matches!(
                    kind,
                    prism_ir::ConflictOverlapKind::Node
                        | prism_ir::ConflictOverlapKind::Lineage
                        | prism_ir::ConflictOverlapKind::File
                )
            })
    }));

    let snapshot = reloaded
        .load_coordination_snapshot()
        .unwrap()
        .expect("coordination snapshot should persist");
    assert!(snapshot.tasks.iter().any(|persisted| {
        persisted.id == task_id
            && persisted
                .anchors
                .contains(&AnchorRef::Lineage(lineage.clone()))
    }));
    assert!(snapshot.claims.iter().any(|persisted| {
        persisted.task.as_ref() == Some(&task_id)
            && persisted.holder == holder
            && persisted
                .anchors
                .contains(&AnchorRef::Lineage(lineage.clone()))
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reloaded_native_plan_bindings_hydrate_through_lineage_without_republishing_runtime_anchors() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let prism = session.prism();
    let alpha = prism
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();
    let alpha_lineage = prism
        .lineage_of(&alpha)
        .expect("alpha should have a lineage before rename");
    drop(prism);

    let (plan_id, node_id) = session
        .mutate_coordination(|prism| {
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:binding-hydration-plan"),
                    ts: 1,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:binding-hydration")),
                    causation: None,
                },
                "Reload native bindings".into(),
                None,
                Some(Default::default()),
            )?;
            let node_id = prism.create_native_plan_node(
                &plan_id,
                prism_ir::PlanNodeKind::Edit,
                "Rename alpha".into(),
                None,
                Some(prism_ir::PlanNodeStatus::Ready),
                None,
                false,
                prism_ir::PlanBinding {
                    anchors: vec![AnchorRef::Node(alpha.clone())],
                    concept_handles: Vec::new(),
                    artifact_refs: Vec::new(),
                    memory_refs: Vec::new(),
                    outcome_refs: Vec::new(),
                },
                Vec::new(),
                Vec::new(),
                Vec::new(),
                prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                None,
                Vec::new(),
            )?;
            Ok((plan_id, node_id))
        })
        .unwrap();

    fs::write(root.join("src/lib.rs"), "fn renamed_alpha() {}\n").unwrap();
    session.refresh_fs().unwrap();
    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let renamed_alpha = reloaded
        .prism()
        .symbol("renamed_alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::renamed_alpha")
        .expect("renamed alpha should be indexed after reload")
        .id()
        .clone();
    let runtime_graph = reloaded
        .prism()
        .plan_graph(&plan_id)
        .expect("runtime graph should reload");
    let runtime_node = runtime_graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .expect("runtime node should reload");
    assert!(runtime_node
        .bindings
        .anchors
        .contains(&AnchorRef::Node(renamed_alpha.clone())));
    assert!(runtime_node
        .bindings
        .anchors
        .contains(&AnchorRef::Lineage(alpha_lineage.clone())));

    reloaded.persist_current_coordination().unwrap();
    let raw_state = reloaded
        .load_coordination_plan_state()
        .unwrap()
        .expect("raw plan state should remain persisted");
    let raw_node = raw_state
        .plan_graphs
        .iter()
        .find(|graph| graph.id == plan_id)
        .and_then(|graph| graph.nodes.iter().find(|node| node.id == node_id))
        .expect("persisted node should exist");
    assert!(raw_node.bindings.anchors.contains(&AnchorRef::Node(alpha)));
    assert!(raw_node
        .bindings
        .anchors
        .contains(&AnchorRef::Lineage(alpha_lineage)));
    assert!(!raw_node
        .bindings
        .anchors
        .contains(&AnchorRef::Node(renamed_alpha)));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plans_hydrate_without_sqlite_coordination_snapshot() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, task_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:published-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-plan")),
                    causation: None,
                },
                "Ship published plan hydration".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:published-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-plan")),
                    causation: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Hydrate plans from repo state".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(prism_ir::SessionId::new("session:published-plan")),
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision,
                },
            )?;
            Ok((plan_id, task.id))
        })
        .unwrap();

    let index_path = root.join(".prism").join("plans").join("index.jsonl");
    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    assert!(index_path.exists(), "published plan index should exist");
    assert!(log_path.exists(), "published plan log should exist");
    let log_contents = fs::read_to_string(&log_path).unwrap();
    assert!(
        !log_contents.contains("session:published-plan"),
        "repo-published plan logs should not persist runtime session ids"
    );
    assert!(
        log_contents.contains("\"kind\":\"plan_created\""),
        "published plan logs should use native plan events"
    );
    assert!(
        log_contents.contains("\"kind\":\"node_added\""),
        "published plan logs should append native node events"
    );

    drop(session);
    fs::remove_file(root.join(".prism").join("cache.db")).unwrap();

    let reloaded = index_workspace_session(&root).unwrap();
    let snapshot = reloaded
        .load_coordination_snapshot()
        .unwrap()
        .expect("published plans should hydrate a coordination snapshot");
    assert!(snapshot
        .plans
        .iter()
        .any(|plan| plan.id == plan_id && plan.goal == "Ship published plan hydration"));
    assert!(snapshot.tasks.iter().any(|task| {
        task.id == task_id
            && task.plan == plan_id
            && task.title == "Hydrate plans from repo state"
            && task.status == prism_ir::CoordinationTaskStatus::Ready
            && task.session.is_none()
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plans_merge_into_existing_coordination_snapshot() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (published_plan_id, published_task_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:published-merge-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-merge-plan")),
                    causation: None,
                },
                "Published plan should stay mutable".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:published-merge-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-merge-plan")),
                    causation: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Published task should be available to mutations".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision,
                },
            )?;
            Ok((plan_id, task.id))
        })
        .unwrap();
    drop(session);

    let coordination = CoordinationStore::new();
    let base_revision = prism_ir::WorkspaceRevision {
        graph_version: 1,
        git_commit: None,
    };
    let (snapshot_plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coordination:snapshot-plan"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:snapshot-plan")),
                causation: None,
            },
            PlanCreateInput {
                goal: "Persisted snapshot should remain authoritative".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (snapshot_task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coordination:snapshot-task"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:snapshot-plan")),
                causation: None,
            },
            TaskCreateInput {
                plan_id: snapshot_plan_id.clone(),
                title: "Snapshot task should survive merge".into(),
                status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision,
            },
        )
        .unwrap();

    let snapshot = coordination.snapshot();
    let loaded = crate::published_plans::load_hydrated_coordination_snapshot(&root, Some(snapshot))
        .unwrap()
        .expect("merged coordination snapshot");
    assert!(loaded.plans.iter().any(|plan| {
        plan.id == published_plan_id && plan.goal == "Published plan should stay mutable"
    }));
    assert!(loaded.tasks.iter().any(|task| {
        task.id == published_task_id
            && task.plan == published_plan_id
            && task.title == "Published task should be available to mutations"
    }));
    assert!(loaded.plans.iter().any(|plan| {
        plan.id == snapshot_plan_id && plan.goal == "Persisted snapshot should remain authoritative"
    }));
    assert!(loaded.tasks.iter().any(|task| {
        task.id == snapshot_task_id
            && task.plan == snapshot_plan_id
            && task.title == "Snapshot task should survive merge"
            && task.status == prism_ir::CoordinationTaskStatus::InProgress
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plan_state_merges_snapshot_and_published_views() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let published_plan_id = session
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:plan-state-merge-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:plan-state-merge-plan")),
                    causation: None,
                },
                "Published plan must exist in both runtimes".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();
    drop(session);

    let coordination = CoordinationStore::new();
    let snapshot = coordination.snapshot();
    let state =
        crate::published_plans::load_hydrated_coordination_plan_state(&root, Some(snapshot))
            .unwrap()
            .expect("hydrated coordination plan state");
    assert!(state
        .snapshot
        .plans
        .iter()
        .any(|plan| plan.id == published_plan_id
            && plan.goal == "Published plan must exist in both runtimes"));
    assert!(state
        .plan_graphs
        .iter()
        .any(|graph| graph.id == published_plan_id));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn replayed_coordination_snapshot_stays_authoritative_over_published_plan_exports() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, task_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:authoritative-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:authoritative-plan")),
                    causation: None,
                },
                "Keep replay authoritative".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:authoritative-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:authoritative-plan")),
                    causation: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Ignore stale export artifacts".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision,
                },
            )?;
            Ok((plan_id, task.id))
        })
        .unwrap();

    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let stale_export = fs::read_to_string(&log_path)
        .unwrap()
        .replace(
            "Keep replay authoritative",
            "Stale published export should not win",
        )
        .replace(
            "Ignore stale export artifacts",
            "Projection mutation should not override replay",
        );
    fs::write(&log_path, stale_export).unwrap();

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let state = reloaded
        .load_coordination_plan_state()
        .unwrap()
        .expect("replayed coordination plan state");
    assert!(state
        .snapshot
        .plans
        .iter()
        .any(|plan| { plan.id == plan_id && plan.goal == "Keep replay authoritative" }));
    assert!(state.snapshot.tasks.iter().any(|task| {
        task.id == task_id && task.plan == plan_id && task.title == "Ignore stale export artifacts"
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn coordination_persistence_backend_wraps_store_and_repo_published_plans() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coordination:persistence-backend-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:persistence-backend")),
                causation: None,
            },
            PlanCreateInput {
                goal: "Exercise backend-neutral coordination persistence".into(),
                status: None,
                policy: Default::default(),
            },
        )
        .unwrap();
    let (task_id, _) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coordination:persistence-backend-task"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:persistence-backend")),
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Hydrate native plan state through the store facade".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let snapshot = coordination.snapshot();
    let mut store = MemoryStore::default();
    store
        .persist_coordination_snapshot_for_root(&root, &snapshot)
        .unwrap();
    assert_eq!(
        store.load_coordination_events().unwrap().len(),
        snapshot.events.len()
    );
    let context = store
        .load_latest_coordination_persist_context()
        .unwrap()
        .expect("coordination persist context should be recorded");
    assert!(context.repo_id.starts_with("repo:"));
    assert!(context.worktree_id.starts_with("worktree:"));
    assert!(context.instance_id.is_some());
    let read_model = store
        .load_coordination_read_model()
        .unwrap()
        .expect("coordination read model should be persisted");
    assert_eq!(read_model.active_plans.len(), 1);
    assert_eq!(read_model.task_count, 1);
    let queue_model = store
        .load_coordination_queue_read_model()
        .unwrap()
        .expect("coordination queue read model should be persisted");
    assert!(queue_model.pending_handoff_tasks.is_empty());
    assert!(queue_model.active_claims.is_empty());
    assert!(queue_model.pending_review_artifacts.is_empty());

    assert!(root
        .join(".prism")
        .join("plans")
        .join("index.jsonl")
        .exists());
    assert!(root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0))
        .exists());

    let hydrated = store
        .load_hydrated_coordination_plan_state_for_root(&root)
        .unwrap()
        .expect("coordination backend should hydrate published plan state");
    assert!(hydrated.snapshot.plans.iter().any(|plan| plan.id == plan_id
        && plan.goal == "Exercise backend-neutral coordination persistence"));
    assert!(hydrated.snapshot.tasks.iter().any(|task| {
        task.id == task_id
            && task.plan == plan_id
            && task.title == "Hydrate native plan state through the store facade"
    }));
    assert!(hydrated.plan_graphs.iter().any(|graph| {
        graph.id == plan_id && graph.nodes.iter().any(|node| node.id.0 == task_id.0)
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn coordination_persistence_incrementally_updates_stored_read_models() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coordination:incremental-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            PlanCreateInput {
                goal: "Exercise incremental read-model persistence".into(),
                status: None,
                policy: None,
            },
        )
        .unwrap();
    let (task_id, task) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coordination:incremental-task"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            TaskCreateInput {
                plan_id: plan_id.clone(),
                title: "Track incremental persistence".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(SessionId::new("session:a")),
                worktree_id: Some("worktree:a".into()),
                branch_ref: Some("refs/heads/main".into()),
                anchors: vec![AnchorRef::Kind(NodeKind::Function)],
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
        )
        .unwrap();

    let initial_snapshot = coordination.snapshot();
    let mut store = MemoryStore::default();
    store
        .persist_coordination_snapshot_for_root(&root, &initial_snapshot)
        .unwrap();
    assert_eq!(store.coordination_revision().unwrap(), 1);

    coordination
        .update_task(
            EventMeta {
                id: EventId::new("coordination:incremental-task-review"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                status: Some(prism_ir::CoordinationTaskStatus::InReview),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                anchors: None,
                depends_on: None,
                acceptance: None,
                base_revision: None,
                completion_context: None,
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
            3,
        )
        .unwrap();
    coordination
        .handoff(
            EventMeta {
                id: EventId::new("coordination:incremental-handoff"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            HandoffInput {
                task_id: task_id.clone(),
                to_agent: Some(prism_ir::AgentId::new("agent:b")),
                summary: "Need another owner".into(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
            },
            prism_ir::WorkspaceRevision {
                graph_version: 1,
                git_commit: None,
            },
        )
        .unwrap();
    coordination
        .acquire_claim(
            EventMeta {
                id: EventId::new("coordination:incremental-claim"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            SessionId::new("session:b"),
            ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::SoftExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                agent: Some(prism_ir::AgentId::new("agent:b")),
                worktree_id: Some("worktree:b".into()),
                branch_ref: Some("refs/heads/feature".into()),
            },
        )
        .unwrap();
    coordination
        .propose_artifact(
            EventMeta {
                id: EventId::new("coordination:incremental-artifact"),
                ts: 6,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:incremental-plan")),
                causation: None,
            },
            ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: task.anchors.clone(),
                diff_ref: Some("patch:feature".into()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                current_revision: prism_ir::WorkspaceRevision {
                    graph_version: 1,
                    git_commit: None,
                },
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: Some(0.1),
                worktree_id: Some("worktree:b".into()),
                branch_ref: Some("refs/heads/feature".into()),
            },
        )
        .unwrap();

    let updated_snapshot = coordination.snapshot();
    let appended_events = updated_snapshot.events[initial_snapshot.events.len()..].to_vec();
    store
        .persist_coordination_mutation_state_for_root_with_session(
            &root,
            1,
            &updated_snapshot,
            &appended_events,
            Some(&SessionId::new("session:b")),
            None,
            None,
        )
        .unwrap();

    let read_model = store
        .load_coordination_read_model()
        .unwrap()
        .expect("incremental coordination read model should be persisted");
    assert_eq!(
        read_model,
        prism_coordination::coordination_read_model_from_snapshot(&updated_snapshot)
    );

    let queue_model = store
        .load_coordination_queue_read_model()
        .unwrap()
        .expect("incremental coordination queue model should be persisted");
    assert_eq!(
        queue_model,
        prism_coordination::coordination_queue_read_model_from_snapshot(&updated_snapshot)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn authoritative_coordination_load_prefers_event_log_over_stale_snapshot_row() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let coordination = CoordinationStore::new();
    let (plan_id, _) = coordination
        .create_plan(
            EventMeta {
                id: EventId::new("coordination:event-backed-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:event-backed-load")),
                causation: None,
            },
            PlanCreateInput {
                goal: "Prefer event-backed continuity load".into(),
                status: None,
                policy: Default::default(),
            },
        )
        .unwrap();
    let (task_id, task) = coordination
        .create_task(
            EventMeta {
                id: EventId::new("coordination:event-backed-task"),
                ts: 2,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:event-backed-load")),
                causation: None,
            },
            TaskCreateInput {
                plan_id,
                title: "Rehydrate continuity from events".into(),
                status: Some(prism_ir::CoordinationTaskStatus::Ready),
                assignee: None,
                session: Some(prism_ir::SessionId::new("session:event-backed")),
                worktree_id: Some("worktree:event-backed".into()),
                branch_ref: Some("refs/heads/main".into()),
                anchors: Vec::new(),
                depends_on: Vec::new(),
                acceptance: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
            },
        )
        .unwrap();
    let (claim_id, _, _) = coordination
        .acquire_claim(
            EventMeta {
                id: EventId::new("coordination:event-backed-claim"),
                ts: 3,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:event-backed-load")),
                causation: None,
            },
            prism_ir::SessionId::new("session:event-backed"),
            prism_coordination::ClaimAcquireInput {
                task_id: Some(task_id.clone()),
                anchors: task.anchors.clone(),
                capability: prism_ir::Capability::Edit,
                mode: Some(prism_ir::ClaimMode::HardExclusive),
                ttl_seconds: Some(60),
                base_revision: prism_ir::WorkspaceRevision::default(),
                current_revision: prism_ir::WorkspaceRevision::default(),
                agent: None,
                worktree_id: Some("worktree:event-backed".into()),
                branch_ref: Some("refs/heads/main".into()),
            },
        )
        .unwrap();
    let claim_id = claim_id.expect("claim id");
    let (artifact_id, _) = coordination
        .propose_artifact(
            EventMeta {
                id: EventId::new("coordination:event-backed-artifact"),
                ts: 4,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:event-backed-load")),
                causation: None,
            },
            prism_coordination::ArtifactProposeInput {
                task_id: task_id.clone(),
                anchors: Vec::new(),
                diff_ref: Some("patch:event-backed".into()),
                evidence: Vec::new(),
                base_revision: prism_ir::WorkspaceRevision::default(),
                current_revision: prism_ir::WorkspaceRevision::default(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
                worktree_id: Some("worktree:event-backed".into()),
                branch_ref: Some("refs/heads/main".into()),
            },
        )
        .unwrap();
    let (review_id, _, _) = coordination
        .review_artifact(
            EventMeta {
                id: EventId::new("coordination:event-backed-review"),
                ts: 5,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:event-backed-load")),
                causation: None,
            },
            prism_coordination::ArtifactReviewInput {
                artifact_id: artifact_id.clone(),
                verdict: prism_ir::ReviewVerdict::Approved,
                summary: "approved".into(),
                required_validations: Vec::new(),
                validated_checks: Vec::new(),
                risk_score: None,
            },
            prism_ir::WorkspaceRevision::default(),
        )
        .unwrap();

    let snapshot = coordination.snapshot();
    let mut store = MemoryStore::default();
    store
        .persist_coordination_snapshot_for_root(&root, &snapshot)
        .unwrap();

    let loaded = store
        .load_hydrated_coordination_snapshot_for_root(&root)
        .unwrap()
        .expect("event-backed snapshot");
    assert_eq!(loaded.claims.len(), 1);
    assert_eq!(loaded.claims[0].id, claim_id);
    assert_eq!(loaded.artifacts.len(), 1);
    assert_eq!(loaded.artifacts[0].id, artifact_id);
    assert_eq!(loaded.reviews.len(), 1);
    assert_eq!(loaded.reviews[0].id, review_id);
}

#[test]
fn coordination_persistence_compacts_large_event_suffixes_into_optional_baseline() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let snapshot = CoordinationSnapshot {
        events: (0..140)
            .map(|index| CoordinationEvent {
                meta: EventMeta {
                    id: EventId::new(format!("coordination:compact:{index}")),
                    ts: index as u64,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:coordination-compaction")),
                    causation: None,
                },
                kind: CoordinationEventKind::PlanCreated,
                summary: format!("event {index}"),
                plan: None,
                task: None,
                claim: None,
                artifact: None,
                review: None,
                metadata: serde_json::Value::Null,
            })
            .collect(),
        ..CoordinationSnapshot::default()
    };

    let mut store = MemoryStore::default();
    store
        .persist_coordination_snapshot_for_root(&root, &snapshot)
        .unwrap();

    let stream = store.load_coordination_event_stream().unwrap();
    assert!(stream.fallback_snapshot.is_some());
    assert!(stream.suffix_events.is_empty());
    let hydrated = store
        .load_hydrated_coordination_snapshot_for_root(&root)
        .unwrap()
        .expect("event-backed snapshot");
    assert!(hydrated.events.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_repo_published_plan_logs_still_hydrate() {
    let root = temp_workspace();
    fs::create_dir_all(root.join(".prism").join("plans").join("active")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    fs::write(
        root.join(".prism").join("plans").join("index.jsonl"),
        concat!(
            "{\"plan_id\":\"plan:1\",\"title\":\"Legacy published plan\",\"status\":\"Active\",\"scope\":\"Repo\",\"kind\":\"TaskExecution\",\"log_path\":\".prism/plans/active/plan:1.jsonl\"}\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join(".prism")
            .join("plans")
            .join("active")
            .join("plan:1.jsonl"),
        concat!(
            "{\"event_id\":\"published:plan:1:1\",\"kind\":\"plan_updated\",\"plan_id\":\"plan:1\",\"node_id\":null,\"payload\":{\"type\":\"plan\",\"plan\":{\"id\":\"plan:1\",\"goal\":\"Legacy published plan\",\"status\":\"Active\",\"policy\":{\"default_claim_mode\":\"Advisory\",\"max_parallel_editors_per_anchor\":2,\"require_review_for_completion\":false,\"require_validation_for_completion\":false,\"stale_after_graph_change\":true,\"review_required_above_risk_score\":null},\"root_tasks\":[\"coord-task:1\"]}}}\n",
            "{\"event_id\":\"published:plan:1:2\",\"kind\":\"node_updated\",\"plan_id\":\"plan:1\",\"node_id\":\"coord-task:1\",\"payload\":{\"type\":\"node\",\"task\":{\"id\":\"coord-task:1\",\"plan\":\"plan:1\",\"title\":\"Hydrate legacy task log\",\"status\":\"Ready\",\"assignee\":null,\"anchors\":[],\"depends_on\":[],\"acceptance\":[],\"base_revision\":{\"graph_version\":1,\"git_commit\":null}}}}\n"
        ),
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let snapshot = session
        .load_coordination_snapshot()
        .unwrap()
        .expect("legacy published plans should hydrate a coordination snapshot");
    assert!(snapshot
        .plans
        .iter()
        .any(|plan| plan.id.0 == "plan:1" && plan.goal == "Legacy published plan"));
    assert!(snapshot.tasks.iter().any(|task| {
        task.id.0 == "coord-task:1"
            && task.plan.0 == "plan:1"
            && task.title == "Hydrate legacy task log"
            && task.status == prism_ir::CoordinationTaskStatus::Ready
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plan_logs_append_deltas_instead_of_rewriting_full_state() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, task_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:append-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:append-plan")),
                    causation: None,
                },
                "Append published plan deltas".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:append-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:append-plan")),
                    causation: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Append a node delta".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision,
                },
            )?;
            Ok((plan_id, task.id))
        })
        .unwrap();

    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let initial_lines = fs::read_to_string(&log_path).unwrap().lines().count();
    assert_eq!(initial_lines, 2, "initial publish should write plan + node");

    session
        .mutate_coordination(|prism| {
            let _ = prism.update_native_task(
                EventMeta {
                    id: EventId::new("coordination:append-task-update"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:append-plan")),
                    causation: None,
                },
                prism_coordination::TaskUpdateInput {
                    task_id: task_id.clone(),
                    status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    anchors: None,
                    depends_on: None,
                    acceptance: None,
                    base_revision: Some(prism.workspace_revision()),
                    completion_context: None,
                },
                prism.workspace_revision(),
                3,
            )?;
            Ok(())
        })
        .unwrap();

    let updated_lines = fs::read_to_string(&log_path).unwrap().lines().count();
    assert_eq!(
        updated_lines, 3,
        "task status change should append one delta event instead of rewriting the full log"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_skips_reindex_when_workspace_is_clean() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let before = session.prism();
    let observed = session.refresh_fs().unwrap();
    let after = session.prism();

    assert!(observed.is_empty());
    assert!(Arc::ptr_eq(&before, &after));

    drop(session);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_nonblocking_defers_when_refresh_is_in_progress() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths(std::iter::empty::<PathBuf>());
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let status = session.refresh_fs_nonblocking().unwrap();
    assert_eq!(status, crate::FsRefreshStatus::DeferredBusy);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_nonblocking_keeps_clean_status_for_busy_fallback_probe() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let status = session.refresh_fs_nonblocking().unwrap();
    assert_eq!(status, crate::FsRefreshStatus::Clean);
    assert!(!session.needs_refresh());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_nonblocking_detects_out_of_band_changes_via_fallback_scan() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/created.md"),
        "# Watcher Created Doc\n\nThis document was added after startup.\n",
    )
    .unwrap();

    let status = session.refresh_fs_nonblocking().unwrap();

    assert_eq!(status, crate::FsRefreshStatus::Rescan);
    assert_eq!(
        session
            .last_refresh()
            .as_ref()
            .map(|refresh| refresh.path.as_str()),
        Some("rescan")
    );
    assert!(session
        .prism()
        .symbol("Watcher Created Doc")
        .iter()
        .any(|symbol| symbol.id().kind == NodeKind::MarkdownHeading));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_with_status_reports_rescan_for_fallback_scan() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/created.md"),
        "# Watcher Created Doc\n\nThis document was added after startup.\n",
    )
    .unwrap();

    let outcome = session.refresh_fs_with_status().unwrap();

    assert_eq!(outcome.status, crate::FsRefreshStatus::Rescan);
    assert_eq!(
        session
            .last_refresh()
            .as_ref()
            .map(|refresh| refresh.path.as_str()),
        Some("rescan")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_rebuild_from_persisted_state_defers_when_refresh_is_in_progress() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let reloaded = session.try_recover_runtime_from_persisted_state().unwrap();
    assert!(!reloaded);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn coordination_mutations_use_live_runtime_state_without_forcing_persisted_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let live_plan_id = session
        .prism()
        .create_native_plan(
            EventMeta {
                id: EventId::new("coordination:live-runtime-plan"),
                ts: 1,
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:live-runtime-plan")),
                causation: None,
            },
            "Use live runtime coordination state".into(),
            None,
            Some(Default::default()),
        )
        .unwrap();

    let observed_plan_id = session
        .mutate_coordination(|prism| {
            Ok(prism
                .coordination_plan(&live_plan_id)
                .expect("live-only plan should still be visible during mutation")
                .id)
        })
        .unwrap();

    assert_eq!(observed_plan_id, live_plan_id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_hydrates_persisted_curated_concepts_even_when_derived_projections_stay_disabled() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let persisted_concept = ConceptPacket {
        handle: "concept://persisted-only".to_string(),
        canonical_name: "persisted only".to_string(),
        summary:
            "Curated session concepts should still load even when derived projection hydration stays disabled."
                .to_string(),
        aliases: Vec::new(),
        confidence: 0.9,
        core_members: Vec::new(),
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["seeded from a persisted projection snapshot".to_string()],
        risk_hint: None,
        decode_lenses: Vec::new(),
        scope: ConceptScope::Session,
        provenance: ConceptProvenance::default(),
        publication: None,
    };

    let mut default_store = MemoryStore::default();
    default_store
        .save_projection_snapshot(&ProjectionSnapshot {
            co_change_by_lineage: Vec::new(),
            validation_by_lineage: Vec::new(),
            curated_concepts: vec![persisted_concept.clone()],
            concept_relations: Vec::new(),
        })
        .unwrap();
    let default_indexer = WorkspaceIndexer::with_store_and_options(
        &root,
        default_store,
        WorkspaceSessionOptions::default(),
    )
    .unwrap();
    assert!(default_indexer
        .projections
        .curated_concepts()
        .iter()
        .any(|concept| concept.handle == persisted_concept.handle));

    let mut hydrated_store = MemoryStore::default();
    hydrated_store
        .save_projection_snapshot(&ProjectionSnapshot {
            co_change_by_lineage: Vec::new(),
            validation_by_lineage: Vec::new(),
            curated_concepts: vec![persisted_concept.clone()],
            concept_relations: Vec::new(),
        })
        .unwrap();
    let hydrated_indexer = WorkspaceIndexer::with_store_and_options(
        &root,
        hydrated_store,
        WorkspaceSessionOptions {
            hydrate_persisted_projections: true,
            ..WorkspaceSessionOptions::default()
        },
    )
    .unwrap();
    assert!(hydrated_indexer
        .projections
        .curated_concepts()
        .iter()
        .any(|concept| concept.handle == persisted_concept.handle));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_state_throttles_clean_fallback_checks() {
    let state = crate::session::WorkspaceRefreshState::new();

    assert!(state.should_run_fallback_check(1_000));
    assert!(!state.should_run_fallback_check(1_100));
    assert!(state.should_run_fallback_check(1_250));
}

#[test]
fn refresh_state_keeps_later_dirty_path_revisions_pending() {
    let state = crate::session::WorkspaceRefreshState::new();
    let path = PathBuf::from("/tmp/demo.rs");

    let first_revision = state.mark_fs_dirty_paths([path.clone()]);
    let second_revision = state.mark_fs_dirty_paths([path.clone()]);

    state.mark_refreshed_revision(first_revision, std::slice::from_ref(&path));
    assert!(state.needs_refresh());
    let pending = state.dirty_paths_snapshot();
    assert_eq!(pending.len(), 1);
    assert!(pending.contains(&path));

    state.mark_refreshed_revision(second_revision, std::slice::from_ref(&path));
    assert!(!state.needs_refresh());
    assert!(state.dirty_paths_snapshot().is_empty());
}

#[test]
fn refresh_fs_falls_back_to_full_reindex_for_out_of_root_watch_paths() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/created.md"),
        "# Watcher Created Doc\n\nThis document was added after startup.\n",
    )
    .unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([PathBuf::from("/tmp/editor-copy-created.md")]);

    let observed = session.refresh_fs().unwrap();

    assert!(!observed.is_empty());
    assert!(session
        .prism()
        .symbol("Watcher Created Doc")
        .iter()
        .any(|symbol| symbol.id().kind == NodeKind::MarkdownHeading));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn index_with_scope_refreshes_only_dirty_paths_and_removals() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/a.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(root.join("src/b.rs"), "pub fn beta() {}\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod a;\nmod b;\npub use a::alpha;\npub use b::beta;\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();
    assert!(indexer
        .graph()
        .nodes_by_name("alpha")
        .iter()
        .any(|node| node.id.path.ends_with("::alpha")));
    assert!(indexer
        .graph()
        .nodes_by_name("beta")
        .iter()
        .any(|node| node.id.path.ends_with("::beta")));

    fs::write(root.join("src/a.rs"), "pub fn gamma() {}\n").unwrap();
    indexer
        .index_with_scope(ChangeTrigger::FsWatch, [root.join("src/a.rs")])
        .unwrap();

    assert!(indexer
        .graph()
        .all_nodes()
        .any(|node| node.id.path.ends_with("::a::gamma")));
    assert!(indexer
        .graph()
        .all_nodes()
        .any(|node| node.id.path.ends_with("::b::beta")));

    fs::remove_file(root.join("src/b.rs")).unwrap();
    indexer
        .index_with_scope(ChangeTrigger::FsWatch, [root.join("src/b.rs")])
        .unwrap();

    assert!(indexer
        .graph()
        .all_nodes()
        .any(|node| node.id.path.ends_with("::a::gamma")));
    assert!(indexer
        .graph()
        .file_record(&root.join("src/b.rs"))
        .is_none());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn full_reindex_of_large_repo_defaults_to_shallow_parse_depth() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    for index in 0..64 {
        fs::write(
            root.join(format!("src/helper_{index}.rs")),
            format!("pub fn helper_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let target_path = root.join("src/lib.rs");
    fs::write(
        &target_path,
        "pub fn alpha() {\n    beta();\n}\n\nfn beta() {}\n",
    )
    .unwrap();
    let target_path = fs::canonicalize(target_path).unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    let tracked_files = indexer.graph().tracked_files();
    let record = indexer
        .graph()
        .file_record(&target_path)
        .unwrap_or_else(|| panic!("lib file should be indexed; tracked={tracked_files:?}"));
    assert_eq!(record.parse_depth, ParseDepth::Shallow);
    assert!(record.unresolved_calls.is_empty());
    assert!(indexer
        .graph()
        .nodes_by_name("alpha")
        .iter()
        .any(|node| node.id.path.ends_with("::alpha")));
}

#[test]
fn workspace_session_can_deepen_unchanged_shallow_file_on_demand() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    for index in 0..64 {
        fs::write(
            root.join(format!("src/helper_{index}.rs")),
            format!("pub fn helper_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let target_path = root.join("src/lib.rs");
    fs::write(
        &target_path,
        "pub fn alpha() {\n    beta();\n}\n\nfn beta() {}\n",
    )
    .unwrap();
    let target_path = fs::canonicalize(target_path).unwrap();

    let session = index_workspace_session(&root).unwrap();
    let initial = session.prism();
    let tracked_files = initial.graph().tracked_files();
    let initial_record = initial
        .graph()
        .file_record(&target_path)
        .unwrap_or_else(|| panic!("lib file should be indexed; tracked={tracked_files:?}"));
    assert_eq!(initial_record.parse_depth, ParseDepth::Shallow);
    assert!(initial_record.unresolved_calls.is_empty());

    assert!(session
        .ensure_paths_deep([target_path.clone()])
        .expect("deepening should succeed"));

    let refreshed = session.prism();
    let refreshed_record = refreshed
        .graph()
        .file_record(&target_path)
        .expect("deepened file should remain indexed");
    assert_eq!(refreshed_record.parse_depth, ParseDepth::Deep);
    assert!(refreshed_record.unresolved_calls.iter().any(|call| call
        .caller
        .path
        .ends_with("::alpha")
        && call.name == "beta"));
}

#[test]
fn refresh_invalidation_scope_preserves_monotonic_scope_expansion() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/a.rs"), "pub fn alpha() -> i32 { 1 }\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod a;\npub fn uses_alpha() -> i32 { a::alpha() }\n",
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    indexer.index().unwrap();

    let changed = root.join("src/a.rs");
    let scope = crate::invalidation::RefreshInvalidationScope::from_graph(
        indexer.graph(),
        &HashSet::from([changed.clone()]),
    );

    assert!(scope.direct_paths.contains(&changed));
    assert!(scope.dependency_paths.is_superset(&scope.direct_paths));
    assert!(scope
        .edge_resolution_paths
        .is_superset(&scope.dependency_paths));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_materialization_summary_reports_sparse_boundary_regions() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    let prism = index_workspace(&root).unwrap();
    let mut snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
    let extra_path = root.join("src/extra.rs");
    let template_fingerprint = snapshot
        .files
        .get(&root.join("src/lib.rs"))
        .cloned()
        .expect("lib file fingerprint should exist");
    snapshot.files.insert(extra_path, template_fingerprint);

    let summary = summarize_workspace_materialization(&root, &snapshot, prism.graph());

    assert!(summary.known_files > summary.materialized_files);
    assert_eq!(summary.boundaries.len(), 1);
    let boundary = &summary.boundaries[0];
    assert_eq!(boundary.id, "boundary:src:in_scope");
    assert_eq!(boundary.path, PathBuf::from("src"));
    assert_eq!(boundary.provenance, "workspace_tree");
    assert_eq!(boundary.materialization_state, "known_unmaterialized");
    assert_eq!(boundary.scope_state, "in_scope");
    assert_eq!(boundary.known_file_count, 2);
    assert_eq!(boundary.materialized_file_count, 0);
}

#[test]
fn workspace_materialization_summary_reports_out_of_scope_regions() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("web")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(root.join("web/app.js"), "export const alpha = 1;\n").unwrap();

    let prism = index_workspace(&root).unwrap();
    let snapshot = build_workspace_tree_snapshot(&root, None).unwrap();
    let summary = summarize_workspace_materialization(&root, &snapshot, prism.graph());

    let boundary = summary
        .boundaries
        .iter()
        .find(|boundary| boundary.id == "boundary:web:out_of_scope")
        .expect("out-of-scope region should be reported");
    assert_eq!(boundary.path, PathBuf::from("web"));
    assert_eq!(boundary.provenance, "workspace_walk");
    assert_eq!(boundary.materialization_state, "out_of_scope");
    assert_eq!(boundary.scope_state, "out_of_scope");
    assert_eq!(boundary.known_file_count, 1);
    assert_eq!(boundary.materialized_file_count, 0);
}

#[test]
fn curator_context_loads_lineage_history_from_store_when_hot_history_is_empty() {
    let root = temp_workspace();
    let cache_path = root.join(".prism").join("cache.db");
    let mut store = SqliteStore::open(&cache_path).unwrap();

    let node = prism_ir::Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: prism_ir::FileId(1),
        span: prism_ir::Span::new(1, 3),
        language: prism_ir::Language::Rust,
    };
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node.clone());

    let mut hot_history = prism_history::HistoryStore::new();
    hot_history.seed_nodes([node.id.clone()]);
    let lineage = hot_history
        .lineage_of(&node.id)
        .expect("seeded node should have lineage");
    let persisted_event = LineageEvent {
        meta: EventMeta {
            id: EventId::new("event:curator-lineage"),
            ts: 11,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
        },
        lineage: lineage.clone(),
        kind: LineageEventKind::Updated,
        before: vec![node.id.clone()],
        after: vec![node.id.clone()],
        confidence: 0.95,
        evidence: vec![LineageEvidence::ExactNodeId],
    };
    let mut persisted_history = hot_history.snapshot();
    persisted_history.events = vec![persisted_event.clone()];
    store.save_history_snapshot(&persisted_history).unwrap();

    let prism = Prism::with_history(graph, hot_history);
    let context = build_curator_context(
        &prism,
        &mut store,
        &[AnchorRef::Node(node.id.clone())],
        &CuratorBudget::default(),
    )
    .unwrap();

    assert_eq!(context.lineage.events, vec![persisted_event]);
}

struct PanicOutcomeBackend;

impl OutcomeReadBackend for PanicOutcomeBackend {
    fn query_outcomes(&self, _query: &OutcomeRecallQuery) -> anyhow::Result<Vec<OutcomeEvent>> {
        panic!("curator context should not re-enter the cold outcome backend while holding the store lock");
    }

    fn load_outcome_event(&self, _event_id: &EventId) -> anyhow::Result<Option<OutcomeEvent>> {
        panic!("curator context should not load outcome events through the cold outcome backend");
    }

    fn load_task_replay(&self, _task_id: &TaskId) -> anyhow::Result<prism_memory::TaskReplay> {
        panic!("curator context should not load task replay through the cold outcome backend");
    }
}

#[test]
fn curator_context_loads_outcomes_from_locked_store_without_backend_reentry() {
    let root = temp_workspace();
    let cache_path = root.join(".prism").join("cache.db");
    let mut store = SqliteStore::open(&cache_path).unwrap();

    let node = prism_ir::Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: prism_ir::FileId(1),
        span: prism_ir::Span::new(1, 3),
        language: prism_ir::Language::Rust,
    };
    let mut graph = Graph::default();
    graph.nodes.insert(node.id.clone(), node.clone());

    let mut hot_history = prism_history::HistoryStore::new();
    hot_history.seed_nodes([node.id.clone()]);

    let persisted_event = OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:curator-store"),
            ts: 12,
            actor: EventActor::Agent,
            correlation: None,
            causation: None,
        },
        anchors: vec![AnchorRef::Node(node.id.clone())],
        kind: OutcomeKind::FixValidated,
        result: OutcomeResult::Success,
        summary: "persisted outcome".into(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    store
        .save_outcome_snapshot(&prism_memory::OutcomeMemorySnapshot {
            events: vec![persisted_event.clone()],
        })
        .unwrap();

    let prism = Prism::with_history(graph, hot_history);
    prism.set_outcome_backend(Some(Arc::new(PanicOutcomeBackend)));

    let context = build_curator_context(
        &prism,
        &mut store,
        &[AnchorRef::Node(node.id.clone())],
        &CuratorBudget::default(),
    )
    .unwrap();

    assert_eq!(context.outcomes, vec![persisted_event]);
}

#[test]
fn refresh_fs_preserves_live_projection_state_and_coordination_context() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    session.prism().upsert_curated_concept(ConceptPacket {
        handle: "concept://live_refresh_state".to_string(),
        canonical_name: "live_refresh_state".to_string(),
        summary: "Session-local concept kept across fs refresh.".to_string(),
        aliases: vec!["live refresh".to_string()],
        confidence: 0.9,
        core_members: Vec::new(),
        core_member_lineages: Vec::new(),
        supporting_members: Vec::new(),
        supporting_member_lineages: Vec::new(),
        likely_tests: Vec::new(),
        likely_test_lineages: Vec::new(),
        evidence: vec!["Added directly to the live prism state in a refresh test.".to_string()],
        risk_hint: None,
        decode_lenses: vec![ConceptDecodeLens::Open],
        scope: ConceptScope::Session,
        provenance: ConceptProvenance {
            origin: "test".to_string(),
            kind: "refresh_live_state".to_string(),
            task_id: None,
        },
        publication: None,
    });
    assert!(session.prism().coordination_context().is_some());

    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    )
    .unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([root.join("src/lib.rs")]);

    let observed = session.refresh_fs().unwrap();
    assert!(!observed.is_empty());

    let prism = session.prism();
    assert!(prism.coordination_context().is_some());
    assert!(prism
        .concept_by_handle("concept://live_refresh_state")
        .is_some());
    assert!(prism
        .symbol("beta")
        .into_iter()
        .any(|symbol| symbol.id().path.ends_with("::beta")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn index_workspace_tracks_unsupported_text_files_for_file_anchors() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("www/dashboard/src")).unwrap();
    fs::write(
        root.join("www/dashboard/src/App.tsx"),
        "export const app = 1;\n",
    )
    .unwrap();

    let session = index_workspace_session(&root).unwrap();
    let app_path = root
        .join("www/dashboard/src/App.tsx")
        .canonicalize()
        .unwrap();
    let file_id = session
        .prism()
        .graph()
        .file_record(&app_path)
        .map(|record| record.file_id)
        .expect("unsupported text files should still produce file records");
    assert_eq!(
        session.prism().graph().file_path(file_id),
        Some(&app_path),
        "file ids for unsupported text files should round-trip to paths"
    );

    let reloaded = index_workspace_session(&root).unwrap();
    assert!(reloaded.prism().graph().file_record(&app_path).is_some());
}

#[test]
fn appended_outcome_flushes_projection_materialization_off_request_path() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:test"),
                ts: 10,
                actor: EventActor::User,
                correlation: Some(TaskId::new("task:test")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha.clone())],
            kind: OutcomeKind::FailureObserved,
            result: OutcomeResult::Failure,
            summary: "alpha needs integration coverage".into(),
            evidence: vec![OutcomeEvidence::Test {
                name: "alpha_integration".into(),
                passed: false,
            }],
            metadata: serde_json::Value::Null,
        })
        .unwrap();
    session.flush_materializations().unwrap();
    drop(session);

    let prism = index_workspace(&root).unwrap();
    let recipe = prism.validation_recipe(&alpha);
    assert!(recipe
        .scored_checks
        .iter()
        .any(|check| check.label == "test:alpha_integration" && check.score > 0.0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clean_reindex_skips_sqlite_persist_and_keeps_workspace_revision() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let mut indexer = WorkspaceIndexer::new(&root).unwrap();
    indexer.index().unwrap();
    let revision_after_first_index = indexer.store.workspace_revision().unwrap();

    indexer.index().unwrap();
    let revision_after_second_index = indexer.store.workspace_revision().unwrap();

    assert_eq!(revision_after_second_index, revision_after_first_index);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_session_can_disable_coordination_entirely() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let enabled = index_workspace_session(&root).unwrap();
    enabled
        .mutate_coordination(|prism| {
            let _ = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:test"),
                    ts: 1,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:test")),
                    causation: None,
                },
                "Coordinate alpha".into(),
                None,
                Some(Default::default()),
            )?;
            Ok(())
        })
        .unwrap();
    drop(enabled);

    let disabled = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            coordination: false,
            shared_runtime: SharedRuntimeBackend::Disabled,
            hydrate_persisted_projections: false,
        },
    )
    .unwrap();
    assert!(!disabled.coordination_enabled);
    assert!(disabled.load_coordination_snapshot().unwrap().is_none());
    assert!(disabled.prism().coordination_snapshot().plans.is_empty());
    let error = disabled
        .mutate_coordination(|_| Ok::<_, anyhow::Error>(()))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "coordination is disabled for this workspace session"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn curator_backend_processes_and_persists_task_boundary_jobs() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    #[derive(Clone, Default)]
    struct FakeCurator {
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl CuratorBackend for FakeCurator {
        fn run(&self, _job: &CuratorJob, ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
            self.seen
                .lock()
                .unwrap()
                .push(format!("nodes:{}", ctx.graph.nodes.len()));
            Ok(CuratorRun {
                proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                    anchors: Vec::new(),
                    summary: "alpha needs follow-up".into(),
                    severity: "medium".into(),
                    evidence_events: Vec::new(),
                })],
                diagnostics: Vec::new(),
            })
        }
    }

    let backend = FakeCurator::default();
    let session = index_workspace_session_with_curator(&root, Arc::new(backend.clone())).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    session
        .append_outcome(OutcomeEvent {
            meta: EventMeta {
                id: EventId::new("outcome:validated"),
                ts: 42,
                actor: EventActor::User,
                correlation: Some(TaskId::new("task:alpha")),
                causation: None,
            },
            anchors: vec![AnchorRef::Node(alpha)],
            kind: OutcomeKind::FixValidated,
            result: OutcomeResult::Success,
            summary: "alpha fix validated".into(),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

    let mut completed = false;
    for _ in 0..40 {
        let snapshot = session.curator_snapshot().unwrap();
        if snapshot
            .records
            .iter()
            .any(|record| record.status == prism_curator::CuratorJobStatus::Completed)
        {
            completed = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(completed);
    assert_eq!(backend.seen.lock().unwrap().len(), 1);
    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    assert!(!reloaded.is_curator_snapshot_loaded());
    let snapshot = reloaded.curator_snapshot().unwrap();
    assert!(reloaded.is_curator_snapshot_loaded());
    assert_eq!(snapshot.records.len(), 1);
    assert!(matches!(
        snapshot.records[0].run.as_ref().and_then(|run| run.proposals.first()),
        Some(CuratorProposal::RiskSummary(summary)) if summary.summary == "alpha needs follow-up"
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn default_curator_synthesizes_memory_proposals_without_backend() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    for (id, ts, summary) in [
        ("outcome:repeat:1", 40, "alpha failed under routing load"),
        (
            "outcome:repeat:2",
            41,
            "alpha failed again after routing edits",
        ),
    ] {
        session
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(id),
                    ts,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: summary.into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_regression".into(),
                    passed: false,
                }],
                metadata: serde_json::Value::Null,
            })
            .unwrap();
    }

    let mut proposals = Vec::new();
    for _ in 0..40 {
        let snapshot = session.curator_snapshot().unwrap();
        if let Some(run) = snapshot
            .records
            .iter()
            .find(|record| record.status == prism_curator::CuratorJobStatus::Completed)
            .and_then(|record| record.run.clone())
        {
            proposals = run.proposals;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::StructuralMemory(candidate)
            if candidate.content.contains("should run validation")
    )));
    assert!(proposals.iter().any(|proposal| matches!(
        proposal,
        CuratorProposal::SemanticMemory(candidate)
            if candidate.content.contains("Recent outcome context")
    )));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn indexes_python_workspace_without_cargo_manifest() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src/demo_pkg")).unwrap();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo-pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/demo_pkg/base.py"), "class Base:\n    pass\n").unwrap();
    fs::write(
        root.join("src/demo_pkg/service.py"),
        r#"
from .base import Base


class Service(Base):
    setting = 1

    def __init__(self):
        self.value = helper()


def helper():
    return 1
"#,
    )
    .unwrap();

    let mut indexer = WorkspaceIndexer::new(&root).unwrap();
    indexer.index().unwrap();

    assert!(indexer
        .graph()
        .nodes_by_name("Base")
        .into_iter()
        .any(|node| node.id.path == "demo_pkg::base::Base"));
    assert!(indexer
        .graph()
        .nodes_by_name("Service")
        .into_iter()
        .any(|node| node.id.path == "demo_pkg::service::Service"));
    assert!(indexer
        .graph()
        .nodes_by_name("__init__")
        .into_iter()
        .any(|node| node.id.path == "demo_pkg::service::Service::__init__"));
    assert!(
        indexer
            .graph()
            .nodes_by_name("setting")
            .into_iter()
            .any(|node| node.id.path == "demo_pkg::service::Service::setting"),
        "setting nodes: {:?}",
        indexer
            .graph()
            .nodes_by_name("setting")
            .into_iter()
            .map(|node| node.id.path.clone())
            .collect::<Vec<_>>()
    );
    assert!(indexer
        .graph()
        .nodes_by_name("value")
        .into_iter()
        .any(|node| node.id.path == "demo_pkg::service::Service::value"));
    assert!(indexer.graph().edges.iter().any(|edge| {
        edge.kind == EdgeKind::Calls
            && edge.source.path == "demo_pkg::service::Service::__init__"
            && edge.target.path == "demo_pkg::service::helper"
    }));
    assert!(indexer.graph().edges.iter().any(|edge| {
        edge.kind == EdgeKind::Imports
            && edge.source.path == "demo_pkg::service"
            && edge.target.path == "demo_pkg::base::Base"
    }));
    assert!(indexer.graph().edges.iter().any(|edge| {
        edge.kind == EdgeKind::RelatedTo
            && edge.source.path == "demo_pkg::service::Service"
            && edge.target.path == "demo_pkg::base::Base"
    }));

    let _ = fs::remove_dir_all(root);
}

fn temp_workspace() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_TEMP_WORKSPACE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "prism-test-{}-{stamp}-{sequence}",
        std::process::id()
    ))
}
