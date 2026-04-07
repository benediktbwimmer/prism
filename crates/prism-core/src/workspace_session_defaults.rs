use std::path::Path;

use anyhow::Result;

use crate::{SharedRuntimeBackend, WorkspaceSessionOptions};

pub fn default_workspace_shared_runtime(root: impl AsRef<Path>) -> Result<SharedRuntimeBackend> {
    let _ = root.as_ref();
    Ok(SharedRuntimeBackend::Disabled)
}

pub fn default_workspace_session_options(
    root: impl AsRef<Path>,
) -> Result<WorkspaceSessionOptions> {
    Ok(WorkspaceSessionOptions {
        shared_runtime: default_workspace_shared_runtime(root)?,
        ..WorkspaceSessionOptions::default()
    })
}
