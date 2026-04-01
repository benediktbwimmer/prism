use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;

use anyhow::Result;
use prism_agent::{InferenceSnapshot, InferredEdgeRecord};
use prism_coordination::{
    CoordinationEvent, CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
};
use prism_curator::CuratorSnapshot;
use prism_history::{HistoryPersistDelta, HistorySnapshot};
use prism_ir::{EventId, LineageEvent, LineageId, PrincipalRegistrySnapshot, TaskId};
use prism_memory::{
    EpisodicMemorySnapshot, MemoryEvent, OutcomeEvent, OutcomeMemory, OutcomeMemorySnapshot,
    OutcomeRecallQuery, TaskReplay,
};
use prism_projections::{CoChangeDelta, ProjectionSnapshot, ValidationDelta};
use serde::{Deserialize, Serialize};

use crate::graph::Graph;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkspaceTreeSnapshot {
    pub root_hash: u64,
    pub files: BTreeMap<PathBuf, WorkspaceTreeFileFingerprint>,
    pub directories: BTreeMap<PathBuf, WorkspaceTreeDirectoryFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTreeFileFingerprint {
    pub len: u64,
    pub modified_ns: Option<u128>,
    pub changed_ns: Option<u128>,
    pub content_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTreeDirectoryFingerprint {
    pub aggregate_hash: u64,
    pub file_count: usize,
    pub modified_ns: Option<u128>,
    pub changed_ns: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoordinationPersistContext {
    pub repo_id: String,
    pub worktree_id: String,
    pub branch_ref: Option<String>,
    pub session_id: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexPersistBatch {
    pub upserted_paths: Vec<PathBuf>,
    pub in_place_upserted_paths: Vec<PathBuf>,
    pub removed_paths: Vec<PathBuf>,
    pub history_snapshot: HistorySnapshot,
    pub history_delta: Option<HistoryPersistDelta>,
    pub outcome_snapshot: OutcomeMemorySnapshot,
    pub outcome_events: Vec<OutcomeEvent>,
    pub defer_graph_materialization: bool,
    pub co_change_deltas: Vec<CoChangeDelta>,
    pub validation_deltas: Vec<ValidationDelta>,
    pub projection_snapshot: Option<ProjectionSnapshot>,
    pub workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct AuxiliaryPersistBatch {
    pub outcome_snapshot: Option<OutcomeMemorySnapshot>,
    pub outcome_events: Vec<OutcomeEvent>,
    pub validation_deltas: Vec<ValidationDelta>,
    pub memory_events: Vec<MemoryEvent>,
    pub episodic_snapshot: Option<EpisodicMemorySnapshot>,
    pub inference_records: Vec<InferredEdgeRecord>,
    pub inference_snapshot: Option<InferenceSnapshot>,
    pub curator_snapshot: Option<CuratorSnapshot>,
}

#[derive(Debug, Clone)]
pub struct CoordinationPersistBatch {
    pub context: CoordinationPersistContext,
    pub expected_revision: Option<u64>,
    pub appended_events: Vec<CoordinationEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinationPersistResult {
    pub revision: u64,
    pub inserted_events: usize,
    pub applied: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoordinationEventStream {
    pub fallback_snapshot: Option<CoordinationSnapshot>,
    pub suffix_events: Vec<CoordinationEvent>,
}

pub trait Store {
    fn load_graph(&mut self) -> Result<Option<Graph>>;
    fn load_history_snapshot(&mut self) -> Result<Option<HistorySnapshot>>;
    fn load_history_snapshot_with_options(
        &mut self,
        _include_events: bool,
    ) -> Result<Option<HistorySnapshot>> {
        self.load_history_snapshot()
    }
    fn load_lineage_history(&mut self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        let Some(snapshot) = self.load_history_snapshot_with_options(true)? else {
            return Ok(Vec::new());
        };
        Ok(snapshot
            .events
            .into_iter()
            .filter(|event| &event.lineage == lineage)
            .collect())
    }
    fn save_history_snapshot(&mut self, snapshot: &HistorySnapshot) -> Result<()>;
    fn apply_history_delta(&mut self, delta: &HistoryPersistDelta) -> Result<()>;
    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &HistorySnapshot,
        deltas: &[CoChangeDelta],
    ) -> Result<()>;
    fn load_outcome_snapshot(&mut self) -> Result<Option<OutcomeMemorySnapshot>>;
    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<OutcomeMemorySnapshot>> {
        let Some(snapshot) = self.load_outcome_snapshot()? else {
            return Ok(None);
        };
        if limit == 0 || snapshot.events.len() <= limit {
            return Ok(Some(snapshot));
        }
        Ok(Some(OutcomeMemorySnapshot {
            events: snapshot.events.into_iter().take(limit).collect(),
        }))
    }
    fn load_outcomes(&mut self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        let Some(snapshot) = self.load_outcome_snapshot()? else {
            return Ok(Vec::new());
        };
        Ok(OutcomeMemory::from_snapshot(snapshot).query_events(query))
    }
    fn load_outcome_event(&mut self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Ok(self.load_outcome_snapshot()?.and_then(|snapshot| {
            snapshot
                .events
                .into_iter()
                .find(|event| event.meta.id == *event_id)
        }))
    }
    fn load_task_replay(&mut self, task_id: &TaskId) -> Result<TaskReplay> {
        Ok(TaskReplay {
            task: task_id.clone(),
            events: self.load_outcomes(&OutcomeRecallQuery {
                task: Some(task_id.clone()),
                limit: 0,
                ..OutcomeRecallQuery::default()
            })?,
        })
    }
    fn save_outcome_snapshot(&mut self, snapshot: &OutcomeMemorySnapshot) -> Result<()>;
    fn append_outcome_events(
        &mut self,
        events: &[OutcomeEvent],
        validation_deltas: &[ValidationDelta],
    ) -> Result<usize>;
    fn append_local_outcome_projection(&mut self, events: &[OutcomeEvent]) -> Result<usize> {
        self.append_outcome_events(events, &[])
    }
    fn apply_validation_deltas(&mut self, deltas: &[ValidationDelta]) -> Result<()>;
    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &OutcomeMemorySnapshot,
        deltas: &[ValidationDelta],
    ) -> Result<()>;
    fn load_memory_events(&mut self) -> Result<Vec<MemoryEvent>>;
    fn append_memory_events(&mut self, events: &[MemoryEvent]) -> Result<usize>;
    fn load_episodic_snapshot(&mut self) -> Result<Option<EpisodicMemorySnapshot>>;
    fn save_episodic_snapshot(&mut self, snapshot: &EpisodicMemorySnapshot) -> Result<()>;
    fn load_inference_snapshot(&mut self) -> Result<Option<InferenceSnapshot>>;
    fn save_inference_snapshot(&mut self, snapshot: &InferenceSnapshot) -> Result<()>;
    fn load_projection_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>>;
    fn load_projection_knowledge_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>> {
        Ok(self
            .load_projection_snapshot()?
            .map(|snapshot| ProjectionSnapshot {
                co_change_by_lineage: Vec::new(),
                validation_by_lineage: Vec::new(),
                curated_concepts: snapshot.curated_concepts,
                concept_relations: snapshot.concept_relations,
            }))
    }
    fn save_projection_snapshot(&mut self, snapshot: &ProjectionSnapshot) -> Result<()>;
    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[CoChangeDelta],
        validation_deltas: &[ValidationDelta],
    ) -> Result<()>;
    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>>;
    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()>;
    fn load_curator_snapshot(&mut self) -> Result<Option<CuratorSnapshot>>;
    fn save_curator_snapshot(&mut self, snapshot: &CuratorSnapshot) -> Result<()>;
    fn load_principal_registry_snapshot(&mut self) -> Result<Option<PrincipalRegistrySnapshot>>;
    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &PrincipalRegistrySnapshot,
    ) -> Result<()>;
    fn coordination_revision(&self) -> Result<u64>;
    fn load_coordination_events(&mut self) -> Result<Vec<CoordinationEvent>>;
    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream>;
    fn save_coordination_compaction(&mut self, snapshot: &CoordinationSnapshot) -> Result<()>;
    fn load_coordination_read_model(&mut self) -> Result<Option<CoordinationReadModel>>;
    fn save_coordination_read_model(&mut self, read_model: &CoordinationReadModel) -> Result<()>;
    fn load_coordination_queue_read_model(&mut self) -> Result<Option<CoordinationQueueReadModel>>;
    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &CoordinationQueueReadModel,
    ) -> Result<()>;
    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>>;
    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult>;
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

impl<T: Store + ?Sized> Store for MutexGuard<'_, T> {
    fn load_graph(&mut self) -> Result<Option<Graph>> {
        Store::load_graph(&mut **self)
    }

    fn load_history_snapshot(&mut self) -> Result<Option<HistorySnapshot>> {
        Store::load_history_snapshot(&mut **self)
    }

    fn load_history_snapshot_with_options(
        &mut self,
        include_events: bool,
    ) -> Result<Option<HistorySnapshot>> {
        Store::load_history_snapshot_with_options(&mut **self, include_events)
    }

    fn load_lineage_history(&mut self, lineage: &LineageId) -> Result<Vec<LineageEvent>> {
        Store::load_lineage_history(&mut **self, lineage)
    }

    fn save_history_snapshot(&mut self, snapshot: &HistorySnapshot) -> Result<()> {
        Store::save_history_snapshot(&mut **self, snapshot)
    }

    fn apply_history_delta(&mut self, delta: &HistoryPersistDelta) -> Result<()> {
        Store::apply_history_delta(&mut **self, delta)
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &HistorySnapshot,
        deltas: &[CoChangeDelta],
    ) -> Result<()> {
        Store::save_history_snapshot_with_co_change_deltas(&mut **self, snapshot, deltas)
    }

    fn load_outcome_snapshot(&mut self) -> Result<Option<OutcomeMemorySnapshot>> {
        Store::load_outcome_snapshot(&mut **self)
    }

    fn load_recent_outcome_snapshot(
        &mut self,
        limit: usize,
    ) -> Result<Option<OutcomeMemorySnapshot>> {
        Store::load_recent_outcome_snapshot(&mut **self, limit)
    }

    fn load_outcomes(&mut self, query: &OutcomeRecallQuery) -> Result<Vec<OutcomeEvent>> {
        Store::load_outcomes(&mut **self, query)
    }

    fn load_outcome_event(&mut self, event_id: &EventId) -> Result<Option<OutcomeEvent>> {
        Store::load_outcome_event(&mut **self, event_id)
    }

    fn load_task_replay(&mut self, task_id: &TaskId) -> Result<TaskReplay> {
        Store::load_task_replay(&mut **self, task_id)
    }

    fn save_outcome_snapshot(&mut self, snapshot: &OutcomeMemorySnapshot) -> Result<()> {
        Store::save_outcome_snapshot(&mut **self, snapshot)
    }

    fn append_outcome_events(
        &mut self,
        events: &[OutcomeEvent],
        validation_deltas: &[ValidationDelta],
    ) -> Result<usize> {
        Store::append_outcome_events(&mut **self, events, validation_deltas)
    }

    fn append_local_outcome_projection(&mut self, events: &[OutcomeEvent]) -> Result<usize> {
        Store::append_local_outcome_projection(&mut **self, events)
    }

    fn apply_validation_deltas(&mut self, deltas: &[ValidationDelta]) -> Result<()> {
        Store::apply_validation_deltas(&mut **self, deltas)
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &OutcomeMemorySnapshot,
        deltas: &[ValidationDelta],
    ) -> Result<()> {
        Store::save_outcome_snapshot_with_validation_deltas(&mut **self, snapshot, deltas)
    }

    fn load_memory_events(&mut self) -> Result<Vec<MemoryEvent>> {
        Store::load_memory_events(&mut **self)
    }

    fn append_memory_events(&mut self, events: &[MemoryEvent]) -> Result<usize> {
        Store::append_memory_events(&mut **self, events)
    }

    fn load_episodic_snapshot(&mut self) -> Result<Option<EpisodicMemorySnapshot>> {
        Store::load_episodic_snapshot(&mut **self)
    }

    fn save_episodic_snapshot(&mut self, snapshot: &EpisodicMemorySnapshot) -> Result<()> {
        Store::save_episodic_snapshot(&mut **self, snapshot)
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<InferenceSnapshot>> {
        Store::load_inference_snapshot(&mut **self)
    }

    fn save_inference_snapshot(&mut self, snapshot: &InferenceSnapshot) -> Result<()> {
        Store::save_inference_snapshot(&mut **self, snapshot)
    }

    fn load_projection_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>> {
        Store::load_projection_snapshot(&mut **self)
    }

    fn load_projection_knowledge_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>> {
        Store::load_projection_knowledge_snapshot(&mut **self)
    }

    fn save_projection_snapshot(&mut self, snapshot: &ProjectionSnapshot) -> Result<()> {
        Store::save_projection_snapshot(&mut **self, snapshot)
    }

    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[CoChangeDelta],
        validation_deltas: &[ValidationDelta],
    ) -> Result<()> {
        Store::apply_projection_deltas(&mut **self, co_change_deltas, validation_deltas)
    }

    fn load_workspace_tree_snapshot(&mut self) -> Result<Option<WorkspaceTreeSnapshot>> {
        Store::load_workspace_tree_snapshot(&mut **self)
    }

    fn save_workspace_tree_snapshot(&mut self, snapshot: &WorkspaceTreeSnapshot) -> Result<()> {
        Store::save_workspace_tree_snapshot(&mut **self, snapshot)
    }

    fn load_curator_snapshot(&mut self) -> Result<Option<CuratorSnapshot>> {
        Store::load_curator_snapshot(&mut **self)
    }

    fn save_curator_snapshot(&mut self, snapshot: &CuratorSnapshot) -> Result<()> {
        Store::save_curator_snapshot(&mut **self, snapshot)
    }

    fn load_principal_registry_snapshot(&mut self) -> Result<Option<PrincipalRegistrySnapshot>> {
        Store::load_principal_registry_snapshot(&mut **self)
    }

    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &PrincipalRegistrySnapshot,
    ) -> Result<()> {
        Store::save_principal_registry_snapshot(&mut **self, snapshot)
    }

    fn coordination_revision(&self) -> Result<u64> {
        Store::coordination_revision(&**self)
    }

    fn load_coordination_events(&mut self) -> Result<Vec<CoordinationEvent>> {
        Store::load_coordination_events(&mut **self)
    }

    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream> {
        Store::load_coordination_event_stream(&mut **self)
    }

    fn save_coordination_compaction(&mut self, snapshot: &CoordinationSnapshot) -> Result<()> {
        Store::save_coordination_compaction(&mut **self, snapshot)
    }

    fn load_coordination_read_model(&mut self) -> Result<Option<CoordinationReadModel>> {
        Store::load_coordination_read_model(&mut **self)
    }

    fn save_coordination_read_model(&mut self, read_model: &CoordinationReadModel) -> Result<()> {
        Store::save_coordination_read_model(&mut **self, read_model)
    }

    fn load_coordination_queue_read_model(&mut self) -> Result<Option<CoordinationQueueReadModel>> {
        Store::load_coordination_queue_read_model(&mut **self)
    }

    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &CoordinationQueueReadModel,
    ) -> Result<()> {
        Store::save_coordination_queue_read_model(&mut **self, read_model)
    }

    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>> {
        Store::load_latest_coordination_persist_context(&mut **self)
    }

    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult> {
        Store::commit_coordination_persist_batch(&mut **self, batch)
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        Store::commit_auxiliary_persist_batch(&mut **self, batch)
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()> {
        Store::commit_index_persist_batch(&mut **self, graph, batch)
    }

    fn save_graph_snapshot(&mut self, graph: &Graph) -> Result<()> {
        Store::save_graph_snapshot(&mut **self, graph)
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        Store::save_file_state(&mut **self, path, graph)
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        Store::remove_file_state(&mut **self, path)
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        Store::replace_derived_edges(&mut **self, graph)
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        Store::finalize(&mut **self, graph)
    }
}
