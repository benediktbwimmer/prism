use anyhow::Result;
use prism_ir::LineageEvent;

use crate::anchored::AnchoredMemory;
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct SemanticMemory {
    inner: AnchoredMemory,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self {
            inner: AnchoredMemory::new("semantic", "semantic", &[MemoryKind::Semantic]),
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
            inner: AnchoredMemory::from_snapshot(
                "semantic",
                "semantic",
                &[MemoryKind::Semantic],
                snapshot,
            ),
        }
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
        self.inner.recall(query)
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.inner.apply_lineage(events)
    }
}
