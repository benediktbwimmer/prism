use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::Result;
use prism_coordination::CoordinationSnapshot;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::{EdgeKind, PlanExecutionOverlay, PlanGraph};
use prism_memory::OutcomeMemory;
use prism_parser::LanguageAdapter;
use prism_projections::ProjectionIndex;
use prism_store::{Graph, SqliteStore, WorkspaceTreeSnapshot};
use tracing::info;

use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::curator::{CuratorHandle, CuratorHandleRef};
use crate::indexer::PendingFileParse;
use crate::resolution::{resolve_calls, resolve_impls, resolve_imports, resolve_intents};
use crate::session::{WorkspaceRefreshSeed, WorkspaceRefreshState, WorkspaceSession};
use crate::shared_runtime::composite_workspace_revision;
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::util::{persisted_file_hash, workspace_walk};
use crate::watch::spawn_fs_watch;
use crate::workspace_identity::coordination_persist_context_for_root;
use crate::workspace_runtime_state::WorkspaceRuntimeState;

pub(crate) fn build_workspace_session(
    root: PathBuf,
    store: SqliteStore,
    workspace_tree_snapshot: WorkspaceTreeSnapshot,
    shared_runtime: SharedRuntimeBackend,
    shared_runtime_store: Option<SqliteStore>,
    graph: Graph,
    history: HistoryStore,
    outcomes: OutcomeMemory,
    coordination_snapshot: CoordinationSnapshot,
    plan_graphs: Vec<PlanGraph>,
    plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    projections: ProjectionIndex,
    initial_refresh: Option<WorkspaceRefreshSeed>,
    coordination_enabled: bool,
    backend: Option<Arc<dyn CuratorBackend>>,
) -> Result<WorkspaceSession> {
    let started = Instant::now();
    let workspace_revision = composite_workspace_revision(
        store.workspace_revision()?,
        shared_runtime_store
            .as_ref()
            .map(SqliteStore::workspace_revision)
            .transpose()?,
    );
    let cold_query_store = store.reopen_runtime_reader()?;
    let curator_store = store.reopen_runtime_reader()?;
    let loaded_workspace_revision = Arc::new(AtomicU64::new(workspace_revision));
    let store = Arc::new(Mutex::new(store));
    let cold_query_store = Arc::new(Mutex::new(cold_query_store));
    let shared_runtime_store = shared_runtime_store.map(|store| Arc::new(Mutex::new(store)));
    let shared_runtime_materializer = shared_runtime_store
        .as_ref()
        .map(|store| CheckpointMaterializerHandle::new(root.clone(), Arc::clone(store)));
    let runtime_state = Arc::new(Mutex::new(WorkspaceRuntimeState::new(
        graph,
        history,
        outcomes,
        coordination_snapshot,
        plan_graphs,
        plan_execution_overlays,
        projections,
    )));
    let prism = Arc::new(
        runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .publish_prism(
                prism_ir::WorkspaceRevision {
                    graph_version: store
                        .lock()
                        .expect("workspace store lock poisoned")
                        .workspace_revision()?,
                    git_commit: None,
                },
                Some(coordination_persist_context_for_root(&root, None)),
            ),
    );
    WorkspaceSession::attach_cold_query_backends(prism.as_ref(), &cold_query_store);
    let prism = Arc::new(RwLock::new(prism));
    let refresh_lock = Arc::new(Mutex::new(()));
    let refresh_state = Arc::new(WorkspaceRefreshState::new());
    if let Some(refresh) = initial_refresh {
        refresh_state.record_runtime_refresh_observation_with_work(
            refresh.path,
            refresh.duration_ms,
            workspace_revision,
            refresh.work,
        );
    }
    let fs_snapshot = Arc::new(Mutex::new(workspace_tree_snapshot));
    let load_curator_snapshot_ms = 0_u128;
    let curator = CuratorHandle::new(
        backend,
        Arc::clone(&prism),
        Arc::clone(&store),
        Arc::new(Mutex::new(curator_store)),
        Arc::clone(&refresh_lock),
    );
    let checkpoint_materializer =
        CheckpointMaterializerHandle::new(root.clone(), Arc::clone(&store));
    let watch_started = Instant::now();
    let watch = Some(spawn_fs_watch(
        root.clone(),
        Arc::clone(&prism),
        Arc::clone(&runtime_state),
        Arc::clone(&store),
        Arc::clone(&cold_query_store),
        shared_runtime.sqlite_path().map(Path::to_path_buf),
        Arc::clone(&refresh_lock),
        Arc::clone(&refresh_state),
        Arc::clone(&loaded_workspace_revision),
        Arc::clone(&fs_snapshot),
        Some(checkpoint_materializer.clone()),
        shared_runtime_materializer.clone(),
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
        load_curator_snapshot_ms,
        watch_start_ms,
        total_ms = started.elapsed().as_millis(),
        "built prism workspace session"
    );
    Ok(WorkspaceSession {
        root,
        prism,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime,
        shared_runtime_store,
        refresh_lock,
        refresh_state,
        loaded_workspace_revision,
        fs_snapshot,
        watch,
        curator: Some(curator),
        checkpoint_materializer: Some(checkpoint_materializer),
        shared_runtime_materializer,
        coordination_enabled,
    })
}

pub(crate) fn collect_pending_file_parses(
    walk_root: &Path,
    adapters: &[Box<dyn LanguageAdapter + Send + Sync>],
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
        if !seen_files.insert(path.clone()) {
            continue;
        }

        let supported_by_adapter = adapters.iter().any(|adapter| adapter.supports_path(&path));
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if supported_by_adapter => return Err(error.into()),
            Err(_) => continue,
        };
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
    pub(crate) cleared_derived_edge_count: usize,
    pub(crate) resolution_scope_path_count: usize,
    pub(crate) resolution_scope_node_count: usize,
    pub(crate) unresolved_call_count: usize,
    pub(crate) unresolved_import_count: usize,
    pub(crate) unresolved_impl_count: usize,
    pub(crate) unresolved_intent_count: usize,
    pub(crate) collect_scope_nodes_ms: u128,
    pub(crate) clear_derived_edges_ms: u128,
    pub(crate) collect_unresolved_ms: u128,
    pub(crate) resolve_calls_ms: u128,
    pub(crate) resolve_imports_ms: u128,
    pub(crate) resolve_impls_ms: u128,
    pub(crate) resolve_intents_ms: u128,
    pub(crate) extend_edges_ms: u128,
}

