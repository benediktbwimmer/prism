use std::path::Path;

use anyhow::{Context, Result};
use prism_memory::{EpisodicMemorySnapshot, MemoryScope, OutcomeEvent};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use tracing::info;

use super::outcome_events::{append_local_projection_tx, LOCAL_OUTCOME_PROJECTION_LIMIT};
use super::{configure_connection, schema};

const ATTACHED_SHARED_DB: &str = "shared_runtime";
const LOCAL_TABLES: &[&str] = &[
    "nodes",
    "edges",
    "file_records",
    "file_nodes",
    "node_fingerprints",
    "unresolved_calls",
    "unresolved_imports",
    "unresolved_impls",
    "unresolved_intents",
    "history_node_lineages",
    "history_events",
    "history_tombstones",
    "projection_co_change",
    "projection_validation",
    "inference_record_log",
];
const LOCAL_SNAPSHOT_KEYS: &[&str] = &["history", "workspace_tree", "curator", "inference"];
const LOCAL_METADATA_KEYS: &[&str] = &[
    "next_file_id",
    "history:next_lineage",
    "history:next_event",
    "history:legacy_co_change_retired",
    "revision:inference",
];
#[derive(Default)]
struct MigrationStats {
    copied_local_rows: usize,
    copied_local_snapshots: usize,
    copied_local_metadata: usize,
    copied_local_memory_events: usize,
    copied_local_memory_entries: usize,
    copied_local_concepts: usize,
    copied_local_relations: usize,
    rebuilt_local_outcome_projection_rows: usize,
    scrubbed_local_rows: usize,
    scrubbed_local_snapshots: usize,
    scrubbed_local_metadata: usize,
    scrubbed_local_memory_events: usize,
    scrubbed_local_memory_entries: usize,
    scrubbed_local_concepts: usize,
    scrubbed_local_relations: usize,
    scrubbed_shared_outcome_anchors: usize,
}

pub fn migrate_worktree_cache_from_shared_runtime(
    worktree_path: &Path,
    shared_path: &Path,
) -> Result<()> {
    if worktree_path == shared_path || !shared_path.exists() {
        return Ok(());
    }
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let shared_conn = Connection::open(shared_path)
        .with_context(|| format!("failed to open shared runtime db {}", shared_path.display()))?;
    configure_connection(&shared_conn)?;
    schema::init_schema(&shared_conn)?;
    drop(shared_conn);

    let mut local_conn = Connection::open(worktree_path).with_context(|| {
        format!(
            "failed to open worktree cache db {}",
            worktree_path.display()
        )
    })?;
    configure_connection(&local_conn)?;
    schema::init_schema(&local_conn)?;
    local_conn.execute(
        &format!("ATTACH DATABASE ?1 AS {ATTACHED_SHARED_DB}"),
        params![shared_path.display().to_string()],
    )?;

    let tx = local_conn.transaction()?;
    let shared_has_local_state = database_has_local_semantic_state(&tx, ATTACHED_SHARED_DB)?;
    let local_has_local_state = database_has_local_semantic_state(&tx, "main")?;
    let shared_has_outcomes = database_has_outcome_state(&tx, ATTACHED_SHARED_DB)?;
    let local_has_outcome_projection = database_has_local_outcome_projection(&tx, "main")?;

    let mut stats = MigrationStats::default();
    let copied_local_state = if shared_has_local_state && !local_has_local_state {
        copy_local_state(&tx, &mut stats)?;
        true
    } else {
        false
    };

    if shared_has_outcomes && !local_has_outcome_projection {
        rebuild_local_outcome_projection(&tx, &mut stats)?;
    }

    if shared_has_local_state
        && (copied_local_state || database_has_local_semantic_state(&tx, "main")?)
    {
        scrub_shared_local_state(&tx, &mut stats)?;
    }
    if shared_has_outcomes && database_has_local_outcome_projection(&tx, "main")? {
        stats.scrubbed_shared_outcome_anchors +=
            delete_all_rows(&tx, ATTACHED_SHARED_DB, "outcome_event_anchor")?;
    }

    tx.commit()?;
    local_conn.execute(&format!("DETACH DATABASE {ATTACHED_SHARED_DB}"), [])?;

    if stats.copied_local_rows > 0
        || stats.copied_local_snapshots > 0
        || stats.copied_local_metadata > 0
        || stats.copied_local_memory_events > 0
        || stats.copied_local_concepts > 0
        || stats.rebuilt_local_outcome_projection_rows > 0
        || stats.scrubbed_local_rows > 0
        || stats.scrubbed_local_snapshots > 0
        || stats.scrubbed_local_memory_events > 0
        || stats.scrubbed_local_concepts > 0
    {
        info!(
            worktree_cache_path = %worktree_path.display(),
            shared_runtime_path = %shared_path.display(),
            copied_local_rows = stats.copied_local_rows,
            copied_local_snapshots = stats.copied_local_snapshots,
            copied_local_metadata = stats.copied_local_metadata,
            copied_local_memory_events = stats.copied_local_memory_events,
            copied_local_memory_entries = stats.copied_local_memory_entries,
            copied_local_concepts = stats.copied_local_concepts,
            copied_local_relations = stats.copied_local_relations,
            rebuilt_local_outcome_projection_rows = stats.rebuilt_local_outcome_projection_rows,
            scrubbed_local_rows = stats.scrubbed_local_rows,
            scrubbed_local_snapshots = stats.scrubbed_local_snapshots,
            scrubbed_local_metadata = stats.scrubbed_local_metadata,
            scrubbed_local_memory_events = stats.scrubbed_local_memory_events,
            scrubbed_local_memory_entries = stats.scrubbed_local_memory_entries,
            scrubbed_local_concepts = stats.scrubbed_local_concepts,
            scrubbed_local_relations = stats.scrubbed_local_relations,
            scrubbed_shared_outcome_anchors = stats.scrubbed_shared_outcome_anchors,
            "migrated worktree-local cache state out of shared runtime db"
        );
    }
    Ok(())
}

