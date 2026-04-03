use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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
    AnchorRef, ChangeTrigger, CoordinationEventKind, CredentialCapability, CredentialId,
    CredentialRecord, CredentialStatus, EdgeKind, EventActor, EventId, EventMeta, GraphChange,
    LineageEvent, LineageEventKind, LineageEvidence, LineageId, NodeId, NodeKind,
    PrincipalAuthorityId, PrincipalId, PrincipalKind, PrincipalProfile, PrincipalRegistrySnapshot,
    PrincipalStatus, SessionId, TaskId, WorkContextKind,
};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind, MemoryEventQuery, MemoryId,
    MemoryKind, MemoryModule, MemoryScope, MemorySource, OutcomeEvent, OutcomeEvidence,
    OutcomeKind, OutcomeMemorySnapshot, OutcomeRecallQuery, OutcomeResult, SessionMemory,
};
use prism_parser::ParseDepth;
use prism_projections::{CoChangeRecord, ProjectionSnapshot, ValidationCheck};
use prism_query::{
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptPacket,
    ConceptProvenance, ConceptPublication, ConceptPublicationStatus, ConceptRelation,
    ConceptRelationEvent, ConceptRelationEventAction, ConceptRelationKind, ConceptScope,
    ContractCompatibility, ContractEvent, ContractEventAction, ContractGuarantee, ContractKind,
    ContractPacket, ContractStatus, ContractTarget, OutcomeReadBackend, Prism,
};
use prism_store::{Graph, MemoryStore, ProjectionMaterializationMetadata, SqliteStore, Store};
use serde_json::json;

use super::{
    hydrate_workspace_session, hydrate_workspace_session_with_options, index_workspace,
    index_workspace_session, index_workspace_session_with_curator,
    index_workspace_session_with_options, inspect_repo_published_plan_artifacts,
    regenerate_repo_published_plan_artifacts, repair_repo_published_plan_artifacts,
    BootstrapOwnerInput, MintPrincipalRequest, PrismDocSyncStatus, PrismPaths,
    SharedRuntimeBackend, ValidationFeedbackCategory, ValidationFeedbackRecord,
    ValidationFeedbackVerdict, WorkspaceIndexer, WorkspaceSessionOptions,
};
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::curator_support::build_curator_context;
use crate::materialization::summarize_workspace_materialization;
use crate::memory_events::append_repo_memory_event;
use crate::memory_refresh::reanchor_persisted_memory_snapshot;
use crate::protected_state::repo_streams::{
    append_protected_stream_event, inspect_protected_stream,
};
use crate::protected_state::streams::ProtectedRepoStream;
use crate::repo_patch_events::load_repo_patch_events;
use crate::session::HOT_OUTCOME_HYDRATION_LIMIT;
use crate::workspace_tree::build_workspace_tree_snapshot;

static NEXT_TEMP_WORKSPACE: AtomicU64 = AtomicU64::new(0);
static PRISM_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
static BACKGROUND_WORKER_TEST_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static TEMP_TEST_DIRS: RefCell<TempTestDirState> = RefCell::new(TempTestDirState {
        paths: Vec::new(),
    });
}

struct TempTestDirState {
    paths: Vec<PathBuf>,
}

impl Drop for TempTestDirState {
    fn drop(&mut self) {
        for path in self.paths.drain(..).rev() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct PrismHomeEnvGuard {
    _guard: crate::prism_paths::TestPrismHomeOverrideGuard,
}

impl PrismHomeEnvGuard {
    fn set(path: &PathBuf) -> Self {
        Self {
            _guard: crate::prism_paths::set_test_prism_home_override(path),
        }
    }
}

fn background_worker_test_guard() -> MutexGuard<'static, ()> {
    BACKGROUND_WORKER_TEST_LOCK
        .lock()
        .expect("background worker test lock poisoned")
}

fn track_temp_dir(path: &std::path::Path) {
    TEMP_TEST_DIRS.with(|state| state.borrow_mut().paths.push(path.to_path_buf()));
}

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
fn workspace_indexer_rebuilds_missing_derived_edges_from_persisted_file_state() {
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

    let mut source_indexer = WorkspaceIndexer::with_store(&root, MemoryStore::default()).unwrap();
    source_indexer.index().unwrap();
    assert!(source_indexer
        .graph()
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Calls));

    let sqlite_path = std::env::temp_dir().join(format!(
        "prism-derived-edge-reload-{}.db",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut store = SqliteStore::open(&sqlite_path).unwrap();
    for (path, _) in source_indexer.graph().file_records() {
        store.save_file_state(path, source_indexer.graph()).unwrap();
    }
    store.finalize(source_indexer.graph()).unwrap();

    let persisted_graph = store.load_graph().unwrap().unwrap();
    assert!(persisted_graph
        .edges
        .iter()
        .all(|edge| edge.kind != EdgeKind::Calls));

    let reloaded = WorkspaceIndexer::with_store(&root, store).unwrap();
    assert!(reloaded
        .graph()
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::Calls));

    let _ = fs::remove_dir_all(root);
    let _ = std::fs::remove_file(&sqlite_path);
    let _ = std::fs::remove_file(sqlite_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(sqlite_path.with_extension("db-shm"));
}

#[test]
fn hydrated_workspace_session_marks_background_refresh_pending() {
    let _guard = background_worker_test_guard();
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
    let recovery = session
        .last_refresh()
        .expect("hydrated session should record startup recovery metadata");
    assert_eq!(recovery.path, "recovery");
    assert!(recovery.workspace_reloaded);
    assert_eq!(recovery.full_rebuild_count, 0);
    assert!(recovery.loaded_bytes > 0);
    assert!(recovery.replay_volume > 0);
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
                execution_context: None,
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
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

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

    let cache_db = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
    assert!(cache_db.exists());

    let second = WorkspaceIndexer::new(&root).unwrap();
    assert!(second
        .graph()
        .nodes_by_name("alpha")
        .into_iter()
        .any(|node| node.id.path.ends_with("::alpha")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn migrates_legacy_repo_local_cache_db_to_state_db() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".prism")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let legacy_cache = root.join(".prism").join("cache.db");
    SqliteStore::open(&legacy_cache).unwrap();
    assert!(legacy_cache.exists());

    let indexer = WorkspaceIndexer::new(&root).unwrap();
    let state_db = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
    assert!(state_db.exists());
    assert!(!legacy_cache.exists());
    drop(indexer);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn prism_paths_respect_prism_home_override_and_write_metadata_manifests() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let root = temp_workspace();
    let prism_home = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&prism_home).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();

    let _env = PrismHomeEnvGuard::set(&prism_home);

    let paths = PrismPaths::for_workspace_root(&root).unwrap();
    let shared_runtime_db = paths.shared_runtime_db_path().unwrap();
    let worktree_cache_db = paths.worktree_cache_db_path().unwrap();
    let credentials_path = paths.credentials_path().unwrap();
    let repo_metadata_path = paths.repo_home_dir().join("repo.json");
    let worktree_metadata_path = paths.worktree_dir().join("worktree.json");

    assert!(shared_runtime_db.starts_with(&prism_home));
    assert!(worktree_cache_db.starts_with(&prism_home));
    assert_eq!(credentials_path, prism_home.join("credentials.toml"));
    assert!(repo_metadata_path.exists());
    assert!(worktree_metadata_path.exists());

    let repo_metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&repo_metadata_path).unwrap()).unwrap();
    let worktree_metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&worktree_metadata_path).unwrap()).unwrap();
    let canonical_root = root.canonicalize().unwrap();

    assert_eq!(repo_metadata["version"], json!(1));
    assert!(repo_metadata["repo_id"]
        .as_str()
        .unwrap()
        .starts_with("repo:"));
    assert_eq!(repo_metadata["locator_kind"], json!("canonical_root"));
    assert_eq!(
        repo_metadata["locator_path"],
        json!(canonical_root.to_string_lossy().to_string())
    );
    assert_eq!(
        repo_metadata["canonical_root_hint"],
        json!(canonical_root.to_string_lossy().to_string())
    );
    assert_eq!(worktree_metadata["version"], json!(1));
    assert_eq!(
        worktree_metadata["repo_id"], repo_metadata["repo_id"],
        "repo and worktree metadata should share repo identity"
    );
    assert!(worktree_metadata["worktree_id"]
        .as_str()
        .unwrap()
        .starts_with("worktree:"));
    assert_eq!(
        worktree_metadata["canonical_root"],
        json!(canonical_root.to_string_lossy().to_string())
    );
    assert!(worktree_metadata["created_at"].as_u64().is_some());
    assert!(worktree_metadata["last_seen_at"].as_u64().is_some());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(prism_home);
}

