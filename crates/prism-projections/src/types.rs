use prism_ir::{LineageId, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeRecord {
    pub lineage: LineageId,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoChangeDelta {
    pub source_lineage: LineageId,
    pub target_lineage: LineageId,
    pub count_delta: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationDelta {
    pub lineage: LineageId,
    pub label: String,
    pub score_delta: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectionSnapshot {
    pub co_change_by_lineage: Vec<(LineageId, Vec<CoChangeRecord>)>,
    pub validation_by_lineage: Vec<(LineageId, Vec<ValidationCheck>)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentSpecProjection {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IntentDriftRecord {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub reasons: Vec<String>,
}
