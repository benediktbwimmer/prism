use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{
    Edge, EdgeIndex, EdgeKind, EdgeOrigin, FileId, GraphChange, Language, Node, NodeId, NodeKind,
};
use prism_parser::{NodeFingerprint, UnresolvedCall, UnresolvedImpl, UnresolvedImport};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};

const SCHEMA_VERSION: i64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: FileId,
    pub hash: u64,
    pub nodes: Vec<NodeId>,
    pub fingerprints: HashMap<NodeId, NodeFingerprint>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub unresolved_imports: Vec<UnresolvedImport>,
    pub unresolved_impls: Vec<UnresolvedImpl>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUpdate {
    pub file_id: FileId,
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
    next_file_id: u32,
}

pub trait Store {
    fn load_graph(&mut self) -> Result<Option<Graph>>;
    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()>;
    fn remove_file_state(&mut self, path: &Path) -> Result<()>;
    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()>;
    fn finalize(&mut self, graph: &Graph) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct MemoryStore {
    snapshot: Option<GraphSnapshot>,
}

impl Store for MemoryStore {
    fn load_graph(&mut self) -> Result<Option<Graph>> {
        Ok(self.snapshot.clone().map(Graph::from_snapshot))
    }

    fn save_file_state(&mut self, _path: &Path, _graph: &Graph) -> Result<()> {
        Ok(())
    }

    fn remove_file_state(&mut self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn replace_derived_edges(&mut self, _graph: &Graph) -> Result<()> {
        Ok(())
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        self.snapshot = Some(graph.snapshot());
        Ok(())
    }
}

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            self.conn.execute_batch(
                r#"
                DROP TABLE IF EXISTS metadata;
                DROP TABLE IF EXISTS nodes;
                DROP TABLE IF EXISTS edges;
                DROP TABLE IF EXISTS file_records;
                DROP TABLE IF EXISTS file_nodes;
                DROP TABLE IF EXISTS node_fingerprints;
                DROP TABLE IF EXISTS unresolved_calls;
                DROP TABLE IF EXISTS unresolved_imports;
                DROP TABLE IF EXISTS unresolved_impls;
                "#,
            )?;
        }

        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS nodes (
                crate_name TEXT NOT NULL,
                path TEXT NOT NULL,
                kind INTEGER NOT NULL,
                name TEXT NOT NULL,
                file_id INTEGER NOT NULL,
                start_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                language INTEGER NOT NULL,
                PRIMARY KEY (crate_name, path, kind)
            );

