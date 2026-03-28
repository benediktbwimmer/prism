mod common;
mod coordination;
mod impact;
mod intent;
mod outcomes;
mod plan_runtime;
mod source;
mod symbol;
mod types;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use prism_coordination::{CoordinationSnapshot, CoordinationStore};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{AnchorRef, LineageEvent, LineageId, NodeId, PlanExecutionOverlay, PlanGraph};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot};
pub use prism_projections::ConceptResolution;
use prism_projections::{IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::Graph;
use tracing::info;

use crate::common::{anchor_sort_key, dedupe_node_ids, sort_node_ids};
use crate::plan_runtime::NativePlanRuntimeState;

pub use crate::source::{
    source_excerpt_for_line_range, source_excerpt_for_span, source_location_for_span,
    source_slice_around_line, EditSlice, EditSliceOptions, SourceExcerpt, SourceExcerptOptions,
    SourceLocation,
};
pub use crate::symbol::{Relations, Symbol};
pub use crate::types::{
    canonical_concept_handle, ArtifactRisk, ChangeImpact, CoChange, ConceptDecodeLens,
    ConceptEvent, ConceptEventAction, ConceptPacket, ConceptProvenance, ConceptPublication,
    ConceptPublicationStatus, ConceptScope, DriftCandidate, QueryLimits, TaskIntent, TaskRisk,
    TaskValidationRecipe, ValidationCheck, ValidationRecipe,
};

pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    outcomes: Arc<OutcomeMemory>,
    coordination: Arc<CoordinationStore>,
    plan_runtime: RwLock<NativePlanRuntimeState>,
    projections: RwLock<ProjectionIndex>,
    intent: RwLock<IntentIndex>,
}

impl Prism {
    pub fn new(graph: Graph) -> Self {
        let mut history = HistoryStore::new();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        Self::with_history(graph, history)
    }

    pub fn with_history(graph: Graph, history: HistoryStore) -> Self {
        Self::with_history_and_outcomes(graph, history, OutcomeMemory::new())
    }

