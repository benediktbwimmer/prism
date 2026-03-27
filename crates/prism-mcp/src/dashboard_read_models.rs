use anyhow::Result;
use prism_ir::{ArtifactStatus, ClaimStatus, CoordinationTaskStatus, PlanStatus, TaskId};
use prism_js::TaskJournalView;

use crate::dashboard_types::{
    DashboardCoordinationSummaryView, DashboardSummaryView, DashboardTaskSnapshotView,
};
use crate::runtime_views::{runtime_status, runtime_timeline};
use crate::{
    artifact_view, current_timestamp, policy_violation_record_view, task_journal_view,
    CoordinationFeaturesView, FeatureFlagsView, QueryHost, RuntimeTimelineArgs,
    SessionLimitsView, SessionTaskView, SessionView,
};

const DASHBOARD_TASK_EVENT_LIMIT: usize = 12;
const DASHBOARD_TASK_MEMORY_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_REVIEW_LIMIT: usize = 6;
const DASHBOARD_COORDINATION_VIOLATION_LIMIT: usize = 6;

impl QueryHost {
    pub(crate) fn dashboard_summary_view(&self) -> Result<DashboardSummaryView> {
        let session = dashboard_session_view(self);
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

    pub(crate) fn dashboard_task_snapshot(&self) -> Result<DashboardTaskSnapshotView> {
        let session = dashboard_session_view(self);
        let journal = session
            .current_task
            .as_ref()
            .map(|task| current_task_journal(self, &task.task_id))
            .transpose()?;
        Ok(DashboardTaskSnapshotView { session, journal })
    }

    pub(crate) fn dashboard_coordination_summary(&self) -> Result<DashboardCoordinationSummaryView> {
        if !self.features.coordination_layer_enabled() {
            return Ok(DashboardCoordinationSummaryView::disabled());
        }

        let prism = self.current_prism();
        let snapshot = prism.coordination().snapshot();
        let now = current_timestamp();
        let active_plan_ids = snapshot
            .plans
            .iter()
            .filter(|plan| plan.status == PlanStatus::Active)
            .map(|plan| plan.id.clone())
            .collect::<Vec<_>>();
        let ready_task_count = active_plan_ids
            .iter()
            .map(|plan_id| prism.ready_tasks(plan_id, now).len())
            .sum();
        let pending_reviews = prism.pending_reviews(None);
        let pending_review_count = pending_reviews.len();
        let recent_pending_reviews = pending_reviews
            .into_iter()
            .take(DASHBOARD_COORDINATION_REVIEW_LIMIT)
            .map(artifact_view)
            .collect();
        let recent_violations = prism
            .policy_violations(None, None, DASHBOARD_COORDINATION_VIOLATION_LIMIT)
            .into_iter()
            .map(policy_violation_record_view)
            .collect::<Vec<_>>();

        Ok(DashboardCoordinationSummaryView {
            enabled: true,
            active_plan_count: active_plan_ids.len(),
            task_count: snapshot.tasks.len(),
            ready_task_count,
            in_review_task_count: snapshot
                .tasks
                .iter()
                .filter(|task| {
                    matches!(
                        task.status,
                        CoordinationTaskStatus::InReview | CoordinationTaskStatus::Validating
                    )
                })
                .count(),
            active_claim_count: snapshot
                .claims
                .iter()
                .filter(|claim| {
                    claim.status == ClaimStatus::Active && claim.expires_at > now
                })
                .count(),
            pending_review_count,
            proposed_artifact_count: snapshot
                .artifacts
                .iter()
                .filter(|artifact| artifact.status == ArtifactStatus::Proposed)
                .count(),
            recent_pending_reviews,
            recent_violations,
        })
    }

    pub(crate) fn publish_dashboard_task_update(&self) -> Result<()> {
        let snapshot = self.dashboard_task_snapshot()?;
        self.dashboard_state()
            .publish_value("task.updated", serde_json::to_value(snapshot)?);
        Ok(())
    }

    pub(crate) fn publish_dashboard_coordination_update(&self) -> Result<()> {
        let summary = self.dashboard_coordination_summary()?;
        self.dashboard_state()
            .publish_value("coordination.updated", serde_json::to_value(summary)?);
        Ok(())
    }
}

fn dashboard_session_view(host: &QueryHost) -> SessionView {
    let limits = host.session.limits();
    SessionView {
        workspace_root: host
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().display().to_string()),
        current_task: host.session.current_task_state().map(|task| SessionTaskView {
            task_id: task.id.0.to_string(),
            description: task.description,
            tags: task.tags,
        }),
        current_agent: host
            .session
            .current_agent()
            .map(|agent| agent.0.to_string()),
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

fn current_task_journal(host: &QueryHost, task_id: &str) -> Result<TaskJournalView> {
    let prism = host.current_prism();
    let task_id = TaskId::new(task_id.to_string());
    task_journal_view(
        host.session.as_ref(),
        prism.as_ref(),
        &task_id,
        None,
        DASHBOARD_TASK_EVENT_LIMIT,
        DASHBOARD_TASK_MEMORY_LIMIT,
    )
}
