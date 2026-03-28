use anyhow::{Context, Result};
use prism_memory::{EpisodicMemorySnapshot, MemoryEntry};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::memory_projection::{append_only_delta, latest_snapshot, merge_snapshot};

use super::snapshots;

pub(super) fn load_snapshot(conn: &Connection) -> Result<Option<EpisodicMemorySnapshot>> {
    let entries = load_entries(conn)?;
    if !entries.is_empty() {
        return Ok(latest_snapshot(entries));
    }
    snapshots::load_snapshot_row(conn, "episodic")
}

pub(super) fn save_snapshot_tx(
    tx: &Transaction<'_>,
    snapshot: &EpisodicMemorySnapshot,
) -> Result<bool> {
    let current = load_snapshot_tx(tx)?;
    let delta = append_only_delta(current.as_ref(), snapshot);
    if delta.is_empty() {
        if let Some(merged) = current {
            snapshots::save_snapshot_row_tx(tx, "episodic", &merged)?;
        }
        return Ok(false);
    }

    for entry in &delta {
        tx.execute(
            "INSERT INTO memory_entry_log(memory_id, payload) VALUES (?1, ?2)",
            params![entry.id.0, serde_json::to_string(entry)?],
        )?;
    }

    if let Some(merged) = merge_snapshot(current, snapshot) {
        snapshots::save_snapshot_row_tx(tx, "episodic", &merged)?;
    }
    Ok(true)
}

pub(super) fn backfill_from_snapshot_if_needed(conn: &Connection) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row("SELECT 1 FROM memory_entry_log LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let Some(snapshot) = snapshots::load_snapshot_row::<EpisodicMemorySnapshot>(conn, "episodic")?
    else {
        return Ok(());
    };

    let tx = conn.unchecked_transaction()?;
    for entry in snapshot.entries {
        tx.execute(
            "INSERT INTO memory_entry_log(memory_id, payload) VALUES (?1, ?2)",
            params![entry.id.0, serde_json::to_string(&entry)?],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn load_snapshot_tx(tx: &Transaction<'_>) -> Result<Option<EpisodicMemorySnapshot>> {
    let entries = load_entries_tx(tx)?;
    if !entries.is_empty() {
        return Ok(latest_snapshot(entries));
    }

    let raw = tx
        .query_row(
            "SELECT value FROM snapshots WHERE key = 'episodic'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|value| {
        serde_json::from_str(&value)
            .with_context(|| "failed to decode snapshot `episodic` from sqlite transaction")
    })
    .transpose()
    .map_err(Into::into)
}

fn load_entries(conn: &Connection) -> Result<Vec<MemoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM memory_entry_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_rows(rows)
}

fn load_entries_tx(tx: &Transaction<'_>) -> Result<Vec<MemoryEntry>> {
    let mut stmt = tx.prepare(
        "SELECT payload FROM memory_entry_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_rows(rows)
}

fn decode_rows<'stmt, F>(rows: rusqlite::MappedRows<'stmt, F>) -> Result<Vec<MemoryEntry>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>,
{
    let mut entries = Vec::new();
    for raw in rows {
        let raw = raw?;
        let entry = serde_json::from_str::<MemoryEntry>(&raw)
            .with_context(|| "failed to decode memory entry payload from sqlite")?;
        entries.push(entry);
    }
    Ok(entries)
}
