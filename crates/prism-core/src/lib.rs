mod curator;
mod curator_support;
mod indexer;
mod indexer_support;
mod layout;
mod patch_outcomes;
mod reanchor;
mod resolution;
mod session;
mod util;
mod watch;

use std::sync::Arc;

use anyhow::Result;
use prism_curator::CuratorBackend;
use prism_query::Prism;

pub(crate) use indexer::PendingFileParse;
pub use indexer::WorkspaceIndexer;
pub use session::WorkspaceSession;

pub fn index_workspace(root: impl AsRef<std::path::Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub fn index_workspace_session(root: impl AsRef<std::path::Path>) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new(&root)?;
    indexer.index()?;
    indexer.into_session(root, None)
}

pub fn index_workspace_session_with_curator(
    root: impl AsRef<std::path::Path>,
    backend: Arc<dyn CuratorBackend>,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new(&root)?;
    indexer.index()?;
    indexer.into_session(root, Some(backend))
}

#[cfg(test)]
mod tests;
