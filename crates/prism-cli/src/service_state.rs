use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use prism_core::PrismPaths;
use serde::{Deserialize, Serialize};

use crate::mcp::{normalize_service_endpoint_uri, probe_service_endpoint};

const SERVICE_STATE_DIR_NAME: &str = "service";
const LOCAL_ENDPOINT_FILE_NAME: &str = "local-endpoint.json";
const ENROLLED_REPOS_FILE_NAME: &str = "enrolled-repos.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceEndpointSource {
    Configured,
    LocalDiscovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedServiceEndpoint {
    pub(crate) endpoint: String,
    pub(crate) source: ServiceEndpointSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepoEnrollmentRecord {
    pub(crate) canonical_root: String,
    pub(crate) enrolled_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceServiceConfigFile {
    service_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalServiceEndpointRecord {
    endpoint: String,
    canonical_root: String,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct RepoEnrollmentRegistry {
    repos: Vec<RepoEnrollmentRecord>,
}

pub(crate) fn sync_local_service_endpoint(root: &Path) -> Result<()> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let uri_path = paths.mcp_http_uri_path()?;
    let uri = read_optional_trimmed_file(&uri_path)?.ok_or_else(|| {
        anyhow!(
            "missing local daemon uri file {}; the service did not publish a local endpoint",
            uri_path.display()
        )
    })?;
    let endpoint = normalize_service_endpoint_uri(&uri)?;
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let record = LocalServiceEndpointRecord {
        endpoint,
        canonical_root: canonical_root.display().to_string(),
        updated_at_ms: current_timestamp_millis(),
    };
    write_json(service_local_endpoint_path(&paths)?, &record)
}

pub(crate) fn clear_local_service_endpoint(root: &Path) -> Result<()> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let path = service_local_endpoint_path(&paths)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub(crate) fn resolve_service_endpoint(root: &Path) -> Result<ResolvedServiceEndpoint> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let configured = load_configured_service_endpoint(paths.service_config_path())?;
    let local = read_local_service_endpoint(service_local_endpoint_path(&paths)?)?;
    resolve_service_endpoint_from_inputs(configured.as_deref(), local.as_ref())
}

pub(crate) fn enroll_current_repo(root: &Path) -> Result<RepoEnrollmentRecord> {
    let resolved = resolve_service_endpoint(root)?;
    if resolved.source != ServiceEndpointSource::LocalDiscovery {
        bail!(
            "temporary `prism service enroll-repo` bootstrap only supports machine-local service discovery"
        );
    }

    let paths = PrismPaths::for_workspace_root(root)?;
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let record = RepoEnrollmentRecord {
        canonical_root: canonical_root.display().to_string(),
        enrolled_at_ms: current_timestamp_millis(),
    };
    let registry_path = service_enrolled_repos_path(&paths)?;
    let mut registry = read_json::<RepoEnrollmentRegistry>(&registry_path)?.unwrap_or_default();
    if let Some(existing) = registry
        .repos
        .iter_mut()
        .find(|repo| repo.canonical_root == record.canonical_root)
    {
        *existing = record.clone();
    } else {
        registry.repos.push(record.clone());
    }
    registry
        .repos
        .sort_by(|left, right| left.canonical_root.cmp(&right.canonical_root));
    write_json(registry_path, &registry)?;
    Ok(record)
}

pub(crate) fn render_endpoint(root: &Path) -> Result<String> {
    let resolved = resolve_service_endpoint(root)?;
    let source = match resolved.source {
        ServiceEndpointSource::Configured => "configured",
        ServiceEndpointSource::LocalDiscovery => "local",
    };
    Ok(format!("endpoint: {}\nsource: {source}", resolved.endpoint))
}

fn resolve_service_endpoint_from_inputs(
    configured: Option<&str>,
    local: Option<&LocalServiceEndpointRecord>,
) -> Result<ResolvedServiceEndpoint> {
    if let Some(configured) = configured {
        let endpoint = normalize_service_endpoint_uri(configured)?;
        probe_service_endpoint(&endpoint).with_context(|| {
            format!("configured PRISM service endpoint `{endpoint}` is unavailable")
        })?;
        return Ok(ResolvedServiceEndpoint {
            endpoint,
            source: ServiceEndpointSource::Configured,
        });
    }

    if let Some(local) = local {
        probe_service_endpoint(&local.endpoint).with_context(|| {
            format!(
                "machine-local PRISM service endpoint `{}` is unavailable; start it with `prism service up`",
                local.endpoint
            )
        })?;
        return Ok(ResolvedServiceEndpoint {
            endpoint: local.endpoint.clone(),
            source: ServiceEndpointSource::LocalDiscovery,
        });
    }

    bail!(
        "no configured PRISM service endpoint and no machine-local service is registered; start it with `prism service up`"
    )
}

fn load_configured_service_endpoint(path: PathBuf) -> Result<Option<String>> {
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("failed to read {}", path.display())),
    };
    let config: WorkspaceServiceConfigFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config.service_endpoint)
}

