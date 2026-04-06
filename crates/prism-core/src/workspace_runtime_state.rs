use prism_coordination::CoordinationSnapshot;
use prism_history::HistoryStore;
use prism_ir::{PlanExecutionOverlay, PlanGraph, WorkspaceRevision};
use prism_memory::OutcomeMemory;
use prism_projections::{IntentIndex, ProjectionIndex};
use prism_query::Prism;
use prism_store::{CoordinationPersistContext, Graph};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::layout::WorkspaceLayout;

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct WorkspacePublishedGeneration {
    prism: Arc<Prism>,
    pub(crate) workspace_revision: WorkspaceRevision,
    pub(crate) coordination_context: Option<CoordinationPersistContext>,
}

impl WorkspacePublishedGeneration {
    pub(crate) fn new(
        prism: Prism,
        workspace_revision: WorkspaceRevision,
        coordination_context: Option<CoordinationPersistContext>,
    ) -> Self {
        Self {
            prism: Arc::new(prism),
            workspace_revision,
            coordination_context,
        }
    }

    pub(crate) fn prism_arc(&self) -> Arc<Prism> {
        Arc::clone(&self.prism)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn workspace_revision(&self) -> WorkspaceRevision {
        self.workspace_revision.clone()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn coordination_context(&self) -> Option<CoordinationPersistContext> {
        self.coordination_context.clone()
    }
}

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeState {
    pub(crate) layout: Arc<WorkspaceLayout>,
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
        layout: WorkspaceLayout,
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination_snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
        projections: ProjectionIndex,
    ) -> Self {
        Self {
            layout: Arc::new(layout),
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
            coordination_snapshot,
            plan_graphs,
            plan_execution_overlays,
            projections,
        }
    }

    pub(crate) fn layout(&self) -> WorkspaceLayout {
        Arc::as_ref(&self.layout).clone()
    }

    pub(crate) fn placeholder_with_layout(layout: WorkspaceLayout) -> Self {
        Self::new(
            layout,
            Graph::default(),
            HistoryStore::default(),
            OutcomeMemory::default(),
            CoordinationSnapshot::default(),
            Vec::new(),
            BTreeMap::new(),
            ProjectionIndex::default(),
        )
    }

    pub(crate) fn publish_generation(
        &self,
        workspace_revision: WorkspaceRevision,
        coordination_context: Option<CoordinationPersistContext>,
    ) -> WorkspacePublishedGeneration {
        self.publish_generation_with_intent(workspace_revision, coordination_context, None)
    }

    pub(crate) fn publish_generation_with_intent(
        &self,
        workspace_revision: WorkspaceRevision,
        coordination_context: Option<CoordinationPersistContext>,
        intent_override: Option<IntentIndex>,
    ) -> WorkspacePublishedGeneration {
        let prism =
            Prism::with_shared_history_outcomes_coordination_projections_and_plan_graphs_and_intent(
                Arc::clone(&self.graph),
                Arc::clone(&self.history),
                Arc::clone(&self.outcomes),
                self.coordination_snapshot.clone(),
                self.projections.clone(),
                self.plan_graphs.clone(),
                self.plan_execution_overlays.clone(),
                intent_override,
            );
        prism.set_workspace_revision(workspace_revision.clone());
        prism.set_coordination_context(coordination_context.clone());
        WorkspacePublishedGeneration::new(prism, workspace_revision, coordination_context)
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
