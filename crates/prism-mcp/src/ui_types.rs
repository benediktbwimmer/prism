use prism_js::{
    AgentOutcomeSummaryView, ArtifactView, BlockerView, ClaimView, ConceptPacketView,
    CoordinationPlanV2View, CoordinationTaskV2View, CoordinationTaskView, NodeRefView,
    PlanListEntryView, PlanSummaryView, PolicyViolationRecordView, RuntimeLogEventView,
    RuntimeStatusView, TaskJournalView, ValidationRefView,
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
    pub(crate) ready_tasks: Vec<CoordinationTaskV2View>,
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
    pub(crate) children: Vec<NodeRefView>,
    pub(crate) child_plans: Vec<CoordinationPlanV2View>,
    pub(crate) child_tasks: Vec<CoordinationTaskV2View>,
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
pub(crate) struct PrismUiTaskDetailView {
    pub(crate) task: CoordinationTaskView,
    pub(crate) editable: PrismUiTaskEditableMetadataView,
    pub(crate) claim_history: Vec<PrismUiTaskClaimHistoryEntryView>,
    pub(crate) blockers: Vec<PrismUiTaskBlockerEntryView>,
    pub(crate) outcomes: Vec<AgentOutcomeSummaryView>,
    pub(crate) recent_commits: Vec<PrismUiTaskCommitView>,
    pub(crate) artifacts: Vec<ArtifactView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiTaskEditableMetadataView {
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) priority: Option<u8>,
    pub(crate) assignee: Option<String>,
    pub(crate) status: String,
    pub(crate) validation_refs: Vec<ValidationRefView>,
    pub(crate) validation_guidance: Vec<String>,
    pub(crate) status_options: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiTaskClaimHistoryEntryView {
    pub(crate) id: String,
    pub(crate) holder: String,
    pub(crate) agent: Option<String>,
    pub(crate) status: String,
    pub(crate) capability: String,
    pub(crate) mode: String,
    pub(crate) started_at: u64,
    pub(crate) refreshed_at: Option<u64>,
    pub(crate) stale_at: Option<u64>,
    pub(crate) expires_at: u64,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) branch_ref: Option<String>,
    pub(crate) worktree_id: Option<String>,
    pub(crate) claim: ClaimView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiTaskBlockerEntryView {
    pub(crate) blocker: BlockerView,
    pub(crate) related_task: Option<CoordinationTaskView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiTaskCommitView {
    pub(crate) kind: String,
    pub(crate) commit: String,
    pub(crate) reference: Option<String>,
    pub(crate) label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiFleetView {
    pub(crate) generated_at: u64,
    pub(crate) window_start: u64,
    pub(crate) window_end: u64,
    pub(crate) lanes: Vec<PrismUiFleetLaneView>,
    pub(crate) bars: Vec<PrismUiFleetBarView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiFleetLaneView {
    pub(crate) id: String,
    pub(crate) runtime_id: Option<String>,
    pub(crate) label: String,
    pub(crate) principal_id: Option<String>,
    pub(crate) worktree_id: Option<String>,
    pub(crate) branch_ref: Option<String>,
    pub(crate) discovery_mode: Option<String>,
    pub(crate) last_seen_at: Option<u64>,
    pub(crate) active_bar_count: usize,
    pub(crate) stale_bar_count: usize,
    pub(crate) idle: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrismUiFleetBarView {
    pub(crate) id: String,
    pub(crate) lane_id: String,
    pub(crate) runtime_id: Option<String>,
    pub(crate) task_id: Option<String>,
    pub(crate) task_title: String,
    pub(crate) task_status: String,
    pub(crate) claim_id: Option<String>,
    pub(crate) claim_status: Option<String>,
    pub(crate) holder: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) capability: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) branch_ref: Option<String>,
    pub(crate) started_at: u64,
    pub(crate) ended_at: Option<u64>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) active: bool,
    pub(crate) stale: bool,
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
