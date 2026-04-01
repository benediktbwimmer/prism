use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use anyhow::Result;
use prism_ir::{AnchorRef, EventActor, EventId, LineageEvent, LineageEventKind, NodeId, TaskId};

use crate::common::dedupe_anchors;
use crate::outcome_query::OutcomeRecallQuery;
use crate::types::{OutcomeEvent, OutcomeKind, OutcomeMemorySnapshot, OutcomeResult, TaskReplay};

#[derive(Default)]
pub struct OutcomeMemory {
    state: RwLock<OutcomeState>,
}

impl Clone for OutcomeMemory {
    fn clone(&self) -> Self {
        Self::from_snapshot(self.snapshot())
    }
}

#[derive(Default)]
struct OutcomeState {
    events: HashMap<EventId, OutcomeEvent>,
    anchor_index: HashMap<AnchorRef, HashSet<EventId>>,
    task_index: HashMap<TaskId, HashSet<EventId>>,
    kind_index: HashMap<OutcomeKind, HashSet<EventId>>,
    result_index: HashMap<OutcomeResult, HashSet<EventId>>,
    actor_index: HashMap<String, HashSet<EventId>>,
    order: Vec<EventId>,
}

impl OutcomeMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn event(&self, id: &EventId) -> Option<OutcomeEvent> {
        self.state
            .read()
            .expect("outcome memory lock poisoned")
            .events
            .get(id)
            .cloned()
    }

    pub fn store_event(&self, mut event: OutcomeEvent) -> Result<EventId> {
        event.anchors = dedupe_anchors(event.anchors);
        let id = event.meta.id.clone();

        let mut state = self.state.write().expect("outcome memory lock poisoned");
        index_outcome(&mut state, &event);
        state.order.push(id.clone());
        state.events.insert(id.clone(), event);
        Ok(id)
    }

    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        self.query_events(&OutcomeRecallQuery {
            anchors: anchors.to_vec(),
            limit,
            ..OutcomeRecallQuery::default()
        })
    }

    pub fn related_failures(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        self.query_events(&OutcomeRecallQuery {
            anchors: anchors.to_vec(),
            kinds: Some(vec![
                OutcomeKind::FailureObserved,
                OutcomeKind::RegressionObserved,
            ]),
            result: Some(OutcomeResult::Failure),
            limit,
            ..OutcomeRecallQuery::default()
        })
    }

    pub fn resume_task(&self, task: &TaskId) -> TaskReplay {
        TaskReplay {
            task: task.clone(),
            events: self.query_events(&OutcomeRecallQuery {
                task: Some(task.clone()),
                limit: 0,
                ..OutcomeRecallQuery::default()
            }),
        }
    }

    pub fn query_events(&self, query: &OutcomeRecallQuery) -> Vec<OutcomeEvent> {
        let state = self.state.read().expect("outcome memory lock poisoned");
        let candidate_ids = outcome_candidates(&state, query);
        let mut events = candidate_ids
            .into_iter()
            .filter_map(|id| state.events.get(&id).cloned())
            .filter(|event| query.since.is_none_or(|since| event.meta.ts >= since))
            .filter(|event| {
                query
                    .actor
                    .as_ref()
                    .is_none_or(|actor| actor_key(&event.meta.actor) == actor_key(actor))
            })
            .filter(|event| {
                query
                    .kinds
                    .as_ref()
                    .is_none_or(|kinds| kinds.iter().any(|kind| kind == &event.kind))
            })
            .filter(|event| query.result.is_none_or(|result| event.result == result))
            .collect::<Vec<_>>();
        events.sort_by(compare_outcome_event);
        if query.limit > 0 {
            events.truncate(query.limit);
        }
        events
    }

    pub fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        let mut state = self.state.write().expect("outcome memory lock poisoned");

        for event in events {
            let lineage_anchor = AnchorRef::Lineage(event.lineage.clone());
            match event.kind {
                LineageEventKind::Born
                | LineageEventKind::Updated
                | LineageEventKind::Ambiguous => {
                    for after in &event.after {
                        add_outcome_anchor_to_matching_lineage(&mut state, &lineage_anchor, after);
                    }
                }
                LineageEventKind::Renamed
                | LineageEventKind::Moved
                | LineageEventKind::Reparented
                | LineageEventKind::Revived => {
                    apply_outcome_reanchor_event(
                        &mut state,
                        &event.before,
                        &event.after,
                        &lineage_anchor,
                    );
                }
                LineageEventKind::Split | LineageEventKind::Merged | LineageEventKind::Died => {
                    for before in &event.before {
                        replace_outcome_anchor(
                            &mut state,
                            &AnchorRef::Node(before.clone()),
                            &[lineage_anchor.clone()],
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn snapshot(&self) -> OutcomeMemorySnapshot {
        let state = self.state.read().expect("outcome memory lock poisoned");
        let mut events = state.events.values().cloned().collect::<Vec<_>>();
        events.sort_by(compare_outcome_event);
        OutcomeMemorySnapshot { events }
    }

    pub fn from_snapshot(snapshot: OutcomeMemorySnapshot) -> Self {
        let memory = Self::new();
        for event in snapshot.events {
            let _ = memory.store_event(event);
        }
        memory
    }
}

fn outcome_candidates(state: &OutcomeState, query: &OutcomeRecallQuery) -> HashSet<EventId> {
    let mut candidates = if query.anchors.is_empty() {
        state.events.keys().cloned().collect::<HashSet<_>>()
    } else {
        query
            .anchors
            .iter()
            .filter_map(|anchor| state.anchor_index.get(anchor))
            .flat_map(|ids| ids.iter().cloned())
            .collect::<HashSet<_>>()
    };

    if let Some(task) = query.task.as_ref() {
        intersect_with(&mut candidates, state.task_index.get(task));
    }
    if let Some(kinds) = query.kinds.as_ref() {
        let kind_matches = kinds
            .iter()
            .filter_map(|kind| state.kind_index.get(kind))
            .flat_map(|ids| ids.iter().cloned())
            .collect::<HashSet<_>>();
        intersect_with(&mut candidates, Some(&kind_matches));
    }
    if let Some(result) = query.result {
        intersect_with(&mut candidates, state.result_index.get(&result));
    }
    if let Some(actor) = query.actor.as_ref() {
        let key = actor_key(actor);
        intersect_with(&mut candidates, state.actor_index.get(&key));
    }

    candidates
}

fn intersect_with(candidates: &mut HashSet<EventId>, matching: Option<&HashSet<EventId>>) {
    match matching {
        Some(matching) => candidates.retain(|id| matching.contains(id)),
        None => candidates.clear(),
    }
}

fn compare_outcome_event(left: &OutcomeEvent, right: &OutcomeEvent) -> Ordering {
    right
        .meta
        .ts
        .cmp(&left.meta.ts)
        .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
}

fn index_outcome(state: &mut OutcomeState, event: &OutcomeEvent) {
    let id = event.meta.id.clone();
    for anchor in &event.anchors {
        state
            .anchor_index
            .entry(anchor.clone())
            .or_default()
            .insert(id.clone());
    }
    if let Some(task) = &event.meta.correlation {
        state
            .task_index
            .entry(task.clone())
            .or_default()
            .insert(id.clone());
    }
    state
        .kind_index
        .entry(event.kind.clone())
        .or_default()
        .insert(id.clone());
    state
        .result_index
        .entry(event.result)
        .or_default()
        .insert(id.clone());
    state
        .actor_index
        .entry(actor_key(&event.meta.actor))
        .or_default()
        .insert(id);
}

fn actor_key(actor: &EventActor) -> String {
    actor.canonical_identity_key()
}

fn apply_outcome_reanchor_event(
    state: &mut OutcomeState,
    before: &[NodeId],
    after: &[NodeId],
    lineage_anchor: &AnchorRef,
) {
    if before.len() == 1 && after.len() == 1 {
        replace_outcome_anchor(
            state,
            &AnchorRef::Node(before[0].clone()),
            &[AnchorRef::Node(after[0].clone()), lineage_anchor.clone()],
        );
        return;
    }

    for previous in before {
        replace_outcome_anchor(
            state,
            &AnchorRef::Node(previous.clone()),
            &[lineage_anchor.clone()],
        );
    }

    for next in after {
        add_outcome_anchor_to_matching_lineage(state, lineage_anchor, next);
    }
}

fn add_outcome_anchor_to_matching_lineage(
    state: &mut OutcomeState,
    lineage_anchor: &AnchorRef,
    node: &NodeId,
) {
    let Some(event_ids) = state.anchor_index.get(lineage_anchor).cloned() else {
        return;
    };

    let new_anchor = AnchorRef::Node(node.clone());
    for event_id in event_ids {
        let Some(event) = state.events.get_mut(&event_id) else {
            continue;
        };
        let old_anchors = event.anchors.clone();
        event.anchors.push(new_anchor.clone());
        event.anchors = dedupe_anchors(event.anchors.clone());
        let new_anchors = event.anchors.clone();
        let _ = event;
        reindex_outcome(state, &event_id, &old_anchors, &new_anchors);
    }
}

fn replace_outcome_anchor(
    state: &mut OutcomeState,
    old_anchor: &AnchorRef,
    replacements: &[AnchorRef],
) {
    let Some(event_ids) = state.anchor_index.get(old_anchor).cloned() else {
        return;
    };

    for event_id in event_ids {
        let Some(event) = state.events.get_mut(&event_id) else {
            continue;
        };
        let old_anchors = event.anchors.clone();
        event.anchors.retain(|anchor| anchor != old_anchor);
        event.anchors.extend(replacements.iter().cloned());
        event.anchors = dedupe_anchors(event.anchors.clone());
        let new_anchors = event.anchors.clone();
        let empty = new_anchors.is_empty();
        let _ = event;
        if empty {
            remove_outcome_event(state, &event_id);
        } else {
            reindex_outcome(state, &event_id, &old_anchors, &new_anchors);
        }
    }
}

fn reindex_outcome(
    state: &mut OutcomeState,
    event_id: &EventId,
    old_anchors: &[AnchorRef],
    new_anchors: &[AnchorRef],
) {
    let old_set = old_anchors.iter().cloned().collect::<HashSet<_>>();
    let new_set = new_anchors.iter().cloned().collect::<HashSet<_>>();

    for removed in old_set.difference(&new_set) {
        if let Some(ids) = state.anchor_index.get_mut(removed) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.anchor_index.remove(removed);
            }
        }
    }

    for added in new_set.difference(&old_set) {
        state
            .anchor_index
            .entry(added.clone())
            .or_default()
            .insert(event_id.clone());
    }
}

fn remove_outcome_event(state: &mut OutcomeState, event_id: &EventId) {
    let Some(event) = state.events.remove(event_id) else {
        return;
    };

    for anchor in event.anchors {
        if let Some(ids) = state.anchor_index.get_mut(&anchor) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.anchor_index.remove(&anchor);
            }
        }
    }

    if let Some(task) = event.meta.correlation {
        if let Some(ids) = state.task_index.get_mut(&task) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.task_index.remove(&task);
            }
        }
    }

    if let Some(ids) = state.kind_index.get_mut(&event.kind) {
        ids.remove(event_id);
        if ids.is_empty() {
            state.kind_index.remove(&event.kind);
        }
    }

    if let Some(ids) = state.result_index.get_mut(&event.result) {
        ids.remove(event_id);
        if ids.is_empty() {
            state.result_index.remove(&event.result);
        }
    }

    let actor = actor_key(&event.meta.actor);
    if let Some(ids) = state.actor_index.get_mut(&actor) {
        ids.remove(event_id);
        if ids.is_empty() {
            state.actor_index.remove(&actor);
        }
    }
}
