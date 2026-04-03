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
    Ok(Some(format!(
        "{}/{}",
        uri.trim_end_matches('/'),
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
