mod common;
mod contracts;
mod coordination;
mod impact;
mod intent;
mod outcomes;
mod plan_bindings;
mod plan_completion;
mod plan_discovery;
mod plan_hydration;
mod plan_insights;
mod plan_runtime;
mod source;
mod symbol;
mod types;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{anyhow, Result};
use prism_coordination::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    CoordinationConflict, CoordinationRuntimeState, CoordinationSnapshot, CoordinationTask,
    HandoffAcceptInput, HandoffInput, TaskCreateInput, TaskReclaimInput, TaskResumeInput,
    TaskUpdateInput, WorkClaim,
};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ClaimId, EventId, EventMeta, LineageEvent, LineageId, NodeId,
    PlanEdgeKind, PlanExecutionOverlay, PlanGraph, PlanId, PlanNodeId, PlanNodeKind,
    PlanNodeStatus, ReviewId, SessionId, TaskId, WorkspaceRevision,
};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot};
use prism_memory::{OutcomeRecallQuery, TaskReplay};
pub use prism_projections::ConceptResolution;
use prism_projections::{IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::{CoordinationPersistContext, Graph};
use tracing::info;

use crate::common::{anchor_sort_key, dedupe_node_ids, sort_node_ids};
use crate::plan_bindings::validate_authored_plan_binding;
use crate::plan_runtime::NativePlanRuntimeState;

pub use crate::source::{
    source_excerpt_for_line_range, source_excerpt_for_span, source_location_for_span,
    source_slice_around_line, EditSlice, EditSliceOptions, SourceExcerpt, SourceExcerptOptions,
    SourceLocation,
};
pub use crate::symbol::{Relations, Symbol};
pub use crate::types::{
    canonical_concept_handle, canonical_contract_handle, ArtifactRisk, ChangeImpact, CoChange,
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptHealth,
    ConceptHealthSignals, ConceptHealthStatus, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent,
    ConceptRelationEventAction, ConceptRelationKind, ConceptScope, ContractCompatibility,
    ContractEvent, ContractEventAction, ContractEventPatch, ContractGuarantee,
    ContractGuaranteeStrength, ContractHealth, ContractHealthSignals, ContractHealthStatus,
    ContractKind, ContractPacket, ContractProvenance, ContractPublication,
    ContractPublicationStatus, ContractResolution, ContractScope, ContractStability,
    ContractStatus, ContractTarget, ContractValidation, DriftCandidate, PlanListEntry,
    PlanNodeRecommendation, PlanSummary, QueryLimits, TaskIntent, TaskRisk, TaskValidationRecipe,
    ValidationCheck, ValidationRecipe,
};

pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    outcomes: Arc<OutcomeMemory>,
    history_backend: RwLock<Option<Arc<dyn HistoryReadBackend>>>,
    outcome_backend: RwLock<Option<Arc<dyn OutcomeReadBackend>>>,
    workspace_revision: RwLock<WorkspaceRevision>,
    plan_runtime: RwLock<NativePlanRuntimeState>,
    continuity_runtime: RwLock<CoordinationRuntimeState>,
    coordination_context: RwLock<Option<CoordinationPersistContext>>,
    projections: RwLock<ProjectionIndex>,
    intent: RwLock<IntentIndex>,
}

