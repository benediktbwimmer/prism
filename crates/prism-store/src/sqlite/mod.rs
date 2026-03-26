mod codecs;
mod graph_io;
mod projections;
mod schema;
mod snapshots;

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::graph::Graph;
use crate::store::{AuxiliaryPersistBatch, IndexPersistBatch, Store};

const WORKSPACE_REVISION_KEY: &str = "revision:workspace";
const EPISODIC_REVISION_KEY: &str = "revision:episodic";
const INFERENCE_REVISION_KEY: &str = "revision:inference";
const COORDINATION_REVISION_KEY: &str = "revision:coordination";

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
        configure_connection(&conn)?;
        schema::init_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn episodic_revision(&self) -> Result<u64> {
        metadata_value(&self.conn, EPISODIC_REVISION_KEY)
    }

    pub fn inference_revision(&self) -> Result<u64> {
        metadata_value(&self.conn, INFERENCE_REVISION_KEY)
    }

    pub fn workspace_revision(&self) -> Result<u64> {
        metadata_value(&self.conn, WORKSPACE_REVISION_KEY)
    }

    pub fn coordination_revision(&self) -> Result<u64> {
        metadata_value(&self.conn, COORDINATION_REVISION_KEY)
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
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "history", snapshot)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn save_history_snapshot_with_co_change_deltas(
        &mut self,
        snapshot: &prism_history::HistorySnapshot,
        deltas: &[prism_projections::CoChangeDelta],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "history", snapshot)?;
        projections::apply_projection_co_change_deltas_tx(&tx, deltas)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
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
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "outcomes", snapshot)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn save_outcome_snapshot_with_validation_deltas(
        &mut self,
        snapshot: &prism_memory::OutcomeMemorySnapshot,
        deltas: &[prism_projections::ValidationDelta],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "outcomes", snapshot)?;
        projections::apply_projection_validation_deltas_tx(&tx, deltas)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
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
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "episodic", snapshot)?;
        bump_metadata_value_tx(&tx, EPISODIC_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn load_inference_snapshot(&mut self) -> Result<Option<prism_agent::InferenceSnapshot>> {
        snapshots::load_snapshot_row(&self.conn, "inference")
    }

    fn save_inference_snapshot(&mut self, snapshot: &prism_agent::InferenceSnapshot) -> Result<()> {
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "inference", snapshot)?;
        bump_metadata_value_tx(&tx, INFERENCE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
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
        let tx = self.conn.transaction()?;
        projections::save_projection_snapshot_tx(&tx, snapshot)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
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
        let tx = self.conn.transaction()?;
        snapshots::save_snapshot_row_tx(&tx, "coordination", snapshot)?;
        bump_metadata_value_tx(&tx, COORDINATION_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn commit_auxiliary_persist_batch(&mut self, batch: &AuxiliaryPersistBatch) -> Result<()> {
        let tx = self.conn.transaction()?;
        let mut workspace_changed = false;
        if let Some(snapshot) = &batch.outcome_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "outcomes", snapshot)?;
            workspace_changed = true;
        }
        if let Some(snapshot) = &batch.episodic_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "episodic", snapshot)?;
            bump_metadata_value_tx(&tx, EPISODIC_REVISION_KEY)?;
        }
        if let Some(snapshot) = &batch.inference_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "inference", snapshot)?;
            bump_metadata_value_tx(&tx, INFERENCE_REVISION_KEY)?;
        }
        if let Some(snapshot) = &batch.curator_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "curator", snapshot)?;
        }
        if let Some(snapshot) = &batch.coordination_snapshot {
            snapshots::save_snapshot_row_tx(&tx, "coordination", snapshot)?;
            bump_metadata_value_tx(&tx, COORDINATION_REVISION_KEY)?;
        }
        projections::apply_projection_validation_deltas_tx(&tx, &batch.validation_deltas)?;
        if !batch.validation_deltas.is_empty() {
            workspace_changed = true;
        }
        if workspace_changed {
            bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        }
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

        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        let Some(state) = graph.file_state(path) else {
            return Ok(());
        };
        let tx = self.conn.transaction()?;
        graph_io::save_file_state_tx(&tx, &state)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::delete_file_state(&tx, path)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn replace_derived_edges(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::replace_derived_edges_tx(&tx, graph)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }

    fn finalize(&mut self, graph: &Graph) -> Result<()> {
        let tx = self.conn.transaction()?;
        graph_io::finalize_tx(&tx, graph)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        tx.commit()?;
        Ok(())
    }
}

fn metadata_value(conn: &Connection, key: &str) -> Result<u64> {
    let value = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok(value as u64)
}

fn bump_metadata_value_tx(tx: &Transaction<'_>, key: &str) -> Result<u64> {
    let next = tx
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0)
        + 1;
    tx.execute(
        "INSERT INTO metadata(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, next],
    )?;
    Ok(next as u64)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "wal_autocheckpoint", 1000_i64)?;
    Ok(())
}
