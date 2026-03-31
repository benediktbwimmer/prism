use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_agent::{InferenceSnapshot, InferredEdgeRecord};
use prism_coordination::{
    CoordinationEvent, CoordinationQueueReadModel, CoordinationReadModel, CoordinationSnapshot,
};
use prism_curator::CuratorSnapshot;
use prism_history::{HistoryPersistDelta, HistorySnapshot};
use prism_ir::{EventId, LineageEvent, LineageId, TaskId};
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
    pub removed_paths: Vec<PathBuf>,
    pub history_snapshot: HistorySnapshot,
    pub history_delta: Option<HistoryPersistDelta>,
    pub outcome_snapshot: OutcomeMemorySnapshot,
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
