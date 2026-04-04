use std::collections::{BTreeSet, HashSet};

use anyhow::{anyhow, Result};
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, coordination_read_model_from_snapshot,
    ready_task_count_for_active_plans, CoordinationQueueReadModel, CoordinationReadModel,
};
use prism_ir::ClaimStatus;
use prism_ir::{PlanId, PlanStatus, TaskId};
use prism_memory::OutcomeRecallQuery;

use crate::ui_types::{
    GraphPlanTouchpointView, GraphTouchedNodeView, OverviewConceptSpotlightView,
    OverviewPlanSignalsView, OverviewPlanSpotlightView, PrismGraphView,
    PrismOverviewCoordinationQueuesView, PrismOverviewCoordinationView, PrismOverviewSummaryView,
    PrismOverviewTaskView, PrismOverviewView, PrismPlanDetailView, PrismPlansView,
};
use crate::views::{
    artifact_view, concept_packet_view, plan_execution_overlay_view, plan_graph_view,
    plan_list_entry_view, plan_node_recommendation_view, plan_summary_view,
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
const OVERVIEW_TASK_EVENT_LIMIT: usize = 12;
const OVERVIEW_TASK_MEMORY_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_REVIEW_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_VIOLATION_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_HANDOFF_LIMIT: usize = 6;
const OVERVIEW_COORDINATION_CLAIM_LIMIT: usize = 6;

pub(crate) trait QueryHostUiReadModelsExt {
    fn ui_overview_view(&self) -> Result<PrismOverviewView>;
    fn ui_plans_view(&self, selected_plan_id: Option<&str>) -> Result<PrismPlansView>;
    fn ui_graph_view(&self, selected_concept_handle: Option<&str>) -> Result<PrismGraphView>;
}

impl QueryHostUiReadModelsExt for QueryHost {
    fn ui_overview_view(&self) -> Result<PrismOverviewView> {
        let summary = ui_overview_summary_view(self)?;
        let task = ui_overview_task_view(self, None)?;
        let coordination = ui_overview_coordination_summary(self)?;
        let coordination_queues = ui_overview_coordination_queues(self)?;
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
                    title: plan.title.clone(),
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
        prism.ready_tasks(plan_id, now).len()
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
        bridge_identity: None,
        limits: crate::SessionLimitsView {
            max_result_nodes: limits.max_result_nodes,
            max_call_graph_depth: limits.max_call_graph_depth,
            max_output_json_bytes: limits.max_output_json_bytes,
        },
        features: crate::FeatureFlagsView {
            mode: host.features.mode_label().to_string(),
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
                    .plan_summary
                    .in_progress_nodes
                    .cmp(&left.plan.plan_summary.in_progress_nodes)
            })
            .then_with(|| left.plan.plan_id.cmp(&right.plan.plan_id))
    });
    touchpoints.truncate(GRAPH_PLAN_LIMIT);
    touchpoints
}