            CREATE TABLE IF NOT EXISTS edges (
                file_path TEXT,
                kind INTEGER NOT NULL,
                source_crate_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                source_kind INTEGER NOT NULL,
                target_crate_name TEXT NOT NULL,
                target_path TEXT NOT NULL,
                target_kind INTEGER NOT NULL,
                origin INTEGER NOT NULL,
                confidence REAL NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_records (
                path TEXT PRIMARY KEY,
                file_id INTEGER NOT NULL,
                hash INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_nodes (
                file_path TEXT NOT NULL,
                node_crate_name TEXT NOT NULL,
                node_path TEXT NOT NULL,
                node_kind INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS node_fingerprints (
                file_path TEXT NOT NULL,
                node_crate_name TEXT NOT NULL,
                node_path TEXT NOT NULL,
                node_kind INTEGER NOT NULL,
                fingerprint TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS unresolved_calls (
                file_path TEXT NOT NULL,
                source_crate_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                source_kind INTEGER NOT NULL,
                name TEXT NOT NULL,
                module_path TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS unresolved_imports (
                file_path TEXT NOT NULL,
                source_crate_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                source_kind INTEGER NOT NULL,
                name TEXT NOT NULL,
                module_path TEXT NOT NULL,
                target_path TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS unresolved_impls (
                file_path TEXT NOT NULL,
                source_crate_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                source_kind INTEGER NOT NULL,
                name TEXT NOT NULL,
                module_path TEXT NOT NULL,
                trait_path TEXT NOT NULL
            );
            "#,
        )?;
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }
}

impl Store for SqliteStore {
    fn load_graph(&mut self) -> Result<Option<Graph>> {
        let next_file_id = self
            .conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'next_file_id'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(next_file_id) = next_file_id else {
            return Ok(None);
        };

        let mut nodes = HashMap::<NodeId, Node>::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT crate_name, path, kind, name, file_id, start_line, start_col, end_line, end_col, language FROM nodes",
            )?;
            let rows = stmt.query_map([], |row| {
                let kind = decode_node_kind(row.get(2)?)?;
                let id = NodeId::new(row.get::<_, String>(0)?, row.get::<_, String>(1)?, kind);
                Ok(Node {
                    id: id.clone(),
                    name: row.get::<_, String>(3)?.into(),
                    kind,
                    file: FileId(row.get::<_, u32>(4)?),
                    span: prism_ir::Span {
                        start_line: row.get(5)?,
                        start_col: row.get(6)?,
                        end_line: row.get(7)?,
                        end_col: row.get(8)?,
                    },
                    language: decode_language(row.get(9)?)?,
                })
            })?;
            for node in rows {
                let node = node?;
                nodes.insert(node.id.clone(), node);
            }
        }

        let mut edges = Vec::<Edge>::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT kind, source_crate_name, source_path, source_kind, target_crate_name, target_path, target_kind, origin, confidence FROM edges",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Edge {
                    kind: decode_edge_kind(row.get(0)?)?,
                    source: NodeId::new(
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        decode_node_kind(row.get(3)?)?,
                    ),
                    target: NodeId::new(
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        decode_node_kind(row.get(6)?)?,
                    ),
                    origin: decode_edge_origin(row.get(7)?)?,
                    confidence: row.get(8)?,
                })
            })?;
            for edge in rows {
                edges.push(edge?);
            }
        }

        let mut file_records = HashMap::<PathBuf, FileRecord>::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT path, file_id, hash FROM file_records ORDER BY path")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    PathBuf::from(row.get::<_, String>(0)?),
                    FileId(row.get::<_, u32>(1)?),
                    row.get::<_, i64>(2)? as u64,
                ))
            })?;
            for row in rows {
                let (path, file_id, hash) = row?;
                file_records.insert(
                    path,
                    FileRecord {
                        file_id,
                        hash,
                        nodes: Vec::new(),
                        fingerprints: HashMap::new(),
                        unresolved_calls: Vec::new(),
                        unresolved_imports: Vec::new(),
                        unresolved_impls: Vec::new(),
                    },
                );
            }
        }

        {
            let mut stmt = self.conn.prepare(
                "SELECT file_path, node_crate_name, node_path, node_kind FROM file_nodes ORDER BY file_path",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    PathBuf::from(row.get::<_, String>(0)?),
                    NodeId::new(
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        decode_node_kind(row.get(3)?)?,
                    ),
                ))
            })?;
            for row in rows {
                let (path, node_id) = row?;
                if let Some(record) = file_records.get_mut(&path) {
                    record.nodes.push(node_id);
                }
            }
        }

        load_node_fingerprints(&self.conn, &mut file_records)?;
        load_unresolved_calls(&self.conn, &mut file_records)?;
        load_unresolved_imports(&self.conn, &mut file_records)?;
        load_unresolved_impls(&self.conn, &mut file_records)?;

        Ok(Some(Graph::from_snapshot(GraphSnapshot {
            nodes,
            edges,
            file_records,
            next_file_id: next_file_id as u32,
        })))
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        let Some(state) = graph.file_state(path) else {
            return Ok(());
        };
        let tx = self.conn.transaction()?;
        delete_file_state(&tx, path)?;

        let file_path = state.path.to_string_lossy();
        tx.execute(
            "INSERT INTO file_records(path, file_id, hash) VALUES (?1, ?2, ?3)",
            params![
                file_path.as_ref(),
                state.record.file_id.0,
                state.record.hash as i64
            ],
        )?;

        for node in &state.nodes {
            tx.execute(
                "INSERT INTO nodes(crate_name, path, kind, name, file_id, start_line, start_col, end_line, end_col, language)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    node.id.crate_name.as_str(),
                    node.id.path.as_str(),
                    encode_node_kind(node.kind),
                    node.name.as_str(),
                    node.file.0,
                    node.span.start_line,
                    node.span.start_col,
                    node.span.end_line,
                    node.span.end_col,
                    encode_language(node.language),
                ],
            )?;
        }

        for node_id in &state.record.nodes {
            tx.execute(
                "INSERT INTO file_nodes(file_path, node_crate_name, node_path, node_kind) VALUES (?1, ?2, ?3, ?4)",
                params![
                    file_path.as_ref(),
                    node_id.crate_name.as_str(),
                    node_id.path.as_str(),
                    encode_node_kind(node_id.kind),
                ],
            )?;
        }

        for (node_id, fingerprint) in &state.record.fingerprints {
            tx.execute(
                "INSERT INTO node_fingerprints(file_path, node_crate_name, node_path, node_kind, fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file_path.as_ref(),
                    node_id.crate_name.as_str(),
                    node_id.path.as_str(),
                    encode_node_kind(node_id.kind),
                    fingerprint.0.as_str(),
                ],
            )?;
        }

        for edge in &state.edges {
            tx.execute(
                "INSERT INTO edges(file_path, kind, source_crate_name, source_path, source_kind, target_crate_name, target_path, target_kind, origin, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    file_path.as_ref(),
                    encode_edge_kind(edge.kind),
                    edge.source.crate_name.as_str(),
                    edge.source.path.as_str(),
                    encode_node_kind(edge.source.kind),
                    edge.target.crate_name.as_str(),
                    edge.target.path.as_str(),
                    encode_node_kind(edge.target.kind),
                    encode_edge_origin(edge.origin),
                    edge.confidence,
                ],
            )?;
        }

        for call in &state.record.unresolved_calls {
            tx.execute(
                "INSERT INTO unresolved_calls(file_path, source_crate_name, source_path, source_kind, name, module_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    file_path.as_ref(),
                    call.source.crate_name.as_str(),
                    call.source.path.as_str(),
                    encode_node_kind(call.source.kind),
                    call.name.as_str(),
                    call.module_path.as_str(),
                ],
            )?;
        }

        for import in &state.record.unresolved_imports {
            tx.execute(
                "INSERT INTO unresolved_imports(file_path, source_crate_name, source_path, source_kind, name, module_path, target_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file_path.as_ref(),
                    import.source.crate_name.as_str(),
                    import.source.path.as_str(),
                    encode_node_kind(import.source.kind),
                    import.name.as_str(),
                    import.module_path.as_str(),
                    import.target_path.as_str(),
                ],
            )?;
        }

        for implementation in &state.record.unresolved_impls {
            tx.execute(
                "INSERT INTO unresolved_impls(file_path, source_crate_name, source_path, source_kind, name, module_path, trait_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file_path.as_ref(),
                    implementation.source.crate_name.as_str(),
                    implementation.source.path.as_str(),
                    encode_node_kind(implementation.source.kind),
                    implementation.name.as_str(),
                    implementation.module_path.as_str(),
                    implementation.trait_path.as_str(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        delete_file_state(&tx, path)?;
        tx.commit()?;
        Ok(())
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM edges WHERE file_path IS NULL AND kind IN (?1, ?2, ?3)",
            params![
                encode_edge_kind(EdgeKind::Calls),
                encode_edge_kind(EdgeKind::Imports),
                encode_edge_kind(EdgeKind::Implements)
            ],
        )?;

        for edge in graph.derived_edges() {
            tx.execute(
                "INSERT INTO edges(file_path, kind, source_crate_name, source_path, source_kind, target_crate_name, target_path, target_kind, origin, confidence)
                 VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    encode_edge_kind(edge.kind),
                    edge.source.crate_name.as_str(),
                    edge.source.path.as_str(),
                    encode_node_kind(edge.source.kind),
                    edge.target.crate_name.as_str(),
                    edge.target.path.as_str(),
                    encode_node_kind(edge.target.kind),
                    encode_edge_origin(edge.origin),
                    edge.confidence,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO metadata(key, value) VALUES ('next_file_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![graph.next_file_id()],
        )?;

        for node in graph.root_nodes() {
            tx.execute(
                "INSERT INTO nodes(crate_name, path, kind, name, file_id, start_line, start_col, end_line, end_col, language)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(crate_name, path, kind) DO UPDATE SET
                    name = excluded.name,
                    file_id = excluded.file_id,
                    start_line = excluded.start_line,
                    start_col = excluded.start_col,
                    end_line = excluded.end_line,
                    end_col = excluded.end_col,
                    language = excluded.language",
                params![
                    node.id.crate_name.as_str(),
                    node.id.path.as_str(),
                    encode_node_kind(node.kind),
                    node.name.as_str(),
                    node.file.0,
                    node.span.start_line,
                    node.span.start_col,
                    node.span.end_line,
                    node.span.end_col,
                    encode_language(node.language),
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM edges
             WHERE file_path IS NULL
               AND kind = ?1
               AND source_kind = ?2
               AND target_kind = ?3",
            params![
                encode_edge_kind(EdgeKind::Contains),
                encode_node_kind(NodeKind::Workspace),
                encode_node_kind(NodeKind::Package)
            ],
        )?;

        for edge in graph.root_edges() {
            tx.execute(
                "INSERT INTO edges(file_path, kind, source_crate_name, source_path, source_kind, target_crate_name, target_path, target_kind, origin, confidence)
                 VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    encode_edge_kind(edge.kind),
                    edge.source.crate_name.as_str(),
                    edge.source.path.as_str(),
                    encode_node_kind(edge.source.kind),
                    edge.target.crate_name.as_str(),
                    edge.target.path.as_str(),
                    encode_node_kind(edge.target.kind),
                    encode_edge_origin(edge.origin),
                    edge.confidence,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
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
        reanchors: &[(NodeId, NodeId)],
    ) -> FileUpdate {
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
            reanchors,
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
        reanchors: &[(NodeId, NodeId)],
    ) -> FileUpdate {
        let baseline_path = previous_path.unwrap_or(path);
        let previous = self.file_records.get(baseline_path).cloned();
        let file_id = previous
            .as_ref()
            .map(|record| record.file_id)
            .unwrap_or_else(|| self.ensure_file(path));
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
                fingerprints,
                unresolved_calls,
                unresolved_imports,
                unresolved_impls,
            },
        );
        self.rebuild_adjacency();
        FileUpdate { file_id, changes }
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

    pub fn next_file_id(&self) -> u32 {
        self.next_file_id
    }

    pub fn file_state(&self, path: &Path) -> Option<FileState> {
        let record = self.file_records.get(path)?.clone();
        let node_ids: HashSet<NodeId> = record.nodes.iter().cloned().collect();
        let nodes = record
            .nodes
            .iter()
            .filter_map(|id| self.nodes.get(id).cloned())
            .collect::<Vec<_>>();
        let edges = self
            .edges
            .iter()
            .filter(|edge| !is_derived_kind(edge.kind))
            .filter(|edge| node_ids.contains(&edge.source) || node_ids.contains(&edge.target))
            .cloned()
            .collect::<Vec<_>>();

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
        self.remove_file_nodes(path);
        if let Some(file_id) = self.path_to_file.remove(path) {
            self.file_paths.remove(&file_id);
        }
        self.rebuild_adjacency();
        changes
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
}

fn is_derived_kind(kind: EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Calls | EdgeKind::Imports | EdgeKind::Implements
    )
}

fn delete_file_state(tx: &rusqlite::Transaction<'_>, path: &Path) -> Result<()> {
    let file_path = path.to_string_lossy();
    tx.execute(
        "DELETE FROM edges WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM nodes
         WHERE EXISTS (
            SELECT 1 FROM file_nodes
            WHERE file_nodes.file_path = ?1
              AND file_nodes.node_crate_name = nodes.crate_name
              AND file_nodes.node_path = nodes.path
              AND file_nodes.node_kind = nodes.kind
         )",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM file_nodes WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM node_fingerprints WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM file_records WHERE path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM unresolved_calls WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM unresolved_imports WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    tx.execute(
        "DELETE FROM unresolved_impls WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    Ok(())
}

fn load_node_fingerprints(
    conn: &Connection,
    file_records: &mut HashMap<PathBuf, FileRecord>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT file_path, node_crate_name, node_path, node_kind, fingerprint FROM node_fingerprints",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            NodeId::new(
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                decode_node_kind(row.get(3)?)?,
            ),
            NodeFingerprint::new(row.get::<_, String>(4)?),
        ))
    })?;
    for row in rows {
        let (path, node_id, fingerprint) = row?;
        if let Some(record) = file_records.get_mut(&path) {
            record.fingerprints.insert(node_id, fingerprint);
        }
    }
    Ok(())
}

fn load_unresolved_calls(
    conn: &Connection,
    file_records: &mut HashMap<PathBuf, FileRecord>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT file_path, source_crate_name, source_path, source_kind, name, module_path FROM unresolved_calls",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            UnresolvedCall {
                source: NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                name: row.get::<_, String>(4)?.into(),
                module_path: row.get::<_, String>(5)?.into(),
            },
        ))
    })?;
    for row in rows {
        let (path, unresolved) = row?;
        if let Some(record) = file_records.get_mut(&path) {
            record.unresolved_calls.push(unresolved);
        }
    }
    Ok(())
}

