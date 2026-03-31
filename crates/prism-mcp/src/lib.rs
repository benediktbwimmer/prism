use anyhow::{anyhow, Result};
use clap::{ArgAction, ValueEnum};
use prism_agent::InferenceStore;
use prism_core::{
    hydrate_workspace_session_with_options, SharedRuntimeBackend, WorkspaceSession,
    WorkspaceSessionOptions,
};
use prism_ir::TaskId;
use prism_js::{api_reference_markdown, CuratorJobView, API_REFERENCE_URI};
use prism_memory::{EpisodicMemorySnapshot, OutcomeEvent, SessionMemory};
use prism_query::{Prism, QueryLimits};
use rmcp::{
    handler::server::router::tool::ToolRouter,
    service::{RoleServer, RunningService, ServerInitializeError},
    transport::{stdio, IntoTransport},
    ServiceExt,
};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tracing::{debug, info, Level};

mod ambiguity;
mod capabilities_resource;
mod change_views;
mod common;
mod compact_followups;
mod compact_tools;
mod concept_resolution;
mod daemon_mode;
mod dashboard_events;
mod dashboard_read_models;
mod dashboard_router;
mod dashboard_types;
mod diagnostics;
mod discovery_bundle;
mod discovery_helpers;
mod features;
mod file_queries;
mod host_mutations;
mod host_resources;
mod js_runtime;
mod lineage_views;
mod logging;
mod mcp_call_log;
mod memory_metadata;
mod process_lifecycle;
mod proxy_server;
mod query_errors;
mod query_helpers;
mod query_log;
mod query_runtime;
mod query_typecheck;
mod query_types;
mod query_view_after_edit;
mod query_view_command_memory;
mod query_view_impact;
mod query_view_materialization;
mod query_view_playbook;
mod query_view_validation_plan;
mod query_views;
mod refresh_phases;
mod request_envelope;
mod resource_schemas;
mod resource_trace;
mod resources;
mod runtime_state;
mod runtime_views;
mod schema_examples;
mod semantic_contexts;
mod server_surface;
mod session_seed;
mod session_state;
mod slow_call_snapshot;
mod spec_insights;
mod suggested_queries;
mod task_journal;
mod text_search;
mod tool_args;
mod tool_schemas;
mod ui_assets;
mod ui_read_models;
mod ui_router;
mod ui_types;
mod views;
mod vocab_resource;
mod vocabulary;
mod workspace_runtime;

use ambiguity::*;
use capabilities_resource::*;
use change_views::*;
use common::*;
use concept_resolution::*;
pub use daemon_mode::serve_with_mode;
use dashboard_events::*;
use diagnostics::*;
use discovery_bundle::*;
use discovery_helpers::*;
pub use features::{CoordinationFeatureFlag, PrismMcpFeatures, QueryViewFeatureFlag};
use js_runtime::JsWorkerPool;
use lineage_views::*;
pub use logging::{init_logging, log_process_start, log_top_level_error};
use mcp_call_log::*;
use memory_metadata::*;
pub use process_lifecycle::maybe_daemonize_process;
use query_errors::*;
use query_helpers::*;
use query_log::*;
use query_runtime::*;
use query_types::*;
use request_envelope::*;
use resource_schemas::*;
use resources::*;
use runtime_state::*;
use schema_examples::*;
use semantic_contexts::*;
use session_seed::{
    load_session_seed, persist_session_seed, restore_session_seed, PersistedSessionSeed,
};
use session_state::SessionState;
use spec_insights::*;
use suggested_queries::*;
use task_journal::*;
use tool_args::*;
use tool_schemas::*;
use views::*;
use vocab_resource::*;
use vocabulary::*;
use workspace_runtime::*;

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const DEFAULT_RESOURCE_PAGE_LIMIT: usize = 50;
const SLOW_WORKSPACE_REFRESH_LOG_MS: u128 = 1_000;
const INSTRUCTIONS_URI: &str = "prism://instructions";
const ENTRYPOINTS_URI: &str = "prism://entrypoints";
const CAPABILITIES_URI: &str = "prism://capabilities";
const SESSION_URI: &str = "prism://session";
const PLANS_URI: &str = "prism://plans";
const CONTRACTS_URI: &str = "prism://contracts";
const FILE_RESOURCE_TEMPLATE_URI: &str =
    "prism://file/{path}?startLine={startLine}&endLine={endLine}&maxChars={maxChars}";
const VOCAB_URI: &str = "prism://vocab";
const SCHEMAS_URI: &str = "prism://schemas";
const TOOL_SCHEMAS_URI: &str = "prism://tool-schemas";
const ENTRYPOINTS_RESOURCE_TEMPLATE_URI: &str = "prism://entrypoints?limit={limit}&cursor={cursor}";
const SYMBOL_RESOURCE_TEMPLATE_URI: &str = "prism://symbol/{crateName}/{kind}/{path}";
const SEARCH_RESOURCE_TEMPLATE_URI: &str =
    "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}";
const LINEAGE_RESOURCE_TEMPLATE_URI: &str =
    "prism://lineage/{lineageId}?limit={limit}&cursor={cursor}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}?limit={limit}&cursor={cursor}";
