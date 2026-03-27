use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_store::Graph;
use smol_str::SmolStr;
use toml::Value;

use crate::util::workspace_walk;

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceLayout {
    pub(crate) workspace_name: String,
    pub(crate) workspace_display_name: String,
    pub(crate) workspace_manifest: PathBuf,
    pub(crate) packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct PackageInfo {
    pub(crate) package_name: String,
    pub(crate) crate_name: String,
    pub(crate) root: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) node_id: NodeId,
}

impl WorkspaceLayout {
    pub(crate) fn package_for(&self, path: &Path) -> &PackageInfo {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
            .unwrap_or(&self.packages[0])
    }
}

impl PackageInfo {
    pub(crate) fn new(package_name: String, root: PathBuf, manifest_path: PathBuf) -> Self {
        let crate_name = normalize_identifier(&package_name);
        let node_id = NodeId::new(crate_name.clone(), crate_name.clone(), NodeKind::Package);
        Self {
            package_name,
            crate_name,
            root,
            manifest_path,
            node_id,
        }
    }
}

pub(crate) fn sync_root_nodes(graph: &mut Graph, layout: &WorkspaceLayout) -> NodeId {
    let manifest_file = graph.ensure_file(&layout.workspace_manifest);
    let workspace_id = NodeId::new(
        layout.workspace_name.clone(),
        format!("{}::workspace", layout.workspace_name),
        NodeKind::Workspace,
    );
    let allowed_root_ids = std::iter::once(workspace_id.clone())
        .chain(
            layout
                .packages
                .iter()
                .map(|package| package.node_id.clone()),
        )
        .collect::<std::collections::HashSet<_>>();
    graph.retain_root_nodes(&allowed_root_ids);

    graph.add_node(Node {
        id: workspace_id.clone(),
        name: SmolStr::new(layout.workspace_display_name.clone()),
        kind: NodeKind::Workspace,
        file: manifest_file,
        span: Span::line(1),
        language: Language::Unknown,
    });

    for package in &layout.packages {
        let manifest_file = graph.ensure_file(&package.manifest_path);
        graph.add_node(Node {
            id: package.node_id.clone(),
            name: SmolStr::new(package.package_name.clone()),
            kind: NodeKind::Package,
            file: manifest_file,
            span: Span::line(1),
            language: Language::Unknown,
        });
    }

    graph.clear_root_contains_edges();
    for package in &layout.packages {
        graph.add_edge(Edge {
            kind: EdgeKind::Contains,
            source: workspace_id.clone(),
            target: package.node_id.clone(),
            origin: EdgeOrigin::Static,
            confidence: 1.0,
        });
    }

    workspace_id
}

pub(crate) fn discover_layout(root: &Path) -> Result<WorkspaceLayout> {
    let workspace_display_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace")
        .to_owned();
    let workspace_name = normalize_identifier(&workspace_display_name);
    if root.join("Cargo.toml").exists() {
        return discover_cargo_layout(root, workspace_name, workspace_display_name);
    }

    discover_python_layout(root, workspace_name, workspace_display_name)
}

fn manifest_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let manifest = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let value: Value = toml::from_str(&manifest)
        .with_context(|| format!("failed to parse manifest {}", path.display()))?;
    Ok(value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

fn discover_cargo_layout(
    root: &Path,
    workspace_name: String,
    workspace_display_name: String,
) -> Result<WorkspaceLayout> {
    let workspace_manifest = root.join("Cargo.toml");
    let root_package_name = manifest_package_name(&workspace_manifest)?
        .unwrap_or_else(|| workspace_display_name.clone());
    let mut packages = vec![PackageInfo::new(
        root_package_name,
        root.to_path_buf(),
        workspace_manifest.clone(),
    )];

    for entry in workspace_walk(root).filter_map(Result::ok) {
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
            || entry.file_name() != "Cargo.toml"
        {
            continue;
        }

        let manifest_path = entry.path();
        if manifest_path == workspace_manifest {
            continue;
        }

        let Some(package_name) = manifest_package_name(manifest_path)? else {
            continue;
        };
        let package_root = manifest_path
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.parent().unwrap_or(root).to_path_buf());
        packages.push(PackageInfo::new(
            package_name,
            package_root,
            manifest_path.to_path_buf(),
        ));
    }

    Ok(WorkspaceLayout {
        workspace_name,
        workspace_display_name,
        workspace_manifest,
        packages,
    })
}

