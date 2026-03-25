use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use prism_ir::{Edge, EdgeIndex, EdgeKind, FileId, Node, NodeId};
use prism_parser::{UnresolvedCall, UnresolvedImpl, UnresolvedImport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: HashMap<NodeId, Node>,
    pub edges: Vec<Edge>,
    pub adjacency: HashMap<NodeId, Vec<EdgeIndex>>,
    pub reverse_adjacency: HashMap<NodeId, Vec<EdgeIndex>>,
    file_records: HashMap<PathBuf, FileRecord>,
    file_paths: HashMap<FileId, PathBuf>,
    path_to_file: HashMap<PathBuf, FileId>,
    next_file_id: u32,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_file(&mut self, path: &Path) -> FileId {
        let path = path.to_path_buf();
        if let Some(existing) = self.path_to_file.get(&path) {
            return *existing;
        }

        let id = FileId(self.next_file_id);
        self.next_file_id += 1;
        self.file_paths.insert(id, path.clone());
        self.path_to_file.insert(path, id);
        id
    }

    pub fn file_path(&self, file_id: FileId) -> Option<&PathBuf> {
        self.file_paths.get(&file_id)
    }

    pub fn file_record(&self, path: &Path) -> Option<&FileRecord> {
        self.file_records.get(path)
    }

    pub fn upsert_file(
        &mut self,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
    ) -> FileId {
        let file_id = self.ensure_file(path);
        self.remove_file_nodes(path);

        let node_ids: Vec<NodeId> = nodes.iter().map(|node| node.id.clone()).collect();
        for node in nodes {
            self.nodes.insert(node.id.clone(), node);
        }
        self.edges.extend(edges);
        self.file_records.insert(
            path.to_path_buf(),
            FileRecord {
                file_id,
                hash,
                nodes: node_ids,
                unresolved_calls,
                unresolved_imports,
                unresolved_impls,
            },
        );
        self.rebuild_adjacency();
        file_id
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
        self.rebuild_adjacency();
    }

    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn nodes_by_name(&self, name: &str) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|node| node.name == name)
            .collect()
    }

    pub fn edges_from(&self, id: &NodeId, kind: Option<EdgeKind>) -> Vec<&Edge> {
        self.adjacency
            .get(id)
            .into_iter()
            .flat_map(|indexes| indexes.iter())
            .filter_map(|index| self.edges.get(*index))
            .filter(|edge| kind.map_or(true, |expected| edge.kind == expected))
            .collect()
    }

    pub fn edges_to(&self, id: &NodeId, kind: Option<EdgeKind>) -> Vec<&Edge> {
        self.reverse_adjacency
            .get(id)
            .into_iter()
            .flat_map(|indexes| indexes.iter())
            .filter_map(|index| self.edges.get(*index))
            .filter(|edge| kind.map_or(true, |expected| edge.kind == expected))
            .collect()
    }

    pub fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn tracked_files(&self) -> Vec<PathBuf> {
        self.file_records.keys().cloned().collect()
    }

    pub fn remove_file(&mut self, path: &Path) {
        self.remove_file_nodes(path);
        if let Some(file_id) = self.path_to_file.remove(path) {
            self.file_paths.remove(&file_id);
        }
        self.rebuild_adjacency();
    }

    pub fn clear_edges_by_kind(&mut self, kinds: &[EdgeKind]) {
        self.edges.retain(|edge| !kinds.contains(&edge.kind));
        self.rebuild_adjacency();
    }

    pub fn unresolved_calls(&self) -> Vec<UnresolvedCall> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_calls.clone())
            .collect()
    }

    pub fn unresolved_imports(&self) -> Vec<UnresolvedImport> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_imports.clone())
            .collect()
    }

    pub fn unresolved_impls(&self) -> Vec<UnresolvedImpl> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_impls.clone())
            .collect()
    }

    fn remove_file_nodes(&mut self, path: &Path) {
        let Some(record) = self.file_records.remove(path) else {
            return;
        };

        let removed: HashSet<NodeId> = record.nodes.into_iter().collect();
        self.nodes.retain(|id, _| !removed.contains(id));
        self.edges
            .retain(|edge| !removed.contains(&edge.source) && !removed.contains(&edge.target));
    }

    fn rebuild_adjacency(&mut self) {
        self.adjacency.clear();
        self.reverse_adjacency.clear();

        for (index, edge) in self.edges.iter().enumerate() {
            self.adjacency
                .entry(edge.source.clone())
                .or_default()
                .push(index);
            self.reverse_adjacency
                .entry(edge.target.clone())
                .or_default()
                .push(index);
        }
    }
}
