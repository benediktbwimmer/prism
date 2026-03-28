use prism_ir::{CoordinationTaskId, PlanNodeId, PlanScope, PlanStatus};

use crate::{NativePlanRuntimeState, PlanListEntry, Prism};

impl Prism {
    pub fn plans(
        &self,
        status: Option<PlanStatus>,
        scope: Option<PlanScope>,
        contains: Option<&str>,
    ) -> Vec<PlanListEntry> {
        let runtime = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .clone();
        self.plans_for_runtime(&runtime, status, scope, contains)
    }

    fn plans_for_runtime(
        &self,
        runtime: &NativePlanRuntimeState,
        status: Option<PlanStatus>,
        scope: Option<PlanScope>,
        contains: Option<&str>,
    ) -> Vec<PlanListEntry> {
        let contains = contains
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());

        let mut plans = runtime
            .plan_graphs()
            .into_iter()
            .filter(|graph| status.is_none_or(|expected| graph.status == expected))
            .filter(|graph| scope.is_none_or(|expected| graph.scope == expected))
            .filter(|graph| {
                contains.as_ref().is_none_or(|needle| {
                    graph.id.0.to_ascii_lowercase().contains(needle)
                        || graph.title.to_ascii_lowercase().contains(needle)
                        || graph.goal.to_ascii_lowercase().contains(needle)
                })
            })
            .filter_map(|graph| {
                let summary = self.plan_summary_for_runtime(runtime, &graph.id)?;
                Some(PlanListEntry {
                    plan_id: graph.id.clone(),
                    title: graph.title,
                    goal: graph.goal,
                    status: graph.status,
                    scope: graph.scope,
                    kind: graph.kind,
                    root_task_ids: root_task_ids(&graph.root_nodes),
                    summary,
                })
            })
            .collect::<Vec<_>>();

        plans.sort_by(|left, right| {
            plan_status_rank(left.status)
                .cmp(&plan_status_rank(right.status))
                .then_with(|| {
                    right
                        .summary
                        .actionable_nodes
                        .cmp(&left.summary.actionable_nodes)
                })
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.plan_id.0.cmp(&right.plan_id.0))
        });
        plans
    }
}

fn root_task_ids(root_nodes: &[PlanNodeId]) -> Vec<CoordinationTaskId> {
    root_nodes
        .iter()
        .map(|node_id| CoordinationTaskId::new(node_id.0.clone()))
        .collect()
}

fn plan_status_rank(status: PlanStatus) -> u8 {
    match status {
        PlanStatus::Active => 0,
        PlanStatus::Blocked => 1,
        PlanStatus::Draft => 2,
        PlanStatus::Completed => 3,
        PlanStatus::Abandoned => 4,
    }
}