pub trait OutcomeReadBackend: Send + Sync {
    fn query_outcomes(&self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>>;
    fn load_outcome_event(&self, event_id: &EventId) -> Result<Option<OutcomeEvent>>;
    fn load_task_replay(&self, task_id: &TaskId) -> Result<TaskReplay>;
}

pub trait HistoryReadBackend: Send + Sync {
    fn load_lineage_history(&self, lineage: &LineageId) -> Result<Vec<LineageEvent>>;
    fn load_history_snapshot(&self) -> Result<Option<HistorySnapshot>>;
}

impl Prism {
    fn validate_native_plan_binding(&self, binding: &prism_ir::PlanBinding) -> Result<()> {
        validate_authored_plan_binding(
            binding,
            |handle| self.concept_by_handle(handle).is_some(),
            |artifact_ref| {
                self.coordination_artifact(&ArtifactId::new(artifact_ref))
                    .is_some()
            },
            |outcome_ref| self.outcome_event(&EventId::new(outcome_ref)).is_some(),
        )
    }

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
            CoordinationSnapshot::default(),
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
            CoordinationSnapshot::default(),
            projections,
        )
    }

    pub fn with_history_outcomes_coordination_and_projections(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
    ) -> Self {
        let native_plans = NativePlanRuntimeState::from_coordination_snapshot(&coordination);
        let continuity_runtime = CoordinationRuntimeState::from_snapshot(coordination);
        Self::with_shared_history_outcomes_coordination_projections_and_native_plans(
            Arc::new(graph),
            Arc::new(history),
            Arc::new(outcomes),
            projections,
            native_plans,
            continuity_runtime,
        )
    }

    pub fn with_history_outcomes_coordination_projections_and_plan_graphs(
        graph: Graph,
        history: HistoryStore,
        outcomes: OutcomeMemory,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        Self::with_shared_history_outcomes_coordination_projections_and_plan_graphs(
            Arc::new(graph),
            Arc::new(history),
            Arc::new(outcomes),
            coordination,
            projections,
            plan_graphs,
            execution_overlays,
        )
    }

    pub fn with_shared_history_outcomes_coordination_projections_and_plan_graphs(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) -> Self {
        let native_plans = NativePlanRuntimeState::from_snapshot_with_graphs_and_overlays(
            &coordination,
            plan_graphs,
            execution_overlays,
        );
        let coordination = native_plans
            .apply_task_execution_authored_fields_to_coordination_snapshot(coordination);
        Self::with_shared_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            projections,
            native_plans,
            CoordinationRuntimeState::from_snapshot(coordination),
        )
    }

    fn with_shared_history_outcomes_coordination_projections_and_native_plans(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        mut projections: ProjectionIndex,
        native_plans: NativePlanRuntimeState,
        continuity_runtime: CoordinationRuntimeState,
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
        let default_workspace_revision = WorkspaceRevision {
            graph_version: history.snapshot().events.len() as u64,
            git_commit: None,
        };
        info!(
            node_count,
            edge_count,
            file_count,
            derive_intent_ms,
            total_ms = started.elapsed().as_millis(),
            "built prism query state"
        );
        Self {
            graph,
            history,
            outcomes,
            history_backend: RwLock::new(None),
            outcome_backend: RwLock::new(None),
            workspace_revision: RwLock::new(default_workspace_revision),
            plan_runtime: RwLock::new(native_plans),
            continuity_runtime: RwLock::new(continuity_runtime),
            coordination_context: RwLock::new(None),
            projections: RwLock::new(projections),
            intent: RwLock::new(intent),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn set_outcome_backend(&self, backend: Option<Arc<dyn OutcomeReadBackend>>) {
        *self
            .outcome_backend
            .write()
            .expect("outcome backend lock poisoned") = backend;
    }

    pub fn set_history_backend(&self, backend: Option<Arc<dyn HistoryReadBackend>>) {
        *self
            .history_backend
            .write()
            .expect("history backend lock poisoned") = backend;
    }

    pub fn lineage_of(&self, node: &NodeId) -> Option<LineageId> {
        self.history.lineage_of(node)
    }

    pub fn hot_lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.history.lineage_history(lineage)
    }

    pub fn cold_lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        self.history_backend
            .read()
            .expect("history backend lock poisoned")
            .as_ref()
            .and_then(|backend| backend.load_lineage_history(lineage).ok())
            .unwrap_or_default()
    }

    pub fn lineage_history(&self, lineage: &LineageId) -> Vec<LineageEvent> {
        let hot = self.hot_lineage_history(lineage);
        let cold = self.cold_lineage_history(lineage);
        merge_lineage_events(hot, cold)
    }

    pub fn current_nodes_for_lineage(&self, lineage: &LineageId) -> Vec<NodeId> {
        self.history.current_nodes_for_lineage(lineage)
    }

    pub fn outcome_memory(&self) -> Arc<OutcomeMemory> {
        Arc::clone(&self.outcomes)
    }

    pub fn hot_outcome_event(&self, event_id: &EventId) -> Option<OutcomeEvent> {
        self.outcomes.event(event_id)
    }

    pub fn cold_outcome_event(&self, event_id: &EventId) -> Option<OutcomeEvent> {
        self.outcome_backend
            .read()
            .expect("outcome backend lock poisoned")
            .as_ref()
            .and_then(|backend| backend.load_outcome_event(event_id).ok().flatten())
    }

    pub fn outcome_event(&self, event_id: &EventId) -> Option<OutcomeEvent> {
        self.hot_outcome_event(event_id)
            .or_else(|| self.cold_outcome_event(event_id))
    }

    pub fn set_coordination_context(&self, context: Option<CoordinationPersistContext>) {
        *self
            .coordination_context
            .write()
            .expect("coordination context lock poisoned") = context;
    }

    pub fn coordination_context(&self) -> Option<CoordinationPersistContext> {
        self.coordination_context
            .read()
            .expect("coordination context lock poisoned")
            .clone()
    }

    pub fn set_workspace_revision(&self, revision: WorkspaceRevision) {
        *self
            .workspace_revision
            .write()
            .expect("workspace revision lock poisoned") = revision;
    }

    pub fn anchors_for(&self, anchors: &[AnchorRef]) -> Vec<AnchorRef> {
        self.expand_anchors(anchors)
    }

    pub fn history_snapshot(&self) -> HistorySnapshot {
        let hot = self.hot_history_snapshot();
        if let Some(cold) = self.cold_history_snapshot() {
            merge_history_snapshots(hot, cold)
        } else {
            hot
        }
    }

    pub fn hot_history_snapshot(&self) -> HistorySnapshot {
        self.history.snapshot()
    }

    pub fn cold_history_snapshot(&self) -> Option<HistorySnapshot> {
        self.history_backend
            .read()
            .expect("history backend lock poisoned")
            .as_ref()
            .and_then(|backend| backend.load_history_snapshot().ok().flatten())
    }

    pub fn outcome_snapshot(&self) -> OutcomeMemorySnapshot {
        self.outcomes.snapshot()
    }

    fn continuity_snapshot(&self) -> CoordinationSnapshot {
        self.continuity_runtime
            .read()
            .expect("continuity runtime lock poisoned")
            .snapshot()
    }

    pub fn coordination_snapshot(&self) -> CoordinationSnapshot {
        self.continuity_snapshot()
    }

    pub fn replace_coordination_snapshot(&self, snapshot: CoordinationSnapshot) {
        let native_plans = NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        let continuity_runtime = CoordinationRuntimeState::from_snapshot(snapshot.clone());
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") = native_plans;
        *self
            .continuity_runtime
            .write()
            .expect("continuity runtime lock poisoned") = continuity_runtime;
    }

    pub fn replace_coordination_snapshot_and_plan_graphs(
        &self,
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        let native_plans = NativePlanRuntimeState::from_snapshot_with_graphs_and_overlays(
            &snapshot,
            plan_graphs,
            execution_overlays,
        );
        let continuity_runtime = CoordinationRuntimeState::from_snapshot(
            native_plans.apply_task_execution_authored_fields_to_coordination_snapshot(snapshot),
        );
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") = native_plans;
        *self
            .continuity_runtime
            .write()
            .expect("continuity runtime lock poisoned") = continuity_runtime;
    }

    pub fn refresh_plan_runtime_from_coordination(&self) {
        let snapshot = self.continuity_snapshot();
        *self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned") =
            NativePlanRuntimeState::from_coordination_snapshot(&snapshot);
        *self
            .continuity_runtime
            .write()
            .expect("continuity runtime lock poisoned") =
            CoordinationRuntimeState::from_snapshot(snapshot);
    }

    fn mutate_native_plan_runtime<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut NativePlanRuntimeState) -> Result<T>,
    {
        let snapshot = self.continuity_snapshot();
        let mut runtime = self
            .plan_runtime
            .write()
            .expect("plan runtime lock poisoned");
        let result = mutate(&mut runtime)?;
        let snapshot = runtime.apply_to_coordination_snapshot(snapshot);
        self.replace_continuity_snapshot(snapshot);
        Ok(result)
    }

    fn apply_coordination_snapshot_with_native_runtime<T, F>(
        &self,
        snapshot: CoordinationSnapshot,
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
        let snapshot = runtime.apply_to_coordination_snapshot(snapshot);
        self.replace_continuity_snapshot(snapshot);
        Ok(result)
    }

    fn replace_continuity_snapshot(&self, snapshot: CoordinationSnapshot) {
        self.continuity_runtime
            .write()
            .expect("continuity runtime lock poisoned")
            .replace_from_snapshot(snapshot);
    }

    fn persist_coordination_snapshot(&self, snapshot: CoordinationSnapshot) -> Result<()> {
        let snapshot = self
            .plan_runtime
            .read()
            .expect("plan runtime lock poisoned")
            .apply_to_coordination_snapshot(snapshot);
        self.replace_continuity_snapshot(snapshot);
        Ok(())
    }

    fn mutate_validated_coordination_snapshot<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut CoordinationRuntimeState) -> Result<T>,
    {
        let (result, snapshot) = {
            let mut runtime = self
                .continuity_runtime
                .write()
                .expect("continuity runtime lock poisoned");
            let result = mutate(&mut runtime);
            let snapshot = runtime.snapshot();
            (result, snapshot)
        };
        self.persist_coordination_snapshot(snapshot)?;
        result
    }

    fn mutate_live_coordination_runtime<T, F>(
        &self,
        mutate: F,
    ) -> (CoordinationSnapshot, CoordinationSnapshot, Result<T>)
    where
        F: FnOnce(&mut CoordinationRuntimeState) -> Result<T>,
    {
        let mut runtime = self
            .continuity_runtime
            .write()
            .expect("continuity runtime lock poisoned");
        let before_snapshot = runtime.snapshot();
        let result = mutate(&mut runtime);
        let after_snapshot = runtime.snapshot();
        (before_snapshot, after_snapshot, result)
    }

    pub fn create_native_task(
        &self,
        meta: EventMeta,
        mut input: TaskCreateInput,
    ) -> Result<CoordinationTask> {
        if let Some(context) = self.coordination_context() {
            if input.session.is_some() {
                input.worktree_id = Some(context.worktree_id);
                input.branch_ref = context.branch_ref;
            }
        }
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| runtime.create_task(meta, input));
        match result {
            Ok((_, task)) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.create_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn update_native_task(
        &self,
        meta: EventMeta,
        mut input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: u64,
    ) -> Result<CoordinationTask> {
        if let Some(context) = self.coordination_context() {
            if matches!(input.session, Some(Some(_))) {
                input.worktree_id = Some(Some(context.worktree_id));
                input.branch_ref = Some(context.branch_ref);
            } else if matches!(input.session, Some(None)) {
                input.worktree_id = Some(None);
                input.branch_ref = Some(None);
            }
        }
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                runtime.update_task(meta, input, current_revision, now)
            });
        match result {
            Ok(task) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
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
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                runtime.handoff(meta, input, current_revision)
            });
        match result {
            Ok(task) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn accept_native_handoff(
        &self,
        meta: EventMeta,
        mut input: HandoffAcceptInput,
    ) -> Result<CoordinationTask> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| runtime.accept_handoff(meta, input));
        match result {
            Ok(task) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn resume_native_task(
        &self,
        meta: EventMeta,
        mut input: TaskResumeInput,
    ) -> Result<CoordinationTask> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| runtime.resume_task(meta, input));
        match result {
            Ok(task) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn reclaim_native_task(
        &self,
        meta: EventMeta,
        mut input: TaskReclaimInput,
    ) -> Result<CoordinationTask> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| runtime.reclaim_task(meta, input));
        match result {
            Ok(task) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_from_coordination(&task)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn acquire_native_claim(
        &self,
        meta: EventMeta,
        session_id: SessionId,
        mut input: prism_coordination::ClaimAcquireInput,
    ) -> Result<(
        Option<ClaimId>,
        Vec<CoordinationConflict>,
        Option<WorkClaim>,
    )> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
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
        mut input: ArtifactProposeInput,
    ) -> Result<(ArtifactId, Artifact)> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        self.mutate_validated_coordination_snapshot(|store| store.propose_artifact(meta, input))
    }

    pub fn supersede_native_artifact(
        &self,
        meta: EventMeta,
        input: ArtifactSupersedeInput,
    ) -> Result<Artifact> {
        self.mutate_validated_coordination_snapshot(|store| store.supersede_artifact(meta, input))
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
        kind: PlanNodeKind,
        title: String,
        summary: Option<String>,
        status: Option<PlanNodeStatus>,
        assignee: Option<AgentId>,
        is_abstract: bool,
        bindings: prism_ir::PlanBinding,
        depends_on: Vec<String>,
        acceptance: Vec<prism_ir::PlanAcceptanceCriterion>,
        validation_refs: Vec<prism_ir::ValidationRef>,
        base_revision: WorkspaceRevision,
        priority: Option<u8>,
        tags: Vec<String>,
    ) -> Result<PlanNodeId> {
        self.validate_native_plan_binding(&bindings)?;
        self.mutate_native_plan_runtime(|runtime| {
            runtime.create_node(
                plan_id,
                kind,
                title,
                summary,
                status,
                assignee,
                is_abstract,
                bindings,
                depends_on,
                acceptance,
                validation_refs,
                base_revision,
                priority,
                tags,
            )
        })
    }

    pub fn create_native_plan(
        &self,
        meta: EventMeta,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<PlanId> {
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                runtime.create_plan(
                    meta,
                    prism_coordination::PlanCreateInput {
                        goal,
                        status,
                        policy,
                    },
                )
            });
        match result {
            Ok((plan_id, plan)) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.create_plan_from_coordination(&plan)?;
                    Ok(plan_id)
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn update_native_plan(
        &self,
        meta: EventMeta,
        plan_id: &PlanId,
        status: Option<prism_ir::PlanStatus>,
        goal: Option<String>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<()> {
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                runtime.update_plan(
                    meta,
                    prism_coordination::PlanUpdateInput {
                        plan_id: plan_id.clone(),
                        goal,
                        status,
                        policy,
                    },
                )
            });
        match result {
            Ok(plan) => self
                .apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_plan_from_coordination(&plan)
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone())),
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn update_native_plan_node(
        &self,
        node_id: &PlanNodeId,
        kind: Option<PlanNodeKind>,
        status: Option<PlanNodeStatus>,
        assignee: Option<Option<AgentId>>,
        is_abstract: Option<bool>,
        title: Option<String>,
        summary: Option<String>,
        clear_summary: bool,
        bindings: Option<prism_ir::PlanBinding>,
        depends_on: Option<Vec<String>>,
        acceptance: Option<Vec<prism_ir::PlanAcceptanceCriterion>>,
        validation_refs: Option<Vec<prism_ir::ValidationRef>>,
        base_revision: Option<WorkspaceRevision>,
        priority: Option<u8>,
        clear_priority: bool,
        tags: Option<Vec<String>>,
    ) -> Result<PlanId> {
        if self
            .coordination_task(&prism_ir::CoordinationTaskId::new(node_id.0.clone()))
            .is_some()
        {
            return Err(anyhow!(
                "plan node `{}` is task-backed; update the coordination task instead",
                node_id.0
            ));
        }
        if let Some(bindings) = bindings.as_ref() {
            self.validate_native_plan_binding(bindings)?;
        }
        if matches!(status, Some(PlanNodeStatus::Completed)) {
            let mut preview = self
                .plan_runtime
                .read()
                .expect("plan runtime lock poisoned")
                .clone();
            let plan_id = preview.update_node(
                node_id,
                kind,
                status,
                assignee.clone(),
                is_abstract,
                title.clone(),
                summary.clone(),
                clear_summary,
                bindings.clone(),
                depends_on.clone(),
                acceptance.clone(),
                validation_refs.clone(),
                base_revision.clone(),
                priority,
                clear_priority,
                tags.clone(),
            )?;
            self.validate_native_plan_node_completion_preview(&preview, &plan_id, node_id)?;
        }
        self.mutate_native_plan_runtime(|runtime| {
            runtime.update_node(
                node_id,
                kind,
                status,
                assignee,
                is_abstract,
                title,
                summary,
                clear_summary,
                bindings,
                depends_on,
                acceptance,
                validation_refs,
                base_revision,
                priority,
                clear_priority,
                tags,
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

    pub fn projection_lineage_counts(&self) -> (usize, usize) {
        let projections = self.projections.read().expect("projection lock poisoned");
        (
            projections.co_change_lineage_count(),
            projections.validation_lineage_count(),
        )
    }

    pub fn refresh_projections(&self) {
        let curated = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .curated_concepts()
            .to_vec();
        let relations = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .concept_relations()
            .to_vec();
        let contracts = self
            .projections
            .read()
            .expect("projection lock poisoned")
            .curated_contracts()
            .to_vec();
        let mut next = ProjectionIndex::derive_with_knowledge(
            &self.history.snapshot(),
            &self.outcomes.snapshot(),
            curated,
            relations,
        );
        next.replace_curated_contracts(contracts);
        *self.projections.write().expect("projection lock poisoned") = next;
    }

    pub fn replace_curated_concepts(&self, concepts: Vec<ConceptPacket>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_curated_concepts(concepts);
    }

    pub fn curated_concepts_snapshot(&self) -> Vec<ConceptPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .curated_concepts()
            .to_vec()
    }

    pub fn upsert_curated_concept(&self, concept: ConceptPacket) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .upsert_curated_concept(concept);
    }

    pub fn replace_curated_contracts(&self, contracts: Vec<ContractPacket>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_curated_contracts(contracts);
    }

    pub fn upsert_curated_contract(&self, contract: ContractPacket) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .upsert_curated_contract(contract);
    }

    pub fn replace_concept_relations(&self, relations: Vec<ConceptRelation>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_concept_relations(relations);
    }

    pub fn concept_relations_snapshot(&self) -> Vec<ConceptRelation> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .concept_relations()
            .to_vec()
    }

    pub fn upsert_concept_relation(&self, relation: ConceptRelation) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .upsert_concept_relation(relation);
    }

    pub fn remove_concept_relation(
        &self,
        source_handle: &str,
        target_handle: &str,
        kind: ConceptRelationKind,
    ) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .remove_concept_relation(source_handle, target_handle, kind);
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

    pub fn concept_relations_for_handle(&self, handle: &str) -> Vec<ConceptRelation> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .concept_relations_for_handle(handle)
    }

    pub fn contracts(&self, query: &str, limit: usize) -> Vec<ContractPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .contracts(query, limit)
    }

    pub fn curated_contracts(&self) -> Vec<ContractPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .curated_contracts()
            .to_vec()
    }

    pub fn resolve_contracts(&self, query: &str, limit: usize) -> Vec<ContractResolution> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .resolve_contracts(query, limit)
    }

    pub fn contract(&self, query: &str) -> Option<ContractPacket> {
        self.contracts(query, 1).into_iter().next()
    }

    pub fn resolve_contract(&self, query: &str) -> Option<ContractResolution> {
        self.resolve_contracts(query, 1).into_iter().next()
    }

    pub fn contract_by_handle(&self, handle: &str) -> Option<ContractPacket> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .contract_by_handle(handle)
    }

    pub fn contract_health(&self, query: &str) -> Option<ContractHealth> {
        let handle = self.resolve_contract(query)?.packet.handle;
        self.contract_health_by_handle(&handle)
    }

    pub fn contract_health_by_handle(&self, handle: &str) -> Option<ContractHealth> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .contract_health(handle)
    }

    pub fn concept_health(&self, query: &str) -> Option<ConceptHealth> {
        let handle = self.resolve_concept(query)?.packet.handle;
        self.concept_health_by_handle(&handle)
    }

    pub fn concept_health_by_handle(&self, handle: &str) -> Option<ConceptHealth> {
        self.projections
            .read()
            .expect("projection lock poisoned")
            .concept_health(handle)
    }
}

