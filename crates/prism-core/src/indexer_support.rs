use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::Result;
use prism_coordination::CoordinationStore;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::EdgeKind;
use prism_memory::OutcomeMemory;
use prism_parser::LanguageAdapter;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::{Graph, SqliteStore, Store};
use tracing::info;

use crate::curator::{CuratorHandle, CuratorHandleRef};
use crate::indexer::PendingFileParse;
use crate::resolution::{resolve_calls, resolve_impls, resolve_imports, resolve_intents};
use crate::session::{WorkspaceRefreshState, WorkspaceSession};
use crate::util::{persisted_file_hash, workspace_fingerprint, workspace_walk};
use crate::watch::spawn_fs_watch;

pub(crate) fn build_workspace_session(
    root: PathBuf,
    store: SqliteStore,
    graph: Graph,
    history: HistoryStore,
    outcomes: OutcomeMemory,
    coordination: CoordinationStore,
    projections: ProjectionIndex,
    coordination_enabled: bool,
    backend: Option<Arc<dyn CuratorBackend>>,
) -> Result<WorkspaceSession> {
    let started = Instant::now();
    let prism = Arc::new(Prism::with_history_outcomes_coordination_and_projections(
        graph,
        history,
        outcomes,
        coordination,
        projections,
    ));
    let prism = Arc::new(RwLock::new(prism));
    let store = Arc::new(Mutex::new(store));
    let refresh_lock = Arc::new(Mutex::new(()));
    let refresh_state = Arc::new(WorkspaceRefreshState::new());
    let fingerprint_started = Instant::now();
    let fs_snapshot = Arc::new(Mutex::new(workspace_fingerprint(&root, None)?));
    let fingerprint_ms = fingerprint_started.elapsed().as_millis();
    let curator_snapshot_started = Instant::now();
    let curator_snapshot = {
        let mut store = store.lock().expect("workspace store lock poisoned");
        store.load_curator_snapshot()?.unwrap_or_default()
    };
    let load_curator_snapshot_ms = curator_snapshot_started.elapsed().as_millis();
    let curator = CuratorHandle::new(
        curator_snapshot,
        backend,
        Arc::clone(&store),
        Arc::clone(&refresh_lock),
    );
    let watch_started = Instant::now();
    let watch = Some(spawn_fs_watch(
        root.clone(),
        Arc::clone(&prism),
        Arc::clone(&store),
        Arc::clone(&refresh_lock),
        Arc::clone(&refresh_state),
        Arc::clone(&fs_snapshot),
        coordination_enabled,
        Some(CuratorHandleRef::from(&curator)),
    )?);
    let watch_start_ms = watch_started.elapsed().as_millis();
    let graph = prism.read().expect("workspace prism lock poisoned");
    let node_count = graph.graph().node_count();
    let edge_count = graph.graph().edge_count();
    let file_count = graph.graph().file_count();
    drop(graph);
    info!(
        root = %root.display(),
        coordination_enabled,
        node_count,
        edge_count,
        file_count,
        fingerprint_ms,
        load_curator_snapshot_ms,
        watch_start_ms,
        total_ms = started.elapsed().as_millis(),
        "built prism workspace session"
    );
    Ok(WorkspaceSession {
        root,
        prism,
        store,
        refresh_lock,
        refresh_state,
        fs_snapshot,
        watch,
        curator: Some(curator),
        coordination_enabled,
    })
}

pub(crate) fn collect_pending_file_parses(
    walk_root: &Path,
    adapters: &[Box<dyn LanguageAdapter>],
) -> Result<(Vec<PendingFileParse>, HashSet<PathBuf>)> {
    let mut pending = Vec::<PendingFileParse>::new();
    let mut seen_files = HashSet::<PathBuf>::new();

    for entry in workspace_walk(walk_root).filter_map(Result::ok) {
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let path = entry.path();
        let Some(_adapter) = adapters.iter().find(|adapter| adapter.supports_path(path)) else {
            continue;
        };

        let canonical_path = path.to_path_buf();
        seen_files.insert(canonical_path.clone());
        let source = fs::read_to_string(path)?;
        let hash = persisted_file_hash(&source);
        pending.push(PendingFileParse {
            path: canonical_path,
            source,
            hash,
            previous_path: None,
        });
    }

    Ok((pending, seen_files))
}

pub(crate) fn resolve_graph_edges(graph: &mut Graph) {
    graph.clear_edges_by_kind(&[
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::Implements,
        EdgeKind::Specifies,
        EdgeKind::Validates,
        EdgeKind::RelatedTo,
    ]);
    let unresolved_calls = graph.unresolved_calls();
    let unresolved_imports = graph.unresolved_imports();
    let unresolved_impls = graph.unresolved_impls();
    let unresolved_intents = graph.unresolved_intents();
    resolve_calls(graph, unresolved_calls);
    resolve_imports(graph, unresolved_imports);
    resolve_impls(graph, unresolved_impls);
    resolve_intents(graph, unresolved_intents);
}
