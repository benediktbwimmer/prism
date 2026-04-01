use std::collections::HashSet;
use std::path::Path;

use crate::shared_runtime_backend::SharedRuntimeBackend;
use anyhow::{bail, Result};
use prism_ir::EventId;
use prism_store::{
    AuxiliaryPersistBatch, ColdQueryStore, CoordinationCheckpointStore, CoordinationEventStream,
    CoordinationJournal, CoordinationPersistBatch, CoordinationPersistContext,
    CoordinationPersistResult, EventJournalStore, Graph, IndexPersistBatch, MaterializationStore,
    SnapshotRevisions, SqliteStore, Store, WorkspaceTreeSnapshot,
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

    pub(crate) fn load_projection_knowledge_snapshot(
        &mut self,
    ) -> Result<Option<prism_projections::ProjectionSnapshot>> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::load_projection_knowledge_snapshot(store)
            }
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

    pub(crate) fn load_outcome_events_by_ids(
        &mut self,
        event_ids: &[EventId],
    ) -> Result<Vec<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => store.load_outcome_events_by_ids(event_ids),
        }
    }

    pub(crate) fn load_outcomes_by_payload_scan(
        &mut self,
        query: &prism_memory::OutcomeRecallQuery,
        exclude: &HashSet<EventId>,
    ) -> Result<Vec<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => store.load_outcomes_by_payload_scan(query, exclude),
        }
    }
}

impl ColdQueryStore for SharedRuntimeStore {
    fn load_lineage_history(
        &mut self,
        lineage: &prism_ir::LineageId,
    ) -> Result<Vec<prism_ir::LineageEvent>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_lineage_history(store, lineage),
        }
    }

    fn load_history_snapshot(&mut self) -> Result<Option<prism_history::HistorySnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_history_snapshot(store),
        }
    }

    fn load_history_snapshot_with_options(
        &mut self,
        include_events: bool,
    ) -> Result<Option<prism_history::HistorySnapshot>> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::load_history_snapshot_with_options(store, include_events)
            }
        }
    }

    fn load_outcome_snapshot(&mut self) -> Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_outcome_snapshot(store),
        }
    }

    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::load_recent_outcome_snapshot(store, limit)
            }
        }
    }

    fn load_outcomes(
        &mut self,
        query: &prism_memory::OutcomeRecallQuery,
    ) -> Result<Vec<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_outcomes(store, query),
        }
    }

    fn load_outcome_event(
        &mut self,
        event_id: &prism_ir::EventId,
    ) -> Result<Option<prism_memory::OutcomeEvent>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_outcome_event(store, event_id),
        }
    }

    fn load_task_replay(&mut self, task_id: &prism_ir::TaskId) -> Result<prism_memory::TaskReplay> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_task_replay(store, task_id),
        }
    }

    fn load_memory_events(&mut self) -> Result<Vec<prism_memory::MemoryEvent>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_memory_events(store),
        }
    }
}

impl EventJournalStore for SharedRuntimeStore {
    fn apply_history_delta(&mut self, delta: &prism_history::HistoryPersistDelta) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::apply_history_delta(store, delta),
        }
    }

    fn append_outcome_events(
        &mut self,
        events: &[prism_memory::OutcomeEvent],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> Result<usize> {
        match self {
            Self::Sqlite(store) => store.append_shared_outcome_events(events, validation_deltas),
        }
    }

    fn append_memory_events(&mut self, events: &[prism_memory::MemoryEvent]) -> Result<usize> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::append_memory_events(store, events),
        }
    }
}