pub(crate) fn resolve_graph_edges(
    graph: &mut Graph,
    refresh_scope: Option<&HashSet<PathBuf>>,
) -> ResolveGraphEdgesStats {
    let scope_nodes_started = Instant::now();
    let scope_nodes = refresh_scope.map(|scope| graph.node_ids_for_paths(scope));
    let collect_scope_nodes_ms = scope_nodes_started.elapsed().as_millis();
    let clear_derived_edges_started = Instant::now();
    let (cleared_derived_edge_count, resolution_scope_path_count, resolution_scope_node_count) =
        if let Some(scope) = refresh_scope {
            let scope_nodes = scope_nodes
                .as_ref()
                .expect("scope_nodes available for refresh scope");
            (
                graph.clear_derived_edges_for_nodes(scope_nodes),
                scope.len(),
                scope_nodes.len(),
            )
        } else {
            (
                graph.clear_edges_by_kind(&[
                    EdgeKind::Calls,
                    EdgeKind::Imports,
                    EdgeKind::Implements,
                    EdgeKind::Specifies,
                    EdgeKind::Validates,
                    EdgeKind::RelatedTo,
                ]),
                graph.file_count(),
                graph.node_count(),
            )
        };
    let clear_derived_edges_ms = clear_derived_edges_started.elapsed().as_millis();
    let collect_unresolved_started = Instant::now();
    let unresolved_calls = if let Some(scope) = refresh_scope {
        graph.unresolved_calls_for_paths(scope)
    } else {
        graph.unresolved_calls()
    };
    let unresolved_imports = if let Some(scope) = refresh_scope {
        graph.unresolved_imports_for_paths(scope)
    } else {
        graph.unresolved_imports()
    };
    let unresolved_impls = if let Some(scope) = refresh_scope {
        graph.unresolved_impls_for_paths(scope)
    } else {
        graph.unresolved_impls()
    };
    let unresolved_intents = if let Some(scope) = refresh_scope {
        graph.unresolved_intents_for_paths(scope)
    } else {
        graph.unresolved_intents()
    };
    let collect_unresolved_ms = collect_unresolved_started.elapsed().as_millis();
    let unresolved_call_count = unresolved_calls.len();
    let unresolved_import_count = unresolved_imports.len();
    let unresolved_impl_count = unresolved_impls.len();
    let unresolved_intent_count = unresolved_intents.len();
    let resolve_calls_started = Instant::now();
    let resolved_calls = resolve_calls(graph, unresolved_calls);
    let resolve_calls_ms = resolve_calls_started.elapsed().as_millis();
    let resolve_imports_started = Instant::now();
    let resolved_imports = resolve_imports(graph, unresolved_imports);
    let resolve_imports_ms = resolve_imports_started.elapsed().as_millis();
    let resolve_impls_started = Instant::now();
    let resolved_impls = resolve_impls(graph, unresolved_impls);
    let resolve_impls_ms = resolve_impls_started.elapsed().as_millis();
    let resolve_intents_started = Instant::now();
    let resolved_intents = resolve_intents(graph, unresolved_intents);
    let resolve_intents_ms = resolve_intents_started.elapsed().as_millis();
    let extend_edges_started = Instant::now();
    graph.extend_edges(resolved_calls);
    graph.extend_edges(resolved_imports);
    graph.extend_edges(resolved_impls);
    graph.extend_edges(resolved_intents);
    let extend_edges_ms = extend_edges_started.elapsed().as_millis();
    ResolveGraphEdgesStats {
        cleared_derived_edge_count,
        resolution_scope_path_count,
        resolution_scope_node_count,
        unresolved_call_count,
        unresolved_import_count,
        unresolved_impl_count,
        unresolved_intent_count,
        collect_scope_nodes_ms,
        clear_derived_edges_ms,
        collect_unresolved_ms,
        resolve_calls_ms,
        resolve_imports_ms,
        resolve_impls_ms,
        resolve_intents_ms,
        extend_edges_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_graph_edges;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use prism_ir::{EdgeKind, FileId, Language, Node, NodeId, NodeKind, Span, UnresolvedCall};
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

    #[test]
    fn scoped_edge_resolution_replaces_only_edges_for_scoped_files() {
        let alpha_path = Path::new("src/alpha.rs");
        let beta_path = Path::new("src/beta.rs");
        let caller_alpha_path = PathBuf::from("src/caller_alpha.rs");
        let caller_beta_path = PathBuf::from("src/caller_beta.rs");
        let mut graph = Graph::new();

        let alpha_file = graph.ensure_file(alpha_path);
        let beta_file = graph.ensure_file(beta_path);
        let caller_alpha_file = graph.ensure_file(&caller_alpha_path);
        let caller_beta_file = graph.ensure_file(&caller_beta_path);

        let alpha = function(alpha_file, "alpha");
        let beta = function(beta_file, "beta");
        let caller_alpha = function(caller_alpha_file, "caller_alpha");
        let caller_beta = function(caller_beta_file, "caller_beta");

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
            &caller_alpha_path,
            1,
            vec![caller_alpha.clone()],
            Vec::new(),
            HashMap::new(),
            vec![unresolved_call(&caller_alpha, "demo", "alpha")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        graph.upsert_file(
            &caller_beta_path,
            1,
            vec![caller_beta.clone()],
            Vec::new(),
            HashMap::new(),
            vec![unresolved_call(&caller_beta, "demo", "beta")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        resolve_graph_edges(&mut graph, None);
        assert!(graph
            .edges_from(&caller_alpha.id, Some(EdgeKind::Calls))
            .iter()
            .any(|edge| edge.target == alpha.id));
        assert!(graph
            .edges_from(&caller_beta.id, Some(EdgeKind::Calls))
            .iter()
            .any(|edge| edge.target == beta.id));

        graph.upsert_file(
            &caller_alpha_path,
            2,
            vec![caller_alpha.clone()],
            Vec::new(),
            HashMap::new(),
            vec![unresolved_call(&caller_alpha, "demo", "beta")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let scope = std::iter::once(caller_alpha_path.clone()).collect();
        let stats = resolve_graph_edges(&mut graph, Some(&scope));
        assert_eq!(stats.resolution_scope_path_count, 1);
        assert_eq!(stats.unresolved_call_count, 1);
        assert!(graph
            .edges_from(&caller_alpha.id, Some(EdgeKind::Calls))
            .iter()
            .any(|edge| edge.target == beta.id));
        assert!(!graph
            .edges_from(&caller_alpha.id, Some(EdgeKind::Calls))
            .iter()
            .any(|edge| edge.target == alpha.id));
        assert!(graph
            .edges_from(&caller_beta.id, Some(EdgeKind::Calls))
            .iter()
            .any(|edge| edge.target == beta.id));
    }
}
