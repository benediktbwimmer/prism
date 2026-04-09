use std::collections::BTreeMap;

use anyhow::{Context, Result};
use prism_ir::{AnchorRef, EventActor, EventId};
use prism_memory::{OutcomeEvent, OutcomeKind};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension, Transaction,
};
use serde_json::Value;

use crate::{PatchEventSummary, PatchEventSummaryQuery, PatchFileSummary, PatchFileSummaryQuery};

const PATCH_PROJECTION_BACKFILLED_KEY: &str = "outcomes:patch_projection_backfilled";

#[derive(Debug, Default, Clone)]
struct ParsedPatchMetadata {
    trigger: Option<String>,
    reason: Option<String>,
    changed_files: Vec<ParsedChangedFileSummary>,
}

#[derive(Debug, Clone)]
struct ParsedChangedFileSummary {
    path: String,
    changed_symbol_count: usize,
    added_count: usize,
    removed_count: usize,
    updated_count: usize,
}

pub(super) fn append_patch_projection_tx(
    tx: &Transaction<'_>,
    events: &[OutcomeEvent],
) -> Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let mut event_stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO projection_patch_event(
             event_id, ts, task_id, trigger, actor, reason, work_id, work_title, summary
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut file_stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO projection_patch_file(
             event_id, ts, task_id, file_path, trigger, actor, reason, work_id, work_title,
             summary, changed_symbol_count, added_count, removed_count, updated_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut inserted = 0;
    for event in events {
        let Some((event_summary, file_summaries)) = patch_projection_rows(event) else {
            continue;
        };
        let ts = i64::try_from(event_summary.ts)
            .with_context(|| "patch event timestamp exceeds sqlite integer range")?;
        inserted += event_stmt.execute(params![
            event_summary.event_id.0.as_str(),
            ts,
            event_summary.task_id.as_deref(),
            event_summary.trigger.as_deref(),
            event_summary.actor.as_deref(),
            event_summary.reason.as_deref(),
            event_summary.work_id.as_deref(),
            event_summary.work_title.as_deref(),
            event_summary.summary.as_str(),
        ])?;
        for file_summary in file_summaries {
            file_stmt.execute(params![
                file_summary.event_id.0.as_str(),
                ts,
                file_summary.task_id.as_deref(),
                file_summary.path.as_str(),
                file_summary.trigger.as_deref(),
                file_summary.actor.as_deref(),
                file_summary.reason.as_deref(),
                file_summary.work_id.as_deref(),
                file_summary.work_title.as_deref(),
                file_summary.summary.as_str(),
                i64::try_from(file_summary.changed_symbol_count)?,
                i64::try_from(file_summary.added_count)?,
                i64::try_from(file_summary.removed_count)?,
                i64::try_from(file_summary.updated_count)?,
            ])?;
        }
    }
    Ok(inserted)
}

pub(super) fn delete_patch_projection_rows_tx(tx: &Transaction<'_>, event_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM projection_patch_file WHERE event_id = ?1",
        params![event_id],
    )?;
    tx.execute(
        "DELETE FROM projection_patch_event WHERE event_id = ?1",
        params![event_id],
    )?;
    Ok(())
}

pub(super) fn backfill_patch_projection_if_needed(conn: &mut Connection) -> Result<usize> {
    if metadata_value(conn, PATCH_PROJECTION_BACKFILLED_KEY)?.is_some() {
        return Ok(0);
    }
    if !table_exists(conn, "outcome_event_log")? {
        set_metadata_value(conn, PATCH_PROJECTION_BACKFILLED_KEY, 1)?;
        return Ok(0);
    }

    let mut events = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT payload FROM outcome_event_log ORDER BY sequence ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            events.push(serde_json::from_str::<OutcomeEvent>(&row?).context(
                "failed to decode outcome event payload during patch projection backfill",
            )?);
        }
    }

    super::run_with_immediate_tx(conn, |tx| {
        let inserted = append_patch_projection_tx(tx, &events)?;
        set_metadata_value_tx(tx, PATCH_PROJECTION_BACKFILLED_KEY, 1)?;
        Ok(inserted)
    })
}

