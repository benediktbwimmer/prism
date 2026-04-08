use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::{anyhow, Result};
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, ready_task_count_for_active_plans,
    CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot, WorkClaim,
};
use prism_ir::{
    sortable_token_timestamp, ClaimStatus, CoordinationEventKind, CoordinationTaskId,
    CoordinationTaskStatus,
};
use prism_ir::{
    DerivedPlanStatus, EffectiveTaskStatus, NodeRefKind, PlanId, PlanScope, PlanStatus, TaskId,
};
use prism_memory::OutcomeRecallQuery;
use prism_query::PlanActivity;

use crate::coordination_executor::current_executor_caller;
use crate::ui_identity::ui_operator_identity_view;
use crate::ui_types::{
    GraphPlanTouchpointView, GraphTouchedNodeView, OverviewConceptSpotlightView,
    OverviewPlanSignalsView, OverviewPlanSpotlightView, PrismGraphView,
    PrismOverviewCoordinationQueuesView, PrismOverviewCoordinationView, PrismOverviewSummaryView,
    PrismOverviewTaskView, PrismOverviewView, PrismPlanDetailView, PrismPlansView,
    PrismUiApiPlaceholderView, PrismUiFleetBarView, PrismUiFleetLaneView, PrismUiFleetView,
    PrismUiPlansFiltersView, PrismUiPlansStatsView, PrismUiSessionBootstrapView,
    PrismUiTaskBlockerEntryView, PrismUiTaskClaimHistoryEntryView, PrismUiTaskCommitView,
    PrismUiTaskDetailView, PrismUiTaskEditableMetadataView,
};
use crate::views::{
    artifact_view, blocker_view, concept_packet_view, coordination_plan_v2_view,
    coordination_task_v2_view, git_execution_policy_view, node_ref_view, plan_activity_view,
    plan_list_entry_view, plan_node_status_counts_view, plan_scheduling_view, plan_summary_view,
    policy_violation_record_view, ConceptVerbosity,
};
use crate::{claim_view, coordination_task_view, current_timestamp, QueryHost, SessionState};
use crate::{host_resources::session_task_view, runtime_views::runtime_status};

const OVERVIEW_PLAN_LIMIT: usize = 3;
const OVERVIEW_PLAN_NEXT_LIMIT: usize = 2;
const OVERVIEW_CONCEPT_LIMIT: usize = 4;
const OVERVIEW_OUTCOME_LIMIT: usize = 6;
const OVERVIEW_HANDOFF_LIMIT: usize = 4;
const OVERVIEW_TEXT_MAX_CHARS: usize = 180;
const PLAN_DETAIL_CHILD_LIMIT: usize = 24;
const PLAN_DETAIL_READY_LIMIT: usize = 8;
const PLAN_DETAIL_REVIEW_LIMIT: usize = 8;
const PLAN_DETAIL_HANDOFF_LIMIT: usize = 6;
const PLAN_DETAIL_VIOLATION_LIMIT: usize = 6;
const PLAN_DETAIL_OUTCOME_LIMIT: usize = 8;
const PLAN_DETAIL_OUTCOMES_PER_TASK: usize = 2;
const GRAPH_ENTRY_LIMIT: usize = 8;
const GRAPH_PLAN_LIMIT: usize = 6;
const GRAPH_TOUCHED_NODE_LIMIT: usize = 4;
const GRAPH_DEFAULT_CONCEPT_HANDLE: &str = "concept://prism_architecture";
const OVERVIEW_TASK_EVENT_LIMIT: usize = 12;
const OVERVIEW_TASK_MEMORY_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_REVIEW_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_VIOLATION_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_HANDOFF_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_CLAIM_LIMIT: usize = 6;
const UI_FLEET_LOOKBACK_SECONDS: u64 = 7 * 24 * 60 * 60;
const UI_FLEET_BAR_LIMIT: usize = 256;
pub(crate) const UI_POLLING_INTERVAL_MS: u64 = 2_000;

#[derive(Debug, Clone, Default)]
pub(crate) struct UiPlansQueryOptions {
    pub(crate) selected_plan_id: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) search: Option<String>,
    pub(crate) sort: Option<String>,
    pub(crate) agent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiPlanStatusFilter {
    Active,
    Completed,
    Archived,
    Blocked,
    Abandoned,
    Draft,
    All,
}

impl Default for UiPlanStatusFilter {
    fn default() -> Self {
        Self::Active
    }
}

impl UiPlanStatusFilter {
    fn parse(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("all") => Self::All,
            Some("completed") => Self::Completed,
            Some("archived") => Self::Archived,
            Some("blocked") => Self::Blocked,
            Some("abandoned") => Self::Abandoned,
            Some("draft") => Self::Draft,
            Some("active") | None => Self::Active,
            _ => Self::Active,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Archived => "archived",
            Self::Blocked => "blocked",
            Self::Abandoned => "abandoned",
            Self::Draft => "draft",
            Self::All => "all",
        }
    }

    fn matches(self, status: PlanStatus) -> bool {
        match self {
            Self::Active => status == PlanStatus::Active,
            Self::Completed => status == PlanStatus::Completed,
            Self::Archived => status == PlanStatus::Archived,
            Self::Blocked => status == PlanStatus::Blocked,
            Self::Abandoned => status == PlanStatus::Abandoned,
            Self::Draft => status == PlanStatus::Draft,
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiPlanSort {
    Newest,
    Oldest,
    Priority,
    Actionable,
    Completion,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlansResourceSort {
    LastUpdatedDesc,
    LastUpdatedAsc,
    CreatedAtDesc,
    CreatedAtAsc,
    ProposedDesc,
    ReadyDesc,
    InProgressDesc,
    BlockedDesc,
    WaitingDesc,
    InReviewDesc,
    ValidatingDesc,
    CompletedDesc,
    AbandonedDesc,
}

impl Default for PlansResourceSort {
    fn default() -> Self {
        Self::LastUpdatedDesc
    }
}

impl PlansResourceSort {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("last_updated_asc") | Some("updated_asc") => Self::LastUpdatedAsc,
            Some("last_updated_desc")
            | Some("last_updated")
            | Some("updated")
            | Some("updated_desc")
            | None => Self::LastUpdatedDesc,
            Some("created_at_asc") | Some("created_asc") => Self::CreatedAtAsc,
            Some("created_at_desc")
            | Some("created_at")
            | Some("created")
            | Some("created_desc") => Self::CreatedAtDesc,
            Some("proposed_desc") | Some("proposed") => Self::ProposedDesc,
            Some("ready_desc") | Some("ready") => Self::ReadyDesc,
            Some("in_progress_desc") | Some("in_progress") => Self::InProgressDesc,
            Some("blocked_desc") | Some("blocked") => Self::BlockedDesc,
            Some("waiting_desc") | Some("waiting") => Self::WaitingDesc,
            Some("in_review_desc") | Some("in_review") => Self::InReviewDesc,
            Some("validating_desc") | Some("validating") => Self::ValidatingDesc,
            Some("completed_desc") | Some("completed") => Self::CompletedDesc,
            Some("abandoned_desc") | Some("abandoned") => Self::AbandonedDesc,
            _ => Self::LastUpdatedDesc,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LastUpdatedDesc => "last_updated_desc",
            Self::LastUpdatedAsc => "last_updated_asc",
            Self::CreatedAtDesc => "created_at_desc",
            Self::CreatedAtAsc => "created_at_asc",
            Self::ProposedDesc => "proposed_desc",
            Self::ReadyDesc => "ready_desc",
            Self::InProgressDesc => "in_progress_desc",
            Self::BlockedDesc => "blocked_desc",
            Self::WaitingDesc => "waiting_desc",
            Self::InReviewDesc => "in_review_desc",
            Self::ValidatingDesc => "validating_desc",
            Self::CompletedDesc => "completed_desc",
            Self::AbandonedDesc => "abandoned_desc",
        }
    }
}

impl Default for UiPlanSort {
    fn default() -> Self {
        Self::Newest
    }
}

impl UiPlanSort {
    fn parse(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("oldest") => Self::Oldest,
            Some("priority") => Self::Priority,
            Some("actionable") => Self::Actionable,
            Some("completion") => Self::Completion,
            Some("title") => Self::Title,
            Some("newest") | None => Self::Newest,
            _ => Self::Newest,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Oldest => "oldest",
            Self::Priority => "priority",
            Self::Actionable => "actionable",
            Self::Completion => "completion",
            Self::Title => "title",
        }
    }
}

pub(crate) trait QueryHostUiReadModelsExt {
    fn ui_session_bootstrap_view(&self) -> Result<PrismUiSessionBootstrapView>;
    fn ui_overview_view(&self) -> Result<PrismOverviewView>;
    fn ui_plans_view(&self, options: UiPlansQueryOptions) -> Result<PrismPlansView>;
    fn ui_concept_entrypoints_view(&self) -> Result<Vec<OverviewConceptSpotlightView>>;
    fn ui_graph_view(&self, selected_concept_handle: Option<&str>) -> Result<PrismGraphView>;
    fn ui_plan_detail_view(&self, plan_id: &str) -> Result<Option<PrismPlanDetailView>>;
    fn ui_task_detail_view(&self, task_id: &str) -> Result<Option<PrismUiTaskDetailView>>;
    fn ui_fleet_view(&self) -> Result<PrismUiFleetView>;
    fn ui_placeholder_view(&self, endpoint: &str, message: &str) -> PrismUiApiPlaceholderView;
}

impl QueryHostUiReadModelsExt for QueryHost {
    fn ui_session_bootstrap_view(&self) -> Result<PrismUiSessionBootstrapView> {
        Ok(PrismUiSessionBootstrapView {
            session: ui_session_view(self, None),
            runtime: runtime_status(self)?,
            polling_interval_ms: UI_POLLING_INTERVAL_MS,
        })
    }

