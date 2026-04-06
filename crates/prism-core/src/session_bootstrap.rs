use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use prism_curator::CuratorBackend;
use tracing::info;

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
        .refresh_state
        .mark_fs_dirty_paths(std::iter::empty::<std::path::PathBuf>());
    Ok(session)
}
