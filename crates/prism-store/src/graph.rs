use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use prism_ir::{
    new_prefixed_id, ChangeTrigger, Edge, EdgeIndex, EdgeKind, EdgeOrigin, EventActor, EventId,
    EventMeta, FileId, GraphChange, Node, NodeId, NodeKind, ObservedChangeSet, ObservedNode,
};
use prism_parser::{
    NodeFingerprint, UnresolvedCall, UnresolvedImpl, UnresolvedImport, UnresolvedIntent,
};
use serde::{Deserialize, Serialize};

static NEXT_OBSERVED_EVENT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
    pub edges: Vec<Edge>,
    pub fingerprints: HashMap<NodeId, NodeFingerprint>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
    pub unresolved_intents: Vec<UnresolvedIntent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub nodes: HashMap<NodeId, Node>,
    pub edges: Vec<Edge>,
    pub file_records: HashMap<PathBuf, FileRecord>,
    pub next_file_id: u32,
}

#[derive(Debug, Clone)]
pub struct FileState {
    pub path: PathBuf,
    pub record: FileRecord,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileUpdate {
    pub file_id: FileId,
    pub observed: ObservedChangeSet,
    pub changes: Vec<GraphChange>,
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
    #[serde(skip)]
    node_name_index: HashMap<String, Vec<NodeId>>,
    #[serde(skip)]
    node_path_index: HashMap<String, Vec<NodeId>>,
    next_file_id: u32,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> GraphSnapshot {
        GraphSnapshot {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            file_records: self.file_records.clone(),
            next_file_id: self.next_file_id,
        }
    }

    pub fn from_snapshot(snapshot: GraphSnapshot) -> Self {
        let mut graph = Self {
            nodes: snapshot.nodes,
            edges: snapshot.edges,
            adjacency: HashMap::new(),
            reverse_adjacency: HashMap::new(),
            file_records: snapshot.file_records,
            file_paths: HashMap::new(),
            path_to_file: HashMap::new(),
            node_name_index: HashMap::new(),
            node_path_index: HashMap::new(),
            next_file_id: snapshot.next_file_id,
        };

        for (path, record) in &graph.file_records {
            graph.file_paths.insert(record.file_id, path.clone());
            graph.path_to_file.insert(path.clone(), record.file_id);
        }
        graph.rebuild_adjacency();
        graph
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

    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        self.path_to_file.get(path).copied()
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
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
    ) -> FileId {
        self.upsert_file_from(
            None,
            path,
            hash,
            nodes,
            edges,
            fingerprints,
            unresolved_calls,
            unresolved_imports,
            unresolved_impls,
            unresolved_intents,
            &[],
        )
        .file_id
    }

    pub fn upsert_file_with_reanchors(
        &mut self,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
        reanchors: &[(NodeId, NodeId)],
    ) -> FileUpdate {
        self.upsert_file_from_with_observed(
            None,
            path,
            hash,
            nodes,
            edges,
            fingerprints,
            unresolved_calls,
            unresolved_imports,
            unresolved_impls,
            unresolved_intents,
            reanchors,
            default_event_meta(),
            ChangeTrigger::ManualReindex,
        )
    }

    pub fn upsert_file_from(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
        reanchors: &[(NodeId, NodeId)],
    ) -> FileUpdate {
        self.upsert_file_from_with_observed(
            previous_path,
            path,
            hash,
            nodes,
            edges,
            fingerprints,
            unresolved_calls,
            unresolved_imports,
            unresolved_impls,
            unresolved_intents,
            reanchors,
            default_event_meta(),
            ChangeTrigger::ManualReindex,
        )
    }

    pub fn upsert_file_from_with_observed(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
        reanchors: &[(NodeId, NodeId)],
        meta: EventMeta,
        trigger: ChangeTrigger,
    ) -> FileUpdate {
        self.upsert_file_from_with_observed_internal(
            previous_path,
            path,
            hash,
            nodes,
            edges,
            fingerprints,
            unresolved_calls,
            unresolved_imports,
            unresolved_impls,
            unresolved_intents,
            reanchors,
            meta,
            trigger,
            true,
        )
    }

