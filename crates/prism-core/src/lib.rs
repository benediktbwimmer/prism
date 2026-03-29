mod concept_events;
mod concept_relation_events;
mod coordination_persistence;
mod curator;
mod curator_support;
mod indexer;
mod indexer_support;
mod layout;
mod memory_events;
mod memory_refresh;
mod parse_pipeline;
mod patch_outcomes;
mod published_knowledge;
mod published_plans;
mod reanchor;
mod resolution;
mod session;
mod shared_runtime;
mod util;
mod validation_feedback;
mod watch;
mod workspace_identity;

use std::sync::Arc;

use anyhow::Result;
use prism_curator::CuratorBackend;
use prism_query::Prism;

pub(crate) use indexer::PendingFileParse;
pub use indexer::WorkspaceIndexer;
pub use session::{
    CoordinationPlanState, FsRefreshStatus, WorkspaceSession, WorkspaceSnapshotRevisions,
};
pub use validation_feedback::{
    ValidationFeedbackCategory, ValidationFeedbackEntry, ValidationFeedbackRecord,
    ValidationFeedbackVerdict,
};

#[derive(Debug, Clone)]
pub struct WorkspaceSessionOptions {
    pub coordination: bool,
    pub shared_runtime_sqlite: Option<std::path::PathBuf>,
}

impl Default for WorkspaceSessionOptions {
    fn default() -> Self {
        Self {
            coordination: true,
            shared_runtime_sqlite: None,
        }
    }
}

pub fn index_workspace(root: impl AsRef<std::path::Path>) -> Result<Prism> {
    let mut indexer = WorkspaceIndexer::new(root)?;
    indexer.index()?;
    Ok(indexer.into_prism())
}

pub fn index_workspace_session(root: impl AsRef<std::path::Path>) -> Result<WorkspaceSession> {
    index_workspace_session_with_options(root, WorkspaceSessionOptions::default())
}

pub fn index_workspace_session_with_options(
    root: impl AsRef<std::path::Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new_with_options(&root, options)?;
    indexer.index()?;
    indexer.into_session(root, None)
}

pub fn index_workspace_session_with_curator(
    root: impl AsRef<std::path::Path>,
    backend: Arc<dyn CuratorBackend>,
) -> Result<WorkspaceSession> {
    index_workspace_session_with_curator_and_options(
        root,
        backend,
        WorkspaceSessionOptions::default(),
    )
}

pub fn index_workspace_session_with_curator_and_options(
    root: impl AsRef<std::path::Path>,
    backend: Arc<dyn CuratorBackend>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    let root = root.as_ref().canonicalize()?;
    let mut indexer = WorkspaceIndexer::new_with_options(&root, options)?;
    indexer.index()?;
    indexer.into_session(root, Some(backend))
}

#[cfg(test)]
mod tests;
