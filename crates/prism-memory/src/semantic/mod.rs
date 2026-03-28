mod config;
#[cfg(feature = "openai-embeddings")]
mod openai;
mod runtime;

pub use config::{SemanticBackendKind, SemanticMemoryConfig};

use anyhow::Result;
use prism_ir::LineageEvent;

use crate::entry_store::EntryStore;
use crate::recall::{base_signals, score_entry, sort_and_limit};
use crate::text::{
    cosine_similarity, embedding_text, expanded_token_set, hashed_embedding, substring_score,
    token_overlap,
};
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

use self::runtime::{SemanticBackendRuntime, SemanticSignalSource};

pub struct SemanticMemory {
    inner: EntryStore,
    config: SemanticMemoryConfig,
    runtime: SemanticBackendRuntime,
}

struct SemanticCandidate {
    entry: MemoryEntry,
    signals: crate::recall::BaseSignals,
    text: String,
    substring: f32,
    lexical: f32,
    alias: f32,
    semantic: f32,
    semantic_source: SemanticSignalSource,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self::with_config(SemanticMemoryConfig::from_env())
    }
}

impl SemanticMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: SemanticMemoryConfig) -> Self {
        Self {
            inner: EntryStore::new("semantic", "semantic", &[MemoryKind::Semantic]),
            runtime: SemanticBackendRuntime::new(&config),
            config,
        }
    }

    pub fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        self.inner.entry(id)
    }

    pub fn snapshot(&self) -> EpisodicMemorySnapshot {
        self.inner.snapshot()
    }

    pub fn from_snapshot(snapshot: EpisodicMemorySnapshot) -> Self {
        Self::from_snapshot_with_config(snapshot, SemanticMemoryConfig::from_env())
    }

    pub fn from_snapshot_with_config(
        snapshot: EpisodicMemorySnapshot,
        config: SemanticMemoryConfig,
    ) -> Self {
        Self {
            inner: EntryStore::from_snapshot(
                "semantic",
                "semantic",
                &[MemoryKind::Semantic],
                snapshot,
            ),
            runtime: SemanticBackendRuntime::new(&config),
            config,
        }
    }

    pub fn replace_from_snapshot(&self, snapshot: EpisodicMemorySnapshot) {
        self.inner.replace_from_snapshot(snapshot);
    }

    #[cfg(test)]
    pub(crate) fn configured_backend(&self) -> SemanticBackendKind {
        self.config.preferred_backend
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
        let query_terms = expanded_token_set(query_text);
        let mut candidates = self
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
                let text_terms = expanded_token_set(&text);
                let lexical = if query.text.is_some() {
                    token_overlap(&text_terms, &query_terms)
                } else {
                    0.0
                };
                let alias = if query.text.is_some() {
                    alias_bridge_score(&text_terms, &query_terms)
                } else {
                    0.0
                };
                let semantic = if query.text.is_some() {
                    cosine_similarity(&hashed_embedding(&text), &hashed_embedding(query_text))
                } else {
                    0.0
                };
                let text_score = substring
                    .max((0.30 * lexical + 0.20 * alias + 0.50 * semantic).clamp(0.0, 1.0));
                if query.text.is_some() && text_score == 0.0 {
                    return None;
                }

                Some(SemanticCandidate {
                    entry,
                    signals,
                    text,
                    substring,
                    lexical,
                    alias,
                    semantic,
                    semantic_source: SemanticSignalSource::Local,
                })
            })
            .collect::<Vec<_>>();

        if query.text.is_some() && !candidates.is_empty() {
            self.runtime.refresh_semantic_scores(
                query_text,
                &mut candidates,
                self.config.remote_candidate_limit(),
            );
        }

        let results = candidates
            .into_iter()
            .filter_map(|candidate| {
                let text_score = candidate.substring.max(
                    (0.28 * candidate.lexical + 0.17 * candidate.alias + 0.55 * candidate.semantic)
                        .clamp(0.0, 1.0),
                );
                if query.text.is_some() && text_score == 0.0 {
                    return None;
                }

                let score = if query.text.is_some() {
                    0.30 * candidate.signals.overlap.max(0.20)
                        + 0.37 * text_score
                        + 0.10 * candidate.lexical
                        + 0.08 * candidate.alias
                        + 0.05 * candidate.signals.recency
                        + 0.10 * candidate.signals.trust
                } else if query.focus.is_empty() {
                    0.65 * candidate.signals.recency + 0.35 * candidate.signals.trust
                } else {
                    0.55 * candidate.signals.overlap
                        + 0.20 * candidate.signals.recency
                        + 0.25 * candidate.signals.trust
                };

                let explanation = if query.text.is_some() {
                    Some(format!(
                        "anchor overlap {:.2}, semantic {:.2}, lexical {:.2}, alias {:.2}, backend {}, recency {:.2}, trust {:.2}",
                        candidate.signals.overlap,
                        candidate.semantic,
                        candidate.lexical,
                        candidate.alias,
                        candidate.semantic_source.label(),
                        candidate.signals.recency,
                        candidate.signals.trust
                    ))
                } else {
                    Some(format!(
                        "anchor overlap {:.2}, recency {:.2}, trust {:.2}",
                        candidate.signals.overlap, candidate.signals.recency, candidate.signals.trust
                    ))
                };

                Some(score_entry(self.name(), candidate.entry, score, explanation))
            })
            .collect();
        Ok(sort_and_limit(results, query.limit))
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.inner.apply_lineage(events)
    }
}

fn alias_bridge_score(
    left: &std::collections::HashSet<String>,
    right: &std::collections::HashSet<String>,
) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let shared = left.intersection(right).count() as f32;
    let left_norm = left.len() as f32;
    let right_norm = right.len() as f32;
    (shared / left_norm.min(right_norm)).clamp(0.0, 1.0)
}
