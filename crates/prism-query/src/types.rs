use prism_ir::{ArtifactId, CoordinationTaskId, LineageId, NodeId};
use prism_memory::OutcomeEvent;
use serde::{Deserialize, Serialize};

pub use prism_projections::ValidationCheck;
pub use prism_projections::{ConceptDecodeLens, ConceptPacket};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryLimits {
    pub max_result_nodes: usize,
    pub max_call_graph_depth: usize,
    pub max_output_json_bytes: usize,
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            max_result_nodes: 500,
            max_call_graph_depth: 10,
            max_output_json_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChangeImpact {
    pub direct_nodes: Vec<NodeId>,
    pub lineages: Vec<LineageId>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoChange {
    pub lineage: LineageId,
    pub count: u32,
    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecipe {
    pub target: NodeId,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheck>,
    pub related_nodes: Vec<NodeId>,
    pub co_change_neighbors: Vec<CoChange>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskValidationRecipe {
    pub task_id: CoordinationTaskId,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheck>,
    pub related_nodes: Vec<NodeId>,
    pub co_change_neighbors: Vec<CoChange>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRisk {
    pub task_id: CoordinationTaskId,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale_task: bool,
    pub has_approved_artifact: bool,
    pub likely_validations: Vec<String>,
    pub missing_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
    pub approved_artifact_ids: Vec<ArtifactId>,
    pub stale_artifact_ids: Vec<ArtifactId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRisk {
    pub artifact_id: ArtifactId,
    pub task_id: CoordinationTaskId,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale: bool,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub missing_validations: Vec<String>,
    pub co_change_neighbors: Vec<CoChange>,
    pub risk_events: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriftCandidate {
    pub spec: NodeId,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub reasons: Vec<String>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskIntent {
    pub task_id: CoordinationTaskId,
    pub specs: Vec<NodeId>,
    pub implementations: Vec<NodeId>,
    pub validations: Vec<NodeId>,
    pub related: Vec<NodeId>,
    pub drift_candidates: Vec<DriftCandidate>,
}
