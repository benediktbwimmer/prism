use anyhow::{Context, Result};
use prism_memory::{OutcomeEvent, OutcomeMemorySnapshot};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::outcome_projection::{append_only_delta, snapshot_from_events};

use super::snapshots;

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
        let event = serde_json::from_str::<OutcomeEvent>(&raw)
            .with_context(|| "failed to decode outcome event payload from sqlite")?;
        events.push(event);
    }
    Ok(events)
}
