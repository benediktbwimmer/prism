use crate::common::{
    anchor_overlap, clamp_unit, compare_scored_memory, recency_score, trust_score,
};
use crate::types::{MemoryEntry, RecallQuery, ScoredMemory};

pub(crate) fn score_entry(
    module_name: &str,
    entry: MemoryEntry,
    score: f32,
    explanation: Option<String>,
) -> ScoredMemory {
    ScoredMemory {
        id: entry.id.clone(),
        entry,
        score: clamp_unit(score),
        source_module: module_name.to_string(),
        explanation,
    }
}

pub(crate) fn base_filters(entry: &MemoryEntry, query: &RecallQuery) -> bool {
    if let Some(kinds) = &query.kinds {
        if !kinds.contains(&entry.kind) {
            return false;
        }
    }
    if let Some(since) = query.since {
        if entry.created_at < since {
            return false;
        }
    }
    true
}

pub(crate) fn base_signals(entry: &MemoryEntry, query: &RecallQuery) -> Option<BaseSignals> {
    if !base_filters(entry, query) {
        return None;
    }
    let overlap = anchor_overlap(&entry.anchors, &query.focus);
    if !query.focus.is_empty() && overlap == 0.0 {
        return None;
    }
    Some(BaseSignals {
        overlap,
        recency: recency_score(entry.created_at),
        trust: trust_score(entry.trust),
    })
}

pub(crate) fn sort_and_limit(mut results: Vec<ScoredMemory>, limit: usize) -> Vec<ScoredMemory> {
    results.sort_by(compare_scored_memory);
    if limit > 0 {
        results.truncate(limit);
    }
    results
}

pub(crate) struct BaseSignals {
    pub overlap: f32,
    pub recency: f32,
    pub trust: f32,
}
