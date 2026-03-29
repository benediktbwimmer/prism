use std::cmp::Ordering;
use std::collections::HashMap;

use prism_memory::{OutcomeEvent, OutcomeMemorySnapshot};

pub(crate) fn merge_snapshot(
    current: Option<OutcomeMemorySnapshot>,
    incoming: &OutcomeMemorySnapshot,
) -> Option<OutcomeMemorySnapshot> {
    let mut by_id = HashMap::<String, OutcomeEvent>::new();
    if let Some(snapshot) = current {
        for event in snapshot.events {
            by_id.insert(event.meta.id.0.to_string(), event);
        }
    }
    for event in &incoming.events {
        by_id.insert(event.meta.id.0.to_string(), event.clone());
    }
    finalize_snapshot(by_id.into_values().collect())
}

pub(crate) fn append_only_delta(
    current: Option<&OutcomeMemorySnapshot>,
    incoming: &OutcomeMemorySnapshot,
) -> Vec<OutcomeEvent> {
    let current_by_id = current
        .map(|snapshot| {
            snapshot
                .events
                .iter()
                .map(|event| (event.meta.id.0.clone(), event))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    incoming
        .events
        .iter()
        .filter(|event| {
            current_by_id
                .get(&event.meta.id.0)
                .is_none_or(|current| *current != *event)
        })
        .cloned()
        .collect()
}

pub(crate) fn snapshot_from_events<I>(events: I) -> Option<OutcomeMemorySnapshot>
where
    I: IntoIterator<Item = OutcomeEvent>,
{
    let mut by_id = HashMap::<String, OutcomeEvent>::new();
    for event in events {
        by_id.insert(event.meta.id.0.to_string(), event);
    }
    finalize_snapshot(by_id.into_values().collect())
}

fn finalize_snapshot(mut events: Vec<OutcomeEvent>) -> Option<OutcomeMemorySnapshot> {
    if events.is_empty() {
        return None;
    }
    events.sort_by(compare_outcome_event);
    Some(OutcomeMemorySnapshot { events })
}

fn compare_outcome_event(left: &OutcomeEvent, right: &OutcomeEvent) -> Ordering {
    right
        .meta
        .ts
        .cmp(&left.meta.ts)
        .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
}
