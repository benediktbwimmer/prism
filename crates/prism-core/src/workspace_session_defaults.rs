use std::path::Path;

use anyhow::Result;

use crate::{PrismPaths, SharedRuntimeBackend, WorkspaceSessionOptions};

pub fn default_workspace_shared_runtime(root: impl AsRef<Path>) -> Result<SharedRuntimeBackend> {
    let paths = PrismPaths::for_workspace_root(root.as_ref())?;
    Ok(SharedRuntimeBackend::Sqlite {
        path: paths.shared_runtime_db_path()?,
    })
}

pub fn default_workspace_session_options(
    root: impl AsRef<Path>,
) -> Result<WorkspaceSessionOptions> {
    Ok(WorkspaceSessionOptions {
        shared_runtime: default_workspace_shared_runtime(root)?,
        ..WorkspaceSessionOptions::default()
    })
}
