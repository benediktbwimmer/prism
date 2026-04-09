use prism_coordination::{CoordinationSnapshot, CoordinationSnapshotV2, RuntimeDescriptor};
use prism_history::HistoryStore;
use prism_ir::{PrismRuntimeCapabilities, WorkspaceRevision};
use prism_memory::OutcomeMemory;
use prism_projections::{IntentIndex, ProjectionIndex};
use prism_query::Prism;
use prism_store::{CoordinationPersistContext, Graph};
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
    pub(crate) canonical_snapshot_v2: CoordinationSnapshotV2,
    pub(crate) runtime_descriptors: Vec<RuntimeDescriptor>,
    pub(crate) projections: ProjectionIndex,
    pub(crate) runtime_capabilities: PrismRuntimeCapabilities,
}

impl WorkspaceRuntimeState {
    pub(crate) fn new(
        layout: WorkspaceLayout,
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination_snapshot: CoordinationSnapshot,
        runtime_descriptors: Vec<RuntimeDescriptor>,
        projections: ProjectionIndex,
        runtime_capabilities: PrismRuntimeCapabilities,
    ) -> Self {
        let canonical_snapshot_v2 = coordination_snapshot.to_canonical_snapshot_v2();
        Self::new_with_coordination_state(
            layout,
            graph,
            history,
            outcomes,
            coordination_snapshot,
            canonical_snapshot_v2,
            runtime_descriptors,
            projections,
            runtime_capabilities,
        )
    }

    pub(crate) fn new_with_coordination_state(
        layout: WorkspaceLayout,
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination_snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
        runtime_descriptors: Vec<RuntimeDescriptor>,
        projections: ProjectionIndex,
        runtime_capabilities: PrismRuntimeCapabilities,
    ) -> Self {
        Self {
            layout: Arc::new(layout),
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
            coordination_snapshot,
            canonical_snapshot_v2,
            runtime_descriptors,
            projections,
            runtime_capabilities,
        }
    }

    pub(crate) fn layout(&self) -> WorkspaceLayout {
        Arc::as_ref(&self.layout).clone()
    }

    pub(crate) fn placeholder_with_layout_and_capabilities(
        layout: WorkspaceLayout,
        runtime_capabilities: PrismRuntimeCapabilities,
    ) -> Self {
        Self::new(
            layout,
            Graph::default(),
            HistoryStore::default(),
            OutcomeMemory::default(),
            CoordinationSnapshot::default(),
            Vec::new(),
            ProjectionIndex::default(),
            runtime_capabilities,
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
            Prism::with_shared_history_outcomes_coordination_projections_and_query_state_v2(
            Arc::clone(&self.graph),
            Arc::clone(&self.history),
            Arc::clone(&self.outcomes),
            self.coordination_snapshot.clone(),
            self.canonical_snapshot_v2.clone(),
            self.projections.clone(),
            self.runtime_descriptors.clone(),
            intent_override,
            false,
        );
        prism.set_runtime_capabilities(self.runtime_capabilities);
        prism.set_workspace_revision(workspace_revision.clone());
        prism.set_coordination_context(coordination_context.clone());
        WorkspacePublishedGeneration::new(prism, workspace_revision, coordination_context)
    }

    pub(crate) fn sanitize_for_runtime_capabilities(&mut self) {
        if !self.runtime_capabilities.knowledge_storage_enabled() {
            self.graph = Arc::new(Graph::default());
            self.history = Arc::new(HistoryStore::default());
            self.outcomes = Arc::new(OutcomeMemory::default());
            self.projections = ProjectionIndex::default();
        }
    }

    pub(crate) fn replace_coordination_runtime(
        &mut self,
        snapshot: CoordinationSnapshot,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) {
        let canonical_snapshot_v2 = snapshot.to_canonical_snapshot_v2();
        self.replace_coordination_runtime_with_snapshot_v2(
            snapshot,
            canonical_snapshot_v2,
            runtime_descriptors,
        );
    }

    pub(crate) fn replace_coordination_runtime_with_snapshot_v2(
        &mut self,
        snapshot: CoordinationSnapshot,
        canonical_snapshot_v2: CoordinationSnapshotV2,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) {
        self.coordination_snapshot = snapshot;
        self.canonical_snapshot_v2 = canonical_snapshot_v2;
        self.runtime_descriptors = runtime_descriptors;
    }

    pub(crate) fn overlay_live_projection_knowledge(&mut self, prism: &Prism) {
        if !self.runtime_capabilities.knowledge_storage_enabled() {
            return;
        }
        self.projections
            .replace_curated_concepts(prism.curated_concepts_snapshot());
        self.projections
            .replace_concept_relations(prism.concept_relations_snapshot());
        self.projections
            .replace_curated_contracts(prism.curated_contracts());
    }

    pub(crate) fn apply_outcome_event(&mut self, event: &prism_memory::OutcomeEvent) {
        if !self.runtime_capabilities.knowledge_storage_enabled() {
            return;
        }
        self.projections
            .apply_outcome_event(event, |node| self.history.lineage_of(node));
        let _ = Arc::make_mut(&mut self.outcomes).store_event(event.clone());
    }
}
