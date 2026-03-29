use std::collections::HashSet;

use anyhow::Result;
use prism_coordination::CoordinationEvent;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(super) fn load_events(conn: &Connection) -> Result<Vec<CoordinationEvent>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM coordination_event_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut events = Vec::new();
    for row in rows {
        let payload = row?;
        events.push(serde_json::from_str(&payload)?);
    }
    Ok(events)
}

pub(super) fn event_ids_exist_tx(
    tx: &Transaction<'_>,
    event_ids: &[String],
) -> Result<HashSet<String>> {
    if event_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let mut existing = HashSet::new();
    let mut stmt = tx.prepare(
        "SELECT event_id FROM coordination_event_log
         WHERE event_id = ?1",
    )?;
    for event_id in event_ids {
        if let Some(found) = stmt
            .query_row(params![event_id], |row| row.get::<_, String>(0))
            .optional()?
        {
            existing.insert(found);
        }
    }
    Ok(existing)
}

pub(super) fn append_events_tx(
    tx: &Transaction<'_>,
    events: &[CoordinationEvent],
) -> Result<usize> {
    let event_ids = events
        .iter()
        .map(|event| event.meta.id.0.to_string())
        .collect::<Vec<_>>();
    let existing = event_ids_exist_tx(tx, &event_ids)?;
    let mut inserted = 0;
    for event in events {
        if existing.contains(event.meta.id.0.as_str()) {
            continue;
        }
        tx.execute(
            "INSERT INTO coordination_event_log(event_id, ts, payload)
             VALUES (?1, ?2, ?3)",
            params![
                event.meta.id.0.as_str(),
                event.meta.ts as i64,
                serde_json::to_string(event)?,
            ],
        )?;
        inserted += 1;
    }
    Ok(inserted)
}
