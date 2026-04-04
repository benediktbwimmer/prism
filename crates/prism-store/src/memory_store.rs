use anyhow::{anyhow, Result};
use prism_coordination::{CoordinationEvent, CoordinationQueueReadModel, CoordinationReadModel};
use prism_projections::ProjectionIndex;

use crate::graph::{Graph, GraphSnapshot};
use crate::memory_projection::{append_only_delta, merge_snapshot, snapshot_from_events};
use crate::outcome_projection::{
    append_only_delta as outcome_append_only_delta, merge_snapshot as merge_outcome_snapshot,
    snapshot_from_events as outcome_snapshot_from_events,
};
use crate::store::{
    AuxiliaryPersistBatch, CoordinationEventStream, CoordinationPersistBatch,
    CoordinationPersistContext, CoordinationPersistResult, IndexPersistBatch, Store,
    WorkspaceTreeSnapshot,
};
use crate::CoordinationStartupCheckpoint;
use prism_memory::{MemoryEvent, MemoryEventKind, OutcomeEvent};

#[derive(Debug, Default)]
pub struct MemoryStore {
    snapshot: Option<GraphSnapshot>,
    history_snapshot: Option<prism_history::HistorySnapshot>,
    outcome_snapshot: Option<prism_memory::OutcomeMemorySnapshot>,
    outcome_events: Vec<OutcomeEvent>,
    memory_events: Vec<MemoryEvent>,
    episodic_snapshot: Option<prism_memory::EpisodicMemorySnapshot>,
    inference_snapshot: Option<prism_agent::InferenceSnapshot>,
    projection_snapshot: Option<prism_projections::ProjectionSnapshot>,
    workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
    curator_snapshot: Option<prism_curator::CuratorSnapshot>,
    principal_registry_snapshot: Option<prism_ir::PrincipalRegistrySnapshot>,
    coordination_events: Vec<CoordinationEvent>,
    coordination_compaction: Option<(usize, prism_coordination::CoordinationSnapshot)>,
    coordination_startup_checkpoint: Option<CoordinationStartupCheckpoint>,
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

    fn load_history_snapshot_with_options(
        &mut self,
        _include_co_change: bool,
    ) -> anyhow::Result<Option<prism_history::HistorySnapshot>> {
        Ok(self.history_snapshot.clone())
    }

