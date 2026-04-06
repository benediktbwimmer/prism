mod ad_hoc_projections;
mod common;
mod contracts;
mod coordination;
mod impact;
mod intent;
mod materialized_runtime;
mod outcomes;
mod plan_activity;
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

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{anyhow, Result};
use prism_coordination::{
    Artifact, ArtifactProposeInput, ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput,
    CoordinationConflict, CoordinationRuntimeState, CoordinationSnapshot, CoordinationTask,
    HandoffAcceptInput, HandoffInput, PlanScheduling, TaskCreateInput, TaskReclaimInput,
    TaskResumeInput, TaskUpdateInput, WorkClaim,
};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ClaimId, CoordinationTaskId, EdgeKind, EventId, EventMeta,
    LineageEvent, LineageId, NodeId, NodeKind, ObservedChangeSet, PlanEdgeKind,
    PlanExecutionOverlay, PlanGraph, PlanId, PlanNodeId, PlanNodeKind, PlanNodeStatus, PlanStatus,
    ReviewId, SessionId, TaskId, ValidationRef, WorkspaceRevision,
};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot};
use prism_memory::{OutcomeRecallQuery, TaskReplay};
pub use prism_projections::ConceptResolution;
use prism_projections::{IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::{CoordinationPersistContext, Graph};
use tracing::info;

use crate::common::{anchor_sort_key, dedupe_node_ids, sort_node_ids};
use crate::materialized_runtime::MaterializedCoordinationRuntime;
use crate::plan_bindings::validate_authored_plan_binding;
use crate::plan_discovery::PlanDiscoveryCache;
use crate::plan_runtime::NativePlanRuntimeState;

pub use crate::source::{
    source_excerpt_for_line_range, source_excerpt_for_span, source_location_for_span,
    source_slice_around_line, EditSlice, EditSliceOptions, SourceExcerpt, SourceExcerptOptions,
    SourceLocation,
};
pub use crate::symbol::{Relations, Symbol};
pub use crate::types::{
    canonical_concept_handle, canonical_contract_handle, AdHocPlanProjection,
    AdHocPlanProjectionDiff, AdHocPlanProjectionSummary, ArtifactRisk, ChangeImpact, CoChange,
    ConceptDecodeLens, ConceptEvent, ConceptEventAction, ConceptEventPatch, ConceptHealth,
    ConceptHealthSignals, ConceptHealthStatus, ConceptPacket, ConceptProvenance,
    ConceptPublication, ConceptPublicationStatus, ConceptRelation, ConceptRelationEvent,
    ConceptRelationEventAction, ConceptRelationKind, ConceptScope, ContractCompatibility,
    ContractEvent, ContractEventAction, ContractEventPatch, ContractGuarantee,
    ContractGuaranteeStrength, ContractHealth, ContractHealthSignals, ContractHealthStatus,
    ContractKind, ContractPacket, ContractProvenance, ContractPublication,
    ContractPublicationStatus, ContractResolution, ContractScope, ContractStability,
    ContractStatus, ContractTarget, ContractValidation, DriftCandidate, PlanActivity,
    PlanListEntry, PlanNodeRecommendation, PlanNodeStatusCounts, PlanSummary, QueryLimits,
    TaskIntent, TaskRisk, TaskValidationRecipe, ValidationCheck, ValidationRecipe,
};

pub struct Prism {
    graph: Arc<Graph>,
    history: Arc<HistoryStore>,
    outcomes: Arc<OutcomeMemory>,
    history_backend: RwLock<Option<Arc<dyn HistoryReadBackend>>>,
    outcome_backend: RwLock<Option<Arc<dyn OutcomeReadBackend>>>,
    workspace_revision: RwLock<WorkspaceRevision>,
    materialized_runtime: RwLock<MaterializedCoordinationRuntime>,
    plan_discovery_cache: RwLock<Option<PlanDiscoveryCache>>,
    local_assisted_leases: RwLock<LocalAssistedLeaseRuntime>,
    coordination_context: RwLock<Option<CoordinationPersistContext>>,
    projections: RwLock<ProjectionIndex>,
    intent: RwLock<IntentIndex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalAssistedLeaseState {
    observed_at: u64,
    local_until: u64,
}

#[derive(Debug, Default)]
struct LocalAssistedLeaseRuntime {
    tasks: BTreeMap<String, LocalAssistedLeaseState>,
    claims: BTreeMap<String, LocalAssistedLeaseState>,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapInput {
    pub title: String,
    pub goal: String,
    pub status: Option<PlanStatus>,
    pub policy: Option<prism_coordination::CoordinationPolicy>,
    pub scheduling: Option<PlanScheduling>,
    pub tasks: Vec<NativePlanBootstrapTaskInput>,
    pub nodes: Vec<NativePlanBootstrapNodeInput>,
    pub edges: Vec<NativePlanBootstrapEdgeInput>,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapTaskInput {
    pub client_id: String,
    pub title: String,
    pub status: Option<prism_ir::CoordinationTaskStatus>,
    pub assignee: Option<AgentId>,
    pub session: Option<SessionId>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<String>,
    pub coordination_depends_on: Vec<String>,
    pub integrated_depends_on: Vec<String>,
    pub acceptance: Vec<prism_coordination::AcceptanceCriterion>,
    pub base_revision: WorkspaceRevision,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapNodeInput {
    pub client_id: String,
    pub kind: PlanNodeKind,
    pub title: String,
    pub summary: Option<String>,
    pub status: Option<PlanNodeStatus>,
    pub assignee: Option<AgentId>,
    pub is_abstract: bool,
    pub bindings: prism_ir::PlanBinding,
    pub depends_on: Vec<String>,
    pub acceptance: Vec<prism_ir::PlanAcceptanceCriterion>,
    pub validation_refs: Vec<ValidationRef>,
    pub base_revision: WorkspaceRevision,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapEdgeInput {
    pub from_client_id: String,
    pub to_client_id: String,
    pub kind: PlanEdgeKind,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapEdgeResult {
    pub from_node_id: PlanNodeId,
    pub to_node_id: PlanNodeId,
    pub kind: PlanEdgeKind,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapResult {
    pub plan_id: PlanId,
    pub task_ids_by_client_id: BTreeMap<String, CoordinationTaskId>,
    pub node_ids_by_client_id: BTreeMap<String, PlanNodeId>,
    pub edges: Vec<NativePlanBootstrapEdgeResult>,
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
        Self::with_shared_history_outcomes_coordination_projections_and_native_plans(
            Arc::new(graph),
            Arc::new(history),
            Arc::new(outcomes),
            projections,
            MaterializedCoordinationRuntime::from_snapshot(coordination),
            None,
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
        Self::with_shared_history_outcomes_coordination_projections_and_plan_graphs_and_intent(
            graph,
            history,
            outcomes,
            coordination,
            projections,
            plan_graphs,
            execution_overlays,
            None,
        )
    }

    pub fn with_shared_history_outcomes_coordination_projections_and_plan_graphs_and_intent(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
        intent_override: Option<IntentIndex>,
    ) -> Self {
        Self::with_shared_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            projections,
            MaterializedCoordinationRuntime::from_snapshot_with_graphs_and_overlays(
                coordination,
                plan_graphs,
                execution_overlays,
            ),
            intent_override,
        )
    }

    fn with_shared_history_outcomes_coordination_projections_and_native_plans(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        mut projections: ProjectionIndex,
        materialized_runtime: MaterializedCoordinationRuntime,
        intent_override: Option<IntentIndex>,
    ) -> Self {
        projections.reseed_from_history(&history.snapshot());
        let started = Instant::now();
        let node_count = graph.node_count();
        let edge_count = graph.edge_count();
        let file_count = graph.file_count();
        let intent_started = Instant::now();
        let (intent, reused_intent) = intent_override.map_or_else(
            || {
                (
                    IntentIndex::derive(
                        graph.all_nodes().collect::<Vec<_>>(),
                        graph.edges.iter().collect::<Vec<_>>(),
                    ),
                    false,
                )
            },
            |intent| (intent, true),
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
            reused_intent,
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
            materialized_runtime: RwLock::new(materialized_runtime),
            plan_discovery_cache: RwLock::new(None),
            local_assisted_leases: RwLock::new(LocalAssistedLeaseRuntime::default()),
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
        self.invalidate_plan_discovery_cache();
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
        self.invalidate_plan_discovery_cache();
    }

    pub fn intent_snapshot(&self) -> IntentIndex {
        self.intent.read().expect("intent lock poisoned").clone()
    }

    pub fn updated_intent_for_observed_changes(
        &self,
        graph: &Graph,
        observed_changes: &[ObservedChangeSet],
    ) -> IntentIndex {
        let mut intent = self.intent_snapshot();
        for spec in affected_intent_specs(observed_changes) {
            intent.remove_spec_projection(&spec);
            let Some(node) = graph.node(&spec) else {
                continue;
            };
            if !is_intent_source_node_kind(node.kind) {
                continue;
            }
            let mut implementations = Vec::new();
            let mut validations = Vec::new();
            let mut related = Vec::new();
            for edge in graph.edges_from(&spec, None) {
                match edge.kind {
                    EdgeKind::Specifies => implementations.push(edge.target.clone()),
                    EdgeKind::Validates => validations.push(edge.target.clone()),
                    EdgeKind::RelatedTo => related.push(edge.target.clone()),
                    _ => {}
                }
            }
            intent.replace_spec_projection(spec, implementations, validations, related);
        }
        intent
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

    pub(crate) fn plan_runtime_state(&self) -> NativePlanRuntimeState {
        self.materialized_runtime
            .read()
            .expect("materialized runtime lock poisoned")
            .plan_runtime()
            .clone()
    }

    pub(crate) fn with_coordination_runtime<T>(
        &self,
        read: impl FnOnce(&CoordinationRuntimeState) -> T,
    ) -> T {
        let runtime = self
            .materialized_runtime
            .read()
            .expect("materialized runtime lock poisoned");
        read(runtime.continuity_runtime())
    }

    pub(crate) fn with_coordination_runtime_mut<T>(
        &self,
        mutate: impl FnOnce(&mut CoordinationRuntimeState) -> T,
    ) -> T {
        let mut runtime = self
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        mutate(runtime.continuity_runtime_mut())
    }

    fn continuity_snapshot(&self) -> CoordinationSnapshot {
        self.materialized_runtime
            .read()
            .expect("materialized runtime lock poisoned")
            .snapshot()
    }

    pub fn coordination_snapshot(&self) -> CoordinationSnapshot {
        self.continuity_snapshot()
    }

    pub fn replace_coordination_snapshot(&self, snapshot: CoordinationSnapshot) {
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .replace_from_snapshot(snapshot.clone());
        self.prune_local_assisted_leases(&snapshot);
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_coordination_snapshot_and_plan_graphs(
        &self,
        snapshot: CoordinationSnapshot,
        plan_graphs: Vec<PlanGraph>,
        execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    ) {
        let prune_snapshot = snapshot.clone();
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .replace_from_snapshot_with_graphs_and_overlays(
                snapshot,
                plan_graphs,
                execution_overlays,
            );
        self.prune_local_assisted_leases(&prune_snapshot);
        self.invalidate_plan_discovery_cache();
    }

    pub fn record_local_assisted_task_lease(
        &self,
        task_id: &CoordinationTaskId,
        observed_at: u64,
        local_until: u64,
    ) -> bool {
        let mut assisted = self
            .local_assisted_leases
            .write()
            .expect("local assisted lease lock poisoned");
        let next = LocalAssistedLeaseState {
            observed_at,
            local_until,
        };
        let key = task_id.0.to_string();
        let changed = assisted.tasks.get(&key) != Some(&next);
        assisted.tasks.insert(key, next);
        changed
    }

    pub fn record_local_assisted_claim_lease(
        &self,
        claim_id: &ClaimId,
        observed_at: u64,
        local_until: u64,
    ) -> bool {
        let mut assisted = self
            .local_assisted_leases
            .write()
            .expect("local assisted lease lock poisoned");
        let next = LocalAssistedLeaseState {
            observed_at,
            local_until,
        };
        let key = claim_id.0.to_string();
        let changed = assisted.claims.get(&key) != Some(&next);
        assisted.claims.insert(key, next);
        changed
    }

    pub fn task_has_active_local_assisted_lease(&self, task: &CoordinationTask, now: u64) -> bool {
        let key = task.id.0.to_string();
        let mut assisted = self
            .local_assisted_leases
            .write()
            .expect("local assisted lease lock poisoned");
        let Some(state) = assisted.tasks.get(&key).copied() else {
            return false;
        };
        if now > state.local_until
            || task
                .lease_refreshed_at
                .is_some_and(|refreshed_at| refreshed_at >= state.observed_at)
        {
            assisted.tasks.remove(&key);
            return false;
        }
        true
    }

    pub fn claim_has_active_local_assisted_lease(&self, claim: &WorkClaim, now: u64) -> bool {
        let key = claim.id.0.to_string();
        let mut assisted = self
            .local_assisted_leases
            .write()
            .expect("local assisted lease lock poisoned");
        let Some(state) = assisted.claims.get(&key).copied() else {
            return false;
        };
        if now > state.local_until
            || claim
                .refreshed_at
                .is_some_and(|refreshed_at| refreshed_at >= state.observed_at)
        {
            assisted.claims.remove(&key);
            return false;
        }
        true
    }

    pub fn refresh_plan_runtime_from_coordination(&self) {
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .refresh_plan_runtime_from_coordination();
        self.invalidate_plan_discovery_cache();
    }

    fn prune_local_assisted_leases(&self, snapshot: &CoordinationSnapshot) {
        let task_ids = snapshot
            .tasks
            .iter()
            .map(|task| task.id.0.to_string())
            .collect::<BTreeSet<_>>();
        let task_refreshed = snapshot
            .tasks
            .iter()
            .map(|task| {
                (
                    task.id.0.to_string(),
                    task.lease_refreshed_at.unwrap_or_default(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let claim_ids = snapshot
            .claims
            .iter()
            .map(|claim| claim.id.0.to_string())
            .collect::<BTreeSet<_>>();
        let claim_refreshed = snapshot
            .claims
            .iter()
            .map(|claim| {
                (
                    claim.id.0.to_string(),
                    claim.refreshed_at.unwrap_or_default(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut assisted = self
            .local_assisted_leases
            .write()
            .expect("local assisted lease lock poisoned");
        assisted.tasks.retain(|task_id, state| {
            task_ids.contains(task_id)
                && task_refreshed.get(task_id).copied().unwrap_or_default() < state.observed_at
        });
        assisted.claims.retain(|claim_id, state| {
            claim_ids.contains(claim_id)
                && claim_refreshed.get(claim_id).copied().unwrap_or_default() < state.observed_at
        });
    }

    fn mutate_native_plan_runtime<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut NativePlanRuntimeState) -> Result<T>,
    {
        let result = {
            let mut runtime = self
                .materialized_runtime
                .write()
                .expect("materialized runtime lock poisoned");
            let result = mutate(runtime.plan_runtime_mut())?;
            runtime.apply_plan_runtime_to_current_snapshot();
            result
        };
        self.invalidate_plan_discovery_cache();
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
        let result = {
            let mut runtime = self
                .materialized_runtime
                .write()
                .expect("materialized runtime lock poisoned");
            let result = mutate(runtime.plan_runtime_mut())?;
            let snapshot = runtime
                .plan_runtime()
                .apply_to_coordination_snapshot(snapshot);
            runtime.replace_continuity_snapshot(snapshot);
            result
        };
        self.invalidate_plan_discovery_cache();
        Ok(result)
    }

    fn replace_continuity_snapshot(&self, snapshot: CoordinationSnapshot) {
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .replace_continuity_snapshot(snapshot);
        self.invalidate_plan_discovery_cache();
    }

    fn persist_coordination_snapshot(&self, snapshot: CoordinationSnapshot) -> Result<()> {
        {
            let mut runtime = self
                .materialized_runtime
                .write()
                .expect("materialized runtime lock poisoned");
            runtime.persist_coordination_snapshot(snapshot.clone())?;
        }
        self.prune_local_assisted_leases(&snapshot);
        self.invalidate_plan_discovery_cache();
        Ok(())
    }

    fn mutate_validated_coordination_snapshot<T, F>(&self, mutate: F) -> Result<T>
    where
        F: FnOnce(&mut CoordinationRuntimeState) -> Result<T>,
    {
        let (result, snapshot) = {
            let mut runtime = self
                .materialized_runtime
                .write()
                .expect("materialized runtime lock poisoned");
            let result = mutate(runtime.continuity_runtime_mut());
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
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        let before_snapshot = runtime.snapshot();
        let result = mutate(runtime.continuity_runtime_mut());
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
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn update_native_task_authoritative_only(
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
                runtime.update_task_authoritative_only(meta, input, current_revision, now)
            });
        match result {
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
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
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
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
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
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
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
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
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    pub fn heartbeat_native_task(
        &self,
        meta: EventMeta,
        task_id: &CoordinationTaskId,
        renewal_provenance: &str,
    ) -> Result<CoordinationTask> {
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                runtime.heartbeat_task(meta, task_id, renewal_provenance)
            });
        match result {
            Ok(task) => {
                let plan = snapshot
                    .plans
                    .iter()
                    .find(|plan| plan.id == task.plan)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plan `{}`", task.plan.0))?;
                self.apply_coordination_snapshot_with_native_runtime(snapshot, |plan_runtime| {
                    plan_runtime.update_task_and_plan_from_coordination(&task, &plan)?;
                    Ok(task.clone())
                })
                .inspect_err(|_| self.replace_continuity_snapshot(before_snapshot.clone()))
            }
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
        renewal_provenance: &str,
    ) -> Result<WorkClaim> {
        self.mutate_validated_coordination_snapshot(|store| {
            store.renew_claim(meta, session_id, claim_id, ttl_seconds, renewal_provenance)
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
        title: String,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<PlanId> {
        self.create_native_plan_with_scheduling(meta, title, goal, status, policy, None)
    }

    pub fn create_native_plan_with_scheduling(
        &self,
        meta: EventMeta,
        title: String,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
        scheduling: Option<prism_coordination::PlanScheduling>,
    ) -> Result<PlanId> {
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                let (plan_id, _plan) = runtime.create_plan(
                    meta.clone(),
                    prism_coordination::PlanCreateInput {
                        title,
                        goal,
                        status,
                        policy,
                    },
                )?;
                if let Some(scheduling) = scheduling {
                    runtime.set_plan_scheduling(
                        derived_coordination_meta(&meta, "plan-scheduling"),
                        plan_id.clone(),
                        scheduling,
                    )?;
                }
                let plan = runtime
                    .plan(&plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;
                Ok((plan_id, plan))
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

    pub fn bootstrap_native_plan(
        &self,
        meta: EventMeta,
        input: NativePlanBootstrapInput,
    ) -> Result<NativePlanBootstrapResult> {
        struct CreatedTaskSpec {
            client_id: String,
            task_id: CoordinationTaskId,
            base_revision: WorkspaceRevision,
            depends_on: Vec<String>,
            coordination_depends_on: Vec<String>,
            integrated_depends_on: Vec<String>,
        }

        struct CreatedNodeSpec {
            client_id: String,
            node_id: PlanNodeId,
            depends_on: Vec<String>,
        }

        let NativePlanBootstrapInput {
            title,
            goal,
            status,
            policy,
            scheduling,
            tasks,
            nodes,
            edges,
        } = input;

        let mut seen_client_ids = BTreeSet::new();
        for client_id in tasks
            .iter()
            .map(|task| task.client_id.as_str())
            .chain(nodes.iter().map(|node| node.client_id.as_str()))
        {
            ensure_unique_bootstrap_client_id(&mut seen_client_ids, client_id)?;
        }

        for node in &nodes {
            self.validate_native_plan_binding(&node.bindings)?;
        }

        let before_snapshot = self.continuity_snapshot();
        let mut coordination_runtime = CoordinationRuntimeState::from_snapshot(before_snapshot);
        let coordination_context = self.coordination_context();
        let now = meta.ts;

        let (plan_id, _) = coordination_runtime.create_plan(
            meta.clone(),
            prism_coordination::PlanCreateInput {
                title,
                goal,
                status,
                policy,
            },
        )?;
        if let Some(scheduling) = scheduling {
            coordination_runtime.set_plan_scheduling(
                derived_coordination_meta(&meta, "plan-scheduling"),
                plan_id.clone(),
                scheduling,
            )?;
        }

        let mut created_tasks = Vec::with_capacity(tasks.len());
        let mut task_ids_by_client_id = BTreeMap::new();
        for task in tasks {
            let worktree_id = coordination_context
                .as_ref()
                .and_then(|context| task.session.as_ref().map(|_| context.worktree_id.clone()));
            let branch_ref = coordination_context
                .as_ref()
                .and_then(|context| task.session.as_ref().and(context.branch_ref.clone()));
            let (task_id, _) = coordination_runtime.create_task(
                derived_coordination_meta(
                    &meta,
                    &format!("bootstrap-task-create:{}", task.client_id),
                ),
                TaskCreateInput {
                    plan_id: plan_id.clone(),
                    title: task.title,
                    status: task.status,
                    assignee: task.assignee,
                    session: task.session,
                    worktree_id,
                    branch_ref,
                    anchors: task.anchors,
                    depends_on: Vec::new(),
                    coordination_depends_on: Vec::new(),
                    integrated_depends_on: Vec::new(),
                    acceptance: task.acceptance,
                    base_revision: task.base_revision.clone(),
                },
            )?;
            task_ids_by_client_id.insert(task.client_id.clone(), task_id.clone());
            created_tasks.push(CreatedTaskSpec {
                client_id: task.client_id,
                task_id,
                base_revision: task.base_revision,
                depends_on: task.depends_on,
                coordination_depends_on: task.coordination_depends_on,
                integrated_depends_on: task.integrated_depends_on,
            });
        }

        for task in &created_tasks {
            if task.depends_on.is_empty()
                && task.coordination_depends_on.is_empty()
                && task.integrated_depends_on.is_empty()
            {
                continue;
            }
            coordination_runtime.update_task(
                derived_coordination_meta(
                    &meta,
                    &format!("bootstrap-task-deps:{}", task.client_id),
                ),
                TaskUpdateInput {
                    task_id: task.task_id.clone(),
                    kind: None,
                    status: None,
                    published_task_status: None,
                    git_execution: None,
                    assignee: None,
                    session: None,
                    worktree_id: None,
                    branch_ref: None,
                    title: None,
                    summary: None,
                    anchors: None,
                    bindings: None,
                    depends_on: Some(resolve_bootstrap_task_dependencies(
                        &task_ids_by_client_id,
                        &task.client_id,
                        "dependsOn",
                        &task.depends_on,
                    )?),
                    coordination_depends_on: Some(resolve_bootstrap_task_dependencies(
                        &task_ids_by_client_id,
                        &task.client_id,
                        "coordinationDependsOn",
                        &task.coordination_depends_on,
                    )?),
                    integrated_depends_on: Some(resolve_bootstrap_task_dependencies(
                        &task_ids_by_client_id,
                        &task.client_id,
                        "integratedDependsOn",
                        &task.integrated_depends_on,
                    )?),
                    acceptance: None,
                    validation_refs: None,
                    is_abstract: None,
                    base_revision: Some(task.base_revision.clone()),
                    priority: None,
                    tags: None,
                    completion_context: None,
                },
                task.base_revision.clone(),
                now,
            )?;
        }

        let after_coordination_snapshot = coordination_runtime.snapshot();
        let plan = coordination_runtime
            .plan(&plan_id)
            .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))?;

        let tasks_in_order = created_tasks
            .iter()
            .map(|task| {
                coordination_runtime
                    .task(&task.task_id)
                    .ok_or_else(|| anyhow!("unknown task `{}`", task.task_id.0))
                    .map(|state| (task.client_id.clone(), state))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut plan_runtime = self.plan_runtime_state();
        plan_runtime.create_plan_from_coordination(&plan)?;
        for (_, task) in &tasks_in_order {
            plan_runtime.create_task_from_coordination(task)?;
        }

        let mut node_ids_by_client_id = task_ids_by_client_id
            .iter()
            .map(|(client_id, task_id)| (client_id.clone(), PlanNodeId::new(task_id.0.clone())))
            .collect::<BTreeMap<_, _>>();

        let mut created_nodes = Vec::with_capacity(nodes.len());
        for node in nodes {
            let node_id = plan_runtime.create_node(
                &plan_id,
                node.kind,
                node.title,
                node.summary,
                node.status,
                node.assignee,
                node.is_abstract,
                node.bindings,
                Vec::new(),
                node.acceptance,
                node.validation_refs,
                node.base_revision,
                node.priority,
                node.tags,
            )?;
            node_ids_by_client_id.insert(node.client_id.clone(), node_id.clone());
            created_nodes.push(CreatedNodeSpec {
                client_id: node.client_id,
                node_id,
                depends_on: node.depends_on,
            });
        }

        for node in &created_nodes {
            if node.depends_on.is_empty() {
                continue;
            }
            plan_runtime.update_node(
                &node.node_id,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                Some(resolve_bootstrap_node_dependencies(
                    &node_ids_by_client_id,
                    &node.client_id,
                    "dependsOn",
                    &node.depends_on,
                )?),
                None,
                None,
                None,
                None,
                false,
                None,
            )?;
        }

        let mut created_edges = Vec::with_capacity(edges.len());
        for edge in edges {
            let from_node_id = resolve_bootstrap_node_reference(
                &node_ids_by_client_id,
                &edge.from_client_id,
                "fromClientId",
            )?;
            let to_node_id = resolve_bootstrap_node_reference(
                &node_ids_by_client_id,
                &edge.to_client_id,
                "toClientId",
            )?;
            plan_runtime.create_edge(&plan_id, &from_node_id, &to_node_id, edge.kind)?;
            created_edges.push(NativePlanBootstrapEdgeResult {
                from_node_id,
                to_node_id,
                kind: edge.kind,
            });
        }

        {
            let mut runtime = self
                .materialized_runtime
                .write()
                .expect("materialized runtime lock poisoned");
            let final_snapshot =
                plan_runtime.apply_to_coordination_snapshot(after_coordination_snapshot);
            *runtime.plan_runtime_mut() = plan_runtime;
            runtime.replace_continuity_snapshot(final_snapshot);
        }
        self.invalidate_plan_discovery_cache();

        Ok(NativePlanBootstrapResult {
            plan_id,
            task_ids_by_client_id,
            node_ids_by_client_id,
            edges: created_edges,
        })
    }

    pub fn update_native_plan(
        &self,
        meta: EventMeta,
        plan_id: &PlanId,
        title: Option<String>,
        status: Option<prism_ir::PlanStatus>,
        goal: Option<String>,
        policy: Option<prism_coordination::CoordinationPolicy>,
    ) -> Result<()> {
        self.update_native_plan_with_scheduling(meta, plan_id, title, status, goal, policy, None)
    }

    pub fn update_native_plan_with_scheduling(
        &self,
        meta: EventMeta,
        plan_id: &PlanId,
        title: Option<String>,
        status: Option<prism_ir::PlanStatus>,
        goal: Option<String>,
        policy: Option<prism_coordination::CoordinationPolicy>,
        scheduling: Option<prism_coordination::PlanScheduling>,
    ) -> Result<()> {
        let (before_snapshot, snapshot, result) =
            self.mutate_live_coordination_runtime(|runtime| {
                if title.is_some() || status.is_some() || goal.is_some() || policy.is_some() {
                    runtime.update_plan(
                        meta.clone(),
                        prism_coordination::PlanUpdateInput {
                            plan_id: plan_id.clone(),
                            title,
                            goal,
                            status,
                            policy,
                        },
                    )?;
                }
                if let Some(scheduling) = scheduling {
                    runtime.set_plan_scheduling(
                        derived_coordination_meta(&meta, "plan-scheduling"),
                        plan_id.clone(),
                        scheduling,
                    )?;
                }
                runtime
                    .plan(plan_id)
                    .ok_or_else(|| anyhow!("unknown plan `{}`", plan_id.0))
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
            let mut preview = self.plan_runtime_state();
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
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_curated_concepts(&self, concepts: Vec<ConceptPacket>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_curated_concepts(concepts);
        self.invalidate_plan_discovery_cache();
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
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_curated_contracts(&self, contracts: Vec<ContractPacket>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_curated_contracts(contracts);
        self.invalidate_plan_discovery_cache();
    }

    pub fn upsert_curated_contract(&self, contract: ContractPacket) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .upsert_curated_contract(contract);
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_concept_relations(&self, relations: Vec<ConceptRelation>) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .replace_concept_relations(relations);
        self.invalidate_plan_discovery_cache();
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
        self.invalidate_plan_discovery_cache();
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
        self.invalidate_plan_discovery_cache();
    }

    pub fn apply_outcome_event_to_projections(&self, event: &OutcomeEvent) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_outcome_event(event, |node| self.history.lineage_of(node));
        self.invalidate_plan_discovery_cache();
    }

    pub fn apply_lineage_events_to_projections(&self, events: &[LineageEvent]) {
        self.projections
            .write()
            .expect("projection lock poisoned")
            .apply_lineage_events(events);
        self.invalidate_plan_discovery_cache();
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

fn derived_coordination_meta(meta: &EventMeta, suffix: &str) -> EventMeta {
    let mut derived = meta.clone();
    derived.id = prism_ir::EventId::new(format!("{}:{suffix}", meta.id.0));
    derived
}

fn ensure_unique_bootstrap_client_id(
    seen_client_ids: &mut BTreeSet<String>,
    client_id: &str,
) -> Result<()> {
    if client_id.is_empty() {
        return Err(anyhow!("plan bootstrap client ids must be non-empty"));
    }
    if !seen_client_ids.insert(client_id.to_string()) {
        return Err(anyhow!(
            "plan bootstrap client id `{client_id}` is duplicated"
        ));
    }
    Ok(())
}

fn resolve_bootstrap_task_dependencies(
    task_ids_by_client_id: &BTreeMap<String, CoordinationTaskId>,
    owner_client_id: &str,
    field: &str,
    refs: &[String],
) -> Result<Vec<CoordinationTaskId>> {
    refs.iter()
        .map(|client_id| {
            task_ids_by_client_id
                .get(client_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "plan bootstrap task `{owner_client_id}` references unknown task client id `{client_id}` in `{field}`"
                    )
                })
        })
        .collect()
}

fn resolve_bootstrap_node_dependencies(
    node_ids_by_client_id: &BTreeMap<String, PlanNodeId>,
    owner_client_id: &str,
    field: &str,
    refs: &[String],
) -> Result<Vec<String>> {
    refs.iter()
        .map(|client_id| {
            node_ids_by_client_id
                .get(client_id)
                .map(|node_id| node_id.0.to_string())
                .ok_or_else(|| {
                    anyhow!(
                        "plan bootstrap node `{owner_client_id}` references unknown client id `{client_id}` in `{field}`"
                    )
                })
        })
        .collect()
}

fn resolve_bootstrap_node_reference(
    node_ids_by_client_id: &BTreeMap<String, PlanNodeId>,
    client_id: &str,
    field: &str,
) -> Result<PlanNodeId> {
    node_ids_by_client_id
        .get(client_id)
        .cloned()
        .ok_or_else(|| {
            anyhow!("plan bootstrap references unknown client id `{client_id}` in `{field}`")
        })
}

fn affected_intent_specs(observed_changes: &[ObservedChangeSet]) -> HashSet<NodeId> {
    let mut specs = HashSet::new();
    for observed in observed_changes {
        for added in &observed.added {
            if is_intent_source_node_kind(added.node.kind) {
                specs.insert(added.node.id.clone());
            }
        }
        for removed in &observed.removed {
            if is_intent_source_node_kind(removed.node.kind) {
                specs.insert(removed.node.id.clone());
            }
        }
        for (before, after) in &observed.updated {
            if is_intent_source_node_kind(before.node.kind) {
                specs.insert(before.node.id.clone());
            }
            if is_intent_source_node_kind(after.node.kind) {
                specs.insert(after.node.id.clone());
            }
        }
        for edge in observed
            .edge_added
            .iter()
            .chain(observed.edge_removed.iter())
            .filter(|edge| {
                matches!(
                    edge.kind,
                    EdgeKind::Specifies | EdgeKind::Validates | EdgeKind::RelatedTo
                )
            })
        {
            specs.insert(edge.source.clone());
        }
    }
    specs
}

fn is_intent_source_node_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Document
            | NodeKind::MarkdownHeading
            | NodeKind::JsonKey
            | NodeKind::TomlKey
            | NodeKind::YamlKey
    )
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