    pub fn upsert_file_from_with_observed_without_rebuild(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
        reanchors: &[(NodeId, NodeId)],
        meta: EventMeta,
        trigger: ChangeTrigger,
    ) -> FileUpdate {
        self.upsert_file_from_with_observed_internal(
            previous_path,
            path,
            hash,
            nodes,
            edges,
            fingerprints,
            unresolved_calls,
            unresolved_imports,
            unresolved_impls,
            unresolved_intents,
            reanchors,
            meta,
            trigger,
            false,
        )
    }

    fn upsert_file_from_with_observed_internal(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        fingerprints: HashMap<NodeId, NodeFingerprint>,
        unresolved_calls: Vec<UnresolvedCall>,
        unresolved_imports: Vec<UnresolvedImport>,
        unresolved_impls: Vec<UnresolvedImpl>,
        unresolved_intents: Vec<UnresolvedIntent>,
        reanchors: &[(NodeId, NodeId)],
        meta: EventMeta,
        trigger: ChangeTrigger,
        rebuild_indexes: bool,
    ) -> FileUpdate {
        let baseline_path = previous_path.unwrap_or(path);
        let previous = self.file_records.get(baseline_path).cloned();
        let previous_state = self.file_state(baseline_path);
        let file_id = previous
            .as_ref()
            .map(|record| record.file_id)
            .unwrap_or_else(|| self.ensure_file(path));
        let observed = self.compute_observed_changes(
            previous_state.as_ref(),
            file_id,
            previous_path.or(Some(path)),
            Some(path),
            &nodes,
            &edges,
            &fingerprints,
            meta,
            trigger,
        );
        let changes = self.compute_file_changes(previous.as_ref(), &nodes, reanchors);
        self.remove_file_nodes(baseline_path);

        if baseline_path != path {
            if let Some(previous_file_id) = self.path_to_file.remove(baseline_path) {
                self.file_paths.remove(&previous_file_id);
            }
            self.file_paths.insert(file_id, path.to_path_buf());
            self.path_to_file.insert(path.to_path_buf(), file_id);
        }

        let node_ids: Vec<NodeId> = nodes.iter().map(|node| node.id.clone()).collect();
        let record_edges = edges.clone();
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
                edges: record_edges,
                fingerprints,
                unresolved_calls,
                unresolved_imports,
                unresolved_impls,
                unresolved_intents,
            },
        );
        if rebuild_indexes {
            self.rebuild_adjacency();
        }
        FileUpdate {
            file_id,
            observed,
            changes,
        }
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.extend_edges(std::iter::once(edge));
    }

    pub fn extend_edges<I>(&mut self, edges: I) -> usize
    where
        I: IntoIterator<Item = Edge>,
    {
        let mut appended = 0usize;
        for edge in edges {
            self.edges.push(edge);
            appended += 1;
        }
        if appended > 0 {
            self.rebuild_adjacency();
        }
        appended
    }

    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn nodes_by_name(&self, name: &str) -> Vec<&Node> {
        self.node_name_index
            .get(name)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    pub fn nodes_by_exact_path(&self, path: &str) -> Vec<&Node> {
        self.node_path_index
            .get(path)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.nodes.get(id))
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

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn file_count(&self) -> usize {
        self.file_records.len()
    }

    pub fn tracked_files(&self) -> Vec<PathBuf> {
        self.file_records.keys().cloned().collect()
    }

    pub fn file_records(&self) -> impl Iterator<Item = (&PathBuf, &FileRecord)> {
        self.file_records.iter()
    }

    pub fn next_file_id(&self) -> u32 {
        self.next_file_id
    }

    pub fn file_state(&self, path: &Path) -> Option<FileState> {
        let record = self.file_records.get(path)?.clone();
        let nodes = record
            .nodes
            .iter()
            .filter_map(|id| self.nodes.get(id).cloned())
            .collect::<Vec<_>>();
        let edges = record.edges.clone();

        Some(FileState {
            path: path.to_path_buf(),
            record,
            nodes,
            edges,
        })
    }

    pub fn root_nodes(&self) -> Vec<Node> {
        self.nodes
            .values()
            .filter(|node| matches!(node.kind, NodeKind::Workspace | NodeKind::Package))
            .cloned()
            .collect()
    }

    pub fn root_edges(&self) -> Vec<Edge> {
        self.edges
            .iter()
            .filter(|edge| {
                edge.kind == EdgeKind::Contains
                    && edge.source.kind == NodeKind::Workspace
                    && edge.target.kind == NodeKind::Package
            })
            .cloned()
            .collect()
    }

    pub fn retain_root_nodes(&mut self, allowed: &HashSet<NodeId>) {
        self.nodes.retain(|id, node| {
            !matches!(node.kind, NodeKind::Workspace | NodeKind::Package) || allowed.contains(id)
        });
        self.edges.retain(|edge| {
            self.nodes.contains_key(&edge.source) && self.nodes.contains_key(&edge.target)
        });
        self.rebuild_adjacency();
    }

    pub fn clear_root_contains_edges(&mut self) {
        self.edges.retain(|edge| {
            !(edge.kind == EdgeKind::Contains
                && edge.source.kind == NodeKind::Workspace
                && edge.target.kind == NodeKind::Package)
        });
        self.rebuild_adjacency();
    }

    pub fn derived_edges(&self) -> Vec<Edge> {
        self.edges
            .iter()
            .filter(|edge| is_derived_kind(edge.kind))
            .cloned()
            .collect()
    }

    pub fn remove_file(&mut self, path: &Path) {
        self.remove_file_with_changes(path);
    }

    pub fn remove_file_with_changes(&mut self, path: &Path) -> Vec<GraphChange> {
        self.remove_file_with_update(path).changes
    }

    pub fn remove_file_with_update(&mut self, path: &Path) -> FileUpdate {
        self.remove_file_with_observed(path, default_event_meta(), ChangeTrigger::ManualReindex)
    }

    pub fn remove_file_with_observed(
        &mut self,
        path: &Path,
        meta: EventMeta,
        trigger: ChangeTrigger,
    ) -> FileUpdate {
        self.remove_file_with_observed_internal(path, meta, trigger, true)
    }

    pub fn remove_file_with_observed_without_rebuild(
        &mut self,
        path: &Path,
        meta: EventMeta,
        trigger: ChangeTrigger,
    ) -> FileUpdate {
        self.remove_file_with_observed_internal(path, meta, trigger, false)
    }

    fn remove_file_with_observed_internal(
        &mut self,
        path: &Path,
        meta: EventMeta,
        trigger: ChangeTrigger,
        rebuild_indexes: bool,
    ) -> FileUpdate {
        let previous_state = self.file_state(path);
        let changes = self
            .file_records
            .get(path)
            .map(|record| {
                record
                    .nodes
                    .iter()
                    .cloned()
                    .map(GraphChange::Removed)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let file_id = previous_state
            .as_ref()
            .map(|state| state.record.file_id)
            .unwrap_or(FileId(0));
        let observed = self.compute_observed_changes(
            previous_state.as_ref(),
            file_id,
            Some(path),
            None,
            &[],
            &[],
            &HashMap::new(),
            meta,
            trigger,
        );
        self.remove_file_nodes(path);
        if let Some(file_id) = self.path_to_file.remove(path) {
            self.file_paths.remove(&file_id);
        }
        if rebuild_indexes {
            self.rebuild_adjacency();
        }
        FileUpdate {
            file_id,
            observed,
            changes,
        }
    }

    pub fn rebuild_indexes(&mut self) {
        self.rebuild_adjacency();
    }

    pub fn clear_edges_by_kind(&mut self, kinds: &[EdgeKind]) -> usize {
        let before = self.edges.len();
        self.edges.retain(|edge| !kinds.contains(&edge.kind));
        let removed = before.saturating_sub(self.edges.len());
        if removed > 0 {
            self.rebuild_adjacency();
        }
        removed
    }

    pub fn clear_derived_edges_for_nodes(&mut self, node_ids: &HashSet<NodeId>) -> usize {
        if node_ids.is_empty() {
            return 0;
        }
        let before = self.edges.len();
        self.edges.retain(|edge| {
            !is_derived_kind(edge.kind)
                || (!node_ids.contains(&edge.source) && !node_ids.contains(&edge.target))
        });
        let removed = before.saturating_sub(self.edges.len());
        if removed > 0 {
            self.rebuild_adjacency();
        }
        removed
    }

    pub fn unresolved_calls(&self) -> Vec<UnresolvedCall> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_calls.clone())
            .collect()
    }

    pub fn unresolved_calls_for_paths(&self, paths: &HashSet<PathBuf>) -> Vec<UnresolvedCall> {
        self.file_records
            .iter()
            .filter(|(path, _)| paths.contains(*path))
            .flat_map(|(_, record)| record.unresolved_calls.clone())
            .collect()
    }

    pub fn unresolved_imports(&self) -> Vec<UnresolvedImport> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_imports.clone())
            .collect()
    }

    pub fn unresolved_imports_for_paths(&self, paths: &HashSet<PathBuf>) -> Vec<UnresolvedImport> {
        self.file_records
            .iter()
            .filter(|(path, _)| paths.contains(*path))
            .flat_map(|(_, record)| record.unresolved_imports.clone())
            .collect()
    }

    pub fn unresolved_impls(&self) -> Vec<UnresolvedImpl> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_impls.clone())
            .collect()
    }

    pub fn unresolved_impls_for_paths(&self, paths: &HashSet<PathBuf>) -> Vec<UnresolvedImpl> {
        self.file_records
            .iter()
            .filter(|(path, _)| paths.contains(*path))
            .flat_map(|(_, record)| record.unresolved_impls.clone())
            .collect()
    }

    pub fn unresolved_intents(&self) -> Vec<UnresolvedIntent> {
        self.file_records
            .values()
            .flat_map(|record| record.unresolved_intents.clone())
            .collect()
    }

    pub fn unresolved_intents_for_paths(&self, paths: &HashSet<PathBuf>) -> Vec<UnresolvedIntent> {
        self.file_records
            .iter()
            .filter(|(path, _)| paths.contains(*path))
            .flat_map(|(_, record)| record.unresolved_intents.clone())
            .collect()
    }

    pub fn node_ids_for_paths(&self, paths: &HashSet<PathBuf>) -> HashSet<NodeId> {
        paths
            .iter()
            .filter_map(|path| self.file_records.get(path))
            .flat_map(|record| record.nodes.iter().cloned())
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
        self.rebuild_node_indexes();

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

    fn rebuild_node_indexes(&mut self) {
        self.node_name_index.clear();
        self.node_path_index.clear();

        for node in self.nodes.values() {
            self.node_name_index
                .entry(node.name.to_string())
                .or_default()
                .push(node.id.clone());
            self.node_path_index
                .entry(node.id.path.to_string())
                .or_default()
                .push(node.id.clone());
        }
    }

    fn compute_file_changes(
        &self,
        previous: Option<&FileRecord>,
        nodes: &[Node],
        reanchors: &[(NodeId, NodeId)],
    ) -> Vec<GraphChange> {
        let old_nodes = previous
            .map(|record| record.nodes.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();
        let new_nodes = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();
        let valid_reanchors = reanchors
            .iter()
            .filter(|(old, new)| old_nodes.contains(old) && new_nodes.contains(new))
            .cloned()
            .collect::<Vec<_>>();
        let reanchored_old = valid_reanchors
            .iter()
            .map(|(old, _)| old.clone())
            .collect::<HashSet<_>>();
        let reanchored_new = valid_reanchors
            .iter()
            .map(|(_, new)| new.clone())
            .collect::<HashSet<_>>();

        let mut changes = Vec::new();

        for id in old_nodes
            .intersection(&new_nodes)
            .filter(|id| !reanchored_old.contains(*id) && !reanchored_new.contains(*id))
        {
            changes.push(GraphChange::Modified((*id).clone()));
        }

        for (old, new) in valid_reanchors {
            changes.push(GraphChange::Reanchored { old, new });
        }

        for id in old_nodes
            .difference(&new_nodes)
            .filter(|id| !reanchored_old.contains(*id))
        {
            changes.push(GraphChange::Removed((*id).clone()));
        }

        for id in new_nodes
            .difference(&old_nodes)
            .filter(|id| !reanchored_new.contains(*id))
        {
            changes.push(GraphChange::Added((*id).clone()));
        }

        changes
    }

    fn compute_observed_changes(
        &self,
        previous: Option<&FileState>,
        file_id: FileId,
        previous_path: Option<&Path>,
        current_path: Option<&Path>,
        nodes: &[Node],
        edges: &[Edge],
        fingerprints: &HashMap<NodeId, NodeFingerprint>,
        meta: EventMeta,
        trigger: ChangeTrigger,
    ) -> ObservedChangeSet {
        let empty_record = FileRecord {
            file_id,
            hash: 0,
            nodes: Vec::new(),
            edges: Vec::new(),
            fingerprints: HashMap::new(),
            unresolved_calls: Vec::new(),
            unresolved_imports: Vec::new(),
            unresolved_impls: Vec::new(),
            unresolved_intents: Vec::new(),
        };
        let previous_record = previous.map(|state| &state.record).unwrap_or(&empty_record);
        let previous_nodes = previous
            .map(|state| {
                state
                    .nodes
                    .iter()
                    .map(|node| (node.id.clone(), node.clone()))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let next_nodes = nodes
            .iter()
            .map(|node| (node.id.clone(), node.clone()))
            .collect::<HashMap<_, _>>();

        let old_ids = previous_nodes.keys().cloned().collect::<HashSet<_>>();
        let new_ids = next_nodes.keys().cloned().collect::<HashSet<_>>();

        let removed = old_ids
            .difference(&new_ids)
            .filter_map(|id| {
                previous_nodes
                    .get(id)
                    .map(|node| observed_node(node.clone(), previous_record.fingerprints.get(id)))
            })
            .collect::<Vec<_>>();
        let added = new_ids
            .difference(&old_ids)
            .filter_map(|id| {
                next_nodes
                    .get(id)
                    .map(|node| observed_node(node.clone(), fingerprints.get(id)))
            })
            .collect::<Vec<_>>();
        let updated = old_ids
            .intersection(&new_ids)
            .filter_map(|id| {
                let before = previous_nodes.get(id)?.clone();
                let after = next_nodes.get(id)?.clone();
                Some((
                    observed_node(before, previous_record.fingerprints.get(id)),
                    observed_node(after, fingerprints.get(id)),
                ))
            })
            .collect::<Vec<_>>();

        let previous_edges = previous
            .map(|state| state.edges.clone())
            .unwrap_or_default();
        let previous_edge_keys = previous_edges.iter().map(edge_key).collect::<HashSet<_>>();
        let next_edge_keys = edges.iter().map(edge_key).collect::<HashSet<_>>();
        let edge_removed = previous_edges
            .into_iter()
            .filter(|edge| !next_edge_keys.contains(&edge_key(edge)))
            .collect::<Vec<_>>();
        let edge_added = edges
            .iter()
            .filter(|edge| !previous_edge_keys.contains(&edge_key(edge)))
            .cloned()
            .collect::<Vec<_>>();

        ObservedChangeSet {
            meta,
            trigger,
            files: vec![file_id],
            previous_path: previous_path.map(|path| path.to_string_lossy().into_owned().into()),
            current_path: current_path.map(|path| path.to_string_lossy().into_owned().into()),
            added,
            removed,
            updated,
            edge_added,
            edge_removed,
        }
    }
}

fn is_derived_kind(kind: EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls
            | EdgeKind::Imports
            | EdgeKind::Implements
            | EdgeKind::Specifies
            | EdgeKind::Validates
            | EdgeKind::RelatedTo
    )
}

fn observed_node(node: Node, fingerprint: Option<&NodeFingerprint>) -> ObservedNode {
    ObservedNode {
        node,
        fingerprint: fingerprint
            .cloned()
            .unwrap_or_else(|| prism_ir::SymbolFingerprint::new(0)),
    }
}

fn edge_key(edge: &Edge) -> (EdgeKind, NodeId, NodeId, u8, u32) {
    (
        edge.kind,
        edge.source.clone(),
        edge.target.clone(),
        match edge.origin {
            EdgeOrigin::Static => 0,
            EdgeOrigin::Inferred => 1,
        },
        edge.confidence.to_bits(),
    )
}

fn default_event_meta() -> EventMeta {
    NEXT_OBSERVED_EVENT_ID.fetch_add(1, Ordering::Relaxed);
    EventMeta {
        id: EventId::new(new_prefixed_id("observed")),
        ts: current_timestamp(),
        actor: EventActor::System,
        correlation: None,
        causation: None,
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}
