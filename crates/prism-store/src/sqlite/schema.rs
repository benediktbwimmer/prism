use anyhow::Result;
use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 13;

pub(super) fn init_schema(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 | SCHEMA_VERSION => {}
        11 | 12 => {}
        _ => reset_schema(conn)?,
    }

    conn.execute_batch(current_schema_sql())?;
    if matches!(version, 11 | 12) {
        super::memory_entries::backfill_event_log_if_needed(conn)?;
    }
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn current_schema_sql() -> &'static str {
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
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL,
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
            caller_crate_name TEXT NOT NULL,
            caller_path TEXT NOT NULL,
            caller_kind INTEGER NOT NULL,
            name TEXT NOT NULL,
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL,
            module_path TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS unresolved_imports (
            file_path TEXT NOT NULL,
            importer_crate_name TEXT NOT NULL,
            importer_path TEXT NOT NULL,
            importer_kind INTEGER NOT NULL,
            path TEXT NOT NULL,
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL,
            module_path TEXT NOT NULL,
            target_path TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS unresolved_impls (
            file_path TEXT NOT NULL,
            impl_crate_name TEXT NOT NULL,
            impl_path TEXT NOT NULL,
            impl_kind INTEGER NOT NULL,
            target TEXT NOT NULL,
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL,
            module_path TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS unresolved_intents (
            file_path TEXT NOT NULL,
            source_crate_name TEXT NOT NULL,
            source_path TEXT NOT NULL,
            source_kind INTEGER NOT NULL,
            kind INTEGER NOT NULL,
            target TEXT NOT NULL,
            span_start INTEGER NOT NULL,
            span_end INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS snapshots (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS memory_entry_log (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id TEXT NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_memory_entry_log_memory_id_sequence
            ON memory_entry_log(memory_id, sequence DESC);

        CREATE TABLE IF NOT EXISTS memory_event_log (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            memory_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            action TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_memory_event_log_memory_id_sequence
            ON memory_event_log(memory_id, sequence DESC);

        CREATE INDEX IF NOT EXISTS idx_memory_event_log_scope_sequence
            ON memory_event_log(scope, sequence DESC);

        CREATE TABLE IF NOT EXISTS history_node_lineages (
            node_crate_name TEXT NOT NULL,
            node_path TEXT NOT NULL,
            node_kind INTEGER NOT NULL,
            lineage TEXT NOT NULL,
            PRIMARY KEY (node_crate_name, node_path, node_kind)
        );

        CREATE TABLE IF NOT EXISTS history_events (
            event_id TEXT PRIMARY KEY,
            ts INTEGER NOT NULL,
            lineage TEXT NOT NULL,
            payload TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS history_co_change (
            source_lineage TEXT NOT NULL,
            target_lineage TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (source_lineage, target_lineage)
        );

        CREATE TABLE IF NOT EXISTS history_tombstones (
            lineage TEXT PRIMARY KEY,
            payload TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS projection_co_change (
            source_lineage TEXT NOT NULL,
            target_lineage TEXT NOT NULL,
            count INTEGER NOT NULL,
            PRIMARY KEY (source_lineage, target_lineage)
        );

        CREATE INDEX IF NOT EXISTS idx_projection_co_change_rank
            ON projection_co_change(source_lineage, count DESC, target_lineage);

        CREATE TABLE IF NOT EXISTS projection_validation (
            lineage TEXT NOT NULL,
            label TEXT NOT NULL,
            score REAL NOT NULL,
            last_seen INTEGER NOT NULL,
            PRIMARY KEY (lineage, label)
        );

        CREATE INDEX IF NOT EXISTS idx_edges_file_path_kind
            ON edges(file_path, kind);

        CREATE INDEX IF NOT EXISTS idx_file_nodes_file_path_node
            ON file_nodes(file_path, node_crate_name, node_path, node_kind);

        CREATE INDEX IF NOT EXISTS idx_node_fingerprints_file_path
            ON node_fingerprints(file_path);

        CREATE INDEX IF NOT EXISTS idx_unresolved_calls_file_path
            ON unresolved_calls(file_path);

        CREATE INDEX IF NOT EXISTS idx_unresolved_imports_file_path
            ON unresolved_imports(file_path);

        CREATE INDEX IF NOT EXISTS idx_unresolved_impls_file_path
            ON unresolved_impls(file_path);

        CREATE INDEX IF NOT EXISTS idx_unresolved_intents_file_path
            ON unresolved_intents(file_path);

        CREATE INDEX IF NOT EXISTS idx_history_events_ts_id
            ON history_events(ts, event_id);

        CREATE INDEX IF NOT EXISTS idx_history_events_lineage
            ON history_events(lineage, ts, event_id);
    "#
}

fn reset_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
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
        DROP TABLE IF EXISTS unresolved_intents;
        DROP TABLE IF EXISTS snapshots;
        DROP TABLE IF EXISTS memory_entry_log;
        DROP TABLE IF EXISTS memory_event_log;
        DROP TABLE IF EXISTS history_node_lineages;
        DROP TABLE IF EXISTS history_events;
        DROP TABLE IF EXISTS history_co_change;
        DROP TABLE IF EXISTS history_tombstones;
        DROP TABLE IF EXISTS projection_co_change;
        DROP TABLE IF EXISTS projection_validation;
        "#,
    )?;
    Ok(())
}
