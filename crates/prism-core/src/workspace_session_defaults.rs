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

pub(crate) fn runtime_rebuild_session_options(
    coordination: bool,
    shared_runtime: &SharedRuntimeBackend,
) -> WorkspaceSessionOptions {
    WorkspaceSessionOptions {
        coordination,
        shared_runtime: shared_runtime.clone(),
        hydrate_persisted_projections: false,
        hydrate_persisted_co_change: true,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::runtime_rebuild_session_options;
    use crate::SharedRuntimeBackend;

    #[test]
    fn runtime_rebuild_session_options_preserves_backend_variant() {
        let remote = SharedRuntimeBackend::Remote {
            uri: "https://runtime.example/prism".to_string(),
        };
        let options = runtime_rebuild_session_options(true, &remote);
        assert_eq!(options.shared_runtime, remote);
        assert!(options.coordination);
        assert!(!options.hydrate_persisted_projections);
        assert!(options.hydrate_persisted_co_change);

        let sqlite = SharedRuntimeBackend::Sqlite {
            path: PathBuf::from("/tmp/shared-runtime.db"),
        };
        let sqlite_options = runtime_rebuild_session_options(false, &sqlite);
        assert_eq!(sqlite_options.shared_runtime, sqlite);
        assert!(!sqlite_options.coordination);
    }
}
