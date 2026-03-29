use anyhow::{anyhow, Result};
use prism_coordination::{CoordinationEvent, CoordinationQueueReadModel, CoordinationReadModel};
use prism_projections::ProjectionIndex;

use crate::graph::{Graph, GraphSnapshot};
use crate::memory_projection::{append_only_delta, merge_snapshot, snapshot_from_events};
use crate::store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationPersistBatch,
    CoordinationPersistContext, CoordinationPersistResult, IndexPersistBatch, Store,
};
use prism_memory::{MemoryEvent, MemoryEventKind};

#[derive(Debug, Default)]
pub struct MemoryStore {
    snapshot: Option<GraphSnapshot>,
    history_snapshot: Option<prism_history::HistorySnapshot>,
    outcome_snapshot: Option<prism_memory::OutcomeMemorySnapshot>,
    memory_events: Vec<MemoryEvent>,
    episodic_snapshot: Option<prism_memory::EpisodicMemorySnapshot>,
    inference_snapshot: Option<prism_agent::InferenceSnapshot>,
    projection_snapshot: Option<prism_projections::ProjectionSnapshot>,
    curator_snapshot: Option<prism_curator::CuratorSnapshot>,
    coordination_events: Vec<CoordinationEvent>,
    coordination_compaction: Option<(usize, prism_coordination::CoordinationSnapshot)>,
    coordination_read_model: Option<CoordinationReadModel>,
    coordination_queue_read_model: Option<CoordinationQueueReadModel>,
    coordination_revision: u64,
    latest_coordination_context: Option<CoordinationPersistContext>,
}

impl Store for MemoryStore {
    fn load_graph(&mut self) -> anyhow::Result<Option<Graph>> {
        Ok(self.snapshot.clone().map(Graph::from_snapshot))
    }

    fn load_history_snapshot(&mut self) -> anyhow::Result<Option<prism_history::HistorySnapshot>> {
        Ok(self.history_snapshot.clone())
    }

    fn save_history_snapshot(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
    ) -> anyhow::Result<()> {
        self.history_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
        deltas: &[prism_projections::CoChangeDelta],
    ) -> anyhow::Result<()> {
        self.history_snapshot = Some(snapshot.clone());
        if deltas.is_empty() {
            return Ok(());
        }
        let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
        let mut index = ProjectionIndex::from_snapshot(snapshot);
        index.apply_co_change_deltas(deltas);
        snapshot = index.snapshot();
        self.projection_snapshot = Some(snapshot);
        Ok(())
    }

