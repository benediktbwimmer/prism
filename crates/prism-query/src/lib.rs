mod common;
mod contracts;
mod coordination;
mod coordination_query_engine;
mod coordination_transaction;
mod impact;
mod intent;
mod materialized_runtime;
mod outcomes;
mod plan_activity;
mod plan_discovery;
mod plan_insights;
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
    canonical_task_heartbeat_due_state_with_runtime_descriptors,
    canonical_task_lease_state_with_runtime_descriptors, Artifact, ArtifactProposeInput,
    ArtifactReview, ArtifactReviewInput, ArtifactSupersedeInput, CoordinationConflict,
    CoordinationRuntimeState, CoordinationSnapshot, CoordinationSnapshotV2, CoordinationSpecRef,
    CoordinationTask, CoordinationTaskSpecRef, HandoffAcceptInput, HandoffInput,
    LeaseHeartbeatDueState, LeaseState, PlanScheduling, RuntimeDescriptor, TaskCreateInput,
    TaskReclaimInput, TaskResumeInput, TaskUpdateInput, WorkClaim,
};
use prism_history::{HistorySnapshot, HistoryStore};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, ClaimId, CoordinationTaskId, EdgeKind, EventId, EventMeta,
    LineageEvent, LineageId, NodeId, NodeKind, ObservedChangeSet, PlanId, PlanStatus,
    PrismRuntimeCapabilities, PrismRuntimeMode, ReviewId, SessionId, TaskId, WorkspaceRevision,
};
use prism_memory::{OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot};
use prism_memory::{OutcomeRecallQuery, TaskReplay};
pub use prism_projections::ConceptResolution;
use prism_projections::{IntentIndex, ProjectionIndex, ProjectionSnapshot};
use prism_store::{CoordinationPersistContext, Graph};
use tracing::info;

