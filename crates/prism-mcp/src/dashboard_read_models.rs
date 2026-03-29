use anyhow::Result;
use prism_coordination::{
    coordination_queue_read_model_from_snapshot, ready_task_count_for_active_plans,
    CoordinationQueueReadModel, CoordinationReadModel,
};
use prism_ir::{ClaimStatus, TaskId};
use prism_js::TaskJournalView;

use crate::dashboard_types::{
    DashboardCoordinationQueuesView, DashboardCoordinationSummaryView, DashboardSummaryView,
    DashboardTaskSnapshotView,
};
use crate::runtime_views::{runtime_status, runtime_timeline};
use crate::{
    artifact_view, claim_view, coordination_task_view, current_timestamp,
    policy_violation_record_view, task_journal_view, CoordinationFeaturesView, FeatureFlagsView,
    QueryHost, RuntimeTimelineArgs, SessionLimitsView, SessionState, SessionTaskView, SessionView,
};

const DASHBOARD_TASK_EVENT_LIMIT: usize = 12;
const DASHBOARD_TASK_MEMORY_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_REVIEW_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_VIOLATION_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_HANDOFF_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_CLAIM_LIMIT: usize = 6;

impl QueryHost {
    pub(crate) fn dashboard_summary_view(&self) -> Result<DashboardSummaryView> {
        let session = dashboard_session_view(self, None);
        let runtime = runtime_status(self)?;
        let active = self.dashboard_state().active_operations();
        let active_query_count = active.iter().filter(|op| op.kind == "query").count();
        let active_mutation_count = active.iter().filter(|op| op.kind == "mutation").count();
        let recent_queries = self.query_log_entries(crate::QueryLogArgs {
            limit: Some(10),
            since: None,
            target: None,
            operation: None,
            task_id: None,
            min_duration_ms: None,
        });
        let recent_query_error_count = recent_queries.iter().filter(|entry| !entry.success).count();
        let last_runtime_event = runtime_timeline(
            self,
            RuntimeTimelineArgs {
                limit: Some(1),
                contains: None,
            },
        )?
        .pop();

        Ok(DashboardSummaryView {
            session,
            runtime,
            active_query_count,
            active_mutation_count,
            recent_query_error_count,
            last_runtime_event,
        })
    }

    pub(crate) fn dashboard_task_snapshot(
        &self,
        active_session: Option<&SessionState>,
    ) -> Result<DashboardTaskSnapshotView> {
        let session = dashboard_session_view(self, active_session);
        let journal = session
            .current_task
            .as_ref()
            .and_then(|task| {
                active_session
                    .map(|active_session| current_task_journal(self, active_session, &task.task_id))
            })
            .transpose()?;
        Ok(DashboardTaskSnapshotView { session, journal })
    }