#[test]
fn prism_paths_default_test_home_is_temporary_and_cleans_up_after_thread_exit() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let root = temp_workspace();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();

    let home_root = thread::spawn({
        let root = root.clone();
        move || {
            let paths = PrismPaths::for_workspace_root(&root).unwrap();
            let _ = paths.shared_runtime_db_path().unwrap();
            let home_root = paths.home_root().to_path_buf();
            assert!(home_root.starts_with(std::env::temp_dir()));
            if let Some(home) = std::env::var_os("HOME") {
                assert_ne!(home_root, PathBuf::from(home).join(".prism"));
            }
            assert!(paths.repo_home_dir().exists());
            home_root
        }
    })
    .join()
    .expect("temp home worker should finish");

    assert!(
        !home_root.exists(),
        "thread-local default PRISM_HOME should be removed after the test thread exits"
    );

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

    let session = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            shared_runtime: SharedRuntimeBackend::Disabled,
            ..WorkspaceSessionOptions::default()
        },
    )
    .unwrap();
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
            actor: None,
            execution_context: None,
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

    let session = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            shared_runtime: SharedRuntimeBackend::Disabled,
            ..WorkspaceSessionOptions::default()
        },
    )
    .unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let entry = session
        .append_validation_feedback(ValidationFeedbackRecord {
            task_id: Some("task:feedback".to_string()),
            actor: None,
            execution_context: None,
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
            execution_context: None,
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
fn mutate_coordination_with_wait_succeeds_after_refresh_lock_releases() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let refresh_lock = Arc::clone(&session.refresh_lock);
    let holder = thread::spawn(move || {
        let _guard = refresh_lock
            .lock()
            .expect("workspace refresh lock poisoned");
        thread::sleep(Duration::from_millis(75));
    });

    let started = std::time::Instant::now();
    let result = session
        .mutate_coordination_with_session_wait_observed(
            None,
            |_| Ok::<_, anyhow::Error>(()),
            |_operation, _duration, _args, _success, _error| {},
        )
        .unwrap();
    holder.join().expect("lock holder should finish");

    assert!(result.is_some());
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "bounded-wait coordination mutation should wait for the refresh lock"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn coordination_mutation_updates_published_plans_without_reloading_full_projection() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let plan_id = session
        .mutate_coordination_with_session_wait_observed(
            None,
            |prism| {
                prism.create_native_plan(
                    EventMeta {
                        id: EventId::new("coordination:delta-plan"),
                        ts: 1,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:delta-plan")),
                        causation: None,
                        execution_context: None,
                    },
                    "Exercise incremental published-plan sync".into(),
                    "Exercise incremental published-plan sync".into(),
                    None,
                    Some(Default::default()),
                )
            },
            |_operation, _duration, _args, _success, _error| {},
        )
        .unwrap()
        .expect("coordination mutation should acquire the refresh lock");

    let mut operations = Vec::new();
    session
        .mutate_coordination_with_session_wait_observed(
            None,
            |prism| {
                prism.update_native_plan(
                    EventMeta {
                        id: EventId::new("coordination:delta-plan-update"),
                        ts: 2,
                        actor: EventActor::Agent,
                        correlation: Some(TaskId::new("task:delta-plan")),
                        causation: None,
                        execution_context: None,
                    },
                    &plan_id,
                    None,
                    Some(prism_ir::PlanStatus::Active),
                    Some("Exercise incremental published-plan sync again".into()),
                    None,
                )
            },
            |operation, _duration, _args, _success, _error| {
                operations.push(operation.to_string());
            },
        )
        .unwrap()
        .expect("coordination mutation should acquire the refresh lock");

    assert!(operations.contains(&"mutation.coordination.syncPublishedPlans".to_string()));
    assert!(operations.contains(&"mutation.coordination.publishedPlans.writeLogs".to_string()));
    assert!(
        !operations.contains(&"mutation.coordination.publishedPlans.loadProjection".to_string()),
        "incremental coordination mutation should reuse in-memory plan state instead of replaying all published plan logs"
    );

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
fn repo_patch_events_capture_provenance_and_reload_without_local_cache() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    *session
        .worktree_principal_binding
        .lock()
        .expect("worktree principal binding lock poisoned") =
        Some(crate::worktree_principal::BoundWorktreePrincipal {
            authority_id: "local-daemon".to_string(),
            principal_id: "agent:patch-provenance".to_string(),
            principal_name: "patch-provenance".to_string(),
        });
    session.bind_active_work_context(crate::ActiveWorkContextBinding {
        work_id: "work:rename-alpha".to_string(),
        kind: WorkContextKind::AdHoc,
        title: "Rename alpha".to_string(),
        summary: Some("rename alpha to beta".to_string()),
        parent_work_id: None,
        coordination_task_id: Some("task:rename-alpha".to_string()),
        plan_id: None,
        plan_title: None,
    });

    fs::write(root.join("src/lib.rs"), "pub fn beta() {}\n").unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([root.join("src/lib.rs")]);
    let observed = session.refresh_fs().unwrap();
    assert!(!observed.is_empty());

    let repo_events = load_repo_patch_events(&root).unwrap();
    assert_eq!(repo_events.len(), 1);
    let event = &repo_events[0];
    assert_eq!(event.kind, OutcomeKind::PatchApplied);
    assert_eq!(event.result, OutcomeResult::Success);
    let EventActor::Principal(actor) = &event.meta.actor else {
        panic!("expected principal actor");
    };
    assert_eq!(
        actor.authority_id,
        PrincipalAuthorityId::new("local-daemon")
    );
    assert_eq!(
        actor.principal_id,
        PrincipalId::new("agent:patch-provenance")
    );
    assert_eq!(actor.name.as_deref(), Some("patch-provenance"));
    assert_eq!(
        event.meta.correlation,
        Some(TaskId::new("task:rename-alpha"))
    );
    let work = event
        .meta
        .execution_context
        .as_ref()
        .and_then(|context| context.work_context.as_ref())
        .expect("work context should be present");
    assert_eq!(work.work_id, "work:rename-alpha");
    assert_eq!(work.title, "Rename alpha");
    assert_eq!(
        work.coordination_task_id.as_deref(),
        Some("task:rename-alpha")
    );
    assert_eq!(
        event.metadata["reason"].as_str(),
        Some("work Rename alpha (work:rename-alpha)")
    );
    assert!(event.metadata["filePaths"][0]
        .as_str()
        .unwrap_or_default()
        .ends_with("src/lib.rs"));
    let changes_doc = fs::read_to_string(root.join("docs/prism/changes.md")).unwrap();
    assert!(changes_doc.contains("# PRISM Changes"));
    assert!(changes_doc.contains("Rename alpha"));
    assert!(changes_doc.contains("src/lib.rs"));

    let cache_db = crate::util::cache_path(&root).unwrap();
    drop(session);
    let _ = fs::remove_file(cache_db);

    let reloaded = index_workspace_session(&root).unwrap();
    let reloaded_events = reloaded
        .prism()
        .outcome_memory()
        .snapshot()
        .events
        .into_iter()
        .filter(|event| event.kind == OutcomeKind::PatchApplied)
        .collect::<Vec<_>>();
    assert_eq!(reloaded_events.len(), 1);
    assert_eq!(
        reloaded_events[0].metadata["reason"].as_str(),
        Some("work Rename alpha (work:rename-alpha)")
    );
    let EventActor::Principal(reloaded_actor) = &reloaded_events[0].meta.actor else {
        panic!("expected reloaded principal actor");
    };
    assert_eq!(
        reloaded_actor.principal_id,
        PrincipalId::new("agent:patch-provenance")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn binding_principal_backfills_repo_patch_events_for_active_work() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    session.bind_active_work_context(crate::ActiveWorkContextBinding {
        work_id: "work:late-auth".to_string(),
        kind: WorkContextKind::AdHoc,
        title: "Late auth publish".to_string(),
        summary: Some("bind principal after edits".to_string()),
        parent_work_id: None,
        coordination_task_id: None,
        plan_id: None,
        plan_title: None,
    });

    fs::write(root.join("src/lib.rs"), "pub fn beta() {}\n").unwrap();
    session
        .refresh_state
        .mark_fs_dirty_paths([root.join("src/lib.rs")]);
    let observed = session.refresh_fs().unwrap();
    assert!(!observed.is_empty());

    let runtime_patch = session
        .prism()
        .query_outcomes(&OutcomeRecallQuery {
            kinds: Some(vec![OutcomeKind::PatchApplied]),
            result: Some(OutcomeResult::Success),
            limit: 1,
            ..OutcomeRecallQuery::default()
        })
        .into_iter()
        .next()
        .expect("patch event should exist");
    assert!(matches!(runtime_patch.meta.actor, EventActor::System));
    assert_eq!(
        runtime_patch.metadata["reason"].as_str(),
        Some("work Late auth publish (work:late-auth)")
    );
    assert!(load_repo_patch_events(&root).unwrap().is_empty());

    let owner = session
        .bootstrap_owner_principal(BootstrapOwnerInput {
            authority_id: Some(PrincipalAuthorityId::new("local-daemon")),
            name: "Late Auth Owner".to_string(),
            role: Some("repo_owner".to_string()),
        })
        .unwrap();
    let authenticated = session
        .authenticate_principal_credential(&owner.credential.credential_id, &owner.principal_token)
        .unwrap();
    session
        .bind_or_validate_worktree_principal(&authenticated)
        .unwrap();

    let repo_events = load_repo_patch_events(&root).unwrap();
    assert_eq!(repo_events.len(), 1);
    let repo_event = &repo_events[0];
    let EventActor::Principal(actor) = &repo_event.meta.actor else {
        panic!("repo event should be principal-authored");
    };
    assert_eq!(actor.authority_id, owner.principal.authority_id);
    assert_eq!(actor.principal_id, owner.principal.principal_id);
    assert_eq!(actor.name.as_deref(), Some(owner.principal.name.as_str()));
    assert_eq!(
        repo_event.metadata["reason"].as_str(),
        Some("work Late auth publish (work:late-auth)")
    );

    let live_event = session
        .load_outcome_event(&repo_event.meta.id)
        .unwrap()
        .expect("live runtime should expose rewritten patch event");
    let EventActor::Principal(live_actor) = &live_event.meta.actor else {
        panic!("live event should be principal-authored after backfill");
    };
    assert_eq!(live_actor.principal_id, owner.principal.principal_id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn fs_watch_refresh_enqueues_curator_with_patch_outcomes_and_projection_context() {
    let _guard = background_worker_test_guard();
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
                execution_context: None,
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
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();

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
                    execution_context: None,
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
    let failure_query = OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha.clone())],
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        limit: 10,
        ..OutcomeRecallQuery::default()
    };
    assert!(reloaded
        .load_hot_outcomes(&failure_query)
        .unwrap()
        .is_empty());
    assert!(reloaded
        .load_cold_outcomes(&failure_query)
        .unwrap()
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:cold:0")));
    assert!(reloaded
        .prism()
        .query_hot_outcomes(&failure_query)
        .is_empty());
    assert!(reloaded
        .prism()
        .query_cold_outcomes(&failure_query)
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:cold:0")));

    let failures = reloaded.load_outcomes(&failure_query).unwrap();
    assert!(failures
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:cold:0")));

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
fn reload_bounds_hot_outcomes_from_authoritative_journal_without_checkpoint_flush() {
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
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new(format!("outcome:journal-bound:{idx}")),
                    ts: u64::try_from(idx + 1).unwrap(),
                    actor: EventActor::Agent,
                    correlation: None,
                    causation: None,
                    execution_context: None,
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
                summary: format!("journal event {idx}"),
                evidence: Vec::new(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();
    }

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    assert!(
        reloaded.prism().outcome_snapshot().events.len()
            <= crate::session::HOT_OUTCOME_HYDRATION_LIMIT
    );

    let failure_query = OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha.clone())],
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        limit: 10,
        ..OutcomeRecallQuery::default()
    };
    assert!(reloaded
        .load_hot_outcomes(&failure_query)
        .unwrap()
        .is_empty());
    assert!(reloaded
        .load_cold_outcomes(&failure_query)
        .unwrap()
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:journal-bound:0")));
    assert!(reloaded
        .load_outcomes(&failure_query)
        .unwrap()
        .iter()
        .any(|event| event.meta.id == EventId::new("outcome:journal-bound:0")));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn persist_outcomes_flushes_checkpoint_materialization() {
    let _guard = background_worker_test_guard();
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
                execution_context: None,
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
    let hot_event = OutcomeEvent {
        meta: EventMeta {
            id: event_id.clone(),
            ts: 1,
            actor: EventActor::Agent,
            correlation: Some(task_id.clone()),
            causation: None,
            execution_context: None,
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
        .store_event(hot_event.clone())
        .unwrap();
    let cold_event = OutcomeEvent {
        meta: EventMeta {
            id: EventId::new("outcome:cold-session"),
            ts: 2,
            actor: EventActor::Agent,
            correlation: Some(task_id.clone()),
            causation: None,
            execution_context: None,
        },
        anchors: vec![AnchorRef::Node(alpha.clone())],
        kind: OutcomeKind::FailureObserved,
        result: OutcomeResult::Failure,
        summary: "cold only failure".into(),
        evidence: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    session
        .store
        .lock()
        .unwrap()
        .append_outcome_events(std::slice::from_ref(&cold_event), &[])
        .unwrap();

    let hot_replay = session.load_hot_task_replay(&task_id).unwrap();
    assert_eq!(hot_replay.task, task_id);
    assert_eq!(hot_replay.events, vec![hot_event.clone()]);

    let cold_replay = session.load_cold_task_replay(&task_id).unwrap();
    assert_eq!(cold_replay.task, task_id);
    assert_eq!(cold_replay.events, vec![cold_event.clone()]);

    let replay = session.load_task_replay(&task_id).unwrap();
    assert_eq!(replay.task, task_id);
    assert_eq!(replay.events.len(), 2);
    assert_eq!(replay.events[0].meta.id, cold_event.meta.id);
    assert_eq!(replay.events[1].meta.id, hot_event.meta.id);

    let query = OutcomeRecallQuery {
        anchors: vec![AnchorRef::Node(alpha)],
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        result: Some(OutcomeResult::Failure),
        limit: 10,
        ..OutcomeRecallQuery::default()
    };
    assert_eq!(
        session.load_hot_outcomes(&query).unwrap(),
        vec![hot_event.clone()]
    );
    assert_eq!(
        session.load_cold_outcomes(&query).unwrap(),
        vec![cold_event.clone()]
    );
    let loaded = session.load_outcomes(&query).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].meta.id, cold_event.meta.id);
    assert_eq!(loaded[1].meta.id, hot_event.meta.id);

    assert_eq!(
        session.load_hot_outcome_event(&event_id).unwrap(),
        Some(hot_event.clone())
    );
    assert_eq!(session.load_cold_outcome_event(&event_id).unwrap(), None);
    assert_eq!(
        session
            .load_cold_outcome_event(&cold_event.meta.id)
            .unwrap(),
        Some(cold_event.clone())
    );
    assert_eq!(
        session.load_hot_outcome_event(&cold_event.meta.id).unwrap(),
        None
    );
    assert_eq!(
        session.load_outcome_event(&event_id).unwrap(),
        Some(hot_event)
    );
    assert_eq!(
        session.load_outcome_event(&cold_event.meta.id).unwrap(),
        Some(cold_event)
    );

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
            execution_context: None,
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
    assert!(reloaded
        .load_hot_lineage_history(&lineage)
        .unwrap()
        .is_empty());
    assert_eq!(
        reloaded.load_cold_lineage_history(&lineage).unwrap(),
        vec![persisted_event.clone()]
    );
    assert!(reloaded.prism().hot_lineage_history(&lineage).is_empty());
    assert_eq!(
        reloaded.prism().cold_lineage_history(&lineage),
        vec![persisted_event.clone()]
    );
    let events = reloaded.load_lineage_history(&lineage).unwrap();
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
fn repo_memory_reads_do_not_lazy_import_new_repo_events_after_startup() {
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

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "published after startup");
    entry.id = MemoryId("structural:post-startup".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Repo;
    entry.source = MemorySource::User;
    entry.trust = 0.9;

    append_repo_memory_event(
        &root,
        &MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            entry.clone(),
            Some("task:post-startup".to_string()),
            Vec::new(),
            Vec::new(),
        ),
    )
    .unwrap();

    let live_events = session
        .memory_events(&MemoryEventQuery {
            memory_id: Some(entry.id.clone()),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Promoted]),
            scope: Some(MemoryScope::Repo),
            task_id: Some("task:post-startup".to_string()),
            since: None,
        })
        .unwrap();
    assert!(
        live_events.is_empty(),
        "read paths should not lazily import newly published repo memory"
    );
    assert_eq!(session.load_episodic_snapshot().unwrap(), None);

    let reloaded = index_workspace_session(&root).unwrap();
    let reloaded_events = reloaded
        .memory_events(&MemoryEventQuery {
            memory_id: Some(entry.id.clone()),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Promoted]),
            scope: Some(MemoryScope::Repo),
            task_id: Some("task:post-startup".to_string()),
            since: None,
        })
        .unwrap();
    assert_eq!(reloaded_events.len(), 1);
    let reloaded_snapshot = reloaded
        .load_episodic_snapshot()
        .unwrap()
        .expect("bootstrap should hydrate repo memory after restart");
    assert!(reloaded_snapshot
        .entries
        .iter()
        .any(|candidate| candidate.id == entry.id));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn protected_state_watcher_imports_repo_memory_without_source_refresh() {
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
    let observed_before = session.observed_fs_revision();
    let applied_before = session.applied_fs_revision();

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "watched protected memory");
    entry.id = MemoryId("structural:watched-protected-memory".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Repo;
    entry.source = MemorySource::User;
    entry.trust = 0.9;
    let event = MemoryEvent::from_entry(
        MemoryEventKind::Promoted,
        entry.clone(),
        Some("task:watched-protected-memory".to_string()),
        Vec::new(),
        Vec::new(),
    );
    append_repo_memory_event(&root, &event).unwrap();

    crate::watch::sync_protected_state_watch_update(
        &root,
        &session.published_generation,
        &session.runtime_state,
        &session.store,
        &session.cold_query_store,
        session.shared_runtime_store.as_ref(),
        session.shared_runtime.sqlite_path(),
        &session.refresh_lock,
        &session.loaded_workspace_revision,
        session.coordination_enabled,
        &[
            crate::protected_state::streams::ProtectedRepoStream::memory_stream("events.jsonl")
                .expect("memory stream should classify"),
        ],
    )
    .unwrap();

    let events = session
        .memory_events(&MemoryEventQuery {
            memory_id: Some(entry.id.clone()),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Promoted]),
            scope: Some(MemoryScope::Repo),
            task_id: Some("task:watched-protected-memory".to_string()),
            since: None,
        })
        .unwrap();
    assert!(
        !events.is_empty(),
        "protected-state sync should import repo memory"
    );
    assert_eq!(session.observed_fs_revision(), observed_before);
    assert_eq!(session.applied_fs_revision(), applied_before);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn protected_state_watcher_imports_repo_concepts_without_source_refresh() {
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
    let observed_before = session.observed_fs_revision();
    let applied_before = session.applied_fs_revision();

    crate::concept_events::append_repo_concept_event(
        &root,
        &ConceptEvent {
            id: "concept-event:watched-protected-concept".to_string(),
            recorded_at: 19,
            task_id: Some("task:watched-protected-concept".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://watched_alpha_flow".to_string(),
                canonical_name: "watched_alpha_flow".to_string(),
                summary: "Curated alpha concept imported by the protected-state watcher."
                    .to_string(),
                aliases: vec!["alpha".to_string(), "alpha flow".to_string()],
                confidence: 0.94,
                core_members: vec![alpha.clone(), beta.clone()],
                core_member_lineages: vec![
                    session.prism().lineage_of(&alpha),
                    session.prism().lineage_of(&beta),
                ],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["protected-state watcher test".to_string()],
                risk_hint: None,
                decode_lenses: vec![ConceptDecodeLens::Open],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "protected_state_watch".to_string(),
                    task_id: Some("task:watched-protected-concept".to_string()),
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
        },
    )
    .unwrap();

    crate::watch::sync_protected_state_watch_update(
        &root,
        &session.published_generation,
        &session.runtime_state,
        &session.store,
        &session.cold_query_store,
        session.shared_runtime_store.as_ref(),
        session.shared_runtime.sqlite_path(),
        &session.refresh_lock,
        &session.loaded_workspace_revision,
        session.coordination_enabled,
        &[crate::protected_state::streams::ProtectedRepoStream::concept_events()],
    )
    .unwrap();

    assert!(
        session
            .prism()
            .concept_by_handle("concept://watched_alpha_flow")
            .is_some(),
        "protected-state sync should import repo concepts into the live runtime"
    );
    assert_eq!(session.observed_fs_revision(), observed_before);
    assert_eq!(session.applied_fs_revision(), applied_before);

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
            actor: None,
            execution_context: None,
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
fn tampered_repo_concept_stream_is_rejected_on_reload() {
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
        .next()
        .expect("alpha should be indexed")
        .id()
        .clone();
    session
        .append_concept_event(ConceptEvent {
            id: "concept-event:repo-tampered".to_string(),
            recorded_at: 17,
            task_id: Some("task:repo-concept-tampered".to_string()),
            actor: None,
            execution_context: None,
            action: ConceptEventAction::Promote,
            patch: None,
            concept: ConceptPacket {
                handle: "concept://signed_tampered_alpha".to_string(),
                canonical_name: "signed_tampered_alpha".to_string(),
                summary: "Signed concept summary for tamper coverage.".to_string(),
                aliases: vec!["alpha".to_string()],
                confidence: 0.91,
                core_members: vec![alpha.clone(), alpha.clone()],
                core_member_lineages: vec![prism.lineage_of(&alpha), prism.lineage_of(&alpha)],
                supporting_members: Vec::new(),
                supporting_member_lineages: Vec::new(),
                likely_tests: Vec::new(),
                likely_test_lineages: Vec::new(),
                evidence: vec!["Signed publication".to_string()],
                risk_hint: None,
                decode_lenses: vec![ConceptDecodeLens::Open],
                scope: ConceptScope::Repo,
                provenance: ConceptProvenance {
                    origin: "test".to_string(),
                    kind: "repo_concept_tamper".to_string(),
                    task_id: Some("task:repo-concept-tampered".to_string()),
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
    let tampered = fs::read_to_string(&repo_log).unwrap().replace(
        "Signed concept summary for tamper coverage.",
        "Tampered concept summary for tamper coverage.",
    );
    fs::write(&repo_log, tampered).unwrap();

    let error = match index_workspace_session(&root) {
        Ok(_) => panic!("tampered concept stream should not reload"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Tampered") || error.contains("signature verification"),
        "expected tamper detection error, got: {error}"
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
        hydrate_persisted_co_change: true,
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
            actor: None,
            execution_context: None,
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
fn shared_runtime_sqlite_shares_memory_events_without_checkpoint_flush() {
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
        hydrate_persisted_co_change: true,
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

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "shared event journal memory");
    entry.id = MemoryId("memory:shared-runtime-journal".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Session;
    entry.source = MemorySource::User;
    entry.trust = 0.9;
    session_one
        .append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Stored,
            entry.clone(),
            Some("task:shared-runtime-journal".to_string()),
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();
    drop(session_one);

    let session_two = index_workspace_session_with_options(&root_two, options).unwrap();
    let events = session_two
        .memory_events(&MemoryEventQuery {
            memory_id: Some(entry.id.clone()),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Stored]),
            scope: Some(MemoryScope::Session),
            task_id: Some("task:shared-runtime-journal".to_string()),
            since: None,
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]
            .entry
            .as_ref()
            .map(|candidate| candidate.id.clone()),
        Some(entry.id)
    );
    let snapshot = session_two
        .load_episodic_snapshot()
        .unwrap()
        .expect("shared runtime should rebuild episodic view from authoritative memory events");
    assert!(snapshot
        .entries
        .iter()
        .any(|candidate| candidate.content == "shared event journal memory"));

    let _ = fs::remove_dir_all(root_one);
    let _ = fs::remove_dir_all(root_two);
    let _ = fs::remove_dir_all(shared_runtime_root);
}

#[test]
fn shared_runtime_sqlite_shares_principal_registry_across_workspaces() {
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
        hydrate_persisted_co_change: true,
    };
    let session_one = index_workspace_session_with_options(&root_one, options.clone()).unwrap();
    let snapshot = PrincipalRegistrySnapshot {
        principals: vec![PrincipalProfile {
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:agent-one".into()),
            kind: PrincipalKind::Agent,
            name: "Agent One".to_string(),
            role: Some("coordination_worker".to_string()),
            status: PrincipalStatus::Active,
            created_at: 101,
            updated_at: 102,
            parent_principal_id: Some(PrincipalId("principal:human-root".into())),
            profile: json!({
                "session": "one",
            }),
        }],
        credentials: vec![CredentialRecord {
            credential_id: CredentialId("credential:agent-one".into()),
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:agent-one".into()),
            token_verifier: "verifier:agent-one".to_string(),
            capabilities: vec![
                CredentialCapability::MutateCoordination,
                CredentialCapability::MutateRepoMemory,
            ],
            status: CredentialStatus::Active,
            created_at: 103,
            last_used_at: Some(104),
            revoked_at: None,
        }],
    };
    session_one.persist_principal_registry(&snapshot).unwrap();
    drop(session_one);

    let session_two = index_workspace_session_with_options(&root_two, options).unwrap();
    assert_eq!(
        session_two.load_principal_registry().unwrap(),
        Some(snapshot)
    );

    let _ = fs::remove_dir_all(root_one);
    let _ = fs::remove_dir_all(root_two);
    let _ = fs::remove_dir_all(shared_runtime_root);
}

#[test]
fn bootstrap_owner_and_mint_child_principal_round_trip_through_shared_runtime_registry() {
    let shared_runtime_root = temp_workspace();
    let shared_runtime_sqlite = shared_runtime_root.join("shared-runtime.db");
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            coordination: false,
            shared_runtime: SharedRuntimeBackend::Sqlite {
                path: shared_runtime_sqlite.clone(),
            },
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: true,
        },
    )
    .unwrap();

    let owner = session
        .bootstrap_owner_principal(BootstrapOwnerInput {
            authority_id: Some(PrincipalAuthorityId::new("local-daemon")),
            name: "Owner".to_string(),
            role: Some("repo_owner".to_string()),
        })
        .unwrap();
    assert_eq!(owner.principal.kind, PrincipalKind::Human);
    assert_eq!(
        owner.credential.capabilities,
        vec![CredentialCapability::All]
    );

    let snapshot = session.load_principal_registry().unwrap().unwrap();
    assert_eq!(snapshot.principals.len(), 1);
    assert_eq!(snapshot.credentials.len(), 1);

    let authenticated = session
        .authenticate_principal_credential(&owner.credential.credential_id, &owner.principal_token)
        .unwrap();
    assert_eq!(
        authenticated.principal.principal_id,
        owner.principal.principal_id
    );

    let child = session
        .mint_principal_credential(
            &authenticated,
            MintPrincipalRequest {
                authority_id: Some(PrincipalAuthorityId::new("local-daemon")),
                kind: PrincipalKind::Agent,
                name: "Worker".to_string(),
                role: Some("coordination_worker".to_string()),
                parent_principal_id: Some(owner.principal.principal_id.clone()),
                capabilities: vec![CredentialCapability::MutateCoordination],
                profile: json!({ "lane": "coordination" }),
            },
        )
        .unwrap();
    assert_eq!(
        child.principal.parent_principal_id,
        Some(owner.principal.principal_id.clone())
    );
    assert_eq!(
        child.credential.capabilities,
        vec![CredentialCapability::MutateCoordination]
    );

    let updated_snapshot = session.load_principal_registry().unwrap().unwrap();
    assert_eq!(updated_snapshot.principals.len(), 2);
    assert_eq!(updated_snapshot.credentials.len(), 2);
    assert!(updated_snapshot.credentials.iter().any(|credential| {
        credential.credential_id == owner.credential.credential_id
            && credential.last_used_at.is_some()
    }));
    assert!(updated_snapshot.principals.iter().any(|principal| {
        principal.principal_id == child.principal.principal_id
            && principal.parent_principal_id == Some(owner.principal.principal_id.clone())
    }));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared_runtime_root);
}

#[test]
fn default_workspace_session_uses_repo_shared_runtime_registry() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();
    let prism_home = temp_workspace();
    let _home = PrismHomeEnvGuard::set(&prism_home);
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let snapshot = PrincipalRegistrySnapshot {
        principals: vec![PrincipalProfile {
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:default-runtime".into()),
            kind: PrincipalKind::Human,
            name: "Default Runtime".to_string(),
            role: Some("owner".to_string()),
            status: PrincipalStatus::Active,
            created_at: 101,
            updated_at: 102,
            parent_principal_id: None,
            profile: json!({ "source": "default" }),
        }],
        credentials: vec![CredentialRecord {
            credential_id: CredentialId("credential:default-runtime".into()),
            authority_id: PrincipalAuthorityId("authority:test".into()),
            principal_id: PrincipalId("principal:default-runtime".into()),
            token_verifier: "verifier:default-runtime".to_string(),
            capabilities: vec![CredentialCapability::All],
            status: CredentialStatus::Active,
            created_at: 103,
            last_used_at: Some(104),
            revoked_at: None,
        }],
    };
    session.persist_principal_registry(&snapshot).unwrap();
    drop(session);

    let reloaded = hydrate_workspace_session(&root).unwrap();
    assert_eq!(reloaded.load_principal_registry().unwrap(), Some(snapshot));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(prism_home);
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
            actor: None,
            execution_context: None,
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
            actor: None,
            execution_context: None,
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
    let memory_doc = fs::read_to_string(root.join("docs/prism/memory.md")).unwrap();
    let changes_doc = fs::read_to_string(root.join("docs/prism/changes.md")).unwrap();
    let plans_doc = fs::read_to_string(root.join("docs/prism/plans/index.md")).unwrap();
    assert!(prism_doc.contains("# PRISM"));
    assert!(prism_doc.contains("## Projection Metadata"));
    assert!(prism_doc.contains("- Projection class: `published`"));
    assert!(prism_doc.contains("- Authority planes: `published_repo`"));
    assert!(prism_doc.contains("- Projection version: `1`"));
    assert!(prism_doc.contains("- Source head: `sha256:"));
    assert!(prism_doc.contains("## How to Read This Repo"));
    assert!(prism_doc.contains("docs/prism/concepts.md"));
    assert!(prism_doc.contains("docs/prism/relations.md"));
    assert!(prism_doc.contains("docs/prism/contracts.md"));
    assert!(prism_doc.contains("docs/prism/memory.md"));
    assert!(prism_doc.contains("docs/prism/changes.md"));
    assert!(prism_doc.contains("docs/prism/plans/index.md"));
    assert!(prism_doc.contains("- Active repo concepts: 1"));
    assert!(concepts_doc.contains("# PRISM Concepts"));
    assert!(concepts_doc.contains("## Projection Metadata"));
    assert!(concepts_doc.contains("`alpha_flow` (`concept://alpha_flow`)"));
    assert!(concepts_doc.contains("Explains how alpha delegates work into beta."));
    assert!(concepts_doc.contains("### Core Members"));
    assert!(concepts_doc.contains("demo::alpha"));
    assert!(concepts_doc.contains("### Supporting Members"));
    assert!(concepts_doc.contains("demo::gamma"));
    assert!(concepts_doc.contains("### Risk Hint"));
    assert!(relations_doc.contains("# PRISM Relations"));
    assert!(relations_doc.contains("## Projection Metadata"));
    assert!(contracts_doc.contains("# PRISM Contracts"));
    assert!(contracts_doc.contains("## Projection Metadata"));
    assert!(contracts_doc.contains("No active repo-scoped contracts are currently published."));
    assert!(memory_doc.contains("# PRISM Memory"));
    assert!(memory_doc.contains("No active repo-scoped memories are currently published."));
    assert!(changes_doc.contains("# PRISM Changes"));
    assert!(changes_doc.contains("No repo-scoped patch events are currently published."));
    assert!(plans_doc.contains("# PRISM Plans"));
    assert!(plans_doc.contains("No repo-scoped plans are currently published."));

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
                actor: None,
                execution_context: None,
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
            actor: None,
            execution_context: None,
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
    assert!(prism_doc.contains("- Source snapshot: `2` concepts, `1` relations, `0` contracts"));
    assert!(relations_doc.contains("# PRISM Relations"));
    assert!(relations_doc.contains("- Source logical timestamp: `37`"));
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
            actor: None,
            execution_context: None,
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
    assert!(contracts_doc.contains("- Source logical timestamp: `43`"));
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
fn repo_memory_events_auto_sync_prism_doc() {
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

    let mut entry = MemoryEntry::new(
        MemoryKind::Structural,
        "Alpha ownership is published repo memory for generated docs.",
    );
    entry.id = MemoryId("memory:alpha-owner".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Repo;
    entry.source = MemorySource::User;
    entry.trust = 0.95;
    entry.metadata = json!({
        "provenance": {
            "origin": "test",
            "kind": "repo_memory_prism_doc",
        },
        "publication": {
            "publishedAt": 51,
            "lastReviewedAt": 51,
            "status": "active",
        }
    });
    session
        .append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Promoted,
            entry,
            Some("task:repo-memory-prism-doc".to_string()),
            vec![MemoryId("memory:source".to_string())],
            Vec::new(),
        ))
        .unwrap();

    let prism_doc = fs::read_to_string(root.join("PRISM.md")).unwrap();
    let memory_doc = fs::read_to_string(root.join("docs/prism/memory.md")).unwrap();
    assert!(prism_doc.contains("- Active repo memories: 1"));
    assert!(prism_doc.contains("docs/prism/memory.md"));
    assert!(memory_doc.contains("# PRISM Memory"));
    assert!(memory_doc.contains("memory:alpha-owner"));
    assert!(memory_doc.contains("Alpha ownership is published repo memory for generated docs."));
    assert!(memory_doc.contains("kind: `repo_memory_prism_doc`"));

    let sync = session.sync_prism_doc().unwrap();
    assert_eq!(sync.status, PrismDocSyncStatus::Unchanged);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_plan_events_auto_sync_prism_doc() {
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
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:repo-plan-prism-doc"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:repo-plan-prism-doc")),
                    causation: None,
                    execution_context: None,
                },
                "Project repo state docs".into(),
                "Ship generated repo state docs".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();

    let prism_doc = fs::read_to_string(root.join("PRISM.md")).unwrap();
    let plans_doc = fs::read_to_string(root.join("docs/prism/plans/index.md")).unwrap();
    assert!(prism_doc.contains("- Published plans: 1"));
    assert!(prism_doc.contains("docs/prism/plans/index.md"));
    assert!(plans_doc.contains("# PRISM Plans"));
    assert!(plans_doc.contains("[Project repo state docs]"));
    assert!(plans_doc.contains("Ship generated repo state docs"));

    let generated_plan_doc = fs::read_dir(root.join("docs/prism/plans/active"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .expect("plan markdown should be generated");
    let plan_doc = fs::read_to_string(generated_plan_doc).unwrap();
    assert!(plan_doc.contains("# Project repo state docs"));
    assert!(plan_doc.contains("Ship generated repo state docs"));
    assert!(plan_doc.contains("## Goal"));

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
            actor: None,
            execution_context: None,
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
            actor: None,
            execution_context: None,
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
                    execution_context: None,
                },
                "Coordinate rename follow-up".into(),
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
                    execution_context: None,
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
                    execution_context: None,
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
                    execution_context: None,
                },
                "Reload native bindings".into(),
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
                    execution_context: None,
                },
                "Ship published plan hydration".into(),
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
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Hydrate plans from repo state".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(prism_ir::SessionId::new("session:published-plan")),
                    worktree_id: Some("worktree:published-plan".into()),
                    branch_ref: Some("refs/heads/published-plan".into()),
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
        !log_contents.contains("worktree:published-plan"),
        "repo-published plan logs should not persist runtime worktree ids"
    );
    assert!(
        !log_contents.contains("refs/heads/published-plan"),
        "repo-published plan logs should not persist runtime branch refs"
    );
    assert!(
        log_contents.contains("\"kind\":\"plan_created\""),
        "published plan logs should use native plan events"
    );
    assert!(
        log_contents.contains("\"kind\":\"node_added\""),
        "published plan logs should append native node events"
    );
    assert!(
        !log_contents.contains("\"kind\":\"execution_updated\""),
        "published plan logs should not persist runtime execution overlay events"
    );

    drop(session);
    fs::remove_file(
        PrismPaths::for_workspace_root(&root)
            .unwrap()
            .shared_runtime_db_path()
            .unwrap(),
    )
    .unwrap();

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
                    execution_context: None,
                },
                "Published plan should stay mutable".into(),
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
                    execution_context: None,
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Persisted snapshot should remain authoritative".into(),
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
                execution_context: None,
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
                    execution_context: None,
                },
                "Published plan must exist in both runtimes".into(),
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
fn derived_published_plan_mirrors_do_not_override_replayed_task_backed_authored_fields() {
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
                    execution_context: None,
                },
                "Keep replay authoritative".into(),
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
                    execution_context: None,
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
            "Stale published export should now win",
        )
        .replace(
            "Ignore stale export artifacts",
            "Published task title should now win",
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
fn refresh_fs_ignores_external_derived_plan_mirror_edits_without_source_changes() {
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
                    id: EventId::new("coordination:published-authority-live-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-authority-live-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Original authored goal".into(),
                "Original authored goal".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:published-authority-live-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:published-authority-live-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Original authored title".into(),
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
    let edited = fs::read_to_string(&log_path)
        .unwrap()
        .replace("Original authored goal", "Externally edited goal")
        .replace("Original authored title", "Externally edited title");
    fs::write(&log_path, edited).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(300));
    let outcome = session.refresh_fs_with_status().unwrap();
    assert_eq!(outcome.status, crate::session::FsRefreshStatus::Clean);

    let state = session
        .load_coordination_plan_state()
        .unwrap()
        .expect("live coordination plan state");
    assert!(state
        .snapshot
        .plans
        .iter()
        .any(|plan| plan.id == plan_id && plan.goal == "Original authored goal"));
    assert!(state.snapshot.tasks.iter().any(|task| {
        task.id == task_id && task.plan == plan_id && task.title == "Original authored title"
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Exercise backend-neutral coordination persistence".into(),
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
                execution_context: None,
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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Exercise incremental read-model persistence".into(),
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
                execution_context: None,
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
                execution_context: None,
            },
            TaskUpdateInput {
                task_id: task_id.clone(),
                kind: None,
                status: Some(prism_ir::CoordinationTaskStatus::InReview),
                assignee: None,
                session: None,
                worktree_id: None,
                branch_ref: None,
                title: None,
                summary: None,
                anchors: None,
                bindings: None,
                depends_on: None,
                acceptance: None,
                validation_refs: None,
                is_abstract: None,
                base_revision: None,
                priority: None,
                tags: None,
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
                execution_context: None,
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
                execution_context: None,
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
                execution_context: None,
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
fn coordination_session_materializes_read_models_off_request_path() {
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
        .mutate_coordination(|prism| {
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:async-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:async-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Exercise async coordination materialization".into(),
                "Exercise async coordination materialization".into(),
                None,
                Some(Default::default()),
            )?;
            let _ = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:async-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:async-plan")),
                    causation: None,
                    execution_context: None,
                },
                TaskCreateInput {
                    plan_id,
                    title: "Queue coordination read-model persistence".into(),
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
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();

    {
        let mut store = session.store.lock().expect("workspace store lock poisoned");
        assert!(store.load_coordination_read_model().unwrap().is_none());
        assert!(store
            .load_coordination_queue_read_model()
            .unwrap()
            .is_none());
    }

    let live_read_model = session
        .load_coordination_read_model()
        .unwrap()
        .expect("session should derive a live coordination read model");
    assert_eq!(live_read_model.active_plans.len(), 1);
    assert_eq!(live_read_model.task_count, 1);

    let live_queue_model = session
        .load_coordination_queue_read_model()
        .unwrap()
        .expect("session should derive a live coordination queue model");
    assert!(live_queue_model.pending_handoff_tasks.is_empty());

    session.flush_materializations().unwrap();

    let mut shared_runtime_store = SqliteStore::open(
        PrismPaths::for_workspace_root(&root)
            .unwrap()
            .shared_runtime_db_path()
            .unwrap(),
    )
    .unwrap();
    let persisted_read_model = shared_runtime_store
        .load_coordination_read_model()
        .unwrap()
        .expect("persisted coordination read model should materialize after flush");
    assert_eq!(persisted_read_model.active_plans.len(), 1);
    assert_eq!(persisted_read_model.task_count, 1);
    let persisted_queue_model = shared_runtime_store
        .load_coordination_queue_read_model()
        .unwrap()
        .expect("persisted coordination queue model should materialize after flush");
    assert!(persisted_queue_model.pending_handoff_tasks.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn coordination_journal_recovers_after_restart_without_read_model_flush() {
    let _guard = background_worker_test_guard();
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
        .mutate_coordination(|prism| {
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:restart-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:restart-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Recover coordination state from authoritative journal".into(),
                "Recover coordination state from authoritative journal".into(),
                None,
                Some(Default::default()),
            )?;
            let _ = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:restart-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:restart-plan")),
                    causation: None,
                    execution_context: None,
                },
                TaskCreateInput {
                    plan_id,
                    title: "Recover coordination task from journal".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: prism_ir::WorkspaceRevision::default(),
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();

    {
        let mut store = session.store.lock().expect("workspace store lock poisoned");
        assert!(store.load_coordination_read_model().unwrap().is_none());
        assert!(store
            .load_coordination_queue_read_model()
            .unwrap()
            .is_none());
    }

    drop(session);

    let reloaded = index_workspace_session(&root).unwrap();
    let snapshot = reloaded.prism().coordination_snapshot();
    assert!(snapshot
        .tasks
        .iter()
        .any(|task| task.title == "Recover coordination task from journal"));
    let live_read_model = reloaded
        .load_coordination_read_model()
        .unwrap()
        .expect("session should derive a live coordination read model after restart");
    assert_eq!(live_read_model.task_count, 1);

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
                execution_context: None,
            },
            PlanCreateInput {
                title: "Prefer event-backed continuity load".into(),
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
                execution_context: None,
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
                execution_context: None,
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
                execution_context: None,
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
                execution_context: None,
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
                    execution_context: None,
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
                    execution_context: None,
                },
                "Append published plan deltas".into(),
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
                    execution_context: None,
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
                    execution_context: None,
                },
                prism_coordination::TaskUpdateInput {
                    task_id: task_id.clone(),
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: Some(prism.workspace_revision()),
                    priority: None,
                    tags: None,
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
fn repo_published_plan_logs_do_not_reemit_existing_child_of_edges() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, child_id, parent_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:edge-delta-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:edge-delta-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Avoid duplicate child edges".into(),
                "Avoid duplicate child edges".into(),
                None,
                Some(Default::default()),
            )?;
            let parent = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:edge-delta-parent"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:edge-delta-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Parent".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: base_revision.clone(),
                },
            )?;
            let child = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:edge-delta-child"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:edge-delta-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Child".into(),
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
            prism.create_native_plan_edge(
                &plan_id,
                &prism_ir::PlanNodeId::new(child.id.0.clone()),
                &prism_ir::PlanNodeId::new(parent.id.0.clone()),
                prism_ir::PlanEdgeKind::ChildOf,
            )?;
            Ok((plan_id, child.id, parent.id))
        })
        .unwrap();

    session
        .mutate_coordination(|prism| {
            let current_revision = prism.workspace_revision();
            let _ = prism.update_native_task(
                EventMeta {
                    id: EventId::new("coordination:edge-delta-child-update"),
                    ts: 4,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:edge-delta-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskUpdateInput {
                    task_id: child_id.clone(),
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::InProgress),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: Some("Child renamed once".into()),
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: None,
                    priority: None,
                    tags: None,
                    completion_context: None,
                },
                current_revision,
                4,
            )?;
            Ok(())
        })
        .unwrap();

    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let edge_id = format!("plan-edge:{}:child-of:{}", child_id.0, parent_id.0);
    let edge_added_count = fs::read_to_string(&log_path)
        .unwrap()
        .lines()
        .filter(|line| line.contains("\"kind\":\"edge_added\"") && line.contains(&edge_id))
        .count();
    assert_eq!(
        edge_added_count, 1,
        "incremental sync should not republish the same child-of edge after unrelated mutations"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn regenerate_repo_published_plan_artifacts_restores_index_and_derived_log_from_streams() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let plan_id = session
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:regen-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:regen-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Regenerate plan projections".into(),
                "Regenerate plan projections".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();

    let stream_path = root
        .join(".prism")
        .join("plans")
        .join("streams")
        .join(format!("{}.jsonl", plan_id.0));
    let active_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let index_path = root.join(".prism").join("plans").join("index.jsonl");
    let stream_bytes = fs::read(&stream_path).unwrap();

    fs::remove_file(&active_path).unwrap();
    fs::remove_file(&index_path).unwrap();

    regenerate_repo_published_plan_artifacts(&root).unwrap();

    assert_eq!(fs::read(&active_path).unwrap(), stream_bytes);
    let index_contents = fs::read_to_string(&index_path).unwrap();
    assert!(index_contents.contains(&format!(
        "\"log_path\":\".prism/plans/streams/{}.jsonl\"",
        plan_id.0
    )));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repair_repo_published_plan_artifacts_removes_redundant_edge_adds() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, parent_id, child_id, edge_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:repair-published-plan-artifacts"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:repair-published-plan-artifacts")),
                    causation: None,
                    execution_context: None,
                },
                "Repair published plan artifacts".into(),
                "Repair published plan artifacts".into(),
                None,
                Some(Default::default()),
            )?;
            let parent = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:repair-published-plan-parent"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:repair-published-plan-artifacts")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Parent".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    anchors: Vec::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: base_revision.clone(),
                },
            )?;
            let child = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:repair-published-plan-child"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:repair-published-plan-artifacts")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Child".into(),
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
            prism.create_native_plan_edge(
                &plan_id,
                &prism_ir::PlanNodeId::new(child.id.0.clone()),
                &prism_ir::PlanNodeId::new(parent.id.0.clone()),
                prism_ir::PlanEdgeKind::ChildOf,
            )?;
            let edge_id = prism_ir::PlanEdgeId::new(format!(
                "plan-edge:{}:child-of:{}",
                child.id.0, parent.id.0
            ));
            anyhow::Ok((plan_id, parent.id, child.id, edge_id))
        })
        .unwrap();

    let stream = ProtectedRepoStream::plan_stream(&plan_id);
    let inspection = inspect_protected_stream::<serde_json::Value>(&root, &stream).unwrap();
    let duplicate_event_id = format!("published:{}:duplicate-edge-add", plan_id.0);
    let mut duplicate_payload = inspection
        .payloads
        .iter()
        .find(|payload| {
            payload.get("kind") == Some(&json!("edge_added"))
                && payload.get("edge_id") == Some(&json!(edge_id.0))
        })
        .cloned()
        .expect("expected an edge_added payload for the child edge");
    duplicate_payload["event_id"] = json!(duplicate_event_id.clone());
    append_protected_stream_event(
        &root,
        &stream,
        &duplicate_event_id,
        &duplicate_payload,
        &crate::protected_state::repo_streams::implicit_principal_identity(None, None),
    )
    .unwrap();

    let report = inspect_repo_published_plan_artifacts(&root).unwrap();
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.plan_id == plan_id)
        .expect("repair report should include the plan");
    assert_eq!(entry.redundant_edge_add_count, 1);
    assert!(!entry.repaired);

    let repair_report = repair_repo_published_plan_artifacts(&root).unwrap();
    let repaired_entry = repair_report
        .entries
        .iter()
        .find(|entry| entry.plan_id == plan_id)
        .expect("repair report should include the plan");
    assert_eq!(repair_report.repaired_plan_count, 1);
    assert_eq!(repaired_entry.redundant_edge_add_count, 1);
    assert!(repaired_entry.repaired);

    let repaired_inspection =
        inspect_protected_stream::<serde_json::Value>(&root, &stream).unwrap();
    let repaired_edge_add_count = repaired_inspection
        .payloads
        .iter()
        .filter(|payload| {
            payload.get("kind") == Some(&json!("edge_added"))
                && payload.get("edge_id") == Some(&json!(edge_id.0))
        })
        .count();
    assert_eq!(repaired_edge_add_count, 1);

    let active_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    assert_eq!(
        fs::read(&active_path).unwrap(),
        fs::read(root.join(stream.relative_path())).unwrap()
    );

    let hydrated = crate::published_plans::load_hydrated_coordination_plan_state(&root, None)
        .unwrap()
        .expect("published plans should still hydrate after repair");
    let repaired_graph = hydrated
        .plan_graphs
        .iter()
        .find(|graph| graph.id == plan_id)
        .expect("hydrated state should include the repaired plan");
    assert!(repaired_graph
        .nodes
        .iter()
        .any(|node| node.id == prism_ir::PlanNodeId::new(parent_id.0.clone())));
    assert!(repaired_graph
        .nodes
        .iter()
        .any(|node| node.id == prism_ir::PlanNodeId::new(child_id.0.clone())));
    assert_eq!(
        repaired_graph
            .edges
            .iter()
            .filter(|edge| edge.id == edge_id)
            .count(),
        1
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn completing_last_task_appends_plan_completion_to_published_plan_log() {
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
                    id: EventId::new("coordination:auto-complete-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-complete-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Persist derived plan completion".into(),
                "Persist derived plan completion".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:auto-complete-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-complete-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Complete the only task".into(),
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

    session
        .mutate_coordination(|prism| {
            let _ = prism.update_native_task(
                EventMeta {
                    id: EventId::new("coordination:auto-complete-task-update"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-complete-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskUpdateInput {
                    task_id: task_id.clone(),
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: Some(prism.workspace_revision()),
                    priority: None,
                    tags: None,
                    completion_context: None,
                },
                prism.workspace_revision(),
                3,
            )?;
            Ok(())
        })
        .unwrap();

    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let log_contents = fs::read_to_string(&log_path).unwrap();
    assert!(
        log_contents.contains("\"kind\":\"plan_updated\""),
        "completing the last task should append a plan update event"
    );
    assert!(
        log_contents.contains("\"status\":\"Completed\""),
        "published plan log should persist the derived completed plan status"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn releasing_last_claim_appends_plan_completion_to_published_plan_log() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let (plan_id, task_id, claim_id) = session
        .mutate_coordination(|prism| {
            let base_revision = prism.workspace_revision();
            let plan_id = prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:auto-close-on-claim-release-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-close-on-claim-release")),
                    causation: None,
                    execution_context: None,
                },
                "Persist derived plan completion after claim release".into(),
                "Persist derived plan completion after claim release".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:auto-close-on-claim-release-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-close-on-claim-release")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Complete the only task and release the claim".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: None,
                    session: Some(SessionId::new("session:auto-close-on-claim-release")),
                    worktree_id: None,
                    branch_ref: None,
                    anchors: vec![AnchorRef::Kind(prism_ir::NodeKind::Function)],
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    base_revision: base_revision.clone(),
                },
            )?;
            let (claim_id, _conflicts, _claim) = prism.acquire_native_claim(
                EventMeta {
                    id: EventId::new("coordination:auto-close-on-claim-release-claim"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-close-on-claim-release")),
                    causation: None,
                    execution_context: None,
                },
                SessionId::new("session:auto-close-on-claim-release"),
                prism_coordination::ClaimAcquireInput {
                    task_id: Some(task.id.clone()),
                    anchors: task.anchors.clone(),
                    capability: prism_ir::Capability::Edit,
                    mode: Some(prism_ir::ClaimMode::SoftExclusive),
                    ttl_seconds: Some(60),
                    base_revision: base_revision.clone(),
                    current_revision: base_revision,
                    agent: None,
                    worktree_id: None,
                    branch_ref: None,
                },
            )?;
            Ok((plan_id, task.id, claim_id.expect("claim id")))
        })
        .unwrap();

    session
        .mutate_coordination(|prism| {
            let _ = prism.update_native_task(
                EventMeta {
                    id: EventId::new("coordination:auto-close-on-claim-release-task-update"),
                    ts: 4,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-close-on-claim-release")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskUpdateInput {
                    task_id: task_id.clone(),
                    kind: None,
                    status: Some(prism_ir::CoordinationTaskStatus::Completed),
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: None,
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: Some(prism.workspace_revision()),
                    priority: None,
                    tags: None,
                    completion_context: Some(prism_coordination::TaskCompletionContext::default()),
                },
                prism.workspace_revision(),
                4,
            )?;
            Ok(())
        })
        .unwrap();

    session
        .mutate_coordination(|prism| {
            let _ = prism.release_native_claim(
                EventMeta {
                    id: EventId::new("coordination:auto-close-on-claim-release-release"),
                    ts: 5,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:auto-close-on-claim-release")),
                    causation: None,
                    execution_context: None,
                },
                &SessionId::new("session:auto-close-on-claim-release"),
                &claim_id,
            )?;
            Ok(())
        })
        .unwrap();

    assert_eq!(
        session
            .prism()
            .coordination_plan(&plan_id)
            .expect("plan after release")
            .status,
        prism_ir::PlanStatus::Completed
    );
    assert_eq!(
        session
            .prism()
            .plan_graph(&plan_id)
            .expect("authored plan graph")
            .status,
        prism_ir::PlanStatus::Completed
    );

    let log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let log_contents = fs::read_to_string(&log_path).unwrap();
    assert!(
        log_contents.contains("\"kind\":\"plan_updated\""),
        "releasing the last claim should append a plan update event"
    );
    assert!(
        log_contents.contains("\"status\":\"Completed\""),
        "published plan log should persist the derived completed plan status after claim release"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plan_logs_skip_runtime_handoff_deltas() {
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
                    id: EventId::new("coordination:runtime-overlay-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:runtime-overlay-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Skip runtime-only handoff deltas".into(),
                "Skip runtime-only handoff deltas".into(),
                None,
                Some(Default::default()),
            )?;
            let task = prism.create_native_task(
                EventMeta {
                    id: EventId::new("coordination:runtime-overlay-task"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:runtime-overlay-plan")),
                    causation: None,
                    execution_context: None,
                },
                prism_coordination::TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: "Keep published logs structural".into(),
                    status: Some(prism_ir::CoordinationTaskStatus::Ready),
                    assignee: Some(prism_ir::AgentId::new("agent:a")),
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
            prism.request_native_handoff(
                EventMeta {
                    id: EventId::new("coordination:runtime-overlay-handoff"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:runtime-overlay-plan")),
                    causation: None,
                    execution_context: None,
                },
                HandoffInput {
                    task_id: task_id.clone(),
                    to_agent: Some(prism_ir::AgentId::new("agent:b")),
                    summary: "Shift runtime ownership only".into(),
                    base_revision: prism.workspace_revision(),
                },
                prism.workspace_revision(),
            )?;
            Ok(())
        })
        .unwrap();

    let log_contents = fs::read_to_string(&log_path).unwrap();
    let updated_lines = log_contents.lines().count();
    assert_eq!(
        updated_lines, 3,
        "handoff should only publish the task-backed node delta, not a separate execution overlay event"
    );
    assert!(
        !log_contents.contains("\"kind\":\"execution_updated\""),
        "published plan logs should not emit execution overlay events for handoffs"
    );
    assert!(
        !log_contents.contains("pending_handoff_to"),
        "published plan logs should not serialize runtime handoff overlay fields"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn repo_published_plans_archive_transition_emits_archive_event_and_moves_log() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let plan_id = session
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:archive-plan"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:archive-plan")),
                    causation: None,
                    execution_context: None,
                },
                "Archive published plan logs explicitly".into(),
                "Archive published plan logs explicitly".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();

    let active_log_path = root
        .join(".prism")
        .join("plans")
        .join("active")
        .join(format!("{}.jsonl", plan_id.0));
    let archived_log_path = root
        .join(".prism")
        .join("plans")
        .join("archived")
        .join(format!("{}.jsonl", plan_id.0));

    session
        .mutate_coordination(|prism| {
            prism.update_native_plan(
                EventMeta {
                    id: EventId::new("coordination:archive-plan-abandon"),
                    ts: 2,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:archive-plan")),
                    causation: None,
                    execution_context: None,
                },
                &plan_id,
                None,
                Some(prism_ir::PlanStatus::Abandoned),
                None,
                None,
            )
        })
        .unwrap();
    assert!(
        active_log_path.exists(),
        "abandoned plans should remain under the active log directory until archived"
    );
    assert!(
        !archived_log_path.exists(),
        "archive storage should not move on abandonment alone"
    );
    let abandoned_log = fs::read_to_string(&active_log_path).unwrap();
    assert!(
        abandoned_log.contains("\"kind\":\"plan_updated\""),
        "abandoning the plan should stay a normal plan update"
    );

    session
        .mutate_coordination(|prism| {
            prism.update_native_plan(
                EventMeta {
                    id: EventId::new("coordination:archive-plan-archive"),
                    ts: 3,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:archive-plan")),
                    causation: None,
                    execution_context: None,
                },
                &plan_id,
                None,
                Some(prism_ir::PlanStatus::Archived),
                None,
                None,
            )
        })
        .unwrap();

    assert!(
        !active_log_path.exists(),
        "archived plans should move out of the active log directory"
    );
    assert!(
        archived_log_path.exists(),
        "archived plans should persist under the archived log directory"
    );
    let archived_log = fs::read_to_string(&archived_log_path).unwrap();
    assert!(
        archived_log.contains("\"kind\":\"plan_archived\""),
        "archiving the plan should emit a first-class archive event"
    );

    let index_contents =
        fs::read_to_string(root.join(".prism").join("plans").join("index.jsonl")).unwrap();
    assert!(index_contents.contains(&format!(
        "\"log_path\":\".prism/plans/streams/{}.jsonl\"",
        plan_id.0
    )));

    drop(session);
    fs::remove_file(
        PrismPaths::for_workspace_root(&root)
            .unwrap()
            .shared_runtime_db_path()
            .unwrap(),
    )
    .unwrap();

    let reloaded = index_workspace_session(&root).unwrap();
    let snapshot = reloaded
        .load_coordination_snapshot()
        .unwrap()
        .expect("archived published plans should hydrate a coordination snapshot");
    assert!(snapshot
        .plans
        .iter()
        .any(|plan| plan.id == plan_id && plan.status == prism_ir::PlanStatus::Archived));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tampered_authoritative_plan_stream_is_rejected_on_reload() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let plan_id = session
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:tampered-plan-stream"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:tampered-plan-stream")),
                    causation: None,
                    execution_context: None,
                },
                "Signed plan goal".into(),
                "Signed plan goal".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();

    let stream_path = root
        .join(".prism")
        .join("plans")
        .join("streams")
        .join(format!("{}.jsonl", plan_id.0));
    let tampered = fs::read_to_string(&stream_path)
        .unwrap()
        .replace("Signed plan goal", "Tampered plan goal");
    fs::write(&stream_path, tampered).unwrap();

    let error = match index_workspace_session(&root) {
        Ok(_) => panic!("tampered authoritative plan stream should not reload"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Tampered") || error.contains("signature verification"),
        "expected tamper detection error, got: {error}"
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
fn refresh_fs_with_paths_consumes_only_scoped_dirty_paths() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(root.join("docs/guide.md"), "# Guide\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    let lib = root.join("src/lib.rs");
    let guide = root.join("docs/guide.md");
    session
        .refresh_state
        .mark_fs_dirty_paths([lib.clone(), guide.clone()]);

    let outcome = session.refresh_fs_with_paths(vec![lib]).unwrap();

    assert_eq!(outcome.status, crate::FsRefreshStatus::Clean);
    assert_eq!(session.pending_refresh_paths(), vec![guide]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_rebuild_from_persisted_state_defers_when_refresh_is_in_progress() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session_with_options(
        &root,
        WorkspaceSessionOptions {
            shared_runtime: SharedRuntimeBackend::Disabled,
            ..WorkspaceSessionOptions::default()
        },
    )
    .unwrap();
    let _guard = session
        .refresh_lock
        .lock()
        .expect("workspace refresh lock poisoned");

    let reloaded = session.try_recover_runtime_from_persisted_state().unwrap();
    assert!(!reloaded);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_rebuild_from_persisted_state_records_replay_bounds() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let session = index_workspace_session(&root).unwrap();
    session.record_runtime_refresh_observation("deferred", 0);

    let reloaded = session.try_recover_runtime_from_persisted_state().unwrap();
    assert!(reloaded);

    let recovery = session
        .last_refresh()
        .expect("recovery should record runtime refresh metadata");
    assert_eq!(recovery.path, "recovery");
    assert!(recovery.workspace_reloaded);
    assert_eq!(recovery.full_rebuild_count, 0);
    assert!(recovery.loaded_bytes > 0);
    assert!(recovery.replay_volume > 0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn recovery_rebuild_from_shared_runtime_journals_without_checkpoint_flush() {
    let _guard = PRISM_HOME_ENV_LOCK.lock().unwrap();

    let shared_runtime_root = temp_workspace();
    let shared_runtime_sqlite = shared_runtime_root.join("shared-runtime.db");
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let options = WorkspaceSessionOptions {
        coordination: true,
        shared_runtime: SharedRuntimeBackend::Sqlite {
            path: shared_runtime_sqlite,
        },
        hydrate_persisted_projections: false,
        hydrate_persisted_co_change: true,
    };
    let session = index_workspace_session_with_options(&root, options.clone()).unwrap();
    let alpha = session
        .prism()
        .symbol("alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::alpha")
        .expect("alpha should be indexed")
        .id()
        .clone();

    let mut entry = MemoryEntry::new(MemoryKind::Structural, "shared runtime recovery memory");
    entry.id = MemoryId("memory:shared-runtime-recovery".to_string());
    entry.anchors = vec![AnchorRef::Node(alpha)];
    entry.scope = MemoryScope::Session;
    entry.source = MemorySource::User;
    entry.trust = 0.9;
    session
        .append_memory_event(MemoryEvent::from_entry(
            MemoryEventKind::Stored,
            entry.clone(),
            Some("task:shared-runtime-recovery".to_string()),
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();

    let plan_id = session
        .mutate_coordination(|prism| {
            prism.create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:shared-runtime-recovery"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:shared-runtime-recovery")),
                    causation: None,
                    execution_context: None,
                },
                "Recover coordination from shared-runtime journal".into(),
                "Recover coordination from shared-runtime journal".into(),
                None,
                Some(Default::default()),
            )
        })
        .unwrap();
    drop(session);

    let reloaded = index_workspace_session_with_options(&root, options).unwrap();
    reloaded.record_runtime_refresh_observation("deferred", 0);

    let recovered = reloaded.try_recover_runtime_from_persisted_state().unwrap();
    assert!(recovered);

    let recovery = reloaded
        .last_refresh()
        .expect("recovery should record runtime refresh metadata");
    assert_eq!(recovery.path, "recovery");
    assert!(recovery.workspace_reloaded);
    assert_eq!(recovery.full_rebuild_count, 0);
    assert!(recovery.loaded_bytes > 0);
    assert!(recovery.replay_volume > 0);

    let events = reloaded
        .memory_events(&MemoryEventQuery {
            memory_id: Some(entry.id.clone()),
            focus: Vec::new(),
            text: None,
            limit: 5,
            kinds: None,
            actions: Some(vec![MemoryEventKind::Stored]),
            scope: Some(MemoryScope::Session),
            task_id: Some("task:shared-runtime-recovery".to_string()),
            since: None,
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    let state = reloaded
        .load_coordination_plan_state()
        .unwrap()
        .expect("shared runtime coordination state should replay");
    assert!(state.snapshot.plans.iter().any(|plan| plan.id == plan_id
        && plan.goal == "Recover coordination from shared-runtime journal"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(shared_runtime_root);
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
                execution_context: None,
            },
            "Use live runtime coordination state".into(),
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
            hydrate_persisted_co_change: true,
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
fn startup_treats_incomplete_persisted_projection_materialization_as_missing_snapshot() {
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    )
    .unwrap();

    let _ = index_workspace_session(&root).unwrap();
    let db_path = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
    let mut store = SqliteStore::open(&db_path).unwrap();
    let mut persisted = store.load_projection_snapshot().unwrap().unwrap();
    assert!(!persisted.co_change_by_lineage.is_empty());
    if persisted.validation_by_lineage.is_empty() {
        persisted.validation_by_lineage.push((
            LineageId::new("lineage:alpha"),
            vec![ValidationCheck {
                label: "test:smoke".to_string(),
                score: 1.0,
                last_seen: 1,
            }],
        ));
    }
    persisted.co_change_by_lineage.clear();
    store.save_projection_snapshot(&persisted).unwrap();
    assert_eq!(
        store.load_projection_materialization_metadata().unwrap(),
        ProjectionMaterializationMetadata {
            has_co_change: false,
            has_validation: true,
            has_knowledge: false,
        }
    );

    let indexer =
        WorkspaceIndexer::new_with_options(&root, WorkspaceSessionOptions::default()).unwrap();
    assert!(!indexer.had_projection_snapshot);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_keeps_hot_outcomes_bounded_when_persisted_projection_materialization_is_co_change_only()
{
    let root = temp_workspace();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();

    let _ = index_workspace_session(&root).unwrap();
    let db_path = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
    let mut store = SqliteStore::open(&db_path).unwrap();
    let events = (0..(HOT_OUTCOME_HYDRATION_LIMIT + 8))
        .map(|index| OutcomeEvent {
            meta: EventMeta {
                id: EventId::new(format!("outcome:test:projection-boundary:{index}")),
                ts: u64::try_from(index + 1).unwrap(),
                actor: EventActor::Agent,
                correlation: Some(TaskId::new("task:projection-boundary")),
                causation: None,
                execution_context: None,
            },
            anchors: Vec::new(),
            kind: OutcomeKind::PlanCreated,
            result: OutcomeResult::Success,
            summary: format!("outcome {index}"),
            evidence: Vec::new(),
            metadata: serde_json::Value::Null,
        })
        .collect::<Vec<_>>();
    store
        .save_outcome_snapshot(&OutcomeMemorySnapshot {
            events: events.clone(),
        })
        .unwrap();
    store
        .save_projection_snapshot(&ProjectionSnapshot {
            co_change_by_lineage: vec![(
                LineageId::new("lineage:alpha"),
                vec![CoChangeRecord {
                    lineage: LineageId::new("lineage:beta"),
                    count: 3,
                }],
            )],
            validation_by_lineage: Vec::new(),
            curated_concepts: Vec::new(),
            concept_relations: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        store.load_projection_materialization_metadata().unwrap(),
        ProjectionMaterializationMetadata {
            has_co_change: true,
            has_validation: false,
            has_knowledge: false,
        }
    );

    let indexer =
        WorkspaceIndexer::new_with_options(&root, WorkspaceSessionOptions::default()).unwrap();
    assert!(indexer.outcomes.snapshot().events.len() <= HOT_OUTCOME_HYDRATION_LIMIT);

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
fn refresh_state_filters_stale_path_requests_to_latest_revision_only() {
    let state = crate::session::WorkspaceRefreshState::new();
    let path = PathBuf::from("/tmp/demo.rs");

    let first_revision = state.mark_fs_dirty_paths([path.clone()]);
    let second_revision = state.mark_fs_dirty_paths([path.clone()]);

    let stale = state.scoped_dirty_paths_for_requests(&[
        crate::runtime_engine::WorkspaceRuntimePathRequest {
            path: path.clone(),
            revision: first_revision,
        },
    ]);
    assert!(stale.is_empty());

    let current = state.scoped_dirty_paths_for_requests(&[
        crate::runtime_engine::WorkspaceRuntimePathRequest {
            path: path.clone(),
            revision: second_revision,
        },
    ]);
    assert_eq!(current, vec![path]);
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
fn oversized_targeted_refresh_in_large_repo_stays_shallow_until_explicitly_deepened() {
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
    let oversized_comment = "x".repeat(140 * 1024);
    fs::write(
        &target_path,
        format!("// {oversized_comment}\n\npub fn alpha() {{\n    beta();\n}}\n\nfn beta() {{}}\n"),
    )
    .unwrap();
    let target_path = fs::canonicalize(target_path).unwrap();

    let session = index_workspace_session(&root).unwrap();
    let initial_prism = session.prism();
    let initial_record = initial_prism
        .graph()
        .file_record(&target_path)
        .expect("oversized file should be indexed");
    assert_eq!(initial_record.parse_depth, ParseDepth::Shallow);
    assert!(initial_record.unresolved_calls.is_empty());

    fs::write(
        &target_path,
        format!(
            "// edited\n// {oversized_comment}\n\npub fn alpha() {{\n    beta();\n}}\n\nfn beta() {{}}\n"
        ),
    )
    .unwrap();
    session.refresh_fs_with_status().unwrap();

    let refreshed_prism = session.prism();
    let refreshed_record = refreshed_prism
        .graph()
        .file_record(&target_path)
        .expect("oversized file should remain indexed");
    assert_eq!(refreshed_record.parse_depth, ParseDepth::Shallow);
    assert!(refreshed_record.unresolved_calls.is_empty());

    assert!(session
        .ensure_paths_deep([target_path.clone()])
        .expect("deepening oversized file should succeed"));

    let deepened_prism = session.prism();
    let deepened_record = deepened_prism
        .graph()
        .file_record(&target_path)
        .expect("deepened oversized file should remain indexed");
    assert_eq!(deepened_record.parse_depth, ParseDepth::Deep);
    assert!(deepened_record.unresolved_calls.iter().any(|call| call
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
fn refresh_invalidation_scope_expands_only_real_dependents() {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use prism_ir::{FileId, Language, Node, NodeId, NodeKind, Span};
    use prism_parser::UnresolvedCall;
    use prism_store::Graph;

    fn function(file: FileId, name: &str) -> Node {
        Node {
            id: NodeId::new("demo", format!("demo::{name}"), NodeKind::Function),
            name: name.into(),
            kind: NodeKind::Function,
            file,
            span: Span::line(1),
            language: Language::Rust,
        }
    }

    fn unresolved_call(caller: &Node, module_path: &str, name: &str) -> UnresolvedCall {
        UnresolvedCall {
            caller: caller.id.clone(),
            name: name.into(),
            span: Span::line(1),
            module_path: module_path.into(),
        }
    }

    let changed = PathBuf::from("src/a.rs");
    let caller_path = PathBuf::from("src/lib.rs");
    let unrelated = PathBuf::from("src/other.rs");
    let mut graph = Graph::new();

    let changed_file = graph.ensure_file(Path::new(&changed));
    let caller_file = graph.ensure_file(Path::new(&caller_path));
    let unrelated_file = graph.ensure_file(Path::new(&unrelated));

    let alpha = function(changed_file, "alpha");
    let caller = function(caller_file, "caller");
    let other = function(unrelated_file, "other");

    graph.upsert_file(
        Path::new(&changed),
        1,
        vec![alpha],
        Vec::new(),
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        Path::new(&caller_path),
        1,
        vec![caller.clone()],
        Vec::new(),
        HashMap::new(),
        vec![unresolved_call(&caller, "demo", "alpha")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    graph.upsert_file(
        Path::new(&unrelated),
        1,
        vec![other.clone()],
        Vec::new(),
        HashMap::new(),
        vec![unresolved_call(&other, "demo", "gamma")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let scope = crate::invalidation::RefreshInvalidationScope::from_graph(
        &graph,
        &HashSet::from([changed.clone()]),
    );

    assert!(scope.edge_resolution_paths.contains(&changed));
    assert!(scope.edge_resolution_paths.contains(&caller_path));
    assert!(!scope.edge_resolution_paths.contains(&unrelated));
}

#[test]
fn body_only_updates_do_not_require_dependent_edge_resolution() {
    use prism_ir::{
        ChangeTrigger, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId, NodeKind,
        ObservedChangeSet, ObservedNode, Span, SymbolFingerprint,
    };

    let node_before = Node {
        id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
        name: "alpha".into(),
        kind: NodeKind::Function,
        file: FileId(1),
        span: Span::line(1),
        language: Language::Rust,
    };
    let node_after = Node {
        span: Span::line(2),
        ..node_before.clone()
    };
    let observed = ObservedChangeSet {
        meta: EventMeta {
            id: EventId::new("evt:body-only"),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        trigger: ChangeTrigger::FsWatch,
        files: vec![FileId(1)],
        previous_path: None,
        current_path: None,
        added: Vec::new(),
        removed: Vec::new(),
        updated: vec![(
            ObservedNode {
                node: node_before,
                fingerprint: SymbolFingerprint::new(1),
            },
            ObservedNode {
                node: node_after,
                fingerprint: SymbolFingerprint::new(1),
            },
        )],
        edge_added: Vec::new(),
        edge_removed: Vec::new(),
    };

    assert!(!crate::invalidation::observed_changes_require_dependent_edge_resolution(&[observed]));
}

#[test]
fn renamed_symbols_expand_dependents_from_emitted_dependency_keys() {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use prism_ir::{
        ChangeTrigger, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId, NodeKind,
        Span,
    };
    use prism_parser::{ParseDepth, UnresolvedCall};
    use prism_store::Graph;

    fn function(file: FileId, name: &str) -> Node {
        Node {
            id: NodeId::new("demo", format!("demo::{name}"), NodeKind::Function),
            name: name.into(),
            kind: NodeKind::Function,
            file,
            span: Span::line(1),
            language: Language::Rust,
        }
    }

    fn unresolved_call(caller: &Node, module_path: &str, name: &str) -> UnresolvedCall {
        UnresolvedCall {
            caller: caller.id.clone(),
            name: name.into(),
            span: Span::line(1),
            module_path: module_path.into(),
        }
    }

    let changed = PathBuf::from("src/a.rs");
    let caller_path = PathBuf::from("src/lib.rs");
    let mut graph = Graph::new();

    let changed_file = graph.ensure_file(Path::new(&changed));
    let caller_file = graph.ensure_file(Path::new(&caller_path));

    let alpha = function(changed_file, "alpha");
    let caller = function(caller_file, "caller");

    graph.upsert_file(
        Path::new(&changed),
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
        Path::new(&caller_path),
        1,
        vec![caller.clone()],
        Vec::new(),
        HashMap::new(),
        vec![unresolved_call(&caller, "demo", "alpha")],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let update = graph.upsert_file_from_with_observed_without_rebuild(
        None,
        Path::new(&changed),
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
            id: EventId::new("evt:rename".to_string()),
            ts: 1,
            actor: EventActor::System,
            correlation: None,
            causation: None,
            execution_context: None,
        },
        ChangeTrigger::ManualReindex,
    );

    let dependency_paths =
        crate::invalidation::expand_dependency_paths(&graph, &HashSet::from([changed.clone()]));
    let edge_paths = crate::invalidation::edge_resolution_paths_for_dependency_keys(
        &graph,
        &dependency_paths,
        &update.dependency_invalidation_keys,
    );

    assert!(edge_paths.contains(&changed));
    assert!(edge_paths.contains(&caller_path));
}

#[test]
fn published_prism_shares_runtime_graph_backing() {
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
    let runtime_state = indexer.into_runtime_state();
    let workspace_revision = prism_ir::WorkspaceRevision {
        graph_version: 1,
        git_commit: None,
    };
    let published = runtime_state.publish_generation(workspace_revision.clone(), None);
    let prism = published.prism_arc();
    assert_eq!(published.workspace_revision(), workspace_revision);
    assert!(published.coordination_context().is_none());
    assert!(std::ptr::eq(
        prism.graph(),
        std::sync::Arc::as_ptr(&runtime_state.graph)
    ));
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
    let cache_path = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
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
            execution_context: None,
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
    let cache_path = PrismPaths::for_workspace_root(&root)
        .unwrap()
        .worktree_cache_db_path()
        .unwrap();
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
            execution_context: None,
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
    let _guard = background_worker_test_guard();
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
                execution_context: None,
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
    let reloaded_alpha = prism
        .symbol("alpha")
        .into_iter()
        .next()
        .unwrap()
        .id()
        .clone();
    let recipe = prism.validation_recipe(&reloaded_alpha);
    assert!(recipe
        .scored_checks
        .iter()
        .any(|check| check.label == "test:alpha_integration" && check.score > 0.0));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_materializes_graph_snapshot_off_request_path() {
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
    session.refresh_fs().unwrap();

    let gamma = NodeId::new("demo", "demo::gamma", NodeKind::Function);
    assert!(session
        .prism()
        .symbol("gamma")
        .iter()
        .any(|symbol| symbol.id() == &gamma));

    {
        let mut store = session.store.lock().expect("workspace store lock poisoned");
        let persisted = store.load_graph().unwrap().unwrap();
        assert!(
            persisted.nodes.contains_key(&gamma),
            "file-local graph state should stay coherent before flush"
        );
        assert!(!persisted.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Calls
                && edge.source == NodeId::new("demo", "demo::alpha", NodeKind::Function)
                && edge.target == gamma
        }));
    }

    session.flush_materializations().unwrap();

    let mut store = session.store.lock().expect("workspace store lock poisoned");
    let persisted = store.load_graph().unwrap().unwrap();
    assert!(persisted.nodes.contains_key(&gamma));
    assert!(persisted.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Calls
            && edge.source == NodeId::new("demo", "demo::alpha", NodeKind::Function)
            && edge.target == gamma
    }));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn refresh_fs_defers_episodic_memory_reanchor_off_request_path() {
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
    assert!(!observed.is_empty());

    let snapshot = {
        let mut store = session.store.lock().expect("workspace store lock poisoned");
        store
            .load_episodic_snapshot()
            .unwrap()
            .expect("episodic snapshot should still exist")
    };
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.content == "alpha previously regressed")
        .expect("note should remain persisted before flush");
    assert!(entry.anchors.contains(&AnchorRef::Node(alpha.clone())));
    assert!(
        !entry
            .anchors
            .iter()
            .any(|anchor| matches!(anchor, AnchorRef::Lineage(_))),
        "reanchor should still be deferred before flush"
    );

    session.flush_materializations().unwrap();

    let renamed_alpha = session
        .prism()
        .symbol("renamed_alpha")
        .into_iter()
        .find(|symbol| symbol.id().path == "demo::renamed_alpha")
        .expect("renamed alpha should be indexed after refresh")
        .id()
        .clone();
    let lineage = session
        .prism()
        .lineage_of(&renamed_alpha)
        .expect("renamed alpha should keep a lineage");
    let snapshot = session
        .load_episodic_snapshot()
        .unwrap()
        .expect("reanchored note should persist after flush");
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.content == "alpha previously regressed")
        .expect("reanchored note should be present after flush");
    assert!(entry
        .anchors
        .contains(&AnchorRef::Node(renamed_alpha.clone())));
    assert!(entry.anchors.contains(&AnchorRef::Lineage(lineage)));
    assert!(!entry.anchors.contains(&AnchorRef::Node(alpha)));

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
                    execution_context: None,
                },
                "Coordinate alpha".into(),
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
            hydrate_persisted_co_change: true,
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
    let _guard = background_worker_test_guard();
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
                execution_context: None,
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
                    execution_context: None,
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
    let root = std::env::temp_dir().join(format!(
        "prism-test-{}-{stamp}-{sequence}",
        std::process::id()
    ));
    track_temp_dir(&root);
    root
}
