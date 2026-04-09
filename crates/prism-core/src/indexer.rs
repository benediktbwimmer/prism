use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::checkpoint_materializer::CheckpointMaterializerHandle;
use crate::indexer_support::{
    build_workspace_session, collect_pending_file_parses, path_matches_refresh_scope,
    resolve_graph_edges, ResolveGraphEdgesStats,
};
use crate::invalidation::{
    edge_resolution_paths_for_dependency_keys, observed_changes_require_dependent_edge_resolution,
    RefreshInvalidationScope,
};
use crate::layout::{discover_layout, sync_root_nodes, PackageInfo, WorkspaceLayout};
use crate::memory_refresh::reanchor_persisted_memory_snapshot;
use crate::parse_pipeline::{parse_jobs_in_parallel, PreparedParseJob};
use crate::patch_outcomes::{default_outcome_meta, RecordedPatchOutcome};
use crate::projection_hydration::persisted_projection_load_plan;
use crate::protected_state::runtime_sync::{
    load_repo_protected_knowledge_for_runtime, load_repo_protected_plan_state,
    load_repo_protected_plan_state_or_default,
};
use crate::reanchor::{detect_moved_files, infer_reanchors};
use crate::repo_patch_events::merge_repo_patch_events_into_memory;
use crate::session::{
    WorkspaceRefreshSeed, WorkspaceRefreshWork, WorkspaceSession, HOT_OUTCOME_HYDRATION_LIMIT,
};
use crate::shared_runtime::{
    merged_projection_index, overlay_persisted_projection_knowledge,
    projection_snapshot_without_knowledge,
};
use crate::shared_runtime_backend::SharedRuntimeBackend;
use crate::util::{cache_path, cleanup_legacy_cache, default_adapters};
use crate::workspace_runtime_state::WorkspaceRuntimeState;
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
    ChangeTrigger, Edge, EdgeKind, EdgeOrigin, EventMeta, LineageEvent, ObservedChangeSet,
    PrismRuntimeCapabilities,
};
use prism_memory::OutcomeMemory;
use prism_parser::{LanguageAdapter, ParseDepth, ParseResult};
use prism_projections::{
    co_change_delta_batch_for_events, CoChangeDelta, IntentIndex, ProjectionIndex, ValidationDelta,
    MAX_CO_CHANGE_DELTAS_PER_CHANGESET, MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
    MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET,
};
use prism_query::Prism;
use prism_store::{
    DependencyInvalidationKeys, Graph, IndexPersistBatch, SqliteStore, Store, WorkspaceTreeSnapshot,
};
use tracing::{info, warn};

const SLOW_FILE_PHASE_THRESHOLD_MS: u128 = 200;
const SMALL_REPO_DEEP_PARSE_FILE_LIMIT: usize = 64;
const OVERSIZED_TARGETED_DEEP_PARSE_BYTE_LIMIT: usize = 128 * 1024;

fn log_truncated_co_change_fallback(
    root: &Path,
    path: &Path,
    event_count: usize,
    distinct: usize,
    sampled: usize,
) {
    warn!(
        root = %root.display(),
        path = %path.display(),
        lineage_event_count = event_count,
        distinct_lineage_count = distinct,
        sampled_lineage_count = sampled,
        max_co_change_lineages_per_changeset = MAX_CO_CHANGE_LINEAGES_PER_CHANGESET,
        max_co_change_sampled_lineages_per_changeset = MAX_CO_CHANGE_SAMPLED_LINEAGES_PER_CHANGESET,
        max_co_change_deltas_per_changeset = MAX_CO_CHANGE_DELTAS_PER_CHANGESET,
        "sampling symbol-level co-change deltas for oversized change set"
    );
}

pub struct WorkspaceIndexer<S: Store> {
    pub(crate) root: PathBuf,
    pub(crate) layout: WorkspaceLayout,
    pub(crate) graph: Graph,
    pub(crate) history: HistoryStore,
    pub(crate) outcomes: OutcomeMemory,
    pub(crate) coordination_snapshot: CoordinationSnapshot,
    pub(crate) projections: ProjectionIndex,
    pub(crate) had_prior_snapshot: bool,
    pub(crate) had_projection_snapshot: bool,
    pub(crate) adapters: Vec<Box<dyn LanguageAdapter + Send + Sync>>,
    pub(crate) store: S,
    pub(crate) checkpoint_materializer: Option<CheckpointMaterializerHandle>,
    pub(crate) workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
    pub(crate) shared_runtime: SharedRuntimeBackend,
    pub(crate) hydrate_persisted_projections: bool,
    pub(crate) hydrate_persisted_co_change: bool,
    pub(crate) coordination_enabled: bool,
    pub(crate) runtime_capabilities: PrismRuntimeCapabilities,
    pub(crate) startup_refresh: Option<WorkspaceRefreshSeed>,
    pub(crate) startup_intent: Option<IntentIndex>,
    pub(crate) trust_cached_query_state: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingFileParse {
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    pub(crate) hash: u64,
    pub(crate) previous_path: Option<PathBuf>,
}

impl WorkspaceIndexer<SqliteStore> {
    #[allow(dead_code)]
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_options(root, WorkspaceSessionOptions::default())
    }

    pub fn new_with_options(
        root: impl AsRef<Path>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let workspace_store_path = cache_path(&root)?;
        let store = SqliteStore::open(&workspace_store_path)?;
        let started = Instant::now();
        let build_local_indexer_started = Instant::now();
        let mut indexer = Self::with_store_and_options(root.clone(), store, options.clone())?;
        let build_local_indexer_ms = build_local_indexer_started.elapsed().as_millis();
        indexer.shared_runtime = options.shared_runtime.clone();
        info!(
            root = %root.display(),
            coordination_enabled = options.coordination_enabled(),
            node_count = indexer.graph.node_count(),
            edge_count = indexer.graph.edge_count(),
            file_count = indexer.graph.file_count(),
            build_local_indexer_ms,
            total_ms = started.elapsed().as_millis(),
            "finalized prism workspace indexer startup state"
        );
        Ok(indexer)
    }

