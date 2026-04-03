use prism_ir::{
    AgentId, ArtifactId, CoordinationTaskId, LineageId, NodeId, PlanEdgeId, PlanExecutionOverlay,
    PlanGraph, PlanId, PlanKind, PlanNode, PlanNodeBlocker, PlanNodeId, PlanNodeStatus, PlanScope,
    PlanStatus, Timestamp,
};
use prism_memory::OutcomeEvent;
use serde::{Deserialize, Serialize};

pub use prism_projections::ValidationCheck;
pub use prism_projections::{
    canonical_concept_handle, canonical_contract_handle, ConceptDecodeLens, ConceptEvent,
    ConceptEventAction, ConceptEventPatch, ConceptHealth, ConceptHealthSignals,
    ConceptHealthStatus, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent, ConceptRelationEventAction,
    ConceptRelationKind, ConceptScope, ContractCompatibility, ContractEvent, ContractEventAction,
    ContractEventPatch, ContractGuarantee, ContractGuaranteeStrength, ContractHealth,
    ContractHealthSignals, ContractHealthStatus, ContractKind, ContractPacket, ContractProvenance,
    ContractPublication, ContractPublicationStatus, ContractResolution, ContractScope,
    ContractStability, ContractStatus, ContractTarget, ContractValidation,
    ProjectionAuthorityPlane, ProjectionClass,
};

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
    pub contracts: Vec<ContractPacket>,
    pub contract_review_notes: Vec<String>,
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
    pub contracts: Vec<ContractPacket>,
    pub contract_review_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub plan_id: PlanId,
    pub status: PlanStatus,
    pub total_nodes: usize,
    pub completed_nodes: usize,
    pub abandoned_nodes: usize,
    pub in_progress_nodes: usize,
    pub actionable_nodes: usize,
    pub execution_blocked_nodes: usize,
    pub completion_gated_nodes: usize,
    pub review_gated_nodes: usize,
    pub validation_gated_nodes: usize,
    pub stale_nodes: usize,
    pub claim_conflicted_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanListEntry {
    pub plan_id: PlanId,
    pub title: String,
    pub goal: String,
    pub status: PlanStatus,
    pub scope: PlanScope,
    pub kind: PlanKind,
    pub policy: prism_coordination::CoordinationPolicy,
    pub scheduling: prism_coordination::PlanScheduling,
    pub root_node_ids: Vec<PlanNodeId>,
    pub summary: String,
    pub plan_summary: PlanSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanNodeRecommendation {
    pub node: PlanNode,
    pub actionable: bool,
    pub effective_assignee: Option<AgentId>,
    pub score: f32,
    pub reasons: Vec<String>,
    pub blockers: Vec<PlanNodeBlocker>,
    pub unblocks: Vec<PlanNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdHocPlanProjectionSummary {
    pub total_nodes: usize,
    pub abstract_nodes: usize,
    pub proposed_nodes: usize,
    pub ready_nodes: usize,
    pub waiting_nodes: usize,
    pub in_progress_nodes: usize,
    pub in_review_nodes: usize,
    pub validating_nodes: usize,
    pub blocked_nodes: usize,
    pub completed_nodes: usize,
    pub abandoned_nodes: usize,
    pub total_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdHocPlanProjection {
    pub projection_class: ProjectionClass,
    pub authority_planes: Vec<ProjectionAuthorityPlane>,
    pub history_source: String,
    pub plan_id: PlanId,
    pub as_of: Timestamp,
    pub replayed_event_count: usize,
    pub graph: PlanGraph,
    pub execution_overlays: Vec<PlanExecutionOverlay>,
    pub summary: AdHocPlanProjectionSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdHocPlanProjectionDiff {
    pub projection_class: ProjectionClass,
    pub authority_planes: Vec<ProjectionAuthorityPlane>,
    pub history_source: String,
    pub plan_id: PlanId,
    pub from: Timestamp,
    pub to: Timestamp,
    pub before: Option<AdHocPlanProjection>,
    pub after: Option<AdHocPlanProjection>,
    pub plan_metadata_changed: bool,
    pub added_nodes: Vec<PlanNodeId>,
    pub removed_nodes: Vec<PlanNodeId>,
    pub changed_nodes: Vec<PlanNodeId>,
    pub added_edges: Vec<PlanEdgeId>,
    pub removed_edges: Vec<PlanEdgeId>,
    pub changed_edges: Vec<PlanEdgeId>,
    pub changed_execution_nodes: Vec<PlanNodeId>,
}

pub(crate) fn ad_hoc_plan_projection_summary(graph: &PlanGraph) -> AdHocPlanProjectionSummary {
    let mut summary = AdHocPlanProjectionSummary {
        total_nodes: graph.nodes.len(),
        abstract_nodes: 0,
        proposed_nodes: 0,
        ready_nodes: 0,
        waiting_nodes: 0,
        in_progress_nodes: 0,
        in_review_nodes: 0,
        validating_nodes: 0,
        blocked_nodes: 0,
        completed_nodes: 0,
        abandoned_nodes: 0,
        total_edges: graph.edges.len(),
    };
    for node in &graph.nodes {
        if node.is_abstract {
            summary.abstract_nodes += 1;
        }
        match node.status {
            PlanNodeStatus::Proposed => summary.proposed_nodes += 1,
            PlanNodeStatus::Ready => summary.ready_nodes += 1,
            PlanNodeStatus::Waiting => summary.waiting_nodes += 1,
            PlanNodeStatus::InProgress => summary.in_progress_nodes += 1,
            PlanNodeStatus::InReview => summary.in_review_nodes += 1,
            PlanNodeStatus::Validating => summary.validating_nodes += 1,
            PlanNodeStatus::Blocked => summary.blocked_nodes += 1,
            PlanNodeStatus::Completed => summary.completed_nodes += 1,
            PlanNodeStatus::Abandoned => summary.abandoned_nodes += 1,
        }
    }
    summary
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