fn merge_lineage_events(hot: Vec<LineageEvent>, cold: Vec<LineageEvent>) -> Vec<LineageEvent> {
    let mut events = hot
        .into_iter()
        .chain(cold)
        .fold(BTreeMap::<String, LineageEvent>::new(), |mut acc, event| {
            acc.entry(event.meta.id.0.to_string()).or_insert(event);
            acc
        })
        .into_values()
        .collect::<Vec<_>>();
    events.sort_by(|left, right| {
        left.meta
            .ts
            .cmp(&right.meta.ts)
            .then_with(|| left.meta.id.0.cmp(&right.meta.id.0))
    });
    events
}

fn merge_history_snapshots(hot: HistorySnapshot, cold: HistorySnapshot) -> HistorySnapshot {
    let mut node_to_lineage = cold
        .node_to_lineage
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    for (node, lineage) in hot.node_to_lineage {
        node_to_lineage.insert(node, lineage);
    }

    let mut tombstones = cold
        .tombstones
        .into_iter()
        .map(|tombstone| (tombstone.lineage.clone(), tombstone))
        .collect::<BTreeMap<_, _>>();
    for tombstone in hot.tombstones {
        tombstones.insert(tombstone.lineage.clone(), tombstone);
    }

    HistorySnapshot {
        node_to_lineage: {
            let mut merged = node_to_lineage.into_iter().collect::<Vec<_>>();
            merged.sort_by(|left, right| {
                anchor_sort_key(
                    &AnchorRef::Node(left.0.clone()),
                    &AnchorRef::Node(right.0.clone()),
                )
            });
            merged
        },
        events: merge_lineage_events(hot.events, cold.events),
        tombstones: tombstones.into_values().collect(),
        next_lineage: hot.next_lineage.max(cold.next_lineage),
        next_event: hot.next_event.max(cold.next_event),
    }
}
