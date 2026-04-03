use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_memory::{OutcomeEvent, OutcomeKind};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::outcome_patch_projection::{
    append_patch_projection_tx, delete_patch_projection_rows_tx,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PatchPathIdentityRepairReport {
    pub scanned_patch_event_count: usize,
    pub patch_events_needing_repair: usize,
    pub repaired_projection_event_count: usize,
    pub repaired: bool,
}

pub(super) fn inspect_or_repair_patch_path_identity(
    conn: &mut Connection,
    roots: &[PathBuf],
    apply_repairs: bool,
) -> Result<PatchPathIdentityRepairReport> {
    if !outcome_event_log_exists(conn)? {
        return Ok(PatchPathIdentityRepairReport::default());
    }

    let mut scanned_patch_event_count = 0usize;
    let mut patch_events_needing_repair = 0usize;
    let mut updates = Vec::<(i64, String, OutcomeEvent)>::new();
    {
        let mut stmt = conn.prepare(
            "SELECT sequence, event_id, payload FROM outcome_event_log ORDER BY sequence ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (sequence, event_id, raw) = row?;
            let mut event = serde_json::from_str::<OutcomeEvent>(&raw).with_context(|| {
                format!(
                    "failed to decode outcome event payload during path-identity repair for {event_id}"
                )
            })?;
            if event.kind != OutcomeKind::PatchApplied {
                continue;
            }
            scanned_patch_event_count += 1;
            if normalize_patch_event_paths(roots, &mut event) {
                patch_events_needing_repair += 1;
                if apply_repairs {
                    updates.push((sequence, event_id, event));
                }
            }
        }
    }

    if !apply_repairs || updates.is_empty() {
        return Ok(PatchPathIdentityRepairReport {
            scanned_patch_event_count,
            patch_events_needing_repair,
            repaired_projection_event_count: 0,
            repaired: false,
        });
    }

    super::run_with_immediate_tx(conn, |tx| {
        let mut update_stmt =
            tx.prepare_cached("UPDATE outcome_event_log SET payload = ?2 WHERE sequence = ?1")?;
        for (sequence, event_id, event) in &updates {
            update_stmt.execute(params![sequence, serde_json::to_string(event)?])?;
            delete_patch_projection_rows_tx(tx, event_id)?;
        }
        let repaired_events = updates
            .iter()
            .map(|(_, _, event)| event.clone())
            .collect::<Vec<_>>();
        append_patch_projection_tx(tx, &repaired_events)?;
        super::bump_metadata_value_tx(tx, super::OUTCOME_REVISION_KEY)?;
        super::bump_metadata_value_tx(tx, super::WORKSPACE_REVISION_KEY)?;
        Ok(())
    })?;

    Ok(PatchPathIdentityRepairReport {
        scanned_patch_event_count,
        patch_events_needing_repair,
        repaired_projection_event_count: updates.len(),
        repaired: true,
    })
}

fn outcome_event_log_exists(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'outcome_event_log' LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

fn normalize_patch_event_paths(roots: &[PathBuf], event: &mut OutcomeEvent) -> bool {
    if event.kind != OutcomeKind::PatchApplied {
        return false;
    }
    let Some(metadata) = event.metadata.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    if let Some(file_paths) = metadata.get_mut("filePaths").and_then(Value::as_array_mut) {
        for path in file_paths.iter_mut() {
            changed |= normalize_json_path_value(roots, path);
        }
    }
    if let Some(summaries) = metadata
        .get_mut("changedFilesSummary")
        .and_then(Value::as_array_mut)
    {
        for summary in summaries {
            let Some(file_path) = summary.get_mut("filePath") else {
                continue;
            };
            changed |= normalize_json_path_value(roots, file_path);
        }
    }
    if let Some(symbols) = metadata
        .get_mut("changedSymbols")
        .and_then(Value::as_array_mut)
    {
        for symbol in symbols {
            let Some(file_path) = symbol.get_mut("filePath") else {
                continue;
            };
            changed |= normalize_json_path_value(roots, file_path);
        }
    }
    changed
}

fn normalize_json_path_value(roots: &[PathBuf], value: &mut Value) -> bool {
    let Some(path) = value.as_str() else {
        return false;
    };
    let normalized = normalize_repo_relative_path(roots, Path::new(path));
    let normalized = normalized.to_string_lossy().into_owned();
    if normalized == path {
        return false;
    }
    *value = Value::String(normalized);
    true
}

fn normalize_repo_relative_path(roots: &[PathBuf], path: &Path) -> PathBuf {
    if path.is_relative() {
        return normalize_path_components(path);
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    for root in roots {
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if let Ok(relative) = canonical.strip_prefix(&canonical_root) {
            return normalize_path_components(relative);
        }
        if let Ok(relative) = path.strip_prefix(&canonical_root) {
            return normalize_path_components(relative);
        }
        if let Ok(relative) = canonical.strip_prefix(root) {
            return normalize_path_components(relative);
        }
        if let Ok(relative) = path.strip_prefix(root) {
            return normalize_path_components(relative);
        }
    }
    path.to_path_buf()
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(part) => normalized.push(part),
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
