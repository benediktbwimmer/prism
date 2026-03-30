use prism_js::{
    AgentOutcomeSummaryView, ArtifactView, ConceptPacketView, CoordinationTaskView,
    PlanExecutionOverlayView, PlanGraphView, PlanListEntryView, PlanNodeRecommendationView,
    PlanSummaryView, PolicyViolationRecordView,
};
use serde::Serialize;

use crate::dashboard_types::{
    DashboardCoordinationSummaryView, DashboardSummaryView, DashboardTaskSnapshotView,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewView {
    pub(crate) summary: DashboardSummaryView,
    pub(crate) task: DashboardTaskSnapshotView,
    pub(crate) coordination: DashboardCoordinationSummaryView,
    pub(crate) plan_signals: OverviewPlanSignalsView,
    pub(crate) spotlight_plans: Vec<OverviewPlanSpotlightView>,
    pub(crate) hot_concepts: Vec<OverviewConceptSpotlightView>,
    pub(crate) recent_outcomes: Vec<AgentOutcomeSummaryView>,
    pub(crate) pending_handoffs: Vec<CoordinationTaskView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverviewPlanSignalsView {
    pub(crate) blocked_nodes: usize,
    pub(crate) review_gated_nodes: usize,
    pub(crate) validation_gated_nodes: usize,
    pub(crate) claim_conflicted_nodes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverviewPlanSpotlightView {
    pub(crate) plan_id: String,
    pub(crate) title: String,
    pub(crate) goal: String,
    pub(crate) summary: PlanSummaryView,
    pub(crate) next_nodes: Vec<PlanNodeRecommendationView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverviewConceptSpotlightView {
    pub(crate) handle: String,
    pub(crate) canonical_name: String,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismPlansView {
    pub(crate) plans: Vec<PlanListEntryView>,
    pub(crate) selected_plan_id: Option<String>,
    pub(crate) selected_plan: Option<PrismPlanDetailView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismPlanDetailView {
    pub(crate) plan: PlanListEntryView,
    pub(crate) summary: PlanSummaryView,
    pub(crate) graph: PlanGraphView,
    pub(crate) execution: Vec<PlanExecutionOverlayView>,
    pub(crate) next_nodes: Vec<PlanNodeRecommendationView>,
    pub(crate) ready_tasks: Vec<CoordinationTaskView>,
    pub(crate) pending_reviews: Vec<ArtifactView>,
    pub(crate) pending_handoffs: Vec<CoordinationTaskView>,
    pub(crate) recent_violations: Vec<PolicyViolationRecordView>,
    pub(crate) recent_outcomes: Vec<AgentOutcomeSummaryView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismGraphView {
    pub(crate) selected_concept_handle: String,
    pub(crate) focus: ConceptPacketView,
    pub(crate) entry_concepts: Vec<ConceptPacketView>,
    pub(crate) related_plans: Vec<GraphPlanTouchpointView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphPlanTouchpointView {
    pub(crate) plan: PlanListEntryView,
    pub(crate) touched_nodes: Vec<GraphTouchedNodeView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphTouchedNodeView {
    pub(crate) node_id: String,
    pub(crate) title: String,
    pub(crate) status: String,
}