pub(super) fn load_patch_event_summaries(
    conn: &Connection,
    query: &PatchEventSummaryQuery,
) -> Result<Vec<PatchEventSummary>> {
    let mut sql = String::from(
        "SELECT event_id, ts, task_id, trigger, actor, reason, work_id, work_title, summary
         FROM projection_patch_event
         WHERE 1 = 1",
    );
    let mut params = Vec::<SqlValue>::new();
    if let Some(task_id) = query.task_id.as_ref() {
        sql.push_str(" AND task_id = ?");
        params.push(SqlValue::from(task_id.0.to_string()));
    }
    if let Some(since) = query.since {
        sql.push_str(" AND ts >= ?");
        params.push(SqlValue::from(i64::try_from(since)?));
    }
    if let Some(target) = query.target.as_ref() {
        let (kind, value) = anchor_key(target);
        sql.push_str(
            " AND EXISTS (
                SELECT 1
                FROM outcome_event_anchor a
                WHERE a.event_id = projection_patch_event.event_id
                  AND a.anchor_kind = ?
                  AND a.anchor_value = ?
            )",
        );
        params.push(SqlValue::from(kind.to_string()));
        params.push(SqlValue::from(value));
    }
    if let Some(path) = query.path.as_deref() {
        sql.push_str(
            " AND EXISTS (
                SELECT 1
                FROM projection_patch_file f
                WHERE f.event_id = projection_patch_event.event_id
                  AND instr(f.file_path, ?) > 0
            )",
        );
        params.push(SqlValue::from(path.to_string()));
    }
    sql.push_str(" ORDER BY ts DESC, event_id DESC");
    if query.limit > 0 {
        sql.push_str(" LIMIT ?");
        params.push(SqlValue::from(i64::try_from(query.limit)?));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        Ok(PatchEventSummary {
            event_id: EventId::new(row.get::<_, String>(0)?),
            ts: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
            task_id: row.get(2)?,
            trigger: row.get(3)?,
            actor: row.get(4)?,
            reason: row.get(5)?,
            work_id: row.get(6)?,
            work_title: row.get(7)?,
            summary: row.get(8)?,
        })
    })?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row?);
    }
    Ok(summaries)
}

pub(super) fn load_patch_file_summaries(
    conn: &Connection,
    query: &PatchFileSummaryQuery,
) -> Result<Vec<PatchFileSummary>> {
    let mut sql = String::from(
        "SELECT event_id, ts, task_id, file_path, trigger, actor, reason, work_id, work_title,
                summary, changed_symbol_count, added_count, removed_count, updated_count
         FROM (
             SELECT event_id, ts, task_id, file_path, trigger, actor, reason, work_id,
                    work_title, summary, changed_symbol_count, added_count, removed_count,
                    updated_count,
                    ROW_NUMBER() OVER (
                        PARTITION BY file_path
                        ORDER BY ts DESC, event_id DESC
                    ) AS path_rank
             FROM projection_patch_file
             WHERE 1 = 1",
    );
    let mut params = Vec::<SqlValue>::new();
    if let Some(task_id) = query.task_id.as_ref() {
        sql.push_str(" AND task_id = ?");
        params.push(SqlValue::from(task_id.0.to_string()));
    }
    if let Some(since) = query.since {
        sql.push_str(" AND ts >= ?");
        params.push(SqlValue::from(i64::try_from(since)?));
    }
    if let Some(path) = query.path.as_deref() {
        sql.push_str(" AND instr(file_path, ?) > 0");
        params.push(SqlValue::from(path.to_string()));
    }
    sql.push_str(") WHERE path_rank = 1 ORDER BY ts DESC, event_id DESC");
    if query.limit > 0 {
        sql.push_str(" LIMIT ?");
        params.push(SqlValue::from(i64::try_from(query.limit)?));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params.iter()), |row| {
        Ok(PatchFileSummary {
            event_id: EventId::new(row.get::<_, String>(0)?),
            ts: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
            task_id: row.get(2)?,
            path: row.get(3)?,
            trigger: row.get(4)?,
            actor: row.get(5)?,
            reason: row.get(6)?,
            work_id: row.get(7)?,
            work_title: row.get(8)?,
            summary: row.get(9)?,
            changed_symbol_count: usize::try_from(row.get::<_, i64>(10)?).unwrap_or_default(),
            added_count: usize::try_from(row.get::<_, i64>(11)?).unwrap_or_default(),
            removed_count: usize::try_from(row.get::<_, i64>(12)?).unwrap_or_default(),
            updated_count: usize::try_from(row.get::<_, i64>(13)?).unwrap_or_default(),
        })
    })?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row?);
    }
    Ok(summaries)
}

