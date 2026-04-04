use std::fs;
use std::path::Path;

use anyhow::Result;
use prism_coordination::RuntimeDiscoveryMode;

use crate::PrismPaths;

pub const PEER_RUNTIME_READ_PATH: &str = "/peer/runtime/read";

pub fn local_peer_runtime_endpoint(root: &Path) -> Result<Option<String>> {
    let uri_path = PrismPaths::for_workspace_root(root)?.mcp_http_uri_path()?;
    let Ok(uri) = fs::read_to_string(uri_path) else {
        return Ok(None);
    };
    let uri = uri.trim();
    if uri.is_empty() {
        return Ok(None);
    }
    let base = uri.trim_end_matches('/');
    let base = base.strip_suffix("/mcp").unwrap_or(base);
    Ok(Some(format!(
        "{}/{}",
        base,
        PEER_RUNTIME_READ_PATH.trim_start_matches('/')
    )))
}

pub fn local_peer_runtime_discovery_mode(endpoint: Option<&str>) -> RuntimeDiscoveryMode {
    if endpoint.is_some() {
        RuntimeDiscoveryMode::LanDirect
    } else {
        RuntimeDiscoveryMode::None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::local_peer_runtime_endpoint;

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_git_repo() -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-peer-runtime-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&root)
            .output()
            .expect("git init should succeed");
        root
    }

    #[test]
    fn local_peer_runtime_endpoint_strips_mcp_suffix() {
        let root = temp_git_repo();
        let uri_path = crate::PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        fs::write(&uri_path, "http://127.0.0.1:52695/mcp\n").unwrap();
        assert_eq!(
            local_peer_runtime_endpoint(&root).unwrap().as_deref(),
            Some("http://127.0.0.1:52695/peer/runtime/read")
        );
    }
}
