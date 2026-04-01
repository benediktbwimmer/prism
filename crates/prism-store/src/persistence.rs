use std::path::Path;

use anyhow::Result;
use prism_coordination::{
    CoordinationEvent, CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
};
use prism_history::{HistoryPersistDelta, HistorySnapshot};
use prism_ir::{EventId, LineageEvent, LineageId, PrincipalRegistrySnapshot, TaskId};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEvent, OutcomeEvent, OutcomeMemorySnapshot, OutcomeRecallQuery,
    TaskReplay,
};
use prism_projections::{CoChangeDelta, ProjectionSnapshot, ValidationDelta};

use crate::graph::Graph;
use crate::store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationPersistBatch,
    CoordinationPersistContext, CoordinationPersistResult, IndexPersistBatch, Store,
    WorkspaceTreeSnapshot,
};

/// Synchronous runtime-authority operations that must remain durable across crash/restart.
///
/// This is intentionally narrower than `Store`: it captures the mutation and replay surface that
/// later persistence-migration tasks will move onto authoritative journal semantics.
pub trait CoordinationJournal {
    fn coordination_revision(&self) -> Result<u64>;
    fn load_coordination_events(&mut self) -> Result<Vec<CoordinationEvent>>;
    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream>;
    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>>;
    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult>;
}

/// Checkpoints and read models that accelerate coordination recovery and queries, but are not the
/// coordination source of truth.
pub trait CoordinationCheckpointStore {
    fn save_coordination_compaction(&mut self, snapshot: &CoordinationSnapshot) -> Result<()>;
    fn load_coordination_read_model(&mut self) -> Result<Option<CoordinationReadModel>>;
    fn save_coordination_read_model(&mut self, read_model: &CoordinationReadModel) -> Result<()>;
    fn load_coordination_queue_read_model(&mut self) -> Result<Option<CoordinationQueueReadModel>>;
    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &CoordinationQueueReadModel,
    ) -> Result<()>;
}

/// Cold history and outcome lookups that remain legitimate on bounded query paths even after live
/// runtime state becomes memory-authoritative.
pub trait ColdQueryStore {
    fn load_lineage_history(&mut self, lineage: &LineageId) -> Result<Vec<LineageEvent>>;
    fn load_history_snapshot(&mut self) -> Result<Option<HistorySnapshot>>;
    fn load_history_snapshot_with_options(
        &mut self,
        include_events: bool,
    ) -> Result<Option<HistorySnapshot>>;
    fn load_outcome_snapshot(&mut self) -> Result<Option<OutcomeMemorySnapshot>>;
    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<OutcomeMemorySnapshot>>;
    fn load_outcomes(&mut self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>>;
    fn load_outcome_event(&mut self, event_id: &EventId) -> Result<Option<OutcomeEvent>>;
    fn load_task_replay(&mut self, task_id: &TaskId) -> Result<TaskReplay>;
    fn load_memory_events(&mut self) -> Result<Vec<MemoryEvent>>;
}

/// Authoritative append-only journal operations for mutable runtime facts.
pub trait EventJournalStore {
    fn apply_history_delta(&mut self, delta: &HistoryPersistDelta) -> Result<()>;
    fn append_outcome_events(
        &mut self,
        events: &[OutcomeEvent],
        validation_deltas: &[ValidationDelta],
    ) -> Result<usize>;
    fn append_memory_events(&mut self, events: &[MemoryEvent]) -> Result<usize>;
}

/// Derived checkpoints and materializations that may lag behind hot runtime state and should move
/// off the request path when correctness allows.
pub trait MaterializationStore {
    fn apply_validation_deltas(&mut self, deltas: &[ValidationDelta]) -> Result<()>;
    fn load_graph(&mut self) -> Result<Option<Graph>>;
    fn save_history_snapshot(&mut self, snapshot: &HistorySnapshot) -> Result<()>;
    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &HistorySnapshot,
        deltas: &[CoChangeDelta],
    ) -> Result<()>;
    fn save_outcome_snapshot(&mut self, snapshot: &OutcomeMemorySnapshot) -> Result<()>;
    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &OutcomeMemorySnapshot,
        deltas: &[ValidationDelta],
    ) -> Result<()>;
    fn load_episodic_snapshot(&mut self) -> Result<Option<EpisodicMemorySnapshot>>;
    fn save_episodic_snapshot(&mut self, snapshot: &EpisodicMemorySnapshot) -> Result<()>;
    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>>;
    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()>;
    fn load_projection_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>>;
    fn save_projection_snapshot(&mut self, snapshot: &ProjectionSnapshot) -> Result<()>;
    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[CoChangeDelta],
        validation_deltas: &[ValidationDelta],
    ) -> Result<()>;
    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>>;
    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()>;
    fn load_curator_snapshot(&mut self) -> Result<Option<prism_curator::CuratorSnapshot>>;
    fn save_curator_snapshot(&mut self, snapshot: &prism_curator::CuratorSnapshot) -> Result<()>;
    fn load_principal_registry_snapshot(&mut self) -> Result<Option<PrincipalRegistrySnapshot>>;
    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &PrincipalRegistrySnapshot,
    ) -> Result<()>;
    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()>;
    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()>;
    fn save_graph_snapshot(&mut self, graph: &Graph) -> Result<()>;
    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()>;
    fn remove_file_state(&mut self, path: &Path) -> Result<()>;
    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()>;
    fn finalize(&mut self, graph: &Graph) -> Result<()>;
}

