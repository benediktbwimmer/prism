use anyhow::{Context, Result};
use prism_memory::{OutcomeEvent, OutcomeKind, OutcomeMemorySnapshot};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{Map, Value};

use crate::outcome_projection::{append_only_delta, snapshot_from_events};

use super::snapshots;

const MAX_HOT_PATCH_CHANGED_SYMBOLS: usize = 256;
const PATCH_PAYLOADS_COMPACTED_KEY: &str = "outcomes:hot_patch_payloads_compacted";
const MIN_VACUUM_RECLAIM_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct PatchPayloadCompaction {
    pub updated_rows: usize,
    pub reclaimed_bytes_before_vacuum: u64,
    pub vacuumed: bool,
}

pub(super) fn load_snapshot(conn: &Connection) -> Result<Option<OutcomeMemorySnapshot>> {
    let events = load_events(conn)?;
    if !events.is_empty() {
        return Ok(snapshot_from_events(events));
    }
    snapshots::load_snapshot_row(conn, "outcomes")
}

pub(super) fn save_snapshot_tx(
    tx: &Transaction<'_>,
    snapshot: &OutcomeMemorySnapshot,
) -> Result<bool> {
    let current = load_snapshot_tx(tx)?;
    let delta = append_only_delta(current.as_ref(), snapshot);
    if delta.is_empty() {
        return Ok(false);
    }
    Ok(append_events_tx(tx, &delta)? > 0)
}

pub(super) fn append_events_tx(tx: &Transaction<'_>, events: &[OutcomeEvent]) -> Result<usize> {
    let mut inserted = 0;
    for event in events {
        let ts = i64::try_from(event.meta.ts)
            .with_context(|| "outcome event timestamp exceeds sqlite integer range")?;
        inserted += tx.execute(
            "INSERT OR IGNORE INTO outcome_event_log(event_id, ts, payload)
             VALUES (?1, ?2, ?3)",
            params![
                event.meta.id.0.to_string(),
                ts,
                serde_json::to_string(event)?
            ],
        )?;
    }
    Ok(inserted)
}

pub(super) fn backfill_event_log_if_needed(conn: &Connection) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row("SELECT 1 FROM outcome_event_log LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let Some(snapshot) = snapshots::load_snapshot_row::<OutcomeMemorySnapshot>(conn, "outcomes")?
    else {
        return Ok(());
    };

    let tx = conn.unchecked_transaction()?;
    append_events_tx(&tx, &snapshot.events)?;
    tx.commit()?;
    Ok(())
}

pub(super) fn compact_hot_patch_payloads_on_open(
    conn: &mut Connection,
) -> Result<PatchPayloadCompaction> {
    if metadata_value(conn, PATCH_PAYLOADS_COMPACTED_KEY)?.is_some() {
        return Ok(PatchPayloadCompaction::default());
    }
    if !table_exists(conn, "outcome_event_log")? {
        set_metadata_value(conn, PATCH_PAYLOADS_COMPACTED_KEY, 1)?;
        return Ok(PatchPayloadCompaction::default());
    }

    let mut updates = Vec::<(i64, String)>::new();
    {
        let mut stmt =
            conn.prepare("SELECT sequence, payload FROM outcome_event_log ORDER BY sequence ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (sequence, raw) = row?;
            let mut event = serde_json::from_str::<OutcomeEvent>(&raw).with_context(|| {
                "failed to decode outcome event payload from sqlite during compaction"
            })?;
            compact_hot_patch_metadata(&mut event);
            let compacted = serde_json::to_string(&event)?;
            if compacted != raw {
                updates.push((sequence, compacted));
            }
        }
    }

    {
        let tx = conn.transaction()?;
        if !updates.is_empty() {
            let mut stmt =
                tx.prepare_cached("UPDATE outcome_event_log SET payload = ?2 WHERE sequence = ?1")?;
            for (sequence, payload) in &updates {
                stmt.execute(params![sequence, payload])?;
            }
        }
        set_metadata_value_tx(&tx, PATCH_PAYLOADS_COMPACTED_KEY, 1)?;
        tx.commit()?;
    }

    let page_size = conn.pragma_query_value(None, "page_size", |row| row.get::<_, i64>(0))? as u64;
    let freelist_count =
        conn.pragma_query_value(None, "freelist_count", |row| row.get::<_, i64>(0))? as u64;
    let reclaimed_bytes_before_vacuum = page_size.saturating_mul(freelist_count);
    let vacuumed = reclaimed_bytes_before_vacuum >= MIN_VACUUM_RECLAIM_BYTES;
    if vacuumed {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    }

    Ok(PatchPayloadCompaction {
        updated_rows: updates.len(),
        reclaimed_bytes_before_vacuum,
        vacuumed,
    })
}

