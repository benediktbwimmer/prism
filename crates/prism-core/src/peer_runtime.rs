use std::fs;
use std::path::Path;

use anyhow::Result;
use prism_coordination::{RuntimeDescriptor, RuntimeDiscoveryMode};

use crate::PrismPaths;
use crate::workspace_identity::workspace_identity_for_root;

pub const PEER_RUNTIME_QUERY_PATH: &str = "/peer/query";

pub fn local_runtime_id(root: &Path) -> String {
    workspace_identity_for_root(root).instance_id
}

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
        PEER_RUNTIME_QUERY_PATH.trim_start_matches('/')
    )))
}

pub fn configured_public_runtime_endpoint(root: &Path) -> Result<Option<String>> {
    let path = PrismPaths::for_workspace_root(root)?.mcp_public_url_path()?;
    let Ok(value) = fs::read_to_string(path) else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

pub fn runtime_query_endpoint(descriptor: &RuntimeDescriptor) -> Option<&str> {
    descriptor
        .public_endpoint
        .as_deref()
        .or(descriptor.peer_endpoint.as_deref())
}

pub fn local_peer_runtime_discovery_mode(
    peer_endpoint: Option<&str>,
    public_endpoint: Option<&str>,
) -> RuntimeDiscoveryMode {
    if public_endpoint.is_some() {
        RuntimeDiscoveryMode::PublicUrl
    } else if peer_endpoint.is_some() {
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

    use super::{
        configured_public_runtime_endpoint, local_peer_runtime_discovery_mode,
        local_peer_runtime_endpoint, PEER_RUNTIME_QUERY_PATH,
    };
    use crate::PrismPaths;

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
        let uri_path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_http_uri_path()
            .unwrap();
        fs::create_dir_all(uri_path.parent().unwrap()).unwrap();
        fs::write(&uri_path, "http://127.0.0.1:52695/mcp\n").unwrap();
        assert_eq!(
            local_peer_runtime_endpoint(&root).unwrap().as_deref(),
            Some("http://127.0.0.1:52695/peer/query")
        );
    }

    #[test]
    fn configured_public_runtime_endpoint_reads_trimmed_value() {
        let root = temp_git_repo();
        let path = PrismPaths::for_workspace_root(&root)
            .unwrap()
            .mcp_public_url_path()
            .unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, " https://runtime.example/peer/query \n").unwrap();
        assert_eq!(
            configured_public_runtime_endpoint(&root)
                .unwrap()
                .as_deref(),
            Some("https://runtime.example/peer/query")
        );
    }

    #[test]
    fn local_peer_runtime_discovery_mode_prefers_public_url() {
        assert_eq!(
            local_peer_runtime_discovery_mode(
                Some(&format!("http://127.0.0.1:52695{PEER_RUNTIME_QUERY_PATH}")),
                Some("https://runtime.example/peer/query"),
            ),
            prism_coordination::RuntimeDiscoveryMode::PublicUrl
        );
        assert_eq!(
            local_peer_runtime_discovery_mode(
                Some(&format!("http://127.0.0.1:52695{PEER_RUNTIME_QUERY_PATH}")),
                None,
            ),
            prism_coordination::RuntimeDiscoveryMode::LanDirect
        );
        assert_eq!(
            local_peer_runtime_discovery_mode(None, None),
            prism_coordination::RuntimeDiscoveryMode::None
        );
    }
}
