use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use prism_ir::{AnchorRef, LineageEvent, LineageEventKind, NodeId};

use crate::common::{clamp_unit, dedupe_anchors};
use crate::types::{EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, RecallQuery};

pub(crate) struct EntryStore {
    module_name: &'static str,
    id_prefix: &'static str,
    supported_kinds: &'static [MemoryKind],
    state: RwLock<EntryState>,
}

#[derive(Default)]
struct EntryState {
    next_sequence: u64,
    entries: HashMap<MemoryId, MemoryEntry>,
    anchor_index: HashMap<AnchorRef, HashSet<MemoryId>>,
}

impl EntryStore {
    pub(crate) fn new(
        module_name: &'static str,
        id_prefix: &'static str,
        supported_kinds: &'static [MemoryKind],
    ) -> Self {
        Self {
            module_name,
            id_prefix,
            supported_kinds,
            state: RwLock::new(EntryState::default()),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        self.module_name
    }

    pub(crate) fn supports_kind(&self, kind: MemoryKind) -> bool {
        self.supported_kinds.contains(&kind)
    }

    pub(crate) fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        self.state
            .read()
            .expect("memory entry store lock poisoned")
            .entries
            .get(id)
            .cloned()
    }

    pub(crate) fn snapshot(&self) -> EpisodicMemorySnapshot {
        let state = self.state.read().expect("memory entry store lock poisoned");
        let mut entries = state.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        EpisodicMemorySnapshot { entries }
    }

    pub(crate) fn from_snapshot(
        module_name: &'static str,
        id_prefix: &'static str,
        supported_kinds: &'static [MemoryKind],
        snapshot: EpisodicMemorySnapshot,
    ) -> Self {
        let store = Self::new(module_name, id_prefix, supported_kinds);
        let mut state = store
            .state
            .write()
            .expect("memory entry store lock poisoned");
        for entry in snapshot.entries {
            if supported_kinds.contains(&entry.kind) {
                restore_entry(&mut state, entry);
            }
        }
        drop(state);
        store
    }

    pub(crate) fn store(&self, mut entry: MemoryEntry) -> Result<MemoryId> {
        if !self.supports_kind(entry.kind) {
            return Err(anyhow!(
                "{} memory does not support {:?} entries",
                self.module_name,
                entry.kind
            ));
        }

        entry.anchors = dedupe_anchors(entry.anchors);
        entry.trust = clamp_unit(entry.trust);

        let mut state = self
            .state
            .write()
            .expect("memory entry store lock poisoned");
        state.next_sequence += 1;
        let id = MemoryId(format!("{}:{}", self.id_prefix, state.next_sequence));
        entry.id = id.clone();
        insert_entry(&mut state, entry.clone());
        Ok(id)
    }

    pub(crate) fn candidates(&self, query: &RecallQuery) -> Vec<MemoryEntry> {
        let state = self.state.read().expect("memory entry store lock poisoned");
        let candidate_ids = candidate_ids(&state, query);
        candidate_ids
            .into_iter()
            .filter_map(|id| state.entries.get(&id).cloned())
            .collect()
    }

    pub(crate) fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        let mut state = self
            .state
            .write()
            .expect("memory entry store lock poisoned");

