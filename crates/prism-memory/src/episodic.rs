use anyhow::Result;
use prism_ir::LineageEvent;

use crate::entry_store::EntryStore;
use crate::recall::{base_signals, score_entry, sort_and_limit};
use crate::text::{substring_score, token_overlap, token_set};
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct EpisodicMemory {
    inner: EntryStore,
}

impl Default for EpisodicMemory {
    fn default() -> Self {
        Self {
            inner: EntryStore::new("episodic", "memory", &[MemoryKind::Episodic]),
        }
    }
}

impl EpisodicMemory {
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
                "episodic",
                "memory",
                &[MemoryKind::Episodic],
                snapshot,
            ),
        }
    }

    pub fn replace_from_snapshot(&self, snapshot: EpisodicMemorySnapshot) {
        self.inner.replace_from_snapshot(snapshot);
    }
}

impl MemoryModule for EpisodicMemory {
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
        let results = self
            .inner
            .candidates(query)
            .into_iter()
            .filter_map(|entry| {
                let signals = base_signals(&entry, query)?;
                let text_score = query.text.as_ref().map_or(0.0, |text| {
                    let substring = substring_score(&entry.content, text);
                    if substring == 1.0 {
                        return 1.0;
                    }
                    token_overlap(&token_set(&entry.content), &token_set(text))
                });
                if query.text.is_some() && text_score == 0.0 {
                    return None;
                }

                let score = if query.text.is_some() {
                    0.45 * signals.overlap.max(0.25)
                        + 0.30 * text_score
                        + 0.15 * signals.recency
                        + 0.10 * signals.trust
                } else if query.focus.is_empty() {
                    0.70 * signals.recency + 0.30 * signals.trust
                } else {
                    0.65 * signals.overlap + 0.20 * signals.recency + 0.15 * signals.trust
                };

                let explanation = if query.text.is_some() {
                    Some(format!(
                        "anchor overlap {:.2}, text match {:.2}, recency {:.2}, trust {:.2}",
                        signals.overlap, text_score, signals.recency, signals.trust
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
