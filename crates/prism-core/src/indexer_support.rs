use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

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
use walkdir::WalkDir;

use crate::curator::{CuratorHandle, CuratorHandleRef};
use crate::indexer::PendingFileParse;
use crate::resolution::{resolve_calls, resolve_impls, resolve_imports, resolve_intents};
use crate::session::WorkspaceSession;
use crate::util::{should_walk, stable_hash};
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
    let curator_snapshot = {
        let mut store = store.lock().expect("workspace store lock poisoned");
        store.load_curator_snapshot()?.unwrap_or_default()
    };
    let curator = CuratorHandle::new(
        curator_snapshot,
        backend,
        Arc::clone(&store),
        Arc::clone(&refresh_lock),
    );
    let watch = Some(spawn_fs_watch(
        root.clone(),
        Arc::clone(&prism),
        Arc::clone(&store),
        Arc::clone(&refresh_lock),
        coordination_enabled,
        Some(CuratorHandleRef::from(&curator)),
    )?);
    Ok(WorkspaceSession {
        root,
        prism,
        store,
        refresh_lock,
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

    for entry in WalkDir::new(walk_root)
        .into_iter()
        .filter_entry(|entry| should_walk(entry.path(), walk_root))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let Some(_adapter) = adapters.iter().find(|adapter| adapter.supports_path(path)) else {
            continue;
        };

        let canonical_path = path.to_path_buf();
        seen_files.insert(canonical_path.clone());
        let source = fs::read_to_string(path)?;
        let hash = stable_hash(&source);
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