    #[allow(dead_code)]
    pub(crate) fn new_from_live_prism_with_options(
        root: impl AsRef<Path>,
        prism: &Prism,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        checkpoint_materializer: Option<CheckpointMaterializerHandle>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root)?)?;
        let mut indexer = Self::with_live_prism_and_options(
            root.clone(),
            store,
            prism,
            workspace_tree_snapshot,
            checkpoint_materializer,
            options.clone(),
        )?;
        indexer.shared_runtime = options.shared_runtime.clone();
        Ok(indexer)
    }

    #[allow(dead_code)]
    pub(crate) fn with_runtime_state_and_options(
        root: impl AsRef<Path>,
        runtime_state: WorkspaceRuntimeState,
        layout: WorkspaceLayout,
        refresh_runtime_roots: bool,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        checkpoint_materializer: Option<CheckpointMaterializerHandle>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let store = SqliteStore::open(cache_path(&root)?)?;
        Self::with_runtime_state_stores_and_options(
            root,
            store,
            runtime_state,
            layout,
            refresh_runtime_roots,
            workspace_tree_snapshot,
            checkpoint_materializer,
            options,
        )
    }

    pub(crate) fn with_runtime_state_stores_and_options(
        root: impl AsRef<Path>,
        store: SqliteStore,
        runtime_state: WorkspaceRuntimeState,
        layout: WorkspaceLayout,
        refresh_runtime_roots: bool,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        checkpoint_materializer: Option<CheckpointMaterializerHandle>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        cleanup_legacy_cache(&root)?;
        let mut indexer = Self::with_live_runtime_state_and_options(
            root.clone(),
            store,
            runtime_state,
            layout,
            refresh_runtime_roots,
            workspace_tree_snapshot,
            checkpoint_materializer,
            options.clone(),
        )?;
        indexer.shared_runtime = options.shared_runtime.clone();
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
            self.hydrate_persisted_projections,
            self.hydrate_persisted_co_change,
            self.layout,
            self.graph,
            self.history,
            self.outcomes,
            self.coordination_snapshot,
            Vec::new(),
            self.projections,
            self.startup_refresh,
            self.coordination_enabled,
            self.startup_intent,
            self.trust_cached_query_state,
            self.runtime_capabilities,
            backend,
        )
    }
}

impl<S: Store> WorkspaceIndexer<S> {
    pub fn with_store(root: impl AsRef<Path>, store: S) -> Result<Self> {
        Self::with_store_and_options(root, store, WorkspaceSessionOptions::default())
    }

