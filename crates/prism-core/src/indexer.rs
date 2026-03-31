use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::concept_events::load_repo_curated_concepts;
use crate::concept_relation_events::load_repo_concept_relations;
use crate::contract_events::load_repo_curated_contracts;
use crate::coordination_persistence::CoordinationPersistenceBackend;
use crate::indexer_support::{
    build_workspace_session, collect_pending_file_parses, path_matches_refresh_scope,
    resolve_graph_edges,
};
use crate::invalidation::RefreshInvalidationScope;
use crate::layout::{discover_layout, sync_root_nodes, PackageInfo, WorkspaceLayout};
use crate::memory_refresh::reanchor_persisted_memory_snapshot;
use crate::parse_pipeline::{parse_jobs_in_parallel, PreparedParseJob};
use crate::patch_outcomes::default_outcome_meta;
use crate::reanchor::{detect_moved_files, infer_reanchors};
use crate::session::{WorkspaceSession, HOT_OUTCOME_HYDRATION_LIMIT};
use crate::shared_runtime::{
    local_projection_snapshot_for_persist, merged_projection_index,
    overlay_persisted_projection_knowledge, projection_snapshot_without_knowledge,
};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::util::{cache_path, cleanup_legacy_cache, default_adapters};
use crate::workspace_tree::{
    build_workspace_tree_snapshot, plan_incremental_refresh, populate_package_regions,
    WorkspaceRefreshPlan,
};
use crate::WorkspaceSessionOptions;
use anyhow::Result;
use prism_coordination::CoordinationSnapshot;
use prism_curator::CuratorBackend;
use prism_history::HistoryStore;
use prism_ir::{
    ChangeTrigger, Edge, EdgeKind, EdgeOrigin, LineageEvent, ObservedChangeSet,
    PlanExecutionOverlay, PlanGraph,
};
use prism_memory::OutcomeMemory;
use prism_parser::{LanguageAdapter, ParseDepth, ParseResult};
use prism_projections::{
    co_change_deltas_for_events, CoChangeDelta, ProjectionIndex, ValidationDelta,
    MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
};
use prism_query::Prism;
use prism_store::{Graph, IndexPersistBatch, SqliteStore, Store, WorkspaceTreeSnapshot};
use tracing::{info, warn};

const SLOW_FILE_PHASE_THRESHOLD_MS: u128 = 200;
const SMALL_REPO_DEEP_PARSE_FILE_LIMIT: usize = 64;

fn distinct_lineage_count(events: &[LineageEvent]) -> usize {
    let mut lineages = events
        .iter()
        .map(|event| event.lineage.clone())
        .collect::<Vec<_>>();
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages.dedup();
    lineages.len()
}

