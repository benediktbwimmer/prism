use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::graph::{FileRecord, FileState, Graph, GraphSnapshot};

use super::codecs::{
    decode_edge_kind, decode_edge_origin, decode_language, decode_node_kind,
    deserialize_fingerprint, encode_edge_kind, encode_edge_origin, encode_language,
    encode_node_kind,
};

pub(super) fn load_graph(conn: &Connection) -> Result<Option<Graph>> {
    let next_file_id = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'next_file_id'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(next_file_id) = next_file_id else {
        return Ok(None);
    };

    let mut nodes = HashMap::<prism_ir::NodeId, prism_ir::Node>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT crate_name, path, kind, name, file_id, span_start, span_end, language FROM nodes",
        )?;
        let rows = stmt.query_map([], |row| {
            let kind = decode_node_kind(row.get(2)?)?;
            let id =
                prism_ir::NodeId::new(row.get::<_, String>(0)?, row.get::<_, String>(1)?, kind);
            Ok(prism_ir::Node {
                id: id.clone(),
                name: row.get::<_, String>(3)?.into(),
                kind,
                file: prism_ir::FileId(row.get::<_, u32>(4)?),
                span: prism_ir::Span {
                    start: row.get(5)?,
                    end: row.get(6)?,
                },
                language: decode_language(row.get(7)?)?,
            })
        })?;
        for node in rows {
            let node = node?;
            nodes.insert(node.id.clone(), node);
        }
    }

    let mut edges = Vec::<prism_ir::Edge>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT kind, source_crate_name, source_path, source_kind, target_crate_name, target_path, target_kind, origin, confidence FROM edges",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(prism_ir::Edge {
                kind: decode_edge_kind(row.get(0)?)?,
                source: prism_ir::NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                target: prism_ir::NodeId::new(
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
        let mut stmt =
            conn.prepare("SELECT path, file_id, hash FROM file_records ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                prism_ir::FileId(row.get::<_, u32>(1)?),
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
                    unresolved_intents: Vec::new(),
                },
            );
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT file_path, node_crate_name, node_path, node_kind FROM file_nodes ORDER BY file_path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                prism_ir::NodeId::new(
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

    load_node_fingerprints(conn, &mut file_records)?;
    load_unresolved_calls(conn, &mut file_records)?;
    load_unresolved_imports(conn, &mut file_records)?;
    load_unresolved_impls(conn, &mut file_records)?;
    load_unresolved_intents(conn, &mut file_records)?;

    Ok(Some(Graph::from_snapshot(GraphSnapshot {
        nodes,
        edges,
        file_records,
        next_file_id: next_file_id as u32,
    })))
}

pub(super) fn delete_file_state(tx: &Transaction<'_>, path: &Path) -> Result<()> {
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
    tx.execute(
        "DELETE FROM unresolved_intents WHERE file_path = ?1",
        params![file_path.as_ref()],
    )?;
    Ok(())
}

pub(super) fn save_file_state_tx(tx: &Transaction<'_>, state: &FileState) -> Result<()> {
    delete_file_state(tx, &state.path)?;

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
            "INSERT INTO nodes(crate_name, path, kind, name, file_id, span_start, span_end, language)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                node.id.crate_name.as_str(),
                node.id.path.as_str(),
                encode_node_kind(node.kind),
                node.name.as_str(),
                node.file.0,
                node.span.start,
                node.span.end,
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
                serde_json::to_string(fingerprint)?,
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
            "INSERT INTO unresolved_calls(file_path, caller_crate_name, caller_path, caller_kind, name, span_start, span_end, module_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                file_path.as_ref(),
                call.caller.crate_name.as_str(),
                call.caller.path.as_str(),
                encode_node_kind(call.caller.kind),
                call.name.as_str(),
                call.span.start,
                call.span.end,
                call.module_path.as_str(),
            ],
        )?;
    }

    for import in &state.record.unresolved_imports {
        tx.execute(
            "INSERT INTO unresolved_imports(file_path, importer_crate_name, importer_path, importer_kind, path, span_start, span_end, module_path, target_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                file_path.as_ref(),
                import.importer.crate_name.as_str(),
                import.importer.path.as_str(),
                encode_node_kind(import.importer.kind),
                import.path.as_str(),
                import.span.start,
                import.span.end,
                import.module_path.as_str(),
                import.path.as_str(),
            ],
        )?;
    }

    for implementation in &state.record.unresolved_impls {
        tx.execute(
            "INSERT INTO unresolved_impls(file_path, impl_crate_name, impl_path, impl_kind, target, span_start, span_end, module_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                file_path.as_ref(),
                implementation.impl_node.crate_name.as_str(),
                implementation.impl_node.path.as_str(),
                encode_node_kind(implementation.impl_node.kind),
                implementation.target.as_str(),
                implementation.span.start,
                implementation.span.end,
                implementation.module_path.as_str(),
            ],
        )?;
    }

    for intent in &state.record.unresolved_intents {
        tx.execute(
            "INSERT INTO unresolved_intents(file_path, source_crate_name, source_path, source_kind, kind, target, span_start, span_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                file_path.as_ref(),
                intent.source.crate_name.as_str(),
                intent.source.path.as_str(),
                encode_node_kind(intent.source.kind),
                encode_edge_kind(intent.kind),
                intent.target.as_str(),
                intent.span.start,
                intent.span.end,
            ],
        )?;
    }

    Ok(())
}

