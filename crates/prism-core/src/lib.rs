mod concept_events;
mod concept_relation_events;
mod contract_events;
mod coordination_persistence;
mod curator;
mod curator_support;
mod indexer;
mod indexer_support;
mod invalidation;
mod layout;
mod materialization;
mod memory_events;
mod memory_refresh;
pub mod mutation_trace;
mod outcome_backend;
mod parse_pipeline;
mod patch_outcomes;
mod prism_doc;
mod published_knowledge;
mod published_plans;
mod reanchor;
mod resolution;
mod session;
mod session_bootstrap;
mod shared_runtime;
mod shared_runtime_backend;
mod util;
mod validation_feedback;
mod watch;
mod workspace_identity;
mod workspace_tree;

use std::sync::Arc;

use anyhow::Result;
use prism_curator::CuratorBackend;
use prism_query::Prism;
use session_bootstrap::hydrate_workspace_session_with_options as bootstrap_workspace_session;

pub(crate) use indexer::PendingFileParse;
pub use indexer::WorkspaceIndexer;
pub use materialization::{WorkspaceBoundaryRegion, WorkspaceMaterializationSummary};
pub use prism_doc::{PrismDocSyncResult, PrismDocSyncStatus};
pub use session::{
    CoordinationPlanState, FsRefreshStatus, WorkspaceFsRefreshOutcome, WorkspaceSession,
    WorkspaceSnapshotRevisions,
};
pub use shared_runtime_backend::SharedRuntimeBackend;
pub use validation_feedback::{
    ValidationFeedbackCategory, ValidationFeedbackEntry, ValidationFeedbackRecord,
    ValidationFeedbackVerdict,
};

#[derive(Debug, Clone)]
pub struct WorkspaceSessionOptions {
    pub coordination: bool,
    pub shared_runtime: SharedRuntimeBackend,
    pub hydrate_persisted_projections: bool,
}

impl Default for WorkspaceSessionOptions {
    fn default() -> Self {
        Self {
            coordination: true,
            shared_runtime: SharedRuntimeBackend::Disabled,
            hydrate_persisted_projections: false,
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

pub fn hydrate_workspace_session(root: impl AsRef<std::path::Path>) -> Result<WorkspaceSession> {
    hydrate_workspace_session_with_options(root, WorkspaceSessionOptions::default())
}

pub fn hydrate_workspace_session_with_options(
    root: impl AsRef<std::path::Path>,
    options: WorkspaceSessionOptions,
) -> Result<WorkspaceSession> {
    bootstrap_workspace_session(root, options)
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