const PLANS_RESOURCE_TEMPLATE_URI: &str =
    "prism://plans?status={status}&scope={scope}&contains={contains}&limit={limit}&cursor={cursor}";
const CONTRACTS_RESOURCE_TEMPLATE_URI: &str =
    "prism://contracts?contains={contains}&status={status}&scope={scope}&kind={kind}&limit={limit}&cursor={cursor}";
const EVENT_RESOURCE_TEMPLATE_URI: &str = "prism://event/{eventId}";
const MEMORY_RESOURCE_TEMPLATE_URI: &str = "prism://memory/{memoryId}";
const EDGE_RESOURCE_TEMPLATE_URI: &str = "prism://edge/{edgeId}";
const SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/{resourceKind}";
const TOOL_SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/tool/{toolName}";
const TOOL_ACTION_SCHEMA_RESOURCE_TEMPLATE_URI: &str =
    "prism://schema/tool/{toolName}/action/{action}";
const AGENT_INSTRUCTIONS_MARKDOWN: &str = include_str!("../../../AGENT_INSTRUCTIONS.md");
static WORKSPACE_RUNTIME_SYNC_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> =
    OnceLock::new();

#[cfg(test)]
fn default_query_worker_pool() -> JsWorkerPool {
    JsWorkerPool::with_worker_count(1)
}

#[cfg(not(test))]
fn default_query_worker_pool() -> JsWorkerPool {
    JsWorkerPool::spawn()
}

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "prism-mcp")]
#[command(about = "MCP server for programmable PRISM queries")]
pub struct PrismMcpCli {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    #[arg(long, value_enum, default_value_t = PrismMcpMode::Stdio)]
    pub mode: PrismMcpMode,
    #[arg(long = "no-coordination", alias = "simple", default_value_t = false)]
    pub no_coordination: bool,
    #[arg(long = "internal-developer", default_value_t = false)]
    pub internal_developer: bool,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub enable_coordination: Vec<CoordinationFeatureFlag>,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub disable_coordination: Vec<CoordinationFeatureFlag>,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub enable_query_view: Vec<QueryViewFeatureFlag>,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub disable_query_view: Vec<QueryViewFeatureFlag>,
    #[arg(long = "daemon-log")]
    pub daemon_log: Option<PathBuf>,
    #[arg(long = "shared-runtime-sqlite")]
    pub shared_runtime_sqlite: Option<PathBuf>,
    #[arg(long = "shared-runtime-uri")]
    pub shared_runtime_uri: Option<String>,
    #[arg(long = "daemon-start-timeout-ms")]
    pub daemon_start_timeout_ms: Option<u64>,
    #[arg(long = "http-bind", default_value = "127.0.0.1:0")]
    pub http_bind: String,
    #[arg(long = "http-path", default_value = "/mcp")]
    pub http_path: String,
    #[arg(long = "health-path", default_value = "/healthz")]
    pub health_path: String,
    #[arg(long = "http-uri-file")]
    pub http_uri_file: Option<PathBuf>,
    #[arg(long = "upstream-uri")]
    pub upstream_uri: Option<String>,
    #[arg(long, hide = true, default_value_t = false)]
    pub daemonize: bool,
}

impl PrismMcpCli {
    pub fn shared_runtime_backend(&self) -> Result<SharedRuntimeBackend> {
        match (&self.shared_runtime_sqlite, &self.shared_runtime_uri) {
            (Some(_), Some(_)) => Err(anyhow!(
                "shared runtime backend must be configured with either --shared-runtime-sqlite or --shared-runtime-uri, not both"
            )),
            (Some(path), None) => Ok(SharedRuntimeBackend::Sqlite { path: path.clone() }),
            (None, Some(uri)) => Ok(SharedRuntimeBackend::Remote { uri: uri.clone() }),
            (None, None) => Ok(SharedRuntimeBackend::Disabled),
        }
    }

    pub fn features(&self) -> PrismMcpFeatures {
        let mut features = if self.no_coordination {
            PrismMcpFeatures::simple()
        } else {
            PrismMcpFeatures::full()
        };
        features.internal_developer = self.internal_developer;
        for flag in &self.enable_coordination {
            features.coordination.apply(*flag, true);
        }
        for flag in &self.disable_coordination {
            features.coordination.apply(*flag, false);
        }
        for flag in &self.enable_query_view {
            features.query_views.apply(*flag, true);
        }
        for flag in &self.disable_query_view {
            features.query_views.apply(*flag, false);
        }
        features
    }

    fn http_uri_file_path(&self, root: &Path) -> PathBuf {
        self.http_uri_file
            .clone()
            .unwrap_or_else(|| daemon_mode::default_http_uri_file_path(root))
    }

    fn log_path(&self, root: &Path) -> PathBuf {
        self.daemon_log
            .clone()
            .unwrap_or_else(|| daemon_mode::default_log_path(root))
    }