    #[allow(dead_code)]
    pub(crate) fn with_live_prism_and_options(
        root: impl AsRef<Path>,
        mut store: S,
        prism: &Prism,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        checkpoint_materializer: Option<CheckpointMaterializerHandle>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let started = Instant::now();
        let root = root.as_ref().canonicalize()?;
        let WorkspaceSessionOptions {
            runtime_mode,
            shared_runtime,
            hydrate_persisted_projections: _,
            hydrate_persisted_co_change: _,
            ..
        } = options;
        let runtime_capabilities = runtime_mode.capabilities();
        let coordination = runtime_capabilities.coordination_enabled();
        let knowledge_storage = runtime_capabilities.knowledge_storage_enabled();
        let layout_started = Instant::now();
        let layout = discover_layout(&root)?;
        let discover_layout_ms = layout_started.elapsed().as_millis();
        let restore_runtime_started = Instant::now();
        let mut graph = if knowledge_storage {
            Graph::from_snapshot(prism.graph().snapshot())
        } else {
            Graph::default()
        };
        if knowledge_storage {
            graph.bind_workspace_root(&root);
            sync_root_nodes(&mut graph, &layout);
        }
        let mut history = if knowledge_storage {
            HistoryStore::from_snapshot(prism.history_snapshot())
        } else {
            HistoryStore::new()
        };
        if knowledge_storage {
            history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        }
        let history_snapshot = history.snapshot();
        let outcomes = if knowledge_storage {
            let outcomes = OutcomeMemory::from_snapshot(prism.outcome_snapshot());
            merge_repo_patch_events_into_memory(&root, &outcomes)?;
            outcomes
        } else {
            OutcomeMemory::new()
        };
        let repo_knowledge =
            load_repo_protected_knowledge_for_runtime(&root, runtime_capabilities)?;
        let projections = if knowledge_storage {
            merged_projection_index(
                Some(prism.projection_snapshot()),
                None,
                repo_knowledge.curated_concepts,
                repo_knowledge.curated_contracts,
                repo_knowledge.concept_relations,
                &history_snapshot,
                &outcomes.snapshot(),
            )
        } else {
            ProjectionIndex::default()
        };
        let coordination_snapshot = if coordination {
            load_repo_protected_plan_state_or_default(&root, &mut store)?.snapshot
        } else {
            CoordinationSnapshot::default()
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
            projections,
            had_prior_snapshot: true,
            had_projection_snapshot: true,
            adapters: default_adapters(),
            store,
            checkpoint_materializer,
            workspace_tree_snapshot,
            shared_runtime,
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: true,
            coordination_enabled: coordination,
            runtime_capabilities,
            startup_refresh: None,
            startup_intent: None,
            trust_cached_query_state: false,
        })
    }

    pub(crate) fn with_live_runtime_state_and_options(
        root: impl AsRef<Path>,
        store: S,
        runtime_state: WorkspaceRuntimeState,
        layout: WorkspaceLayout,
        refresh_runtime_roots: bool,
        workspace_tree_snapshot: Option<WorkspaceTreeSnapshot>,
        checkpoint_materializer: Option<CheckpointMaterializerHandle>,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let started = Instant::now();
        let root = root.as_ref().canonicalize()?;
        let WorkspaceSessionOptions {
            runtime_mode,
            shared_runtime,
            hydrate_persisted_projections: _,
            hydrate_persisted_co_change: _,
            ..
        } = options;
        let runtime_capabilities = runtime_mode.capabilities();
        let coordination = runtime_capabilities.coordination_enabled();
        let knowledge_storage = runtime_capabilities.knowledge_storage_enabled();
        let restore_runtime_started = Instant::now();
        let WorkspaceRuntimeState {
            layout: _cached_layout,
            mut graph,
            mut history,
            mut outcomes,
            coordination_snapshot,
            canonical_snapshot_v2: _,
            runtime_descriptors: _,
            mut projections,
            runtime_capabilities: _,
        } = runtime_state;
        if knowledge_storage {
            merge_repo_patch_events_into_memory(&root, &outcomes)?;
        } else {
            graph = Arc::new(Graph::default());
            history = Arc::new(HistoryStore::default());
            outcomes = Arc::new(OutcomeMemory::default());
            projections = ProjectionIndex::default();
        }
        if refresh_runtime_roots && knowledge_storage {
            let workspace_id = sync_root_nodes(Arc::make_mut(&mut graph), &layout);
            Arc::make_mut(&mut history).seed_nodes(
                std::iter::once(workspace_id).chain(
                    layout
                        .packages
                        .iter()
                        .map(|package| package.node_id.clone()),
                ),
            );
        }
        let restore_runtime_ms = restore_runtime_started.elapsed().as_millis();

        info!(
            root = %root.display(),
            coordination_enabled = coordination,
            node_count = graph.node_count(),
            edge_count = graph.edge_count(),
            file_count = graph.file_count(),
            layout_source = if refresh_runtime_roots { "rediscovered" } else { "cached" },
            restore_runtime_ms,
            total_ms = started.elapsed().as_millis(),
            "prepared prism workspace indexer from mutable runtime state"
        );

        Ok(Self {
            root,
            layout,
            graph: Arc::unwrap_or_clone(graph),
            history: Arc::unwrap_or_clone(history),
            outcomes: Arc::unwrap_or_clone(outcomes),
            coordination_snapshot: if coordination {
                coordination_snapshot
            } else {
                CoordinationSnapshot::default()
            },
            projections,
            had_prior_snapshot: true,
            had_projection_snapshot: true,
            adapters: default_adapters(),
            store,
            checkpoint_materializer,
            workspace_tree_snapshot,
            shared_runtime,
            hydrate_persisted_projections: false,
            hydrate_persisted_co_change: true,
            coordination_enabled: coordination,
            runtime_capabilities,
            startup_refresh: None,
            startup_intent: None,
            trust_cached_query_state: false,
        })
    }

    pub fn with_store_and_options(
        root: impl AsRef<Path>,
        mut store: S,
        options: WorkspaceSessionOptions,
    ) -> Result<Self> {
        let started = Instant::now();
        let root = root.as_ref().canonicalize()?;
        let runtime_capabilities = options.runtime_capabilities();
        let coordination_enabled = options.coordination_enabled();
        let knowledge_storage = runtime_capabilities.knowledge_storage_enabled();
        let layout_started = Instant::now();
        let layout = discover_layout(&root)?;
        let discover_layout_ms = layout_started.elapsed().as_millis();
        let load_graph_started = Instant::now();
        let stored_graph = if knowledge_storage {
            store.load_graph()?
        } else {
            None
        };
        let load_graph_ms = load_graph_started.elapsed().as_millis();
        let had_prior_snapshot = stored_graph.is_some();
        let mut graph = stored_graph.unwrap_or_default();
        if knowledge_storage {
            graph.bind_workspace_root(&root);
            sync_root_nodes(&mut graph, &layout);
            resolve_graph_edges(&mut graph, None);
        }
        let load_projection_started = Instant::now();
        let projection_metadata = if knowledge_storage {
            store.load_projection_materialization_metadata()?
        } else {
            Default::default()
        };
        let persisted_projection_snapshot = if knowledge_storage {
            if options.hydrate_persisted_projections {
                store.load_projection_snapshot()?
            } else if options.hydrate_persisted_co_change {
                store.load_projection_snapshot()?
            } else {
                store.load_projection_snapshot_without_co_change()?
            }
        } else {
            None
        };
        let load_plan = persisted_projection_load_plan(
            projection_metadata,
            options.hydrate_persisted_projections && knowledge_storage,
            options.hydrate_persisted_co_change && knowledge_storage,
        );
        let workspace_tree_snapshot = store.load_workspace_tree_snapshot()?;
        let base_projection_snapshot = if knowledge_storage {
            persisted_projection_snapshot.clone().map(|snapshot| {
                if options.hydrate_persisted_projections {
                    snapshot
                } else {
                    projection_snapshot_without_knowledge(snapshot)
                }
            })
        } else {
            None
        };
        let load_projection_ms = load_projection_started.elapsed().as_millis();
        let load_history_started = Instant::now();
        let mut history = if knowledge_storage {
            store
                .load_history_snapshot_with_options(load_plan.load_history_events)?
                .map(HistoryStore::from_snapshot)
                .unwrap_or_else(HistoryStore::new)
        } else {
            HistoryStore::new()
        };
        let load_history_ms = load_history_started.elapsed().as_millis();
        if knowledge_storage {
            history.seed_nodes(graph.all_nodes().map(|node| node.id.clone()));
        }
        let load_outcomes_started = Instant::now();
        let outcomes = if knowledge_storage {
            let outcomes = if load_plan.load_full_outcomes {
                store.load_outcome_snapshot()?
            } else {
                store.load_recent_outcome_snapshot(HOT_OUTCOME_HYDRATION_LIMIT)?
            }
            .map(OutcomeMemory::from_snapshot)
            .unwrap_or_else(OutcomeMemory::new);
            merge_repo_patch_events_into_memory(&root, &outcomes)?;
            outcomes
        } else {
            OutcomeMemory::new()
        };
        let load_outcomes_ms = load_outcomes_started.elapsed().as_millis();
        let load_coordination_started = Instant::now();
        let plan_state = if coordination_enabled {
            load_repo_protected_plan_state(&root, &mut store)?
        } else {
            None
        };
        let coordination_snapshot = plan_state
            .as_ref()
            .map(|state| state.snapshot.clone())
            .unwrap_or_default();
        let load_coordination_ms = load_coordination_started.elapsed().as_millis();
        let had_projection_snapshot = load_plan.had_complete_derived_snapshot;
        let derive_projection_started = Instant::now();
        let repo_knowledge =
            load_repo_protected_knowledge_for_runtime(&root, runtime_capabilities)?;
        let protected_knowledge_work = if knowledge_storage {
            protected_knowledge_recovery_work(&repo_knowledge)?
        } else {
            WorkspaceRefreshWork::default()
        };
        let projections = if knowledge_storage {
            let mut projections = merged_projection_index(
                base_projection_snapshot,
                None,
                repo_knowledge.curated_concepts,
                repo_knowledge.curated_contracts,
                repo_knowledge.concept_relations,
                &history.snapshot(),
                &outcomes.snapshot(),
            );
            if !options.hydrate_persisted_projections {
                overlay_persisted_projection_knowledge(
                    &mut projections,
                    persisted_projection_snapshot.into_iter(),
                );
            }
            projections
        } else {
            ProjectionIndex::default()
        };
        let derive_or_restore_projection_ms = derive_projection_started.elapsed().as_millis();
        let startup_refresh = if had_prior_snapshot || plan_state.is_some() {
            Some(WorkspaceRefreshSeed {
                path: "recovery",
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                work: workspace_recovery_work(
                    &graph,
                    &history,
                    &outcomes,
                    protected_knowledge_work,
                    &coordination_snapshot,
                )?
                .with_workspace_reloaded(true),
            })
        } else {
            None
        };

        info!(
            root = %root.display(),
            coordination_enabled = options.coordination_enabled(),
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
            projections,
            had_prior_snapshot,
            had_projection_snapshot,
            adapters: default_adapters(),
            store,
            checkpoint_materializer: None,
            workspace_tree_snapshot,
            shared_runtime: options.shared_runtime,
            hydrate_persisted_projections: options.hydrate_persisted_projections,
            hydrate_persisted_co_change: options.hydrate_persisted_co_change,
            coordination_enabled,
            runtime_capabilities,
            startup_refresh,
            startup_intent: None,
            trust_cached_query_state: false,
        })
    }

    pub fn index(&mut self) -> Result<()> {
        let _ = self.index_with_observed_changes()?;
        Ok(())
    }

    pub fn index_with_changes(&mut self) -> Result<Vec<prism_ir::GraphChange>> {
        let (_, changes) = self.index_impl(
            ChangeTrigger::ManualReindex,
            None,
            None,
            None,
            default_outcome_meta("observed"),
        )?;
        Ok(changes)
    }

    pub fn index_with_observed_changes(&mut self) -> Result<Vec<ObservedChangeSet>> {
        self.index_with_trigger(ChangeTrigger::ManualReindex)
    }

    pub fn index_with_trigger(&mut self, trigger: ChangeTrigger) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) =
            self.index_impl(trigger, None, None, None, default_outcome_meta("observed"))?;
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
            default_outcome_meta("observed"),
        )?;
        Ok(observed)
    }

    pub(crate) fn index_with_refresh_plan_and_meta(
        &mut self,
        trigger: ChangeTrigger,
        plan: &WorkspaceRefreshPlan,
        observed_meta: EventMeta,
    ) -> Result<Vec<ObservedChangeSet>> {
        let (observed, _) = self.index_impl(
            trigger,
            Some(plan),
            Some(&plan.next_snapshot),
            None,
            observed_meta,
        )?;
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
            default_outcome_meta("observed"),
        )?;
        Ok(observed)
    }

    fn index_impl(
        &mut self,
        trigger: ChangeTrigger,
        refresh_plan: Option<&WorkspaceRefreshPlan>,
        next_tree_snapshot: Option<&WorkspaceTreeSnapshot>,
        forced_deep_paths: Option<&HashSet<PathBuf>>,
        observed_meta: EventMeta,
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
        let mut outcome_events = Vec::new();
        let mut co_change_deltas = Vec::<CoChangeDelta>::new();
        let mut validation_deltas = Vec::<ValidationDelta>::new();
        let mut requires_graph_index_rebuild = false;
        let mut requires_edge_resolution = false;
        let mut dependency_invalidation_keys = DependencyInvalidationKeys::default();
        let mut upserted_paths = Vec::<PathBuf>::new();
        let mut in_place_upserted_paths = Vec::<PathBuf>::new();
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
                pending_file.source.len(),
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
                observed_meta.clone(),
                trigger.clone(),
            );
            requires_graph_index_rebuild |= update.requires_index_rebuild;
            requires_edge_resolution |= update.requires_edge_resolution;
            dependency_invalidation_keys.extend_from(&update.dependency_invalidation_keys);
            let upsert_ms = upsert_started.elapsed().as_millis();
            parsed_file_count += 1;
            let new_lineage_events = self.history.apply(&update.observed);
            let change_set_deltas = co_change_delta_batch_for_events(&new_lineage_events);
            if change_set_deltas.truncated {
                log_truncated_co_change_fallback(
                    &self.root,
                    &parsed_job.pending.path,
                    new_lineage_events.len(),
                    change_set_deltas.distinct_lineage_count,
                    change_set_deltas.sampled_lineage_count,
                );
            }
            self.projections.apply_lineage_events_with_co_change_deltas(
                &new_lineage_events,
                &change_set_deltas.deltas,
            );
            co_change_deltas.extend(change_set_deltas.deltas);
            self.outcomes.apply_lineage(&new_lineage_events)?;
            all_lineage_events.extend(new_lineage_events.iter().cloned());
            if let Some(RecordedPatchOutcome {
                event,
                validation_deltas: patch_validation_deltas,
            }) = self.record_patch_outcome(&update.observed)
            {
                outcome_events.push(event);
                validation_deltas.extend(patch_validation_deltas);
            }
            observed_changes.push(update.observed.clone());
            changes.extend(update.changes);
            if update.persist_in_place {
                in_place_upserted_paths.push(parsed_job.pending.path.clone());
            } else {
                upserted_paths.push(parsed_job.pending.path.clone());
            }
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
                    pending_file.source.len(),
                    forced_deep_paths,
                ),
                observed_meta.clone(),
                trigger.clone(),
            );
            requires_graph_index_rebuild |= update.requires_index_rebuild;
            requires_edge_resolution |= update.requires_edge_resolution;
            self.apply_file_update(
                update,
                &pending_file.path,
                &mut all_lineage_events,
                &mut outcome_events,
                &mut co_change_deltas,
                &mut validation_deltas,
                &mut observed_changes,
                &mut changes,
                &mut upserted_paths,
                &mut in_place_upserted_paths,
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
        for tracked in self.graph.runtime_tracked_files() {
            if refresh_scope
                .as_ref()
                .is_some_and(|scope| !path_matches_refresh_scope(&tracked, scope))
            {
                continue;
            }
            if !seen_files.contains(&tracked) && !moved_paths.contains(&tracked) {
                let update = self.graph.remove_file_with_observed_without_rebuild(
                    &tracked,
                    observed_meta.clone(),
                    trigger.clone(),
                );
                requires_graph_index_rebuild |= update.requires_index_rebuild;
                requires_edge_resolution |= update.requires_edge_resolution;
                let new_lineage_events = self.history.apply(&update.observed);
                let change_set_deltas = co_change_delta_batch_for_events(&new_lineage_events);
                if change_set_deltas.truncated {
                    log_truncated_co_change_fallback(
                        &self.root,
                        &tracked,
                        new_lineage_events.len(),
                        change_set_deltas.distinct_lineage_count,
                        change_set_deltas.sampled_lineage_count,
                    );
                }
                self.projections.apply_lineage_events(&new_lineage_events);
                co_change_deltas.extend(change_set_deltas.deltas);
                self.outcomes.apply_lineage(&new_lineage_events)?;
                all_lineage_events.extend(new_lineage_events.iter().cloned());
                if let Some(RecordedPatchOutcome {
                    event,
                    validation_deltas: patch_validation_deltas,
                }) = self.record_patch_outcome(&update.observed)
                {
                    outcome_events.push(event);
                    validation_deltas.extend(patch_validation_deltas);
                }
                observed_changes.push(update.observed.clone());
                changes.extend(update.changes);
                removed_paths.push(tracked.clone());
            }
        }
        let remove_missing_ms = remove_missing_started.elapsed().as_millis();
        let workspace_tree_snapshot_started = Instant::now();
        let workspace_tree_snapshot = match next_tree_snapshot {
            Some(snapshot) => Some(snapshot.clone()),
            None => Some(build_workspace_tree_snapshot(
                &self.root,
                self.workspace_tree_snapshot.as_ref(),
            )?),
        };
        let workspace_tree_snapshot_ms = workspace_tree_snapshot_started.elapsed().as_millis();
        if observed_changes.is_empty()
            && changes.is_empty()
            && upserted_paths.is_empty()
            && in_place_upserted_paths.is_empty()
            && removed_paths.is_empty()
        {
            self.had_prior_snapshot = true;
            self.had_projection_snapshot = true;
            self.workspace_tree_snapshot = workspace_tree_snapshot;
            info!(
                root = %self.root.display(),
                targeted_refresh,
                refresh_scope_path_count,
                dependency_refresh_scope_path_count,
                pending_file_count,
                pending_bytes,
                seen_file_count = seen_files.len(),
                moved_file_count,
                skipped_unchanged_count,
                collect_pending_ms,
                parse_apply_ms,
                remove_missing_ms,
                rebuild_graph_indexes_ms = 0,
                workspace_tree_snapshot_ms,
                seed_node_lineages_ms = 0,
                projection_snapshot_ms = 0,
                history_delta_ms = 0,
                build_persist_batch_ms = 0,
                total_ms = started.elapsed().as_millis(),
                "skipped downstream prism indexing phases because workspace state is unchanged"
            );
            info!(
                root = %self.root.display(),
                trigger = ?trigger,
                targeted_refresh,
                refresh_scope_path_count,
                dependency_refresh_scope_path_count,
                edge_resolution_scope_path_count = 0,
                edge_resolution_scope_node_count = 0,
                cleared_derived_edge_count = 0,
                pending_file_count,
                pending_bytes,
                seen_file_count = seen_files.len(),
                moved_file_count,
                skipped_unchanged_count,
                upserted_file_count = 0,
                removed_file_count = 0,
                observed_change_sets = 0,
                graph_changes = 0,
                lineage_event_count = 0,
                co_change_delta_count = 0,
                validation_delta_count = 0,
                persist_skipped = true,
                node_count = self.graph.node_count(),
                edge_count = self.graph.edge_count(),
                file_count = self.graph.file_count(),
                unresolved_call_count = 0,
                unresolved_import_count = 0,
                unresolved_impl_count = 0,
                unresolved_intent_count = 0,
                collect_pending_ms,
                parse_apply_ms,
                remove_missing_ms,
                rebuild_graph_indexes_ms = 0,
                workspace_tree_snapshot_ms,
                resolve_calls_ms = 0,
                resolve_imports_ms = 0,
                resolve_impls_ms = 0,
                resolve_intents_ms = 0,
                resolve_edges_ms = 0,
                seed_node_lineages_ms = 0,
                projection_snapshot_ms = 0,
                history_delta_ms = 0,
                build_persist_batch_ms = 0,
                shared_outcome_append_ms = 0,
                materialize_enqueue_ms = 0,
                persist_ms = 0,
                reanchor_memory_ms = 0,
                total_ms = started.elapsed().as_millis(),
                "completed prism workspace indexing"
            );
            return Ok((observed_changes, changes));
        }
        let rebuild_graph_indexes_ms = if requires_graph_index_rebuild {
            let rebuild_graph_indexes_started = Instant::now();
            self.graph.rebuild_indexes();
            rebuild_graph_indexes_started.elapsed().as_millis()
        } else {
            0
        };
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

        let expand_dependency_edge_resolution = requires_edge_resolution
            && !dependency_invalidation_keys.is_empty()
            && observed_changes_require_dependent_edge_resolution(&observed_changes);
        let resolved_edge_scope = if requires_edge_resolution {
            invalidation_scope.as_ref().map(|scope| {
                if expand_dependency_edge_resolution {
                    edge_resolution_paths_for_dependency_keys(
                        &self.graph,
                        &scope.dependency_paths,
                        &dependency_invalidation_keys,
                    )
                } else {
                    scope.direct_paths.clone()
                }
            })
        } else {
            None
        };
        let edge_resolution_scope = resolved_edge_scope.as_ref();
        let (resolve_edge_stats, resolve_edges_ms) = if requires_edge_resolution {
            let resolve_edges_started = Instant::now();
            let stats = resolve_graph_edges(&mut self.graph, edge_resolution_scope);
            (stats, resolve_edges_started.elapsed().as_millis())
        } else {
            (ResolveGraphEdgesStats::default(), 0)
        };
        info!(
            root = %self.root.display(),
            targeted_refresh,
            refresh_scope_path_count,
            dependency_refresh_scope_path_count,
            expand_dependency_edge_resolution,
            edge_resolution_scope_path_count = resolve_edge_stats.resolution_scope_path_count,
            edge_resolution_scope_node_count = resolve_edge_stats.resolution_scope_node_count,
            cleared_derived_edge_count = resolve_edge_stats.cleared_derived_edge_count,
            node_count = self.graph.node_count(),
            edge_count = self.graph.edge_count(),
            unresolved_call_count = resolve_edge_stats.unresolved_call_count,
            unresolved_import_count = resolve_edge_stats.unresolved_import_count,
            unresolved_impl_count = resolve_edge_stats.unresolved_impl_count,
            unresolved_intent_count = resolve_edge_stats.unresolved_intent_count,
            collect_scope_nodes_ms = resolve_edge_stats.collect_scope_nodes_ms,
            clear_derived_edges_ms = resolve_edge_stats.clear_derived_edges_ms,
            collect_unresolved_ms = resolve_edge_stats.collect_unresolved_ms,
            resolve_calls_ms = resolve_edge_stats.resolve_calls_ms,
            resolve_imports_ms = resolve_edge_stats.resolve_imports_ms,
            resolve_impls_ms = resolve_edge_stats.resolve_impls_ms,
            resolve_intents_ms = resolve_edge_stats.resolve_intents_ms,
            extend_edges_ms = resolve_edge_stats.extend_edges_ms,
            resolve_edges_ms,
            "finished prism edge resolution phase"
        );
        let seed_node_lineages_started = Instant::now();
        let seeded_node_lineages = self
            .history
            .seed_nodes(self.graph.all_nodes().map(|node| node.id.clone()));
        let seed_node_lineages_ms = seed_node_lineages_started.elapsed().as_millis();
        let projection_snapshot_started = Instant::now();
        let projection_snapshot =
            (!self.had_projection_snapshot).then(|| self.projections.snapshot());
        let projection_snapshot_ms = projection_snapshot_started.elapsed().as_millis();
        let history_delta_started = Instant::now();
        let history_delta = self.had_prior_snapshot.then(|| {
            self.history
                .persistence_delta(&all_lineage_events, &seeded_node_lineages)
        });
        let history_delta_ms = history_delta_started.elapsed().as_millis();
        let upserted_file_count = upserted_paths.len() + in_place_upserted_paths.len();
        let removed_file_count = removed_paths.len();
        let co_change_delta_count = co_change_deltas.len();
        let validation_delta_count = validation_deltas.len();
        let deferred_materializer = self.checkpoint_materializer.clone();
        let build_persist_batch_started = Instant::now();
        let local_outcome_snapshot = if outcome_events.is_empty() {
            None
        } else {
            Some(self.outcomes.snapshot())
        };
        let local_batch = IndexPersistBatch {
            upserted_paths,
            in_place_upserted_paths,
            removed_paths,
            history_snapshot: if history_delta.is_some() {
                None
            } else {
                Some(self.history.snapshot())
            },
            history_delta,
            outcome_snapshot: local_outcome_snapshot,
            outcome_events,
            defer_graph_materialization: deferred_materializer.is_some(),
            co_change_deltas: if deferred_materializer.is_some() {
                Vec::new()
            } else {
                co_change_deltas.clone()
            },
            validation_deltas: if deferred_materializer.is_some() {
                Vec::new()
            } else {
                validation_deltas.clone()
            },
            projection_snapshot: if deferred_materializer.is_some() {
                None
            } else {
                projection_snapshot.clone()
            },
            workspace_tree_snapshot: if deferred_materializer.is_some() {
                None
            } else {
                workspace_tree_snapshot.clone()
            },
        };
        let build_persist_batch_ms = build_persist_batch_started.elapsed().as_millis();
        let skip_persist = self.had_prior_snapshot
            && self.had_projection_snapshot
            && local_batch.upserted_paths.is_empty()
            && local_batch.in_place_upserted_paths.is_empty()
            && local_batch.removed_paths.is_empty()
            && local_batch.co_change_deltas.is_empty()
            && local_batch.validation_deltas.is_empty()
            && local_batch.projection_snapshot.is_none();
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
            self.store
                .commit_index_persist_batch(&self.graph, &local_batch)?;
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
        let shared_outcome_append_ms = 0_u128;
        let mut materialize_enqueue_ms = 0;
        if let Some(materializer) = deferred_materializer {
            let materialize_started = Instant::now();
            let graph_result = materializer.enqueue_graph_snapshot(self.graph.snapshot());
            let projection_result = if let Some(snapshot) = projection_snapshot.clone() {
                materializer.enqueue_projection_snapshot(snapshot)
            } else {
                materializer
                    .enqueue_projection_deltas(co_change_deltas.clone(), validation_deltas.clone())
            };
            let tree_result = workspace_tree_snapshot
                .clone()
                .map(|snapshot| materializer.enqueue_workspace_tree_snapshot(snapshot))
                .unwrap_or(Ok(()));
            let enqueue_result = graph_result.and(projection_result).and(tree_result);
            if let Err(error) = enqueue_result {
                self.store.save_graph_snapshot(&self.graph)?;
                if let Some(snapshot) = projection_snapshot.as_ref() {
                    self.store.save_projection_snapshot(snapshot)?;
                } else {
                    self.store
                        .apply_projection_deltas(&co_change_deltas, &validation_deltas)?;
                }
                if let Some(snapshot) = workspace_tree_snapshot.as_ref() {
                    self.store.save_workspace_tree_snapshot(snapshot)?;
                }
                warn!(
                    root = %self.root.display(),
                    error = %error,
                    materialize_ms = materialize_started.elapsed().as_millis(),
                    "failed to enqueue prism index materializations; fell back to synchronous persistence"
                );
                materialize_enqueue_ms = materialize_started.elapsed().as_millis();
            } else {
                materialize_enqueue_ms = materialize_started.elapsed().as_millis();
                info!(
                    root = %self.root.display(),
                    targeted_refresh,
                    refresh_scope_path_count,
                    dependency_refresh_scope_path_count,
                    co_change_delta_count,
                    validation_delta_count,
                    materialize_ms = materialize_started.elapsed().as_millis(),
                    "deferred prism index graph, projection, and workspace-tree materializations"
                );
            }
        }
        let reanchor_started = Instant::now();
        let local_reanchor_result =
            if let Some(materializer) = self.checkpoint_materializer.as_ref() {
                materializer.enqueue_episodic_reanchor_events(all_lineage_events.clone())
            } else {
                reanchor_persisted_memory_snapshot(&mut self.store, &all_lineage_events)
            };
        if let Err(error) = local_reanchor_result {
            reanchor_persisted_memory_snapshot(&mut self.store, &all_lineage_events)?;
            warn!(
                root = %self.root.display(),
                error = %error,
                "failed to enqueue episodic reanchor for workspace store; fell back to synchronous persistence"
            );
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
            rebuild_graph_indexes_ms,
            workspace_tree_snapshot_ms,
            resolve_calls_ms = resolve_edge_stats.resolve_calls_ms,
            resolve_imports_ms = resolve_edge_stats.resolve_imports_ms,
            resolve_impls_ms = resolve_edge_stats.resolve_impls_ms,
            resolve_intents_ms = resolve_edge_stats.resolve_intents_ms,
            resolve_edges_ms,
            seed_node_lineages_ms,
            projection_snapshot_ms,
            history_delta_ms,
            build_persist_batch_ms,
            shared_outcome_append_ms,
            materialize_enqueue_ms,
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
        let canonical_snapshot_v2 = self.coordination_snapshot.to_canonical_snapshot_v2();
        Prism::with_history_outcomes_coordination_and_projections_v2(
            self.graph,
            self.history,
            self.outcomes,
            self.coordination_snapshot,
            canonical_snapshot_v2,
            self.projections,
            Vec::new(),
        )
    }

    pub(crate) fn into_runtime_state(self) -> WorkspaceRuntimeState {
        WorkspaceRuntimeState::new(
            self.layout,
            self.graph,
            self.history,
            self.outcomes,
            self.coordination_snapshot,
            Vec::new(),
            self.projections,
            self.runtime_capabilities,
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
        observed_meta: EventMeta,
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
            observed_meta,
            trigger,
        )
    }

    fn upsert_unparsed_file(
        &mut self,
        previous_path: Option<&Path>,
        path: &Path,
        hash: u64,
        parse_depth: ParseDepth,
        observed_meta: EventMeta,
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
            observed_meta,
            trigger,
        )
    }

    fn apply_file_update(
        &mut self,
        update: prism_store::FileUpdate,
        path: &Path,
        all_lineage_events: &mut Vec<LineageEvent>,
        outcome_events: &mut Vec<prism_memory::OutcomeEvent>,
        co_change_deltas: &mut Vec<CoChangeDelta>,
        validation_deltas: &mut Vec<ValidationDelta>,
        observed_changes: &mut Vec<ObservedChangeSet>,
        changes: &mut Vec<prism_ir::GraphChange>,
        upserted_paths: &mut Vec<PathBuf>,
        in_place_upserted_paths: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let new_lineage_events = self.history.apply(&update.observed);
        let change_set_deltas = co_change_delta_batch_for_events(&new_lineage_events);
        if change_set_deltas.truncated {
            log_truncated_co_change_fallback(
                &self.root,
                path,
                new_lineage_events.len(),
                change_set_deltas.distinct_lineage_count,
                change_set_deltas.sampled_lineage_count,
            );
        }
        self.projections.apply_lineage_events_with_co_change_deltas(
            &new_lineage_events,
            &change_set_deltas.deltas,
        );
        co_change_deltas.extend(change_set_deltas.deltas);
        self.outcomes.apply_lineage(&new_lineage_events)?;
        all_lineage_events.extend(new_lineage_events.iter().cloned());
        if let Some(RecordedPatchOutcome {
            event,
            validation_deltas: patch_validation_deltas,
        }) = self.record_patch_outcome(&update.observed)
        {
            outcome_events.push(event);
            validation_deltas.extend(patch_validation_deltas);
        }
        observed_changes.push(update.observed.clone());
        changes.extend(update.changes);
        if update.persist_in_place {
            in_place_upserted_paths.push(path.to_path_buf());
        } else {
            upserted_paths.push(path.to_path_buf());
        }
        Ok(())
    }
}