    fn load_outcome_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        Ok(self.outcome_snapshot.clone())
    }

    fn save_outcome_snapshot(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
    ) -> anyhow::Result<()> {
        self.outcome_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> anyhow::Result<()> {
        self.outcome_snapshot = Some(snapshot.clone());
        if deltas.is_empty() {
            return Ok(());
        }
        let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
        let mut index = ProjectionIndex::from_snapshot(snapshot);
        index.apply_validation_deltas(deltas);
        snapshot = index.snapshot();
        self.projection_snapshot = Some(snapshot);
        Ok(())
    }

    fn load_memory_events(&mut self) -> anyhow::Result<Vec<MemoryEvent>> {
        Ok(self.memory_events.clone())
    }

    fn append_memory_events(&mut self, events: &[MemoryEvent]) -> anyhow::Result<usize> {
        let mut inserted = 0;
        for event in events {
            if self
                .memory_events
                .iter()
                .any(|existing| existing.id == event.id)
            {
                continue;
            }
            self.memory_events.push(event.clone());
            inserted += 1;
        }
        if inserted > 0 {
            self.episodic_snapshot = snapshot_from_events(self.memory_events.clone());
        }
        Ok(inserted)
    }

    fn load_episodic_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_memory::EpisodicMemorySnapshot>> {
        if !self.memory_events.is_empty() {
            self.episodic_snapshot = snapshot_from_events(self.memory_events.clone());
        }
        Ok(self.episodic_snapshot.clone())
    }

    fn save_episodic_snapshot(
        &mut self,
        snapshot: &prism_memory::EpisodicMemorySnapshot,
    ) -> anyhow::Result<()> {
        let current = self.episodic_snapshot.as_ref();
        for entry in append_only_delta(current, snapshot) {
            self.memory_events.push(MemoryEvent::from_entry(
                MemoryEventKind::Stored,
                entry,
                None,
                Vec::new(),
                Vec::new(),
            ));
        }
        self.episodic_snapshot = merge_snapshot(self.episodic_snapshot.clone(), snapshot);
        Ok(())
    }

    fn load_inference_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_agent::InferenceSnapshot>> {
        Ok(self.inference_snapshot.clone())
    }

    fn save_inference_snapshot(
        &mut self,
        snapshot: &prism_agent::InferenceSnapshot,
    ) -> anyhow::Result<()> {
        self.inference_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn load_projection_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_projections::ProjectionSnapshot>> {
        Ok(self.projection_snapshot.clone())
    }

    fn save_projection_snapshot(
        &mut self,
        snapshot: &prism_projections::ProjectionSnapshot,
    ) -> anyhow::Result<()> {
        self.projection_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn load_curator_snapshot(&mut self) -> anyhow::Result<Option<prism_curator::CuratorSnapshot>> {
        Ok(self.curator_snapshot.clone())
    }

    fn save_curator_snapshot(
        &mut self,
        snapshot: &prism_curator::CuratorSnapshot,
    ) -> anyhow::Result<()> {
        self.curator_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn coordination_revision(&self) -> Result<u64> {
        Ok(self.coordination_revision)
    }

    fn load_coordination_events(&mut self) -> Result<Vec<CoordinationEvent>> {
        Ok(self.coordination_events.clone())
    }

    fn load_coordination_event_stream(&mut self) -> Result<CoordinationEventStream> {
        let Some((compacted_events, snapshot)) = self.coordination_compaction.clone() else {
            return Ok(CoordinationEventStream {
                fallback_snapshot: None,
                suffix_events: self.coordination_events.clone(),
            });
        };
        Ok(CoordinationEventStream {
            fallback_snapshot: Some(snapshot),
            suffix_events: self.coordination_events[compacted_events..].to_vec(),
        })
    }

    fn save_coordination_compaction(
        &mut self,
        snapshot: &prism_coordination::CoordinationSnapshot,
    ) -> Result<()> {
        self.coordination_compaction = Some((
            self.coordination_events.len(),
            compacted_snapshot(snapshot.clone()),
        ));
        Ok(())
    }

    fn load_coordination_read_model(&mut self) -> Result<Option<CoordinationReadModel>> {
        Ok(self.coordination_read_model.clone())
    }

    fn save_coordination_read_model(&mut self, read_model: &CoordinationReadModel) -> Result<()> {
        self.coordination_read_model = Some(read_model.clone());
        Ok(())
    }

    fn load_coordination_queue_read_model(&mut self) -> Result<Option<CoordinationQueueReadModel>> {
        Ok(self.coordination_queue_read_model.clone())
    }

    fn save_coordination_queue_read_model(
        &mut self,
        read_model: &CoordinationQueueReadModel,
    ) -> Result<()> {
        self.coordination_queue_read_model = Some(read_model.clone());
        Ok(())
    }

    fn load_latest_coordination_persist_context(
        &mut self,
    ) -> Result<Option<CoordinationPersistContext>> {
        Ok(self.latest_coordination_context.clone())
    }

    fn commit_coordination_persist_batch(
        &mut self,
        batch: &CoordinationPersistBatch,
    ) -> Result<CoordinationPersistResult> {
        let current_revision = self.coordination_revision;
        if let Some(expected_revision) = batch.expected_revision {
            if expected_revision != current_revision {
                if !batch.appended_events.is_empty()
                    && batch.appended_events.iter().all(|event| {
                        self.coordination_events
                            .iter()
                            .any(|stored| stored.meta.id == event.meta.id)
                    })
                {
                    return Ok(CoordinationPersistResult {
                        revision: current_revision,
                        inserted_events: 0,
                        applied: false,
                    });
                }
                return Err(anyhow!(
                    "coordination revision mismatch: expected {}, found {}",
                    expected_revision,
                    current_revision
                ));
            }
        }

        let mut inserted_events = 0;
        for event in &batch.appended_events {
            if self
                .coordination_events
                .iter()
                .any(|stored| stored.meta.id == event.meta.id)
            {
                continue;
            }
            self.coordination_events.push(event.clone());
            inserted_events += 1;
        }

        if inserted_events == 0 {
            self.latest_coordination_context = Some(batch.context.clone());
            return Ok(CoordinationPersistResult {
                revision: current_revision,
                inserted_events,
                applied: false,
            });
        }

        self.latest_coordination_context = Some(batch.context.clone());
        self.coordination_revision += 1;
        Ok(CoordinationPersistResult {
            revision: self.coordination_revision,
            inserted_events,
            applied: true,
        })
    }

    fn commit_auxiliary_persist_batch(
        &mut self,
        batch: &AuxiliaryPersistBatch,
    ) -> anyhow::Result<()> {
        if let Some(snapshot) = &batch.outcome_snapshot {
            self.outcome_snapshot = Some(snapshot.clone());
        }
        if let Some(snapshot) = &batch.episodic_snapshot {
            for entry in append_only_delta(self.episodic_snapshot.as_ref(), snapshot) {
                self.memory_events.push(MemoryEvent::from_entry(
                    MemoryEventKind::Stored,
                    entry,
                    None,
                    Vec::new(),
                    Vec::new(),
                ));
            }
            self.episodic_snapshot = merge_snapshot(self.episodic_snapshot.clone(), snapshot);
        }
        if let Some(snapshot) = &batch.inference_snapshot {
            self.inference_snapshot = Some(snapshot.clone());
        }
        if let Some(snapshot) = &batch.curator_snapshot {
            self.curator_snapshot = Some(snapshot.clone());
        }
        if !batch.validation_deltas.is_empty() {
            let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
            let mut index = ProjectionIndex::from_snapshot(snapshot);
            index.apply_validation_deltas(&batch.validation_deltas);
            snapshot = index.snapshot();
            self.projection_snapshot = Some(snapshot);
        }
        Ok(())
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> anyhow::Result<()> {
        self.snapshot = Some(graph.snapshot());
        self.history_snapshot = Some(batch.history_snapshot.clone());
        self.outcome_snapshot = Some(batch.outcome_snapshot.clone());
        if let Some(snapshot) = &batch.projection_snapshot {
            self.projection_snapshot = Some(snapshot.clone());
        } else if !batch.co_change_deltas.is_empty() || !batch.validation_deltas.is_empty() {
            let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
            let mut index = ProjectionIndex::from_snapshot(snapshot);
            index.apply_co_change_deltas(&batch.co_change_deltas);
            index.apply_validation_deltas(&batch.validation_deltas);
            snapshot = index.snapshot();
            self.projection_snapshot = Some(snapshot);
        }
        Ok(())
    }

    fn save_file_state(&mut self, _path: &std::path::Path, graph: &Graph) -> anyhow::Result<()> {
        self.snapshot = Some(graph.snapshot());
        Ok(())
    }

    fn remove_file_state(&mut self, _path: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> anyhow::Result<()> {
        self.snapshot = Some(graph.snapshot());
        Ok(())
    }

    fn finalize(&mut self, graph: &Graph) -> anyhow::Result<()> {
        self.snapshot = Some(graph.snapshot());
        Ok(())
    }
}

fn compacted_snapshot(
    mut snapshot: prism_coordination::CoordinationSnapshot,
) -> prism_coordination::CoordinationSnapshot {
    snapshot.events.clear();
    snapshot
}