    fn daemon_spawn_args(&self, root: &Path) -> Vec<String> {
        let mut args = vec![
            "--mode".to_string(),
            "daemon".to_string(),
            "--daemonize".to_string(),
            "--root".to_string(),
            root.display().to_string(),
        ];
        if self.no_coordination {
            args.push("--no-coordination".to_string());
        }
        if self.internal_developer {
            args.push("--internal-developer".to_string());
        }
        for flag in &self.enable_coordination {
            args.push("--enable-coordination".to_string());
            args.push(
                flag.to_possible_value()
                    .expect("value enum")
                    .get_name()
                    .to_string(),
            );
        }
        for flag in &self.disable_coordination {
            args.push("--disable-coordination".to_string());
            args.push(
                flag.to_possible_value()
                    .expect("value enum")
                    .get_name()
                    .to_string(),
            );
        }
        for flag in &self.enable_query_view {
            args.push("--enable-query-view".to_string());
            args.push(
                flag.to_possible_value()
                    .expect("value enum")
                    .get_name()
                    .to_string(),
            );
        }
        for flag in &self.disable_query_view {
            args.push("--disable-query-view".to_string());
            args.push(
                flag.to_possible_value()
                    .expect("value enum")
                    .get_name()
                    .to_string(),
            );
        }
        args.push("--http-bind".to_string());
        args.push(self.http_bind.clone());
        args.push("--http-path".to_string());
        args.push(self.http_path.clone());
        args.push("--health-path".to_string());
        args.push(self.health_path.clone());
        args.push("--http-uri-file".to_string());
        args.push(self.http_uri_file_path(root).display().to_string());
        if let Some(path) = &self.shared_runtime_sqlite {
            args.push("--shared-runtime-sqlite".to_string());
            args.push(path.display().to_string());
        }
        if let Some(uri) = &self.shared_runtime_uri {
            args.push("--shared-runtime-uri".to_string());
            args.push(uri.clone());
        }
        args
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PrismMcpMode {
    Stdio,
    Daemon,
    Bridge,
}

pub struct PrismMcpServer {
    tool_router: ToolRouter<PrismMcpServer>,
    host: Arc<QueryHost>,
    session: Arc<SessionState>,
}

impl Clone for PrismMcpServer {
    fn clone(&self) -> Self {
        Self {
            tool_router: self.tool_router.clone(),
            host: Arc::clone(&self.host),
            session: self.host.new_session_state(),
        }
    }
}

impl PrismMcpServer {
    pub(crate) fn mcp_call_log_store(&self) -> Arc<McpCallLogStore> {
        Arc::clone(&self.host.mcp_call_log_store)
    }

    pub(crate) fn dashboard_state(&self) -> Arc<DashboardState> {
        self.host.dashboard_state()
    }

    pub(crate) fn workspace_session(&self) -> Option<&Arc<WorkspaceSession>> {
        self.host.workspace_session()
    }

    pub(crate) fn session_log_context(&self) -> (String, Option<String>) {
        (
            self.session.session_id().0.to_string(),
            self.session.current_task().map(|task| task.0.to_string()),
        )
    }

    pub fn from_workspace(root: impl AsRef<Path>) -> Result<Self> {
        Self::from_workspace_with_features_and_shared_runtime(
            root,
            PrismMcpFeatures::default(),
            SharedRuntimeBackend::Disabled,
        )
    }

    pub fn from_workspace_with_features(
        root: impl AsRef<Path>,
        features: PrismMcpFeatures,
    ) -> Result<Self> {
        Self::from_workspace_with_features_and_shared_runtime(
            root,
            features,
            SharedRuntimeBackend::Disabled,
        )
    }

    pub fn from_workspace_with_features_and_shared_runtime(
        root: impl AsRef<Path>,
        features: PrismMcpFeatures,
        shared_runtime: SharedRuntimeBackend,
    ) -> Result<Self> {
        let root = root.as_ref();
        let started = std::time::Instant::now();
        info!(
            root = %root.display(),
            coordination = %features.mode_label(),
            "building prism-mcp workspace server"
        );
        let session = hydrate_workspace_session_with_options(
            root,
            WorkspaceSessionOptions {
                coordination: features.coordination_layer_enabled(),
                shared_runtime,
                hydrate_persisted_projections: false,
            },
        )?;
        let prism = session.prism_arc();
        info!(
            root = %root.display(),
            node_count = prism.graph().node_count(),
            edge_count = prism.graph().edge_count(),
            file_count = prism.graph().file_count(),
            "built prism-mcp workspace server"
        );
        if let Err(error) = record_workspace_server_built(
            root,
            &features,
            prism.graph().node_count(),
            prism.graph().edge_count(),
            prism.graph().file_count(),
            started.elapsed().as_millis(),
        ) {
            debug!(
                error = %error,
                root = %root.display(),
                "failed to update prism runtime state after building the workspace server"
            );
        }
        Ok(Self::with_session_and_features(session, features))
    }

    pub fn new(prism: Prism) -> Self {
        Self::new_with_limits_and_features(
            prism,
            QueryLimits::default(),
            PrismMcpFeatures::default(),
        )
    }

    pub fn new_with_limits(prism: Prism, limits: QueryLimits) -> Self {
        Self::new_with_limits_and_features(prism, limits, PrismMcpFeatures::default())
    }

    pub fn new_with_features(prism: Prism, features: PrismMcpFeatures) -> Self {
        Self::new_with_limits_and_features(prism, QueryLimits::default(), features)
    }

    pub fn new_with_limits_and_features(
        prism: Prism,
        limits: QueryLimits,
        features: PrismMcpFeatures,
    ) -> Self {
        let host = Arc::new(QueryHost::new_with_limits_and_features(
            prism, limits, features,
        ));
        Self {
            tool_router: Self::build_tool_router(),
            session: host.new_session_state(),
            host,
        }
    }

    pub fn with_session(session: WorkspaceSession) -> Self {
        Self::with_session_limits_and_features(
            session,
            QueryLimits::default(),
            PrismMcpFeatures::default(),
        )
    }

    pub fn with_session_limits(session: WorkspaceSession, limits: QueryLimits) -> Self {
        Self::with_session_limits_and_features(session, limits, PrismMcpFeatures::default())
    }

    pub fn with_session_and_features(
        session: WorkspaceSession,
        features: PrismMcpFeatures,
    ) -> Self {
        Self::with_session_limits_and_features(session, QueryLimits::default(), features)
    }

    pub fn with_session_limits_and_features(
        session: WorkspaceSession,
        limits: QueryLimits,
        features: PrismMcpFeatures,
    ) -> Self {
        let host = Arc::new(QueryHost::with_session_and_limits_and_features(
            session, limits, features,
        ));
        Self {
            tool_router: Self::build_tool_router(),
            session: host.new_session_state(),
            host,
        }
    }

    pub(crate) fn instrumented_service(self) -> InstrumentedServerService {
        InstrumentedServerService::new(self)
    }

    pub(crate) async fn serve<T, E, A>(
        self,
        transport: T,
    ) -> std::result::Result<
        RunningService<RoleServer, InstrumentedServerService>,
        ServerInitializeError,
    >
    where
        T: IntoTransport<RoleServer, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        self.instrumented_service().serve(transport).await
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[derive(Clone)]
struct QueryHost {
    prism: Arc<Prism>,
    notes: Arc<SessionMemory>,
    inferred_edges: Arc<InferenceStore>,
    next_event: Arc<AtomicU64>,
    default_limits: QueryLimits,
    worker_pool: Arc<JsWorkerPool>,
    pub(crate) mcp_call_log_store: Arc<McpCallLogStore>,
    dashboard_state: Arc<DashboardState>,
    workspace: Option<Arc<WorkspaceSession>>,
    workspace_runtime_sync_lock: Arc<Mutex<()>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    loaded_episodic_revision: Arc<AtomicU64>,
    loaded_inference_revision: Arc<AtomicU64>,
    loaded_coordination_revision: Arc<AtomicU64>,
    workspace_runtime: Option<Arc<WorkspaceRuntime>>,
    restored_session_seed: Option<PersistedSessionSeed>,
    features: PrismMcpFeatures,
    #[cfg(test)]
    test_session: OnceLock<Arc<SessionState>>,
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceRefreshReport {
    pub(crate) refresh_path: &'static str,
    pub(crate) runtime_sync_used: bool,
    pub(crate) deferred: bool,
    pub(crate) episodic_reloaded: bool,
    pub(crate) inference_reloaded: bool,
    pub(crate) coordination_reloaded: bool,
    pub(crate) metrics: WorkspaceRefreshMetrics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WorkspaceRefreshMetrics {
    pub(crate) lock_wait_ms: u64,
    pub(crate) lock_hold_ms: u64,
    pub(crate) fs_refresh_ms: u64,
    pub(crate) snapshot_revisions_ms: u64,
    pub(crate) load_episodic_ms: u64,
    pub(crate) load_inference_ms: u64,
    pub(crate) load_coordination_ms: u64,
    pub(crate) loaded_bytes: u64,
    pub(crate) replay_volume: u64,
    pub(crate) full_rebuild_count: u64,
    pub(crate) workspace_reloaded: bool,
}

impl WorkspaceRefreshMetrics {
    fn as_json(self) -> serde_json::Value {
        serde_json::json!({
            "lockWaitMs": self.lock_wait_ms,
            "lockHoldMs": self.lock_hold_ms,
            "fsRefreshMs": self.fs_refresh_ms,
            "snapshotRevisionsMs": self.snapshot_revisions_ms,
            "loadEpisodicMs": self.load_episodic_ms,
            "loadInferenceMs": self.load_inference_ms,
            "loadCoordinationMs": self.load_coordination_ms,
            "reloadWork": {
                "loadedBytes": self.loaded_bytes,
                "replayVolume": self.replay_volume,
                "fullRebuildCount": self.full_rebuild_count,
                "workspaceReloaded": self.workspace_reloaded,
            },
        })
    }
}

fn shared_workspace_runtime_sync_lock(root: &Path) -> Arc<Mutex<()>> {
    let locks = WORKSPACE_RUNTIME_SYNC_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("workspace runtime sync-lock registry poisoned");
    if let Some(existing) = locks.get(root).and_then(Weak::upgrade) {
        return existing;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}

impl QueryHost {
    #[cfg(test)]
    fn new(prism: Prism) -> Self {
        Self::new_with_limits_and_features(
            prism,
            QueryLimits::default(),
            PrismMcpFeatures::default(),
        )
    }

    #[cfg(test)]
    fn new_with_limits(prism: Prism, limits: QueryLimits) -> Self {
        Self::new_with_limits_and_features(prism, limits, PrismMcpFeatures::default())
    }

    fn new_with_limits_and_features(
        prism: Prism,
        limits: QueryLimits,
        features: PrismMcpFeatures,
    ) -> Self {
        Self::new_with_limits_features_and_worker_count(
            prism,
            limits,
            features,
            default_query_worker_pool(),
        )
    }

    fn new_with_limits_features_and_worker_count(
        prism: Prism,
        limits: QueryLimits,
        features: PrismMcpFeatures,
        worker_pool: JsWorkerPool,
    ) -> Self {
        let prism = Arc::new(prism);
        Self {
            prism: prism.clone(),
            notes: Arc::new(SessionMemory::new()),
            inferred_edges: Arc::new(InferenceStore::new()),
            next_event: Arc::new(AtomicU64::new(0)),
            default_limits: limits,
            worker_pool: Arc::new(worker_pool),
            mcp_call_log_store: Arc::new(McpCallLogStore::for_root(None)),
            dashboard_state: Arc::new(DashboardState::default()),
            workspace: None,
            workspace_runtime_sync_lock: Arc::new(Mutex::new(())),
            loaded_workspace_revision: Arc::new(AtomicU64::new(0)),
            loaded_episodic_revision: Arc::new(AtomicU64::new(0)),
            loaded_inference_revision: Arc::new(AtomicU64::new(0)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(0)),
            workspace_runtime: None,
            restored_session_seed: None,
            features: features.clone(),
            #[cfg(test)]
            test_session: OnceLock::new(),
        }
    }

    #[cfg(test)]
    fn with_session(workspace: WorkspaceSession) -> Self {
        Self::with_session_and_limits_and_features(
            workspace,
            QueryLimits::default(),
            PrismMcpFeatures::default(),
        )
    }

    fn with_session_and_limits_and_features(
        workspace: WorkspaceSession,
        limits: QueryLimits,
        features: PrismMcpFeatures,
    ) -> Self {
        Self::with_session_limits_features_and_worker_count(
            workspace,
            limits,
            features,
            default_query_worker_pool(),
        )
    }

    fn with_session_limits_features_and_worker_count(
        workspace: WorkspaceSession,
        limits: QueryLimits,
        features: PrismMcpFeatures,
        worker_pool: JsWorkerPool,
    ) -> Self {
        let workspace = Arc::new(workspace);
        let prism = workspace.prism_arc();
        let notes = Arc::new(SessionMemory::new());
        let inferred_edges = Arc::new(InferenceStore::new());
        let mcp_call_log_store = Arc::new(McpCallLogStore::for_root(Some(workspace.root())));
        let dashboard_state = Arc::new(DashboardState::default());
        let workspace_runtime_sync_lock = shared_workspace_runtime_sync_lock(workspace.root());
        let loaded_workspace_revision = workspace.loaded_workspace_revision_handle();
        let loaded_episodic_revision = Arc::new(AtomicU64::new(0));
        let loaded_inference_revision = Arc::new(AtomicU64::new(0));
        let loaded_coordination_revision = Arc::new(AtomicU64::new(0));
        let runtime_config = WorkspaceRuntimeConfig {
            workspace: Arc::clone(&workspace),
            notes: Arc::clone(&notes),
            inferred_edges: Arc::clone(&inferred_edges),
            dashboard_state: Arc::clone(&dashboard_state),
            sync_lock: Arc::clone(&workspace_runtime_sync_lock),
            loaded_workspace_revision: Arc::clone(&loaded_workspace_revision),
            loaded_episodic_revision: Arc::clone(&loaded_episodic_revision),
            loaded_inference_revision: Arc::clone(&loaded_inference_revision),
            loaded_coordination_revision: Arc::clone(&loaded_coordination_revision),
        };
        let workspace_runtime = Arc::new(WorkspaceRuntime::spawn(runtime_config.clone()));
        let _ = crate::workspace_runtime::hydrate_persisted_workspace_state(&runtime_config);
        if workspace.needs_refresh() {
            workspace_runtime.request_refresh();
        }
        let restored_session_seed = match load_session_seed(workspace.root()) {
            Ok(seed) => seed,
            Err(error) => {
                debug!(error = %error, "failed to load persisted session seed");
                None
            }
        };
        Self {
            prism: Arc::clone(&prism),
            notes,
            inferred_edges,
            next_event: Arc::new(AtomicU64::new(0)),
            default_limits: limits,
            worker_pool: Arc::new(worker_pool),
            mcp_call_log_store,
            dashboard_state,
            workspace: Some(Arc::clone(&workspace)),
            workspace_runtime_sync_lock,
            loaded_workspace_revision,
            loaded_episodic_revision,
            loaded_inference_revision,
            loaded_coordination_revision,
            workspace_runtime: Some(workspace_runtime),
            restored_session_seed,
            features,
            #[cfg(test)]
            test_session: OnceLock::new(),
        }
    }

    fn new_session_state(&self) -> Arc<SessionState> {
        let session = Arc::new(SessionState::new(
            Arc::clone(&self.notes),
            Arc::clone(&self.inferred_edges),
            Arc::clone(&self.next_event),
            self.default_limits,
        ));
        if let Some(seed) = self.restored_session_seed.as_ref() {
            restore_session_seed(session.as_ref(), seed);
        }
        session
    }

    fn persist_session_seed(&self, session: &SessionState) -> Result<()> {
        if let Some(workspace) = &self.workspace {
            persist_session_seed(workspace.root(), session)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn cached_test_session(&self) -> Arc<SessionState> {
        Arc::clone(self.test_session.get_or_init(|| self.new_session_state()))
    }

    #[allow(dead_code)]
    fn configure_session(
        &self,
        session: &SessionState,
        args: PrismConfigureSessionArgs,
    ) -> Result<SessionView> {
        self.refresh_workspace()?;
        self.configure_session_without_refresh(session, args)
    }

    fn configure_session_without_refresh(
        &self,
        session: &SessionState,
        args: PrismConfigureSessionArgs,
    ) -> Result<SessionView> {
        if args.clear_current_task.unwrap_or(false)
            && (args.current_task_id.is_some()
                || args.coordination_task_id.is_some()
                || args.current_task_description.is_some()
                || args.current_task_tags.is_some())
        {
            return Err(anyhow!(
                "clearCurrentTask cannot be combined with currentTaskId, coordinationTaskId, currentTaskDescription, or currentTaskTags"
            ));
        }
        if args.clear_current_agent.unwrap_or(false) && args.current_agent.is_some() {
            return Err(anyhow!(
                "clearCurrentAgent cannot be combined with currentAgent"
            ));
        }

        if let Some(input) = args.limits {
            let mut limits = session.limits();
            if let Some(value) = input.max_result_nodes {
                limits.max_result_nodes = value;
            }
            if let Some(value) = input.max_call_graph_depth {
                limits.max_call_graph_depth = value;
            }
            if let Some(value) = input.max_output_json_bytes {
                limits.max_output_json_bytes = value;
            }
            if limits.max_result_nodes == 0
                || limits.max_call_graph_depth == 0
                || limits.max_output_json_bytes == 0
            {
                return Err(anyhow!("session limits must be greater than zero"));
            }
            session.set_limits(limits);
        }

        if args.clear_current_task.unwrap_or(false) {
            session.clear_current_task();
        } else if let Some(task_id) = args
            .current_task_id
            .clone()
            .or_else(|| args.coordination_task_id.clone())
        {
            let task_id = TaskId::new(task_id);
            let metadata = self.task_metadata(session, &task_id);
            session.set_current_task(
                task_id,
                args.current_task_description.or(metadata.description),
                args.current_task_tags.unwrap_or(metadata.tags),
                args.coordination_task_id.or(metadata.coordination_task_id),
            );
        } else if args.current_task_description.is_some() || args.current_task_tags.is_some() {
            if session.current_task_state().is_none() {
                return Err(anyhow!(
                    "no active task is set; use prism_session with action `start_task` or provide currentTaskId"
                ));
            }
            session.update_current_task_metadata(
                args.current_task_description.map(Some),
                args.current_task_tags,
            );
        }

        if args.clear_current_agent.unwrap_or(false) {
            session.clear_current_agent();
        } else if let Some(agent) = args.current_agent {
            session.set_current_agent(prism_ir::AgentId::new(agent));
        }

        self.persist_session_seed(session)?;
        Ok(self.session_view_without_refresh(session))
    }

    fn current_prism(&self) -> Arc<Prism> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.prism_arc())
            .unwrap_or_else(|| Arc::clone(&self.prism))
    }

    pub(crate) fn workspace_session(&self) -> Option<&Arc<WorkspaceSession>> {
        self.workspace.as_ref()
    }

    pub(crate) fn ensure_workspace_paths_deep<I>(&self, paths: I) -> Result<bool>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let Some(workspace) = &self.workspace else {
            return Ok(false);
        };
        let Some(changed) = workspace.try_ensure_paths_deep(paths)? else {
            return Ok(false);
        };
        if changed {
            self.sync_workspace_revision(workspace)?;
        }
        Ok(changed)
    }

    fn sync_workspace_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let revision = workspace.workspace_revision()?;
        self.sync_workspace_revision_value(revision);
        Ok(())
    }

    pub(crate) fn sync_episodic_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let revision = workspace.episodic_revision()?;
        self.sync_episodic_revision_value(revision);
        Ok(())
    }

    pub(crate) fn reload_episodic_snapshot(&self, workspace: &WorkspaceSession) -> Result<()> {
        let snapshot =
            workspace
                .load_episodic_snapshot_for_runtime()?
                .unwrap_or(EpisodicMemorySnapshot {
                    entries: Vec::new(),
                });
        self.notes.replace_from_snapshot(snapshot);
        self.sync_episodic_revision(workspace)
    }

    pub(crate) fn sync_inference_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let revision = workspace.inference_revision()?;
        self.sync_inference_revision_value(revision);
        Ok(())
    }

    pub(crate) fn sync_coordination_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let _ = workspace.hydrate_coordination_runtime()?;
        let revision = workspace.coordination_revision()?;
        self.sync_coordination_revision_value(revision);
        Ok(())
    }

    fn sync_workspace_revision_value(&self, revision: u64) {
        self.loaded_workspace_revision
            .store(revision, Ordering::Relaxed);
    }

    fn sync_episodic_revision_value(&self, revision: u64) {
        self.loaded_episodic_revision
            .store(revision, Ordering::Relaxed);
    }

    fn sync_inference_revision_value(&self, revision: u64) {
        self.loaded_inference_revision
            .store(revision, Ordering::Relaxed);
    }

    fn sync_coordination_revision_value(&self, revision: u64) {
        self.loaded_coordination_revision
            .store(revision, Ordering::Relaxed);
    }

    fn persist_outcomes(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_outcomes()?;
        self.sync_workspace_revision(workspace)
    }

    fn persist_notes(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_episodic(&self.notes.persisted_snapshot())?;
        self.sync_episodic_revision(workspace)
    }

    fn persist_inferred_edges(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_inference(&self.inferred_edges.snapshot_persisted())?;
        self.sync_inference_revision(workspace)
    }

    fn api_reference_markdown(&self) -> String {
        if self.features.mode_label() == "full"
            && self.features.api_reference_includes_internal_developer()
        {
            return api_reference_markdown().to_string();
        }

        let mut markdown = format!(
            "# PRISM MCP Feature Flags\n\n- mode: `{}`\n",
            self.features.mode_label()
        );
        for line in self.features.coordination_summary_lines() {
            markdown.push_str(&line);
            markdown.push('\n');
        }
        markdown.push_str(
            "\nThe API reference below describes the full PRISM query surface. Disabled coordination and internal-developer groups stay hidden from the visible query surface, and their query helpers fail when called.\n\n---\n\n",
        );
        let mut reference = api_reference_markdown().to_string();
        if !self.features.internal_developer {
            reference = strip_internal_developer_api_reference(&reference);
        }
        markdown.push_str(&reference);
        markdown
    }
}

fn strip_internal_developer_api_reference(markdown: &str) -> String {
    const METHOD_LINES: &[&str] = &[
        "  runtimeStatus(): RuntimeStatusView;",
        "  runtimeLogs(options?: RuntimeLogOptions): RuntimeLogEventView[];",
        "  runtimeTimeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];",
        "  mcpLog(options?: McpLogOptions): McpCallLogEntryView[];",
        "  slowMcpCalls(options?: McpLogOptions): McpCallLogEntryView[];",
        "  mcpTrace(id: string): McpCallTraceView | null;",
        "  mcpStats(options?: McpLogOptions): McpCallStatsView;",
        "  queryLog(options?: QueryLogOptions): QueryLogEntryView[];",
        "  slowQueries(options?: QueryLogOptions): QueryLogEntryView[];",
        "  queryTrace(id: string): QueryTraceView | null;",
        "  validationFeedback(options?: ValidationFeedbackOptions): ValidationFeedbackView[];",
    ];
    const TYPE_BLOCKS: &[&str] = &[
        "type McpLogOptions = {",
        "type QueryLogOptions = {",
        "type ValidationFeedbackOptions = {",
        "type RuntimeLogOptions = {",
        "type RuntimeTimelineOptions = {",
        "type RuntimeHealthView = {",
        "type RuntimeProcessView = {",
        "type RuntimeMaterializationItemView = {",
        "type RuntimeMaterializationView = {",
        "type RuntimeFreshnessView = {",
        "type RuntimeStatusView = {",
        "type RuntimeLogEventView = {",
        "type ValidationFeedbackView = {",
        "type McpCallPayloadSummaryView = {",
        "type McpCallLogEntryView = {",
        "type McpCallTraceView = {",
        "type McpCallStatsBucketView = {",
        "type McpCallStatsView = {",
        "type QueryResultSummaryView = {",
        "type QueryPhaseView = {",
        "type QueryLogEntryView = {",
        "type QueryTraceView = {",
    ];
    const HEADING_SECTIONS: &[&str] = &[
        "### 7a. Inspect recent MCP activity through PRISM itself",
        "### 7e. Inspect daemon status and recent runtime activity through PRISM",
        "### 7f. Inspect validation feedback recorded while dogfooding PRISM",
    ];
    const BULLET_PATTERNS: &[&str] = &[
        "workspace-backed runtime introspection through `prism.runtimeStatus()`",
        "a durable canonical MCP call log through `prism.mcpLog(...)`",
        "internal validation-feedback inspection through `prism.validationFeedback(...)`",
    ];

    let mut output = Vec::new();
    let mut lines = markdown.lines().peekable();
    let mut skipping_type = false;
    let mut skipping_section = false;

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if skipping_type {
            if trimmed == "};" {
                skipping_type = false;
            }
            continue;
        }
        if skipping_section {
            if trimmed.starts_with("### ") {
                skipping_section = false;
            } else {
                continue;
            }
        }
        if TYPE_BLOCKS.contains(&trimmed) {
            skipping_type = true;
            continue;
        }
        if HEADING_SECTIONS.contains(&trimmed) {
            skipping_section = true;
            continue;
        }
        if METHOD_LINES.contains(&line)
            || BULLET_PATTERNS.iter().any(|pattern| line.contains(pattern))
        {
            continue;
        }
        output.push(line);
    }

