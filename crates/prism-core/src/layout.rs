use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_store::Graph;
use smol_str::SmolStr;
use toml::Value;
use walkdir::WalkDir;

use crate::util::should_walk;

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
    let workspace_manifest = root.join("Cargo.toml");
    let root_package_name = manifest_package_name(&workspace_manifest)?
        .unwrap_or_else(|| workspace_display_name.clone());
    let mut packages = vec![PackageInfo::new(
        root_package_name,
        root.to_path_buf(),
        workspace_manifest.clone(),
    )];

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| should_walk(entry.path(), root))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
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
