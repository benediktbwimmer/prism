use anyhow::{Context, Result};
use prism_ir::new_prefixed_id;
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind, MemoryScope,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::memory_projection::{append_only_delta, latest_snapshot, snapshot_from_events};

use super::snapshots;

pub(super) fn load_snapshot(conn: &Connection) -> Result<Option<EpisodicMemorySnapshot>> {
    let events = load_events(conn)?;
    if !events.is_empty() {
        return Ok(snapshot_from_events(events));
    }
    let entries = load_entries(conn)?;
    if !entries.is_empty() {
        return Ok(latest_snapshot(entries));
    }
    snapshots::load_snapshot_row(conn, "episodic")
}

pub(super) fn load_events(conn: &Connection) -> Result<Vec<MemoryEvent>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM memory_event_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_event_rows(rows)
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

    let events = delta
        .into_iter()
        .map(|entry| {
            MemoryEvent::from_entry(
                MemoryEventKind::Stored,
                entry,
                extract_task_id(snapshot),
                Vec::new(),
                Vec::new(),
            )
        })
        .collect::<Vec<_>>();
    let inserted = append_events_tx(tx, &events)?;
    if inserted == 0 {
        if let Some(merged) = current {
            snapshots::save_snapshot_row_tx(tx, "episodic", &merged)?;
        }
        return Ok(false);
    }
    replace_snapshot_row_from_event_log_tx(tx)?;
    Ok(true)
}

pub(super) fn append_events_tx(tx: &Transaction<'_>, events: &[MemoryEvent]) -> Result<usize> {
    let mut inserted = 0;
    for event in events {
        let recorded_at = i64::try_from(event.recorded_at)
            .with_context(|| "memory event recorded_at exceeds sqlite integer range")?;
        inserted += tx.execute(
            "INSERT OR IGNORE INTO memory_event_log(event_id, memory_id, scope, action, recorded_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.id,
                event.memory_id.0,
                memory_scope_label(event.scope),
                memory_event_action_label(event.action),
                recorded_at,
                serde_json::to_string(event)?,
            ],
        )?;
    }
    if inserted > 0 {
        replace_snapshot_row_from_event_log_tx(tx)?;
    }
    Ok(inserted)
}

pub(super) fn backfill_event_log_if_needed(conn: &Connection) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row("SELECT 1 FROM memory_event_log LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;
    let legacy_entries = load_entries_tx(&tx)?;
    if !legacy_entries.is_empty() {
        for entry in legacy_entries {
            let event = MemoryEvent {
                id: new_prefixed_id("memory-event").to_string(),
                action: MemoryEventKind::Stored,
                memory_id: entry.id.clone(),
                scope: entry.scope,
                entry: Some(entry.clone()),
                recorded_at: entry.created_at,
                task_id: extract_task_id_from_entry(&entry),
                promoted_from: Vec::new(),
                supersedes: Vec::new(),
            };
            append_events_tx(&tx, &[event])?;
        }
        tx.commit()?;
        return Ok(());
    }

    let Some(snapshot) = snapshots::load_snapshot_row::<EpisodicMemorySnapshot>(conn, "episodic")?
    else {
        return Ok(());
    };

    for entry in snapshot.entries {
        let event = MemoryEvent {
            id: new_prefixed_id("memory-event").to_string(),
            action: MemoryEventKind::Stored,
            memory_id: entry.id.clone(),
            scope: entry.scope,
            entry: Some(entry.clone()),
            recorded_at: entry.created_at,
            task_id: extract_task_id_from_entry(&entry),
            promoted_from: Vec::new(),
            supersedes: Vec::new(),
        };
        append_events_tx(&tx, &[event])?;
    }
    tx.commit()?;
    Ok(())
}

fn load_snapshot_tx(tx: &Transaction<'_>) -> Result<Option<EpisodicMemorySnapshot>> {
    let events = load_events_tx(tx)?;
    if !events.is_empty() {
        return Ok(snapshot_from_events(events));
    }

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

fn load_events_tx(tx: &Transaction<'_>) -> Result<Vec<MemoryEvent>> {
    let mut stmt = tx.prepare(
        "SELECT payload FROM memory_event_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_event_rows(rows)
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

fn decode_event_rows<'stmt, F>(rows: rusqlite::MappedRows<'stmt, F>) -> Result<Vec<MemoryEvent>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>,
{
    let mut events = Vec::new();
    for raw in rows {
        let raw = raw?;
        let event = serde_json::from_str::<MemoryEvent>(&raw)
            .with_context(|| "failed to decode memory event payload from sqlite")?;
        events.push(event);
    }
    Ok(events)
}

fn replace_snapshot_row_from_event_log_tx(tx: &Transaction<'_>) -> Result<()> {
    let snapshot = snapshot_from_events(load_events_tx(tx)?);
    if let Some(snapshot) = snapshot {
        snapshots::save_snapshot_row_tx(tx, "episodic", &snapshot)?;
    } else {
        snapshots::delete_snapshot_row_tx(tx, "episodic")?;
    }
    Ok(())
}

fn extract_task_id(snapshot: &EpisodicMemorySnapshot) -> Option<String> {
    snapshot
        .entries
        .iter()
        .rev()
        .find_map(extract_task_id_from_entry)
}

fn extract_task_id_from_entry(entry: &MemoryEntry) -> Option<String> {
    entry
        .metadata
        .get("task_id")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn memory_scope_label(scope: MemoryScope) -> &'static str {
    match scope {
        MemoryScope::Local => "local",
        MemoryScope::Session => "session",
        MemoryScope::Repo => "repo",
    }
}

fn memory_event_action_label(action: MemoryEventKind) -> &'static str {
    match action {
        MemoryEventKind::Stored => "stored",
        MemoryEventKind::Promoted => "promoted",
        MemoryEventKind::Superseded => "superseded",
    }
}