    fn ui_overview_view(&self) -> Result<PrismOverviewView> {
        let summary = ui_overview_summary_view(self)?;
        let task = ui_overview_task_view(self, None)?;
        let coordination = ui_overview_coordination_summary(self)?;
        let coordination_queues = ui_overview_coordination_queues(self)?;
        let prism = self.current_prism();

        let now = crate::current_timestamp();
        let mut plan_spotlights = prism
            .root_plans_v2()
            .into_iter()
            .filter(|plan| compatibility_plan_status(plan.status) == PlanStatus::Active)
            .filter_map(|plan| {
                let summary = prism.plan_summary(&plan.plan.id)?;
                let ready_tasks = prism
                    .ready_tasks(&plan.plan.id, now)
                    .into_iter()
                    .filter_map(|task| {
                        prism
                            .coordination_task_v2(&TaskId::new(task.id.0.clone()))
                            .map(coordination_task_v2_view)
                    })
                    .take(OVERVIEW_PLAN_NEXT_LIMIT)
                    .collect::<Vec<_>>();
                Some(OverviewPlanSpotlightView {
                    plan_id: plan.plan.id.0.to_string(),
                    title: plan.plan.title.clone(),
                    goal: plan.plan.goal,
                    summary: plan_summary_view(summary),
                    ready_tasks,
                })
            })
            .collect::<Vec<_>>();

        plan_spotlights.sort_by(|left, right| {
            right
                .summary
                .in_progress_nodes
                .cmp(&left.summary.in_progress_nodes)
                .then_with(|| {
                    right
                        .summary
                        .execution_blocked_nodes
                        .cmp(&left.summary.execution_blocked_nodes)
                })
                .then_with(|| {
                    right
                        .summary
                        .actionable_nodes
                        .cmp(&left.summary.actionable_nodes)
                })
                .then_with(|| left.plan_id.cmp(&right.plan_id))
        });
        plan_spotlights.truncate(OVERVIEW_PLAN_LIMIT);

        let plan_signals = OverviewPlanSignalsView {
            blocked_nodes: plan_spotlights
                .iter()
                .map(|plan| plan.summary.execution_blocked_nodes)
                .sum(),
            review_gated_nodes: plan_spotlights
                .iter()
                .map(|plan| plan.summary.review_gated_nodes)
                .sum(),
            validation_gated_nodes: plan_spotlights
                .iter()
                .map(|plan| plan.summary.validation_gated_nodes)
                .sum(),
            claim_conflicted_nodes: plan_spotlights
                .iter()
                .map(|plan| plan.summary.claim_conflicted_nodes)
                .sum(),
        };

        let mut seen_concepts = HashSet::<String>::new();
        let hot_concepts = plan_spotlights
            .iter()
            .flat_map(|plan| plan.ready_tasks.iter())
            .flat_map(|task| task.bindings.concept_handles.iter())
            .filter(|handle| seen_concepts.insert((*handle).clone()))
            .take(OVERVIEW_CONCEPT_LIMIT)
            .filter_map(|handle| {
                prism
                    .concept_by_handle(handle)
                    .map(|packet| OverviewConceptSpotlightView {
                        handle: packet.handle,
                        canonical_name: packet.canonical_name,
                        summary: clamp_overview_text(&packet.summary),
                    })
            })
            .collect::<Vec<_>>();

        let recent_outcomes = prism
            .query_outcomes(&OutcomeRecallQuery {
                limit: OVERVIEW_OUTCOME_LIMIT,
                ..OutcomeRecallQuery::default()
            })
            .into_iter()
            .map(|event| prism_js::AgentOutcomeSummaryView {
                ts: event.meta.ts,
                kind: format!("{:?}", event.kind),
                result: format!("{:?}", event.result),
                summary: clamp_overview_text(&event.summary),
            })
            .collect::<Vec<_>>();

        let pending_handoffs = coordination_queues
            .pending_handoffs
            .into_iter()
            .take(OVERVIEW_HANDOFF_LIMIT)
            .collect::<Vec<_>>();

        Ok(PrismOverviewView {
            summary,
            task,
            coordination,
            plan_signals,
            spotlight_plans: plan_spotlights,
            hot_concepts,
            recent_outcomes,
            pending_handoffs,
        })
    }

    fn ui_plans_view(&self, options: UiPlansQueryOptions) -> Result<PrismPlansView> {
        let prism = self.current_prism();
        let all_plans = prism
            .plans(None, None, None)
            .into_iter()
            .map(plan_list_entry_view)
            .collect::<Vec<_>>();
        let status_filter = UiPlanStatusFilter::parse(options.status.as_deref());
        let sort = UiPlanSort::parse(options.sort.as_deref());
        let search = options
            .search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let agent = options
            .agent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let stats = PrismUiPlansStatsView {
            total_plans: all_plans.len(),
            visible_plans: 0,
            active_plans: all_plans
                .iter()
                .filter(|plan| plan.status == PlanStatus::Active)
                .count(),
            completed_plans: all_plans
                .iter()
                .filter(|plan| plan.status == PlanStatus::Completed)
                .count(),
            archived_plans: all_plans
                .iter()
                .filter(|plan| plan.status == PlanStatus::Archived)
                .count(),
        };
        let mut plans = all_plans
            .into_iter()
            .filter(|plan| status_filter.matches(plan.status))
            .filter(|plan| {
                search
                    .as_deref()
                    .map(|query| plan_matches_search(plan, query))
                    .unwrap_or(true)
            })
            .filter(|plan| {
                agent
                    .as_deref()
                    .map(|query| plan_matches_agent(&prism, plan, query))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        sort_plan_entries(&mut plans, sort);

        let selected_plan_id = options
            .selected_plan_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| plans.iter().any(|plan| plan.plan_id == *value))
            .map(str::to_string)
            .or_else(|| plans.first().map(|plan| plan.plan_id.clone()));
        let selected_plan = match selected_plan_id.as_deref() {
            Some(plan_id) => build_plan_detail_view(self, &prism, &plans, plan_id)?,
            None => None,
        };

        Ok(PrismPlansView {
            filters: PrismUiPlansFiltersView {
                status: status_filter.as_str().to_string(),
                search,
                sort: sort.as_str().to_string(),
                agent,
            },
            stats: PrismUiPlansStatsView {
                visible_plans: plans.len(),
                ..stats
            },
            plans,
            selected_plan_id,
            selected_plan,
        })
    }

    fn ui_concept_entrypoints_view(&self) -> Result<Vec<OverviewConceptSpotlightView>> {
        let prism = self.current_prism();
        let mut concepts = prism
            .curated_concepts_snapshot()
            .into_iter()
            .map(|packet| OverviewConceptSpotlightView {
                handle: packet.handle,
                canonical_name: packet.canonical_name,
                summary: packet.summary,
            })
            .collect::<Vec<_>>();
        concepts.sort_by(|left, right| {
            left.canonical_name
                .cmp(&right.canonical_name)
                .then_with(|| left.handle.cmp(&right.handle))
        });
        Ok(concepts)
    }

    fn ui_graph_view(&self, selected_concept_handle: Option<&str>) -> Result<PrismGraphView> {
        let prism = self.current_prism();
        let root_packet = prism
            .concept_by_handle(GRAPH_DEFAULT_CONCEPT_HANDLE)
            .or_else(|| prism.concept("prism architecture"))
            .ok_or_else(|| anyhow!("no architecture concept packet is available"))?;
        let root_handle = root_packet.handle.clone();
        let selected_concept_handle = selected_concept_handle
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| prism.concept_by_handle(value).is_some())
            .map(str::to_string)
            .unwrap_or_else(|| root_handle.clone());
        let focus_packet = if selected_concept_handle == root_handle {
            root_packet.clone()
        } else {
            prism
                .concept_by_handle(&selected_concept_handle)
                .unwrap_or_else(|| root_packet.clone())
        };
        let focus = concept_packet_view(
            &prism,
            focus_packet,
            ConceptVerbosity::Standard,
            false,
            None,
        );
        let entry_concepts = graph_entry_concepts(&prism, &root_packet);
        let related_plans = graph_plan_touchpoints(&prism, &selected_concept_handle);

        Ok(PrismGraphView {
            selected_concept_handle,
            focus,
            entry_concepts,
            related_plans,
        })
    }

    fn ui_plan_detail_view(&self, plan_id: &str) -> Result<Option<PrismPlanDetailView>> {
        let prism = self.current_prism();
        let plans = prism
            .plans(None, None, None)
            .into_iter()
            .map(plan_list_entry_view)
            .collect::<Vec<_>>();
        build_plan_detail_view(self, &prism, &plans, plan_id)
    }

    fn ui_task_detail_view(&self, task_id: &str) -> Result<Option<PrismUiTaskDetailView>> {
        let prism = self.current_prism();
        let now = crate::current_timestamp();
        let task_id = prism_ir::CoordinationTaskId::new(task_id.to_string());
        let (task_view, blockers) = if let Some(task) = prism.coordination_task(&task_id) {
            let blockers = prism
                .blockers(&task_id, now)
                .into_iter()
                .map(|blocker| {
                    let related_task = blocker
                        .related_task_id
                        .as_ref()
                        .and_then(|id| prism.coordination_task(id))
                        .map(coordination_task_view);
                    PrismUiTaskBlockerEntryView {
                        blocker: blocker_view(blocker),
                        related_task,
                    }
                })
                .collect::<Vec<_>>();
            (coordination_task_view(task.clone()), blockers)
        } else {
            return Ok(None);
        };
        let claim_history = prism
            .task_claim_history(&task_id, now)
            .into_iter()
            .map(task_claim_history_entry_view)
            .collect::<Vec<_>>();
        let outcomes = prism
            .query_outcomes(&OutcomeRecallQuery {
                task: Some(TaskId::new(task_view.id.clone())),
                limit: PLAN_DETAIL_OUTCOME_LIMIT,
                ..OutcomeRecallQuery::default()
            })
            .into_iter()
            .map(|event| prism_js::AgentOutcomeSummaryView {
                ts: event.meta.ts,
                kind: format!("{:?}", event.kind),
                result: format!("{:?}", event.result),
                summary: clamp_overview_text(&event.summary),
            })
            .collect::<Vec<_>>();
        let recent_commits = task_recent_commits(&task_view);
        let artifacts = prism
            .artifacts(&task_id)
            .into_iter()
            .map(artifact_view)
            .collect::<Vec<_>>();
        let validation_guidance = prism
            .task_validation_recipe(&task_id)
            .map(|recipe| recipe.checks)
            .unwrap_or_default();

        Ok(Some(PrismUiTaskDetailView {
            editable: PrismUiTaskEditableMetadataView {
                title: task_view.title.clone(),
                description: task_view.summary.clone(),
                priority: task_view.priority,
                assignee: task_view.assignee.clone(),
                status: format!("{:?}", task_view.status),
                validation_refs: task_view.validation_refs.clone(),
                validation_guidance,
                status_options: vec![
                    "proposed".to_string(),
                    "ready".to_string(),
                    "in_progress".to_string(),
                    "blocked".to_string(),
                    "in_review".to_string(),
                    "completed".to_string(),
                    "abandoned".to_string(),
                ],
            },
            task: task_view,
            claim_history,
            blockers,
            outcomes,
            recent_commits,
            artifacts,
        }))
    }

