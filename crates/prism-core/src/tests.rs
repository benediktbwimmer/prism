use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use prism_coordination::PlanCreateInput;
use prism_curator::{
    CandidateRiskSummary, CuratorBackend, CuratorContext, CuratorJob, CuratorProposal, CuratorRun,
};
use prism_ir::{
    AnchorRef, ChangeTrigger, EdgeKind, EventActor, EventId, EventMeta, GraphChange, LineageEvent,
    LineageEventKind, LineageEvidence, LineageId, NodeId, NodeKind, TaskId,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryKind, MemoryModule, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeResult, SessionMemory,
};
use prism_store::{MemoryStore, Store};

use super::{
    index_workspace, index_workspace_session, index_workspace_session_with_curator,
    index_workspace_session_with_options, ValidationFeedbackCategory, ValidationFeedbackRecord,
    ValidationFeedbackVerdict, WorkspaceIndexer, WorkspaceSessionOptions,
};
use crate::memory_refresh::reanchor_persisted_memory_snapshot;

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
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

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
fn watcher_refreshes_session_after_external_edit() {
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

    let mut saw_gamma = false;
    for _ in 0..40 {
        if session
            .prism()
            .symbol("gamma")
            .iter()
            .any(|symbol| symbol.id().path == "demo::gamma")
        {
            saw_gamma = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(saw_gamma);
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
fn watcher_refresh_enqueues_curator_with_patch_outcomes_and_projection_context() {
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
        completed = session.curator_snapshot().records.iter().any(|record| {
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
    let snapshot = reloaded.curator_snapshot();
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

            let (plan_id, _) = prism.coordination().create_plan(
                EventMeta {
                    id: EventId::new("coordination:rename-plan"),
                    ts: 1,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:coordination-rename")),
                    causation: None,
                },
                PlanCreateInput {
                    goal: "Coordinate rename follow-up".into(),
                    status: None,
                    policy: Default::default(),
                },
            )?;
            let (task_id, _) = prism.coordination().create_task(
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
                    anchors: scoped_anchors.clone(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: base_revision.clone(),
                },
            )?;
            let holder = prism_ir::SessionId::new("session:rename-owner");
            prism.coordination().acquire_claim(
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
fn try_reload_persisted_prism_defers_when_refresh_is_in_progress() {
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

    let reloaded = session.try_reload_persisted_prism().unwrap();
    assert!(!reloaded);

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
fn appended_outcome_persists_projection_snapshot() {
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
            let _ = prism.coordination().create_plan(
                EventMeta {
                    id: EventId::new("coordination:test"),
                    ts: 1,
                    actor: EventActor::User,
                    correlation: Some(TaskId::new("task:test")),
                    causation: None,
                },
                PlanCreateInput {
                    goal: "Coordinate alpha".into(),
                    status: None,
                    policy: Default::default(),
                },
            )?;
            Ok(())
        })
        .unwrap();
    drop(enabled);

    let disabled = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            coordination: false,
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
        let snapshot = session.curator_snapshot();
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
    let snapshot = reloaded.curator_snapshot();
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
        let snapshot = session.curator_snapshot();
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
