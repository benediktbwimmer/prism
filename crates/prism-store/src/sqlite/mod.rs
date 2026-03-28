mod codecs;
mod graph_io;
mod history_io;
mod memory_entries;
mod projections;
mod schema;
mod snapshots;

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use tracing::info;

use crate::graph::Graph;
use crate::store::{AuxiliaryPersistBatch, IndexPersistBatch, Store};

const WORKSPACE_REVISION_KEY: &str = "revision:workspace";
const EPISODIC_REVISION_KEY: &str = "revision:episodic";
const INFERENCE_REVISION_KEY: &str = "revision:inference";
const COORDINATION_REVISION_KEY: &str = "revision:coordination";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotRevisions {
    pub workspace: u64,
    pub episodic: u64,
    pub inference: u64,
    pub coordination: u64,
}

#[derive(Debug, Default, Clone, Copy)]
struct FileStatePersistTotals {
    persisted_file_state_count: usize,
    skipped_missing_upsert_count: usize,
    node_count: usize,
    edge_count: usize,
    fingerprint_count: usize,
    unresolved_call_count: usize,
    unresolved_import_count: usize,
    unresolved_impl_count: usize,
    unresolved_intent_count: usize,
}

pub struct SqliteStore {
    pub(crate) conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let started = Instant::now();
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let open_started = Instant::now();
        let mut conn = Connection::open(path)?;
        let open_connection_ms = open_started.elapsed().as_millis();
        let configure_started = Instant::now();
        configure_connection(&conn)?;
        let configure_ms = configure_started.elapsed().as_millis();
        let schema_started = Instant::now();
        schema::init_schema(&conn)?;
        let schema_ms = schema_started.elapsed().as_millis();
        let prune_started = Instant::now();
        let pruned_co_change_rows = projections::prune_projection_co_change(&mut conn)?;
        let prune_ms = prune_started.elapsed().as_millis();
        let db_bytes = std::fs::metadata(path).map(|metadata| metadata.len()).ok();
        info!(
            cache_path = %path.display(),
            db_bytes,
            open_connection_ms,
            configure_ms,
            schema_ms,
            prune_ms,
            pruned_co_change_rows,
            total_ms = started.elapsed().as_millis(),
            "opened prism sqlite store"
        );
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

    pub fn snapshot_revisions(&self) -> Result<SnapshotRevisions> {
        let mut revisions = SnapshotRevisions::default();
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM metadata
             WHERE key IN (?1, ?2, ?3, ?4)",
        )?;
        let mut rows = stmt.query(params![
            WORKSPACE_REVISION_KEY,
            EPISODIC_REVISION_KEY,
            INFERENCE_REVISION_KEY,
            COORDINATION_REVISION_KEY,
        ])?;
        while let Some(row) = rows.next()? {
            let key = row.get::<_, String>(0)?;
            let value = row.get::<_, i64>(1)? as u64;
            match key.as_str() {
                WORKSPACE_REVISION_KEY => revisions.workspace = value,
                EPISODIC_REVISION_KEY => revisions.episodic = value,
                INFERENCE_REVISION_KEY => revisions.inference = value,
                COORDINATION_REVISION_KEY => revisions.coordination = value,
                _ => {}
            }
        }
        Ok(revisions)
    }
}

impl Store for SqliteStore {
    fn load_graph(&mut self) -> Result<Option<Graph>> {
        graph_io::load_graph(&self.conn)
    }

    fn load_history_snapshot(&mut self) -> Result<Option<prism_history::HistorySnapshot>> {
        if let Some(snapshot) = history_io::load_history_snapshot(&self.conn)? {
            Ok(Some(snapshot))
        } else {
            snapshots::load_snapshot_row(&self.conn, "history")
        }
    }

    fn save_history_snapshot(&mut self, snapshot: &prism_history::HistorySnapshot) -> Result<()> {
        let tx = self.conn.transaction()?;
        history_io::replace_history_snapshot_tx(&tx, snapshot)?;
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
        history_io::replace_history_snapshot_tx(&tx, snapshot)?;
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
        memory_entries::load_snapshot(&self.conn)
    }