fn patch_projection_rows(
    event: &OutcomeEvent,
) -> Option<(PatchEventSummary, Vec<PatchFileSummary>)> {
    if event.kind != OutcomeKind::PatchApplied {
        return None;
    }
    let metadata = parse_patch_metadata(&event.metadata);
    let task_id = event
        .meta
        .correlation
        .as_ref()
        .map(|task| task.0.to_string());
    let actor = actor_label(&event.meta.actor);
    let work_id = event
        .meta
        .execution_context
        .as_ref()
        .and_then(|context| context.work_context.as_ref())
        .map(|work| work.work_id.clone());
    let work_title = event
        .meta
        .execution_context
        .as_ref()
        .and_then(|context| context.work_context.as_ref())
        .map(|work| work.title.clone());

    let event_summary = PatchEventSummary {
        event_id: event.meta.id.clone(),
        ts: event.meta.ts,
        task_id: task_id.clone(),
        trigger: metadata.trigger.clone(),
        actor: actor.clone(),
        reason: metadata.reason.clone(),
        work_id: work_id.clone(),
        work_title: work_title.clone(),
        summary: event.summary.clone(),
    };

    let file_summaries = metadata
        .changed_files
        .into_iter()
        .map(|summary| PatchFileSummary {
            event_id: event.meta.id.clone(),
            ts: event.meta.ts,
            task_id: task_id.clone(),
            path: summary.path,
            trigger: metadata.trigger.clone(),
            actor: actor.clone(),
            reason: metadata.reason.clone(),
            work_id: work_id.clone(),
            work_title: work_title.clone(),
            summary: event.summary.clone(),
            changed_symbol_count: summary.changed_symbol_count,
            added_count: summary.added_count,
            removed_count: summary.removed_count,
            updated_count: summary.updated_count,
        })
        .collect::<Vec<_>>();

    Some((event_summary, file_summaries))
}

fn parse_patch_metadata(value: &Value) -> ParsedPatchMetadata {
    let Some(metadata) = value.as_object() else {
        return ParsedPatchMetadata::default();
    };

    let file_paths = metadata
        .get("filePaths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut changed_files: Vec<ParsedChangedFileSummary> = metadata
        .get("changedFilesSummary")
        .and_then(Value::as_array)
        .map(|values| parse_changed_file_summary_array(values))
        .unwrap_or_default();
    if changed_files.is_empty() {
        changed_files = changed_file_summary_from_symbols(metadata);
    }
    if changed_files.is_empty() {
        changed_files = file_paths
            .iter()
            .map(|path| ParsedChangedFileSummary {
                path: path.clone(),
                changed_symbol_count: 0,
                added_count: 0,
                removed_count: 0,
                updated_count: 0,
            })
            .collect();
    }

    ParsedPatchMetadata {
        trigger: metadata
            .get("trigger")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        reason: metadata
            .get("reason")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        changed_files,
    }
}

fn parse_changed_file_summary_array(values: &[Value]) -> Vec<ParsedChangedFileSummary> {
    values
        .iter()
        .filter_map(|value| {
            let path = value.get("filePath").and_then(Value::as_str)?.to_string();
            Some(ParsedChangedFileSummary {
                path,
                changed_symbol_count: json_usize(value.get("changedSymbolCount")),
                added_count: json_usize(value.get("addedCount")),
                removed_count: json_usize(value.get("removedCount")),
                updated_count: json_usize(value.get("updatedCount")),
            })
        })
        .collect()
}

fn changed_file_summary_from_symbols(
    metadata: &serde_json::Map<String, Value>,
) -> Vec<ParsedChangedFileSummary> {
    let Some(changed_symbols) = metadata.get("changedSymbols").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut by_path = BTreeMap::<String, ParsedChangedFileSummary>::new();
    for symbol in changed_symbols {
        let Some(path) = symbol
            .get("filePath")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        let summary = by_path
            .entry(path.clone())
            .or_insert_with(|| ParsedChangedFileSummary {
                path,
                changed_symbol_count: 0,
                added_count: 0,
                removed_count: 0,
                updated_count: 0,
            });
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
    by_path.into_values().collect()
}

fn json_usize(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or_default()
}

fn actor_label(actor: &EventActor) -> Option<String> {
    match actor {
        EventActor::Principal(principal) => Some(
            principal
                .name
                .clone()
                .unwrap_or_else(|| principal.scoped_id()),
        ),
        EventActor::Agent => Some("agent".to_string()),
        EventActor::User => Some("user".to_string()),
        EventActor::GitAuthor { name, .. } => Some(name.to_string()),
        EventActor::CI => Some("ci".to_string()),
        EventActor::System => None,
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
        AnchorRef::WorkspacePath(path) => ("file_path", path.clone()),
        AnchorRef::Kind(kind) => ("kind", kind.to_string()),
    }
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

fn metadata_value(conn: &Connection, key: &str) -> Result<Option<u64>> {
    conn.query_row(
        "SELECT value FROM metadata WHERE key = ?1",
        params![key],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|value| value.and_then(|value| u64::try_from(value).ok()))
    .map_err(Into::into)
}

fn set_metadata_value(conn: &Connection, key: &str, value: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, i64::try_from(value)?],
    )?;
    Ok(())
}

fn set_metadata_value_tx(tx: &Transaction<'_>, key: &str, value: u64) -> Result<()> {
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, i64::try_from(value)?],
    )?;
    Ok(())
}