    output.join("\n")
}

fn log_refresh_workspace(
    refresh_path: &str,
    loaded_workspace_revision: u64,
    loaded_episodic_revision: u64,
    loaded_inference_revision: u64,
    loaded_coordination_revision: u64,
    workspace: &WorkspaceSession,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    duration_ms: u128,
    metrics: WorkspaceRefreshMetrics,
) {
    if should_record_workspace_refresh_event(
        refresh_path,
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
    ) {
        if let Err(error) = record_workspace_refresh(
            workspace.root(),
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            workspace
                .snapshot_revisions()
                .ok()
                .map(|revisions| revisions.workspace),
            loaded_workspace_revision,
            workspace.episodic_revision().ok(),
            loaded_episodic_revision,
            workspace.inference_revision().ok(),
            loaded_inference_revision,
            workspace.coordination_revision().ok(),
            loaded_coordination_revision,
            duration_ms,
            metrics,
        ) {
            debug!(
                error = %error,
                root = %workspace.root().display(),
                "failed to update prism runtime state after workspace refresh"
            );
        }
    }
    let debug_refresh_logging = env::var_os("PRISM_MCP_REFRESH_LOG").is_some();
    let Some(log_level) =
        workspace_refresh_log_level(refresh_path, duration_ms, debug_refresh_logging)
    else {
        return;
    };

    match log_level {
        Level::DEBUG => debug!(
            refresh_path,
            fs_observed = workspace.observed_fs_revision(),
            fs_applied = workspace.applied_fs_revision(),
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            duration_ms,
            lock_wait_ms = metrics.lock_wait_ms,
            lock_hold_ms = metrics.lock_hold_ms,
            fs_refresh_ms = metrics.fs_refresh_ms,
            snapshot_revisions_ms = metrics.snapshot_revisions_ms,
            load_episodic_ms = metrics.load_episodic_ms,
            load_inference_ms = metrics.load_inference_ms,
            load_coordination_ms = metrics.load_coordination_ms,
            loaded_bytes = metrics.loaded_bytes,
            replay_volume = metrics.replay_volume,
            full_rebuild_count = metrics.full_rebuild_count,
            workspace_reloaded = metrics.workspace_reloaded,
            "prism-mcp workspace refresh"
        ),
        Level::INFO => info!(
            refresh_path,
            fs_observed = workspace.observed_fs_revision(),
            fs_applied = workspace.applied_fs_revision(),
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            duration_ms,
            lock_wait_ms = metrics.lock_wait_ms,
            lock_hold_ms = metrics.lock_hold_ms,
            fs_refresh_ms = metrics.fs_refresh_ms,
            snapshot_revisions_ms = metrics.snapshot_revisions_ms,
            load_episodic_ms = metrics.load_episodic_ms,
            load_inference_ms = metrics.load_inference_ms,
            load_coordination_ms = metrics.load_coordination_ms,
            loaded_bytes = metrics.loaded_bytes,
            replay_volume = metrics.replay_volume,
            full_rebuild_count = metrics.full_rebuild_count,
            workspace_reloaded = metrics.workspace_reloaded,
            "prism-mcp workspace refresh"
        ),
        _ => {}
    }
}