    fn save_episodic_snapshot(
        &mut self,
        snapshot: &prism_memory::EpisodicMemorySnapshot,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        if memory_entries::save_snapshot_tx(&tx, snapshot)? {
            bump_metadata_value_tx(&tx, EPISODIC_REVISION_KEY)?;
        }
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
            if memory_entries::save_snapshot_tx(&tx, snapshot)? {
                bump_metadata_value_tx(&tx, EPISODIC_REVISION_KEY)?;
            }
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
        let started = Instant::now();
        let tx = self.conn.transaction()?;

        let remove_started = Instant::now();
        {
            let mut file_state_writer = graph_io::FileStateWriter::new(&tx)?;
            for path in &batch.removed_paths {
                file_state_writer.delete_file_state(path)?;
            }
        }
        let delete_file_state_ms = remove_started.elapsed().as_millis();

        let upsert_started = Instant::now();
        let mut file_state_totals = FileStatePersistTotals::default();
        {
            let mut file_state_writer = graph_io::FileStateWriter::new(&tx)?;
            for path in &batch.upserted_paths {
                let Some(state) = graph.file_state(path) else {
                    file_state_totals.skipped_missing_upsert_count += 1;
                    continue;
                };
                file_state_totals.persisted_file_state_count += 1;
                file_state_totals.node_count += state.nodes.len();
                file_state_totals.edge_count += state.edges.len();
                file_state_totals.fingerprint_count += state.record.fingerprints.len();
                file_state_totals.unresolved_call_count += state.record.unresolved_calls.len();
                file_state_totals.unresolved_import_count += state.record.unresolved_imports.len();
                file_state_totals.unresolved_impl_count += state.record.unresolved_impls.len();
                file_state_totals.unresolved_intent_count += state.record.unresolved_intents.len();
                file_state_writer.save_file_state(&state)?;
            }
        }
        let save_file_state_ms = upsert_started.elapsed().as_millis();

        let rewritten_derived_edge_count = graph
            .edges
            .iter()
            .filter(|edge| {
                matches!(
                    edge.kind,
                    prism_ir::EdgeKind::Calls
                        | prism_ir::EdgeKind::Imports
                        | prism_ir::EdgeKind::Implements
                        | prism_ir::EdgeKind::Specifies
                        | prism_ir::EdgeKind::Validates
                        | prism_ir::EdgeKind::RelatedTo
                )
            })
            .count();
        let replace_derived_started = Instant::now();
        graph_io::replace_derived_edges_tx(&tx, graph)?;
        let replace_derived_edges_ms = replace_derived_started.elapsed().as_millis();

        let rewritten_root_node_count = graph
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.kind,
                    prism_ir::NodeKind::Workspace | prism_ir::NodeKind::Package
                )
            })
            .count();
        let rewritten_root_edge_count = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == prism_ir::EdgeKind::Contains
                    && edge.source.kind == prism_ir::NodeKind::Workspace
                    && edge.target.kind == prism_ir::NodeKind::Package
            })
            .count();
        let finalize_started = Instant::now();
        graph_io::finalize_tx(&tx, graph)?;
        let finalize_ms = finalize_started.elapsed().as_millis();

        let save_history_started = Instant::now();
        if let Some(history_delta) = &batch.history_delta {
            history_io::apply_history_delta_tx(&tx, history_delta)?;
        } else {
            history_io::replace_history_snapshot_tx(&tx, &batch.history_snapshot)?;
        }
        let save_history_ms = save_history_started.elapsed().as_millis();

        let save_outcomes_started = Instant::now();
        snapshots::save_snapshot_row_tx(&tx, "outcomes", &batch.outcome_snapshot)?;
        let save_outcomes_ms = save_outcomes_started.elapsed().as_millis();

        let projection_started = Instant::now();
        let projection_mode = if batch.projection_snapshot.is_some() {
            "snapshot"
        } else {
            "delta"
        };
        let projection_snapshot_lineage_count = batch
            .projection_snapshot
            .as_ref()
            .map(|snapshot| snapshot.co_change_by_lineage.len())
            .unwrap_or(0);
        let projection_snapshot_validation_count = batch
            .projection_snapshot
            .as_ref()
            .map(|snapshot| snapshot.validation_by_lineage.len())
            .unwrap_or(0);
        if let Some(snapshot) = &batch.projection_snapshot {
            projections::save_projection_snapshot_tx(&tx, snapshot)?;
        } else {
            projections::apply_projection_co_change_deltas_tx(&tx, &batch.co_change_deltas)?;
            projections::apply_projection_validation_deltas_tx(&tx, &batch.validation_deltas)?;
        }
        let persist_projection_ms = projection_started.elapsed().as_millis();

        let revision_started = Instant::now();
        let workspace_revision = bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        let bump_workspace_revision_ms = revision_started.elapsed().as_millis();
        let commit_started = Instant::now();
        tx.commit()?;
        let commit_tx_ms = commit_started.elapsed().as_millis();
        info!(
            removed_file_count = batch.removed_paths.len(),
            upserted_file_count = batch.upserted_paths.len(),
            persisted_file_state_count = file_state_totals.persisted_file_state_count,
            skipped_missing_upsert_count = file_state_totals.skipped_missing_upsert_count,
            persisted_node_count = file_state_totals.node_count,
            persisted_non_derived_edge_count = file_state_totals.edge_count,
            persisted_fingerprint_count = file_state_totals.fingerprint_count,
            unresolved_call_count = file_state_totals.unresolved_call_count,
            unresolved_import_count = file_state_totals.unresolved_import_count,
            unresolved_impl_count = file_state_totals.unresolved_impl_count,
            unresolved_intent_count = file_state_totals.unresolved_intent_count,
            rewritten_derived_edge_count,
            rewritten_root_node_count,
            rewritten_root_edge_count,
            history_lineage_count = batch.history_snapshot.node_to_lineage.len(),
            history_event_count = batch.history_snapshot.events.len(),
            history_tombstone_count = batch.history_snapshot.tombstones.len(),
            outcome_event_count = batch.outcome_snapshot.events.len(),
            projection_mode,
            projection_snapshot_lineage_count,
            projection_snapshot_validation_count,
            co_change_delta_count = batch.co_change_deltas.len(),
            validation_delta_count = batch.validation_deltas.len(),
            workspace_revision,
            delete_file_state_ms,
            save_file_state_ms,
            replace_derived_edges_ms,
            finalize_ms,
            save_history_ms,
            save_outcomes_ms,
            persist_projection_ms,
            bump_workspace_revision_ms,
            commit_tx_ms,
            total_ms = started.elapsed().as_millis(),
            "persisted prism sqlite index batch"
        );
        Ok(())
    }

    fn save_file_state(&mut self, path: &Path, graph: &Graph) -> Result<()> {
        let Some(state) = graph.file_state(path) else {
            return Ok(());
        };
        let tx = self.conn.transaction()?;
        let mut file_state_writer = graph_io::FileStateWriter::new(&tx)?;
        file_state_writer.save_file_state(&state)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        drop(file_state_writer);
        tx.commit()?;
        Ok(())
    }

    fn remove_file_state(&mut self, path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        let mut file_state_writer = graph_io::FileStateWriter::new(&tx)?;
        file_state_writer.delete_file_state(path)?;
        bump_metadata_value_tx(&tx, WORKSPACE_REVISION_KEY)?;
        drop(file_state_writer);
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
