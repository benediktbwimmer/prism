use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::indexer_support::{
    build_workspace_session, collect_pending_file_parses, resolve_graph_edges,
};
use crate::layout::{discover_layout, sync_root_nodes, PackageInfo, WorkspaceLayout};
use crate::memory_refresh::reanchor_persisted_memory_snapshot;
use crate::patch_outcomes::default_outcome_meta;
use crate::reanchor::{detect_moved_files, infer_reanchors};
use crate::session::WorkspaceSession;
use crate::util::{cache_path, cleanup_legacy_cache, default_adapters};
use crate::WorkspaceSessionOptions;
use anyhow::Result;
use prism_coordination::CoordinationStore;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::{ChangeTrigger, Edge, EdgeKind, EdgeOrigin, LineageEvent, ObservedChangeSet};
use prism_memory::OutcomeMemory;
use prism_parser::{LanguageAdapter, ParseInput, ParseResult};
use prism_projections::{
    co_change_deltas_for_events, CoChangeDelta, ProjectionIndex, ValidationDelta,
};
use prism_query::Prism;
use prism_store::{Graph, IndexPersistBatch, SqliteStore, Store};
use tracing::{info, warn};

const SLOW_FILE_PHASE_THRESHOLD_MS: u128 = 200;

pub struct WorkspaceIndexer<S: Store> {
    pub(crate) root: PathBuf,
    pub(crate) layout: WorkspaceLayout,
    pub(crate) graph: Graph,
    pub(crate) history: HistoryStore,
    pub(crate) outcomes: OutcomeMemory,
    pub(crate) coordination: CoordinationStore,
    pub(crate) projections: ProjectionIndex,
    pub(crate) had_prior_snapshot: bool,
    pub(crate) had_projection_snapshot: bool,
    pub(crate) adapters: Vec<Box<dyn LanguageAdapter>>,
    pub(crate) store: S,
    pub(crate) coordination_enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingFileParse {
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    pub(crate) hash: u64,
    pub(crate) previous_path: Option<PathBuf>,
}

impl WorkspaceIndexer<SqliteStore> {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_options(root, WorkspaceSessionOptions::default())
    }

    pub fn new_with_options(
        root: impl AsRef<Path>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root))?;
        Self::with_store_and_options(root, store, options)
    }

    pub fn into_session(
        self,
        root: PathBuf,
        backend: Option<Arc<dyn CuratorBackend>>,
    ) -> Result<WorkspaceSession> {
        build_workspace_session(
            root,
            self.store,
            self.graph,
            self.history,
            self.outcomes,
            self.coordination,
            self.projections,
            self.coordination_enabled,
            backend,
        )
    }
}

impl<S: Store> WorkspaceIndexer<S> {
    pub fn with_store(root: impl AsRef<Path>, store: S) -> Result<Self> {
        Self::with_store_and_options(root, store, WorkspaceSessionOptions::default())
    }

    pub fn with_store_and_options(
        root: impl AsRef<Path>,
        mut store: S,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let started = Instant::now();
        let root = root.as_ref().canonicalize()?;
        let layout_started = Instant::now();
        let layout = discover_layout(&root)?;
        let discover_layout_ms = layout_started.elapsed().as_millis();
        let load_graph_started = Instant::now();
        let stored_graph = store.load_graph()?;
        let load_graph_ms = load_graph_started.elapsed().as_millis();
        let had_prior_snapshot = stored_graph.is_some();
        let mut graph = stored_graph.unwrap_or_default();
        sync_root_nodes(&mut graph, &layout);
        let load_history_started = Instant::now();
        let mut history = store
            .load_history_snapshot()?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        let load_history_ms = load_history_started.elapsed().as_millis();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let load_outcomes_started = Instant::now();
        let outcomes = store
            .load_outcome_snapshot()?
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
        let load_outcomes_ms = load_outcomes_started.elapsed().as_millis();
        let load_coordination_started = Instant::now();
        let coordination = if options.coordination {
            store
                .load_coordination_snapshot()?
                .map(CoordinationStore::from_snapshot)
                .unwrap_or_else(CoordinationStore::new)
        } else {
            CoordinationStore::new()
        };
        let load_coordination_ms = load_coordination_started.elapsed().as_millis();
        let load_projection_started = Instant::now();
        let stored_projection_snapshot = store.load_projection_snapshot()?;
        let load_projection_ms = load_projection_started.elapsed().as_millis();
        let had_projection_snapshot = stored_projection_snapshot.is_some();
        let derive_projection_started = Instant::now();
        let projections = stored_projection_snapshot
            .map(ProjectionIndex::from_snapshot)
            .unwrap_or_else(|| ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot()));
        let derive_or_restore_projection_ms = derive_projection_started.elapsed().as_millis();