fn load_unresolved_imports(
    conn: &Connection,
    file_records: &mut HashMap<PathBuf, FileRecord>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT file_path, source_crate_name, source_path, source_kind, name, module_path, target_path FROM unresolved_imports",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            UnresolvedImport {
                source: NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                name: row.get::<_, String>(4)?.into(),
                module_path: row.get::<_, String>(5)?.into(),
                target_path: row.get::<_, String>(6)?.into(),
            },
        ))
    })?;
    for row in rows {
        let (path, unresolved) = row?;
        if let Some(record) = file_records.get_mut(&path) {
            record.unresolved_imports.push(unresolved);
        }
    }
    Ok(())
}

fn load_unresolved_impls(
    conn: &Connection,
    file_records: &mut HashMap<PathBuf, FileRecord>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT file_path, source_crate_name, source_path, source_kind, name, module_path, trait_path FROM unresolved_impls",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            UnresolvedImpl {
                source: NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                name: row.get::<_, String>(4)?.into(),
                module_path: row.get::<_, String>(5)?.into(),
                trait_path: row.get::<_, String>(6)?.into(),
            },
        ))
    })?;
    for row in rows {
        let (path, unresolved) = row?;
        if let Some(record) = file_records.get_mut(&path) {
            record.unresolved_impls.push(unresolved);
        }
    }
    Ok(())
}