    fn ui_fleet_view(&self) -> Result<PrismUiFleetView> {
        let prism = self.current_prism();
        let snapshot = prism.coordination_snapshot();
        let now = current_timestamp();
        let window_start = now.saturating_sub(UI_FLEET_LOOKBACK_SECONDS);
        let shared_runtime_descriptors = runtime_status(self)
            .ok()
            .and_then(|runtime| runtime.shared_coordination_ref)
            .map(|shared| shared.runtime_descriptors)
            .unwrap_or_default();

        let task_by_id = snapshot
            .tasks
            .iter()
            .map(|task| (task.id.0.clone(), task.clone()))
            .collect::<BTreeMap<_, _>>();
        let claim_release_ts = snapshot
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    CoordinationEventKind::ClaimReleased | CoordinationEventKind::ClaimContended
                )
            })
            .filter_map(|event| {
                event
                    .claim
                    .as_ref()
                    .map(|claim| (claim.0.to_string(), event.meta.ts))
            })
            .fold(BTreeMap::<String, u64>::new(), |mut acc, (claim_id, ts)| {
                acc.entry(claim_id)
                    .and_modify(|existing| *existing = (*existing).max(ts))
                    .or_insert(ts);
                acc
            });

        let mut lanes = BTreeMap::<String, FleetLaneAccumulator>::new();
        for descriptor in shared_runtime_descriptors {
            let lane = fleet_lane_from_descriptor(&descriptor);
            lanes.insert(lane.id.clone(), lane);
        }

        let mut bars = snapshot
            .claims
            .iter()
            .filter_map(|claim| {
                fleet_bar_from_claim(
                    claim,
                    task_by_id.get(claim.task.as_ref()?.0.as_str()),
                    &claim_release_ts,
                    now,
                    window_start,
                    &mut lanes,
                )
            })
            .collect::<Vec<_>>();

        let claim_task_ids = snapshot
            .claims
            .iter()
            .filter_map(|claim| claim.task.as_ref().map(|task| task.0.clone()))
            .collect::<HashSet<_>>();
        bars.extend(
            snapshot
                .tasks
                .iter()
                .filter(|task| task.lease_started_at.is_some())
                .filter(|task| !claim_task_ids.contains(task.id.0.as_str()))
                .filter_map(|task| fleet_bar_from_task_lease(task, now, window_start, &mut lanes)),
        );

        bars.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        bars.truncate(UI_FLEET_BAR_LIMIT);

        let mut lanes = lanes
            .into_values()
            .map(|lane| lane.finish())
            .collect::<Vec<_>>();
        lanes.sort_by(|left, right| {
            right
                .active_bar_count
                .cmp(&left.active_bar_count)
                .then_with(|| right.last_seen_at.cmp(&left.last_seen_at))
                .then_with(|| left.label.cmp(&right.label))
        });

        Ok(PrismUiFleetView {
            generated_at: now,
            window_start,
            window_end: now,
            lanes,
            bars,
        })
    }

    fn ui_placeholder_view(&self, endpoint: &str, message: &str) -> PrismUiApiPlaceholderView {
        PrismUiApiPlaceholderView {
            endpoint: endpoint.to_string(),
            status: "not_implemented".to_string(),
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct FleetLaneAccumulator {
    id: String,
    runtime_id: Option<String>,
    label: String,
    principal_id: Option<String>,
    worktree_id: Option<String>,
    branch_ref: Option<String>,
    discovery_mode: Option<String>,
    last_seen_at: Option<u64>,
    active_bar_count: usize,
    stale_bar_count: usize,
}

impl FleetLaneAccumulator {
    fn finish(self) -> PrismUiFleetLaneView {
        PrismUiFleetLaneView {
            id: self.id,
            runtime_id: self.runtime_id,
            label: self.label,
            principal_id: self.principal_id,
            worktree_id: self.worktree_id,
            branch_ref: self.branch_ref,
            discovery_mode: self.discovery_mode,
            last_seen_at: self.last_seen_at,
            active_bar_count: self.active_bar_count,
            stale_bar_count: self.stale_bar_count,
            idle: self.active_bar_count == 0,
        }
    }
}

fn fleet_lane_from_descriptor(
    descriptor: &prism_js::RuntimeSharedCoordinationRuntimeDescriptorView,
) -> FleetLaneAccumulator {
    FleetLaneAccumulator {
        id: descriptor.runtime_id.clone(),
        runtime_id: Some(descriptor.runtime_id.clone()),
        label: fleet_lane_label(
            Some(descriptor.runtime_id.as_str()),
            descriptor.branch_ref.as_deref(),
            Some(descriptor.principal_id.as_str()),
            descriptor.worktree_id.as_str(),
        ),
        principal_id: Some(descriptor.principal_id.clone()),
        worktree_id: Some(descriptor.worktree_id.clone()),
        branch_ref: descriptor.branch_ref.clone(),
        discovery_mode: Some(format!("{:?}", descriptor.discovery_mode).to_ascii_lowercase()),
        last_seen_at: Some(descriptor.last_seen_at),
        active_bar_count: 0,
        stale_bar_count: 0,
    }
}

fn fleet_bar_from_claim(
    claim: &WorkClaim,
    task: Option<&prism_coordination::CoordinationTask>,
    claim_release_ts: &BTreeMap<String, u64>,
    now: u64,
    window_start: u64,
    lanes: &mut BTreeMap<String, FleetLaneAccumulator>,
) -> Option<PrismUiFleetBarView> {
    let active = claim.status == ClaimStatus::Active;
    let ended_at = if active {
        None
    } else {
        Some(
            claim_release_ts
                .get(claim.id.0.as_str())
                .copied()
                .or(claim.refreshed_at)
                .or(claim.stale_at)
                .or(Some(claim.expires_at))
                .unwrap_or(claim.since),
        )
    };
    if !active && ended_at.is_some_and(|ended| ended < window_start) {
        return None;
    }

    let lane_id = ensure_fleet_lane(
        lanes,
        claim
            .worktree_id
            .as_deref()
            .or(task.and_then(|task| task.worktree_id.as_deref())),
        claim
            .branch_ref
            .as_deref()
            .or(task.and_then(|task| task.branch_ref.as_deref())),
        Some(claim.holder.0.as_str()),
        claim.agent.as_ref().map(|agent| agent.0.as_str()),
    );
    let duration_end = ended_at.unwrap_or(now);
    let duration_seconds = duration_end.checked_sub(claim.since);
    let stale = !active
        && claim
            .stale_at
            .is_some_and(|stale_at| stale_at <= duration_end);
    if let Some(lane) = lanes.get_mut(lane_id.as_str()) {
        if active {
            lane.active_bar_count += 1;
        }
        if stale {
            lane.stale_bar_count += 1;
        }
    }
    let runtime_id = lanes
        .get(lane_id.as_str())
        .and_then(|lane| lane.runtime_id.clone());

    Some(PrismUiFleetBarView {
        id: claim.id.0.to_string(),
        lane_id,
        runtime_id,
        task_id: claim.task.as_ref().map(|task_id| task_id.0.to_string()),
        task_title: task
            .map(|task| task.title.clone())
            .unwrap_or_else(|| "Unscoped claim".to_string()),
        task_status: task
            .map(|task| format!("{:?}", task.status).to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        claim_id: Some(claim.id.0.to_string()),
        claim_status: Some(format!("{:?}", claim.status).to_ascii_lowercase()),
        holder: Some(claim.holder.0.to_string()),
        agent: claim.agent.as_ref().map(|agent| agent.0.to_string()),
        capability: Some(format!("{:?}", claim.capability).to_ascii_lowercase()),
        mode: Some(format!("{:?}", claim.mode).to_ascii_lowercase()),
        branch_ref: claim
            .branch_ref
            .clone()
            .or_else(|| task.and_then(|task| task.branch_ref.clone())),
        started_at: claim.since,
        ended_at,
        duration_seconds,
        active,
        stale,
    })
}

fn fleet_bar_from_task_lease(
    task: &prism_coordination::CoordinationTask,
    now: u64,
    window_start: u64,
    lanes: &mut BTreeMap<String, FleetLaneAccumulator>,
) -> Option<PrismUiFleetBarView> {
    let started_at = task.lease_started_at?;
    let active = matches!(task.status, CoordinationTaskStatus::InProgress);
    let ended_at = if active {
        None
    } else {
        task.lease_refreshed_at
            .or(task.lease_stale_at)
            .or(task.lease_expires_at)
    };
    if !active && ended_at.is_some_and(|ended| ended < window_start) {
        return None;
    }

    let lane_id = ensure_fleet_lane(
        lanes,
        task.worktree_id.as_deref(),
        task.branch_ref.as_deref(),
        task.session.as_ref().map(|session| session.0.as_str()),
        task.assignee.as_ref().map(|agent| agent.0.as_str()),
    );
    let duration_end = ended_at.unwrap_or(now);
    let stale = !active
        && task
            .lease_stale_at
            .is_some_and(|stale_at| stale_at <= duration_end);
    if let Some(lane) = lanes.get_mut(lane_id.as_str()) {
        if active {
            lane.active_bar_count += 1;
        }
        if stale {
            lane.stale_bar_count += 1;
        }
    }
    let runtime_id = lanes
        .get(lane_id.as_str())
        .and_then(|lane| lane.runtime_id.clone());

    Some(PrismUiFleetBarView {
        id: format!("lease:{}", task.id.0),
        lane_id,
        runtime_id,
        task_id: Some(task.id.0.to_string()),
        task_title: task.title.clone(),
        task_status: format!("{:?}", task.status).to_ascii_lowercase(),
        claim_id: None,
        claim_status: None,
        holder: task.session.as_ref().map(|session| session.0.to_string()),
        agent: task.assignee.as_ref().map(|agent| agent.0.to_string()),
        capability: None,
        mode: None,
        branch_ref: task.branch_ref.clone(),
        started_at,
        ended_at,
        duration_seconds: duration_end.checked_sub(started_at),
        active,
        stale,
    })
}

fn ensure_fleet_lane(
    lanes: &mut BTreeMap<String, FleetLaneAccumulator>,
    worktree_id: Option<&str>,
    branch_ref: Option<&str>,
    holder: Option<&str>,
    agent: Option<&str>,
) -> String {
    if let Some(worktree_id) = worktree_id {
        if let Some(existing) = lanes
            .values()
            .find(|lane| lane.worktree_id.as_deref() == Some(worktree_id))
            .map(|lane| lane.id.clone())
        {
            if let Some(lane) = lanes.get_mut(existing.as_str()) {
                if lane.branch_ref.is_none() {
                    lane.branch_ref = branch_ref.map(str::to_string);
                }
            }
            return existing;
        }
    }

    let id = worktree_id
        .map(|worktree_id| format!("worktree:{worktree_id}"))
        .or_else(|| branch_ref.map(|branch_ref| format!("branch:{branch_ref}")))
        .or_else(|| agent.map(|agent| format!("agent:{agent}")))
        .unwrap_or_else(|| "runtime:unknown".to_string());
    lanes
        .entry(id.clone())
        .or_insert_with(|| FleetLaneAccumulator {
            id: id.clone(),
            runtime_id: None,
            label: fleet_lane_label(None, branch_ref, agent, worktree_id.unwrap_or("unknown")),
            principal_id: holder.map(str::to_string),
            worktree_id: worktree_id.map(str::to_string),
            branch_ref: branch_ref.map(str::to_string),
            discovery_mode: None,
            last_seen_at: None,
            active_bar_count: 0,
            stale_bar_count: 0,
        });
    id
}

fn fleet_lane_label(
    runtime_id: Option<&str>,
    branch_ref: Option<&str>,
    principal_id: Option<&str>,
    worktree_id: &str,
) -> String {
    if let Some(branch_ref) = branch_ref {
        return runtime_id
            .map(|runtime_id| format!("{runtime_id} · {branch_ref}"))
            .unwrap_or_else(|| branch_ref.to_string());
    }
    if let Some(runtime_id) = runtime_id {
        return runtime_id.to_string();
    }
    principal_id
        .map(|principal| format!("{principal} · {worktree_id}"))
        .unwrap_or_else(|| worktree_id.to_string())
}

fn plan_matches_search(plan: &prism_js::PlanListEntryView, query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    [
        plan.title.as_str(),
        plan.goal.as_str(),
        plan.summary.as_str(),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(&query))
}

fn plan_matches_agent(
    prism: &prism_query::Prism,
    plan: &prism_js::PlanListEntryView,
    query: &str,
) -> bool {
    let query = query.to_ascii_lowercase();
    let plan_id = PlanId::new(plan.plan_id.clone());
    let snapshot = prism.coordination_snapshot_v2();
    let Ok(graph) = snapshot.graph() else {
        return false;
    };
    let task_ids = descendant_task_ids_for_plan(&graph, &plan_id);
    task_ids.into_iter().any(|task_id| {
        prism.coordination_task_v2(&task_id).is_some_and(|task| {
            [
                task.task.assignee.as_ref().map(|value| value.0.as_str()),
                task.task.session.as_ref().map(|value| value.0.as_str()),
                task.task.worktree_id.as_deref(),
                task.task.branch_ref.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| value.to_ascii_lowercase().contains(&query))
        })
    })
}

pub(crate) fn filtered_plan_entries_from_snapshot(
    snapshot: &CoordinationSnapshot,
    status: Option<PlanStatus>,
    scope: Option<PlanScope>,
    contains: Option<&str>,
    sort: PlansResourceSort,
) -> Vec<prism_js::PlanListEntryView> {
    let contains = contains
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let mut plans = plan_entries_from_snapshot(snapshot)
        .into_iter()
        .filter(|plan| status.is_none_or(|expected| plan.status == expected))
        .filter(|plan| scope.is_none_or(|expected| plan.scope == expected))
        .filter(|plan| {
            contains
                .as_deref()
                .is_none_or(|needle| plan_entry_matches_contains_filter(plan, needle))
        })
        .collect::<Vec<_>>();
    sort_plan_entries_for_resource(&mut plans, sort);
    plans
}

fn plan_entry_matches_contains_filter(plan: &prism_js::PlanListEntryView, needle: &str) -> bool {
    let id = plan.plan_id.to_ascii_lowercase();
    let title = plan.title.to_ascii_lowercase();
    let goal = plan.goal.to_ascii_lowercase();
    if id.contains(needle) || title.contains(needle) || goal.contains(needle) {
        return true;
    }

    let plan_terms = normalized_plan_terms(&format!("{id} {title} {goal}"));
    let query_terms = normalized_plan_terms(needle);
    !query_terms.is_empty()
        && query_terms
            .iter()
            .all(|term| plan_terms.contains(term.as_str()))
}

fn normalized_plan_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| normalize_plan_term(&token.to_ascii_lowercase()))
        .filter(|token| !token.is_empty())
        .collect()
}

fn normalize_plan_term(token: &str) -> String {
    if token.len() > 3 && token.ends_with("ies") {
        let mut stem = token[..token.len() - 3].to_string();
        stem.push('y');
        return stem;
    }
    if token.len() > 3
        && token.ends_with('s')
        && !token.ends_with("ss")
        && !token.ends_with("us")
        && !token.ends_with("is")
    {
        return token[..token.len() - 1].to_string();
    }
    token.to_string()
}

pub(crate) fn plan_entries_from_snapshot(
    snapshot: &CoordinationSnapshot,
) -> Vec<prism_js::PlanListEntryView> {
    let canonical_snapshot = snapshot.to_canonical_snapshot_v2();
    let Some(derivations) = canonical_snapshot.derive_statuses().ok() else {
        return Vec::new();
    };
    let Ok(graph) = canonical_snapshot.graph() else {
        return Vec::new();
    };
    let activity_by_plan = plan_activity_index_from_snapshot(snapshot);
    let legacy_status_by_plan = snapshot
        .plans
        .iter()
        .map(|plan| (plan.id.0.clone(), plan.status))
        .collect::<BTreeMap<_, _>>();
    canonical_snapshot
        .plans
        .iter()
        .filter_map(|plan| {
            let summary =
                plan_summary_from_canonical_snapshot(&canonical_snapshot, &derivations, &plan.id)?;
            let node_status_counts =
                canonical_plan_node_status_counts(&derivations, &graph, &plan.id);
            let activity = activity_by_plan
                .get(plan.id.0.as_str())
                .cloned()
                .unwrap_or_default();
            let activity = plan_activity_present(&activity).then(|| plan_activity_view(activity));
            let derived_status = derivations.plan_state(&plan.id)?;
            let status = legacy_status_by_plan
                .get(plan.id.0.as_str())
                .copied()
                .unwrap_or_else(|| compatibility_plan_status(derived_status.derived_status));
            Some(prism_js::PlanListEntryView {
                plan_id: plan.id.0.to_string(),
                title: plan.title.clone(),
                goal: plan.goal.clone(),
                status,
                scope: plan.scope,
                kind: plan.kind,
                scheduling: plan_scheduling_view(plan.scheduling.clone()),
                git_execution_policy: git_execution_policy_view(plan.policy.git_execution.clone()),
                created_at: activity.as_ref().and_then(|view| view.created_at),
                last_updated_at: activity.as_ref().and_then(|view| view.last_updated_at),
                node_status_counts: plan_node_status_counts_view(node_status_counts),
                summary: lightweight_plan_summary_text(&plan_summary_view(summary.clone())),
                plan_summary: plan_summary_view(summary),
                activity,
            })
        })
        .collect()
}

fn plan_summary_from_canonical_snapshot(
    snapshot: &prism_coordination::CoordinationSnapshotV2,
    derivations: &prism_coordination::CoordinationDerivations,
    plan_id: &PlanId,
) -> Option<prism_query::PlanSummary> {
    let graph = snapshot.graph().ok()?;
    let derived_plan = derivations.plan_state(plan_id)?;
    let descendant_task_ids = descendant_task_ids_for_plan(&graph, plan_id);
    let descendant_plan_count = descendant_plan_ids_for_plan(&graph, plan_id).len();

    let mut summary = prism_query::PlanSummary {
        plan_id: plan_id.clone(),
        status: compatibility_plan_status(derived_plan.derived_status),
        total_nodes: descendant_plan_count + descendant_task_ids.len(),
        completed_nodes: 0,
        abandoned_nodes: 0,
        in_progress_nodes: 0,
        actionable_nodes: 0,
        execution_blocked_nodes: 0,
        completion_gated_nodes: 0,
        review_gated_nodes: 0,
        validation_gated_nodes: 0,
        stale_nodes: 0,
        claim_conflicted_nodes: 0,
    };

    for task_id in descendant_task_ids {
        let task_state = derivations.task_state(&task_id)?;
        match task_state.effective_status {
            EffectiveTaskStatus::Completed => {
                summary.completed_nodes += 1;
                continue;
            }
            EffectiveTaskStatus::Abandoned => {
                summary.abandoned_nodes += 1;
                continue;
            }
            EffectiveTaskStatus::Active => summary.in_progress_nodes += 1,
            _ => {}
        }

        if task_state.graph_actionable {
            summary.actionable_nodes += 1;
        } else {
            summary.execution_blocked_nodes += 1;
        }
    }

    Some(summary)
}

fn canonical_plan_node_status_counts(
    derivations: &prism_coordination::CoordinationDerivations,
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> prism_query::PlanNodeStatusCounts {
    let mut counts = prism_query::PlanNodeStatusCounts::default();
    for task_id in descendant_task_ids_for_plan(graph, plan_id) {
        let Some(task_state) = derivations.task_state(&task_id) else {
            continue;
        };
        match task_state.effective_status {
            EffectiveTaskStatus::Pending => counts.proposed += 1,
            EffectiveTaskStatus::Active => counts.in_progress += 1,
            EffectiveTaskStatus::Blocked => counts.blocked += 1,
            EffectiveTaskStatus::BrokenDependency => counts.waiting += 1,
            EffectiveTaskStatus::Completed => counts.completed += 1,
            EffectiveTaskStatus::Abandoned => counts.abandoned += 1,
            EffectiveTaskStatus::Failed => counts.blocked += 1,
        }
        if task_state.graph_actionable {
            counts.ready += 1;
        }
    }
    counts
}

fn descendant_task_ids_for_plan(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<TaskId> {
    fn collect(
        graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
        plan_id: &PlanId,
        task_ids: &mut Vec<TaskId>,
    ) {
        for child in graph.children_of_plan(plan_id) {
            match child.kind {
                NodeRefKind::Task => task_ids.push(TaskId::new(child.id)),
                NodeRefKind::Plan => collect(graph, &PlanId::new(child.id), task_ids),
            }
        }
    }

    let mut task_ids = Vec::new();
    collect(graph, plan_id, &mut task_ids);
    task_ids.sort_by(|left, right| left.0.cmp(&right.0));
    task_ids.dedup_by(|left, right| left == right);
    task_ids
}

fn descendant_plan_ids_for_plan(
    graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
    plan_id: &PlanId,
) -> Vec<PlanId> {
    fn collect(
        graph: &prism_coordination::CanonicalCoordinationGraph<'_>,
        plan_id: &PlanId,
        plan_ids: &mut Vec<PlanId>,
    ) {
        for child in graph.children_of_plan(plan_id) {
            if child.kind != NodeRefKind::Plan {
                continue;
            }
            let child_plan_id = PlanId::new(child.id);
            plan_ids.push(child_plan_id.clone());
            collect(graph, &child_plan_id, plan_ids);
        }
    }

    let mut plan_ids = Vec::new();
    collect(graph, plan_id, &mut plan_ids);
    plan_ids.sort_by(|left, right| left.0.cmp(&right.0));
    plan_ids.dedup_by(|left, right| left == right);
    plan_ids
}

fn compatibility_plan_status(status: DerivedPlanStatus) -> PlanStatus {
    match status {
        DerivedPlanStatus::Pending => PlanStatus::Draft,
        DerivedPlanStatus::Active => PlanStatus::Active,
        DerivedPlanStatus::Blocked => PlanStatus::Blocked,
        DerivedPlanStatus::BrokenDependency => PlanStatus::Blocked,
        DerivedPlanStatus::Completed => PlanStatus::Completed,
        DerivedPlanStatus::Failed => PlanStatus::Blocked,
        DerivedPlanStatus::Abandoned => PlanStatus::Abandoned,
        DerivedPlanStatus::Archived => PlanStatus::Archived,
    }
}

fn lightweight_plan_summary_text(summary: &prism_js::PlanSummaryView) -> String {
    let mut parts = Vec::new();
    if summary.actionable_nodes > 0 {
        parts.push(format!("{} actionable", summary.actionable_nodes));
    }
    if summary.in_progress_nodes > 0 {
        parts.push(format!("{} in progress", summary.in_progress_nodes));
    }
    if summary.execution_blocked_nodes > 0 {
        parts.push(format!("{} blocked", summary.execution_blocked_nodes));
    }
    if summary.completed_nodes > 0 {
        parts.push(format!("{} completed", summary.completed_nodes));
    }
    if summary.abandoned_nodes > 0 {
        parts.push(format!("{} abandoned", summary.abandoned_nodes));
    }
    if parts.is_empty() {
        parts.push(format!("{} nodes", summary.total_nodes));
    }
    format!("{} of {} nodes", parts.join(", "), summary.total_nodes)
}

fn sort_plan_entries(plans: &mut [prism_js::PlanListEntryView], sort: UiPlanSort) {
    match sort {
        UiPlanSort::Newest => plans.sort_by(newest_sort_cmp),
        UiPlanSort::Oldest => plans.sort_by(oldest_sort_cmp),
        UiPlanSort::Priority => plans.sort_by(priority_sort_cmp),
        UiPlanSort::Actionable => plans.sort_by(actionable_sort_cmp),
        UiPlanSort::Completion => plans.sort_by(completion_sort_cmp),
        UiPlanSort::Title => plans.sort_by(|left, right| {
            left.title
                .to_ascii_lowercase()
                .cmp(&right.title.to_ascii_lowercase())
                .then_with(|| newest_sort_cmp(left, right))
        }),
    }
}

fn sort_plan_entries_for_resource(
    plans: &mut [prism_js::PlanListEntryView],
    sort: PlansResourceSort,
) {
    match sort {
        PlansResourceSort::LastUpdatedDesc => plans.sort_by(last_updated_desc_sort_cmp),
        PlansResourceSort::LastUpdatedAsc => plans.sort_by(last_updated_asc_sort_cmp),
        PlansResourceSort::CreatedAtDesc => plans.sort_by(created_at_desc_sort_cmp),
        PlansResourceSort::CreatedAtAsc => plans.sort_by(created_at_asc_sort_cmp),
        PlansResourceSort::ProposedDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.proposed)
        }),
        PlansResourceSort::ReadyDesc => plans
            .sort_by(|left, right| status_count_desc_sort_cmp(left, right, |counts| counts.ready)),
        PlansResourceSort::InProgressDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.in_progress)
        }),
        PlansResourceSort::BlockedDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.blocked)
        }),
        PlansResourceSort::WaitingDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.waiting)
        }),
        PlansResourceSort::InReviewDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.in_review)
        }),
        PlansResourceSort::ValidatingDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.validating)
        }),
        PlansResourceSort::CompletedDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.completed)
        }),
        PlansResourceSort::AbandonedDesc => plans.sort_by(|left, right| {
            status_count_desc_sort_cmp(left, right, |counts| counts.abandoned)
        }),
    }
}