        info!(
            root = %root.display(),
            coordination_enabled = options.coordination,
            package_count = layout.packages.len(),
            node_count = graph.node_count(),
            edge_count = graph.edge_count(),
            file_count = graph.file_count(),
            had_prior_snapshot,
            had_projection_snapshot,
            discover_layout_ms,
            load_graph_ms,
            load_history_ms,
            load_outcomes_ms,
            load_coordination_ms,
            load_projection_ms,
            derive_or_restore_projection_ms,
            total_ms = started.elapsed().as_millis(),
            "prepared prism workspace indexer"
        );

        Ok(Self {
            root,
            layout,
            graph,
            history,
            outcomes,
            coordination,
            projections,
            had_prior_snapshot,
            had_projection_snapshot,
            adapters: default_adapters(),
            store,
            coordination_enabled: options.coordination,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let _ = self.index_with_observed_changes()?;
        Ok(())
    }

    pub fn index_with_changes(&mut self) -> Result<Vec<prism_ir::GraphChange>> {
        let (_, changes) = self.index_impl(ChangeTrigger::ManualReindex)?;
        Ok(changes)
    }

    pub fn index_with_observed_changes(&mut self) -> Result<Vec<ObservedChangeSet>> {
        self.index_with_trigger(ChangeTrigger::ManualReindex)
    }

    pub fn index_with_trigger(&mut self, trigger: ChangeTrigger) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl(trigger)?;
        Ok(observed)
    }