impl<T: Store + ?Sized> CoordinationJournal for T {
    fn coordination_revision(&self) -> Result<u64> {
        Store::coordination_revision(self)
    }

    fn load_coordination_events(&mut self) -> Result<Vec<CoordinationEvent>> {
        Store::load_coordination_events(self)
    }

    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream> {
        Store::load_coordination_event_stream(self)
    }

    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>> {
        Store::load_latest_coordination_persist_context(self)
    }

    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult> {
        Store::commit_coordination_persist_batch(self, batch)
    }
}

impl<T: Store + ?Sized> CoordinationCheckpointStore for T {
    fn save_coordination_compaction(&mut self, snapshot: &CoordinationSnapshot) -> Result<()> {
        Store::save_coordination_compaction(self, snapshot)
    }

    fn load_coordination_read_model(&mut self) -> Result<Option<CoordinationReadModel>> {
        Store::load_coordination_read_model(self)
    }

    fn save_coordination_read_model(&mut self, read_model: &CoordinationReadModel) -> Result<()> {
        Store::save_coordination_read_model(self, read_model)
    }

    fn load_coordination_queue_read_model(&mut self) -> Result<Option<CoordinationQueueReadModel>> {
        Store::load_coordination_queue_read_model(self)
    }

    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &CoordinationQueueReadModel,
    ) -> Result<()> {
        Store::save_coordination_queue_read_model(self, read_model)
    }
}

impl<T: Store + ?Sized> ColdQueryStore for T {
    fn load_lineage_history(&mut self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        Store::load_lineage_history(self, lineage)
    }

    fn load_history_snapshot(&mut self) -> Result<Option<HistorySnapshot>> {
        Store::load_history_snapshot(self)
    }

    fn load_history_snapshot_with_options(
        &mut self,
        include_events: bool,
    ) -> Result<Option<HistorySnapshot>> {
        Store::load_history_snapshot_with_options(self, include_events)
    }

    fn load_outcome_snapshot(&mut self) -> Result<Option<OutcomeMemorySnapshot>> {
        Store::load_outcome_snapshot(self)
    }

    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<OutcomeMemorySnapshot>> {
        Store::load_recent_outcome_snapshot(self, limit)
    }

    fn load_outcomes(&mut self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        Store::load_outcomes(self, query)
    }

    fn load_outcome_event(&mut self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Store::load_outcome_event(self, event_id)
    }

    fn load_task_replay(&mut self, task_id: &TaskId) -> Result<TaskReplay> {
        Store::load_task_replay(self, task_id)
    }