fn read_local_service_endpoint(path: PathBuf) -> Result<Option<LocalServiceEndpointRecord>> {
    read_json(&path)
}

fn read_optional_trimmed_file(path: &Path) -> Result<Option<String>> {
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("failed to read {}", path.display())),
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn write_json<T: Serialize>(path: PathBuf, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize service state")?;
    fs::write(&path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("failed to read {}", path.display())),
    };
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn service_state_dir(paths: &PrismPaths) -> PathBuf {
    paths.home_root().join(SERVICE_STATE_DIR_NAME)
}

fn service_local_endpoint_path(paths: &PrismPaths) -> Result<PathBuf> {
    let path = service_state_dir(paths).join(LOCAL_ENDPOINT_FILE_NAME);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(path)
}

fn service_enrolled_repos_path(paths: &PrismPaths) -> Result<PathBuf> {
    let path = service_state_dir(paths).join(ENROLLED_REPOS_FILE_NAME);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(path)
}

fn current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        read_json, resolve_service_endpoint_from_inputs, write_json, LocalServiceEndpointRecord,
        RepoEnrollmentRecord, RepoEnrollmentRegistry,
    };

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn configured_endpoint_wins_over_local_state() {
        let local = LocalServiceEndpointRecord {
            endpoint: "http://example.invalid".to_string(),
            canonical_root: "repo-a".to_string(),
            updated_at_ms: 1,
        };

        let resolved = resolve_service_endpoint_from_inputs(
            Some("http://example.invalid"),
            Some(&local),
        );
        assert!(resolved.is_err());
        assert!(
            resolved
                .unwrap_err()
                .to_string()
                .contains("configured PRISM service endpoint")
        );
    }

    #[test]
    fn local_discovery_requires_endpoint_when_unconfigured() {
        let local = LocalServiceEndpointRecord {
            endpoint: "http://example.invalid".to_string(),
            canonical_root: "repo-a".to_string(),
            updated_at_ms: 1,
        };

        let resolved = resolve_service_endpoint_from_inputs(None, Some(&local));
        assert!(resolved.is_err());
        assert!(
            resolved
                .unwrap_err()
                .to_string()
                .contains("machine-local PRISM service endpoint")
        );
    }

    #[test]
    fn registry_round_trips_json() {
        let root = temp_path("service-state-registry");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("registry.json");
        let registry = RepoEnrollmentRegistry {
            repos: vec![RepoEnrollmentRecord {
                canonical_root: "repo-a".to_string(),
                enrolled_at_ms: 1,
            }],
        };
        write_json(path.clone(), &registry).unwrap();
        let reloaded = read_json::<RepoEnrollmentRegistry>(&path)
            .unwrap()
            .expect("registry should reload");
        assert_eq!(reloaded, registry);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn endpoint_record_round_trips_json() {
        let root = temp_path("service-state-endpoint");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("endpoint.json");
        let record = LocalServiceEndpointRecord {
            endpoint: "http://example.invalid".to_string(),
            canonical_root: "repo-a".to_string(),
            updated_at_ms: 1,
        };
        write_json(path.clone(), &record).unwrap();
        let reloaded = read_json::<LocalServiceEndpointRecord>(&path)
            .unwrap()
            .expect("endpoint record should reload");
        assert_eq!(reloaded, record);
        fs::remove_dir_all(root).ok();
    }

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "prism-cli-{label}-{}",
            NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
