use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use prism_ir::{AnchorRef, Timestamp};

use crate::types::{MemorySource, ScoredMemory};

pub(crate) fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for anchor in anchors {
        if seen.insert(anchor.clone()) {
            deduped.push(anchor);
        }
    }
    deduped
}

pub(crate) fn clamp_unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

pub(crate) fn current_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

pub(crate) fn provenance_score(source: MemorySource, trust: f32) -> f32 {
    let source_bias = match source {
        MemorySource::User => 1.0,
        MemorySource::System => 0.9,
        MemorySource::Agent => 0.75,
    };
    (source_bias + clamp_unit(trust)) / 2.0
}

pub(crate) fn anchor_overlap(anchors: &[AnchorRef], focus: &[AnchorRef]) -> f32 {
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

pub(crate) fn recency_score(created_at: Timestamp) -> f32 {
    let age = current_timestamp().saturating_sub(created_at) as f32;
    let one_week = 7.0 * 24.0 * 60.0 * 60.0;
    1.0 / (1.0 + age / one_week)
}

pub(crate) fn compare_scored_memory(left: &ScoredMemory, right: &ScoredMemory) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| right.entry.created_at.cmp(&left.entry.created_at))
        .then_with(|| {
            provenance_score(right.entry.source, right.entry.trust)
                .total_cmp(&provenance_score(left.entry.source, left.entry.trust))
        })
        .then_with(|| left.id.0.cmp(&right.id.0))
}

pub(crate) fn is_better_candidate(candidate: &ScoredMemory, existing: &ScoredMemory) -> bool {
    compare_scored_memory(candidate, existing) == Ordering::Less
}
