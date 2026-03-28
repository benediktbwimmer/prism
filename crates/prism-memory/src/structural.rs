use anyhow::Result;
use prism_ir::LineageEvent;

use crate::entry_store::EntryStore;
use crate::recall::{base_signals, score_entry, sort_and_limit};
use crate::structural_features::{derive_query_features, derive_structural_features};
use crate::text::token_overlap;
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct StructuralMemory {
    inner: EntryStore,
}

impl Default for StructuralMemory {
    fn default() -> Self {
        Self {
            inner: EntryStore::new("structural", "structural", &[MemoryKind::Structural]),
        }
    }
}

impl StructuralMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        self.inner.entry(id)
    }

    pub fn snapshot(&self) -> EpisodicMemorySnapshot {
        self.inner.snapshot()
    }

    pub fn from_snapshot(snapshot: EpisodicMemorySnapshot) -> Self {
        Self {
            inner: EntryStore::from_snapshot(
                "structural",
                "structural",
                &[MemoryKind::Structural],
                snapshot,
            ),
        }
    }

    pub fn replace_from_snapshot(&self, snapshot: EpisodicMemorySnapshot) {
        self.inner.replace_from_snapshot(snapshot);
    }
}

impl MemoryModule for StructuralMemory {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        self.inner.supports_kind(kind)
    }

    fn store(&self, entry: MemoryEntry) -> Result<MemoryId> {
        self.inner.store(entry)
    }

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        let query_features = query.text.as_deref().map(derive_query_features);
        let results = self
            .inner
            .candidates(query)
            .into_iter()
            .filter_map(|entry| {
                let signals = base_signals(&entry, query)?;
                let features = derive_structural_features(&entry);
                let tag_score = query_features
                    .as_ref()
                    .map(|query| token_overlap(&features.tags, &query.tags))
                    .unwrap_or_else(|| (!features.tags.is_empty()) as i32 as f32 * 0.6);
                let term_score = query_features
                    .as_ref()
                    .map(|query| token_overlap(&features.terms, &query.terms))
                    .unwrap_or(0.0);
                let rule_kind_score = query_features
                    .as_ref()
                    .map(|query| token_overlap(&features.rule_kinds, &query.rule_kinds))
                    .unwrap_or(0.0);
                if query.text.is_some()
                    && tag_score == 0.0
                    && term_score == 0.0
                    && rule_kind_score == 0.0
                {
                    return None;
                }
                let promoted_bonus = if features.promoted_rule {
                    0.6 + 0.4 * features.evidence_strength
                } else {
                    0.0
                };

                let score = if query.text.is_some() {
                    0.40 * signals.overlap.max(0.25)
                        + 0.18 * tag_score
                        + 0.16 * term_score
                        + 0.11 * rule_kind_score
                        + 0.05 * signals.recency
                        + 0.10 * signals.trust
                        + 0.10 * promoted_bonus
                } else if query.focus.is_empty() {
                    0.30 * tag_score
                        + 0.10 * rule_kind_score
                        + 0.10 * promoted_bonus
                        + 0.15 * signals.recency
                        + 0.35 * signals.trust
                } else {
                    0.50 * signals.overlap
                        + 0.15 * tag_score
                        + 0.08 * term_score
                        + 0.08 * rule_kind_score
                        + 0.05 * signals.recency
                        + 0.10 * signals.trust
                        + 0.04 * promoted_bonus
                };

                let explanation = Some(format!(
                    "anchor overlap {:.2}, structural tags {:.2}, rule kinds {:.2}, term overlap {:.2}, promoted rule {:.2}, recency {:.2}, trust {:.2}",
                    signals.overlap,
                    tag_score,
                    rule_kind_score,
                    term_score,
                    promoted_bonus,
                    signals.recency,
                    signals.trust
                ));

                Some(score_entry(self.name(), entry, score, explanation))
            })
            .collect();
        Ok(sort_and_limit(results, query.limit))
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.inner.apply_lineage(events)
    }
}