fn newest_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    plan_created_sort_token(&right.plan_id)
        .cmp(plan_created_sort_token(&left.plan_id))
        .then_with(|| left.title.cmp(&right.title))
}

fn last_updated_desc_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    right
        .last_updated_at
        .cmp(&left.last_updated_at)
        .then_with(|| created_at_desc_sort_cmp(left, right))
}

fn last_updated_asc_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    left.last_updated_at
        .cmp(&right.last_updated_at)
        .then_with(|| created_at_asc_sort_cmp(left, right))
}

fn created_at_desc_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    right
        .created_at
        .cmp(&left.created_at)
        .then_with(|| newest_sort_cmp(left, right))
}

fn created_at_asc_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    left.created_at
        .cmp(&right.created_at)
        .then_with(|| oldest_sort_cmp(left, right))
}

fn oldest_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    plan_created_sort_token(&left.plan_id)
        .cmp(plan_created_sort_token(&right.plan_id))
        .then_with(|| left.title.cmp(&right.title))
}

fn priority_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    right
        .scheduling
        .manual_boost
        .cmp(&left.scheduling.manual_boost)
        .then_with(|| right.scheduling.importance.cmp(&left.scheduling.importance))
        .then_with(|| right.scheduling.urgency.cmp(&left.scheduling.urgency))
        .then_with(|| {
            right
                .plan_summary
                .actionable_nodes
                .cmp(&left.plan_summary.actionable_nodes)
        })
        .then_with(|| {
            right
                .plan_summary
                .in_progress_nodes
                .cmp(&left.plan_summary.in_progress_nodes)
        })
        .then_with(|| newest_sort_cmp(left, right))
}

