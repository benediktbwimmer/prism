use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_agent::InferenceSnapshot;
use prism_coordination::CoordinationSnapshot;
use prism_curator::CuratorSnapshot;
use prism_history::HistorySnapshot;
use prism_memory::{EpisodicMemorySnapshot, OutcomeMemorySnapshot};
use prism_projections::{CoChangeDelta, ProjectionSnapshot, ValidationDelta};

use crate::graph::Graph;

#[derive(Debug, Clone)]
pub struct IndexPersistBatch {
    pub upserted_paths: Vec<PathBuf>,
    pub removed_paths: Vec<PathBuf>,
    pub history_snapshot: HistorySnapshot,
    pub outcome_snapshot: OutcomeMemorySnapshot,
    pub co_change_deltas: Vec<CoChangeDelta>,
    pub validation_deltas: Vec<ValidationDelta>,
    pub projection_snapshot: Option<ProjectionSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct AuxiliaryPersistBatch {
    pub outcome_snapshot: Option<OutcomeMemorySnapshot>,
    pub validation_deltas: Vec<ValidationDelta>,
    pub episodic_snapshot: Option<EpisodicMemorySnapshot>,
    pub inference_snapshot: Option<InferenceSnapshot>,
    pub curator_snapshot: Option<CuratorSnapshot>,
    pub coordination_snapshot: Option<CoordinationSnapshot>,
}

pub trait Store {
    fn load_graph(&mut self) -> Result<Option<Graph>>;
    fn load_history_snapshot(&mut self) -> Result<Option<HistorySnapshot>>;
    fn save_history_snapshot(&mut self, snapshot: &HistorySnapshot) -> Result<()>;
    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &HistorySnapshot,
        deltas: &[CoChangeDelta],
    ) -> Result<()>;
    fn load_outcome_snapshot(&mut self) -> Result<Option<OutcomeMemorySnapshot>>;
    fn save_outcome_snapshot(&mut self, snapshot: &OutcomeMemorySnapshot) -> Result<()>;
    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &OutcomeMemorySnapshot,
        deltas: &[ValidationDelta],
    ) -> Result<()>;
    fn load_episodic_snapshot(&mut self) -> Result<Option<EpisodicMemorySnapshot>>;
    fn save_episodic_snapshot(&mut self, snapshot: &EpisodicMemorySnapshot) -> Result<()>;
    fn load_inference_snapshot(&mut self) -> Result<Option<InferenceSnapshot>>;
    fn save_inference_snapshot(&mut self, snapshot: &InferenceSnapshot) -> Result<()>;
    fn load_projection_snapshot(&mut self) -> Result<Option<ProjectionSnapshot>>;
    fn save_projection_snapshot(&mut self, snapshot: &ProjectionSnapshot) -> Result<()>;
    fn load_curator_snapshot(&mut self) -> Result<Option<CuratorSnapshot>>;
    fn save_curator_snapshot(&mut self, snapshot: &CuratorSnapshot) -> Result<()>;
    fn load_coordination_snapshot(&mut self) -> Result<Option<CoordinationSnapshot>>;
    fn save_coordination_snapshot(&mut self, snapshot: &CoordinationSnapshot) -> Result<()>;
    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()>;
    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()>;
    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()>;
    fn remove_file_state(&mut self, path: &Path) -> Result<()>;
    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()>;
    fn finalize(&mut self, graph: &Graph) -> Result<()>;
}
