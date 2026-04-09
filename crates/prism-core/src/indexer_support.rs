use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use anyhow::Result;
use prism_coordination::{CoordinationSnapshot, RuntimeDescriptor};
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::{EdgeKind, PrismRuntimeCapabilities, PrismRuntimeMode};
use prism_memory::OutcomeMemory;
use prism_parser::LanguageAdapter;
use prism_projections::{IntentIndex, ProjectionIndex};
use prism_store::{Graph, SqliteStore, WorkspaceTreeSnapshot};
use tracing::info;

use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::coordination_authority_api::coordination_authority_live_sync_enabled;
use crate::curator::{CuratorHandle, CuratorHandleRef};
use crate::indexer::PendingFileParse;
use crate::local_principal_registry::ensure_local_principal_registry_snapshot;
use crate::observed_change_tracker::ObservedChangeTracker;
use crate::protected_state::runtime_sync::sync_repo_protected_state;
use crate::resolution::{resolve_calls, resolve_impls, resolve_imports, resolve_intents};
use crate::session::{
    WorkspaceRefreshSeed, WorkspaceRefreshState, WorkspaceSession, WorkspaceSessionFullRuntime,
};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::util::{persisted_file_hash, workspace_walk};
use crate::watch::{
    spawn_coordination_authority_watch, spawn_fs_watch, spawn_protected_state_watch,
};
use crate::workspace_identity::coordination_persist_context_for_root;
use crate::workspace_runtime_state::WorkspaceRuntimeState;

