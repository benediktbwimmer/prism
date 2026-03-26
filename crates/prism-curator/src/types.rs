use anyhow::Result;
use prism_agent::InferredEdgeScope;
use prism_ir::{AnchorRef, Edge, EventId, LineageEvent, LineageId, Node, NodeId, TaskId};
use prism_memory::{MemoryEntry, MemoryKind, OutcomeEvent};
use prism_projections::{CoChangeRecord, ValidationCheck};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratorJobId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorTrigger {
    Manual,
    PostChange,
    TaskCompleted,
    RepeatedFailure,
    AmbiguousLineage,
    HotspotChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratorBudget {
    pub max_input_bytes: usize,
    pub max_context_nodes: usize,
    pub max_outcomes: usize,
    pub max_memories: usize,
    pub max_proposals: usize,
}

impl Default for CuratorBudget {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024,
            max_context_nodes: 128,
            max_outcomes: 64,
            max_memories: 32,
            max_proposals: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorJob {
    pub id: CuratorJobId,
    pub trigger: CuratorTrigger,
    pub task: Option<TaskId>,
    pub focus: Vec<AnchorRef>,
    pub budget: CuratorBudget,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorGraphSlice {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorLineageSlice {
    pub events: Vec<LineageEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorProjectionSlice {
    pub co_change: Vec<CoChangeRecord>,
    pub validation_checks: Vec<ValidationCheck>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorContext {
    pub graph: CuratorGraphSlice,
    pub lineage: CuratorLineageSlice,
    pub outcomes: Vec<OutcomeEvent>,
    pub memories: Vec<MemoryEntry>,
    pub projections: CuratorProjectionSlice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateEdge {
    pub edge: Edge,
    pub scope: InferredEdgeScope,
    pub evidence: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateMemoryEvidence {
    #[serde(default)]
    pub event_ids: Vec<EventId>,
    #[serde(default)]
    pub validation_checks: Vec<String>,
    #[serde(default)]
    pub co_change_lineages: Vec<LineageId>,
}

impl Default for CandidateMemoryEvidence {
    fn default() -> Self {
        Self {
            event_ids: Vec::new(),
            validation_checks: Vec::new(),
            co_change_lineages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateMemory {
    pub anchors: Vec<AnchorRef>,
    pub kind: MemoryKind,
    pub content: String,
    pub trust: f32,
    pub rationale: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub evidence: CandidateMemoryEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateRiskSummary {
    pub anchors: Vec<AnchorRef>,
    pub summary: String,
    pub severity: String,
    pub evidence_events: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateValidationRecipe {
    pub target: NodeId,
    pub checks: Vec<String>,
    pub rationale: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CuratorProposal {
    InferredEdge(CandidateEdge),
    StructuralMemory(CandidateMemory),
    SemanticMemory(CandidateMemory),
    RiskSummary(CandidateRiskSummary),
    ValidationRecipe(CandidateValidationRecipe),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorRun {
    pub proposals: Vec<CuratorProposal>,
    pub diagnostics: Vec<CuratorDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorProposalDisposition {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorProposalState {
    pub disposition: CuratorProposalDisposition,
    pub decided_at: Option<u64>,
    pub task: Option<TaskId>,
    pub note: Option<String>,
    pub output: Option<String>,
}

impl Default for CuratorProposalState {
    fn default() -> Self {
        Self {
            disposition: CuratorProposalDisposition::Pending,
            decided_at: None,
            task: None,
            note: None,
            output: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorJobRecord {
    pub id: CuratorJobId,
    pub job: CuratorJob,
    pub status: CuratorJobStatus,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub run: Option<CuratorRun>,
    #[serde(default)]
    pub proposal_states: Vec<CuratorProposalState>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorSnapshot {
    pub records: Vec<CuratorJobRecord>,
}

pub trait CuratorBackend: Send + Sync {
    fn run(&self, job: &CuratorJob, ctx: &CuratorContext) -> Result<CuratorRun>;
}
