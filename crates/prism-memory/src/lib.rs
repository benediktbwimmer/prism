use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use prism_ir::{GraphChange, NodeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Timestamp = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    fn episodic(sequence: u64) -> Self {
        Self(format!("episodic:{sequence}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryKind {
    Episodic,
    Structural,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemorySource {
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub anchors: Vec<NodeId>,
    pub kind: MemoryKind,
    pub content: String,
    pub metadata: Value,
    pub created_at: Timestamp,
    pub source: MemorySource,
    pub trust: f32,
}

impl MemoryEntry {
    pub fn new(kind: MemoryKind, content: impl Into<String>) -> Self {
        Self {
            anchors: Vec::new(),
            kind,
            content: content.into(),
            metadata: Value::Null,
            created_at: current_timestamp(),
            source: MemorySource::Agent,
            trust: 0.5,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallQuery {
    pub focus: Vec<NodeId>,
    pub text: Option<String>,
    pub limit: usize,
    pub kinds: Option<Vec<MemoryKind>>,
    pub since: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredMemory {
    pub id: MemoryId,
    pub entry: MemoryEntry,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}

pub trait MemoryModule: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports_kind(&self, kind: MemoryKind) -> bool;

    fn store(&self, entry: MemoryEntry) -> Result<MemoryId>;

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>>;

    fn apply_changes(&self, changes: &[GraphChange]) -> Result<()>;
}

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

    fn store(&self, entry: MemoryEntry) -> Result<MemoryId> {
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

    fn apply_changes(&self, changes: &[GraphChange]) -> Result<()> {
        for (module, _) in &self.modules {
            module.apply_changes(changes)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct EpisodicMemory {
    state: RwLock<EpisodicState>,
}

#[derive(Default)]
struct EpisodicState {
    next_sequence: u64,
    entries: HashMap<MemoryId, MemoryEntry>,
    anchor_index: HashMap<NodeId, HashSet<MemoryId>>,
}

impl EpisodicMemory {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryModule for EpisodicMemory {
    fn name(&self) -> &'static str {
        "episodic"
    }

    fn supports_kind(&self, kind: MemoryKind) -> bool {
        kind == MemoryKind::Episodic
    }

    fn store(&self, mut entry: MemoryEntry) -> Result<MemoryId> {
        if entry.kind != MemoryKind::Episodic {
            return Err(anyhow!(
                "episodic memory cannot store {:?} entries",
                entry.kind
            ));
        }

        entry.anchors = dedupe_anchors(entry.anchors);
        entry.trust = clamp_unit(entry.trust);
        let mut state = self.state.write().expect("episodic memory lock poisoned");
        state.next_sequence += 1;
        let id = MemoryId::episodic(state.next_sequence);

        for anchor in &entry.anchors {
            state
                .anchor_index
                .entry(anchor.clone())
                .or_default()
                .insert(id.clone());
        }
        state.entries.insert(id.clone(), entry);

        Ok(id)
    }

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }

        if let Some(kinds) = &query.kinds {
            if !kinds.contains(&MemoryKind::Episodic) {
                return Ok(Vec::new());
            }
        }

        let state = self.state.read().expect("episodic memory lock poisoned");
        let candidate_ids = recall_candidates(&state, query);
        let mut results = candidate_ids
            .into_iter()
            .filter_map(|id| {
                let entry = state.entries.get(&id)?.clone();
                score_episodic_memory(&id, entry, query)
            })
            .collect::<Vec<_>>();
        results.sort_by(compare_scored_memory);
        results.truncate(query.limit);
        Ok(results)
    }

    fn apply_changes(&self, changes: &[GraphChange]) -> Result<()> {
        let mut state = self.state.write().expect("episodic memory lock poisoned");
        for change in changes {
            match change {
                GraphChange::Added(_) | GraphChange::Modified(_) => {}
                GraphChange::Removed(id) => remove_anchor(&mut state, id),
                GraphChange::Reanchored { old, new } => reanchor(&mut state, old, new),
            }
        }
        Ok(())
    }
}

fn recall_candidates(state: &EpisodicState, query: &RecallQuery) -> HashSet<MemoryId> {
    if query.focus.is_empty() {
        return state.entries.keys().cloned().collect();
    }

    query
        .focus
        .iter()
        .filter_map(|node| state.anchor_index.get(node))
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}

fn score_episodic_memory(
    id: &MemoryId,
    entry: MemoryEntry,
    query: &RecallQuery,
) -> Option<ScoredMemory> {
    if let Some(since) = query.since {
        if entry.created_at < since {
            return None;
        }
    }

    let overlap = anchor_overlap(&entry.anchors, &query.focus);
    if !query.focus.is_empty() && overlap == 0.0 {
        return None;
    }

    let text_score = match &query.text {
        Some(text) => text_match_score(&entry.content, text)?,
        None => 0.0,
    };
    let recency = recency_score(entry.created_at);
    let provenance = provenance_score(entry.source, entry.trust);
    let score = if query.text.is_some() {
        0.45 * overlap.max(0.25) + 0.30 * text_score + 0.15 * recency + 0.10 * provenance
    } else if query.focus.is_empty() {
        0.70 * recency + 0.30 * provenance
    } else {
        0.65 * overlap + 0.20 * recency + 0.15 * provenance
    };

    let explanation = if query.text.is_some() {
        Some(format!(
            "anchor overlap {:.2}, text match {:.2}, recency {:.2}, provenance {:.2}",
            overlap, text_score, recency, provenance
        ))
    } else {
        Some(format!(
            "anchor overlap {:.2}, recency {:.2}, provenance {:.2}",
            overlap, recency, provenance
        ))
    };

    Some(ScoredMemory {
        id: id.clone(),
        entry,
        score: clamp_unit(score),
        source_module: "episodic".to_string(),
        explanation,
    })
}

fn anchor_overlap(anchors: &[NodeId], focus: &[NodeId]) -> f32 {
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

fn text_match_score(content: &str, query: &str) -> Option<f32> {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return Some(1.0);
    }

    let normalized_content = content.to_ascii_lowercase();
    normalized_content
        .contains(&normalized_query)
        .then_some(1.0)
}

fn recency_score(created_at: Timestamp) -> f32 {
    let age = current_timestamp().saturating_sub(created_at) as f32;
    let one_week = 7.0 * 24.0 * 60.0 * 60.0;
    1.0 / (1.0 + age / one_week)
}

fn provenance_score(source: MemorySource, trust: f32) -> f32 {
    let source_bias = match source {
        MemorySource::User => 1.0,
        MemorySource::System => 0.9,
        MemorySource::Agent => 0.75,
    };
    (source_bias + clamp_unit(trust)) / 2.0
}

fn compare_scored_memory(left: &ScoredMemory, right: &ScoredMemory) -> Ordering {
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

fn is_better_candidate(candidate: &ScoredMemory, existing: &ScoredMemory) -> bool {
    compare_scored_memory(candidate, existing) == Ordering::Less
}

fn remove_anchor(state: &mut EpisodicState, anchor: &NodeId) {
    let Some(memory_ids) = state.anchor_index.remove(anchor) else {
        return;
    };

    let mut emptied = Vec::new();
    for memory_id in memory_ids {
        if let Some(entry) = state.entries.get_mut(&memory_id) {
            entry.anchors.retain(|existing| existing != anchor);
            if entry.anchors.is_empty() {
                emptied.push(memory_id.clone());
            }
        }
    }

    for memory_id in emptied {
        remove_memory(state, &memory_id);
    }
}

fn reanchor(state: &mut EpisodicState, old: &NodeId, new: &NodeId) {
    if old == new {
        return;
    }

    let Some(memory_ids) = state.anchor_index.remove(old) else {
        return;
    };

    for memory_id in memory_ids {
        if let Some(entry) = state.entries.get_mut(&memory_id) {
            for anchor in &mut entry.anchors {
                if anchor == old {
                    *anchor = new.clone();
                }
            }
            entry.anchors = dedupe_anchors(entry.anchors.clone());
            state
                .anchor_index
                .entry(new.clone())
                .or_default()
                .insert(memory_id.clone());
        }
    }
}

fn remove_memory(state: &mut EpisodicState, memory_id: &MemoryId) {
    let Some(entry) = state.entries.remove(memory_id) else {
        return;
    };

    for anchor in entry.anchors {
        if let Some(ids) = state.anchor_index.get_mut(&anchor) {
            ids.remove(memory_id);
            if ids.is_empty() {
                state.anchor_index.remove(&anchor);
            }
        }
    }
}

fn dedupe_anchors(anchors: Vec<NodeId>) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for anchor in anchors {
        if seen.insert(anchor.clone()) {
            deduped.push(anchor);
        }
    }
    deduped
}

fn clamp_unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn current_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::NodeKind;
    use serde_json::json;

    fn node(name: &str) -> NodeId {
        NodeId::new("demo", format!("demo::{name}"), NodeKind::Function)
    }

    #[test]
    fn episodic_memory_generates_store_owned_ids() {
        let memory = EpisodicMemory::new();
        let mut entry = MemoryEntry::new(
            MemoryKind::Episodic,
            "Function alpha changed in commit abc123",
        );
        entry.anchors = vec![node("alpha")];
        entry.source = MemorySource::User;
        entry.trust = 1.0;

        let id = memory.store(entry).unwrap();

        assert_eq!(id.0, "episodic:1");
    }

    #[test]
    fn recall_uses_anchor_overlap_and_since_filter() {
        let memory = EpisodicMemory::new();

        let mut alpha = MemoryEntry::new(
            MemoryKind::Episodic,
            "Bug report mentioned alpha null handling",
        );
        alpha.anchors = vec![node("alpha"), node("beta")];
        alpha.created_at = 1_000;
        alpha.source = MemorySource::User;
        alpha.trust = 1.0;
        alpha.metadata = json!({"issue": "BUG-1"});
        memory.store(alpha).unwrap();

        let mut beta = MemoryEntry::new(
            MemoryKind::Episodic,
            "User noted beta is performance sensitive",
        );
        beta.anchors = vec![node("beta")];
        beta.created_at = 2_000;
        beta.source = MemorySource::System;
        beta.trust = 0.8;
        memory.store(beta).unwrap();

        let results = memory
            .recall(&RecallQuery {
                focus: vec![node("beta")],
                text: Some("performance".into()),
                limit: 10,
                kinds: Some(vec![MemoryKind::Episodic]),
                since: Some(1_500),
            })
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].entry.content.contains("performance sensitive"));
    }

    #[test]
    fn graph_reanchoring_moves_memory_to_new_node_id() {
        let memory = EpisodicMemory::new();
        let old = node("alpha");
        let new = node("renamed_alpha");

        let mut entry = MemoryEntry::new(
            MemoryKind::Episodic,
            "Function alpha changed in commit abc123",
        );
        entry.anchors = vec![old.clone()];
        memory.store(entry).unwrap();

        memory
            .apply_changes(&[GraphChange::Reanchored {
                old: old.clone(),
                new: new.clone(),
            }])
            .unwrap();

        let old_results = memory
            .recall(&RecallQuery {
                focus: vec![old],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();
        let new_results = memory
            .recall(&RecallQuery {
                focus: vec![new],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert!(old_results.is_empty());
        assert_eq!(new_results.len(), 1);
    }

    #[test]
    fn removing_last_anchor_drops_anchored_memory() {
        let memory = EpisodicMemory::new();
        let alpha = node("alpha");

        let mut entry = MemoryEntry::new(MemoryKind::Episodic, "User noted alpha is sensitive");
        entry.anchors = vec![alpha.clone()];
        memory.store(entry).unwrap();

        memory
            .apply_changes(&[GraphChange::Removed(alpha.clone())])
            .unwrap();

        let results = memory
            .recall(&RecallQuery {
                focus: vec![alpha],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert!(results.is_empty());
    }

    struct StaticModule {
        name: &'static str,
        score: f32,
        id: &'static str,
    }

    impl MemoryModule for StaticModule {
        fn name(&self) -> &'static str {
            self.name
        }

        fn supports_kind(&self, kind: MemoryKind) -> bool {
            kind == MemoryKind::Episodic
        }

        fn store(&self, _entry: MemoryEntry) -> Result<MemoryId> {
            Ok(MemoryId(self.id.to_string()))
        }

        fn recall(&self, _query: &RecallQuery) -> Result<Vec<ScoredMemory>> {
            Ok(vec![ScoredMemory {
                id: MemoryId(self.id.to_string()),
                entry: MemoryEntry::new(MemoryKind::Episodic, format!("from {}", self.name)),
                score: self.score,
                source_module: self.name.to_string(),
                explanation: Some("static test result".to_string()),
            }])
        }

        fn apply_changes(&self, _changes: &[GraphChange]) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn composite_clamps_weights_and_dedupes_ids() {
        let composite = MemoryComposite::new()
            .with_module(
                StaticModule {
                    name: "first",
                    score: 1.4,
                    id: "shared",
                },
                0.25,
            )
            .with_module(
                StaticModule {
                    name: "second",
                    score: 0.8,
                    id: "shared",
                },
                1.0,
            );

        let results = composite
            .recall(&RecallQuery {
                focus: Vec::new(),
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_module, "second");
        assert!((results[0].score - 0.8).abs() < f32::EPSILON);
    }
}
