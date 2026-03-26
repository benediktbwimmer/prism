use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::indexer_support::{
    build_workspace_session, collect_pending_file_parses, resolve_graph_edges,
};
use crate::layout::{discover_layout, sync_root_nodes, PackageInfo, WorkspaceLayout};
use crate::patch_outcomes::default_outcome_meta;
use crate::reanchor::{detect_moved_files, infer_reanchors};
use crate::session::WorkspaceSession;
use crate::util::{cache_path, cleanup_legacy_cache, default_adapters};
use anyhow::Result;
use prism_coordination::CoordinationStore;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::{ChangeTrigger, Edge, EdgeKind, EdgeOrigin, ObservedChangeSet};
use prism_memory::OutcomeMemory;
use prism_parser::{LanguageAdapter, ParseInput, ParseResult};
use prism_projections::{
    co_change_deltas_for_events, CoChangeDelta, ProjectionIndex, ValidationDelta,
};
use prism_query::Prism;
use prism_store::{Graph, IndexPersistBatch, SqliteStore, Store};

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
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root))?;
        Self::with_store(root, store)
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
            backend,
        )
    }
}

impl<S: Store> WorkspaceIndexer<S> {
    pub fn with_store(root: impl AsRef<Path>, mut store: S) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let layout = discover_layout(&root)?;
        let stored_graph = store.load_graph()?;
        let had_prior_snapshot = stored_graph.is_some();
        let mut graph = stored_graph.unwrap_or_default();
        sync_root_nodes(&mut graph, &layout);
        let mut history = store
            .load_history_snapshot()?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let outcomes = store
            .load_outcome_snapshot()?
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
        let coordination = store
            .load_coordination_snapshot()?
            .map(CoordinationStore::from_snapshot)
            .unwrap_or_else(CoordinationStore::new);
        let stored_projection_snapshot = store.load_projection_snapshot()?;
        let had_projection_snapshot = stored_projection_snapshot.is_some();
        let projections = stored_projection_snapshot
            .map(ProjectionIndex::from_snapshot)
            .unwrap_or_else(|| ProjectionIndex::derive(&history.snapshot(), &outcomes.snapshot()));

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
        let mut observed_changes = Vec::<ObservedChangeSet>::new();
        let mut changes = Vec::<prism_ir::GraphChange>::new();
        let mut co_change_deltas = Vec::<CoChangeDelta>::new();
        let mut validation_deltas = Vec::<ValidationDelta>::new();
        let mut upserted_paths = Vec::<PathBuf>::new();
        let mut removed_paths = Vec::<PathBuf>::new();
        let walk_root = self.root.clone();
        let (mut pending, seen_files) = collect_pending_file_parses(&walk_root, &self.adapters)?;

        let moved_paths = detect_moved_files(&self.graph, &seen_files, &mut pending);

        for pending_file in pending {
            if pending_file.previous_path.is_none()
                && self
                    .graph
                    .file_record(&pending_file.path)
                    .map(|record| record.hash == pending_file.hash)
                    .unwrap_or(false)
            {
                continue;
            }

            let Some(adapter) = self
                .adapters
                .iter()
                .find(|adapter| adapter.supports_path(&pending_file.path))
            else {
                continue;
            };

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
            let parsed = adapter.parse(&input)?;
            let update = self.upsert_parsed_file(
                previous_path,
                &pending_file.path,
                pending_file.hash,
                &package,
                parsed,
                trigger.clone(),
            );
            let lineage_events = self.history.apply(&update.observed);
            let change_set_deltas = co_change_deltas_for_events(&lineage_events);
            self.projections.apply_lineage_events(&lineage_events);
            co_change_deltas.extend(change_set_deltas);
            self.outcomes.apply_lineage(&lineage_events)?;
            validation_deltas.extend(self.record_patch_outcome(&update.observed));
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            upserted_paths.push(pending_file.path.clone());
        }

        for tracked in self.graph.tracked_files() {
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_observed(
                    &tracked,
                    default_outcome_meta("observed"),
                    trigger.clone(),
                );
                let lineage_events = self.history.apply(&update.observed);
                let change_set_deltas = co_change_deltas_for_events(&lineage_events);
                self.projections.apply_lineage_events(&lineage_events);
                co_change_deltas.extend(change_set_deltas);
                self.outcomes.apply_lineage(&lineage_events)?;
                validation_deltas.extend(self.record_patch_outcome(&update.observed));
                observed_changes.push(update.observed.clone());
                changes.extend(update.changes);
                removed_paths.push(tracked.clone());
            }
        }

        resolve_graph_edges(&mut self.graph);
        self.history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        let projection_snapshot =
            (!self.had_projection_snapshot).then(|| self.projections.snapshot());
        let batch = IndexPersistBatch {
            upserted_paths,
            removed_paths,
            history_snapshot: self.history.snapshot(),
            outcome_snapshot: self.outcomes.snapshot(),
            co_change_deltas,
            validation_deltas,
            projection_snapshot,
        };
        self.store.commit_index_persist_batch(&self.graph, &batch)?;
        self.had_prior_snapshot = true;
        self.had_projection_snapshot = true;
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
