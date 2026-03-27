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
    refresh_scope: Option<&HashSet<PathBuf>>,
) -> Result<(Vec<PendingFileParse>, HashSet<PathBuf>)> {
    let mut pending = Vec::<PendingFileParse>::new();
    let mut seen_files = HashSet::<PathBuf>::new();
    let paths = if let Some(scope) = refresh_scope {
        collect_refresh_scope_paths(scope)?
    } else {
        workspace_walk(walk_root)
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|file_type| file_type.is_file())
                    .unwrap_or(false)
            })
            .map(|entry| entry.path().to_path_buf())
            .collect()
    };

    for path in paths {
        let Some(_adapter) = adapters.iter().find(|adapter| adapter.supports_path(&path)) else {
            continue;
        };

        if !seen_files.insert(path.clone()) {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let hash = persisted_file_hash(&source);
        pending.push(PendingFileParse {
            path,
            source,
            hash,
            previous_path: None,
        });
    }

    Ok((pending, seen_files))
}

fn collect_refresh_scope_paths(scope: &HashSet<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for path in scope {
        if !path.exists() {
            continue;
        }
        if path.is_file() {
            if seen.insert(path.clone()) {
                paths.push(path.clone());
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        for entry in workspace_walk(path).filter_map(Result::ok) {
            if !entry
                .file_type()
                .map(|file_type| file_type.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let entry_path = entry.path().to_path_buf();
            if seen.insert(entry_path.clone()) {
                paths.push(entry_path);
            }
        }
    }
    Ok(paths)
}

pub(crate) fn path_matches_refresh_scope(path: &Path, refresh_scope: &HashSet<PathBuf>) -> bool {
    refresh_scope.iter().any(|candidate| {
        path == candidate || path.starts_with(candidate) || candidate.starts_with(path)
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ResolveGraphEdgesStats {
    pub(crate) unresolved_call_count: usize,
    pub(crate) unresolved_import_count: usize,
    pub(crate) unresolved_impl_count: usize,
    pub(crate) unresolved_intent_count: usize,
    pub(crate) resolve_calls_ms: u128,
    pub(crate) resolve_imports_ms: u128,
    pub(crate) resolve_impls_ms: u128,
    pub(crate) resolve_intents_ms: u128,
}

pub(crate) fn resolve_graph_edges(graph: &mut Graph) -> ResolveGraphEdgesStats {
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
    let unresolved_call_count = unresolved_calls.len();
    let unresolved_import_count = unresolved_imports.len();
    let unresolved_impl_count = unresolved_impls.len();
    let unresolved_intent_count = unresolved_intents.len();
    let resolve_calls_started = Instant::now();
    resolve_calls(graph, unresolved_calls);
    let resolve_calls_ms = resolve_calls_started.elapsed().as_millis();
    let resolve_imports_started = Instant::now();
    resolve_imports(graph, unresolved_imports);
    let resolve_imports_ms = resolve_imports_started.elapsed().as_millis();
    let resolve_impls_started = Instant::now();
    resolve_impls(graph, unresolved_impls);
    let resolve_impls_ms = resolve_impls_started.elapsed().as_millis();
    let resolve_intents_started = Instant::now();
    resolve_intents(graph, unresolved_intents);
    let resolve_intents_ms = resolve_intents_started.elapsed().as_millis();
    ResolveGraphEdgesStats {
        unresolved_call_count,
        unresolved_import_count,
        unresolved_impl_count,
        unresolved_intent_count,
        resolve_calls_ms,
        resolve_imports_ms,
        resolve_impls_ms,
        resolve_intents_ms,
    }
}
