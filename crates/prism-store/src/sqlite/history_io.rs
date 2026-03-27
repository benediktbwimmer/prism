use anyhow::{Context, Result};
use prism_history::{HistoryPersistDelta, HistorySnapshot, LineageTombstone};
use prism_ir::{LineageEvent, LineageId, NodeId};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::sqlite::codecs::{decode_node_kind, encode_node_kind};

const HISTORY_NEXT_LINEAGE_KEY: &str = "history:next_lineage";
const HISTORY_NEXT_EVENT_KEY: &str = "history:next_event";

pub(super) fn load_history_snapshot(conn: &Connection) -> Result<Option<HistorySnapshot>> {
    if !history_state_present(conn)? {
        return Ok(None);
    }

    let mut node_to_lineage = Vec::<(NodeId, LineageId)>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT node_crate_name, node_path, node_kind, lineage
             FROM history_node_lineages
             ORDER BY node_crate_name, node_path, node_kind",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                NodeId::new(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    decode_node_kind(row.get::<_, i64>(2)?)?,
                ),
                LineageId::new(row.get::<_, String>(3)?),
            ))
        })?;
        for row in rows {
            node_to_lineage.push(row?);
        }
    }

    let mut events = Vec::<LineageEvent>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM history_events
             ORDER BY ts, event_id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let payload = row?;
            events.push(
                serde_json::from_str(&payload)
                    .context("failed to decode persisted history event from sqlite")?,
            );
        }
    }

    let mut co_change_counts = Vec::<(LineageId, LineageId, u32)>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT source_lineage, target_lineage, count
             FROM history_co_change
             ORDER BY source_lineage, target_lineage",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                LineageId::new(row.get::<_, String>(0)?),
                LineageId::new(row.get::<_, String>(1)?),
                row.get::<_, u32>(2)?,
            ))
        })?;
        for row in rows {
            co_change_counts.push(row?);
        }
    }

    let mut tombstones = Vec::<LineageTombstone>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM history_tombstones
             ORDER BY lineage",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let payload = row?;
            tombstones.push(
                serde_json::from_str(&payload)
                    .context("failed to decode persisted history tombstone from sqlite")?,
            );
        }
    }

    Ok(Some(HistorySnapshot {
        node_to_lineage,
        events,
        co_change_counts,
        tombstones,
        next_lineage: metadata_value(conn, HISTORY_NEXT_LINEAGE_KEY)?.unwrap_or_default(),
        next_event: metadata_value(conn, HISTORY_NEXT_EVENT_KEY)?.unwrap_or_default(),
    }))
}