fn actionable_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    right
        .plan_summary
        .actionable_nodes
        .cmp(&left.plan_summary.actionable_nodes)
        .then_with(|| {
            right
                .plan_summary
                .in_progress_nodes
                .cmp(&left.plan_summary.in_progress_nodes)
        })
        .then_with(|| priority_sort_cmp(left, right))
}

fn completion_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
) -> std::cmp::Ordering {
    let left_total = left.plan_summary.total_nodes.max(1);
    let right_total = right.plan_summary.total_nodes.max(1);
    (right.plan_summary.completed_nodes * left_total)
        .cmp(&(left.plan_summary.completed_nodes * right_total))
        .then_with(|| {
            right
                .plan_summary
                .completed_nodes
                .cmp(&left.plan_summary.completed_nodes)
        })
        .then_with(|| priority_sort_cmp(left, right))
}

fn plan_created_sort_token(plan_id: &str) -> &str {
    plan_id.rsplit(':').next().unwrap_or(plan_id)
}

fn status_count_desc_sort_cmp(
    left: &prism_js::PlanListEntryView,
    right: &prism_js::PlanListEntryView,
    select: impl Fn(&prism_js::PlanNodeStatusCountsView) -> usize,
) -> std::cmp::Ordering {
    select(&right.node_status_counts)
        .cmp(&select(&left.node_status_counts))
        .then_with(|| last_updated_desc_sort_cmp(left, right))
}

fn plan_activity_present(activity: &PlanActivity) -> bool {
    activity.created_at.is_some()
        || activity.last_updated_at.is_some()
        || activity.last_event_kind.is_some()
        || activity.last_event_summary.is_some()
        || activity.last_event_task_id.is_some()
}

