use std::collections::BTreeMap;

use prism_coordination::{execution_overlays_from_tasks, snapshot_plan_graphs, CoordinationSnapshot};
use prism_ir::{PlanExecutionOverlay, PlanGraph, PlanId};

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePlanRuntimeState {
    graphs: BTreeMap<String, PlanGraph>,
    execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
}

impl NativePlanRuntimeState {
    pub(crate) fn from_coordination_snapshot(snapshot: &CoordinationSnapshot) -> Self {
        let graphs = snapshot_plan_graphs(snapshot);
        let execution_overlays = snapshot
            .tasks
            .iter()
            .cloned()
            .fold(BTreeMap::new(), |mut map, task| {
                map.entry(task.plan.0.to_string()).or_insert_with(Vec::new).push(task);
                map
            })
            .into_iter()
            .map(|(plan_id, tasks)| {
                (
                    plan_id,
                    sort_execution_overlays(execution_overlays_from_tasks(&tasks)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        Self::from_graphs_and_overlays(graphs, execution_overlays)
    }

    pub(crate) fn from_graphs_and_overlays(
        graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let graphs = graphs
            .into_iter()
            .map(|graph| (graph.id.0.to_string(), graph))
            .collect::<BTreeMap<_, _>>();
        let execution_overlays = execution_overlays
            .into_iter()
            .map(|(plan_id, overlays)| (plan_id, sort_execution_overlays(overlays)))
            .collect::<BTreeMap<_, _>>();
        Self {
            graphs,
            execution_overlays,
        }
    }

    pub(crate) fn plan_graph(&self, plan_id: &PlanId) -> Option<PlanGraph> {
        self.graphs.get(plan_id.0.as_str()).cloned()
    }

    pub(crate) fn plan_execution(&self, plan_id: &PlanId) -> Vec<PlanExecutionOverlay> {
        self.execution_overlays
            .get(plan_id.0.as_str())
            .cloned()
            .unwrap_or_default()
    }
}

fn sort_execution_overlays(
    mut overlays: Vec<PlanExecutionOverlay>,
) -> Vec<PlanExecutionOverlay> {
    overlays.sort_by(|left, right| left.node_id.0.cmp(&right.node_id.0));
    overlays
}
