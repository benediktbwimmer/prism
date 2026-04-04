use prism_js::{
    AgentOutcomeSummaryView, ArtifactView, ClaimView, ConceptPacketView, CoordinationTaskView,
    PlanExecutionOverlayView, PlanGraphView, PlanListEntryView, PlanNodeRecommendationView,
    PlanSummaryView, PolicyViolationRecordView, RuntimeLogEventView, RuntimeStatusView,
    TaskJournalView,
};
use serde::Serialize;

use crate::SessionView;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewSummaryView {
    pub(crate) session: SessionView,
    pub(crate) runtime: RuntimeStatusView,
    pub(crate) active_query_count: usize,
    pub(crate) active_mutation_count: usize,
    pub(crate) recent_query_error_count: usize,
    pub(crate) last_runtime_event: Option<RuntimeLogEventView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewTaskView {
    pub(crate) session: SessionView,
    pub(crate) journal: Option<TaskJournalView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewCoordinationView {
    pub(crate) enabled: bool,
    pub(crate) active_plan_count: usize,
    pub(crate) task_count: usize,
    pub(crate) ready_task_count: usize,
    pub(crate) in_review_task_count: usize,
    pub(crate) active_claim_count: usize,
    pub(crate) pending_handoff_count: usize,
    pub(crate) pending_review_count: usize,
    pub(crate) proposed_artifact_count: usize,
    pub(crate) recent_pending_reviews: Vec<ArtifactView>,
    pub(crate) recent_violations: Vec<PolicyViolationRecordView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewCoordinationQueuesView {
    pub(crate) enabled: bool,
    pub(crate) pending_handoffs: Vec<CoordinationTaskView>,
    pub(crate) active_claims: Vec<ClaimView>,
    pub(crate) pending_reviews: Vec<ArtifactView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismOverviewView {
    pub(crate) summary: PrismOverviewSummaryView,
    pub(crate) task: PrismOverviewTaskView,
    pub(crate) coordination: PrismOverviewCoordinationView,
    pub(crate) plan_signals: OverviewPlanSignalsView,
    pub(crate) spotlight_plans: Vec<OverviewPlanSpotlightView>,
    pub(crate) hot_concepts: Vec<OverviewConceptSpotlightView>,
    pub(crate) recent_outcomes: Vec<AgentOutcomeSummaryView>,
    pub(crate) pending_handoffs: Vec<CoordinationTaskView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiSessionBootstrapView {
    pub(crate) session: SessionView,
    pub(crate) runtime: RuntimeStatusView,
    pub(crate) polling_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiApiPlaceholderView {
    pub(crate) endpoint: String,
    pub(crate) status: String,
    pub(crate) message: String,
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
    pub(crate) filters: PrismUiPlansFiltersView,
    pub(crate) stats: PrismUiPlansStatsView,
    pub(crate) plans: Vec<PlanListEntryView>,
    pub(crate) selected_plan_id: Option<String>,
    pub(crate) selected_plan: Option<PrismPlanDetailView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiPlansFiltersView {
    pub(crate) status: String,
    pub(crate) search: Option<String>,
    pub(crate) sort: String,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiPlansStatsView {
    pub(crate) total_plans: usize,
    pub(crate) visible_plans: usize,
    pub(crate) active_plans: usize,
    pub(crate) completed_plans: usize,
    pub(crate) archived_plans: usize,
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