    fn index_impl(
        &mut self,
        trigger: ChangeTrigger,
    ) -> Result<(Vec<ObservedChangeSet>, Vec<prism_ir::GraphChange>)> {
        let started = Instant::now();
        info!(
            root = %self.root.display(),
            trigger = ?trigger,
            existing_node_count = self.graph.node_count(),
            existing_edge_count = self.graph.edge_count(),
            existing_file_count = self.graph.file_count(),
            "starting prism workspace indexing"
        );
        let mut observed_changes = Vec::<ObservedChangeSet>::new();
        let mut changes = Vec::<prism_ir::GraphChange>::new();
        let mut all_lineage_events = Vec::<LineageEvent>::new();
        let mut co_change_deltas = Vec::<CoChangeDelta>::new();
        let mut validation_deltas = Vec::<ValidationDelta>::new();
        let mut upserted_paths = Vec::<PathBuf>::new();
        let mut removed_paths = Vec::<PathBuf>::new();
        let walk_root = self.root.clone();
        let collect_pending_started = Instant::now();
        let (mut pending, seen_files) = collect_pending_file_parses(&walk_root, &self.adapters)?;
        let collect_pending_ms = collect_pending_started.elapsed().as_millis();
        let pending_file_count = pending.len();
        let pending_bytes = pending
            .iter()
            .map(|pending_file| pending_file.source.len())
            .sum::<usize>();
        info!(
            root = %self.root.display(),
            pending_file_count,
            pending_bytes,
            seen_file_count = seen_files.len(),
            collect_pending_ms,
            "collected prism pending file parses"
        );

        let moved_paths = detect_moved_files(&self.graph, &seen_files, &mut pending);
        let moved_file_count = moved_paths.len();
        let mut skipped_unchanged_count = 0usize;
        let parse_apply_started = Instant::now();
        let mut parsed_file_count = 0usize;

        for pending_file in pending {
            if pending_file.previous_path.is_none()
                && self
                    .graph
                    .file_record(&pending_file.path)
                    .map(|record| record.hash == pending_file.hash)
                    .unwrap_or(false)
            {
                skipped_unchanged_count += 1;
                continue;
            }

            let Some(adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(&pending_file.path))
            else {
                continue;
            };

            let file_started = Instant::now();
            let previous_path = pending_file.previous_path.as_deref();
            let file_id = previous_path
                .and_then(|path| self.graph.file_record(path).map(|record| record.file_id))
                .unwrap_or_else(|| self.graph.ensure_file(&pending_file.path));
            let package = self.layout.package_for(&pending_file.path).clone();
            let input = ParseInput {
                package_name: &package.package_name,
                crate_name: &package.crate_name,
                package_root: &package.root,
                path: &pending_file.path,
                file_id,
                source: &pending_file.source,
            };
            let language = adapter.language();
            let parse_started = Instant::now();
            let parsed = adapter.parse(&input)?;
            let parse_ms = parse_started.elapsed().as_millis();
            let upsert_started = Instant::now();
            let update = self.upsert_parsed_file(
                previous_path,
                &pending_file.path,
                pending_file.hash,
                &package,
                parsed,
                trigger.clone(),
            );
            let upsert_ms = upsert_started.elapsed().as_millis();
            parsed_file_count += 1;
            let new_lineage_events = self.history.apply(&update.observed);
            let change_set_deltas = co_change_deltas_for_events(&new_lineage_events);
            self.projections.apply_lineage_events(&new_lineage_events);
            co_change_deltas.extend(change_set_deltas);
            self.outcomes.apply_lineage(&new_lineage_events)?;
            all_lineage_events.extend(new_lineage_events.iter().cloned());
            validation_deltas.extend(self.record_patch_outcome(&update.observed));
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            upserted_paths.push(pending_file.path.clone());
            let file_total_ms = file_started.elapsed().as_millis();

            if parse_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
                || upsert_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
                || file_total_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
            {
                warn!(
                    root = %self.root.display(),
                    path = %pending_file.path.display(),
                    language = ?language,
                    source_bytes = pending_file.source.len(),
                    parse_ms,
                    upsert_ms,
                    file_total_ms,
                    parsed_file_count,
                    skipped_unchanged_count,
                    "slow prism file processing"
                );
            }

            if parsed_file_count % 25 == 0 {
                info!(
                    root = %self.root.display(),
                    parsed_file_count,
                    skipped_unchanged_count,
                    elapsed_ms = parse_apply_started.elapsed().as_millis(),
                    "processed prism file parse batch"
                );
            }
        }
        let parse_apply_ms = parse_apply_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            parsed_file_count,
            skipped_unchanged_count,
            moved_file_count,
            parse_apply_ms,
            "finished prism parse and update loop"
        );