pub(crate) fn build_workspace_session(
    root: PathBuf,
    mut store: SqliteStore,
    workspace_tree_snapshot: WorkspaceTreeSnapshot,
    shared_runtime: SharedRuntimeBackend,
    hydrate_persisted_projections: bool,
    hydrate_persisted_co_change: bool,
    layout: crate::layout::WorkspaceLayout,
    graph: Graph,
    history: HistoryStore,
    outcomes: OutcomeMemory,
    coordination_snapshot: CoordinationSnapshot,
    runtime_descriptors: Vec<RuntimeDescriptor>,
    projections: ProjectionIndex,
    initial_refresh: Option<WorkspaceRefreshSeed>,
    coordination_enabled: bool,
    startup_intent: Option<IntentIndex>,
    trust_cached_query_state: bool,
    runtime_capabilities: PrismRuntimeCapabilities,
    backend: Option<Arc<dyn CuratorBackend>>,
) -> Result<WorkspaceSession> {
    let started = Instant::now();
    let coordination_only_runtime = matches!(
        PrismRuntimeMode::from_capabilities(runtime_capabilities),
        Some(PrismRuntimeMode::CoordinationOnly)
    );
    let sync_repo_protected_state_ms = if coordination_only_runtime {
        0
    } else {
        let sync_protected_started = Instant::now();
        sync_repo_protected_state(&root, &mut store, runtime_capabilities)?;
        sync_protected_started.elapsed().as_millis()
    };
    let workspace_revision_started = Instant::now();
    let workspace_revision = store.workspace_revision()?;
    let workspace_revision_ms = workspace_revision_started.elapsed().as_millis();
    let reopen_stores_started = Instant::now();
    let cold_query_store = if coordination_only_runtime {
        None
    } else {
        Some(store.reopen_runtime_reader()?)
    };
    let curator_store = if coordination_only_runtime {
        None
    } else {
        Some(store.reopen_runtime_reader()?)
    };
    let reopen_runtime_readers_ms = reopen_stores_started.elapsed().as_millis();
    let loaded_workspace_revision = Arc::new(AtomicU64::new(workspace_revision));
    let coordination_runtime_revision = Arc::new(AtomicU64::new(0));
    let store = Arc::new(Mutex::new(store));
    let cold_query_store = cold_query_store
        .map(|store| Arc::new(Mutex::new(store)))
        .unwrap_or_else(|| Arc::clone(&store));
    let load_principal_registry_started = Instant::now();
    let principal_registry = if coordination_only_runtime {
        Default::default()
    } else {
        let mut store = store.lock().expect("workspace store lock poisoned");
        ensure_local_principal_registry_snapshot(&root, &mut *store)?.unwrap_or_default()
    };
    let load_principal_registry_ms = load_principal_registry_started.elapsed().as_millis();
    let runtime_state_started = Instant::now();
    let runtime_state = Arc::new(Mutex::new(WorkspaceRuntimeState::new(
        layout,
        graph,
        history,
        outcomes,
        coordination_snapshot,
        runtime_descriptors,
        projections,
        runtime_capabilities,
    )));
    let build_runtime_state_ms = runtime_state_started.elapsed().as_millis();
    let publish_generation_started = Instant::now();
    let published_generation = Arc::new(RwLock::new(
        runtime_state
            .lock()
            .expect("workspace runtime state lock poisoned")
            .publish_generation_with_intent(
                prism_ir::WorkspaceRevision {
                    graph_version: store
                        .lock()
                        .expect("workspace store lock poisoned")
                        .workspace_revision()?,
                    git_commit: None,
                },
                Some(coordination_persist_context_for_root(&root, None)),
                startup_intent,
            ),
    ));
    let _ = trust_cached_query_state;
    let publish_generation_ms = publish_generation_started.elapsed().as_millis();
    let attach_cold_backends_started = Instant::now();
    if !coordination_only_runtime {
        WorkspaceSession::attach_cold_query_backends(
            published_generation
                .read()
                .expect("workspace published generation lock poisoned")
                .prism_arc()
                .as_ref(),
            &cold_query_store,
        );
    }
    let attach_cold_query_backends_ms = attach_cold_backends_started.elapsed().as_millis();
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
    let observed_change_tracker = Arc::new(Mutex::new(ObservedChangeTracker::default()));
    let worktree_mutator_slot = Arc::new(Mutex::new(None));
    let worktree_principal_binding = Arc::new(Mutex::new(None));
    let checkpoint_materializer = if coordination_only_runtime {
        None
    } else {
        Some(CheckpointMaterializerHandle::new(
            root.clone(),
            Arc::clone(&store),
        ))
    };
    let load_curator_snapshot_ms = 0_u128;
    let curator = if coordination_only_runtime {
        None
    } else {
        Some(CuratorHandle::new(
            backend,
            Arc::clone(&published_generation),
            Arc::clone(&store),
            Arc::new(Mutex::new(curator_store.expect(
                "curator store should exist outside coordination-only mode",
            ))),
            checkpoint_materializer.clone(),
            Arc::clone(&refresh_lock),
        ))
    };
    // Coordination-only mode intentionally stays off the workspace watch pipeline for now.
    let live_watches_enabled =
        !coordination_only_runtime && !env_flag_enabled("PRISM_TEST_DISABLE_LIVE_WATCHERS");
    let watch_started = Instant::now();
    let (watch, protected_state_watch, coordination_authority_watch) = if live_watches_enabled {
        let watch = Some(spawn_fs_watch(
            root.clone(),
            Arc::clone(&published_generation),
            Arc::clone(&runtime_state),
            Arc::clone(&store),
            Arc::clone(&cold_query_store),
            Arc::clone(&refresh_lock),
            Arc::clone(&refresh_state),
            Arc::clone(&loaded_workspace_revision),
            Arc::clone(&fs_snapshot),
            checkpoint_materializer.clone(),
            coordination_enabled,
            curator.as_ref().map(CuratorHandleRef::from),
            Arc::clone(&observed_change_tracker),
            Arc::clone(&worktree_principal_binding),
        )?);
        let protected_state_watch = Some(spawn_protected_state_watch(
            root.clone(),
            Arc::clone(&published_generation),
            Arc::clone(&runtime_state),
            Arc::clone(&store),
            Arc::clone(&cold_query_store),
            Arc::clone(&refresh_lock),
            Arc::clone(&loaded_workspace_revision),
            coordination_enabled,
        )?);
        let coordination_authority_watch = if coordination_authority_live_sync_enabled(&root)? {
            Some(spawn_coordination_authority_watch(
                root.clone(),
                Arc::clone(&published_generation),
                Arc::clone(&runtime_state),
                Arc::clone(&store),
                Arc::clone(&cold_query_store),
                Arc::clone(&refresh_lock),
                Arc::clone(&loaded_workspace_revision),
                Arc::clone(&coordination_runtime_revision),
                coordination_enabled,
            )?)
        } else {
            None
        };
        (watch, protected_state_watch, coordination_authority_watch)
    } else {
        (None, None, None)
    };
    let watch_start_ms = watch_started.elapsed().as_millis();
    let graph = published_generation
        .read()
        .expect("workspace published generation lock poisoned");
    let prism = graph.prism_arc();
    let node_count = prism.graph().node_count();
    let edge_count = prism.graph().edge_count();
    let file_count = prism.graph().file_count();
    drop(graph);
    info!(
        root = %root.display(),
        coordination_enabled,
        live_watches_enabled,
        node_count,
        edge_count,
        file_count,
        sync_repo_protected_state_ms,
        workspace_revision_ms,
        reopen_runtime_readers_ms,
        load_principal_registry_ms,
        build_runtime_state_ms,
        publish_generation_ms,
        attach_cold_query_backends_ms,
        load_curator_snapshot_ms,
        watch_start_ms,
        total_ms = started.elapsed().as_millis(),
        "built prism workspace session"
    );
    let full_runtime = if coordination_only_runtime {
        None
    } else {
        Some(WorkspaceSessionFullRuntime {
            repo_projection_sync_pending: Arc::new(AtomicBool::new(false)),
            repo_patch_provenance_sync_pending: Arc::new(AtomicBool::new(false)),
            refresh_lock,
            refresh_state,
            fs_snapshot,
            watch,
            protected_state_watch,
            coordination_authority_watch,
            curator,
            checkpoint_materializer,
            observed_change_tracker,
        })
    };
    Ok(WorkspaceSession {
        root,
        published_generation,
        runtime_state,
        store,
        cold_query_store,
        shared_runtime,
        hydrate_persisted_projections,
        hydrate_persisted_co_change,
        principal_registry: Arc::new(RwLock::new(principal_registry)),
        loaded_workspace_revision,
        coordination_runtime_revision,
        full_runtime,
        coordination_enabled,
        worktree_mutator_slot,
        worktree_principal_binding,
    })
}

fn env_flag_enabled(name: &str) -> bool {
    env::var_os(name)
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false)
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
