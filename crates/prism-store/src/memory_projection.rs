use std::collections::HashMap;

use prism_memory::{EpisodicMemorySnapshot, MemoryEntry, MemoryEvent, MemoryEventKind};

pub(crate) fn latest_snapshot<I>(entries: I) -> Option<EpisodicMemorySnapshot>
where
    I: IntoIterator<Item = MemoryEntry>,
{
    let mut by_id = HashMap::<String, MemoryEntry>::new();
    for entry in entries {
        by_id.insert(entry.id.0.clone(), entry);
    }
    finalize_snapshot(by_id.into_values().collect())
}

pub(crate) fn merge_snapshot(
    current: Option<EpisodicMemorySnapshot>,
    incoming: &EpisodicMemorySnapshot,
) -> Option<EpisodicMemorySnapshot> {
    let mut by_id = HashMap::<String, MemoryEntry>::new();
    if let Some(snapshot) = current {
        for entry in snapshot.entries {
            by_id.insert(entry.id.0.clone(), entry);
        }
    }
    for entry in &incoming.entries {
        by_id.insert(entry.id.0.clone(), entry.clone());
    }
    finalize_snapshot(by_id.into_values().collect())
}

pub(crate) fn append_only_delta(
    current: Option<&EpisodicMemorySnapshot>,
    incoming: &EpisodicMemorySnapshot,
) -> Vec<MemoryEntry> {
    let current_by_id = current
        .map(|snapshot| {
            snapshot
                .entries
                .iter()
                .map(|entry| (entry.id.0.clone(), entry))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    incoming
        .entries
        .iter()
        .filter(|entry| {
            current_by_id
                .get(&entry.id.0)
                .is_none_or(|current| *current != *entry)
        })
        .cloned()
        .collect()
}

pub(crate) fn snapshot_from_events<I>(events: I) -> Option<EpisodicMemorySnapshot>
where
    I: IntoIterator<Item = MemoryEvent>,
{
    let mut by_id = HashMap::<String, MemoryEntry>::new();
    for event in events {
        for superseded in &event.supersedes {
            by_id.remove(&superseded.0);
        }
        match event.action {
            MemoryEventKind::Stored | MemoryEventKind::Promoted | MemoryEventKind::Superseded => {
                if let Some(entry) = event.entry {
                    by_id.insert(event.memory_id.0, entry);
                }
            }
            MemoryEventKind::Retired => {
                by_id.remove(&event.memory_id.0);
            }
        }
    }
    finalize_snapshot(by_id.into_values().collect())
}

fn finalize_snapshot(mut entries: Vec<MemoryEntry>) -> Option<EpisodicMemorySnapshot> {
    if entries.is_empty() {
        return None;
    }
    entries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    Some(EpisodicMemorySnapshot { entries })
}
