use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use prism_ir::AnchorRef;
use prism_memory::{MemoryEvent, MemoryEventKind, MemoryEventQuery, MemoryScope};

use crate::util::repo_memory_events_path;

pub(crate) fn append_repo_memory_event(root: &Path, event: &MemoryEvent) -> Result<()> {
    let path = repo_memory_events_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn load_repo_memory_events(root: &Path) -> Result<Vec<MemoryEvent>> {
    let path = repo_memory_events_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<MemoryEvent>(&line).with_context(|| {
            format!(
                "failed to parse memory event on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        if event.scope != MemoryScope::Repo {
            return Err(anyhow::anyhow!(
                "repo memory log {} contained non-repo event `{}`",
                path.display(),
                event.id
            ));
        }
        events.push(event);
    }
    Ok(events)
}

pub(crate) fn filter_memory_events(
    events: Vec<MemoryEvent>,
    query: &MemoryEventQuery,
) -> Vec<MemoryEvent> {
    let text = query.text.as_ref().map(|value| value.to_ascii_lowercase());
    let task_id = query.task_id.as_deref();
    let kinds = query.kinds.as_ref();
    let actions = query.actions.as_ref();
    let scope = query.scope;
    let memory_id = query.memory_id.as_ref();
    let since = query.since;

    let mut filtered = events
        .into_iter()
        .filter(|event| {
            memory_id.is_none_or(|value| &event.memory_id == value)
                && scope.is_none_or(|value| event.scope == value)
                && since.is_none_or(|value| event.recorded_at >= value)
                && task_id.is_none_or(|value| event.task_id.as_deref() == Some(value))
                && actions
                    .is_none_or(|values| values.iter().any(|action| *action == event.action))
                && kinds.is_none_or(|values| {
                    event.entry
                        .as_ref()
                        .is_some_and(|entry| values.iter().any(|kind| *kind == entry.kind))
                })
                && query.focus.iter().all(|anchor| event_matches_anchor(event, anchor))
                && text.as_ref().is_none_or(|needle| event_matches_text(event, needle))
        })
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| {
        right
            .recorded_at
            .cmp(&left.recorded_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    let limit = if query.limit == 0 {
        filtered.len()
    } else {
        query.limit
    };
    filtered.truncate(limit);
    filtered
}

fn event_matches_anchor(event: &MemoryEvent, anchor: &AnchorRef) -> bool {
    event.entry
        .as_ref()
        .is_some_and(|entry| entry.anchors.iter().any(|candidate| candidate == anchor))
}

fn event_matches_text(event: &MemoryEvent, needle: &str) -> bool {
    let Some(entry) = &event.entry else {
        return false;
    };
    entry.content.to_ascii_lowercase().contains(needle)
        || entry
            .metadata
            .to_string()
            .to_ascii_lowercase()
            .contains(needle)
        || event.id.to_ascii_lowercase().contains(needle)
        || event
            .task_id
            .as_ref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
        || matches!(event.action, MemoryEventKind::Promoted)
            && event
                .promoted_from
                .iter()
                .any(|id| id.0.to_ascii_lowercase().contains(needle))
        || event
            .supersedes
            .iter()
            .any(|id| id.0.to_ascii_lowercase().contains(needle))
}
