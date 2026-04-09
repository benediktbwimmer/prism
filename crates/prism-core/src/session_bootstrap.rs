use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_store::{Graph, SqliteStore, WorkspaceTreeSnapshot};
use tracing::info;

use crate::indexer_support::build_workspace_session;
use crate::layout::discover_layout;
use crate::protected_state::runtime_sync::load_repo_protected_plan_state_or_default;
use crate::util::{cache_path, cleanup_legacy_cache};
use crate::{WorkspaceIndexer, WorkspaceSession, WorkspaceSessionOptions};

pub(crate) fn hydrate_workspace_session_with_options(
    root: impl AsRef<Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    start_workspace_session_with_options(root, options, None, false)
}

pub(crate) fn index_workspace_session_with_options(
    root: impl AsRef<Path>,
    options: WorkspaceSessionOptions,
    backend: Option<Arc<dyn CuratorBackend>>,
) -> Result<WorkspaceSession> {
    start_workspace_session_with_options(root, options, backend, true)
}

fn start_workspace_session_with_options(
    root: impl AsRef<Path>,
    options: WorkspaceSessionOptions,
    backend: Option<Arc<dyn CuratorBackend>>,
    force_index: bool,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    if options.runtime_mode == prism_ir::PrismRuntimeMode::CoordinationOnly {
        return start_coordination_only_workspace_session(root, options, backend);
    }
    let started = Instant::now();
    let cognition_enabled = options.cognition_enabled();
    let build_indexer_started = Instant::now();
    let mut indexer = WorkspaceIndexer::new_with_options(&root, options)?;
    let build_indexer_ms = build_indexer_started.elapsed().as_millis();
    if cognition_enabled && (force_index || !indexer.had_prior_snapshot) {
        let full_index_started = Instant::now();
        indexer.index()?;
        let full_index_ms = full_index_started.elapsed().as_millis();
        let into_session_started = Instant::now();
        let session = indexer.into_session(root.clone(), backend)?;
        info!(
            root = %root.display(),
            build_indexer_ms,
            full_index_ms,
            into_session_ms = into_session_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "bootstrapped prism workspace session from fresh index"
        );
        return Ok(session);
    }

    if !indexer.had_prior_snapshot {
        let into_session_started = Instant::now();
        let session = indexer.into_session(root.clone(), backend)?;
        info!(
            root = %root.display(),
            build_indexer_ms,
            into_session_ms = into_session_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "bootstrapped prism workspace session without graph indexing"
        );
        session
            .full_runtime_state()
            .expect("full runtime refresh state should exist after index bootstrap")
            .refresh_state
            .mark_fs_dirty_paths(std::iter::empty::<std::path::PathBuf>());
        return Ok(session);
    }

    info!(
        root = %root.display(),
        node_count = indexer.graph.node_count(),
        edge_count = indexer.graph.edge_count(),
        file_count = indexer.graph.file_count(),
        build_indexer_ms,
        "hydrated prism workspace session from persisted state"
    );
    let into_session_started = Instant::now();
    let session = indexer.into_session(root.clone(), backend)?;
    let into_session_ms = into_session_started.elapsed().as_millis();
    info!(
        root = %root.display(),
        build_indexer_ms,
        into_session_ms,
        total_ms = started.elapsed().as_millis(),
        "bootstrapped prism workspace session from persisted state"
    );
    session
        .full_runtime_state()
        .expect("full runtime refresh state should exist after index bootstrap")
        .refresh_state
        .mark_fs_dirty_paths(std::iter::empty::<std::path::PathBuf>());
    Ok(session)
}

fn start_coordination_only_workspace_session(
    root: impl AsRef<Path>,
    options: WorkspaceSessionOptions,
    backend: Option<Arc<dyn CuratorBackend>>,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let started = Instant::now();
    let shared_runtime = options.shared_runtime.clone();
    let coordination_enabled = options.coordination_enabled();
    let runtime_capabilities = options.runtime_capabilities();
    cleanup_legacy_cache(&root)?;
    let workspace_store_path = cache_path(&root)?;
    let mut store = SqliteStore::open(&workspace_store_path)?;
    let layout_started = Instant::now();
    let layout = discover_layout(&root)?;
    let discover_layout_ms = layout_started.elapsed().as_millis();
    let coordination_state_started = Instant::now();
    let coordination_state = load_repo_protected_plan_state_or_default(&root, &mut store)?;
    let load_coordination_ms = coordination_state_started.elapsed().as_millis();
    let session = build_workspace_session(
        root.clone(),
        store,
        WorkspaceTreeSnapshot::default(),
        shared_runtime,
        false,
        false,
        layout,
        Graph::default(),
        HistoryStore::default(),
        OutcomeMemory::default(),
        coordination_state.snapshot,
        coordination_state.runtime_descriptors,
        ProjectionIndex::default(),
        None,
        coordination_enabled,
        None,
        false,
        runtime_capabilities,
        backend,
    )?;
    info!(
        root = %root.display(),
        discover_layout_ms,
        load_coordination_ms,
        total_ms = started.elapsed().as_millis(),
        "bootstrapped coordination-only workspace session without indexer state"
    );
    Ok(session)
}