        let remove_missing_started = Instant::now();
        for tracked in self.graph.tracked_files() {
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_observed(
                    &tracked,
                    default_outcome_meta("observed"),
                    trigger.clone(),
                );
                let new_lineage_events = self.history.apply(&update.observed);
                let change_set_deltas = co_change_deltas_for_events(&new_lineage_events);
                self.projections.apply_lineage_events(&new_lineage_events);
                co_change_deltas.extend(change_set_deltas);
                self.outcomes.apply_lineage(&new_lineage_events)?;
                all_lineage_events.extend(new_lineage_events.iter().cloned());
                validation_deltas.extend(self.record_patch_outcome(&update.observed));
                observed_changes.push(update.observed.clone());
                changes.extend(update.changes);
                removed_paths.push(tracked.clone());
            }
        }
        let remove_missing_ms = remove_missing_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            removed_file_count = removed_paths.len(),
            remove_missing_ms,
            "finished prism missing-file removal phase"
        );

        let resolve_edges_started = Instant::now();
        resolve_graph_edges(&mut self.graph);
        let resolve_edges_ms = resolve_edges_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            node_count = self.graph.node_count(),
            edge_count = self.graph.edge_count(),
            resolve_edges_ms,
            "finished prism edge resolution phase"
        );
        self.history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        let projection_snapshot =
            (!self.had_projection_snapshot).then(|| self.projections.snapshot());
        let upserted_file_count = upserted_paths.len();
        let removed_file_count = removed_paths.len();
        let co_change_delta_count = co_change_deltas.len();
        let validation_delta_count = validation_deltas.len();
        let batch = IndexPersistBatch {
            upserted_paths,
            removed_paths,
            history_snapshot: self.history.snapshot(),
            outcome_snapshot: self.outcomes.snapshot(),
            co_change_deltas,
            validation_deltas,
            projection_snapshot,
        };
        let skip_persist = self.had_prior_snapshot
            && self.had_projection_snapshot
            && batch.upserted_paths.is_empty()
            && batch.removed_paths.is_empty()
            && batch.co_change_deltas.is_empty()
            && batch.validation_deltas.is_empty()
            && batch.projection_snapshot.is_none();
        let persist_ms = if skip_persist {
            info!(
                root = %self.root.display(),
                "skipped prism index persistence batch because workspace state is unchanged"
            );
            0
        } else {
            let persist_started = Instant::now();
            self.store.commit_index_persist_batch(&self.graph, &batch)?;
            let persist_ms = persist_started.elapsed().as_millis();
            info!(
                root = %self.root.display(),
                upserted_file_count,
                removed_file_count,
                co_change_delta_count,
                validation_delta_count,
                persist_ms,
                "persisted prism index batch"
            );
            persist_ms
        };
        let reanchor_started = Instant::now();
        reanchor_persisted_memory_snapshot(&mut self.store, &all_lineage_events)?;
        let reanchor_memory_ms = reanchor_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            lineage_event_count = all_lineage_events.len(),
            reanchor_memory_ms,
            "reanchored persisted prism memory"
        );
        self.had_prior_snapshot = true;
        self.had_projection_snapshot = true;
        info!(
            root = %self.root.display(),
            trigger = ?trigger,
            pending_file_count,
            pending_bytes,
            seen_file_count = seen_files.len(),
            moved_file_count,
            skipped_unchanged_count,
            upserted_file_count,
            removed_file_count,
            observed_change_sets = observed_changes.len(),
            graph_changes = changes.len(),
            lineage_event_count = all_lineage_events.len(),
            co_change_delta_count,
            validation_delta_count,
            persist_skipped = skip_persist,
            node_count = self.graph.node_count(),
            edge_count = self.graph.edge_count(),
            file_count = self.graph.file_count(),
            collect_pending_ms,
            parse_apply_ms,
            remove_missing_ms,
            resolve_edges_ms,
            persist_ms,
            reanchor_memory_ms,
            total_ms = started.elapsed().as_millis(),
            "completed prism workspace indexing"
        );
        Ok((observed_changes, changes))
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn into_prism(self) -> Prism {
        Prism::with_history_outcomes_coordination_and_projections(
            self.graph,
            self.history,
            self.outcomes,
            self.coordination,
            self.projections,
        )
    }

    fn upsert_parsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        package: &PackageInfo,
        parsed: ParseResult,
        trigger: ChangeTrigger,
    ) -> prism_store::FileUpdate {
        let previous_state = previous_path
            .or(Some(path))
            .and_then(|candidate| self.graph.file_state(candidate));
        let reanchors = previous_state
            .as_ref()
            .map(|state| infer_reanchors(state, &parsed))
            .unwrap_or_default();
        let package_id = package.node_id.clone();
        let contained_nodes = parsed
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Contains)
            .map(|edge| edge.target.clone())
            .collect::<HashSet<_>>();
        let package_edges = parsed
            .nodes
            .iter()
            .filter(|node| !contained_nodes.contains(&node.id))
            .map(|node| Edge {
                kind: EdgeKind::Contains,
                source: package_id.clone(),
                target: node.id.clone(),
                origin: EdgeOrigin::Static,
                confidence: 1.0,
            })
            .collect::<Vec<_>>();

        let mut edges = parsed.edges;
        edges.extend(package_edges);
        self.graph.upsert_file_from_with_observed(
            previous_path,
            path,
            hash,
            parsed.nodes,
            edges,
            parsed.fingerprints,
            parsed.unresolved_calls,
            parsed.unresolved_imports,
            parsed.unresolved_impls,
            parsed.unresolved_intents,
            &reanchors,
            default_outcome_meta("observed"),
            trigger,
        )
    }
}