impl MaterializationStore for SharedRuntimeStore {
    fn apply_validation_deltas(
        &mut self,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::apply_validation_deltas(store, deltas),
        }
    }

    fn load_graph(&mut self) -> Result<Option<Graph>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_graph(store),
        }
    }

    fn save_history_snapshot(&mut self, snapshot: &prism_history::HistorySnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_history_snapshot(store, snapshot),
        }
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
        deltas: &[prism_projections::CoChangeDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_history_snapshot_with_co_change_deltas(
                    store, snapshot, deltas,
                )
            }
        }
    }

    fn save_outcome_snapshot(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => store.save_shared_outcome_snapshot(snapshot),
        }
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        let _ = deltas;
        match self {
            Self::Sqlite(store) => store.save_shared_outcome_snapshot(snapshot),
        }
    }

    fn load_episodic_snapshot(&mut self) -> Result<Option<prism_memory::EpisodicMemorySnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_episodic_snapshot(store),
        }
    }

    fn save_episodic_snapshot(
        &mut self,
        snapshot: &prism_memory::EpisodicMemorySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_episodic_snapshot(store, snapshot),
        }
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_inference_snapshot(store),
        }
    }

    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_inference_snapshot(store, snapshot),
        }
    }

    fn load_projection_snapshot(
        &mut self,
    ) -> Result<Option<prism_projections::ProjectionSnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_projection_snapshot(store),
        }
    }

    fn save_projection_snapshot(
        &mut self,
        snapshot: &prism_projections::ProjectionSnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_projection_snapshot(store, snapshot)
            }
        }
    }

    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[prism_projections::CoChangeDelta],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::apply_projection_deltas(
                store,
                co_change_deltas,
                validation_deltas,
            ),
        }
    }

    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_workspace_tree_snapshot(store),
        }
    }

    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_workspace_tree_snapshot(store, snapshot)
            }
        }
    }

    fn load_curator_snapshot(&mut self) -> Result<Option<prism_curator::CuratorSnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_curator_snapshot(store),
        }
    }

    fn save_curator_snapshot(&mut self, snapshot: &prism_curator::CuratorSnapshot) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_curator_snapshot(store, snapshot),
        }
    }

    fn load_principal_registry_snapshot(
        &mut self,
    ) -> Result<Option<prism_ir::PrincipalRegistrySnapshot>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_principal_registry_snapshot(store),
        }
    }

    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &prism_ir::PrincipalRegistrySnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_principal_registry_snapshot(store, snapshot)
            }
        }
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::commit_auxiliary_persist_batch(store, batch)
            }
        }
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::commit_index_persist_batch(store, graph, batch)
            }
        }
    }

    fn save_graph_snapshot(&mut self, graph: &Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_graph_snapshot(store, graph),
        }
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::save_file_state(store, path, graph),
        }
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::remove_file_state(store, path),
        }
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::replace_derived_edges(store, graph),
        }
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::finalize(store, graph),
        }
    }
}

impl CoordinationJournal for SharedRuntimeStore {
    fn coordination_revision(&self) -> Result<u64> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::coordination_revision(store),
        }
    }

    fn load_coordination_events(&mut self) -> Result<Vec<prism_coordination::CoordinationEvent>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_coordination_events(store),
        }
    }

    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_coordination_event_stream(store),
        }
    }

    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::load_latest_coordination_persist_context(store)
            }
        }
    }

    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::commit_coordination_persist_batch(store, batch)
            }
        }
    }
}

impl CoordinationCheckpointStore for SharedRuntimeStore {
    fn save_coordination_compaction(
        &mut self,
        snapshot: &prism_coordination::CoordinationSnapshot,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_coordination_compaction(store, snapshot)
            }
        }
    }

    fn load_coordination_read_model(
        &mut self,
    ) -> Result<Option<prism_coordination::CoordinationReadModel>> {
        match self {
            Self::Sqlite(store) => <SqliteStore as Store>::load_coordination_read_model(store),
        }
    }

    fn save_coordination_read_model(
        &mut self,
        read_model: &prism_coordination::CoordinationReadModel,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_coordination_read_model(store, read_model)
            }
        }
    }

    fn load_coordination_queue_read_model(
        &mut self,
    ) -> Result<Option<prism_coordination::CoordinationQueueReadModel>> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::load_coordination_queue_read_model(store)
            }
        }
    }

    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &prism_coordination::CoordinationQueueReadModel,
    ) -> Result<()> {
        match self {
            Self::Sqlite(store) => {
                <SqliteStore as Store>::save_coordination_queue_read_model(store, read_model)
            }
        }
    }
}
