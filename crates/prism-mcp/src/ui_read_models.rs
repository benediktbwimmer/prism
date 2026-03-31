use std::collections::{BTreeSet, HashSet};

use anyhow::{anyhow, Result};
use prism_coordination::coordination_read_model_from_snapshot;
use prism_ir::{PlanId, PlanStatus, TaskId};
use prism_memory::OutcomeRecallQuery;

use crate::ui_types::{
    GraphPlanTouchpointView, GraphTouchedNodeView, OverviewConceptSpotlightView,
    OverviewPlanSignalsView, OverviewPlanSpotlightView, PrismGraphView, PrismOverviewView,
    PrismPlanDetailView, PrismPlansView,
};
use crate::views::{
    artifact_view, concept_packet_view, plan_execution_overlay_view, plan_graph_view,
    plan_list_entry_view, plan_node_recommendation_view, plan_summary_view,
    policy_violation_record_view, ConceptVerbosity,
};
use crate::QueryHost;

const OVERVIEW_PLAN_LIMIT: usize = 3;
const OVERVIEW_PLAN_NEXT_LIMIT: usize = 2;
const OVERVIEW_CONCEPT_LIMIT: usize = 4;
const OVERVIEW_OUTCOME_LIMIT: usize = 6;
const OVERVIEW_HANDOFF_LIMIT: usize = 4;
const OVERVIEW_TEXT_MAX_CHARS: usize = 180;
const PLAN_DETAIL_NEXT_LIMIT: usize = 6;
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

pub(crate) trait QueryHostUiReadModelsExt {
    fn ui_overview_view(&self) -> Result<PrismOverviewView>;
    fn ui_plans_view(&self, selected_plan_id: Option<&str>) -> Result<PrismPlansView>;
    fn ui_graph_view(&self, selected_concept_handle: Option<&str>) -> Result<PrismGraphView>;
}

impl QueryHostUiReadModelsExt for QueryHost {
    fn ui_overview_view(&self) -> Result<PrismOverviewView> {
        let summary = self.dashboard_summary_view()?;
        let task = self.dashboard_task_snapshot(None)?;
        let coordination = self.dashboard_coordination_summary()?;
        let coordination_queues = self.dashboard_coordination_queues()?;
        let prism = self.current_prism();

        let read_model = self
            .workspace_session()
            .and_then(|workspace| workspace.load_coordination_read_model().ok().flatten())
            .unwrap_or_else(|| {
                coordination_read_model_from_snapshot(&prism.coordination_snapshot())
            });
        let mut plan_spotlights = read_model
            .active_plans
            .into_iter()
            .filter_map(|plan| {
                let summary = prism.plan_summary(&plan.id)?;
                let next_nodes = prism
                    .plan_next(&plan.id, OVERVIEW_PLAN_NEXT_LIMIT)
                    .into_iter()
                    .map(plan_node_recommendation_view)
                    .collect::<Vec<_>>();
                Some(OverviewPlanSpotlightView {
                    plan_id: plan.id.0.to_string(),
                    title: if plan.title.trim().is_empty() {
                        plan.goal.clone()
                    } else {
                        plan.title.clone()
                    },
                    goal: plan.goal,
                    summary: plan_summary_view(summary),
                    next_nodes,
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
            .flat_map(|plan| plan.next_nodes.iter())
            .flat_map(|node| node.node.bindings.concept_handles.iter())
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

    fn ui_plans_view(&self, selected_plan_id: Option<&str>) -> Result<PrismPlansView> {
        let prism = self.current_prism();
        let plans = prism
            .plans(None, None, None)
            .into_iter()
            .map(plan_list_entry_view)
            .collect::<Vec<_>>();

        let selected_plan_id = selected_plan_id
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
            plans,
            selected_plan_id,
            selected_plan,
        })
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
    let Some(graph) = prism.plan_graph(&plan_id).map(plan_graph_view) else {
        return Ok(None);
    };
    let Some(summary) = prism.plan_summary(&plan_id).map(plan_summary_view) else {
        return Ok(None);
    };

    let execution = prism
        .plan_execution(&plan_id)
        .into_iter()
        .map(plan_execution_overlay_view)
        .collect::<Vec<_>>();
    let next_nodes = prism
        .plan_next(&plan_id, PLAN_DETAIL_NEXT_LIMIT)
        .into_iter()
        .map(plan_node_recommendation_view)
        .collect::<Vec<_>>();
    let ready_tasks = prism
        .ready_tasks(&plan_id, crate::current_timestamp())
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
        .dashboard_coordination_queues()?
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
        graph,
        execution,
        next_nodes,
        ready_tasks,
        pending_reviews,
        pending_handoffs,
        recent_violations,
        recent_outcomes,
    }))
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
    let mut touchpoints = prism
        .plans(Some(PlanStatus::Active), None, None)
        .into_iter()
        .filter_map(|plan| {
            let plan_id = plan.plan_id.clone();
            let graph = prism.plan_graph(&plan_id)?;
            let touched_nodes = graph
                .nodes
                .into_iter()
                .filter(|node| {
                    node.bindings
                        .concept_handles
                        .iter()
                        .any(|handle| handle == selected_concept_handle)
                })
                .take(GRAPH_TOUCHED_NODE_LIMIT)
                .map(|node| GraphTouchedNodeView {
                    node_id: node.id.0.to_string(),
                    title: node.title,
                    status: format!("{:?}", node.status),
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
                    .summary
                    .in_progress_nodes
                    .cmp(&left.plan.summary.in_progress_nodes)
            })
            .then_with(|| left.plan.plan_id.cmp(&right.plan.plan_id))
    });
    touchpoints.truncate(GRAPH_PLAN_LIMIT);
    touchpoints
}
