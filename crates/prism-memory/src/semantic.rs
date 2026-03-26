use anyhow::Result;
use prism_ir::LineageEvent;

use crate::entry_store::EntryStore;
use crate::recall::{base_signals, score_entry, sort_and_limit};
use crate::text::{
    cosine_similarity, embedding_text, hashed_embedding, substring_score, token_overlap, token_set,
};
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct SemanticMemory {
    inner: EntryStore,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self {
            inner: EntryStore::new("semantic", "semantic", &[MemoryKind::Semantic]),
        }
    }
}

impl SemanticMemory {
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
                "semantic",
                "semantic",
                &[MemoryKind::Semantic],
                snapshot,
            ),
        }
    }

    pub fn replace_from_snapshot(&self, snapshot: EpisodicMemorySnapshot) {
        self.inner.replace_from_snapshot(snapshot);
    }
}

impl MemoryModule for SemanticMemory {
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
        let query_text = query.text.as_deref().unwrap_or_default();
        let query_terms = token_set(query_text);
        let query_embedding = hashed_embedding(query_text);
        let results = self
            .inner
            .candidates(query)
            .into_iter()
            .filter_map(|entry| {
                let signals = base_signals(&entry, query)?;
                let text = embedding_text(&entry.content, &entry.metadata);
                let substring = if query.text.is_some() {
                    substring_score(&text, query_text)
                } else {
                    0.0
                };
                let lexical = if query.text.is_some() {
                    token_overlap(&token_set(&text), &query_terms)
                } else {
                    0.0
                };
                let semantic = if query.text.is_some() {
                    cosine_similarity(&hashed_embedding(&text), &query_embedding)
                } else {
                    0.0
                };
                let text_score = substring.max((0.45 * lexical + 0.55 * semantic).clamp(0.0, 1.0));
                if query.text.is_some() && text_score == 0.0 {
                    return None;
                }

                let score = if query.text.is_some() {
                    0.30 * signals.overlap.max(0.20)
                        + 0.40 * text_score
                        + 0.15 * lexical
                        + 0.05 * signals.recency
                        + 0.10 * signals.trust
                } else if query.focus.is_empty() {
                    0.65 * signals.recency + 0.35 * signals.trust
                } else {
                    0.55 * signals.overlap + 0.20 * signals.recency + 0.25 * signals.trust
                };

                let explanation = if query.text.is_some() {
                    Some(format!(
                        "anchor overlap {:.2}, semantic {:.2}, lexical {:.2}, recency {:.2}, trust {:.2}",
                        signals.overlap, semantic, lexical, signals.recency, signals.trust
                    ))
                } else {
                    Some(format!(
                        "anchor overlap {:.2}, recency {:.2}, trust {:.2}",
                        signals.overlap, signals.recency, signals.trust
                    ))
                };

                Some(score_entry(self.name(), entry, score, explanation))
            })
            .collect();
        Ok(sort_and_limit(results, query.limit))
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.inner.apply_lineage(events)
    }
}
