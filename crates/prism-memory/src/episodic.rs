use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use anyhow::Result;
use prism_ir::{AnchorRef, LineageEvent, LineageEventKind, NodeId, Timestamp};

use crate::common::{
    clamp_unit, compare_scored_memory, current_timestamp, dedupe_anchors, provenance_score,
};
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

#[derive(Default)]
pub struct EpisodicMemory {
    state: RwLock<EpisodicState>,
}

#[derive(Default)]
struct EpisodicState {
    next_sequence: u64,
    entries: HashMap<MemoryId, MemoryEntry>,
    anchor_index: HashMap<AnchorRef, HashSet<MemoryId>>,
}

impl EpisodicMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        self.state
            .read()
            .expect("episodic memory lock poisoned")
            .entries
            .get(id)
            .cloned()
    }

    pub fn snapshot(&self) -> EpisodicMemorySnapshot {
        let state = self.state.read().expect("episodic memory lock poisoned");
        let mut entries = state.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        EpisodicMemorySnapshot { entries }
    }

    pub fn from_snapshot(snapshot: EpisodicMemorySnapshot) -> Self {
        let memory = Self::new();
        let mut state = memory.state.write().expect("episodic memory lock poisoned");
        for entry in snapshot.entries {
            restore_entry(&mut state, entry);
        }
        drop(state);
        memory
    }
}

impl MemoryModule for EpisodicMemory {
    fn name(&self) -> &'static str {
        "episodic"
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        matches!(
            kind,
            MemoryKind::Episodic | MemoryKind::Structural | MemoryKind::Semantic
        )
    }

    fn store(&self, mut entry: MemoryEntry) -> Result<MemoryId> {
        entry.anchors = dedupe_anchors(entry.anchors);
        entry.trust = clamp_unit(entry.trust);

        let mut state = self.state.write().expect("episodic memory lock poisoned");
        state.next_sequence += 1;
        let id = MemoryId::stored(state.next_sequence);
        entry.id = id.clone();

        for anchor in &entry.anchors {
            state
                .anchor_index
                .entry(anchor.clone())
                .or_default()
                .insert(id.clone());
        }
        state.entries.insert(id.clone(), entry);

        Ok(id)
    }

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }

        let state = self.state.read().expect("episodic memory lock poisoned");
        let candidate_ids = recall_candidates(&state, query);
        let mut results = candidate_ids
            .into_iter()
            .filter_map(|id| {
                let entry = state.entries.get(&id)?.clone();
                score_episodic_memory(&id, entry, query)
            })
            .collect::<Vec<_>>();
        results.sort_by(compare_scored_memory);
        results.truncate(query.limit);
        Ok(results)
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        let mut state = self.state.write().expect("episodic memory lock poisoned");

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

fn restore_entry(state: &mut EpisodicState, mut entry: MemoryEntry) {
    entry.anchors = dedupe_anchors(entry.anchors);
    entry.trust = clamp_unit(entry.trust);
    state.next_sequence = state
        .next_sequence
        .max(memory_sequence(&entry.id).unwrap_or(state.next_sequence));
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
    id.0.strip_prefix("memory:")
        .or_else(|| id.0.strip_prefix("episodic:"))?
        .parse()
        .ok()
}

fn recall_candidates(state: &EpisodicState, query: &RecallQuery) -> HashSet<MemoryId> {
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

fn score_episodic_memory(
    id: &MemoryId,
    entry: MemoryEntry,
    query: &RecallQuery,
) -> Option<ScoredMemory> {
    if let Some(kinds) = &query.kinds {
        if !kinds.contains(&entry.kind) {
            return None;
        }
    }

    if let Some(since) = query.since {
        if entry.created_at < since {
            return None;
        }
    }

    let overlap = anchor_overlap(&entry.anchors, &query.focus);
    if !query.focus.is_empty() && overlap == 0.0 {
        return None;
    }

    let text_score = match &query.text {
        Some(text) => text_match_score(&entry.content, text)?,
        None => 0.0,
    };
    let recency = recency_score(entry.created_at);
    let provenance = provenance_score(entry.source, entry.trust);
    let score = if query.text.is_some() {
        0.45 * overlap.max(0.25) + 0.30 * text_score + 0.15 * recency + 0.10 * provenance
    } else if query.focus.is_empty() {
        0.70 * recency + 0.30 * provenance
    } else {
        0.65 * overlap + 0.20 * recency + 0.15 * provenance
    };

    let explanation = if query.text.is_some() {
        Some(format!(
            "anchor overlap {:.2}, text match {:.2}, recency {:.2}, provenance {:.2}",
            overlap, text_score, recency, provenance
        ))
    } else {
        Some(format!(
            "anchor overlap {:.2}, recency {:.2}, provenance {:.2}",
            overlap, recency, provenance
        ))
    };

    Some(ScoredMemory {
        id: id.clone(),
        entry,
        score: clamp_unit(score),
        source_module: "episodic".to_string(),
        explanation,
    })
}

fn anchor_overlap(anchors: &[AnchorRef], focus: &[AnchorRef]) -> f32 {
    if anchors.is_empty() {
        return if focus.is_empty() { 1.0 } else { 0.0 };
    }

    if focus.is_empty() {
        return 1.0;
    }

    let focus_set = focus.iter().collect::<HashSet<_>>();
    let overlap = anchors
        .iter()
        .filter(|anchor| focus_set.contains(anchor))
        .count();
    overlap as f32 / anchors.len() as f32
}

fn text_match_score(content: &str, query: &str) -> Option<f32> {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return Some(1.0);
    }

    let normalized_content = content.to_ascii_lowercase();
    normalized_content
        .contains(&normalized_query)
        .then_some(1.0)
}

fn recency_score(created_at: Timestamp) -> f32 {
    let age = current_timestamp().saturating_sub(created_at) as f32;
    let one_week = 7.0 * 24.0 * 60.0 * 60.0;
    1.0 / (1.0 + age / one_week)
}

fn apply_reanchor_event(
    state: &mut EpisodicState,
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
    state: &mut EpisodicState,
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

fn replace_anchor(state: &mut EpisodicState, old_anchor: &AnchorRef, replacements: &[AnchorRef]) {
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
    state: &mut EpisodicState,
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

fn remove_memory(state: &mut EpisodicState, memory_id: &MemoryId) {
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