pub(super) fn replace_history_snapshot_tx(
    tx: &Transaction<'_>,
    snapshot: &HistorySnapshot,
) -> Result<()> {
    clear_history_tables_tx(tx)?;

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_node_lineages(
                 node_crate_name,
                 node_path,
                 node_kind,
                 lineage
             ) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (node, lineage) in &snapshot.node_to_lineage {
            stmt.execute(params![
                node.crate_name.as_str(),
                node.path.as_str(),
                encode_node_kind(node.kind),
                lineage.0.as_str(),
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_events(event_id, ts, lineage, payload)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for event in &snapshot.events {
            stmt.execute(params![
                event.meta.id.0.as_str(),
                event.meta.ts as i64,
                event.lineage.0.as_str(),
                serde_json::to_string(event)?,
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_co_change(source_lineage, target_lineage, count)
             VALUES (?1, ?2, ?3)",
        )?;
        for (source, target, count) in &snapshot.co_change_counts {
            stmt.execute(params![source.0.as_str(), target.0.as_str(), count])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_tombstones(lineage, payload)
             VALUES (?1, ?2)",
        )?;
        for tombstone in &snapshot.tombstones {
            stmt.execute(params![
                tombstone.lineage.0.as_str(),
                serde_json::to_string(tombstone)?,
            ])?;
        }
    }

    set_metadata_value_tx(tx, HISTORY_NEXT_LINEAGE_KEY, snapshot.next_lineage)?;
    set_metadata_value_tx(tx, HISTORY_NEXT_EVENT_KEY, snapshot.next_event)?;
    delete_legacy_history_snapshot_tx(tx)?;
    Ok(())
}

pub(super) fn apply_history_delta_tx(
    tx: &Transaction<'_>,
    delta: &HistoryPersistDelta,
) -> Result<()> {
    {
        let mut stmt = tx.prepare_cached(
            "DELETE FROM history_node_lineages
             WHERE node_crate_name = ?1 AND node_path = ?2 AND node_kind = ?3",
        )?;
        for node in &delta.removed_nodes {
            stmt.execute(params![
                node.crate_name.as_str(),
                node.path.as_str(),
                encode_node_kind(node.kind),
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_node_lineages(
                 node_crate_name,
                 node_path,
                 node_kind,
                 lineage
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(node_crate_name, node_path, node_kind)
             DO UPDATE SET lineage = excluded.lineage",
        )?;
        for (node, lineage) in &delta.upserted_node_lineages {
            stmt.execute(params![
                node.crate_name.as_str(),
                node.path.as_str(),
                encode_node_kind(node.kind),
                lineage.0.as_str(),
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_events(event_id, ts, lineage, payload)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(event_id) DO NOTHING",
        )?;
        for event in &delta.appended_events {
            stmt.execute(params![
                event.meta.id.0.as_str(),
                event.meta.ts as i64,
                event.lineage.0.as_str(),
                serde_json::to_string(event)?,
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_co_change(source_lineage, target_lineage, count)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(source_lineage, target_lineage)
             DO UPDATE SET count = history_co_change.count + excluded.count",
        )?;
        for delta in &delta.co_change_deltas {
            stmt.execute(params![
                delta.source_lineage.0.as_str(),
                delta.target_lineage.0.as_str(),
                delta.count_delta,
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "DELETE FROM history_tombstones
             WHERE lineage = ?1",
        )?;
        for lineage in &delta.removed_tombstone_lineages {
            stmt.execute(params![lineage.0.as_str()])?;
        }
    }

    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO history_tombstones(lineage, payload)
             VALUES (?1, ?2)
             ON CONFLICT(lineage)
             DO UPDATE SET payload = excluded.payload",
        )?;
        for tombstone in &delta.upserted_tombstones {
            stmt.execute(params![
                tombstone.lineage.0.as_str(),
                serde_json::to_string(tombstone)?,
            ])?;
        }
    }

    set_metadata_value_tx(tx, HISTORY_NEXT_LINEAGE_KEY, delta.next_lineage)?;
    set_metadata_value_tx(tx, HISTORY_NEXT_EVENT_KEY, delta.next_event)?;
    delete_legacy_history_snapshot_tx(tx)?;
    Ok(())
}

fn history_state_present(conn: &Connection) -> Result<bool> {
    if metadata_value(conn, HISTORY_NEXT_LINEAGE_KEY)?.is_some()
        || metadata_value(conn, HISTORY_NEXT_EVENT_KEY)?.is_some()
    {
        return Ok(true);
    }
    for table in [
        "history_node_lineages",
        "history_events",
        "history_co_change",
        "history_tombstones",
    ] {
        let query = format!("SELECT 1 FROM {table} LIMIT 1");
        let exists = conn
            .query_row(&query, [], |row| row.get::<_, i64>(0))
            .optional()?
            .is_some();
        if exists {
            return Ok(true);
        }
    }
    Ok(false)
}

fn clear_history_tables_tx(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM history_node_lineages", [])?;
    tx.execute("DELETE FROM history_events", [])?;
    tx.execute("DELETE FROM history_co_change", [])?;
    tx.execute("DELETE FROM history_tombstones", [])?;
    Ok(())
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

fn set_metadata_value_tx(tx: &Transaction<'_>, key: &str, value: u64) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value as i64],
    )?;
    Ok(())
}

fn delete_legacy_history_snapshot_tx(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM snapshots WHERE key = 'history'", [])?;
    Ok(())
}
