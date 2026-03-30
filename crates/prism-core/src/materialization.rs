use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use prism_store::{Graph, WorkspaceTreeSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBoundaryRegion {
    pub id: String,
    pub path: PathBuf,
    pub provenance: String,
    pub materialization_state: String,
    pub scope_state: String,
    pub known_file_count: usize,
    pub materialized_file_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMaterializationSummary {
    pub known_files: usize,
    pub known_directories: usize,
    pub materialized_files: usize,
    pub materialized_nodes: usize,
    pub materialized_edges: usize,
    pub boundaries: Vec<WorkspaceBoundaryRegion>,
}

impl WorkspaceMaterializationSummary {
    pub fn depth(&self) -> &'static str {
        if self.materialized_files == 0 {
            "shallow"
        } else {
            "medium"
        }
    }
}

pub(crate) fn summarize_workspace_materialization(
    root: &Path,
    snapshot: &WorkspaceTreeSnapshot,
    graph: &Graph,
) -> WorkspaceMaterializationSummary {
    let boundaries = summarize_boundary_regions(root, snapshot, graph);
    WorkspaceMaterializationSummary {
        known_files: snapshot.files.len(),
        known_directories: snapshot.directories.len(),
        materialized_files: graph.file_count(),
        materialized_nodes: graph.node_count(),
        materialized_edges: graph.edge_count(),
        boundaries,
    }
}

fn summarize_boundary_regions(
    root: &Path,
    snapshot: &WorkspaceTreeSnapshot,
    graph: &Graph,
) -> Vec<WorkspaceBoundaryRegion> {
    let mut regions = BTreeMap::<PathBuf, (usize, usize)>::new();
    for path in snapshot.files.keys() {
        let region = boundary_region_path(root, path);
        let counts = regions.entry(region).or_insert((0, 0));
        counts.0 += 1;
        if graph.file_record(path).is_some() {
            counts.1 += 1;
        }
    }

    regions
        .into_iter()
        .filter(|(_, (known, materialized))| materialized < known)
        .map(
            |(path, (known_file_count, materialized_file_count))| WorkspaceBoundaryRegion {
                id: format!("boundary:{}", path.display()),
                path,
                provenance: "workspace_tree".to_string(),
                materialization_state: boundary_materialization_state(
                    known_file_count,
                    materialized_file_count,
                )
                .to_string(),
                scope_state: "in_scope".to_string(),
                known_file_count,
                materialized_file_count,
            },
        )
        .collect()
}

fn boundary_region_path(root: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut components = relative.components();
    let Some(first) = components.next() else {
        return relative.to_path_buf();
    };
    if components.next().is_some() {
        PathBuf::from(first.as_os_str())
    } else {
        relative.to_path_buf()
    }
}

fn boundary_materialization_state(
    known_file_count: usize,
    materialized_file_count: usize,
) -> &'static str {
    if materialized_file_count == 0 && known_file_count > 0 {
        "known_unmaterialized"
    } else {
        "sparse"
    }
}
