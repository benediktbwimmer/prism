use anyhow::{Context, Result};
use prism_ir::{AnchorRef, EventActor, EventId, TaskId};
use prism_memory::{
    OutcomeEvent, OutcomeKind, OutcomeMemorySnapshot, OutcomeRecallQuery, OutcomeResult, TaskReplay,
};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension, Transaction,
};
use serde_json::{Map, Value};

use crate::outcome_projection::{append_only_delta, snapshot_from_events};

use super::snapshots;

const MAX_HOT_PATCH_CHANGED_SYMBOLS: usize = 256;
const PATCH_PAYLOADS_COMPACTED_KEY: &str = "outcomes:hot_patch_payloads_compacted";
const OUTCOME_ANCHOR_INDEX_BACKFILLED_KEY: &str = "outcomes:anchor_index_backfilled";
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

pub(super) fn load_task_replay(conn: &Connection, task_id: &TaskId) -> Result<TaskReplay> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM outcome_event_log
         WHERE json_extract(payload, '$.meta.correlation') = ?1
         ORDER BY ts DESC, sequence DESC",
    )?;
    let rows = stmt.query_map(params![task_id.0.as_str()], |row| row.get::<_, String>(0))?;
    let events = decode_event_rows(rows)?;
    Ok(TaskReplay {
        task: task_id.clone(),
        events,
    })
}

pub(super) fn load_recent_snapshot(
    conn: &Connection,
    limit: usize,
) -> Result<Option<OutcomeMemorySnapshot>> {
    if limit == 0 {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT payload FROM outcome_event_log
         ORDER BY ts DESC, sequence DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![i64::try_from(limit)?], |row| {
        row.get::<_, String>(0)
    })?;
    Ok(snapshot_from_events(decode_event_rows(rows)?))
}

pub(super) fn load_outcomes(
    conn: &Connection,
    query: &OutcomeRecallQuery,
) -> Result<Vec<OutcomeEvent>> {
    let mut sql = String::from("SELECT o.payload FROM outcome_event_log o");
    let mut params = Vec::<SqlValue>::new();
    if !query.anchors.is_empty() {
        sql.push_str(" JOIN outcome_event_anchor a ON a.event_id = o.event_id WHERE (");
        for (index, anchor) in query.anchors.iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("(a.anchor_kind = ? AND a.anchor_value = ?)");
            let (kind, value) = anchor_key(anchor);
            params.push(SqlValue::from(kind.to_string()));
            params.push(SqlValue::from(value));
        }
        sql.push(')');
    } else {
        sql.push_str(" WHERE 1 = 1");
    }
    if let Some(task) = query.task.as_ref() {
        sql.push_str(" AND json_extract(o.payload, '$.meta.correlation') = ?");
        params.push(SqlValue::from(task.0.to_string()));
    }
    if let Some(kinds) = query.kinds.as_ref() {
        sql.push_str(" AND json_extract(o.payload, '$.kind') IN (");
        for (index, kind) in kinds.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
            params.push(SqlValue::from(outcome_kind_key(kind.clone())));
        }
        sql.push(')');
    }
    if let Some(result) = query.result {
        sql.push_str(" AND json_extract(o.payload, '$.result') = ?");
        params.push(SqlValue::from(outcome_result_key(result)));
    }
    if let Some(since) = query.since {
        sql.push_str(" AND o.ts >= ?");
        params.push(SqlValue::from(i64::try_from(since)?));
    }
    sql.push_str(" ORDER BY o.ts DESC, o.sequence DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        row.get::<_, String>(0)
    })?;
    let mut seen = std::collections::HashSet::<EventId>::new();
    let mut events = Vec::new();
    for mut event in decode_event_rows(rows)? {
        if !seen.insert(event.meta.id.clone()) {
            continue;
        }
        if query
            .actor
            .as_ref()
            .is_some_and(|actor| actor_key(&event.meta.actor) != actor_key(actor))
        {
            continue;
        }
        compact_hot_patch_metadata(&mut event);
        events.push(event);
        if query.limit > 0 && events.len() >= query.limit {
            break;
        }
    }
    Ok(events)
}

pub(super) fn load_event(conn: &Connection, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
    let raw = conn
        .query_row(
            "SELECT payload FROM outcome_event_log WHERE event_id = ?1",
            params![event_id.0.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|raw| {
        let mut event = serde_json::from_str::<OutcomeEvent>(&raw)
            .with_context(|| "failed to decode outcome event payload from sqlite")?;
        compact_hot_patch_metadata(&mut event);
        Ok(event)
    })
    .transpose()
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
        append_anchor_rows_tx(tx, event)?;
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

pub(super) fn backfill_anchor_index_if_needed(conn: &Connection) -> Result<()> {
    if metadata_value(conn, OUTCOME_ANCHOR_INDEX_BACKFILLED_KEY)?.is_some() {
        return Ok(());
    }
    if !table_exists(conn, "outcome_event_log")? {
        set_metadata_value(conn, OUTCOME_ANCHOR_INDEX_BACKFILLED_KEY, 1)?;
        return Ok(());
    }

    let mut rows = Vec::<(String, String)>::new();
    {
        let mut stmt =
            conn.prepare("SELECT event_id, payload FROM outcome_event_log ORDER BY sequence ASC")?;
        let mapped = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in mapped {
            rows.push(row?);
        }
    }

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR IGNORE INTO outcome_event_anchor(event_id, anchor_kind, anchor_value)
             VALUES (?1, ?2, ?3)",
        )?;
        for (event_id, raw) in rows {
            let event = serde_json::from_str::<OutcomeEvent>(&raw).with_context(|| {
                "failed to decode outcome event payload from sqlite during anchor index backfill"
            })?;
            for anchor in &event.anchors {
                let (kind, value) = anchor_key(anchor);
                stmt.execute(params![event_id, kind, value])?;
            }
        }
    }
    set_metadata_value_tx(&tx, OUTCOME_ANCHOR_INDEX_BACKFILLED_KEY, 1)?;
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

fn append_anchor_rows_tx(tx: &Transaction<'_>, event: &OutcomeEvent) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO outcome_event_anchor(event_id, anchor_kind, anchor_value)
         VALUES (?1, ?2, ?3)",
    )?;
    for anchor in &event.anchors {
        let (kind, value) = anchor_key(anchor);
        stmt.execute(params![event.meta.id.0.as_str(), kind, value])?;
    }
    Ok(())
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

fn anchor_key(anchor: &AnchorRef) -> (&'static str, String) {
    match anchor {
        AnchorRef::Node(node) => (
            "node",
            format!("{}:{}:{}", node.crate_name, node.path, node.kind),
        ),
        AnchorRef::Lineage(lineage) => ("lineage", lineage.0.to_string()),
        AnchorRef::File(file) => ("file", file.0.to_string()),
        AnchorRef::Kind(kind) => ("kind", kind.to_string()),
    }
}

fn outcome_kind_key(kind: OutcomeKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn outcome_result_key(result: OutcomeResult) -> String {
    serde_json::to_value(result)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn actor_key(actor: &EventActor) -> String {
    match actor {
        EventActor::User => "User".to_string(),
        EventActor::Agent => "Agent".to_string(),
        EventActor::System => "System".to_string(),
        EventActor::CI => "CI".to_string(),
        EventActor::GitAuthor { name, email } => {
            format!("GitAuthor:{}:{}", name, email.as_deref().unwrap_or(""))
        }
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
