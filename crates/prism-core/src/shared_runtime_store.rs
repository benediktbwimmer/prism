use std::path::Path;

use crate::shared_runtime_backend::SharedRuntimeBackend;
use anyhow::{bail, Result};
use prism_store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationPersistBatch,
    CoordinationPersistContext, CoordinationPersistResult, IndexPersistBatch, SnapshotRevisions,
    SqliteStore, Store, WorkspaceTreeSnapshot,
};

pub(crate) enum SharedRuntimeStore {
    Sqlite(SqliteStore),
}

impl SharedRuntimeStore {
    pub(crate) fn open(backend: &SharedRuntimeBackend) -> Result<Option<Self>> {
        match backend {
            SharedRuntimeBackend::Disabled => Ok(None),
            SharedRuntimeBackend::Sqlite { path } => {
                Ok(Some(Self::Sqlite(SqliteStore::open(path)?)))
            }
            SharedRuntimeBackend::Remote { uri } => {
                bail!("shared runtime backend `{uri}` is not implemented yet")
            }
        }
    }

    pub(crate) fn reopen_runtime_writer(&self) -> Result<Self> {
        match self {
            Self::Sqlite(store) => Ok(Self::Sqlite(store.reopen_runtime_writer()?)),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reopen_runtime_reader(&self) -> Result<Self> {
        match self {
            Self::Sqlite(store) => Ok(Self::Sqlite(store.reopen_runtime_reader()?)),
        }
    }

    pub(crate) fn workspace_revision(&self) -> Result<u64> {
        match self {
            Self::Sqlite(store) => store.workspace_revision(),
        }
    }

    pub(crate) fn episodic_revision(&self) -> Result<u64> {
        match self {
            Self::Sqlite(store) => store.episodic_revision(),
        }
    }

    pub(crate) fn snapshot_revisions(&self) -> Result<SnapshotRevisions> {
        match self {
            Self::Sqlite(store) => store.snapshot_revisions(),
        }
    }

    pub(crate) fn upsert_projection_concept(
        &mut self,
        concept: &prism_projections::ConceptPacket,
    ) -> Result<bool> {
        match self {
            Self::Sqlite(store) => store.upsert_projection_concept(concept),
        }
    }

    pub(crate) fn delete_projection_concept(&mut self, handle: &str) -> Result<bool> {
        match self {
            Self::Sqlite(store) => store.delete_projection_concept(handle),
        }
    }

    pub(crate) fn upsert_projection_concept_relation(
        &mut self,
        relation: &prism_projections::ConceptRelation,
    ) -> Result<bool> {
        match self {
            Self::Sqlite(store) => store.upsert_projection_concept_relation(relation),
        }
    }

    pub(crate) fn delete_projection_concept_relation(
        &mut self,
        source_handle: &str,
        target_handle: &str,
        kind: prism_projections::ConceptRelationKind,
    ) -> Result<bool> {
        match self {
            Self::Sqlite(store) => {
                store.delete_projection_concept_relation(source_handle, target_handle, kind)
            }
        }
    }
}

impl Store for SharedRuntimeStore {
    fn load_graph(&mut self) -> Result<Option<prism_store::Graph>> {
        match self {
            Self::Sqlite(store) => store.load_graph(),
        }
    }

    fn load_history_snapshot(&mut self) -> Result<Option<prism_history::HistorySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_history_snapshot(),
        }
    }