fn load_snapshot_tx(tx: &Transaction<'_>) -> Result<Option<OutcomeMemorySnapshot>> {
    let events = load_events_tx(tx)?;
    if !events.is_empty() {
        return Ok(snapshot_from_events(events));
    }

    let raw = tx
        .query_row(
            "SELECT value FROM snapshots WHERE key = 'outcomes'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|value| {
        serde_json::from_str(&value)
            .with_context(|| "failed to decode snapshot `outcomes` from sqlite transaction")
    })
    .transpose()
    .map_err(Into::into)
}

fn load_events(conn: &Connection) -> Result<Vec<OutcomeEvent>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM outcome_event_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_event_rows(rows)
}

fn load_events_tx(tx: &Transaction<'_>) -> Result<Vec<OutcomeEvent>> {
    let mut stmt = tx.prepare(
        "SELECT payload FROM outcome_event_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_event_rows(rows)
}

fn decode_event_rows<'stmt, F>(rows: rusqlite::MappedRows<'stmt, F>) -> Result<Vec<OutcomeEvent>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>,
{
    let mut events = Vec::new();
    for raw in rows {
        let raw = raw?;
        let mut event = serde_json::from_str::<OutcomeEvent>(&raw)
            .with_context(|| "failed to decode outcome event payload from sqlite")?;
        compact_hot_patch_metadata(&mut event);
        events.push(event);
    }
    Ok(events)
}

fn compact_hot_patch_metadata(event: &mut OutcomeEvent) {
    if event.kind != OutcomeKind::PatchApplied {
        return;
    }
    let Some(metadata) = event.metadata.as_object_mut() else {
        return;
    };
    let Some(changed_symbols) = metadata.get("changedSymbols").and_then(Value::as_array) else {
        return;
    };

    let total_count = changed_symbols.len();
    let summary_values = changed_file_summary_values(changed_symbols);
    metadata
        .entry("changedSymbolsTotalCount".to_string())
        .or_insert_with(|| Value::from(total_count as u64));
    metadata
        .entry("changedSymbolsTruncated".to_string())
        .or_insert_with(|| Value::Bool(total_count > MAX_HOT_PATCH_CHANGED_SYMBOLS));
    metadata
        .entry("changedFilesSummary".to_string())
        .or_insert_with(|| Value::Array(summary_values));

    if let Some(changed_symbols) = metadata
        .get_mut("changedSymbols")
        .and_then(Value::as_array_mut)
        .filter(|changed_symbols| changed_symbols.len() > MAX_HOT_PATCH_CHANGED_SYMBOLS)
    {
        changed_symbols.truncate(MAX_HOT_PATCH_CHANGED_SYMBOLS);
        metadata.insert("changedSymbolsTruncated".to_string(), Value::Bool(true));
    }
}

fn changed_file_summary_values(changed_symbols: &[Value]) -> Vec<Value> {
    let mut by_path = std::collections::BTreeMap::<String, ChangedFileSummary>::new();
    for symbol in changed_symbols {
        let Some(file_path) = symbol
            .get("filePath")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        let summary = by_path
            .entry(file_path.clone())
            .or_insert_with(|| ChangedFileSummary::new(file_path));
        summary.changed_symbol_count += 1;
        match symbol
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "added" => summary.added_count += 1,
            "removed" => summary.removed_count += 1,
            _ => summary.updated_count += 1,
        }
    }
    by_path
        .into_values()
        .map(ChangedFileSummary::into_json)
        .collect()
}

struct ChangedFileSummary {
    file_path: String,
    changed_symbol_count: usize,
    added_count: usize,
    removed_count: usize,
    updated_count: usize,
}

impl ChangedFileSummary {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            changed_symbol_count: 0,
            added_count: 0,
            removed_count: 0,
            updated_count: 0,
        }
    }

    fn into_json(self) -> Value {
        let mut object = Map::new();
        object.insert("filePath".to_string(), Value::String(self.file_path));
        object.insert(
            "changedSymbolCount".to_string(),
            Value::from(self.changed_symbol_count as u64),
        );
        object.insert(
            "addedCount".to_string(),
            Value::from(self.added_count as u64),
        );
        object.insert(
            "removedCount".to_string(),
            Value::from(self.removed_count as u64),
        );
        object.insert(
            "updatedCount".to_string(),
            Value::from(self.updated_count as u64),
        );
        Value::Object(object)
    }
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

fn metadata_value(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let value = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(value.map(|value| value as u64))
}

fn set_metadata_value(conn: &Connection, key: &str, value: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value as i64],
    )?;
    Ok(())
}

fn set_metadata_value_tx(tx: &Transaction<'_>, key: &str, value: u64) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value as i64],
    )?;
    Ok(())
}
