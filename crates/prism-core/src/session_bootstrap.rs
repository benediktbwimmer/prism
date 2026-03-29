use std::path::Path;

use anyhow::Result;
use tracing::info;

use crate::{WorkspaceIndexer, WorkspaceSession, WorkspaceSessionOptions};

pub(crate) fn hydrate_workspace_session_with_options(
    root: impl AsRef<Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new_with_options(&root, options)?;
    if !indexer.had_prior_snapshot {
        indexer.index()?;
        return indexer.into_session(root, None);
    }

    info!(
        root = %root.display(),
        node_count = indexer.graph.node_count(),
        edge_count = indexer.graph.edge_count(),
        file_count = indexer.graph.file_count(),
        "hydrated prism workspace session from persisted state"
    );
    let session = indexer.into_session(root, None)?;
    session
        .refresh_state
        .mark_fs_dirty_paths(std::iter::empty::<std::path::PathBuf>());
    Ok(session)
}
