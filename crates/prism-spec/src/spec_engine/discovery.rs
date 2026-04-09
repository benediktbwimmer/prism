use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use walkdir::WalkDir;

use super::types::{DiscoveredSpecSource, SpecRootResolution, SpecRootSource};

const DEFAULT_SPEC_ROOT: &str = ".prism/specs";
const SPEC_ENGINE_CONFIG_PATH: &str = ".prism/spec-engine.json";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoSpecEngineConfig {
    root: String,
}

pub fn resolve_spec_root(root: &Path) -> Result<SpecRootResolution> {
    let repo_root = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());
    let config_path = repo_root.join(SPEC_ENGINE_CONFIG_PATH);
    if !config_path.exists() {
        let configured_root = PathBuf::from(DEFAULT_SPEC_ROOT);
        return Ok(SpecRootResolution {
            absolute_root: repo_root.join(&configured_root),
            configured_root,
            config_path: None,
            source: SpecRootSource::Default,
        });
    }

    let config_bytes = fs::read(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: RepoSpecEngineConfig = serde_json::from_slice(&config_bytes)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    let configured_root =
        normalize_repo_relative_directory(Path::new(config.root.trim())).with_context(|| {
            format!(
                "invalid spec root in {}",
                Path::new(SPEC_ENGINE_CONFIG_PATH).display()
            )
        })?;

    Ok(SpecRootResolution {
        absolute_root: repo_root.join(&configured_root),
        configured_root,
        config_path: Some(PathBuf::from(SPEC_ENGINE_CONFIG_PATH)),
        source: SpecRootSource::RepoConfig,
    })
}

pub fn discover_spec_sources(root: &Path) -> Result<Vec<DiscoveredSpecSource>> {
    let repo_root = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());
    let resolution = resolve_spec_root(root)?;
    if !resolution.absolute_root.exists() {
        return Ok(Vec::new());
    }
    if !resolution.absolute_root.is_dir() {
        return Err(anyhow!(
            "configured spec root `{}` is not a directory",
            resolution.configured_root.display()
        ));
    }

    let mut discovered = Vec::new();
    for entry in WalkDir::new(&resolution.absolute_root)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry.with_context(|| {
            format!(
                "failed while walking configured spec root {}",
                resolution.absolute_root.display()
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let absolute_path = entry.into_path();
        let repo_relative_path = absolute_path
            .strip_prefix(&repo_root)
            .map(PathBuf::from)
            .with_context(|| {
                format!(
                    "discovered spec path {} must stay inside repo {}",
                    absolute_path.display(),
                    repo_root.display()
                )
            })?;
        discovered.push(DiscoveredSpecSource {
            repo_relative_path,
            absolute_path,
        });
    }
    discovered.sort_by(|left, right| left.repo_relative_path.cmp(&right.repo_relative_path));
    Ok(discovered)
}

fn normalize_repo_relative_directory(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(anyhow!("spec root must not be empty"));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(anyhow!("spec root override cannot escape the repo root"));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("spec root override must be repo-relative"));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(anyhow!("spec root override must name a directory inside the repo"));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{discover_spec_sources, resolve_spec_root, SpecRootSource};

    static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);

    fn temp_repo(label: &str) -> PathBuf {
        let nonce = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-spec-engine-{label}-{unique}-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        root
    }

    fn write_spec_config(root: &Path, body: &str) {
        let config_path = root.join(".prism").join("spec-engine.json");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(config_path, body).unwrap();
    }

    #[test]
    fn resolve_spec_root_uses_default_when_config_is_missing() {
        let root = temp_repo("default-root");
        let canonical_root = root.canonicalize().unwrap();

        let resolution = resolve_spec_root(&root).unwrap();
        assert_eq!(resolution.source, SpecRootSource::Default);
        assert_eq!(resolution.config_path, None);
        assert_eq!(resolution.configured_root, PathBuf::from(".prism/specs"));
        assert_eq!(resolution.absolute_root, canonical_root.join(".prism/specs"));
    }

    #[test]
    fn resolve_spec_root_reads_repo_override_config() {
        let root = temp_repo("override-root");
        let canonical_root = root.canonicalize().unwrap();
        write_spec_config(&root, "{\n  \"root\": \"docs/specs\"\n}\n");

        let resolution = resolve_spec_root(&root).unwrap();
        assert_eq!(resolution.source, SpecRootSource::RepoConfig);
        assert_eq!(
            resolution.config_path,
            Some(PathBuf::from(".prism/spec-engine.json"))
        );
        assert_eq!(resolution.configured_root, PathBuf::from("docs/specs"));
        assert_eq!(resolution.absolute_root, canonical_root.join("docs/specs"));
    }

    #[test]
    fn resolve_spec_root_rejects_repo_escape() {
        let root = temp_repo("escape-root");
        write_spec_config(&root, "{\n  \"root\": \"../outside\"\n}\n");

        let error = resolve_spec_root(&root).unwrap_err().to_string();
        assert!(error.contains("invalid spec root"));
    }

    #[test]
    fn resolve_spec_root_rejects_absolute_paths() {
        let root = temp_repo("absolute-root");
        write_spec_config(&root, "{\n  \"root\": \"/tmp/specs\"\n}\n");

        let error = resolve_spec_root(&root).unwrap_err().to_string();
        assert!(error.contains("invalid spec root"));
    }

    #[test]
    fn discover_spec_sources_returns_sorted_markdown_files_only() {
        let root = temp_repo("discovery-root");
        write_spec_config(&root, "{\n  \"root\": \"docs/specs\"\n}\n");
        fs::create_dir_all(root.join("docs/specs/nested")).unwrap();
        fs::write(root.join("docs/specs/b.md"), "# b\n").unwrap();
        fs::write(root.join("docs/specs/a.txt"), "ignore\n").unwrap();
        fs::write(root.join("docs/specs/nested/a.md"), "# a\n").unwrap();

        let discovered = discover_spec_sources(&root).unwrap();
        assert_eq!(
            discovered
                .into_iter()
                .map(|entry| entry.repo_relative_path)
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("docs/specs/b.md"),
                PathBuf::from("docs/specs/nested/a.md"),
            ]
        );
    }

    #[test]
    fn discover_spec_sources_returns_empty_when_root_is_missing() {
        let root = temp_repo("missing-root");

        let discovered = discover_spec_sources(&root).unwrap();
        assert!(discovered.is_empty());
    }
}
