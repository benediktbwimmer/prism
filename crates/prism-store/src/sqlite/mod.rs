mod codecs;
mod graph_io;
mod projections;
mod schema;
mod snapshots;

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::graph::Graph;
use crate::store::{AuxiliaryPersistBatch, IndexPersistBatch, Store};

pub struct SqliteStore {
    pub(crate) conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        schema::init_schema(&conn)?;
        Ok(Self { conn })
    }
}

impl Store for SqliteStore {
    fn load_graph(&mut self) -> Result<Option<Graph>> {
        graph_io::load_graph(&self.conn)
    }

    fn load_history_snapshot(&mut self) -> Result<Option<prism_history::HistorySnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "history")
    }

    fn save_history_snapshot(&mut self, snapshot: &prism_history::HistorySnapshot) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "history", snapshot)
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
        deltas: &[prism_projections::CoChangeDelta],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "history", snapshot)?;
        projections::apply_projection_co_change_deltas_tx(&tx, deltas)?;
        tx.commit()?;
        Ok(())
    }

    fn load_outcome_snapshot(&mut self) -> Result<Option<prism_memory::OutcomeMemorySnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "outcomes")
    }

    fn save_outcome_snapshot(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
    ) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "outcomes", snapshot)
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "outcomes", snapshot)?;
        projections::apply_projection_validation_deltas_tx(&tx, deltas)?;
        tx.commit()?;
        Ok(())
    }

    fn load_episodic_snapshot(&mut self) -> Result<Option<prism_memory::EpisodicMemorySnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "episodic")
    }

    fn save_episodic_snapshot(
        &mut self,
        snapshot: &prism_memory::EpisodicMemorySnapshot,
    ) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "episodic", snapshot)
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "inference")
    }

    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "inference", snapshot)
    }

    fn load_projection_snapshot(
        &mut self,
    ) -> Result<Option<prism_projections::ProjectionSnapshot>> {
        projections::load_projection_snapshot_rows(&self.conn)
    }

    fn save_projection_snapshot(
        &mut self,
        snapshot: &prism_projections::ProjectionSnapshot,
    ) -> Result<()> {
        projections::save_projection_snapshot_rows(&mut self.conn, snapshot)
    }

    fn load_curator_snapshot(&mut self) -> Result<Option<prism_curator::CuratorSnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "curator")
    }

    fn save_curator_snapshot(&mut self, snapshot: &prism_curator::CuratorSnapshot) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "curator", snapshot)
    }

    fn load_coordination_snapshot(
        &mut self,
    ) -> Result<Option<prism_coordination::CoordinationSnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "coordination")
    }

    fn save_coordination_snapshot(
        &mut self,
        snapshot: &prism_coordination::CoordinationSnapshot,
    ) -> Result<()> {
        snapshots::save_snapshot_row(&self.conn, "coordination", snapshot)
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        let tx = self.conn.transaction()?;
        if let Some(snapshot) = &batch.outcome_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "outcomes", snapshot)?;
        }
        if let Some(snapshot) = &batch.episodic_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "episodic", snapshot)?;
        }
        if let Some(snapshot) = &batch.inference_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "inference", snapshot)?;
        }
        if let Some(snapshot) = &batch.curator_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "curator", snapshot)?;
        }
        if let Some(snapshot) = &batch.coordination_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "coordination", snapshot)?;
        }
        projections::apply_projection_validation_deltas_tx(&tx, &batch.validation_deltas)?;
        tx.commit()?;
        Ok(())
    }

    fn commit_index_persist_batch(
        &mut self,
        graph: &Graph,
        batch: &IndexPersistBatch,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;

        for path in &batch.removed_paths {
            graph_io::delete_file_state(&tx, path)?;
        }

        for path in &batch.upserted_paths {
            let Some(state) = graph.file_state(path) else {
                continue;
            };
            graph_io::save_file_state_tx(&tx, &state)?;
        }

        graph_io::replace_derived_edges_tx(&tx, graph)?;
        graph_io::finalize_tx(&tx, graph)?;
        snapshots::save_snapshot_row_tx(&tx, "history", &batch.history_snapshot)?;
        snapshots::save_snapshot_row_tx(&tx, "outcomes", &batch.outcome_snapshot)?;

        if let Some(snapshot) = &batch.projection_snapshot {
            projections::save_projection_snapshot_tx(&tx, snapshot)?;
        } else {
            projections::apply_projection_co_change_deltas_tx(&tx, &batch.co_change_deltas)?;
            projections::apply_projection_validation_deltas_tx(&tx, &batch.validation_deltas)?;
        }

        tx.commit()?;
        Ok(())
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        let Some(state) = graph.file_state(path) else {
            return Ok(());
        };
        let tx = self.conn.transaction()?;
        graph_io::save_file_state_tx(&tx, &state)?;
        tx.commit()?;
        Ok(())
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::delete_file_state(&tx, path)?;
        tx.commit()?;
        Ok(())
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::replace_derived_edges_tx(&tx, graph)?;
        tx.commit()?;
        Ok(())
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::finalize_tx(&tx, graph)?;
        tx.commit()?;
        Ok(())
    }
}