fn discover_python_layout(
    root: &Path,
    workspace_name: String,
    workspace_display_name: String,
) -> Result<WorkspaceLayout> {
    let workspace_manifest = root_manifest_path(root);
    let root_package_name = package_name_for_manifest(&workspace_manifest)?
        .unwrap_or_else(|| workspace_display_name.clone());
    let mut packages = vec![PackageInfo::new(
        root_package_name,
        root.to_path_buf(),
        workspace_manifest.clone(),
    )];
    let mut seen_roots = std::collections::HashSet::from([root.to_path_buf()]);

    for entry in workspace_walk(root).filter_map(Result::ok) {
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let manifest_path = entry.path();
        if !is_python_manifest(manifest_path) || manifest_path == workspace_manifest {
            continue;
        }

        let package_root = manifest_path
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .unwrap_or_else(|_| manifest_path.parent().unwrap_or(root).to_path_buf());
        if !seen_roots.insert(package_root.clone()) {
            continue;
        }

        let package_name = package_name_for_manifest(manifest_path)?.unwrap_or_else(|| {
            package_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("workspace")
                .to_owned()
        });
        packages.push(PackageInfo::new(
            package_name,
            package_root,
            manifest_path.to_path_buf(),
        ));
    }

    Ok(WorkspaceLayout {
        workspace_name,
        workspace_display_name,
        workspace_manifest,
        packages,
    })
}

fn root_manifest_path(root: &Path) -> PathBuf {
    [
        root.join("pyproject.toml"),
        root.join("setup.py"),
        root.join("setup.cfg"),
    ]
    .into_iter()
    .find(|path| path.exists())
    .unwrap_or_else(|| root.join("pyproject.toml"))
}

fn is_python_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("pyproject.toml" | "setup.py" | "setup.cfg")
    )
}

fn package_name_for_manifest(path: &Path) -> Result<Option<String>> {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("Cargo.toml") => manifest_package_name(path),
        Some("pyproject.toml") => pyproject_package_name(path),
        Some("setup.cfg") => setup_cfg_package_name(path),
        Some("setup.py") => setup_py_package_name(path),
        _ => Ok(None),
    }
}

fn pyproject_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let manifest = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let value: Value = toml::from_str(&manifest)
        .with_context(|| format!("failed to parse manifest {}", path.display()))?;
    Ok(value
        .get("project")
        .and_then(|project| project.get("name"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("tool")
                .and_then(|tool| tool.get("poetry"))
                .and_then(|poetry| poetry.get("name"))
                .and_then(Value::as_str)
        })
        .map(ToOwned::to_owned))
}

fn setup_cfg_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let config = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let mut in_metadata = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_metadata = trimmed.eq_ignore_ascii_case("[metadata]");
            continue;
        }
        if !in_metadata {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            if key.trim().eq_ignore_ascii_case("name") {
                let name = value.trim();
                if !name.is_empty() {
                    return Ok(Some(name.to_owned()));
                }
            }
        }
    }
    Ok(None)
}

fn setup_py_package_name(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let setup = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    Ok(extract_setup_py_keyword(&setup, "name"))
}

fn extract_setup_py_keyword(source: &str, key: &str) -> Option<String> {
    let patterns = [format!("{key}="), format!("{key} =")];
    for pattern in patterns {
        let Some(index) = source.find(&pattern) else {
            continue;
        };
        let remainder = source[index + pattern.len()..].trim_start();
        let quote = remainder.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let end = remainder[1..].find(quote)?;
        return Some(remainder[1..1 + end].to_owned());
    }
    None
}

fn normalize_identifier(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_underscore = false;

    for ch in value.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_underscore = false;
        } else if !previous_underscore {
            normalized.push('_');
            previous_underscore = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        "workspace".to_owned()
    } else {
        normalized
    }
}