        for event in events {
            let lineage_anchor = AnchorRef::Lineage(event.lineage.clone());
            match event.kind {
                LineageEventKind::Born
                | LineageEventKind::Updated
                | LineageEventKind::Ambiguous => {
                    for after in &event.after {
                        add_anchor_to_matching_lineage(&mut state, &lineage_anchor, after);
                    }
                }
                LineageEventKind::Renamed
                | LineageEventKind::Moved
                | LineageEventKind::Reparented
                | LineageEventKind::Revived => {
                    apply_reanchor_event(&mut state, &event.before, &event.after, &lineage_anchor);
                }
                LineageEventKind::Split | LineageEventKind::Merged | LineageEventKind::Died => {
                    for before in &event.before {
                        replace_anchor(
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
}

fn restore_entry(state: &mut EntryState, mut entry: MemoryEntry) {
    entry.anchors = dedupe_anchors(entry.anchors);
    entry.trust = clamp_unit(entry.trust);
    state.next_sequence = state
        .next_sequence
        .max(memory_sequence(&entry.id).unwrap_or(state.next_sequence));
    insert_entry(state, entry);
}

fn insert_entry(state: &mut EntryState, entry: MemoryEntry) {
    for anchor in &entry.anchors {
        state
            .anchor_index
            .entry(anchor.clone())
            .or_default()
            .insert(entry.id.clone());
    }
    state.entries.insert(entry.id.clone(), entry);
}

fn memory_sequence(id: &MemoryId) -> Option<u64> {
    id.0.split_once(':')?.1.parse().ok()
}

fn candidate_ids(state: &EntryState, query: &RecallQuery) -> HashSet<MemoryId> {
    if query.focus.is_empty() {
        return state.entries.keys().cloned().collect();
    }

    query
        .focus
        .iter()
        .filter_map(|anchor| state.anchor_index.get(anchor))
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}

fn apply_reanchor_event(
    state: &mut EntryState,
    before: &[NodeId],
    after: &[NodeId],
    lineage_anchor: &AnchorRef,
) {
    if before.len() == 1 && after.len() == 1 {
        replace_anchor(
            state,
            &AnchorRef::Node(before[0].clone()),
            &[AnchorRef::Node(after[0].clone()), lineage_anchor.clone()],
        );
        return;
    }

    for previous in before {
        replace_anchor(
            state,
            &AnchorRef::Node(previous.clone()),
            &[lineage_anchor.clone()],
        );
    }

    for next in after {
        add_anchor_to_matching_lineage(state, lineage_anchor, next);
    }
}

fn add_anchor_to_matching_lineage(
    state: &mut EntryState,
    lineage_anchor: &AnchorRef,
    node: &NodeId,
) {
    let Some(memory_ids) = state.anchor_index.get(lineage_anchor).cloned() else {
        return;
    };

    let new_anchor = AnchorRef::Node(node.clone());
    for memory_id in memory_ids {
        let Some(entry) = state.entries.get_mut(&memory_id) else {
            continue;
        };
        let old_anchors = entry.anchors.clone();
        entry.anchors.push(new_anchor.clone());
        entry.anchors = dedupe_anchors(entry.anchors.clone());
        let new_anchors = entry.anchors.clone();
        let _ = entry;
        reindex_memory(state, &memory_id, &old_anchors, &new_anchors);
    }
}

fn replace_anchor(state: &mut EntryState, old_anchor: &AnchorRef, replacements: &[AnchorRef]) {
    let Some(memory_ids) = state.anchor_index.get(old_anchor).cloned() else {
        return;
    };

    for memory_id in memory_ids {
        let Some(entry) = state.entries.get_mut(&memory_id) else {
            continue;
        };
        let old_anchors = entry.anchors.clone();
        entry.anchors.retain(|anchor| anchor != old_anchor);
        entry.anchors.extend(replacements.iter().cloned());
        entry.anchors = dedupe_anchors(entry.anchors.clone());
        let new_anchors = entry.anchors.clone();
        let empty = new_anchors.is_empty();
        let _ = entry;
        if empty {
            remove_memory(state, &memory_id);
        } else {
            reindex_memory(state, &memory_id, &old_anchors, &new_anchors);
        }
    }
}

fn reindex_memory(
    state: &mut EntryState,
    memory_id: &MemoryId,
    old_anchors: &[AnchorRef],
    new_anchors: &[AnchorRef],
) {
    let old_set = old_anchors.iter().cloned().collect::<HashSet<_>>();
    let new_set = new_anchors.iter().cloned().collect::<HashSet<_>>();

    for removed in old_set.difference(&new_set) {
        if let Some(ids) = state.anchor_index.get_mut(removed) {
            ids.remove(memory_id);
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
            .insert(memory_id.clone());
    }
}

fn remove_memory(state: &mut EntryState, memory_id: &MemoryId) {
    let Some(entry) = state.entries.remove(memory_id) else {
        return;
    };

    for anchor in entry.anchors {
        if let Some(ids) = state.anchor_index.get_mut(&anchor) {
            ids.remove(memory_id);
            if ids.is_empty() {
                state.anchor_index.remove(&anchor);
            }
        }
    }
}
