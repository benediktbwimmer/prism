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

pub(crate) fn trust_score(trust: f32) -> f32 {
    clamp_unit(trust)
}

pub(crate) fn source_preference(source: MemorySource) -> u8 {
    match source {
        MemorySource::User => 3,
        MemorySource::System => 2,
        MemorySource::Agent => 1,
    }
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
        .then_with(|| trust_score(right.entry.trust).total_cmp(&trust_score(left.entry.trust)))
        .then_with(|| {
            source_preference(right.entry.source).cmp(&source_preference(left.entry.source))
        })
        .then_with(|| right.entry.created_at.cmp(&left.entry.created_at))
        .then_with(|| left.id.0.cmp(&right.id.0))
}

pub(crate) fn is_better_candidate(candidate: &ScoredMemory, existing: &ScoredMemory) -> bool {
    compare_scored_memory(candidate, existing) == Ordering::Less
}
