use anyhow::Result;
use prism_coordination::{CoordinationEvent, CoordinationSnapshot};
use rusqlite::{params, Connection, OptionalExtension};

use crate::store::CoordinationEventStream;

pub(super) fn load_event_stream(conn: &Connection) -> Result<CoordinationEventStream> {
    let compaction = conn
        .query_row(
            "SELECT last_sequence, payload
             FROM coordination_event_compaction
             WHERE id = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let (fallback_snapshot, after_sequence) = match compaction {
        Some((last_sequence, payload)) => (
            Some(serde_json::from_str::<CoordinationSnapshot>(&payload)?),
            last_sequence,
        ),
        None => (None, 0),
    };

    let mut stmt = conn.prepare(
        "SELECT payload FROM coordination_event_log
         WHERE sequence > ?1
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map(params![after_sequence], |row| row.get::<_, String>(0))?;
    let mut suffix_events = Vec::new();
    for row in rows {
        suffix_events.push(serde_json::from_str::<CoordinationEvent>(&row?)?);
    }

    Ok(CoordinationEventStream {
        fallback_snapshot,
        suffix_events,
    })
}

pub(super) fn save_compaction(conn: &Connection, snapshot: &CoordinationSnapshot) -> Result<()> {
    let last_sequence = conn.query_row(
        "SELECT COALESCE(MAX(sequence), 0) FROM coordination_event_log",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    conn.execute(
        "INSERT INTO coordination_event_compaction(id, last_sequence, payload)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
             last_sequence = excluded.last_sequence,
             payload = excluded.payload",
        params![last_sequence, serde_json::to_string(&compacted_snapshot(snapshot))?],
    )?;
    Ok(())
}

fn compacted_snapshot(snapshot: &CoordinationSnapshot) -> CoordinationSnapshot {
    let mut compacted = snapshot.clone();
    compacted.events.clear();
    compacted
}