    fn load_history_snapshot_with_options(
        &mut self,
        include_events: bool,
    ) -> Result<Option<prism_history::HistorySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_history_snapshot_with_options(include_events),
        }
    }

    fn load_lineage_history(
        &mut self,
        lineage: &prism_ir::LineageId,
    ) -> Result<Vec<prism_ir::LineageEvent>> {
        match self {
            Self::Sqlite(store) => store.load_lineage_history(lineage),
        }
    }

    fn save_history_snapshot(&mut self, snapshot: &prism_history::HistorySnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_history_snapshot(snapshot),
        }
    }

    fn apply_history_delta(&mut self, delta: &prism_history::HistoryPersistDelta) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.apply_history_delta(delta),
        }
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
        deltas: &[prism_projections::CoChangeDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                store.save_history_snapshot_with_co_change_deltas(snapshot, deltas)
            }
        }
    }

    fn load_outcome_snapshot(&mut self) -> Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_outcome_snapshot(),
        }
    }

    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_recent_outcome_snapshot(limit),
        }
    }

    fn load_outcomes(
        &mut self,
        query: &prism_memory::OutcomeRecallQuery,
    ) -> Result<Vec<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => store.load_outcomes(query),
        }
    }

    fn load_outcome_event(
        &mut self,
        event_id: &prism_ir::EventId,
    ) -> Result<Option<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => store.load_outcome_event(event_id),
        }
    }

    fn load_task_replay(&mut self, task_id: &prism_ir::TaskId) -> Result<prism_memory::TaskReplay> {
        match self {
            Self::Sqlite(store) => store.load_task_replay(task_id),
        }
    }

    fn save_outcome_snapshot(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_outcome_snapshot(snapshot),
        }
    }

    fn append_outcome_events(
        &mut self,
        events: &[prism_memory::OutcomeEvent],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> Result<usize> {
        match self {
            Self::Sqlite(store) => store.append_outcome_events(events, validation_deltas),
        }
    }

    fn apply_validation_deltas(
        &mut self,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.apply_validation_deltas(deltas),
        }
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                store.save_outcome_snapshot_with_validation_deltas(snapshot, deltas)
            }
        }
    }

    fn load_memory_events(&mut self) -> Result<Vec<prism_memory::MemoryEvent>> {
        match self {
            Self::Sqlite(store) => store.load_memory_events(),
        }
    }

    fn append_memory_events(&mut self, events: &[prism_memory::MemoryEvent]) -> Result<usize> {
        match self {
            Self::Sqlite(store) => store.append_memory_events(events),
        }
    }

    fn load_episodic_snapshot(&mut self) -> Result<Option<prism_memory::EpisodicMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_episodic_snapshot(),
        }
    }

    fn save_episodic_snapshot(
        &mut self,
        snapshot: &prism_memory::EpisodicMemorySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_episodic_snapshot(snapshot),
        }
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_inference_snapshot(),
        }
    }

    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_inference_snapshot(snapshot),
        }
    }

    fn load_projection_snapshot(
        &mut self,
    ) -> Result<Option<prism_projections::ProjectionSnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_projection_snapshot(),
        }
    }

    fn save_projection_snapshot(
        &mut self,
        snapshot: &prism_projections::ProjectionSnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_projection_snapshot(snapshot),
        }
    }

    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[prism_projections::CoChangeDelta],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                store.apply_projection_deltas(co_change_deltas, validation_deltas)
            }
        }
    }

    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_workspace_tree_snapshot(),
        }
    }

    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_workspace_tree_snapshot(snapshot),
        }
    }

    fn load_curator_snapshot(&mut self) -> Result<Option<prism_curator::CuratorSnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_curator_snapshot(),
        }
    }

    fn save_curator_snapshot(&mut self, snapshot: &prism_curator::CuratorSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_curator_snapshot(snapshot),
        }
    }

    fn load_principal_registry_snapshot(
        &mut self,
    ) -> Result<Option<prism_ir::PrincipalRegistrySnapshot>> {
        match self {
            Self::Sqlite(store) => store.load_principal_registry_snapshot(),
        }
    }

    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &prism_ir::PrincipalRegistrySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_principal_registry_snapshot(snapshot),
        }
    }

    fn coordination_revision(&self) -> Result<u64> {
        match self {
            Self::Sqlite(store) => store.coordination_revision(),
        }
    }

    fn load_coordination_events(&mut self) -> Result<Vec<prism_coordination::CoordinationEvent>> {
        match self {
            Self::Sqlite(store) => store.load_coordination_events(),
        }
    }

    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream> {
        match self {
            Self::Sqlite(store) => store.load_coordination_event_stream(),
        }
    }

    fn save_coordination_compaction(
        &mut self,
        snapshot: &prism_coordination::CoordinationSnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_coordination_compaction(snapshot),
        }
    }

    fn load_coordination_read_model(
        &mut self,
    ) -> Result<Option<prism_coordination::CoordinationReadModel>> {
        match self {
            Self::Sqlite(store) => store.load_coordination_read_model(),
        }
    }

    fn save_coordination_read_model(
        &mut self,
        read_model: &prism_coordination::CoordinationReadModel,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_coordination_read_model(read_model),
        }
    }

    fn load_coordination_queue_read_model(
        &mut self,
    ) -> Result<Option<prism_coordination::CoordinationQueueReadModel>> {
        match self {
            Self::Sqlite(store) => store.load_coordination_queue_read_model(),
        }
    }

    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &prism_coordination::CoordinationQueueReadModel,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_coordination_queue_read_model(read_model),
        }
    }

    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>> {
        match self {
            Self::Sqlite(store) => store.load_latest_coordination_persist_context(),
        }
    }

    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult> {
        match self {
            Self::Sqlite(store) => store.commit_coordination_persist_batch(batch),
        }
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.commit_auxiliary_persist_batch(batch),
        }
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &prism_store::Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.commit_index_persist_batch(graph, batch),
        }
    }

    fn save_graph_snapshot(&mut self, graph: &prism_store::Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_graph_snapshot(graph),
        }
    }

    fn save_file_state(&mut self, path: &Path, graph: &prism_store::Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_file_state(path, graph),
        }
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.remove_file_state(path),
        }
    }

    fn replace_derived_edges(&mut self, graph: &prism_store::Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.replace_derived_edges(graph),
        }
    }

    fn finalize(&mut self, graph: &prism_store::Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.finalize(graph),
        }
    }
}
