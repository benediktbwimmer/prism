use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use prism_ir::{
    AnchorRef, EventId, EventMeta, LineageEvent, LineageEventKind, NodeId, TaskId, Timestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub String);

impl MemoryId {
    fn episodic(sequence: u64) -> Self {
        Self(format!("episodic:{sequence}"))
    }

    fn pending() -> Self {
        Self("pending".to_string())
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
    pub id: MemoryId,
    pub anchors: Vec<AnchorRef>,
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
            id: MemoryId::pending(),
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
    pub focus: Vec<AnchorRef>,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutcomeKind {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    PatchApplied,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutcomeResult {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutcomeEvidence {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeEvent {
    pub meta: EventMeta,
    pub anchors: Vec<AnchorRef>,
    pub kind: OutcomeKind,
    pub result: OutcomeResult,
    pub summary: String,
    pub evidence: Vec<OutcomeEvidence>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskReplay {
    pub task: TaskId,
    pub events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeMemorySnapshot {
    pub events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodicMemorySnapshot {
    pub entries: Vec<MemoryEntry>,
}

pub trait MemoryModule: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports_kind(&self, kind: MemoryKind) -> bool;

    fn store(&self, entry: MemoryEntry) -> Result<MemoryId>;

    fn recall(&self, query: &RecallQuery) -> Result<Vec<ScoredMemory>>;

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()>;
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

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        for (module, _) in &self.modules {
            module.apply_lineage(events)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct OutcomeMemory {
    state: RwLock<OutcomeState>,
}

#[derive(Default)]
struct OutcomeState {
    events: HashMap<EventId, OutcomeEvent>,
    anchor_index: HashMap<AnchorRef, HashSet<EventId>>,
    task_index: HashMap<TaskId, HashSet<EventId>>,
    order: Vec<EventId>,
}

impl OutcomeMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store_event(&self, mut event: OutcomeEvent) -> Result<EventId> {
        event.anchors = dedupe_anchors(event.anchors);
        let id = event.meta.id.clone();

        let mut state = self.state.write().expect("outcome memory lock poisoned");
        for anchor in &event.anchors {
            state
                .anchor_index
                .entry(anchor.clone())
                .or_default()
                .insert(id.clone());
        }
        if let Some(task) = &event.meta.correlation {
            state
                .task_index
                .entry(task.clone())
                .or_default()
                .insert(id.clone());
        }
        state.order.push(id.clone());
        state.events.insert(id.clone(), event);
        Ok(id)
    }

    pub fn outcomes_for(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        let state = self.state.read().expect("outcome memory lock poisoned");
        let candidate_ids = outcome_candidates(&state, anchors);
        let mut events = candidate_ids
            .into_iter()
            .filter_map(|id| state.events.get(&id).cloned())
            .collect::<Vec<_>>();
        events.sort_by(compare_outcome_event);
        if limit > 0 {
            events.truncate(limit);
        }
        events
    }

    pub fn related_failures(&self, anchors: &[AnchorRef], limit: usize) -> Vec<OutcomeEvent> {
        let mut events = self.outcomes_for(anchors, 0);
        events.retain(is_failure_event);
        if limit > 0 {
            events.truncate(limit);
        }
        events
    }

    pub fn resume_task(&self, task: &TaskId) -> TaskReplay {
        let state = self.state.read().expect("outcome memory lock poisoned");
        let mut events = state
            .task_index
            .get(task)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| state.events.get(id).cloned())
            .collect::<Vec<_>>();
        events.sort_by(compare_outcome_event);
        TaskReplay {
            task: task.clone(),
            events,
        }
    }

    pub fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        let mut state = self.state.write().expect("outcome memory lock poisoned");

        for event in events {
            let lineage_anchor = AnchorRef::Lineage(event.lineage.clone());
            match event.kind {
                LineageEventKind::Born
                | LineageEventKind::Updated
                | LineageEventKind::Ambiguous => {
                    for after in &event.after {
                        add_outcome_anchor_to_matching_lineage(&mut state, &lineage_anchor, after);
                    }
                }
                LineageEventKind::Renamed
                | LineageEventKind::Moved
                | LineageEventKind::Reparented
                | LineageEventKind::Revived => {
                    apply_outcome_reanchor_event(
                        &mut state,
                        &event.before,
                        &event.after,
                        &lineage_anchor,
                    );
                }
                LineageEventKind::Split | LineageEventKind::Merged | LineageEventKind::Died => {
                    for before in &event.before {
                        replace_outcome_anchor(
                            &mut state,
                            &AnchorRef::Node(before.clone()),
                            &[lineage_anchor.clone()],
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn snapshot(&self) -> OutcomeMemorySnapshot {
        let state = self.state.read().expect("outcome memory lock poisoned");
        let mut events = state.events.values().cloned().collect::<Vec<_>>();
        events.sort_by(compare_outcome_event);
        OutcomeMemorySnapshot { events }
    }

    pub fn from_snapshot(snapshot: OutcomeMemorySnapshot) -> Self {
        let memory = Self::new();
        for event in snapshot.events {
            let _ = memory.store_event(event);
        }
        memory
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
    anchor_index: HashMap<AnchorRef, HashSet<MemoryId>>,
}

impl EpisodicMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> EpisodicMemorySnapshot {
        let state = self.state.read().expect("episodic memory lock poisoned");
        let mut entries = state.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        EpisodicMemorySnapshot { entries }
    }

    pub fn from_snapshot(snapshot: EpisodicMemorySnapshot) -> Self {
        let memory = Self::new();
        let mut state = memory
            .state
            .write()
            .expect("episodic memory lock poisoned");
        for entry in snapshot.entries {
            restore_entry(&mut state, entry);
        }
        drop(state);
        memory
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
        entry.id = id.clone();

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

    fn apply_lineage(&self, events: &[LineageEvent]) -> Result<()> {
        let mut state = self.state.write().expect("episodic memory lock poisoned");

        for event in events {
            let lineage_anchor = AnchorRef::Lineage(event.lineage.clone());

            match event.kind {
                LineageEventKind::Born
                | LineageEventKind::Updated
                | LineageEventKind::Ambiguous => {
                    for after in &event.after {
                        add_anchor_to_matching_lineage(&mut state, &lineage_anchor, after);
                    }
                }
                LineageEventKind::Renamed
                | LineageEventKind::Moved
                | LineageEventKind::Reparented
                | LineageEventKind::Revived => {
                    apply_reanchor_event(&mut state, &event.before, &event.after, &lineage_anchor);
                }
                LineageEventKind::Split | LineageEventKind::Merged | LineageEventKind::Died => {
                    for before in &event.before {
                        replace_anchor(
                            &mut state,
                            &AnchorRef::Node(before.clone()),
                            &[lineage_anchor.clone()],
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn restore_entry(state: &mut EpisodicState, mut entry: MemoryEntry) {
    entry.anchors = dedupe_anchors(entry.anchors);
    entry.trust = clamp_unit(entry.trust);
    state.next_sequence = state
        .next_sequence
        .max(memory_sequence(&entry.id).unwrap_or(state.next_sequence));
    for anchor in &entry.anchors {
        state
            .anchor_index
            .entry(anchor.clone())
            .or_default()
            .insert(entry.id.clone());
    }
    state.entries.insert(entry.id.clone(), entry);
}

fn memory_sequence(id: &MemoryId) -> Option<u64> {
    id.0.strip_prefix("episodic:")?.parse().ok()
}

fn recall_candidates(state: &EpisodicState, query: &RecallQuery) -> HashSet<MemoryId> {
    if query.focus.is_empty() {
        return state.entries.keys().cloned().collect();
    }

    query
        .focus
        .iter()
        .filter_map(|anchor| state.anchor_index.get(anchor))
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

fn outcome_candidates(state: &OutcomeState, anchors: &[AnchorRef]) -> HashSet<EventId> {
    if anchors.is_empty() {
        return state.events.keys().cloned().collect();
    }

    anchors
        .iter()
        .filter_map(|anchor| state.anchor_index.get(anchor))
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}

fn compare_outcome_event(left: &OutcomeEvent, right: &OutcomeEvent) -> Ordering {
    right
        .meta
        .ts
        .cmp(&left.meta.ts)
        .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
}

fn is_failure_event(event: &OutcomeEvent) -> bool {
    event.result == OutcomeResult::Failure
        || matches!(
            event.kind,
            OutcomeKind::FailureObserved | OutcomeKind::RegressionObserved
        )
}

fn anchor_overlap(anchors: &[AnchorRef], focus: &[AnchorRef]) -> f32 {
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

fn apply_reanchor_event(
    state: &mut EpisodicState,
    before: &[NodeId],
    after: &[NodeId],
    lineage_anchor: &AnchorRef,
) {
    if before.len() == 1 && after.len() == 1 {
        replace_anchor(
            state,
            &AnchorRef::Node(before[0].clone()),
            &[AnchorRef::Node(after[0].clone()), lineage_anchor.clone()],
        );
        return;
    }

    for previous in before {
        replace_anchor(
            state,
            &AnchorRef::Node(previous.clone()),
            &[lineage_anchor.clone()],
        );
    }

    for next in after {
        add_anchor_to_matching_lineage(state, lineage_anchor, next);
    }
}

fn add_anchor_to_matching_lineage(
    state: &mut EpisodicState,
    lineage_anchor: &AnchorRef,
    node: &NodeId,
) {
    let Some(memory_ids) = state.anchor_index.get(lineage_anchor).cloned() else {
        return;
    };

    let new_anchor = AnchorRef::Node(node.clone());
    for memory_id in memory_ids {
        let Some(entry) = state.entries.get_mut(&memory_id) else {
            continue;
        };
        let old_anchors = entry.anchors.clone();
        entry.anchors.push(new_anchor.clone());
        entry.anchors = dedupe_anchors(entry.anchors.clone());
        let new_anchors = entry.anchors.clone();
        let _ = entry;
        reindex_memory(state, &memory_id, &old_anchors, &new_anchors);
    }
}

fn replace_anchor(state: &mut EpisodicState, old_anchor: &AnchorRef, replacements: &[AnchorRef]) {
    let Some(memory_ids) = state.anchor_index.get(old_anchor).cloned() else {
        return;
    };

    for memory_id in memory_ids {
        let Some(entry) = state.entries.get_mut(&memory_id) else {
            continue;
        };
        let old_anchors = entry.anchors.clone();
        entry.anchors.retain(|anchor| anchor != old_anchor);
        entry.anchors.extend(replacements.iter().cloned());
        entry.anchors = dedupe_anchors(entry.anchors.clone());
        let new_anchors = entry.anchors.clone();
        let empty = new_anchors.is_empty();
        let _ = entry;
        if empty {
            remove_memory(state, &memory_id);
        } else {
            reindex_memory(state, &memory_id, &old_anchors, &new_anchors);
        }
    }
}

fn reindex_memory(
    state: &mut EpisodicState,
    memory_id: &MemoryId,
    old_anchors: &[AnchorRef],
    new_anchors: &[AnchorRef],
) {
    let old_set = old_anchors.iter().cloned().collect::<HashSet<_>>();
    let new_set = new_anchors.iter().cloned().collect::<HashSet<_>>();

    for removed in old_set.difference(&new_set) {
        if let Some(ids) = state.anchor_index.get_mut(removed) {
            ids.remove(memory_id);
            if ids.is_empty() {
                state.anchor_index.remove(removed);
            }
        }
    }

    for added in new_set.difference(&old_set) {
        state
            .anchor_index
            .entry(added.clone())
            .or_default()
            .insert(memory_id.clone());
    }
}

fn apply_outcome_reanchor_event(
    state: &mut OutcomeState,
    before: &[NodeId],
    after: &[NodeId],
    lineage_anchor: &AnchorRef,
) {
    if before.len() == 1 && after.len() == 1 {
        replace_outcome_anchor(
            state,
            &AnchorRef::Node(before[0].clone()),
            &[AnchorRef::Node(after[0].clone()), lineage_anchor.clone()],
        );
        return;
    }

    for previous in before {
        replace_outcome_anchor(
            state,
            &AnchorRef::Node(previous.clone()),
            &[lineage_anchor.clone()],
        );
    }

    for next in after {
        add_outcome_anchor_to_matching_lineage(state, lineage_anchor, next);
    }
}

fn add_outcome_anchor_to_matching_lineage(
    state: &mut OutcomeState,
    lineage_anchor: &AnchorRef,
    node: &NodeId,
) {
    let Some(event_ids) = state.anchor_index.get(lineage_anchor).cloned() else {
        return;
    };

    let new_anchor = AnchorRef::Node(node.clone());
    for event_id in event_ids {
        let Some(event) = state.events.get_mut(&event_id) else {
            continue;
        };
        let old_anchors = event.anchors.clone();
        event.anchors.push(new_anchor.clone());
        event.anchors = dedupe_anchors(event.anchors.clone());
        let new_anchors = event.anchors.clone();
        let _ = event;
        reindex_outcome(state, &event_id, &old_anchors, &new_anchors);
    }
}

fn replace_outcome_anchor(
    state: &mut OutcomeState,
    old_anchor: &AnchorRef,
    replacements: &[AnchorRef],
) {
    let Some(event_ids) = state.anchor_index.get(old_anchor).cloned() else {
        return;
    };

    for event_id in event_ids {
        let Some(event) = state.events.get_mut(&event_id) else {
            continue;
        };
        let old_anchors = event.anchors.clone();
        event.anchors.retain(|anchor| anchor != old_anchor);
        event.anchors.extend(replacements.iter().cloned());
        event.anchors = dedupe_anchors(event.anchors.clone());
        let new_anchors = event.anchors.clone();
        let empty = new_anchors.is_empty();
        let _ = event;
        if empty {
            remove_outcome_event(state, &event_id);
        } else {
            reindex_outcome(state, &event_id, &old_anchors, &new_anchors);
        }
    }
}

fn reindex_outcome(
    state: &mut OutcomeState,
    event_id: &EventId,
    old_anchors: &[AnchorRef],
    new_anchors: &[AnchorRef],
) {
    let old_set = old_anchors.iter().cloned().collect::<HashSet<_>>();
    let new_set = new_anchors.iter().cloned().collect::<HashSet<_>>();

    for removed in old_set.difference(&new_set) {
        if let Some(ids) = state.anchor_index.get_mut(removed) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.anchor_index.remove(removed);
            }
        }
    }

    for added in new_set.difference(&old_set) {
        state
            .anchor_index
            .entry(added.clone())
            .or_default()
            .insert(event_id.clone());
    }
}

fn remove_outcome_event(state: &mut OutcomeState, event_id: &EventId) {
    let Some(event) = state.events.remove(event_id) else {
        return;
    };

    for anchor in event.anchors {
        if let Some(ids) = state.anchor_index.get_mut(&anchor) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.anchor_index.remove(&anchor);
            }
        }
    }

    if let Some(task) = event.meta.correlation {
        if let Some(ids) = state.task_index.get_mut(&task) {
            ids.remove(event_id);
            if ids.is_empty() {
                state.task_index.remove(&task);
            }
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

fn dedupe_anchors(anchors: Vec<AnchorRef>) -> Vec<AnchorRef> {
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
    use prism_ir::{
        EventActor, EventId, EventMeta, LineageEvent, LineageEventKind, LineageEvidence, LineageId,
        NodeKind,
    };
    use serde_json::json;

    use super::*;

    fn node(name: &str) -> NodeId {
        NodeId::new("demo", format!("demo::{name}"), NodeKind::Function)
    }

    fn anchor_node(name: &str) -> AnchorRef {
        AnchorRef::Node(node(name))
    }

    fn lineage(name: &str) -> LineageId {
        LineageId::new(format!("lineage::{name}"))
    }

    fn lineage_event(
        lineage: LineageId,
        kind: LineageEventKind,
        before: Vec<NodeId>,
        after: Vec<NodeId>,
    ) -> LineageEvent {
        LineageEvent {
            meta: EventMeta {
                id: EventId::new("event:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            lineage,
            kind,
            before,
            after,
            confidence: 1.0,
            evidence: vec![LineageEvidence::FingerprintMatch],
        }
    }

    #[test]
    fn episodic_memory_generates_store_owned_ids() {
        let memory = EpisodicMemory::new();
        let mut entry = MemoryEntry::new(
            MemoryKind::Episodic,
            "Function alpha changed in commit abc123",
        );
        entry.anchors = vec![anchor_node("alpha")];
        entry.source = MemorySource::User;
        entry.trust = 1.0;

        let id = memory.store(entry).unwrap();

        assert_eq!(id.0, "episodic:1");
    }

    #[test]
    fn episodic_snapshot_round_trip_preserves_ids() {
        let memory = EpisodicMemory::new();
        let mut entry = MemoryEntry::new(MemoryKind::Episodic, "alpha needed a follow-up fix");
        entry.anchors = vec![anchor_node("alpha")];
        entry.created_at = 42;
        let id = memory.store(entry).unwrap();

        let restored = EpisodicMemory::from_snapshot(memory.snapshot());
        let results = restored
            .recall(&RecallQuery {
                focus: vec![anchor_node("alpha")],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert_eq!(results[0].entry.created_at, 42);
    }

    #[test]
    fn recall_uses_anchor_overlap_and_since_filter() {
        let memory = EpisodicMemory::new();

        let mut alpha = MemoryEntry::new(
            MemoryKind::Episodic,
            "Bug report mentioned alpha null handling",
        );
        alpha.anchors = vec![anchor_node("alpha"), anchor_node("beta")];
        alpha.created_at = 1_000;
        alpha.source = MemorySource::User;
        alpha.trust = 1.0;
        alpha.metadata = json!({"issue": "BUG-1"});
        memory.store(alpha).unwrap();

        let mut beta = MemoryEntry::new(
            MemoryKind::Episodic,
            "User noted beta is performance sensitive",
        );
        beta.anchors = vec![anchor_node("beta")];
        beta.created_at = 2_000;
        beta.source = MemorySource::System;
        beta.trust = 0.8;
        memory.store(beta).unwrap();

        let results = memory
            .recall(&RecallQuery {
                focus: vec![anchor_node("beta")],
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
    fn lineage_reanchoring_moves_memory_to_new_node_id_and_adds_lineage_anchor() {
        let memory = EpisodicMemory::new();
        let old = node("alpha");
        let new = node("renamed_alpha");
        let symbol_lineage = lineage("alpha");

        let mut entry = MemoryEntry::new(
            MemoryKind::Episodic,
            "Function alpha changed in commit abc123",
        );
        entry.anchors = vec![AnchorRef::Node(old.clone())];
        memory.store(entry).unwrap();

        memory
            .apply_lineage(&[lineage_event(
                symbol_lineage.clone(),
                LineageEventKind::Renamed,
                vec![old.clone()],
                vec![new.clone()],
            )])
            .unwrap();

        let old_results = memory
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Node(old)],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();
        let new_results = memory
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Node(new.clone())],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();
        let lineage_results = memory
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Lineage(symbol_lineage)],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert!(old_results.is_empty());
        assert_eq!(new_results.len(), 1);
        assert_eq!(lineage_results.len(), 1);
        assert!(lineage_results[0]
            .entry
            .anchors
            .contains(&AnchorRef::Node(new)));
    }

    #[test]
    fn died_lineage_preserves_memory_via_lineage_anchor() {
        let memory = EpisodicMemory::new();
        let alpha = node("alpha");
        let symbol_lineage = lineage("alpha");

        let mut entry = MemoryEntry::new(MemoryKind::Episodic, "User noted alpha is sensitive");
        entry.anchors = vec![AnchorRef::Node(alpha.clone())];
        memory.store(entry).unwrap();

        memory
            .apply_lineage(&[lineage_event(
                symbol_lineage.clone(),
                LineageEventKind::Died,
                vec![alpha.clone()],
                Vec::new(),
            )])
            .unwrap();

        let removed_results = memory
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Node(alpha)],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();
        let lineage_results = memory
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Lineage(symbol_lineage)],
                text: None,
                limit: 10,
                kinds: None,
                since: None,
            })
            .unwrap();

        assert!(removed_results.is_empty());
        assert_eq!(lineage_results.len(), 1);
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

        fn apply_lineage(&self, _events: &[LineageEvent]) -> Result<()> {
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