    pub(crate) fn dashboard_coordination_summary(
        &self,
    ) -> Result<DashboardCoordinationSummaryView> {
        if !self.features.coordination_layer_enabled() {
            return Ok(DashboardCoordinationSummaryView::disabled());
        }

        let prism = self.current_prism();
        let now = current_timestamp();
        let fallback_snapshot = prism.coordination_snapshot();
        let read_model = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.load_coordination_read_model().ok().flatten())
            .unwrap_or_else(|| fallback_coordination_read_model(&fallback_snapshot));
        let queue_model = self
            .workspace
            .as_ref()
            .and_then(|workspace| {
                workspace
                    .load_coordination_queue_read_model()
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| fallback_coordination_queue_read_model(&fallback_snapshot));
        let ready_task_count =
            ready_task_count_for_active_plans(&read_model.active_plans, |plan_id| {
                prism.ready_tasks(plan_id, now).len()
            });
        let recent_pending_reviews = read_model
            .pending_review_artifacts
            .iter()
            .take(DASHBOARD_COORDINATION_REVIEW_LIMIT)
            .cloned()
            .map(artifact_view)
            .collect();
        let recent_violations = read_model
            .recent_violations
            .iter()
            .take(DASHBOARD_COORDINATION_VIOLATION_LIMIT)
            .cloned()
            .map(policy_violation_record_view)
            .collect::<Vec<_>>();
        let active_claim_count = read_model
            .active_claims
            .iter()
            .filter(|claim| claim.status == ClaimStatus::Active && claim.expires_at > now)
            .count();

        Ok(DashboardCoordinationSummaryView {
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

    pub(crate) fn dashboard_coordination_queues(&self) -> Result<DashboardCoordinationQueuesView> {
        if !self.features.coordination_layer_enabled() {
            return Ok(DashboardCoordinationQueuesView::disabled());
        }

        let prism = self.current_prism();
        let fallback_snapshot = prism.coordination_snapshot();
        let queue_model = self
            .workspace
            .as_ref()
            .and_then(|workspace| {
                workspace
                    .load_coordination_queue_read_model()
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| fallback_coordination_queue_read_model(&fallback_snapshot));

        let pending_handoffs = queue_model
            .pending_handoff_tasks
            .iter()
            .take(DASHBOARD_COORDINATION_HANDOFF_LIMIT)
            .cloned()
            .map(coordination_task_view)
            .collect();
        let active_claims = queue_model
            .active_claims
            .iter()
            .take(DASHBOARD_COORDINATION_CLAIM_LIMIT)
            .cloned()
            .map(claim_view)
            .collect();
        let pending_reviews = queue_model
            .pending_review_artifacts
            .iter()
            .take(DASHBOARD_COORDINATION_REVIEW_LIMIT)
            .cloned()
            .map(artifact_view)
            .collect();

        Ok(DashboardCoordinationQueuesView {
            enabled: true,
            pending_handoffs,
            active_claims,
            pending_reviews,
        })
    }

    pub(crate) fn publish_dashboard_task_update(&self, session: &SessionState) -> Result<()> {
        let snapshot = self.dashboard_task_snapshot(Some(session))?;
        self.dashboard_state()
            .publish_value("task.updated", serde_json::to_value(snapshot)?);
        Ok(())
    }

    pub(crate) fn publish_dashboard_coordination_update(&self) -> Result<()> {
        let summary = self.dashboard_coordination_summary()?;
        self.dashboard_state()
            .publish_value("coordination.updated", serde_json::to_value(summary)?);
        let queues = self.dashboard_coordination_queues()?;
        self.dashboard_state()
            .publish_value("coordination.queues.updated", serde_json::to_value(queues)?);
        Ok(())
    }
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

fn dashboard_session_view(host: &QueryHost, session: Option<&SessionState>) -> SessionView {
    let limits = session
        .map(SessionState::limits)
        .unwrap_or(host.default_limits);
    SessionView {
        workspace_root: host
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().display().to_string()),
        current_task: session.and_then(|session| {
            session.current_task_state().map(|task| SessionTaskView {
                task_id: task.id.0.to_string(),
                description: task.description,
                tags: task.tags,
                coordination_task_id: task.coordination_task_id,
            })
        }),
        current_agent: session
            .and_then(|session| session.current_agent().map(|agent| agent.0.to_string())),
        limits: SessionLimitsView {
            max_result_nodes: limits.max_result_nodes,
            max_call_graph_depth: limits.max_call_graph_depth,
            max_output_json_bytes: limits.max_output_json_bytes,
        },
        features: FeatureFlagsView {
            mode: host.features.mode_label().to_string(),
            coordination: CoordinationFeaturesView {
                workflow: host.features.coordination.workflow,
                claims: host.features.coordination.claims,
                artifacts: host.features.coordination.artifacts,
            },
            internal_developer: host.features.internal_developer,
        },
    }
}

fn current_task_journal(
    host: &QueryHost,
    session: &SessionState,
    task_id: &str,
) -> Result<TaskJournalView> {
    let prism = host.current_prism();
    let task_id = TaskId::new(task_id.to_string());
    task_journal_view(
        session,
        prism.as_ref(),
        &task_id,
        None,
        DASHBOARD_TASK_EVENT_LIMIT,
        DASHBOARD_TASK_MEMORY_LIMIT,
    )
}