    pub fn with_history_and_outcomes(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
    ) -> Self {
        let projections = ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot());
        Self::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            CoordinationStore::new(),
            projections,
        )
    }

    pub fn with_history_outcomes_and_projections(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        projections: ProjectionIndex,
    ) -> Self {
        Self::with_history_outcomes_coordination_and_projections(
            graph,
            history,
            outcomes,
            CoordinationStore::new(),
            projections,
        )
    }

    pub fn with_history_outcomes_coordination_and_projections(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationStore,
        projections: ProjectionIndex,
    ) -> Self {
        let native_plans = NativePlanRuntimeState::from_coordination_snapshot(&coordination.snapshot());
        Self::with_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            coordination,
            projections,
            native_plans,
        )
    }

    pub fn with_history_outcomes_coordination_projections_and_plan_graphs(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationStore,
        projections: ProjectionIndex,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        Self::with_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            coordination,
            projections,
            NativePlanRuntimeState::from_graphs_and_overlays(plan_graphs, execution_overlays),
        )
    }

    fn with_history_outcomes_coordination_projections_and_native_plans(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationStore,
        mut projections: ProjectionIndex,
        native_plans: NativePlanRuntimeState,
    ) -> Self {
        projections.reseed_from_history(&history.snapshot());
        let started = Instant::now();
        let node_count = graph.node_count();
        let edge_count = graph.edge_count();
        let file_count = graph.file_count();
        let intent_started = Instant::now();
        let intent = IntentIndex::derive(
            graph.all_nodes().collect::<Vec<_>>(),
            graph.edges.iter().collect::<Vec<_>>(),
        );
        let derive_intent_ms = intent_started.elapsed().as_millis();
        info!(
            node_count,
            edge_count,
            file_count,
            derive_intent_ms,
            total_ms = started.elapsed().as_millis(),
            "built prism query state"
        );
        Self {
            graph: Arc::new(graph),
            history: Arc::new(history),
            outcomes: Arc::new(outcomes),
            coordination: Arc::new(coordination),
            plan_runtime: RwLock::new(native_plans),
            projections: RwLock::new(projections),
            intent: RwLock::new(intent),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId> {
        self.history.lineage_of(node)
    }

    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.history.lineage_history(lineage)
    }

    pub fn current_nodes_for_lineage(&self, lineage: &LineageId) -> Vec<NodeId> {
        self.history.current_nodes_for_lineage(lineage)
    }

    pub fn outcome_memory(&self) -> Arc<OutcomeMemory> {
        Arc::clone(&self.outcomes)
    }

    pub fn coordination(&self) -> Arc<CoordinationStore> {
        Arc::clone(&self.coordination)
    }

    pub fn anchors_for(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        self.expand_anchors(anchors)
    }

    pub fn history_snapshot(&self) -> HistorySnapshot {
        self.history.snapshot()
    }

    pub fn outcome_snapshot(&self) -> OutcomeMemorySnapshot {
        self.outcomes.snapshot()
    }

    pub fn coordination_snapshot(&self) -> CoordinationSnapshot {
        self.coordination.snapshot()
    }

    pub fn replace_coordination_snapshot(&self, snapshot: CoordinationSnapshot) {
        let native_plans = NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        self.coordination.replace_from_snapshot(snapshot);
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") = native_plans;
    }

    pub fn replace_coordination_snapshot_and_plan_graphs(
        &self,
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        self.coordination.replace_from_snapshot(snapshot);
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") =
            NativePlanRuntimeState::from_graphs_and_overlays(plan_graphs, execution_overlays);
    }

    pub fn refresh_plan_runtime_from_coordination(&self) {
        let snapshot = self.coordination.snapshot();
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") =
            NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
    }

    pub fn projection_snapshot(&self) -> ProjectionSnapshot {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .snapshot()
    }

    pub fn refresh_projections(&self) {
        let curated = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .curated_concepts()
            .to_vec();
        let next = ProjectionIndex::derive_with_curated(
            &self.history.snapshot(),
            &self.outcomes.snapshot(),
            curated,
        );
        *self.projections.write().expect("projection lock poisoned") = next;
    }

    pub fn replace_curated_concepts(&self, concepts: Vec<ConceptPacket>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_curated_concepts(concepts);
    }

    pub fn upsert_curated_concept(&self, concept: ConceptPacket) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .upsert_curated_concept(concept);
    }

    pub fn apply_outcome_event_to_projections(&self, event: &OutcomeEvent) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_outcome_event(event, |node| self.history.lineage_of(node));
    }

    pub fn apply_lineage_events_to_projections(&self, events: &[LineageEvent]) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_lineage_events(events);
    }

    pub(crate) fn expand_anchors(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        let mut expanded = anchors.to_vec();
        for anchor in anchors {
            if let AnchorRef::Node(node) = anchor {
                if let Some(lineage) = self.lineage_of(node) {
                    expanded.push(AnchorRef::Lineage(lineage));
                }
            }
        }
        expanded.sort_by(anchor_sort_key);
        expanded.dedup();
        expanded
    }

    pub(crate) fn graph_neighbors(&self, node: &NodeId) -> Vec<NodeId> {
        let mut neighbors = self
            .graph
            .edges_from(node, None)
            .into_iter()
            .map(|edge| edge.target.clone())
            .chain(
                self.graph
                    .edges_to(node, None)
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            )
            .collect::<Vec<_>>();
        sort_node_ids(&mut neighbors);
        neighbors
    }

    pub(crate) fn resolve_anchor_nodes(&self, anchors: &[AnchorRef]) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        for anchor in anchors {
            match anchor {
                AnchorRef::Node(node) => nodes.push(node.clone()),
                AnchorRef::Lineage(lineage) => {
                    nodes.extend(self.history.current_nodes_for_lineage(lineage));
                }
                AnchorRef::File(file) => {
                    nodes.extend(
                        self.graph
                            .all_nodes()
                            .filter(|node| node.file == *file)
                            .map(|node| node.id.clone()),
                    );
                }
                AnchorRef::Kind(kind) => {
                    nodes.extend(
                        self.graph
                            .all_nodes()
                            .filter(|node| node.kind == *kind)
                            .map(|node| node.id.clone()),
                    );
                }
            }
        }
        let mut nodes = dedupe_node_ids(nodes);
        sort_node_ids(&mut nodes);
        nodes
    }

    pub fn concepts(&self, query: &str, limit: usize) -> Vec<ConceptPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .concepts(query, limit)
    }

    pub fn resolve_concepts(&self, query: &str, limit: usize) -> Vec<ConceptResolution> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .resolve_concepts(query, limit)
    }

    pub fn concept(&self, query: &str) -> Option<ConceptPacket> {
        self.concepts(query, 1).into_iter().next()
    }

    pub fn resolve_concept(&self, query: &str) -> Option<ConceptResolution> {
        self.resolve_concepts(query, 1).into_iter().next()
    }

    pub fn concept_by_handle(&self, handle: &str) -> Option<ConceptPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .concept_by_handle(handle)
    }
}