fn encode_node_kind(kind: NodeKind) -> i64 {
    match kind {
        NodeKind::Workspace => 0,
        NodeKind::Package => 1,
        NodeKind::Document => 2,
        NodeKind::Module => 3,
        NodeKind::Function => 4,
        NodeKind::Struct => 5,
        NodeKind::Enum => 6,
        NodeKind::Trait => 7,
        NodeKind::Impl => 8,
        NodeKind::Method => 9,
        NodeKind::Field => 10,
        NodeKind::TypeAlias => 11,
        NodeKind::MarkdownHeading => 12,
        NodeKind::JsonKey => 13,
        NodeKind::YamlKey => 14,
    }
}

fn decode_node_kind(value: i64) -> rusqlite::Result<NodeKind> {
    Ok(match value {
        0 => NodeKind::Workspace,
        1 => NodeKind::Package,
        2 => NodeKind::Document,
        3 => NodeKind::Module,
        4 => NodeKind::Function,
        5 => NodeKind::Struct,
        6 => NodeKind::Enum,
        7 => NodeKind::Trait,
        8 => NodeKind::Impl,
        9 => NodeKind::Method,
        10 => NodeKind::Field,
        11 => NodeKind::TypeAlias,
        12 => NodeKind::MarkdownHeading,
        13 => NodeKind::JsonKey,
        14 => NodeKind::YamlKey,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid node kind: {other}"
            )))
        }
    })
}