    fn load_memory_events(&mut self) -> Result<Vec<MemoryEvent>> {
        Store::load_memory_events(self)
    }
}

impl<T: Store + ?Sized> EventJournalStore for T {
    fn apply_history_delta(&mut self, delta: &HistoryPersistDelta) -> Result<()> {
        Store::apply_history_delta(self, delta)
    }

    fn append_outcome_events(
        &mut self,
        events: &[OutcomeEvent],
        validation_deltas: &[ValidationDelta],
    ) -> Result<usize> {
        Store::append_outcome_events(self, events, validation_deltas)
    }

    fn append_memory_events(&mut self, events: &[MemoryEvent]) -> Result<usize> {
        Store::append_memory_events(self, events)
    }
}

impl<T: Store + ?Sized> MaterializationStore for T {
    fn apply_validation_deltas(&mut self, deltas: &[ValidationDelta]) -> Result<()> {
        Store::apply_validation_deltas(self, deltas)
    }

    fn load_graph(&mut self) -> Result<Option<Graph>> {
        Store::load_graph(self)
    }

    fn save_history_snapshot(&mut self, snapshot: &HistorySnapshot) -> Result<()> {
        Store::save_history_snapshot(self, snapshot)
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &HistorySnapshot,
        deltas: &[CoChangeDelta],
    ) -> Result<()> {
        Store::save_history_snapshot_with_co_change_deltas(self, snapshot, deltas)
    }

    fn save_outcome_snapshot(&mut self, snapshot: &OutcomeMemorySnapshot) -> Result<()> {
        Store::save_outcome_snapshot(self, snapshot)
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &OutcomeMemorySnapshot,
        deltas: &[ValidationDelta],
    ) -> Result<()> {
        Store::save_outcome_snapshot_with_validation_deltas(self, snapshot, deltas)
    }

    fn load_episodic_snapshot(&mut self) -> Result<Option<EpisodicMemorySnapshot>> {
        Store::load_episodic_snapshot(self)
    }

    fn save_episodic_snapshot(&mut self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        Store::save_episodic_snapshot(self, snapshot)
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>> {
        Store::load_inference_snapshot(self)
    }

    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()> {
        Store::save_inference_snapshot(self, snapshot)
    }

    fn load_projection_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>> {
        Store::load_projection_snapshot(self)
    }

    fn save_projection_snapshot(&mut self, snapshot: &ProjectionSnapshot) -> Result<()> {
        Store::save_projection_snapshot(self, snapshot)
    }

    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[CoChangeDelta],
        validation_deltas: &[ValidationDelta],
    ) -> Result<()> {
        Store::apply_projection_deltas(self, co_change_deltas, validation_deltas)
    }

    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>> {
        Store::load_workspace_tree_snapshot(self)
    }

    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()> {
        Store::save_workspace_tree_snapshot(self, snapshot)
    }

    fn load_curator_snapshot(&mut self) -> Result<Option<prism_curator::CuratorSnapshot>> {
        Store::load_curator_snapshot(self)
    }

    fn save_curator_snapshot(&mut self, snapshot: &prism_curator::CuratorSnapshot) -> Result<()> {
        Store::save_curator_snapshot(self, snapshot)
    }

    fn load_principal_registry_snapshot(&mut self) -> Result<Option<PrincipalRegistrySnapshot>> {
        Store::load_principal_registry_snapshot(self)
    }

    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &PrincipalRegistrySnapshot,
    ) -> Result<()> {
        Store::save_principal_registry_snapshot(self, snapshot)
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        Store::commit_auxiliary_persist_batch(self, batch)
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()> {
        Store::commit_index_persist_batch(self, graph, batch)
    }

    fn save_graph_snapshot(&mut self, graph: &Graph) -> Result<()> {
        Store::save_graph_snapshot(self, graph)
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        Store::save_file_state(self, path, graph)
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        Store::remove_file_state(self, path)
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        Store::replace_derived_edges(self, graph)
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        Store::finalize(self, graph)
    }
}