impl WorkspaceRefreshWork {
    fn with_workspace_reloaded(self, workspace_reloaded: bool) -> Self {
        Self {
            workspace_reloaded,
            ..self
        }
    }

    fn saturating_add(self, other: Self) -> Self {
        Self {
            loaded_bytes: self.loaded_bytes.saturating_add(other.loaded_bytes),
            replay_volume: self.replay_volume.saturating_add(other.replay_volume),
            full_rebuild_count: self
                .full_rebuild_count
                .saturating_add(other.full_rebuild_count),
            workspace_reloaded: self.workspace_reloaded || other.workspace_reloaded,
        }
    }
}

pub(crate) fn workspace_recovery_work(
    graph: &Graph,
    history: &HistoryStore,
    outcomes: &OutcomeMemory,
    protected_knowledge_work: WorkspaceRefreshWork,
    coordination_snapshot: &CoordinationSnapshot,
) -> Result<WorkspaceRefreshWork> {
    Ok(graph_recovery_work(graph)?
        .saturating_add(history_recovery_work(history)?)
        .saturating_add(outcomes_recovery_work(outcomes)?)
        .saturating_add(protected_knowledge_work)
        .saturating_add(coordination_recovery_work(coordination_snapshot)?))
}

pub(crate) fn protected_knowledge_recovery_work(
    protected_knowledge: &crate::protected_state::runtime_sync::RepoProtectedKnowledge,
) -> Result<WorkspaceRefreshWork> {
    Ok(WorkspaceRefreshWork {
        loaded_bytes: serialized_size(protected_knowledge)?,
        replay_volume: u64::try_from(
            protected_knowledge
                .curated_concepts
                .len()
                .saturating_add(protected_knowledge.curated_contracts.len())
                .saturating_add(protected_knowledge.concept_relations.len()),
        )
        .unwrap_or(u64::MAX),
        ..WorkspaceRefreshWork::default()
    })
}