fn encode_edge_kind(kind: EdgeKind) -> i64 {
    match kind {
        EdgeKind::Contains => 0,
        EdgeKind::Calls => 1,
        EdgeKind::References => 2,
        EdgeKind::Implements => 3,
        EdgeKind::Defines => 4,
        EdgeKind::Imports => 5,
        EdgeKind::DependsOn => 6,
    }
}

fn decode_edge_kind(value: i64) -> rusqlite::Result<EdgeKind> {
    Ok(match value {
        0 => EdgeKind::Contains,
        1 => EdgeKind::Calls,
        2 => EdgeKind::References,
        3 => EdgeKind::Implements,
        4 => EdgeKind::Defines,
        5 => EdgeKind::Imports,
        6 => EdgeKind::DependsOn,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid edge kind: {other}"
            )))
        }
    })
}

fn encode_language(language: Language) -> i64 {
    match language {
        Language::Rust => 0,
        Language::Markdown => 1,
        Language::Json => 2,
        Language::Yaml => 3,
        Language::Unknown => 4,
    }
}

fn decode_language(value: i64) -> rusqlite::Result<Language> {
    Ok(match value {
        0 => Language::Rust,
        1 => Language::Markdown,
        2 => Language::Json,
        3 => Language::Yaml,
        4 => Language::Unknown,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid language: {other}"
            )))
        }
    })
}

