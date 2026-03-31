use prism_coordination::CoordinationSnapshot;
use prism_history::HistoryStore;
use prism_ir::{PlanExecutionOverlay, PlanGraph, WorkspaceRevision};
use prism_memory::OutcomeMemory;
use prism_projections::ProjectionIndex;
use prism_query::Prism;
use prism_store::{CoordinationPersistContext, Graph};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct WorkspaceRuntimeState {
    pub(crate) graph: Arc<Graph>,
    pub(crate) history: Arc<HistoryStore>,
    pub(crate) outcomes: Arc<OutcomeMemory>,
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
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
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
        let prism = Prism::with_shared_history_outcomes_coordination_projections_and_plan_graphs(
            Arc::clone(&self.graph),
            Arc::clone(&self.history),
            Arc::clone(&self.outcomes),
            self.coordination_snapshot.clone(),
            self.projections.clone(),
            self.plan_graphs.clone(),
            self.plan_execution_overlays.clone(),
        );
        prism.set_workspace_revision(workspace_revision);
        prism.set_coordination_context(coordination_context);
        prism
    }

    pub(crate) fn replace_coordination_runtime(
        &mut self,
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        self.coordination_snapshot = snapshot;
        self.plan_graphs = plan_graphs;
        self.plan_execution_overlays = plan_execution_overlays;
    }

    pub(crate) fn overlay_live_projection_knowledge(&mut self, prism: &Prism) {
        self.projections
            .replace_curated_concepts(prism.curated_concepts_snapshot());
        self.projections
            .replace_concept_relations(prism.concept_relations_snapshot());
        self.projections
            .replace_curated_contracts(prism.curated_contracts());
    }

    pub(crate) fn apply_outcome_event(&mut self, event: &prism_memory::OutcomeEvent) {
        self.projections
            .apply_outcome_event(event, |node| self.history.lineage_of(node));
        let _ = Arc::make_mut(&mut self.outcomes).store_event(event.clone());
    }
}
