use std::env;
use std::path::Path;
#[cfg(test)]
use std::sync::Mutex;

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
    let shared_runtime = if env_flag_enabled("PRISM_TEST_DISABLE_DEFAULT_SHARED_RUNTIME") {
        SharedRuntimeBackend::Disabled
    } else {
        default_workspace_shared_runtime(root)?
    };
    Ok(WorkspaceSessionOptions {
        shared_runtime,
        ..WorkspaceSessionOptions::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    static DEFAULT_OPTIONS_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_workspace_session_options_can_disable_shared_runtime_via_env() {
        let _guard = DEFAULT_OPTIONS_ENV_LOCK
            .lock()
            .expect("workspace session defaults env lock poisoned");
        let root = std::env::temp_dir();
        // SAFETY: this test serializes access to the process-wide env var and restores it
        // before releasing the lock.
        unsafe {
            env::set_var("PRISM_TEST_DISABLE_DEFAULT_SHARED_RUNTIME", "1");
        }
        let options = default_workspace_session_options(&root)
            .expect("default workspace session options should resolve");
        assert!(matches!(
            options.shared_runtime,
            SharedRuntimeBackend::Disabled
        ));
        // SAFETY: this test serializes access to the process-wide env var and restores it
        // before releasing the lock.
        unsafe {
            env::remove_var("PRISM_TEST_DISABLE_DEFAULT_SHARED_RUNTIME");
        }
    }
}
