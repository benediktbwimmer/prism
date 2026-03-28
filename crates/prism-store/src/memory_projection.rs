use std::collections::HashMap;

use prism_memory::{EpisodicMemorySnapshot, MemoryEntry};

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