fn copy_local_state(conn: &Connection, stats: &mut MigrationStats) -> Result<()> {
    for table in LOCAL_TABLES {
        stats.copied_local_rows += copy_table(conn, ATTACHED_SHARED_DB, "main", table)?;
    }
    stats.copied_local_memory_events += copy_filtered_rows(
        conn,
        "memory_event_log",
        &format!("{ATTACHED_SHARED_DB}.memory_event_log"),
        "main.memory_event_log",
        "scope = 'local'",
    )?;
    stats.copied_local_memory_entries += copy_filtered_rows(
        conn,
        "memory_entry_log",
        &format!("{ATTACHED_SHARED_DB}.memory_entry_log"),
        "main.memory_entry_log",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    for key in LOCAL_SNAPSHOT_KEYS {
        stats.copied_local_snapshots += copy_snapshot_if_present(conn, ATTACHED_SHARED_DB, "main", key)?;
    }
    stats.copied_local_snapshots += copy_split_episodic_snapshot(conn)?;
    for key in LOCAL_METADATA_KEYS {
        stats.copied_local_metadata += copy_metadata_if_present(conn, ATTACHED_SHARED_DB, "main", key)?;
    }
    stats.copied_local_metadata += copy_metadata_if_present(
        conn,
        ATTACHED_SHARED_DB,
        "main",
        "revision:workspace",
    )?;
    stats.copied_local_concepts += copy_filtered_rows(
        conn,
        "projection_curated_concept",
        &format!("{ATTACHED_SHARED_DB}.projection_curated_concept"),
        "main.projection_curated_concept",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    stats.copied_local_relations += copy_filtered_rows(
        conn,
        "projection_concept_relation",
        &format!("{ATTACHED_SHARED_DB}.projection_concept_relation"),
        "main.projection_concept_relation",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    Ok(())
}

fn rebuild_local_outcome_projection(
    conn: &rusqlite::Transaction<'_>,
    stats: &mut MigrationStats,
) -> Result<()> {
    let recent = load_recent_shared_outcome_events(conn, LOCAL_OUTCOME_PROJECTION_LIMIT)?;
    stats.rebuilt_local_outcome_projection_rows += append_local_projection_tx(conn, &recent)?;
    Ok(())
}

fn scrub_shared_local_state(conn: &Connection, stats: &mut MigrationStats) -> Result<()> {
    for table in LOCAL_TABLES {
        stats.scrubbed_local_rows += delete_all_rows(conn, ATTACHED_SHARED_DB, table)?;
    }
    stats.scrubbed_local_memory_events += delete_where(
        conn,
        ATTACHED_SHARED_DB,
        "memory_event_log",
        "scope = 'local'",
    )?;
    stats.scrubbed_local_memory_entries += delete_where(
        conn,
        ATTACHED_SHARED_DB,
        "memory_entry_log",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    for key in LOCAL_SNAPSHOT_KEYS {
        stats.scrubbed_local_snapshots += delete_snapshot_if_present(conn, ATTACHED_SHARED_DB, key)?;
    }
    stats.scrubbed_local_snapshots += scrub_shared_episodic_snapshot(conn)?;
    for key in LOCAL_METADATA_KEYS {
        stats.scrubbed_local_metadata += delete_metadata_if_present(conn, ATTACHED_SHARED_DB, key)?;
    }
    stats.scrubbed_local_concepts += delete_where(
        conn,
        ATTACHED_SHARED_DB,
        "projection_curated_concept",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    stats.scrubbed_local_relations += delete_where(
        conn,
        ATTACHED_SHARED_DB,
        "projection_concept_relation",
        "json_extract(payload, '$.scope') = 'local'",
    )?;
    Ok(())
}

fn database_has_local_semantic_state(conn: &Connection, db: &str) -> Result<bool> {
    for table in LOCAL_TABLES {
        if table_has_rows(conn, db, table)? {
            return Ok(true);
        }
    }
    if table_has_rows_where(conn, db, "memory_event_log", "scope = 'local'")?
        || table_has_rows_where(
            conn,
            db,
            "memory_entry_log",
            "json_extract(payload, '$.scope') = 'local'",
        )?
        || table_has_rows_where(
            conn,
            db,
            "projection_curated_concept",
            "json_extract(payload, '$.scope') = 'local'",
        )?
        || table_has_rows_where(
            conn,
            db,
            "projection_concept_relation",
            "json_extract(payload, '$.scope') = 'local'",
        )?
    {
        return Ok(true);
    }
    for key in LOCAL_SNAPSHOT_KEYS {
        if snapshot_exists(conn, db, key)? {
            return Ok(true);
        }
    }
    if let Some(snapshot) = load_snapshot::<EpisodicMemorySnapshot>(conn, db, "episodic")? {
        if snapshot.entries.iter().any(|entry| entry.scope == MemoryScope::Local) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn database_has_outcome_state(conn: &Connection, db: &str) -> Result<bool> {
    Ok(table_has_rows(conn, db, "outcome_event_log")? || snapshot_exists(conn, db, "outcomes")?)
}

fn database_has_local_outcome_projection(conn: &Connection, db: &str) -> Result<bool> {
    Ok(table_has_rows(conn, db, "outcome_event_local")?
        || table_has_rows(conn, db, "outcome_event_anchor")?)
}

fn copy_split_episodic_snapshot(conn: &Connection) -> Result<usize> {
    let Some(snapshot) = load_snapshot::<EpisodicMemorySnapshot>(conn, ATTACHED_SHARED_DB, "episodic")?
    else {
        return Ok(0);
    };
    let local_entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| entry.scope == MemoryScope::Local)
        .collect::<Vec<_>>();
    if local_entries.is_empty() {
        return Ok(0);
    }
    save_snapshot(
        conn,
        "main",
        "episodic",
        &EpisodicMemorySnapshot { entries: local_entries },
    )?;
    Ok(1)
}

fn load_recent_shared_outcome_events(
    conn: &rusqlite::Transaction<'_>,
    limit: usize,
) -> Result<Vec<OutcomeEvent>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT payload FROM {ATTACHED_SHARED_DB}.outcome_event_log
         ORDER BY ts DESC, sequence DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![i64::try_from(limit)?], |row| {
        row.get::<_, String>(0)
    })?;
    let mut events = Vec::new();
    for row in rows {
        events.push(
            serde_json::from_str::<OutcomeEvent>(&row?)
                .context("failed to decode shared outcome event payload during migration")?,
        );
    }
    Ok(events)
}

fn scrub_shared_episodic_snapshot(conn: &Connection) -> Result<usize> {
    let Some(snapshot) = load_snapshot::<EpisodicMemorySnapshot>(conn, ATTACHED_SHARED_DB, "episodic")?
    else {
        return Ok(0);
    };
    let shared_entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| entry.scope != MemoryScope::Local)
        .collect::<Vec<_>>();
    if shared_entries.is_empty() {
        delete_snapshot_if_present(conn, ATTACHED_SHARED_DB, "episodic")
    } else {
        save_snapshot(
            conn,
            ATTACHED_SHARED_DB,
            "episodic",
            &EpisodicMemorySnapshot {
                entries: shared_entries,
            },
        )?;
        Ok(1)
    }
}

fn copy_table(conn: &Connection, source_db: &str, target_db: &str, table: &str) -> Result<usize> {
    let sql = format!("INSERT INTO {target_db}.{table} SELECT * FROM {source_db}.{table}");
    conn.execute(&sql, []).map_err(Into::into)
}

fn copy_filtered_rows(
    conn: &Connection,
    table: &str,
    source: &str,
    target: &str,
    predicate: &str,
) -> Result<usize> {
    let sql = format!("INSERT INTO {target} SELECT * FROM {source} WHERE {predicate}");
    conn.execute(&sql, []).with_context(|| format!("failed to copy filtered rows for {table}"))
}

fn delete_all_rows(conn: &Connection, db: &str, table: &str) -> Result<usize> {
    conn.execute(&format!("DELETE FROM {db}.{table}"), [])
        .with_context(|| format!("failed to delete migrated rows from {db}.{table}"))
}

fn delete_where(conn: &Connection, db: &str, table: &str, predicate: &str) -> Result<usize> {
    conn.execute(&format!("DELETE FROM {db}.{table} WHERE {predicate}"), [])
        .with_context(|| format!("failed to delete filtered rows from {db}.{table}"))
}

fn table_has_rows(conn: &Connection, db: &str, table: &str) -> Result<bool> {
    table_has_rows_where(conn, db, table, "1 = 1")
}

fn table_has_rows_where(conn: &Connection, db: &str, table: &str, predicate: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {db}.{table} WHERE {predicate} LIMIT 1");
    Ok(conn
        .query_row(&sql, [], |row| row.get::<_, i64>(0))
        .optional()?
        .is_some())
}

fn snapshot_exists(conn: &Connection, db: &str, key: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {db}.snapshots WHERE key = ?1 LIMIT 1");
    Ok(conn
        .query_row(&sql, params![key], |row| row.get::<_, i64>(0))
        .optional()?
        .is_some())
}

fn copy_snapshot_if_present(
    conn: &Connection,
    source_db: &str,
    target_db: &str,
    key: &str,
) -> Result<usize> {
    let Some(raw) = load_snapshot_raw(conn, source_db, key)? else {
        return Ok(0);
    };
    let sql = format!(
        "INSERT INTO {target_db}.snapshots(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    );
    conn.execute(&sql, params![key, raw])?;
    Ok(1)
}

fn delete_snapshot_if_present(conn: &Connection, db: &str, key: &str) -> Result<usize> {
    let sql = format!("DELETE FROM {db}.snapshots WHERE key = ?1");
    Ok(conn.execute(&sql, params![key])?)
}

fn load_snapshot_raw(conn: &Connection, db: &str, key: &str) -> Result<Option<String>> {
    let sql = format!("SELECT value FROM {db}.snapshots WHERE key = ?1");
    conn.query_row(&sql, params![key], |row| row.get::<_, String>(0))
        .optional()
        .map_err(Into::into)
}

fn load_snapshot<T>(conn: &Connection, db: &str, key: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    load_snapshot_raw(conn, db, key)?
        .map(|raw| {
            serde_json::from_str(&raw)
                .with_context(|| format!("failed to decode snapshot `{key}` from {db}"))
        })
        .transpose()
}

fn save_snapshot<T>(conn: &Connection, db: &str, key: &str, value: &T) -> Result<()>
where
    T: Serialize,
{
    let sql = format!(
        "INSERT INTO {db}.snapshots(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    );
    conn.execute(&sql, params![key, serde_json::to_string(value)?])?;
    Ok(())
}

fn copy_metadata_if_present(
    conn: &Connection,
    source_db: &str,
    target_db: &str,
    key: &str,
) -> Result<usize> {
    let Some(value) = load_metadata(conn, source_db, key)? else {
        return Ok(0);
    };
    let sql = format!(
        "INSERT INTO {target_db}.metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    );
    conn.execute(&sql, params![key, value])?;
    Ok(1)
}

fn delete_metadata_if_present(conn: &Connection, db: &str, key: &str) -> Result<usize> {
    let sql = format!("DELETE FROM {db}.metadata WHERE key = ?1");
    Ok(conn.execute(&sql, params![key])?)
}

fn load_metadata(conn: &Connection, db: &str, key: &str) -> Result<Option<i64>> {
    let sql = format!("SELECT value FROM {db}.metadata WHERE key = ?1");
    conn.query_row(&sql, params![key], |row| row.get::<_, i64>(0))
        .optional()
        .map_err(Into::into)
}
