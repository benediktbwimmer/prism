use anyhow::{Context, Result};
use prism_agent::{InferenceSnapshot, InferredEdgeRecord};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::snapshots;

pub(super) fn load_snapshot(conn: &Connection) -> Result<Option<InferenceSnapshot>> {
    let records = load_records(conn)?;
    if !records.is_empty() {
        return Ok(Some(InferenceSnapshot { records }));
    }
    snapshots::load_snapshot_row(conn, "inference")
}

pub(super) fn save_snapshot_tx(tx: &Transaction<'_>, snapshot: &InferenceSnapshot) -> Result<bool> {
    let mut changed = false;
    let edge_ids = snapshot
        .records
        .iter()
        .map(|record| record.id.0.as_str())
        .collect::<Vec<_>>();
    for record in &snapshot.records {
        changed |= tx.execute(
            "INSERT INTO inference_record_log(edge_id, payload)
             VALUES (?1, ?2)
             ON CONFLICT(edge_id) DO UPDATE SET payload = excluded.payload",
            params![record.id.0.as_str(), serde_json::to_string(record)?],
        )? > 0;
    }
    if edge_ids.is_empty() {
        changed |= tx.execute("DELETE FROM inference_record_log", [])? > 0;
    } else {
        let placeholders = std::iter::repeat("?")
            .take(edge_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM inference_record_log WHERE edge_id NOT IN ({placeholders})");
        changed |= tx.execute(&sql, rusqlite::params_from_iter(edge_ids.iter().copied()))? > 0;
    }
    Ok(changed)
}

pub(super) fn append_records_tx(
    tx: &Transaction<'_>,
    records: &[InferredEdgeRecord],
) -> Result<usize> {
    let mut inserted = 0;
    for record in records {
        inserted += tx.execute(
            "INSERT OR IGNORE INTO inference_record_log(edge_id, payload)
             VALUES (?1, ?2)",
            params![record.id.0.as_str(), serde_json::to_string(record)?],
        )?;
    }
    Ok(inserted)
}

pub(super) fn backfill_record_log_if_needed(conn: &Connection) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row("SELECT 1 FROM inference_record_log LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let Some(snapshot) = snapshots::load_snapshot_row::<InferenceSnapshot>(conn, "inference")?
    else {
        return Ok(());
    };

    let tx = conn.unchecked_transaction()?;
    append_records_tx(&tx, &snapshot.records)?;
    tx.commit()?;
    Ok(())
}

fn load_records(conn: &Connection) -> Result<Vec<InferredEdgeRecord>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM inference_record_log
         ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    decode_record_rows(rows)
}

fn decode_record_rows<'stmt, F>(
    rows: rusqlite::MappedRows<'stmt, F>,
) -> Result<Vec<InferredEdgeRecord>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>,
{
    let mut records = Vec::new();
    for raw in rows {
        let raw = raw?;
        let record = serde_json::from_str::<InferredEdgeRecord>(&raw)
            .with_context(|| "failed to decode inference record payload from sqlite")?;
        records.push(record);
    }
    Ok(records)
}