pub(super) fn replace_derived_edges_tx(tx: &Transaction<'_>, graph: &Graph) -> Result<()> {
    tx.execute(
        "DELETE FROM edges WHERE file_path IS NULL AND kind IN (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            encode_edge_kind(prism_ir::EdgeKind::Calls),
            encode_edge_kind(prism_ir::EdgeKind::Imports),
            encode_edge_kind(prism_ir::EdgeKind::Implements),
            encode_edge_kind(prism_ir::EdgeKind::Specifies),
            encode_edge_kind(prism_ir::EdgeKind::Validates),
            encode_edge_kind(prism_ir::EdgeKind::RelatedTo),
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

    Ok(())
}

pub(super) fn finalize_tx(tx: &Transaction<'_>, graph: &Graph) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES ('next_file_id', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![graph.next_file_id()],
    )?;

    for node in graph.root_nodes() {
        tx.execute(
            "INSERT INTO nodes(crate_name, path, kind, name, file_id, span_start, span_end, language)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(crate_name, path, kind) DO UPDATE SET
                name = excluded.name,
                file_id = excluded.file_id,
                span_start = excluded.span_start,
                span_end = excluded.span_end,
                language = excluded.language",
            params![
                node.id.crate_name.as_str(),
                node.id.path.as_str(),
                encode_node_kind(node.kind),
                node.name.as_str(),
                node.file.0,
                node.span.start,
                node.span.end,
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
            encode_edge_kind(prism_ir::EdgeKind::Contains),
            encode_node_kind(prism_ir::NodeKind::Workspace),
            encode_node_kind(prism_ir::NodeKind::Package)
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
            prism_ir::NodeId::new(
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                decode_node_kind(row.get(3)?)?,
            ),
            deserialize_fingerprint(&row.get::<_, String>(4)?),
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
        "SELECT file_path, caller_crate_name, caller_path, caller_kind, name, span_start, span_end, module_path FROM unresolved_calls",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            prism_parser::UnresolvedCall {
                caller: prism_ir::NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                name: row.get::<_, String>(4)?.into(),
                span: prism_ir::Span {
                    start: row.get(5)?,
                    end: row.get(6)?,
                },
                module_path: row.get::<_, String>(7)?.into(),
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
        "SELECT file_path, importer_crate_name, importer_path, importer_kind, path, span_start, span_end, module_path FROM unresolved_imports",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            prism_parser::UnresolvedImport {
                importer: prism_ir::NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                path: row.get::<_, String>(4)?.into(),
                span: prism_ir::Span {
                    start: row.get(5)?,
                    end: row.get(6)?,
                },
                module_path: row.get::<_, String>(7)?.into(),
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
        "SELECT file_path, impl_crate_name, impl_path, impl_kind, target, span_start, span_end, module_path FROM unresolved_impls",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            prism_parser::UnresolvedImpl {
                impl_node: prism_ir::NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                target: row.get::<_, String>(4)?.into(),
                span: prism_ir::Span {
                    start: row.get(5)?,
                    end: row.get(6)?,
                },
                module_path: row.get::<_, String>(7)?.into(),
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

fn load_unresolved_intents(
    conn: &Connection,
    file_records: &mut HashMap<PathBuf, FileRecord>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT file_path, source_crate_name, source_path, source_kind, kind, target, span_start, span_end FROM unresolved_intents",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            prism_parser::UnresolvedIntent {
                source: prism_ir::NodeId::new(
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    decode_node_kind(row.get(3)?)?,
                ),
                kind: decode_edge_kind(row.get(4)?)?,
                target: row.get::<_, String>(5)?.into(),
                span: prism_ir::Span {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
            },
        ))
    })?;
    for row in rows {
        let (path, unresolved) = row?;
        if let Some(record) = file_records.get_mut(&path) {
            record.unresolved_intents.push(unresolved);
        }
    }
    Ok(())
}
