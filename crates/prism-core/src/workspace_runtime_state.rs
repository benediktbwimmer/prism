use std::collections::BTreeMap;
use prism_coordination::CoordinationSnapshot;
use prism_history::HistoryStore;
use prism_ir::{PlanExecutionOverlay, PlanGraph, WorkspaceRevision};
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::{CoordinationPersistContext, Graph};

#[derive(Clone, Default)]
pub(crate) struct WorkspaceRuntimeState {
    pub(crate) graph: Graph,
    pub(crate) history: HistoryStore,
    pub(crate) outcomes: OutcomeMemory,
    pub(crate) coordination_snapshot: CoordinationSnapshot,
    pub(crate) plan_graphs: Vec<PlanGraph>,
    pub(crate) plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    pub(crate) projections: ProjectionIndex,
}

impl WorkspaceRuntimeState {
    pub(crate) fn new(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination_snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
        projections: ProjectionIndex,
    ) -> Self {
        Self {
            graph,
            history,
            outcomes,
            coordination_snapshot,
            plan_graphs,
            plan_execution_overlays,
            projections,
        }
    }

    pub(crate) fn publish_prism(
        &self,
        workspace_revision: WorkspaceRevision,
        coordination_context: Option<CoordinationPersistContext>,
    ) -> Prism {
        let prism = Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
            self.graph.clone(),
            self.history.clone(),
            self.outcomes.clone(),
            self.coordination_snapshot.clone(),
            self.projections.clone(),
            self.plan_graphs.clone(),
            self.plan_execution_overlays.clone(),
        );
        prism.set_workspace_revision(workspace_revision);
        prism.set_coordination_context(coordination_context);
        prism
    }

    pub(crate) fn overlay_live_prism_domains(&mut self, prism: &Prism) {
        self.outcomes = OutcomeMemory::from_snapshot(prism.outcome_snapshot());
        self.coordination_snapshot = prism.coordination_snapshot();
        self.plan_graphs = prism.authored_plan_graphs();
        self.plan_execution_overlays = prism.plan_execution_overlays_by_plan();
        self.projections = ProjectionIndex::from_snapshot(prism.projection_snapshot());
    }
}
