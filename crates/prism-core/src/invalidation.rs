use std::collections::HashSet;
use std::path::PathBuf;

use prism_ir::NodeId;
use prism_store::Graph;

#[derive(Debug, Clone, Default)]
pub(crate) struct RefreshInvalidationScope {
    pub(crate) direct_paths: HashSet<PathBuf>,
    pub(crate) dependency_paths: HashSet<PathBuf>,
    pub(crate) edge_resolution_paths: HashSet<PathBuf>,
}

impl RefreshInvalidationScope {
    pub(crate) fn from_graph(graph: &Graph, direct_paths: &HashSet<PathBuf>) -> Self {
        let structural_dependency_paths = expand_dependency_paths(graph, direct_paths);
        let dependency_paths = expand_edge_resolution_paths(graph, &structural_dependency_paths);
        let edge_resolution_paths = dependency_paths.clone();
        Self {
            direct_paths: direct_paths.clone(),
            dependency_paths,
            edge_resolution_paths,
        }
    }
}

fn expand_dependency_paths(graph: &Graph, refresh_scope: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    let mut expanded = refresh_scope.clone();
    for path in refresh_scope {
        let Some(file_state) = graph.file_state(path) else {
            continue;
        };
        for node in &file_state.nodes {
            for edge in graph.edges_from(&node.id, None) {
                if let Some(target) = graph.node(&edge.target) {
                    if let Some(target_path) = graph.file_path(target.file) {
                        expanded.insert(target_path.clone());
                    }
                }
            }
            for edge in graph.edges_to(&node.id, None) {
                if let Some(source) = graph.node(&edge.source) {
                    if let Some(source_path) = graph.file_path(source.file) {
                        expanded.insert(source_path.clone());
                    }
                }
            }
        }
    }
    expanded
}

fn expand_edge_resolution_paths(
    graph: &Graph,
    refresh_scope: &HashSet<PathBuf>,
) -> HashSet<PathBuf> {
    let mut expanded = refresh_scope.clone();
    let mut target_names = HashSet::new();
    let mut target_paths = Vec::new();
    let mut target_node_ids = HashSet::<NodeId>::new();

    for path in refresh_scope {
        let Some(file_state) = graph.file_state(path) else {
            continue;
        };
        for node in &file_state.nodes {
            target_names.insert(node.name.clone());
            target_paths.push(node.id.path.clone());
            target_node_ids.insert(node.id.clone());
        }
    }

    if target_names.is_empty() && target_paths.is_empty() {
        return expanded;
    }

    for (path, record) in graph.file_records() {
        if expanded.contains(path) {
            continue;
        }

        let matches_calls = record.unresolved_calls.iter().any(|call| {
            target_names.contains(call.name.as_str())
                || target_paths
                    .iter()
                    .any(|candidate| candidate == &format!("{}::{}", call.module_path, call.name))
        });
        let matches_imports = record.unresolved_imports.iter().any(|import| {
            let import_name = import
                .path
                .rsplit("::")
                .next()
                .unwrap_or(import.path.as_str());
            target_names.contains(import_name)
                || target_paths.iter().any(|candidate| {
                    candidate == import.path.as_str() || candidate.ends_with(import.path.as_str())
                })
        });
        let matches_impls = record.unresolved_impls.iter().any(|implementation| {
            let target_name = implementation
                .target
                .rsplit("::")
                .next()
                .unwrap_or(implementation.target.as_str());
            target_names.contains(target_name)
                || target_paths.iter().any(|candidate| {
                    candidate == implementation.target.as_str()
                        || candidate.ends_with(implementation.target.as_str())
                })
        });
        let matches_intents = record.unresolved_intents.iter().any(|intent| {
            target_names.contains(intent.target.as_str())
                || target_paths.iter().any(|candidate| {
                    candidate == intent.target.as_str()
                        || candidate.ends_with(intent.target.as_str())
                })
        });
        let matches_existing_edges = graph.file_state(path).is_some_and(|file_state| {
            file_state.nodes.iter().any(|node| {
                graph.edges_from(&node.id, None).iter().any(|edge| {
                    target_node_ids.contains(&edge.source) || target_node_ids.contains(&edge.target)
                }) || graph.edges_to(&node.id, None).iter().any(|edge| {
                    target_node_ids.contains(&edge.source) || target_node_ids.contains(&edge.target)
                })
            })
        });

        if matches_calls
            || matches_imports
            || matches_impls
            || matches_intents
            || matches_existing_edges
        {
            expanded.insert(path.clone());
        }
    }

    expanded
}