fn should_record_workspace_refresh_event(
    refresh_path: &str,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    duration_ms: u128,
) -> bool {
    matches!(refresh_path, "full" | "incremental" | "deferred")
        || episodic_reloaded
        || inference_reloaded
        || coordination_reloaded
        || duration_ms >= SLOW_WORKSPACE_REFRESH_LOG_MS
}

fn workspace_refresh_log_level(
    refresh_path: &str,
    duration_ms: u128,
    debug_refresh_logging: bool,
) -> Option<Level> {
    if debug_refresh_logging {
        return Some(Level::DEBUG);
    }
    if refresh_path == "deferred" || duration_ms >= SLOW_WORKSPACE_REFRESH_LOG_MS {
        return Some(Level::INFO);
    }
    None
}

#[cfg(test)]
mod query_replay_cases;

#[cfg(test)]
#[path = "tests/query_history.rs"]
mod tests_query_history;

#[cfg(test)]
#[path = "tests/coordination_surface.rs"]
mod tests_coordination_surface;

#[cfg(test)]
#[path = "tests/view_surfaces.rs"]
mod tests_view_surfaces;

#[cfg(test)]
#[path = "tests/server_resources.rs"]
mod tests_server_resources;

#[cfg(test)]
#[path = "tests/server_transport.rs"]
mod tests_server_transport;

#[cfg(test)]
#[path = "tests/server_tool_calls.rs"]
mod tests_server_tool_calls;

#[cfg(test)]
mod tests_support;

#[cfg(test)]
mod tests;
