use std::sync::Arc;

use anyhow::Result;
use prism_ir::LineageEvent;

use crate::composite::MemoryComposite;
use crate::episodic::EpisodicMemory;
use crate::semantic::SemanticMemory;
use crate::structural::StructuralMemory;
use crate::types::{
    EpisodicMemorySnapshot, MemoryEntry, MemoryId, MemoryKind, MemoryModule, RecallQuery,
    ScoredMemory,
};

pub struct SessionMemory {
    episodic: Arc<EpisodicMemory>,
    structural: Arc<StructuralMemory>,
    semantic: Arc<SemanticMemory>,
    composite: MemoryComposite,
}

impl Default for SessionMemory {
    fn default() -> Self {
        let episodic = Arc::new(EpisodicMemory::new());
        let structural = Arc::new(StructuralMemory::new());
        let semantic = Arc::new(SemanticMemory::new());

        let mut composite = MemoryComposite::new();
        composite.push_shared_module(episodic.clone(), 1.0);
        composite.push_shared_module(structural.clone(), 1.0);
        composite.push_shared_module(semantic.clone(), 1.0);

        Self {
            episodic,
            structural,
            semantic,
            composite,
        }
    }
}

impl SessionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entry(&self, id: &MemoryId) -> Option<MemoryEntry> {
        self.episodic
            .entry(id)
            .or_else(|| self.structural.entry(id))
            .or_else(|| self.semantic.entry(id))
    }

    pub fn snapshot(&self) -> EpisodicMemorySnapshot {
        let mut entries = self.episodic.snapshot().entries;
        entries.extend(self.structural.snapshot().entries);
        entries.extend(self.semantic.snapshot().entries);
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        EpisodicMemorySnapshot { entries }
    }

    pub fn from_snapshot(snapshot: EpisodicMemorySnapshot) -> Self {
        let mut episodic_entries = Vec::new();
        let mut structural_entries = Vec::new();
        let mut semantic_entries = Vec::new();

        for entry in snapshot.entries {
            match entry.kind {
                MemoryKind::Episodic => episodic_entries.push(entry),
                MemoryKind::Structural => structural_entries.push(entry),
                MemoryKind::Semantic => semantic_entries.push(entry),
            }
        }

        let episodic = Arc::new(EpisodicMemory::from_snapshot(EpisodicMemorySnapshot {
            entries: episodic_entries,
        }));
        let structural = Arc::new(StructuralMemory::from_snapshot(EpisodicMemorySnapshot {
            entries: structural_entries,
        }));
        let semantic = Arc::new(SemanticMemory::from_snapshot(EpisodicMemorySnapshot {
            entries: semantic_entries,
        }));

        let mut composite = MemoryComposite::new();
        composite.push_shared_module(episodic.clone(), 1.0);
        composite.push_shared_module(structural.clone(), 1.0);
        composite.push_shared_module(semantic.clone(), 1.0);

        Self {
            episodic,
            structural,
            semantic,
            composite,
        }
    }
}

impl MemoryModule for SessionMemory {
    fn name(&self) -> &'static str {
        "session"
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        self.composite.supports_kind(kind)
    }

    fn store(&self, entry: MemoryEntry) -> Result<MemoryId> {
        match entry.kind {
            MemoryKind::Episodic => self.episodic.store(entry),
            MemoryKind::Structural => self.structural.store(entry),
            MemoryKind::Semantic => self.semantic.store(entry),
        }
    }

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        self.composite.recall(query)
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        self.composite.apply_lineage(events)
    }
}