fn graph_recovery_work(graph: &Graph) -> Result<WorkspaceRefreshWork> {
    Ok(WorkspaceRefreshWork {
        loaded_bytes: serialized_size(&graph.snapshot())?,
        replay_volume: u64::try_from(
            graph
                .node_count()
                .saturating_add(graph.edge_count())
                .saturating_add(graph.file_count()),
        )
        .unwrap_or(u64::MAX),
        ..WorkspaceRefreshWork::default()
    })
}

fn history_recovery_work(history: &HistoryStore) -> Result<WorkspaceRefreshWork> {
    let snapshot = history.snapshot();
    Ok(WorkspaceRefreshWork {
        loaded_bytes: serialized_size(&snapshot)?,
        replay_volume: u64::try_from(
            snapshot
                .node_to_lineage
                .len()
                .saturating_add(snapshot.events.len())
                .saturating_add(snapshot.tombstones.len()),
        )
        .unwrap_or(u64::MAX),
        ..WorkspaceRefreshWork::default()
    })
}

fn outcomes_recovery_work(outcomes: &OutcomeMemory) -> Result<WorkspaceRefreshWork> {
    let snapshot = outcomes.snapshot();
    Ok(WorkspaceRefreshWork {
        loaded_bytes: serialized_size(&snapshot)?,
        replay_volume: u64::try_from(snapshot.events.len()).unwrap_or(u64::MAX),
        ..WorkspaceRefreshWork::default()
    })
}

