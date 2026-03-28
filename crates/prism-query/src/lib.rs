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

use anyhow::Result;
use prism_coordination::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    CoordinationConflict, CoordinationRuntimeState, CoordinationSnapshot, CoordinationStore,
    CoordinationTask, HandoffAcceptInput, HandoffInput, TaskCreateInput, TaskUpdateInput,
    WorkClaim,
};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AgentId, ArtifactId, AnchorRef, ClaimId, EventMeta, LineageEvent, LineageId, NodeId,
    PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanId, PlanNodeId, PlanNodeStatus, ReviewId,
    SessionId, WorkspaceRevision,
};
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
        let coordination_snapshot = coordination.snapshot();
        Self::with_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            coordination,
            projections,
            NativePlanRuntimeState::from_snapshot_with_graphs_and_overlays(
                &coordination_snapshot,
                plan_graphs,
                execution_overlays,
            ),
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

    fn mutate_native_plan_runtime<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut NativePlanRuntimeState) -> Result<T>,
    {
        let snapshot = self.coordination.snapshot();
        self.mutate_native_plan_runtime_from_snapshot(snapshot, mutate)
    }

    fn mutate_native_plan_runtime_from_snapshot<T, F>(
        &self,
        base_snapshot: CoordinationSnapshot,
        mutate: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut NativePlanRuntimeState) -> Result<T>,
    {
        let mut runtime = self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned");
        let result = mutate(&mut runtime)?;
        let snapshot = runtime.apply_to_coordination_snapshot(base_snapshot);
        self.coordination.replace_from_snapshot(snapshot);
        Ok(result)
    }

    fn persist_native_plan_runtime_against_snapshot(
        &self,
        snapshot: CoordinationSnapshot,
    ) -> Result<()> {
        self.mutate_native_plan_runtime_from_snapshot(snapshot, |_| Ok(()))
    }

    fn mutate_validated_coordination_snapshot<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut CoordinationRuntimeState) -> Result<T>,
    {
        let mut runtime = CoordinationRuntimeState::from_snapshot(self.coordination.snapshot());
        match mutate(&mut runtime) {
            Ok(result) => {
                self.persist_native_plan_runtime_against_snapshot(runtime.snapshot())?;
                Ok(result)
            }
            Err(error) => {
                self.persist_native_plan_runtime_against_snapshot(runtime.snapshot())?;
                Err(error)
            }
        }
    }

    pub fn create_native_task(
        &self,
        meta: EventMeta,
        input: TaskCreateInput,
    ) -> Result<CoordinationTask> {
        let store = CoordinationStore::from_snapshot(self.coordination.snapshot());
        match store.create_task(meta, input) {
            Ok((_, task)) => {
                let snapshot = store.snapshot();
                self.mutate_native_plan_runtime_from_snapshot(snapshot, |runtime| {
                    runtime.create_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
            }
            Err(error) => {
                self.persist_native_plan_runtime_against_snapshot(store.snapshot())?;
                Err(error)
            }
        }
    }

    pub fn update_native_task(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: u64,
    ) -> Result<CoordinationTask> {
        let store = CoordinationStore::from_snapshot(self.coordination.snapshot());
        match store.update_task(meta, input, current_revision, now) {
            Ok(task) => {
                let snapshot = store.snapshot();
                self.mutate_native_plan_runtime_from_snapshot(snapshot, |runtime| {
                    runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
            }
            Err(error) => {
                self.persist_native_plan_runtime_against_snapshot(store.snapshot())?;
                Err(error)
            }
        }
    }

    pub fn request_native_handoff(
        &self,
        meta: EventMeta,
        input: HandoffInput,
        current_revision: WorkspaceRevision,
    ) -> Result<CoordinationTask> {
        let store = CoordinationStore::from_snapshot(self.coordination.snapshot());
        match store.handoff(meta, input, current_revision) {
            Ok(task) => {
                let snapshot = store.snapshot();
                self.mutate_native_plan_runtime_from_snapshot(snapshot, |runtime| {
                    runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
            }
            Err(error) => {
                self.persist_native_plan_runtime_against_snapshot(store.snapshot())?;
                Err(error)
            }
        }
    }

    pub fn accept_native_handoff(
        &self,
        meta: EventMeta,
        input: HandoffAcceptInput,
    ) -> Result<CoordinationTask> {
        let store = CoordinationStore::from_snapshot(self.coordination.snapshot());
        match store.accept_handoff(meta, input) {
            Ok(task) => {
                let snapshot = store.snapshot();
                self.mutate_native_plan_runtime_from_snapshot(snapshot, |runtime| {
                    runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
            }
            Err(error) => {
                self.persist_native_plan_runtime_against_snapshot(store.snapshot())?;
                Err(error)
            }
        }
    }

    pub fn acquire_native_claim(
        &self,
        meta: EventMeta,
        session_id: SessionId,
        input: prism_coordination::ClaimAcquireInput,
    ) -> Result<(Option<ClaimId>, Vec<CoordinationConflict>, Option<WorkClaim>)> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.acquire_claim(meta, session_id, input)
        })
    }

    pub fn renew_native_claim(
        &self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
        ttl_seconds: Option<u64>,
    ) -> Result<WorkClaim> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.renew_claim(meta, session_id, claim_id, ttl_seconds)
        })
    }

    pub fn release_native_claim(
        &self,
        meta: EventMeta,
        session_id: &SessionId,
        claim_id: &ClaimId,
    ) -> Result<WorkClaim> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.release_claim(meta, session_id, claim_id)
        })
    }

    pub fn propose_native_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactProposeInput,
    ) -> Result<(ArtifactId, Artifact)> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.propose_artifact(meta, input)
        })
    }

    pub fn supersede_native_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.supersede_artifact(meta, input)
        })
    }

    pub fn review_native_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactReviewInput,
        current_revision: WorkspaceRevision,
    ) -> Result<(ReviewId, ArtifactReview, Artifact)> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.review_artifact(meta, input, current_revision)
        })
    }

    pub fn create_native_plan_node(
        &self,
        plan_id: &PlanId,
        title: String,
        status: Option<PlanNodeStatus>,
        assignee: Option<AgentId>,
        anchors: Vec<AnchorRef>,
        depends_on: Vec<String>,
        acceptance: Vec<prism_coordination::AcceptanceCriterion>,
        base_revision: WorkspaceRevision,
    ) -> Result<PlanNodeId> {
        self.mutate_native_plan_runtime(|runtime| {
            runtime.create_node(
                plan_id,
                title,
                status,
                assignee,
                anchors,
                depends_on,
                acceptance,
                base_revision,
            )
        })
    }

    pub fn create_native_plan(
        &self,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<PlanId> {
        self.mutate_native_plan_runtime(|runtime| Ok(runtime.create_plan(goal, status, policy)))
    }

    pub fn update_native_plan(
        &self,
        plan_id: &PlanId,
        status: Option<prism_ir::PlanStatus>,
        goal: Option<String>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<()> {
        self.mutate_native_plan_runtime(|runtime| runtime.update_plan(plan_id, status, goal, policy))
    }

    pub fn update_native_plan_node(
        &self,
        node_id: &PlanNodeId,
        status: Option<PlanNodeStatus>,
        assignee: Option<Option<AgentId>>,
        title: Option<String>,
        anchors: Option<Vec<AnchorRef>>,
        depends_on: Option<Vec<String>>,
        acceptance: Option<Vec<prism_coordination::AcceptanceCriterion>>,
        base_revision: Option<WorkspaceRevision>,
    ) -> Result<PlanId> {
        self.mutate_native_plan_runtime(|runtime| {
            runtime.update_node(
                node_id,
                status,
                assignee,
                title,
                anchors,
                depends_on,
                acceptance,
                base_revision,
            )
        })
    }

    pub fn create_native_plan_edge(
        &self,
        plan_id: &PlanId,
        from_node_id: &PlanNodeId,
        to_node_id: &PlanNodeId,
        kind: PlanEdgeKind,
    ) -> Result<()> {
        self.mutate_native_plan_runtime(|runtime| {
            runtime.create_edge(plan_id, from_node_id, to_node_id, kind)
        })
    }

    pub fn delete_native_plan_edge(
        &self,
        plan_id: &PlanId,
        from_node_id: &PlanNodeId,
        to_node_id: &PlanNodeId,
        kind: PlanEdgeKind,
    ) -> Result<()> {
        self.mutate_native_plan_runtime(|runtime| {
            runtime.delete_edge(plan_id, from_node_id, to_node_id, kind)
        })
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