    fn save_history_snapshot(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
    ) -> anyhow::Result<()> {
        self.history_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn apply_history_delta(
        &mut self,
        delta: &prism_history::HistoryPersistDelta,
    ) -> anyhow::Result<()> {
        let mut history =
            prism_history::HistoryStore::from_snapshot(self.history_snapshot.clone().unwrap_or(
                prism_history::HistorySnapshot {
                    node_to_lineage: Vec::new(),
                    events: Vec::new(),
                    tombstones: Vec::new(),
                    next_lineage: 0,
                    next_event: 0,
                },
            ));
        history.apply_persistence_delta(delta);
        self.history_snapshot = Some(history.snapshot());
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
        if !self.outcome_events.is_empty() {
            self.outcome_snapshot = outcome_snapshot_from_events(self.outcome_events.clone());
        }
        Ok(self.outcome_snapshot.clone())
    }

    fn save_outcome_snapshot(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
    ) -> anyhow::Result<()> {
        let current = self.outcome_snapshot.as_ref();
        self.outcome_events
            .extend(outcome_append_only_delta(current, snapshot));
        self.outcome_snapshot = merge_outcome_snapshot(self.outcome_snapshot.clone(), snapshot);
        Ok(())
    }

    fn append_outcome_events(
        &mut self,
        events: &[prism_memory::OutcomeEvent],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> anyhow::Result<usize> {
        let mut inserted = 0;
        for event in events {
            if self
                .outcome_events
                .iter()
                .any(|existing| existing.meta.id == event.meta.id)
            {
                continue;
            }
            self.outcome_events.push(event.clone());
            inserted += 1;
        }
        if inserted > 0 {
            self.outcome_snapshot = outcome_snapshot_from_events(self.outcome_events.clone());
        }
        if !validation_deltas.is_empty() {
            let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
            let mut index = ProjectionIndex::from_snapshot(snapshot);
            index.apply_validation_deltas(validation_deltas);
            snapshot = index.snapshot();
            self.projection_snapshot = Some(snapshot);
        }
        Ok(inserted)
    }

    fn apply_validation_deltas(
        &mut self,
        deltas: &[prism_projections::ValidationDelta],
    ) -> anyhow::Result<()> {
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

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> anyhow::Result<()> {
        self.save_outcome_snapshot(snapshot)?;
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

    fn load_projection_knowledge_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_projections::ProjectionSnapshot>> {
        Ok(self
            .projection_snapshot
            .clone()
            .map(|snapshot| prism_projections::ProjectionSnapshot {
                co_change_by_lineage: Vec::new(),
                validation_by_lineage: Vec::new(),
                curated_concepts: snapshot.curated_concepts,
                concept_relations: snapshot.concept_relations,
            }))
    }

    fn load_projection_snapshot_without_co_change(
        &mut self,
    ) -> anyhow::Result<Option<prism_projections::ProjectionSnapshot>> {
        Ok(self
            .projection_snapshot
            .clone()
            .map(|snapshot| prism_projections::ProjectionSnapshot {
                co_change_by_lineage: Vec::new(),
                validation_by_lineage: snapshot.validation_by_lineage,
                curated_concepts: snapshot.curated_concepts,
                concept_relations: snapshot.concept_relations,
            }))
    }

    fn has_derived_projection_snapshot(&mut self) -> anyhow::Result<bool> {
        Ok(self.projection_snapshot.as_ref().is_some_and(|snapshot| {
            !snapshot.co_change_by_lineage.is_empty() || !snapshot.validation_by_lineage.is_empty()
        }))
    }

    fn save_projection_snapshot(
        &mut self,
        snapshot: &prism_projections::ProjectionSnapshot,
    ) -> anyhow::Result<()> {
        self.projection_snapshot = Some(snapshot.clone());
        Ok(())
    }

    fn apply_projection_deltas(
        &mut self,
        co_change_deltas: &[prism_projections::CoChangeDelta],
        validation_deltas: &[prism_projections::ValidationDelta],
    ) -> anyhow::Result<()> {
        if co_change_deltas.is_empty() && validation_deltas.is_empty() {
            return Ok(());
        }
        let mut snapshot = self.projection_snapshot.clone().unwrap_or_default();
        let mut index = ProjectionIndex::from_snapshot(snapshot);
        index.apply_co_change_deltas(co_change_deltas);
        index.apply_validation_deltas(validation_deltas);
        snapshot = index.snapshot();
        self.projection_snapshot = Some(snapshot);
        Ok(())
    }

    fn load_workspace_tree_snapshot(&mut self) -> anyhow::Result<Option<WorkspaceTreeSnapshot>> {
        Ok(self.workspace_tree_snapshot.clone())
    }

    fn save_workspace_tree_snapshot(
        &mut self,
        snapshot: &WorkspaceTreeSnapshot,
    ) -> anyhow::Result<()> {
        self.workspace_tree_snapshot = Some(snapshot.clone());
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

    fn load_principal_registry_snapshot(
        &mut self,
    ) -> anyhow::Result<Option<prism_ir::PrincipalRegistrySnapshot>> {
        Ok(self.principal_registry_snapshot.clone())
    }

    fn save_principal_registry_snapshot(
        &mut self,
        snapshot: &prism_ir::PrincipalRegistrySnapshot,
    ) -> anyhow::Result<()> {
        self.principal_registry_snapshot = Some(snapshot.clone());
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

    fn load_coordination_startup_checkpoint(
        &mut self,
    ) -> Result<Option<CoordinationStartupCheckpoint>> {
        Ok(self.coordination_startup_checkpoint.clone())
    }

    fn save_coordination_startup_checkpoint(
        &mut self,
        checkpoint: &CoordinationStartupCheckpoint,
    ) -> Result<()> {
        self.coordination_startup_checkpoint = Some(checkpoint.clone());
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
        for event in &batch.outcome_events {
            if self
                .outcome_events
                .iter()
                .any(|existing| existing.meta.id == event.meta.id)
            {
                continue;
            }
            self.outcome_events.push(event.clone());
        }
        if !batch.outcome_events.is_empty() {
            self.outcome_snapshot = outcome_snapshot_from_events(self.outcome_events.clone());
        }
        if let Some(snapshot) = &batch.outcome_snapshot {
            self.save_outcome_snapshot(snapshot)?;
        }
        for event in &batch.memory_events {
            if self
                .memory_events
                .iter()
                .any(|existing| existing.id == event.id)
            {
                continue;
            }
            self.memory_events.push(event.clone());
        }
        if !batch.memory_events.is_empty() {
            self.episodic_snapshot = snapshot_from_events(self.memory_events.clone());
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
        if !batch.inference_records.is_empty() {
            let mut records = self
                .inference_snapshot
                .clone()
                .unwrap_or_default()
                .records
                .into_iter()
                .map(|record| (record.id.0.clone(), record))
                .collect::<std::collections::BTreeMap<_, _>>();
            for record in &batch.inference_records {
                records
                    .entry(record.id.0.clone())
                    .or_insert_with(|| record.clone());
            }
            self.inference_snapshot = Some(prism_agent::InferenceSnapshot {
                records: records.into_values().collect(),
            });
        }
        if let Some(snapshot) = &batch.inference_snapshot {
            self.save_inference_snapshot(snapshot)?;
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
        if let Some(delta) = &batch.history_delta {
            self.apply_history_delta(delta)?;
        } else if let Some(snapshot) = &batch.history_snapshot {
            self.history_snapshot = Some(snapshot.clone());
        }
        if !batch.outcome_events.is_empty() {
            let _ = self.append_outcome_events(&batch.outcome_events, &[])?;
        } else if let Some(snapshot) = &batch.outcome_snapshot {
            self.save_outcome_snapshot(snapshot)?;
        }
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
        if let Some(snapshot) = &batch.workspace_tree_snapshot {
            self.workspace_tree_snapshot = Some(snapshot.clone());
        }
        Ok(())
    }

    fn save_graph_snapshot(&mut self, graph: &Graph) -> anyhow::Result<()> {
        self.snapshot = Some(graph.snapshot());
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
