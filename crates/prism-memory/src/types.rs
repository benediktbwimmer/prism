use anyhow::Result;
use prism_ir::{AnchorRef, EventMeta, LineageEvent, TaskId, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::current_timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MemoryId(pub String);

impl MemoryId {
    pub(crate) fn pending() -> Self {
        Self("pending".to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum MemoryKind {
    Episodic,
    Structural,
    Semantic,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    #[default]
    Local,
    Repo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum MemorySource {
    Agent,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub anchors: Vec<AnchorRef>,
    pub kind: MemoryKind,
    #[serde(default)]
    pub scope: MemoryScope,
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
            scope: MemoryScope::Local,
            content: content.into(),
            metadata: Value::Null,
            created_at: current_timestamp(),
            source: MemorySource::Agent,
            trust: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventKind {
    Stored,
    Promoted,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEvent {
    pub id: String,
    pub action: MemoryEventKind,
    pub memory_id: MemoryId,
    pub scope: MemoryScope,
    pub entry: Option<MemoryEntry>,
    pub recorded_at: Timestamp,
    pub task_id: Option<String>,
    pub promoted_from: Vec<MemoryId>,
    pub supersedes: Vec<MemoryId>,
}

impl MemoryEvent {
    pub fn from_entry(
        action: MemoryEventKind,
        entry: MemoryEntry,
        task_id: Option<String>,
        promoted_from: Vec<MemoryId>,
        supersedes: Vec<MemoryId>,
    ) -> Self {
        Self {
            id: next_memory_event_id(),
            action,
            memory_id: entry.id.clone(),
            scope: entry.scope,
            entry: Some(entry),
            recorded_at: current_timestamp(),
            task_id,
            promoted_from,
            supersedes,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEventQuery {
    pub memory_id: Option<MemoryId>,
    pub focus: Vec<AnchorRef>,
    pub text: Option<String>,
    pub limit: usize,
    pub kinds: Option<Vec<MemoryKind>>,
    pub actions: Option<Vec<MemoryEventKind>>,
    pub scope: Option<MemoryScope>,
    pub task_id: Option<String>,
    pub since: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecallQuery {
    pub focus: Vec<AnchorRef>,
    pub text: Option<String>,
    pub limit: usize,
    pub kinds: Option<Vec<MemoryKind>>,
    pub since: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoredMemory {
    pub id: MemoryId,
    pub entry: MemoryEntry,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum OutcomeResult {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum OutcomeEvidence {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

fn next_memory_event_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    format!("memory-event:{nanos}")
}
