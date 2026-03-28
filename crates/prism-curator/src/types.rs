use anyhow::Result;
use prism_agent::InferredEdgeScope;
use prism_ir::{AnchorRef, Edge, EventId, LineageEvent, LineageId, Node, NodeId, TaskId};
use prism_memory::{MemoryEntry, MemoryId, MemoryKind, OutcomeEvent};
use prism_projections::{CoChangeRecord, ValidationCheck};
use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

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
    pub memory_ids: Vec<MemoryId>,
    #[serde(default)]
    pub validation_checks: Vec<String>,
    #[serde(default)]
    pub co_change_lineages: Vec<LineageId>,
}

impl Default for CandidateMemoryEvidence {
    fn default() -> Self {
        Self {
            event_ids: Vec::new(),
            memory_ids: Vec::new(),
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

#[derive(Debug, Clone, PartialEq)]
pub enum CuratorProposal {
    InferredEdge(CandidateEdge),
    StructuralMemory(CandidateMemory),
    SemanticMemory(CandidateMemory),
    RiskSummary(CandidateRiskSummary),
    ValidationRecipe(CandidateValidationRecipe),
}

impl Serialize for CuratorProposal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::InferredEdge(edge) => serialize_tagged_object("inferred_edge", edge, &mut map)?,
            Self::StructuralMemory(memory) => {
                serialize_tagged_memory("structural_memory", memory, &mut map)?
            }
            Self::SemanticMemory(memory) => {
                serialize_tagged_memory("semantic_memory", memory, &mut map)?
            }
            Self::RiskSummary(summary) => {
                serialize_tagged_object("risk_summary", summary, &mut map)?
            }
            Self::ValidationRecipe(recipe) => {
                serialize_tagged_object("validation_recipe", recipe, &mut map)?
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CuratorProposal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(CuratorProposalVisitor)
    }
}

fn serialize_tagged_object<S, T>(tag: &'static str, value: &T, map: &mut S) -> Result<(), S::Error>
where
    S: SerializeMap,
    T: Serialize,
{
    map.serialize_entry("kind", tag)?;
    let serde_json::Value::Object(object) =
        serde_json::to_value(value).map_err(serde::ser::Error::custom)?
    else {
        return Err(serde::ser::Error::custom(
            "curator proposal payload must serialize to an object",
        ));
    };
    for (key, value) in object {
        map.serialize_entry(&key, &value)?;
    }
    Ok(())
}

fn serialize_tagged_memory<S>(
    tag: &'static str,
    memory: &CandidateMemory,
    map: &mut S,
) -> Result<(), S::Error>
where
    S: SerializeMap,
{
    map.serialize_entry("kind", tag)?;
    let serde_json::Value::Object(mut object) =
        serde_json::to_value(memory).map_err(serde::ser::Error::custom)?
    else {
        return Err(serde::ser::Error::custom(
            "candidate memory must serialize to an object",
        ));
    };
    if let Some(kind) = object.remove("kind") {
        map.serialize_entry("memoryKind", &kind)?;
    }
    for (key, value) in object {
        map.serialize_entry(&key, &value)?;
    }
    Ok(())
}

struct CuratorProposalVisitor;

impl<'de> Visitor<'de> for CuratorProposalVisitor {
    type Value = CuratorProposal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a curator proposal object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut proposal_kind = None::<String>;
        let mut memory_kind = None::<MemoryKind>;
        let mut fields = serde_json::Map::new();

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "kind" => {
                    let raw = map.next_value::<String>()?;
                    if proposal_kind.is_none() && is_proposal_kind(&raw) {
                        proposal_kind = Some(raw);
                    } else {
                        memory_kind = Some(parse_memory_kind(&raw).map_err(de::Error::custom)?);
                    }
                }
                "memoryKind" | "memory_kind" => {
                    let raw = map.next_value::<String>()?;
                    memory_kind = Some(parse_memory_kind(&raw).map_err(de::Error::custom)?);
                }
                _ => {
                    let value = map.next_value::<serde_json::Value>()?;
                    fields.insert(key, value);
                }
            }
        }

        let proposal_kind = proposal_kind.ok_or_else(|| de::Error::missing_field("kind"))?;
        match proposal_kind.as_str() {
            "inferred_edge" => decode_payload(fields).map(CuratorProposal::InferredEdge),
            "risk_summary" => decode_payload(fields).map(CuratorProposal::RiskSummary),
            "validation_recipe" => decode_payload(fields).map(CuratorProposal::ValidationRecipe),
            "structural_memory" => {
                decode_memory_payload(fields, memory_kind.unwrap_or(MemoryKind::Structural))
                    .map(CuratorProposal::StructuralMemory)
            }
            "semantic_memory" => {
                decode_memory_payload(fields, memory_kind.unwrap_or(MemoryKind::Semantic))
                    .map(CuratorProposal::SemanticMemory)
            }
            other => Err(de::Error::unknown_variant(
                other,
                &[
                    "inferred_edge",
                    "structural_memory",
                    "semantic_memory",
                    "risk_summary",
                    "validation_recipe",
                ],
            )),
        }
    }
}

fn is_proposal_kind(value: &str) -> bool {
    matches!(
        value,
        "inferred_edge"
            | "structural_memory"
            | "semantic_memory"
            | "risk_summary"
            | "validation_recipe"
    )
}

fn parse_memory_kind(value: &str) -> Result<MemoryKind, serde_json::Error> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
}

fn decode_payload<T, E>(fields: serde_json::Map<String, serde_json::Value>) -> Result<T, E>
where
    T: for<'de> Deserialize<'de>,
    E: de::Error,
{
    serde_json::from_value(serde_json::Value::Object(fields)).map_err(E::custom)
}

fn decode_memory_payload<E>(
    mut fields: serde_json::Map<String, serde_json::Value>,
    memory_kind: MemoryKind,
) -> Result<CandidateMemory, E>
where
    E: de::Error,
{
    fields.insert(
        "kind".to_string(),
        serde_json::to_value(memory_kind).map_err(E::custom)?,
    );
    decode_payload(fields)
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