use crate::common::{anchor_sort_key, dedupe_node_ids, sort_node_ids};
use crate::materialized_runtime::MaterializedCoordinationRuntime;
use crate::plan_discovery::PlanDiscoveryCache;

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
    ContractStatus, ContractTarget, ContractValidation, CoordinationPlanV2, CoordinationTaskV2,
    DriftCandidate, PlanActivity, PlanListEntry, PlanNodeStatusCounts, PlanSummary, QueryLimits,
    TaskEvidenceArtifactStatus, TaskEvidenceStatus, TaskIntent, TaskReviewStatus, TaskRisk,
    TaskValidationRecipe, ValidationCheck, ValidationRecipe,
};
pub use coordination_transaction::{
    CoordinationDependencyKind, CoordinationTransactionAuthorityVersion,
    CoordinationTransactionCommitMetadata, CoordinationTransactionError,
    CoordinationTransactionGitExecutionPolicyPatch, CoordinationTransactionInput,
    CoordinationTransactionMutation, CoordinationTransactionOutcome,
    CoordinationTransactionPlanRef, CoordinationTransactionPlanSchedulingPatch,
    CoordinationTransactionPolicyPatch, CoordinationTransactionProtocolAuthorityVersion,
    CoordinationTransactionProtocolCommit, CoordinationTransactionProtocolIndeterminate,
    CoordinationTransactionProtocolRejection, CoordinationTransactionProtocolState,
    CoordinationTransactionRejection, CoordinationTransactionRejectionCategory,
    CoordinationTransactionResult, CoordinationTransactionTaskRef,
    CoordinationTransactionValidationStage,
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
    runtime_capabilities: RwLock<PrismRuntimeCapabilities>,
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
pub struct NativePlanBootstrapResult {
    pub plan_id: PlanId,
    pub task_ids_by_client_id: BTreeMap<String, CoordinationTaskId>,
}

#[derive(Debug, Clone)]
pub struct NativePlanMutationResult {
    pub plan_id: PlanId,
    pub transaction: CoordinationTransactionResult,
}

#[derive(Debug, Clone)]
pub struct NativeTaskMutationResult {
    pub task_id: CoordinationTaskId,
    pub transaction: CoordinationTransactionResult,
}

#[derive(Debug, Clone)]
pub struct NativeSpecPlanCreateInput {
    pub title: String,
    pub goal: String,
    pub status: Option<PlanStatus>,
    pub policy: Option<prism_coordination::CoordinationPolicy>,
    pub scheduling: Option<PlanScheduling>,
    pub spec_ref: CoordinationSpecRef,
}

#[derive(Debug, Clone)]
pub struct NativeSpecTaskCreateInput {
    pub task: TaskCreateInput,
    pub spec_ref: CoordinationTaskSpecRef,
}

#[derive(Debug, Clone)]
pub struct NativePlanBootstrapTransactionResult {
    pub plan_id: PlanId,
    pub task_ids_by_client_id: BTreeMap<String, CoordinationTaskId>,
    pub transaction: CoordinationTransactionResult,
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
            false,
        )
    }

    pub fn with_shared_history_outcomes_coordination_projections_and_intent(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
        runtime_descriptors: Vec<RuntimeDescriptor>,
        intent_override: Option<IntentIndex>,
    ) -> Self {
        Self::with_shared_history_outcomes_coordination_projections_and_query_state(
            graph,
            history,
            outcomes,
            coordination,
            projections,
            runtime_descriptors,
            intent_override,
            false,
        )
    }

    pub fn with_shared_history_outcomes_coordination_projections_and_query_state(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        coordination: CoordinationSnapshot,
        projections: ProjectionIndex,
        runtime_descriptors: Vec<RuntimeDescriptor>,
        intent_override: Option<IntentIndex>,
        trust_cached_projections: bool,
    ) -> Self {
        Self::with_shared_history_outcomes_coordination_projections_and_native_plans(
            graph,
            history,
            outcomes,
            projections,
            MaterializedCoordinationRuntime::from_snapshot_with_runtime_descriptors(
                coordination,
                runtime_descriptors,
            ),
            intent_override,
            trust_cached_projections,
        )
    }

    fn with_shared_history_outcomes_coordination_projections_and_native_plans(
        graph: Arc<Graph>,
        history: Arc<HistoryStore>,
        outcomes: Arc<OutcomeMemory>,
        mut projections: ProjectionIndex,
        materialized_runtime: MaterializedCoordinationRuntime,
        intent_override: Option<IntentIndex>,
        trust_cached_projections: bool,
    ) -> Self {
        let graph_version = if trust_cached_projections {
            history.event_count() as u64
        } else {
            let history_snapshot = history.snapshot();
            projections.reseed_from_history(&history_snapshot);
            history_snapshot.events.len() as u64
        };
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
            graph_version,
            git_commit: None,
        };
        info!(
            node_count,
            edge_count,
            file_count,
            derive_intent_ms,
            reused_intent,
            trust_cached_projections,
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
            runtime_capabilities: RwLock::new(PrismRuntimeMode::Full.capabilities()),
        }
    }

    pub fn graph(&self) -> &Graph {
        self.graph.as_ref()
    }

    pub fn runtime_capabilities(&self) -> PrismRuntimeCapabilities {
        *self
            .runtime_capabilities
            .read()
            .expect("runtime capabilities lock poisoned")
    }

    pub fn set_runtime_capabilities(&self, capabilities: PrismRuntimeCapabilities) {
        *self
            .runtime_capabilities
            .write()
            .expect("runtime capabilities lock poisoned") = capabilities;
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

    pub fn coordination_snapshot_v2(&self) -> CoordinationSnapshotV2 {
        let runtime = self
            .materialized_runtime
            .read()
            .expect("materialized runtime lock poisoned");
        runtime.snapshot().to_canonical_snapshot_v2()
    }

    pub fn replace_coordination_snapshot(&self, snapshot: CoordinationSnapshot) {
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .replace_from_snapshot(snapshot.clone());
        self.prune_local_assisted_leases(&snapshot);
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_coordination_runtime(
        &self,
        snapshot: CoordinationSnapshot,
        runtime_descriptors: Vec<RuntimeDescriptor>,
    ) {
        let prune_snapshot = snapshot.clone();
        let mut runtime = self
            .materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned");
        runtime.replace_from_snapshot(snapshot);
        runtime.replace_runtime_descriptors(runtime_descriptors);
        self.prune_local_assisted_leases(&prune_snapshot);
        self.invalidate_plan_discovery_cache();
    }

    pub fn replace_runtime_descriptors(&self, runtime_descriptors: Vec<RuntimeDescriptor>) {
        self.materialized_runtime
            .write()
            .expect("materialized runtime lock poisoned")
            .replace_runtime_descriptors(runtime_descriptors);
        self.invalidate_plan_discovery_cache();
    }

    pub fn runtime_descriptors(&self) -> Vec<RuntimeDescriptor> {
        self.materialized_runtime
            .read()
            .expect("materialized runtime lock poisoned")
            .runtime_descriptors()
            .to_vec()
    }

    pub fn effective_task_lease_state(&self, task: &CoordinationTask, now: u64) -> LeaseState {
        self.with_coordination_runtime(|runtime| runtime.task_lease_state(task, now))
    }

    pub fn effective_canonical_task_lease_state(
        &self,
        task: &prism_coordination::CanonicalTaskRecord,
        now: u64,
    ) -> LeaseState {
        canonical_task_lease_state_with_runtime_descriptors(task, &self.runtime_descriptors(), now)
    }

    pub fn effective_claim_lease_state(&self, claim: &WorkClaim, now: u64) -> LeaseState {
        self.with_coordination_runtime(|runtime| runtime.claim_lease_state(claim, now))
    }

    pub fn effective_task_heartbeat_due_state(
        &self,
        task: &CoordinationTask,
        policy: &prism_coordination::CoordinationPolicy,
        now: u64,
    ) -> LeaseHeartbeatDueState {
        self.with_coordination_runtime(|runtime| {
            runtime.task_heartbeat_due_state(task, policy, now)
        })
    }

    pub fn effective_canonical_task_heartbeat_due_state(
        &self,
        task: &prism_coordination::CanonicalTaskRecord,
        policy: &prism_coordination::CoordinationPolicy,
        now: u64,
    ) -> LeaseHeartbeatDueState {
        canonical_task_heartbeat_due_state_with_runtime_descriptors(
            task,
            policy,
            &self.runtime_descriptors(),
            now,
        )
    }

    pub fn effective_claim_heartbeat_due_state(
        &self,
        claim: &WorkClaim,
        policy: &prism_coordination::CoordinationPolicy,
        now: u64,
    ) -> LeaseHeartbeatDueState {
        self.with_coordination_runtime(|runtime| {
            runtime.claim_heartbeat_due_state(claim, policy, now)
        })
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

    pub fn canonical_task_has_active_local_assisted_lease(
        &self,
        task: &prism_coordination::CanonicalTaskRecord,
        now: u64,
    ) -> bool {
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

    fn finalize_live_coordination_mutation<T>(
        &self,
        snapshot: CoordinationSnapshot,
        result: Result<T>,
    ) -> Result<T> {
        match result {
            Ok(value) => {
                self.refresh_plan_runtime_from_coordination();
                Ok(value)
            }
            Err(error) => {
                self.persist_coordination_snapshot(snapshot)?;
                Err(error)
            }
        }
    }

    fn current_native_task_view(&self, task_id: &CoordinationTaskId) -> Result<CoordinationTaskV2> {
        self.coordination_task_v2_by_coordination_id(task_id)
            .ok_or_else(|| anyhow!("unknown coordination task `{}`", task_id.0))
    }

    pub fn create_native_task(
        &self,
        meta: EventMeta,
        input: TaskCreateInput,
    ) -> Result<CoordinationTaskV2> {
        let result = self.create_native_task_transaction(meta, input)?;
        self.current_native_task_view(&result.task_id)
    }

    pub fn update_native_task_transaction(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        _now: u64,
    ) -> Result<CoordinationTransactionResult> {
        let mut input = input;
        if let Some(context) = self.coordination_context() {
            if matches!(input.session, Some(Some(_))) {
                input.worktree_id = Some(Some(context.worktree_id));
                input.branch_ref = Some(context.branch_ref);
            } else if matches!(input.session, Some(None)) {
                input.worktree_id = Some(None);
                input.branch_ref = Some(None);
            }
        }
        Ok(self.execute_coordination_mutation(
            meta,
            CoordinationTransactionMutation::TaskUpdate {
                task: CoordinationTransactionTaskRef::Id(input.task_id),
                status: input.status,
                published_task_status: input.published_task_status,
                git_execution: input.git_execution,
                assignee: input.assignee,
                session: input.session,
                worktree_id: input.worktree_id,
                branch_ref: input.branch_ref,
                title: input.title,
                summary: input.summary,
                anchors: input.anchors,
                bindings: input.bindings,
                depends_on: input.depends_on.map(|depends_on| {
                    depends_on
                        .into_iter()
                        .map(CoordinationTransactionTaskRef::Id)
                        .collect()
                }),
                acceptance: input.acceptance,
                validation_refs: input.validation_refs,
                base_revision: input.base_revision.unwrap_or(current_revision),
                priority: input.priority,
                tags: input.tags,
                completion_context: input.completion_context,
                spec_refs: input.spec_refs,
            },
        )?)
    }

    pub fn update_native_task(
        &self,
        meta: EventMeta,
        input: TaskUpdateInput,
        current_revision: WorkspaceRevision,
        now: u64,
    ) -> Result<CoordinationTaskV2> {
        let task_id = input.task_id.clone();
        self.update_native_task_transaction(meta, input, current_revision, now)?;
        self.current_native_task_view(&task_id)
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
        let _ = before_snapshot;
        self.finalize_live_coordination_mutation(snapshot, result)
    }

    pub fn request_native_handoff_transaction(
        &self,
        meta: EventMeta,
        input: HandoffInput,
        _current_revision: WorkspaceRevision,
    ) -> Result<NativeTaskMutationResult> {
        let task_id = input.task_id.clone();
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::TaskHandoff {
                    task: CoordinationTransactionTaskRef::Id(task_id.clone()),
                    to_agent: input.to_agent,
                    summary: input.summary,
                    base_revision: input.base_revision,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        Ok(NativeTaskMutationResult {
            task_id,
            transaction,
        })
    }

    pub fn request_native_handoff(
        &self,
        meta: EventMeta,
        input: HandoffInput,
        current_revision: WorkspaceRevision,
    ) -> Result<CoordinationTaskV2> {
        let result = self.request_native_handoff_transaction(meta, input, current_revision)?;
        self.current_native_task_view(&result.task_id)
    }

    pub fn accept_native_handoff_transaction(
        &self,
        meta: EventMeta,
        mut input: HandoffAcceptInput,
    ) -> Result<NativeTaskMutationResult> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let task_id = input.task_id.clone();
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::TaskHandoffAccept {
                    task: CoordinationTransactionTaskRef::Id(task_id.clone()),
                    agent: input.agent,
                    worktree_id: input.worktree_id,
                    branch_ref: input.branch_ref,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        Ok(NativeTaskMutationResult {
            task_id,
            transaction,
        })
    }

    pub fn accept_native_handoff(
        &self,
        meta: EventMeta,
        input: HandoffAcceptInput,
    ) -> Result<CoordinationTaskV2> {
        let result = self.accept_native_handoff_transaction(meta, input)?;
        self.current_native_task_view(&result.task_id)
    }

    pub fn resume_native_task_transaction(
        &self,
        meta: EventMeta,
        mut input: TaskResumeInput,
    ) -> Result<NativeTaskMutationResult> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let task_id = input.task_id.clone();
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::TaskResume {
                    task: CoordinationTransactionTaskRef::Id(task_id.clone()),
                    agent: input.agent,
                    worktree_id: input.worktree_id,
                    branch_ref: input.branch_ref,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        Ok(NativeTaskMutationResult {
            task_id,
            transaction,
        })
    }

    pub fn resume_native_task(
        &self,
        meta: EventMeta,
        input: TaskResumeInput,
    ) -> Result<CoordinationTaskV2> {
        let result = self.resume_native_task_transaction(meta, input)?;
        self.current_native_task_view(&result.task_id)
    }

    pub fn reclaim_native_task_transaction(
        &self,
        meta: EventMeta,
        mut input: TaskReclaimInput,
    ) -> Result<NativeTaskMutationResult> {
        if let Some(context) = self.coordination_context() {
            input.worktree_id = Some(context.worktree_id);
            input.branch_ref = context.branch_ref;
        }
        let task_id = input.task_id.clone();
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::TaskReclaim {
                    task: CoordinationTransactionTaskRef::Id(task_id.clone()),
                    agent: input.agent,
                    worktree_id: input.worktree_id,
                    branch_ref: input.branch_ref,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        Ok(NativeTaskMutationResult {
            task_id,
            transaction,
        })
    }

    pub fn reclaim_native_task(
        &self,
        meta: EventMeta,
        input: TaskReclaimInput,
    ) -> Result<CoordinationTaskV2> {
        let result = self.reclaim_native_task_transaction(meta, input)?;
        self.current_native_task_view(&result.task_id)
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
        let _ = before_snapshot;
        self.finalize_live_coordination_mutation(snapshot, result)
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
        Ok(self
            .create_native_plan_with_scheduling_transaction(
                meta, title, goal, status, policy, scheduling,
            )?
            .plan_id)
    }

    pub fn create_native_plan_with_scheduling_transaction(
        &self,
        meta: EventMeta,
        title: String,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
        scheduling: Option<prism_coordination::PlanScheduling>,
    ) -> Result<NativePlanMutationResult> {
        self.create_native_plan_with_spec_refs_transaction(
            meta,
            title,
            goal,
            status,
            policy,
            scheduling,
            Vec::new(),
        )
    }

    pub fn create_native_plan_from_spec_transaction(
        &self,
        meta: EventMeta,
        input: NativeSpecPlanCreateInput,
    ) -> Result<NativePlanMutationResult> {
        self.create_native_plan_with_spec_refs_transaction(
            meta,
            input.title,
            input.goal,
            input.status,
            input.policy,
            input.scheduling,
            vec![input.spec_ref],
        )
    }

    fn create_native_plan_with_spec_refs_transaction(
        &self,
        meta: EventMeta,
        title: String,
        goal: String,
        status: Option<prism_ir::PlanStatus>,
        policy: Option<prism_coordination::CoordinationPolicy>,
        scheduling: Option<prism_coordination::PlanScheduling>,
        spec_refs: Vec<CoordinationSpecRef>,
    ) -> Result<NativePlanMutationResult> {
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanCreate {
                    client_plan_id: Some("created_plan".to_string()),
                    title,
                    goal,
                    status,
                    policy,
                    scheduling,
                    spec_refs,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        let plan_id = transaction
            .plan_ids_by_client_id
            .get("created_plan")
            .cloned()
            .ok_or_else(|| anyhow!("coordination transaction did not create a plan"))?;
        Ok(NativePlanMutationResult {
            plan_id,
            transaction,
        })
    }

    pub fn bootstrap_native_plan(
        &self,
        meta: EventMeta,
        input: NativePlanBootstrapInput,
    ) -> Result<NativePlanBootstrapResult> {
        let result = self.bootstrap_native_plan_transaction(meta, input)?;
        Ok(NativePlanBootstrapResult {
            plan_id: result.plan_id,
            task_ids_by_client_id: result.task_ids_by_client_id,
        })
    }

    pub fn bootstrap_native_plan_transaction(
        &self,
        meta: EventMeta,
        input: NativePlanBootstrapInput,
    ) -> Result<NativePlanBootstrapTransactionResult> {
        let NativePlanBootstrapInput {
            title,
            goal,
            status,
            policy,
            scheduling,
            tasks,
        } = input;

        let mut mutations = Vec::with_capacity(1 + tasks.len() * 2);
        let bootstrap_plan_client_id = "bootstrap_plan".to_string();
        mutations.push(CoordinationTransactionMutation::PlanCreate {
            client_plan_id: Some(bootstrap_plan_client_id.clone()),
            title,
            goal,
            status,
            policy,
            scheduling,
            spec_refs: Vec::new(),
        });
        for task in &tasks {
            mutations.push(CoordinationTransactionMutation::TaskCreate {
                client_task_id: Some(task.client_id.clone()),
                plan: CoordinationTransactionPlanRef::ClientId(bootstrap_plan_client_id.clone()),
                title: task.title.clone(),
                status: task.status,
                assignee: task.assignee.clone(),
                session: task.session.clone(),
                worktree_id: None,
                branch_ref: None,
                anchors: task.anchors.clone(),
                depends_on: Vec::new(),
                coordination_depends_on: Vec::new(),
                integrated_depends_on: Vec::new(),
                acceptance: task.acceptance.clone(),
                base_revision: task.base_revision.clone(),
                spec_refs: Vec::new(),
            });
        }
        for task in tasks {
            for depends_on in task.depends_on {
                mutations.push(CoordinationTransactionMutation::DependencyCreate {
                    task: CoordinationTransactionTaskRef::ClientId(task.client_id.clone()),
                    depends_on: CoordinationTransactionTaskRef::ClientId(depends_on),
                    kind: CoordinationDependencyKind::DependsOn,
                    base_revision: task.base_revision.clone(),
                });
            }
            for depends_on in task.coordination_depends_on {
                mutations.push(CoordinationTransactionMutation::DependencyCreate {
                    task: CoordinationTransactionTaskRef::ClientId(task.client_id.clone()),
                    depends_on: CoordinationTransactionTaskRef::ClientId(depends_on),
                    kind: CoordinationDependencyKind::CoordinationDependsOn,
                    base_revision: task.base_revision.clone(),
                });
            }
            for depends_on in task.integrated_depends_on {
                mutations.push(CoordinationTransactionMutation::DependencyCreate {
                    task: CoordinationTransactionTaskRef::ClientId(task.client_id.clone()),
                    depends_on: CoordinationTransactionTaskRef::ClientId(depends_on),
                    kind: CoordinationDependencyKind::IntegratedDependsOn,
                    base_revision: task.base_revision.clone(),
                });
            }
        }
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations,
                ..CoordinationTransactionInput::default()
            },
        )?;
        Ok(NativePlanBootstrapTransactionResult {
            plan_id: transaction
                .plan_ids_by_client_id
                .get(&bootstrap_plan_client_id)
                .cloned()
                .ok_or_else(|| anyhow!("coordination transaction did not create bootstrap plan"))?,
            task_ids_by_client_id: transaction.task_ids_by_client_id.clone(),
            transaction,
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
        self.update_native_plan_with_scheduling_transaction(
            meta, plan_id, title, status, goal, policy, scheduling,
        )?;
        Ok(())
    }

    pub fn update_native_plan_with_scheduling_transaction(
        &self,
        meta: EventMeta,
        plan_id: &PlanId,
        title: Option<String>,
        status: Option<prism_ir::PlanStatus>,
        goal: Option<String>,
        policy: Option<prism_coordination::CoordinationPolicy>,
        scheduling: Option<prism_coordination::PlanScheduling>,
    ) -> Result<CoordinationTransactionResult> {
        Ok(self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanUpdate {
                    plan: CoordinationTransactionPlanRef::Id(plan_id.clone()),
                    title,
                    goal,
                    status,
                    policy: policy.map(|policy| CoordinationTransactionPolicyPatch {
                        default_claim_mode: Some(policy.default_claim_mode),
                        max_parallel_editors_per_anchor: Some(
                            policy.max_parallel_editors_per_anchor,
                        ),
                        require_review_for_completion: Some(policy.require_review_for_completion),
                        require_validation_for_completion: Some(
                            policy.require_validation_for_completion,
                        ),
                        stale_after_graph_change: Some(policy.stale_after_graph_change),
                        review_required_above_risk_score: policy.review_required_above_risk_score,
                        lease_stale_after_seconds: Some(policy.lease_stale_after_seconds),
                        lease_expires_after_seconds: Some(policy.lease_expires_after_seconds),
                        lease_renewal_mode: Some(policy.lease_renewal_mode),
                        git_execution: Some(CoordinationTransactionGitExecutionPolicyPatch {
                            start_mode: Some(policy.git_execution.start_mode),
                            completion_mode: Some(policy.git_execution.completion_mode),
                            integration_mode: Some(policy.git_execution.integration_mode),
                            target_ref: policy.git_execution.target_ref,
                            target_branch: Some(policy.git_execution.target_branch),
                            require_task_branch: Some(policy.git_execution.require_task_branch),
                            max_commits_behind_target: Some(
                                policy.git_execution.max_commits_behind_target,
                            ),
                            max_fetch_age_seconds: policy.git_execution.max_fetch_age_seconds,
                        }),
                    }),
                    scheduling: scheduling.map(|scheduling| {
                        CoordinationTransactionPlanSchedulingPatch {
                            importance: Some(scheduling.importance),
                            urgency: Some(scheduling.urgency),
                            manual_boost: Some(scheduling.manual_boost),
                            due_at: scheduling.due_at,
                        }
                    }),
                    spec_refs: None,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?)
    }

    pub fn archive_native_plan_transaction(
        &self,
        meta: EventMeta,
        plan_id: &PlanId,
    ) -> Result<CoordinationTransactionResult> {
        Ok(self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::PlanArchive {
                    plan: CoordinationTransactionPlanRef::Id(plan_id.clone()),
                }],
                ..CoordinationTransactionInput::default()
            },
        )?)
    }

    pub fn create_native_task_transaction(
        &self,
        meta: EventMeta,
        input: TaskCreateInput,
    ) -> Result<NativeTaskMutationResult> {
        self.create_native_task_with_spec_refs_transaction(meta, input)
    }

    pub fn create_native_task_from_spec_transaction(
        &self,
        meta: EventMeta,
        input: NativeSpecTaskCreateInput,
    ) -> Result<NativeTaskMutationResult> {
        let mut task = input.task;
        task.spec_refs.push(input.spec_ref);
        self.create_native_task_with_spec_refs_transaction(meta, task)
    }

    fn create_native_task_with_spec_refs_transaction(
        &self,
        meta: EventMeta,
        mut input: TaskCreateInput,
    ) -> Result<NativeTaskMutationResult> {
        if let Some(context) = self.coordination_context() {
            if input.session.is_some() {
                input.worktree_id = Some(context.worktree_id);
                input.branch_ref = context.branch_ref;
            }
        }
        let transaction = self.execute_coordination_transaction(
            meta,
            CoordinationTransactionInput {
                mutations: vec![CoordinationTransactionMutation::TaskCreate {
                    client_task_id: Some("created_task".to_string()),
                    plan: CoordinationTransactionPlanRef::Id(input.plan_id),
                    title: input.title,
                    status: input.status,
                    assignee: input.assignee,
                    session: input.session,
                    worktree_id: input.worktree_id,
                    branch_ref: input.branch_ref,
                    anchors: input.anchors,
                    depends_on: input
                        .depends_on
                        .into_iter()
                        .map(CoordinationTransactionTaskRef::Id)
                        .collect(),
                    coordination_depends_on: input
                        .coordination_depends_on
                        .into_iter()
                        .map(CoordinationTransactionTaskRef::Id)
                        .collect(),
                    integrated_depends_on: input
                        .integrated_depends_on
                        .into_iter()
                        .map(CoordinationTransactionTaskRef::Id)
                        .collect(),
                    acceptance: input.acceptance,
                    base_revision: input.base_revision,
                    spec_refs: input.spec_refs,
                }],
                ..CoordinationTransactionInput::default()
            },
        )?;
        let task_id = transaction
            .task_ids_by_client_id
            .get("created_task")
            .cloned()
            .ok_or_else(|| anyhow!("coordination transaction did not create a task"))?;
        Ok(NativeTaskMutationResult {
            task_id,
            transaction,
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