fn encode_edge_origin(origin: EdgeOrigin) -> i64 {
    match origin {
        EdgeOrigin::Static => 0,
        EdgeOrigin::Inferred => 1,
    }
}

fn decode_edge_origin(value: i64) -> rusqlite::Result<EdgeOrigin> {
    Ok(match value {
        0 => EdgeOrigin::Static,
        1 => EdgeOrigin::Inferred,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid edge origin: {other}"
            )))
        }
    })
}

fn from_sql_conversion_error(message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Integer,
        Box::new(IoError::new(IoErrorKind::InvalidData, message)),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{GraphChange, Span};

    use super::*;

    fn node(name: &str) -> Node {
        Node {
            id: NodeId::new("demo", format!("demo::{name}"), NodeKind::Function),
            name: name.into(),
            kind: NodeKind::Function,
            file: FileId(0),
            span: Span::line(1),
            language: Language::Rust,
        }
    }

    #[test]
    fn upsert_file_with_reanchors_emits_reanchored_change() {
        let path = Path::new("src/lib.rs");
        let mut graph = Graph::new();

        graph.upsert_file(
            path,
            1,
            vec![node("alpha")],
            Vec::new(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let old = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let new = NodeId::new("demo", "demo::renamed_alpha", NodeKind::Function);
        let update = graph.upsert_file_with_reanchors(
            path,
            2,
            vec![node("renamed_alpha")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &[(old.clone(), new.clone())],
        );

        assert_eq!(update.changes, vec![GraphChange::Reanchored { old, new }]);
    }

    #[test]
    fn remove_file_with_changes_emits_removed_nodes() {
        let path = Path::new("src/lib.rs");
        let mut graph = Graph::new();

        graph.upsert_file(
            path,
            1,
            vec![node("alpha"), node("beta")],
            Vec::new(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let removed = graph.remove_file_with_changes(path);

        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&GraphChange::Removed(NodeId::new(
            "demo",
            "demo::alpha",
            NodeKind::Function,
        ))));
        assert!(removed.contains(&GraphChange::Removed(NodeId::new(
            "demo",
            "demo::beta",
            NodeKind::Function,
        ))));
    }
}
