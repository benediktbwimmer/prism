use std::collections::HashMap;

use anyhow::{anyhow, Result};
use prism_ir::LineageEvent;

use crate::common::{clamp_unit, compare_scored_memory, is_better_candidate};
use crate::types::{MemoryId, MemoryKind, MemoryModule, RecallQuery, ScoredMemory};

#[derive(Default)]
pub struct MemoryComposite {
    modules: Vec<(Box<dyn MemoryModule>, f32)>,
}

impl MemoryComposite {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_module<M>(mut self, module: M, weight: f32) -> Self
    where
        M: MemoryModule + 'static,
    {
        self.modules.push((Box::new(module), weight.max(0.0)));
        self
    }

    pub fn push_module<M>(&mut self, module: M, weight: f32)
    where
        M: MemoryModule + 'static,
    {
        self.modules.push((Box::new(module), weight.max(0.0)));
    }
}

impl MemoryModule for MemoryComposite {
    fn name(&self) -> &'static str {
        "composite"
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        self.modules
            .iter()
            .any(|(module, _)| module.supports_kind(kind))
    }

    fn store(&self, entry: crate::types::MemoryEntry) -> Result<MemoryId> {
        let (module, _) = self
            .modules
            .iter()
            .filter(|(module, _)| module.supports_kind(entry.kind))
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .ok_or_else(|| anyhow!("no memory module registered for {:?}", entry.kind))?;
        module.store(entry)
    }

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        let mut deduped = HashMap::<MemoryId, ScoredMemory>::new();

        for (module, weight) in &self.modules {
            if *weight == 0.0 {
                continue;
            }

            for mut memory in module.recall(query)? {
                memory.score = clamp_unit(memory.score) * clamp_unit(*weight);
                let key = memory.id.clone();
                match deduped.get(&key) {
                    Some(existing) if !is_better_candidate(&memory, existing) => {}
                    _ => {
                        deduped.insert(key, memory);
                    }
                }
            }
        }

        let mut results = deduped.into_values().collect::<Vec<_>>();
        results.sort_by(compare_scored_memory);
        if query.limit > 0 {
            results.truncate(query.limit);
        }
        Ok(results)
    }

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        for (module, _) in &self.modules {
            module.apply_lineage(events)?;
        }
        Ok(())
    }
}