fn coordination_recovery_work(
    coordination_snapshot: &CoordinationSnapshot,
) -> Result<WorkspaceRefreshWork> {
    Ok(WorkspaceRefreshWork {
        loaded_bytes: serialized_size(coordination_snapshot)?,
        replay_volume: u64::try_from(
            coordination_snapshot
                .plans
                .len()
                .saturating_add(coordination_snapshot.tasks.len())
                .saturating_add(coordination_snapshot.claims.len())
                .saturating_add(coordination_snapshot.artifacts.len())
                .saturating_add(coordination_snapshot.reviews.len())
                .saturating_add(coordination_snapshot.events.len()),
        )
        .unwrap_or(u64::MAX),
        ..WorkspaceRefreshWork::default()
    })
}

fn serialized_size<T: Debug + ?Sized>(value: &T) -> Result<u64> {
    Ok(u64::try_from(format!("{value:?}").len()).unwrap_or(u64::MAX))
}

fn desired_parse_depth(
    path: &Path,
    targeted_refresh: bool,
    workspace_file_count: usize,
    source_bytes: usize,
    forced_deep_paths: Option<&HashSet<PathBuf>>,
) -> ParseDepth {
    if forced_deep_paths.is_some_and(|paths| paths.contains(path)) {
        ParseDepth::Deep
    } else if workspace_file_count <= SMALL_REPO_DEEP_PARSE_FILE_LIMIT {
        ParseDepth::Deep
    } else if targeted_refresh && source_bytes <= OVERSIZED_TARGETED_DEEP_PARSE_BYTE_LIMIT {
        ParseDepth::Deep
    } else {
        ParseDepth::Shallow
    }
}
