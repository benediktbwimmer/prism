use std::collections::HashSet;
use std::path::PathBuf;

use prism_ir::{NodeId, ObservedChangeSet};
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

pub(crate) fn observed_changes_require_dependent_edge_resolution(
    observed_changes: &[ObservedChangeSet],
) -> bool {
    observed_changes.iter().any(|observed| {
        !observed.added.is_empty()
            || !observed.removed.is_empty()
            || observed.updated.iter().any(|(before, after)| {
                before.node.id != after.node.id
                    || before.node.name != after.node.name
                    || before.node.kind != after.node.kind
            })
    })
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

    for name in &target_names {
        expanded.extend(graph.files_with_unresolved_call_name(name));
        expanded.extend(graph.files_with_unresolved_import_name(name));
        expanded.extend(graph.files_with_unresolved_impl_name(name));
        expanded.extend(graph.files_with_unresolved_intent_target(name));
    }

    for candidate in &target_paths {
        expanded.extend(graph.files_with_unresolved_call_target_path(candidate));
        for suffix in namespace_suffixes(candidate) {
            expanded.extend(graph.files_with_unresolved_import_path(&suffix));
            expanded.extend(graph.files_with_unresolved_impl_target(&suffix));
            expanded.extend(graph.files_with_unresolved_intent_target(&suffix));
        }
    }

    for node_id in &target_node_ids {
        for edge in graph.edges_from(node_id, None) {
            if let Some(target) = graph.node(&edge.target) {
                if let Some(path) = graph.file_path(target.file) {
                    expanded.insert(path.clone());
                }
            }
        }
        for edge in graph.edges_to(node_id, None) {
            if let Some(source) = graph.node(&edge.source) {
                if let Some(path) = graph.file_path(source.file) {
                    expanded.insert(path.clone());
                }
            }
        }
    }

    expanded
}

fn namespace_suffixes(path: &str) -> Vec<String> {
    let parts = path.split("::").collect::<Vec<_>>();
    (0..parts.len())
        .map(|index| parts[index..].join("::"))
        .collect()
}
