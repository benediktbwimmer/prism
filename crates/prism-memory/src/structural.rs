use anyhow::Result;
use prism_ir::LineageEvent;

use crate::anchored::AnchoredMemory;
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct StructuralMemory {
    inner: AnchoredMemory,
}

impl Default for StructuralMemory {
    fn default() -> Self {
        Self {
            inner: AnchoredMemory::new("structural", "structural", &[MemoryKind::Structural]),
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
            inner: AnchoredMemory::from_snapshot(
                "structural",
                "structural",
                &[MemoryKind::Structural],
                snapshot,
            ),
        }
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
        self.inner.recall(query)
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.inner.apply_lineage(events)
    }
}