pub struct WorkspaceIndexer<S: Store> {
    pub(crate) root: PathBuf,
    pub(crate) layout: WorkspaceLayout,
    pub(crate) graph: Graph,
    pub(crate) history: HistoryStore,
    pub(crate) outcomes: OutcomeMemory,
    pub(crate) coordination_snapshot: CoordinationSnapshot,
    pub(crate) plan_graphs: Vec<PlanGraph>,
    pub(crate) plan_execution_overlays: BTreeMap<String, Vec<PlanExecutionOverlay>>,
    pub(crate) projections: ProjectionIndex,
    pub(crate) had_prior_snapshot: bool,
    pub(crate) had_projection_snapshot: bool,
    pub(crate) adapters: Vec<Box<dyn LanguageAdapter + Send + Sync>>,
    pub(crate) store: S,
    pub(crate) workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
    pub(crate) shared_runtime: SharedRuntimeBackend,
    pub(crate) shared_runtime_store: Option<SqliteStore>,
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
        let mut indexer = Self::with_store_and_options(root.clone(), store, options.clone())?;
        let mut shared_runtime_store = match &options.shared_runtime {
            SharedRuntimeBackend::Disabled => None,
            SharedRuntimeBackend::Sqlite { path } => Some(SqliteStore::open(path)?),
            SharedRuntimeBackend::Remote { uri } => {
                anyhow::bail!("shared runtime backend `{uri}` is not implemented yet")
            }
        };
        if let Some(shared_store) = shared_runtime_store.as_mut() {
            if options.coordination {
                let plan_state =
                    shared_store.load_hydrated_coordination_plan_state_for_root(&root)?;
                indexer.coordination_snapshot = plan_state
                    .as_ref()
                    .map(|state| state.snapshot.clone())
                    .unwrap_or_default();
                indexer.plan_graphs = plan_state
                    .as_ref()
                    .map(|state| state.plan_graphs.clone())
                    .unwrap_or_default();
                indexer.plan_execution_overlays = plan_state
                    .map(|state| state.execution_overlays)
                    .unwrap_or_default();
            }
            let local_projection_snapshot = indexer.store.load_projection_snapshot()?;
            let shared_projection_snapshot = shared_store.load_projection_snapshot()?;
            indexer.projections = merged_projection_index(
                if options.hydrate_persisted_projections {
                    local_projection_snapshot.clone()
                } else {
                    None
                },
                if options.hydrate_persisted_projections {
                    shared_projection_snapshot.clone()
                } else {
                    None
                },
                load_repo_curated_concepts(&root)?,
                load_repo_curated_contracts(&root)?,
                load_repo_concept_relations(&root)?,
                &indexer.history.snapshot(),
                &indexer.outcomes.snapshot(),
            );
            if !options.hydrate_persisted_projections {
                overlay_persisted_projection_knowledge(
                    &mut indexer.projections,
                    local_projection_snapshot
                        .into_iter()
                        .chain(shared_projection_snapshot),
                );
            }
        }
        indexer.shared_runtime = options.shared_runtime.clone();
        indexer.shared_runtime_store = shared_runtime_store;
        Ok(indexer)
    }

    pub fn new_from_live_prism_with_options(
        root: impl AsRef<Path>,
        prism: &Prism,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root))?;
        let mut indexer = Self::with_live_prism_and_options(
            root.clone(),
            store,
            prism,
            workspace_tree_snapshot,
            options.clone(),
        )?;
        let shared_runtime_store = match &options.shared_runtime {
            SharedRuntimeBackend::Disabled => None,
            SharedRuntimeBackend::Sqlite { path } => Some(SqliteStore::open(path)?),
            SharedRuntimeBackend::Remote { uri } => {
                anyhow::bail!("shared runtime backend `{uri}` is not implemented yet")
            }
        };
        indexer.shared_runtime = options.shared_runtime.clone();
        indexer.shared_runtime_store = shared_runtime_store;
        Ok(indexer)
    }

    pub fn into_session(
        self,
        root: PathBuf,
        backend: Option<Arc<dyn CuratorBackend>>,
    ) -> Result<WorkspaceSession> {
        build_workspace_session(
            root,
            self.store,
            self.workspace_tree_snapshot.unwrap_or_default(),
            self.shared_runtime,
            self.shared_runtime_store,
            self.graph,
            self.history,
            self.outcomes,
            self.coordination_snapshot,
            self.plan_graphs,
            self.plan_execution_overlays,
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

    pub fn with_live_prism_and_options(
        root: impl AsRef<Path>,
        store: S,
        prism: &Prism,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let started = Instant::now();
        let root = root.as_ref().canonicalize()?;
        let WorkspaceSessionOptions {
            coordination,
            shared_runtime,
            hydrate_persisted_projections: _,
        } = options;
        let layout_started = Instant::now();
        let layout = discover_layout(&root)?;
        let discover_layout_ms = layout_started.elapsed().as_millis();
        let restore_runtime_started = Instant::now();
        let mut graph = Graph::from_snapshot(prism.graph().snapshot());
        sync_root_nodes(&mut graph, &layout);
        let mut history = HistoryStore::from_snapshot(prism.history_snapshot());
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let history_snapshot = history.snapshot();
        let outcomes = OutcomeMemory::from_snapshot(prism.outcome_snapshot());
        let projections = merged_projection_index(
            Some(prism.projection_snapshot()),
            None,
            load_repo_curated_concepts(&root)?,
            load_repo_curated_contracts(&root)?,
            load_repo_concept_relations(&root)?,
            &history_snapshot,
            &outcomes.snapshot(),
        );
        let (coordination_snapshot, plan_graphs, plan_execution_overlays) = if coordination {
            (
                prism.coordination_snapshot(),
                prism.authored_plan_graphs(),
                prism.plan_execution_overlays_by_plan(),
            )
        } else {
            (CoordinationSnapshot::default(), Vec::new(), BTreeMap::new())
        };
        let restore_runtime_ms = restore_runtime_started.elapsed().as_millis();

        info!(
            root = %root.display(),
            coordination_enabled = coordination,
            node_count = graph.node_count(),
            edge_count = graph.edge_count(),
            file_count = graph.file_count(),
            discover_layout_ms,
            restore_runtime_ms,
            total_ms = started.elapsed().as_millis(),
            "prepared prism workspace indexer from live runtime state"
        );

        Ok(Self {
            root,
            layout,
            graph,
            history,
            outcomes,
            coordination_snapshot,
            plan_graphs,
            plan_execution_overlays,
            projections,
            had_prior_snapshot: true,
            had_projection_snapshot: true,
            adapters: default_adapters(),
            store,
            workspace_tree_snapshot,
            shared_runtime,
            shared_runtime_store: None,
            coordination_enabled: coordination,
        })
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
        let load_projection_started = Instant::now();
        let persisted_projection_snapshot = store.load_projection_snapshot()?;
        let workspace_tree_snapshot = store.load_workspace_tree_snapshot()?;
        let base_projection_snapshot = persisted_projection_snapshot.clone().map(|snapshot| {
            if options.hydrate_persisted_projections {
                snapshot
            } else {
                projection_snapshot_without_knowledge(snapshot)
            }
        });
        let load_projection_ms = load_projection_started.elapsed().as_millis();
        let load_history_started = Instant::now();
        let mut history = store
            .load_history_snapshot_with_options(base_projection_snapshot.is_none())?
            .map(HistoryStore::from_snapshot)
            .unwrap_or_else(HistoryStore::new);
        let load_history_ms = load_history_started.elapsed().as_millis();
        history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        let load_outcomes_started = Instant::now();
        let outcomes = if base_projection_snapshot.is_some() {
            store.load_recent_outcome_snapshot(HOT_OUTCOME_HYDRATION_LIMIT)?
        } else {
            store.load_outcome_snapshot()?
        }
        .map(OutcomeMemory::from_snapshot)
        .unwrap_or_else(OutcomeMemory::new);
        let load_outcomes_ms = load_outcomes_started.elapsed().as_millis();
        let load_coordination_started = Instant::now();
        let plan_state = if options.coordination {
            store.load_hydrated_coordination_plan_state_for_root(&root)?
        } else {
            None
        };
        let coordination_snapshot = plan_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let load_coordination_ms = load_coordination_started.elapsed().as_millis();
        let had_projection_snapshot = base_projection_snapshot.is_some();
        let derive_projection_started = Instant::now();
        let mut projections = merged_projection_index(
            base_projection_snapshot,
            None,
            load_repo_curated_concepts(&root)?,
            load_repo_curated_contracts(&root)?,
            load_repo_concept_relations(&root)?,
            &history.snapshot(),
            &outcomes.snapshot(),
        );
        if !options.hydrate_persisted_projections {
            overlay_persisted_projection_knowledge(
                &mut projections,
                persisted_projection_snapshot.into_iter(),
            );
        }
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
            coordination_snapshot,
            plan_graphs: plan_state
                .as_ref()
                .map(|state| state.plan_graphs.clone())
                .unwrap_or_default(),
            plan_execution_overlays: plan_state
                .map(|state| state.execution_overlays)
                .unwrap_or_default(),
            projections,
            had_prior_snapshot,
            had_projection_snapshot,
            adapters: default_adapters(),
            store,
            workspace_tree_snapshot,
            shared_runtime: options.shared_runtime,
            shared_runtime_store: None,
            coordination_enabled: options.coordination,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let _ = self.index_with_observed_changes()?;
        Ok(())
    }

    pub fn index_with_changes(&mut self) -> Result<Vec<prism_ir::GraphChange>> {
        let (_, changes) = self.index_impl(ChangeTrigger::ManualReindex, None, None, None)?;
        Ok(changes)
    }

    pub fn index_with_observed_changes(&mut self) -> Result<Vec<ObservedChangeSet>> {
        self.index_with_trigger(ChangeTrigger::ManualReindex)
    }

    pub fn index_with_trigger(&mut self, trigger: ChangeTrigger) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl(trigger, None, None, None)?;
        Ok(observed)
    }

    pub fn index_with_scope<I>(
        &mut self,
        trigger: ChangeTrigger,
        dirty_paths: I,
    ) -> Result<Vec<ObservedChangeSet>>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let dirty_paths = dirty_paths.into_iter().collect::<Vec<_>>();
        let cached_snapshot = self.workspace_tree_snapshot.clone().unwrap_or_default();
        let mut plan = plan_incremental_refresh(&self.root, &cached_snapshot, &dirty_paths)?;
        populate_package_regions(&mut plan.delta, &self.layout);
        let deep_paths = dirty_paths.into_iter().collect::<HashSet<_>>();
        let (observed, _) = self.index_impl(
            trigger,
            Some(&plan),
            Some(&plan.next_snapshot),
            Some(&deep_paths),
        )?;
        Ok(observed)
    }

    pub(crate) fn index_with_refresh_plan(
        &mut self,
        trigger: ChangeTrigger,
        plan: &WorkspaceRefreshPlan,
    ) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) =
            self.index_impl(trigger, Some(plan), Some(&plan.next_snapshot), None)?;
        Ok(observed)
    }

    pub(crate) fn index_with_refresh_plan_and_deep_paths(
        &mut self,
        trigger: ChangeTrigger,
        plan: &WorkspaceRefreshPlan,
        deep_paths: &HashSet<PathBuf>,
    ) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl(
            trigger,
            Some(plan),
            Some(&plan.next_snapshot),
            Some(deep_paths),
        )?;
        Ok(observed)
    }

    fn index_impl(
        &mut self,
        trigger: ChangeTrigger,
        refresh_plan: Option<&WorkspaceRefreshPlan>,
        next_tree_snapshot: Option<&WorkspaceTreeSnapshot>,
        forced_deep_paths: Option<&HashSet<PathBuf>>,
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
        let refresh_scope =
            refresh_plan.map(|plan| plan.delta.scope_paths().into_iter().collect::<HashSet<_>>());
        let invalidation_scope = refresh_scope
            .as_ref()
            .map(|scope| RefreshInvalidationScope::from_graph(&self.graph, scope));
        let walk_root = self.root.clone();
        let collect_pending_started = Instant::now();
        let (mut pending, seen_files) =
            collect_pending_file_parses(&walk_root, &self.adapters, refresh_scope.as_ref())?;
        let collect_pending_ms = collect_pending_started.elapsed().as_millis();
        let targeted_refresh = refresh_scope.is_some();
        let workspace_file_count = next_tree_snapshot
            .map(|snapshot| snapshot.files.len())
            .or_else(|| {
                self.workspace_tree_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.files.len())
            })
            .unwrap_or(seen_files.len());
        let refresh_scope_path_count = invalidation_scope
            .as_ref()
            .map_or(0, |scope| scope.direct_paths.len());
        let dependency_refresh_scope_path_count = invalidation_scope
            .as_ref()
            .map_or(0, |scope| scope.dependency_paths.len());
        let pending_file_count = pending.len();
        let pending_bytes = pending
            .iter()
            .map(|pending_file| pending_file.source.len())
            .sum::<usize>();
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            pending_file_count,
            pending_bytes,
            seen_file_count = seen_files.len(),
            collect_pending_ms,
            "collected prism pending file parses"
        );

        let moved_paths = detect_moved_files(
            &self.graph,
            &seen_files,
            refresh_scope.as_ref(),
            &mut pending,
        );
        let moved_file_count = moved_paths.len();
        let mut skipped_unchanged_count = 0usize;
        let parse_apply_started = Instant::now();
        let prepare_parse_started = Instant::now();
        let mut prepared_jobs = Vec::new();
        let mut unsupported_pending = Vec::new();

        for pending_file in pending {
            let desired_parse_depth = desired_parse_depth(
                &pending_file.path,
                targeted_refresh,
                workspace_file_count,
                forced_deep_paths,
            );
            if pending_file.previous_path.is_none()
                && self
                    .graph
                    .file_record(&pending_file.path)
                    .map(|record| {
                        record.hash == pending_file.hash
                            && record.parse_depth == desired_parse_depth
                    })
                    .unwrap_or(false)
            {
                skipped_unchanged_count += 1;
                continue;
            }

            let previous_path = pending_file.previous_path.as_deref();
            if let Some((adapter_index, adapter)) = self
                .adapters
                .iter()
                .enumerate()
                .find(|(_, adapter)| adapter.supports_path(&pending_file.path))
            {
                let file_id = previous_path
                    .and_then(|path| self.graph.file_record(path).map(|record| record.file_id))
                    .unwrap_or_else(|| self.graph.ensure_file(&pending_file.path));
                let package = self.layout.package_for(&pending_file.path).clone();
                prepared_jobs.push(PreparedParseJob {
                    pending: pending_file,
                    file_id,
                    package,
                    adapter_index,
                    language: adapter.language(),
                    parse_depth: desired_parse_depth,
                });
            } else {
                unsupported_pending.push(pending_file);
            }
        }
        let prepare_parse_ms = prepare_parse_started.elapsed().as_millis();
        let prepared_file_count = prepared_jobs.len();
        let parallel_parse_started = Instant::now();
        let parsed_jobs = parse_jobs_in_parallel(&self.adapters, prepared_jobs)?;
        let parallel_parse_ms = parallel_parse_started.elapsed().as_millis();
        let parse_worker_count = parsed_jobs.worker_count;
        let apply_parsed_started = Instant::now();
        let mut parsed_file_count = 0usize;

        for parsed_job in parsed_jobs.jobs {
            let previous_path = parsed_job.pending.previous_path.as_deref();
            let upsert_started = Instant::now();
            let update = self.upsert_parsed_file(
                previous_path,
                &parsed_job.pending.path,
                parsed_job.pending.hash,
                parsed_job.parse_depth,
                &parsed_job.package,
                parsed_job.parsed,
                trigger.clone(),
            );
            let upsert_ms = upsert_started.elapsed().as_millis();
            parsed_file_count += 1;
            let new_lineage_events = self.history.apply(&update.observed);
            let distinct_lineages = distinct_lineage_count(&new_lineage_events);
            let change_set_deltas = co_change_deltas_for_events(&new_lineage_events);
            if change_set_deltas.is_empty()
                && distinct_lineages > MAX_CO_CHANGE_LINEAGES_PER_CHANGESET
            {
                warn!(
                    root = %self.root.display(),
                    path = %parsed_job.pending.path.display(),
                    lineage_event_count = new_lineage_events.len(),
                    distinct_lineage_count = distinct_lineages,
                    max_co_change_lineages_per_changeset = MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
                    "skipping symbol-level co-change deltas for oversized change set"
                );
            }
            self.projections.apply_lineage_events(&new_lineage_events);
            co_change_deltas.extend(change_set_deltas);
            self.outcomes.apply_lineage(&new_lineage_events)?;
            all_lineage_events.extend(new_lineage_events.iter().cloned());
            validation_deltas.extend(self.record_patch_outcome(&update.observed));
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            upserted_paths.push(parsed_job.pending.path.clone());
            let file_total_ms = parsed_job.parse_ms + upsert_ms;

            if parsed_job.parse_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
                || upsert_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
                || file_total_ms >= SLOW_FILE_PHASE_THRESHOLD_MS
            {
                warn!(
                    root = %self.root.display(),
                    targeted_refresh,
                    dependency_refresh_scope_path_count,
                    path = %parsed_job.pending.path.display(),
                    language = ?parsed_job.language,
                    source_bytes = parsed_job.pending.source.len(),
                    parse_ms = parsed_job.parse_ms,
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
                    targeted_refresh,
                    dependency_refresh_scope_path_count,
                    parsed_file_count,
                    prepared_file_count,
                    skipped_unchanged_count,
                    elapsed_ms = parse_apply_started.elapsed().as_millis(),
                    "processed prism file parse batch"
                );
            }
        }
        let apply_unsupported_started = Instant::now();
        let unsupported_file_count = unsupported_pending.len();
        for pending_file in unsupported_pending {
            let previous_path = pending_file.previous_path.as_deref();
            let update = self.upsert_unparsed_file(
                previous_path,
                &pending_file.path,
                pending_file.hash,
                desired_parse_depth(
                    &pending_file.path,
                    targeted_refresh,
                    workspace_file_count,
                    forced_deep_paths,
                ),
                trigger.clone(),
            );
            self.apply_file_update(
                update,
                &pending_file.path,
                &mut all_lineage_events,
                &mut co_change_deltas,
                &mut validation_deltas,
                &mut observed_changes,
                &mut changes,
                &mut upserted_paths,
            )?;
        }
        let apply_unsupported_ms = apply_unsupported_started.elapsed().as_millis();
        let apply_parsed_ms = apply_parsed_started
            .elapsed()
            .as_millis()
            .saturating_sub(apply_unsupported_ms);
        let parse_apply_ms = parse_apply_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            prepared_file_count,
            parsed_file_count,
            parse_worker_count,
            prepare_parse_ms,
            parallel_parse_ms,
            apply_parsed_ms,
            unsupported_file_count,
            apply_unsupported_ms,
            skipped_unchanged_count,
            moved_file_count,
            parse_apply_ms,
            "finished prism parse and update loop"
        );

        let remove_missing_started = Instant::now();
        for tracked in self.graph.tracked_files() {
            if refresh_scope
                .as_ref()
                .is_some_and(|scope| !path_matches_refresh_scope(&tracked, scope))
            {
                continue;
            }
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_observed_without_rebuild(
                    &tracked,
                    default_outcome_meta("observed"),
                    trigger.clone(),
                );
                let new_lineage_events = self.history.apply(&update.observed);
                let distinct_lineages = distinct_lineage_count(&new_lineage_events);
                let change_set_deltas = co_change_deltas_for_events(&new_lineage_events);
                if change_set_deltas.is_empty()
                    && distinct_lineages > MAX_CO_CHANGE_LINEAGES_PER_CHANGESET
                {
                    warn!(
                        root = %self.root.display(),
                        path = %tracked.display(),
                        lineage_event_count = new_lineage_events.len(),
                        distinct_lineage_count = distinct_lineages,
                        max_co_change_lineages_per_changeset = MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
                        "skipping symbol-level co-change deltas for oversized change set"
                    );
                }
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
        let rebuild_graph_indexes_started = Instant::now();
        self.graph.rebuild_indexes();
        let rebuild_graph_indexes_ms = rebuild_graph_indexes_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            removed_file_count = removed_paths.len(),
            remove_missing_ms,
            rebuild_graph_indexes_ms,
            "finished prism missing-file removal phase"
        );

        let edge_resolution_scope = invalidation_scope
            .as_ref()
            .map(|scope| &scope.edge_resolution_paths);
        let resolve_edges_started = Instant::now();
        let resolve_edge_stats = resolve_graph_edges(&mut self.graph, edge_resolution_scope);
        let resolve_edges_ms = resolve_edges_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            edge_resolution_scope_path_count = resolve_edge_stats.resolution_scope_path_count,
            edge_resolution_scope_node_count = resolve_edge_stats.resolution_scope_node_count,
            cleared_derived_edge_count = resolve_edge_stats.cleared_derived_edge_count,
            node_count = self.graph.node_count(),
            edge_count = self.graph.edge_count(),
            unresolved_call_count = resolve_edge_stats.unresolved_call_count,
            unresolved_import_count = resolve_edge_stats.unresolved_import_count,
            unresolved_impl_count = resolve_edge_stats.unresolved_impl_count,
            unresolved_intent_count = resolve_edge_stats.unresolved_intent_count,
            resolve_calls_ms = resolve_edge_stats.resolve_calls_ms,
            resolve_imports_ms = resolve_edge_stats.resolve_imports_ms,
            resolve_impls_ms = resolve_edge_stats.resolve_impls_ms,
            resolve_intents_ms = resolve_edge_stats.resolve_intents_ms,
            resolve_edges_ms,
            "finished prism edge resolution phase"
        );
        let seeded_node_lineages = self
            .history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        let projection_snapshot = (!self.had_projection_snapshot).then(|| {
            let snapshot = self.projections.snapshot();
            if self.shared_runtime_store.is_some() {
                local_projection_snapshot_for_persist(&snapshot)
            } else {
                snapshot
            }
        });
        let history_delta = self.had_prior_snapshot.then(|| {
            self.history
                .persistence_delta(&all_lineage_events, &seeded_node_lineages)
        });
        let upserted_file_count = upserted_paths.len();
        let removed_file_count = removed_paths.len();
        let co_change_delta_count = co_change_deltas.len();
        let validation_delta_count = validation_deltas.len();
        let workspace_tree_snapshot = match next_tree_snapshot {
            Some(snapshot) => Some(snapshot.clone()),
            None => Some(build_workspace_tree_snapshot(
                &self.root,
                self.workspace_tree_snapshot.as_ref(),
            )?),
        };
        let batch = IndexPersistBatch {
            upserted_paths,
            removed_paths,
            history_snapshot: self.history.snapshot(),
            history_delta,
            outcome_snapshot: self.outcomes.snapshot(),
            co_change_deltas,
            validation_deltas,
            projection_snapshot,
            workspace_tree_snapshot: workspace_tree_snapshot.clone(),
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
                targeted_refresh,
                refresh_scope_path_count,
                dependency_refresh_scope_path_count,
                "skipped prism index persistence batch because workspace state is unchanged"
            );
            0
        } else {
            let persist_started = Instant::now();
            self.store.commit_index_persist_batch(&self.graph, &batch)?;
            let persist_ms = persist_started.elapsed().as_millis();
            info!(
                root = %self.root.display(),
                targeted_refresh,
                refresh_scope_path_count,
                dependency_refresh_scope_path_count,
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
        if let Some(shared_runtime_store) = self.shared_runtime_store.as_mut() {
            reanchor_persisted_memory_snapshot(shared_runtime_store, &all_lineage_events)?;
        }
        let reanchor_memory_ms = reanchor_started.elapsed().as_millis();
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            lineage_event_count = all_lineage_events.len(),
            reanchor_memory_ms,
            "reanchored persisted prism memory"
        );
        self.had_prior_snapshot = true;
        self.had_projection_snapshot = true;
        self.workspace_tree_snapshot = workspace_tree_snapshot;
        info!(
            root = %self.root.display(),
            trigger = ?trigger,
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            edge_resolution_scope_path_count = resolve_edge_stats.resolution_scope_path_count,
            edge_resolution_scope_node_count = resolve_edge_stats.resolution_scope_node_count,
            cleared_derived_edge_count = resolve_edge_stats.cleared_derived_edge_count,
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
            unresolved_call_count = resolve_edge_stats.unresolved_call_count,
            unresolved_import_count = resolve_edge_stats.unresolved_import_count,
            unresolved_impl_count = resolve_edge_stats.unresolved_impl_count,
            unresolved_intent_count = resolve_edge_stats.unresolved_intent_count,
            collect_pending_ms,
            parse_apply_ms,
            remove_missing_ms,
            resolve_calls_ms = resolve_edge_stats.resolve_calls_ms,
            resolve_imports_ms = resolve_edge_stats.resolve_imports_ms,
            resolve_impls_ms = resolve_edge_stats.resolve_impls_ms,
            resolve_intents_ms = resolve_edge_stats.resolve_intents_ms,
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
        Prism::with_history_outcomes_coordination_projections_and_plan_graphs(
            self.graph,
            self.history,
            self.outcomes,
            self.coordination_snapshot,
            self.projections,
            self.plan_graphs,
            self.plan_execution_overlays,
        )
    }

    fn upsert_parsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        parse_depth: ParseDepth,
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
        self.graph.upsert_file_from_with_observed_without_rebuild(
            previous_path,
            path,
            hash,
            parse_depth,
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

    fn upsert_unparsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        parse_depth: ParseDepth,
        trigger: ChangeTrigger,
    ) -> prism_store::FileUpdate {
        self.graph.upsert_file_from_with_observed_without_rebuild(
            previous_path,
            path,
            hash,
            parse_depth,
            Vec::new(),
            Vec::new(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            &[],
            default_outcome_meta("observed"),
            trigger,
        )
    }

    fn apply_file_update(
        &mut self,
        update: prism_store::FileUpdate,
        path: &Path,
        all_lineage_events: &mut Vec<LineageEvent>,
        co_change_deltas: &mut Vec<CoChangeDelta>,
        validation_deltas: &mut Vec<ValidationDelta>,
        observed_changes: &mut Vec<ObservedChangeSet>,
        changes: &mut Vec<prism_ir::GraphChange>,
        upserted_paths: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let new_lineage_events = self.history.apply(&update.observed);
        let distinct_lineages = distinct_lineage_count(&new_lineage_events);
        let change_set_deltas = co_change_deltas_for_events(&new_lineage_events);
        if change_set_deltas.is_empty() && distinct_lineages > MAX_CO_CHANGE_LINEAGES_PER_CHANGESET
        {
            warn!(
                root = %self.root.display(),
                path = %path.display(),
                lineage_event_count = new_lineage_events.len(),
                distinct_lineage_count = distinct_lineages,
                max_co_change_lineages_per_changeset = MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
                "skipping symbol-level co-change deltas for oversized change set"
            );
        }
        self.projections.apply_lineage_events(&new_lineage_events);
        co_change_deltas.extend(change_set_deltas);
        self.outcomes.apply_lineage(&new_lineage_events)?;
        all_lineage_events.extend(new_lineage_events.iter().cloned());
        validation_deltas.extend(self.record_patch_outcome(&update.observed));
        observed_changes.push(update.observed.clone());
        changes.extend(update.changes);
        upserted_paths.push(path.to_path_buf());
        Ok(())
    }
}

fn desired_parse_depth(
    path: &Path,
    targeted_refresh: bool,
    workspace_file_count: usize,
    forced_deep_paths: Option<&HashSet<PathBuf>>,
) -> ParseDepth {
    if forced_deep_paths.is_some_and(|paths| paths.contains(path)) {
        ParseDepth::Deep
    } else if targeted_refresh || workspace_file_count <= SMALL_REPO_DEEP_PARSE_FILE_LIMIT {
        ParseDepth::Deep
    } else {
        ParseDepth::Shallow
    }
}