fn plan_activity_index_from_snapshot(
    snapshot: &CoordinationSnapshot,
) -> BTreeMap<String, PlanActivity> {
    let mut fallback_last_updated = BTreeMap::<String, (u64, Option<CoordinationTaskId>)>::new();
    let mut activity = snapshot
        .plans
        .iter()
        .map(|plan| {
            let mut entry = PlanActivity::default();
            observe_created_at(&mut entry, sortable_token_timestamp(plan.id.0.as_str()));
            observe_fallback_update(
                &mut fallback_last_updated,
                plan.id.0.as_str(),
                sortable_token_timestamp(plan.id.0.as_str()),
                None,
            );
            (plan.id.0.to_string(), entry)
        })
        .collect::<BTreeMap<_, _>>();
    let task_to_plan = snapshot
        .tasks
        .iter()
        .map(|task| (task.id.clone(), task.plan.clone()))
        .collect::<BTreeMap<_, _>>();
    let artifact_to_task = snapshot
        .artifacts
        .iter()
        .map(|artifact| (artifact.id.clone(), artifact.task.clone()))
        .collect::<BTreeMap<_, _>>();
    let claim_to_plan = snapshot
        .claims
        .iter()
        .filter_map(|claim| {
            let task_id = claim.task.as_ref()?;
            let plan_id = task_to_plan.get(task_id)?;
            Some((claim.id.clone(), plan_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let review_to_plan = snapshot
        .reviews
        .iter()
        .filter_map(|review| {
            let task_id = artifact_to_task.get(&review.artifact)?;
            let plan_id = task_to_plan.get(task_id)?;
            Some((review.id.clone(), plan_id.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    for task in &snapshot.tasks {
        let Some(entry) = activity.get_mut(task.plan.0.as_str()) else {
            continue;
        };
        let created_at = sortable_token_timestamp(task.id.0.as_str());
        observe_created_at(entry, created_at);
        observe_fallback_update(
            &mut fallback_last_updated,
            task.plan.0.as_str(),
            created_at,
            Some(&task.id),
        );
        observe_fallback_update(
            &mut fallback_last_updated,
            task.plan.0.as_str(),
            task.lease_started_at,
            Some(&task.id),
        );
        observe_fallback_update(
            &mut fallback_last_updated,
            task.plan.0.as_str(),
            task.lease_refreshed_at,
            Some(&task.id),
        );
    }

    for claim in &snapshot.claims {
        let Some(task_id) = claim.task.as_ref() else {
            continue;
        };
        let Some(plan_id) = task_to_plan.get(task_id) else {
            continue;
        };
        let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
            continue;
        };
        let created_at = sortable_token_timestamp(claim.id.0.as_str()).or(Some(claim.since));
        observe_created_at(entry, created_at);
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            sortable_token_timestamp(claim.id.0.as_str()),
            Some(task_id),
        );
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            Some(claim.since),
            Some(task_id),
        );
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            claim.refreshed_at,
            Some(task_id),
        );
    }

    for artifact in &snapshot.artifacts {
        let Some(plan_id) = task_to_plan.get(&artifact.task) else {
            continue;
        };
        let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
            continue;
        };
        let created_at = sortable_token_timestamp(artifact.id.0.as_str());
        observe_created_at(entry, created_at);
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            created_at,
            Some(&artifact.task),
        );
    }

    for review in &snapshot.reviews {
        let Some(task_id) = artifact_to_task.get(&review.artifact) else {
            continue;
        };
        let Some(plan_id) = task_to_plan.get(task_id) else {
            continue;
        };
        let Some(entry) = activity.get_mut(plan_id.0.as_str()) else {
            continue;
        };
        let created_at = sortable_token_timestamp(review.id.0.as_str()).or(Some(review.meta.ts));
        observe_created_at(entry, created_at);
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            sortable_token_timestamp(review.id.0.as_str()),
            Some(task_id),
        );
        observe_fallback_update(
            &mut fallback_last_updated,
            plan_id.0.as_str(),
            Some(review.meta.ts),
            Some(task_id),
        );
    }

    for event in &snapshot.events {
        let plan_id = event
            .plan
            .clone()
            .or_else(|| {
                event
                    .task
                    .as_ref()
                    .and_then(|task_id| task_to_plan.get(task_id).cloned())
            })
            .or_else(|| {
                event
                    .claim
                    .as_ref()
                    .and_then(|claim_id| claim_to_plan.get(claim_id).cloned())
            })
            .or_else(|| {
                event.artifact.as_ref().and_then(|artifact_id| {
                    artifact_to_task
                        .get(artifact_id)
                        .and_then(|task_id| task_to_plan.get(task_id))
                        .cloned()
                })
            })
            .or_else(|| {
                event
                    .review
                    .as_ref()
                    .and_then(|review_id| review_to_plan.get(review_id).cloned())
            });
        let Some(plan_id) = plan_id else {
            continue;
        };
        let entry = activity.entry(plan_id.0.to_string()).or_default();
        entry.created_at = Some(match entry.created_at {
            Some(existing) => existing.min(event.meta.ts),
            None => event.meta.ts,
        });
        let replace_last = match entry.last_updated_at {
            Some(existing) => event.meta.ts >= existing,
            None => true,
        };
        if replace_last {
            entry.last_updated_at = Some(event.meta.ts);
            entry.last_event_kind = Some(event.kind);
            entry.last_event_summary = Some(event.summary.clone());
            entry.last_event_task_id = event.task.clone();
        }
    }

    for (plan_id, (ts, task_id)) in fallback_last_updated {
        let Some(entry) = activity.get_mut(plan_id.as_str()) else {
            continue;
        };
        if entry.last_updated_at.is_none() {
            entry.last_updated_at = Some(ts);
            entry.last_event_kind = None;
            entry.last_event_summary = None;
            entry.last_event_task_id = task_id;
        }
    }

    activity
}

fn observe_created_at(entry: &mut PlanActivity, ts: Option<u64>) {
    let Some(ts) = ts else {
        return;
    };
    entry.created_at = Some(match entry.created_at {
        Some(existing) => existing.min(ts),
        None => ts,
    });
}

fn observe_fallback_update(
    fallback: &mut BTreeMap<String, (u64, Option<CoordinationTaskId>)>,
    plan_id: &str,
    ts: Option<u64>,
    task_id: Option<&CoordinationTaskId>,
) {
    let Some(ts) = ts else {
        return;
    };
    let replace = fallback
        .get(plan_id)
        .is_none_or(|(existing, _)| ts > *existing);
    if replace {
        fallback.insert(plan_id.to_string(), (ts, task_id.cloned()));
    }
}

fn clamp_overview_text(text: &str) -> String {
    if text.chars().count() <= OVERVIEW_TEXT_MAX_CHARS {
        return text.to_string();
    }
    let truncated = text
        .chars()
        .take(OVERVIEW_TEXT_MAX_CHARS - 1)
        .collect::<String>();
    format!("{truncated}…")
}

fn build_plan_detail_view(
    host: &QueryHost,
    prism: &prism_query::Prism,
    plans: &[prism_js::PlanListEntryView],
    selected_plan_id: &str,
) -> Result<Option<PrismPlanDetailView>> {
    let Some(plan) = plans
        .iter()
        .find(|plan| plan.plan_id == selected_plan_id)
        .cloned()
    else {
        return Ok(None);
    };

    let plan_id = PlanId::new(plan.plan_id.clone());
    let Some(summary) = prism.plan_summary(&plan_id).map(plan_summary_view) else {
        return Ok(None);
    };
    let children = prism.plan_children_v2(&plan_id);
    let child_plans = children
        .iter()
        .filter(|child| child.kind == prism_ir::NodeRefKind::Plan)
        .filter_map(|child| prism.coordination_plan_v2(&PlanId::new(child.id.clone())))
        .map(coordination_plan_v2_view)
        .take(PLAN_DETAIL_CHILD_LIMIT)
        .collect::<Vec<_>>();
    let child_tasks = children
        .iter()
        .filter(|child| child.kind == prism_ir::NodeRefKind::Task)
        .filter_map(|child| prism.coordination_task_v2(&TaskId::new(child.id.clone())))
        .map(coordination_task_v2_view)
        .take(PLAN_DETAIL_CHILD_LIMIT)
        .collect::<Vec<_>>();
    let ready_tasks = if let Some(caller) = current_executor_caller(host.workspace_root(), None) {
        prism.ready_tasks_for_executor(&plan_id, crate::current_timestamp(), &caller)
    } else {
        prism.ready_tasks(&plan_id, crate::current_timestamp())
    }
    .into_iter()
    .take(PLAN_DETAIL_READY_LIMIT)
    .map(crate::coordination_task_view)
    .collect::<Vec<_>>();
    let pending_reviews = prism
        .pending_reviews(Some(&plan_id))
        .into_iter()
        .take(PLAN_DETAIL_REVIEW_LIMIT)
        .map(artifact_view)
        .collect::<Vec<_>>();
    let pending_handoffs = host
        .ui_coordination_queues()?
        .pending_handoffs
        .into_iter()
        .filter(|task| task.plan_id == plan.plan_id)
        .take(PLAN_DETAIL_HANDOFF_LIMIT)
        .collect::<Vec<_>>();
    let recent_violations = prism
        .policy_violations(Some(&plan_id), None, PLAN_DETAIL_VIOLATION_LIMIT)
        .into_iter()
        .map(policy_violation_record_view)
        .collect::<Vec<_>>();
    let recent_outcomes =
        plan_recent_outcomes(prism, &ready_tasks, &pending_handoffs, &pending_reviews);

    Ok(Some(PrismPlanDetailView {
        plan,
        summary,
        children: children.into_iter().map(node_ref_view).collect(),
        child_plans,
        child_tasks,
        ready_tasks,
        pending_reviews,
        pending_handoffs,
        recent_violations,
        recent_outcomes,
    }))
}

impl QueryHost {
    pub(crate) fn ui_coordination_queues(&self) -> Result<PrismOverviewCoordinationQueuesView> {
        ui_overview_coordination_queues(self)
    }
}

fn ui_overview_summary_view(host: &QueryHost) -> Result<PrismOverviewSummaryView> {
    let diagnostics = host.diagnostics_state();
    Ok(PrismOverviewSummaryView {
        session: ui_session_view(host, None),
        runtime: runtime_status(host)?,
        active_query_count: 0,
        active_mutation_count: 0,
        recent_query_error_count: diagnostics.recent_query_error_count(Some(10)),
        last_runtime_event: diagnostics.last_runtime_event(),
    })
}

fn ui_overview_task_view(
    host: &QueryHost,
    active_session: Option<&SessionState>,
) -> Result<PrismOverviewTaskView> {
    let session = ui_session_view(host, active_session);
    let journal = session
        .current_task
        .as_ref()
        .and_then(|task| {
            active_session
                .map(|active_session| current_task_journal(host, active_session, &task.task_id))
        })
        .transpose()?;
    Ok(PrismOverviewTaskView { session, journal })
}

fn ui_overview_coordination_summary(host: &QueryHost) -> Result<PrismOverviewCoordinationView> {
    if !host.features.coordination_layer_enabled() {
        return Ok(PrismOverviewCoordinationView {
            enabled: false,
            active_plan_count: 0,
            task_count: 0,
            ready_task_count: 0,
            in_review_task_count: 0,
            active_claim_count: 0,
            pending_handoff_count: 0,
            pending_review_count: 0,
            proposed_artifact_count: 0,
            recent_pending_reviews: Vec::new(),
            recent_violations: Vec::new(),
        });
    }

    let prism = host.current_prism();
    let now = current_timestamp();
    let fallback_snapshot = prism.coordination_snapshot();
    let read_model = host
        .workspace_session()
        .and_then(|workspace| workspace.load_coordination_read_model().ok().flatten())
        .unwrap_or_else(|| fallback_coordination_read_model(&fallback_snapshot));
    let queue_model = host
        .workspace_session()
        .and_then(|workspace| {
            workspace
                .load_coordination_queue_read_model()
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| fallback_coordination_queue_read_model(&fallback_snapshot));
    let ready_task_count = ready_task_count_for_active_plans(&read_model.active_plans, |plan_id| {
        if let Some(caller) = current_executor_caller(host.workspace_root(), None) {
            prism.ready_tasks_for_executor(plan_id, now, &caller).len()
        } else {
            prism.ready_tasks(plan_id, now).len()
        }
    });
    let recent_pending_reviews = read_model
        .pending_review_artifacts
        .iter()
        .take(OVERVIEW_COORDINATION_REVIEW_LIMIT)
        .cloned()
        .map(artifact_view)
        .collect();
    let recent_violations = read_model
        .recent_violations
        .iter()
        .take(OVERVIEW_COORDINATION_VIOLATION_LIMIT)
        .cloned()
        .map(policy_violation_record_view)
        .collect::<Vec<_>>();
    let active_claim_count = read_model
        .active_claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Active && claim.expires_at > now)
        .count();

    Ok(PrismOverviewCoordinationView {
        enabled: true,
        active_plan_count: read_model.active_plans.len(),
        task_count: read_model.task_count,
        ready_task_count,
        in_review_task_count: read_model.in_review_task_ids.len(),
        active_claim_count,
        pending_handoff_count: queue_model.pending_handoff_tasks.len(),
        pending_review_count: read_model.pending_review_artifacts.len(),
        proposed_artifact_count: read_model.proposed_artifact_count,
        recent_pending_reviews,
        recent_violations,
    })
}

fn ui_overview_coordination_queues(
    host: &QueryHost,
) -> Result<PrismOverviewCoordinationQueuesView> {
    if !host.features.coordination_layer_enabled() {
        return Ok(PrismOverviewCoordinationQueuesView {
            enabled: false,
            pending_handoffs: Vec::new(),
            active_claims: Vec::new(),
            pending_reviews: Vec::new(),
        });
    }

    let prism = host.current_prism();
    let fallback_snapshot = prism.coordination_snapshot();
    let queue_model = host
        .workspace_session()
        .and_then(|workspace| {
            workspace
                .load_coordination_queue_read_model()
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| fallback_coordination_queue_read_model(&fallback_snapshot));

    Ok(PrismOverviewCoordinationQueuesView {
        enabled: true,
        pending_handoffs: queue_model
            .pending_handoff_tasks
            .iter()
            .take(OVERVIEW_COORDINATION_HANDOFF_LIMIT)
            .cloned()
            .map(coordination_task_view)
            .collect(),
        active_claims: queue_model
            .active_claims
            .iter()
            .take(OVERVIEW_COORDINATION_CLAIM_LIMIT)
            .cloned()
            .map(claim_view)
            .collect(),
        pending_reviews: queue_model
            .pending_review_artifacts
            .iter()
            .take(OVERVIEW_COORDINATION_REVIEW_LIMIT)
            .cloned()
            .map(artifact_view)
            .collect(),
    })
}

fn fallback_coordination_read_model(
    snapshot: &prism_coordination::CoordinationSnapshot,
) -> CoordinationReadModel {
    prism_coordination::coordination_read_model_from_snapshot(snapshot)
}

fn fallback_coordination_queue_read_model(
    snapshot: &prism_coordination::CoordinationSnapshot,
) -> CoordinationQueueReadModel {
    coordination_queue_read_model_from_snapshot(snapshot)
}

fn ui_session_view(host: &QueryHost, session: Option<&SessionState>) -> crate::SessionView {
    let limits = session
        .map(SessionState::limits)
        .unwrap_or(host.default_limits);
    let bridge_identity = host.workspace_root().map(|root| {
        ui_operator_identity_view(root, host.workspace_session().map(|workspace| &**workspace))
    });
    crate::SessionView {
        workspace_root: host
            .workspace_session()
            .map(|workspace| workspace.root().display().to_string()),
        current_task: session.and_then(|session| {
            session
                .effective_current_task_state()
                .map(|task| session_task_view(host, session, &task))
        }),
        current_work: session.and_then(|session| {
            session
                .current_work_state()
                .map(crate::host_resources::session_work_view)
        }),
        current_agent: session
            .and_then(|session| session.current_agent().map(|agent| agent.0.to_string())),
        bridge_identity,
        limits: crate::SessionLimitsView {
            max_result_nodes: limits.max_result_nodes,
            max_call_graph_depth: limits.max_call_graph_depth,
            max_output_json_bytes: limits.max_output_json_bytes,
        },
        features: crate::FeatureFlagsView {
            mode: host.features.mode_label().to_string(),
            runtime: crate::RuntimeCapabilitiesView {
                mode: host.features.runtime_mode_label().to_string(),
                coordination: host.features.coordination_layer_enabled(),
                knowledge_storage: host.features.knowledge_storage_layer_enabled(),
                cognition: host.features.cognition_layer_enabled(),
            },
            coordination: crate::CoordinationFeaturesView {
                workflow: host.features.coordination.workflow,
                claims: host.features.coordination.claims,
                artifacts: host.features.coordination.artifacts,
            },
            ui: host.features.ui,
            internal_developer: host.features.internal_developer,
        },
    }
}

fn current_task_journal(
    host: &QueryHost,
    session: &SessionState,
    task_id: &str,
) -> Result<prism_js::TaskJournalView> {
    let prism = host.current_prism();
    let task_id = TaskId::new(task_id.to_string());
    let replay = crate::load_task_replay(host.workspace_session_ref(), prism.as_ref(), &task_id)?;
    crate::task_journal_view_from_replay(
        session,
        prism.as_ref(),
        replay,
        None,
        OVERVIEW_TASK_EVENT_LIMIT,
        OVERVIEW_TASK_MEMORY_LIMIT,
    )
}

fn plan_recent_outcomes(
    prism: &prism_query::Prism,
    ready_tasks: &[prism_js::CoordinationTaskView],
    pending_handoffs: &[prism_js::CoordinationTaskView],
    pending_reviews: &[prism_js::ArtifactView],
) -> Vec<prism_js::AgentOutcomeSummaryView> {
    let task_ids = ready_tasks
        .iter()
        .map(|task| task.id.clone())
        .chain(pending_handoffs.iter().map(|task| task.id.clone()))
        .chain(
            pending_reviews
                .iter()
                .map(|artifact| artifact.task_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let mut seen_event_ids = HashSet::<String>::new();
    let mut outcomes = task_ids
        .into_iter()
        .flat_map(|task_id| {
            prism.query_outcomes(&OutcomeRecallQuery {
                task: Some(TaskId::new(task_id)),
                limit: PLAN_DETAIL_OUTCOMES_PER_TASK,
                ..OutcomeRecallQuery::default()
            })
        })
        .filter(|event| seen_event_ids.insert(event.meta.id.0.to_string()))
        .collect::<Vec<_>>();

    outcomes.sort_by(|left, right| {
        right
            .meta
            .ts
            .cmp(&left.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });
    outcomes.truncate(PLAN_DETAIL_OUTCOME_LIMIT);

    outcomes
        .into_iter()
        .map(|event| prism_js::AgentOutcomeSummaryView {
            ts: event.meta.ts,
            kind: format!("{:?}", event.kind),
            result: format!("{:?}", event.result),
            summary: clamp_overview_text(&event.summary),
        })
        .collect()
}

fn task_claim_history_entry_view(claim: WorkClaim) -> PrismUiTaskClaimHistoryEntryView {
    let holder = claim
        .agent
        .as_ref()
        .map(|agent| agent.0.clone())
        .unwrap_or_else(|| claim.holder.0.clone());
    let duration_end = claim
        .refreshed_at
        .or(claim.stale_at)
        .unwrap_or(claim.expires_at);
    PrismUiTaskClaimHistoryEntryView {
        id: claim.id.0.to_string(),
        holder: holder.to_string(),
        agent: claim.agent.as_ref().map(|agent| agent.0.to_string()),
        status: format!("{:?}", claim.status),
        capability: format!("{:?}", claim.capability),
        mode: format!("{:?}", claim.mode),
        started_at: claim.since,
        refreshed_at: claim.refreshed_at,
        stale_at: claim.stale_at,
        expires_at: claim.expires_at,
        duration_seconds: duration_end.checked_sub(claim.since),
        branch_ref: claim.branch_ref.clone(),
        worktree_id: claim.worktree_id.clone(),
        claim: claim_view(claim),
    }
}

fn task_recent_commits(task: &prism_js::CoordinationTaskView) -> Vec<PrismUiTaskCommitView> {
    let mut commits = Vec::new();
    push_task_commit(
        &mut commits,
        "source",
        task.git_execution.source_commit.as_deref(),
        task.git_execution.source_ref.as_deref(),
        "Source commit",
    );
    push_task_commit(
        &mut commits,
        "publish",
        task.git_execution.publish_commit.as_deref(),
        task.git_execution.publish_ref.as_deref(),
        "Publish commit",
    );
    push_task_commit(
        &mut commits,
        "integration",
        task.git_execution.integration_commit.as_deref(),
        task.git_execution.target_ref.as_deref(),
        "Integration commit",
    );
    if let Some(report) = task.git_execution.last_publish.as_ref() {
        push_task_commit(
            &mut commits,
            "coordination_publish",
            report.coordination_commit.as_deref(),
            report.pushed_ref.as_deref(),
            "Coordination publish commit",
        );
        push_task_commit(
            &mut commits,
            "code_publish",
            report.code_commit.as_deref(),
            report.pushed_ref.as_deref(),
            "Code publish commit",
        );
    }
    dedupe_task_commits(commits)
}

fn push_task_commit(
    commits: &mut Vec<PrismUiTaskCommitView>,
    kind: &str,
    commit: Option<&str>,
    reference: Option<&str>,
    label: &str,
) {
    let Some(commit) = commit.filter(|value| !value.is_empty()) else {
        return;
    };
    commits.push(PrismUiTaskCommitView {
        kind: kind.to_string(),
        commit: commit.to_string(),
        reference: reference.map(str::to_string),
        label: label.to_string(),
    });
}

fn dedupe_task_commits(commits: Vec<PrismUiTaskCommitView>) -> Vec<PrismUiTaskCommitView> {
    let mut seen = HashSet::<(String, String)>::new();
    commits
        .into_iter()
        .filter(|entry| seen.insert((entry.kind.clone(), entry.commit.clone())))
        .collect()
}

fn graph_entry_concepts(
    prism: &prism_query::Prism,
    root_packet: &prism_query::ConceptPacket,
) -> Vec<prism_js::ConceptPacketView> {
    let mut handles = std::iter::once(root_packet.handle.clone())
        .chain(
            prism
                .concept_relations_for_handle(&root_packet.handle)
                .into_iter()
                .map(|relation| {
                    if relation.source_handle == root_packet.handle {
                        relation.target_handle
                    } else {
                        relation.source_handle
                    }
                }),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    handles.truncate(GRAPH_ENTRY_LIMIT);

    handles
        .into_iter()
        .filter_map(|handle| prism.concept_by_handle(&handle))
        .map(|packet| concept_packet_view(prism, packet, ConceptVerbosity::Summary, false, None))
        .collect()
}

fn graph_plan_touchpoints(
    prism: &prism_query::Prism,
    selected_concept_handle: &str,
) -> Vec<GraphPlanTouchpointView> {
    let snapshot = prism.coordination_snapshot_v2();
    let Ok(graph) = snapshot.graph() else {
        return Vec::new();
    };
    let Some(derivations) = snapshot.derive_statuses().ok() else {
        return Vec::new();
    };
    let task_records = snapshot
        .tasks
        .iter()
        .map(|task| (task.id.0.to_string(), task))
        .collect::<BTreeMap<_, _>>();
    let mut touchpoints = prism
        .plans(Some(PlanStatus::Active), None, None)
        .into_iter()
        .filter_map(|plan| {
            let plan_id = plan.plan_id.clone();
            let touched_nodes = descendant_task_ids_for_plan(&graph, &plan_id)
                .into_iter()
                .filter_map(|task_id| task_records.get(task_id.0.as_str()).copied())
                .filter(|task| {
                    task.bindings
                        .concept_handles
                        .iter()
                        .any(|handle| handle == selected_concept_handle)
                })
                .take(GRAPH_TOUCHED_NODE_LIMIT)
                .filter_map(|task| {
                    Some(GraphTouchedNodeView {
                        node_id: task.id.0.to_string(),
                        title: task.title.clone(),
                        status: format!("{:?}", derivations.task_state(&task.id)?.effective_status),
                    })
                })
                .collect::<Vec<_>>();
            if touched_nodes.is_empty() {
                return None;
            }
            Some(GraphPlanTouchpointView {
                plan: plan_list_entry_view(plan),
                touched_nodes,
            })
        })
        .collect::<Vec<_>>();

    touchpoints.sort_by(|left, right| {
        right
            .touched_nodes
            .len()
            .cmp(&left.touched_nodes.len())
            .then_with(|| {
                right
                    .plan
                    .plan_summary
                    .in_progress_nodes
                    .cmp(&left.plan.plan_summary.in_progress_nodes)
            })
            .then_with(|| left.plan.plan_id.cmp(&right.plan.plan_id))
    });
    touchpoints.truncate(GRAPH_PLAN_LIMIT);
    touchpoints
}

#[cfg(test)]
mod tests {
    use super::{sort_plan_entries, sort_plan_entries_for_resource, PlansResourceSort, UiPlanSort};
    use prism_ir::{PlanKind, PlanScope, PlanStatus};
    use prism_js::{
        GitExecutionPolicyView, PlanListEntryView, PlanNodeStatusCountsView, PlanSchedulingView,
        PlanSummaryView,
    };

    #[test]
    fn ui_plan_sort_defaults_to_newest() {
        assert_eq!(UiPlanSort::default(), UiPlanSort::Newest);
        assert_eq!(UiPlanSort::parse(None), UiPlanSort::Newest);
        assert_eq!(UiPlanSort::parse(Some("oldest")), UiPlanSort::Oldest);
        assert_eq!(
            UiPlanSort::parse(Some("actionable")),
            UiPlanSort::Actionable
        );
    }

    #[test]
    fn newest_sort_prefers_more_recent_plan_ids() {
        let mut plans = vec![
            test_plan("plan:01kn0000000000000000000000", "older", 0, 4, 0, 0),
            test_plan("plan:01kp0000000000000000000000", "newer", 0, 4, 0, 0),
        ];
        sort_plan_entries(&mut plans, UiPlanSort::Newest);
        assert_eq!(plans[0].title, "newer");
        assert_eq!(plans[1].title, "older");
    }

    #[test]
    fn resource_sort_defaults_to_last_updated_desc() {
        assert_eq!(
            PlansResourceSort::default(),
            PlansResourceSort::LastUpdatedDesc
        );
        assert_eq!(
            PlansResourceSort::parse(None),
            PlansResourceSort::LastUpdatedDesc
        );
        assert_eq!(
            PlansResourceSort::parse(Some("created_at_asc")),
            PlansResourceSort::CreatedAtAsc
        );
        assert_eq!(
            PlansResourceSort::parse(Some("blocked_desc")),
            PlansResourceSort::BlockedDesc
        );
    }

    #[test]
    fn resource_sort_prefers_recent_updates_before_creation_time() {
        let mut plans = vec![
            plan_with_activity(
                test_plan("plan:01kn0000000000000000000000", "older", 0, 4, 0, 0),
                Some(10),
                Some(20),
            ),
            plan_with_activity(
                test_plan("plan:01kp0000000000000000000000", "newer", 0, 4, 0, 0),
                Some(30),
                Some(15),
            ),
        ];
        sort_plan_entries_for_resource(&mut plans, PlansResourceSort::LastUpdatedDesc);
        assert_eq!(plans[0].title, "older");
        assert_eq!(plans[1].title, "newer");
    }

    #[test]
    fn resource_sort_can_rank_by_completed_nodes() {
        let mut plans = vec![
            test_plan("plan:01kn0000000000000000000000", "few", 1, 4, 0, 0),
            test_plan("plan:01kp0000000000000000000000", "many", 3, 4, 0, 0),
        ];
        sort_plan_entries_for_resource(&mut plans, PlansResourceSort::CompletedDesc);
        assert_eq!(plans[0].title, "many");
        assert_eq!(plans[1].title, "few");
    }

    #[test]
    fn lightweight_plan_summary_tracks_progress_without_blocker_scoring() {
        let summary = PlanSummaryView {
            plan_id: "plan:01kp0000000000000000000000".to_string(),
            status: PlanStatus::Active,
            total_nodes: 5,
            completed_nodes: 1,
            abandoned_nodes: 0,
            in_progress_nodes: 1,
            actionable_nodes: 2,
            execution_blocked_nodes: 1,
            completion_gated_nodes: 0,
            review_gated_nodes: 0,
            validation_gated_nodes: 0,
            stale_nodes: 0,
            claim_conflicted_nodes: 0,
        };

        assert_eq!(summary.total_nodes, 5);
        assert_eq!(summary.actionable_nodes, 2);
        assert_eq!(summary.in_progress_nodes, 1);
        assert_eq!(summary.completed_nodes, 1);
        assert_eq!(summary.execution_blocked_nodes, 1);
    }

    fn plan_with_activity(
        mut plan: PlanListEntryView,
        created_at: Option<u64>,
        last_updated_at: Option<u64>,
    ) -> PlanListEntryView {
        plan.created_at = created_at;
        plan.last_updated_at = last_updated_at;
        plan.activity = Some(prism_js::PlanActivityView {
            created_at,
            last_updated_at,
            last_event_kind: None,
            last_event_summary: None,
            last_event_task_id: None,
        });
        plan
    }

    fn test_plan(
        plan_id: &str,
        title: &str,
        completed_nodes: usize,
        total_nodes: usize,
        actionable_nodes: usize,
        in_progress_nodes: usize,
    ) -> PlanListEntryView {
        PlanListEntryView {
            plan_id: plan_id.to_string(),
            title: title.to_string(),
            goal: format!("{title} goal"),
            status: PlanStatus::Active,
            scope: PlanScope::Repo,
            kind: PlanKind::TaskExecution,
            scheduling: PlanSchedulingView {
                importance: 0,
                urgency: 0,
                manual_boost: 0,
                due_at: None,
            },
            git_execution_policy: GitExecutionPolicyView {
                start_mode: "auto".to_string(),
                completion_mode: "auto".to_string(),
                integration_mode: "branch".to_string(),
                target_ref: None,
                target_branch: "main".to_string(),
                require_task_branch: false,
                max_commits_behind_target: 0,
                max_fetch_age_seconds: None,
            },
            created_at: None,
            last_updated_at: None,
            node_status_counts: PlanNodeStatusCountsView {
                proposed: 0,
                ready: actionable_nodes.saturating_sub(in_progress_nodes),
                in_progress: in_progress_nodes,
                blocked: 0,
                waiting: 0,
                in_review: 0,
                validating: 0,
                completed: completed_nodes,
                abandoned: 0,
                abstract_nodes: 0,
            },
            summary: title.to_string(),
            plan_summary: PlanSummaryView {
                plan_id: plan_id.to_string(),
                status: PlanStatus::Active,
                total_nodes,
                completed_nodes,
                abandoned_nodes: 0,
                in_progress_nodes,
                actionable_nodes,
                execution_blocked_nodes: 0,
                completion_gated_nodes: 0,
                review_gated_nodes: 0,
                validation_gated_nodes: 0,
                stale_nodes: 0,
                claim_conflicted_nodes: 0,
            },
            activity: None,
        }
    }
}
