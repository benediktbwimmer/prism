use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use clap::{ArgAction, ValueEnum};
use prism_agent::InferenceStore;
use prism_core::{
    index_workspace_session_with_options, FsRefreshStatus, WorkspaceSession,
    WorkspaceSessionOptions,
};
use prism_ir::TaskId;
use prism_js::{api_reference_markdown, CuratorJobView, API_REFERENCE_URI};
use prism_memory::{EpisodicMemorySnapshot, OutcomeEvent, SessionMemory};
use prism_query::{Prism, QueryLimits};
use rmcp::{handler::server::router::tool::ToolRouter, transport::stdio, ServiceExt};
use serde_json::json;
use tracing::{debug, info};

mod ambiguity;
mod capabilities_resource;
mod change_views;
mod common;
mod compact_followups;
mod compact_tools;
mod daemon_mode;
mod dashboard_assets;
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
mod memory_metadata;
mod process_lifecycle;
mod proxy_server;
mod query_errors;
mod query_helpers;
mod query_log;
mod query_runtime;
mod query_types;
mod resource_schemas;
mod resources;
mod runtime_state;
mod runtime_views;
mod schema_examples;
mod semantic_contexts;
mod server_surface;
mod session_state;
mod spec_insights;
mod suggested_queries;
mod task_journal;
mod text_search;
mod tool_args;
mod tool_schemas;
mod views;

use ambiguity::*;
use capabilities_resource::*;
use change_views::*;
use common::*;
pub use daemon_mode::serve_with_mode;
use dashboard_events::*;
use diagnostics::*;
use discovery_bundle::*;
use discovery_helpers::*;
pub use features::{CoordinationFeatureFlag, PrismMcpFeatures};
use js_runtime::JsWorkerPool;
use lineage_views::*;
pub use logging::{init_logging, log_process_start, log_top_level_error};
use memory_metadata::*;
pub use process_lifecycle::maybe_daemonize_process;
use query_errors::*;
use query_helpers::*;
use query_log::*;
use query_runtime::*;
use query_types::*;
use resource_schemas::*;
use resources::*;
use runtime_state::*;
use schema_examples::*;
use semantic_contexts::*;
use session_state::SessionState;
use spec_insights::*;
use suggested_queries::*;
use task_journal::*;
use tool_args::*;
use tool_schemas::*;
use views::*;

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const DEFAULT_RESOURCE_PAGE_LIMIT: usize = 50;
const ENTRYPOINTS_URI: &str = "prism://entrypoints";
const CAPABILITIES_URI: &str = "prism://capabilities";
const SESSION_URI: &str = "prism://session";
const SCHEMAS_URI: &str = "prism://schemas";
const TOOL_SCHEMAS_URI: &str = "prism://tool-schemas";
const ENTRYPOINTS_RESOURCE_TEMPLATE_URI: &str = "prism://entrypoints?limit={limit}&cursor={cursor}";
const SYMBOL_RESOURCE_TEMPLATE_URI: &str = "prism://symbol/{crateName}/{kind}/{path}";
const SEARCH_RESOURCE_TEMPLATE_URI: &str =
    "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}";
const LINEAGE_RESOURCE_TEMPLATE_URI: &str =
    "prism://lineage/{lineageId}?limit={limit}&cursor={cursor}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}?limit={limit}&cursor={cursor}";
const EVENT_RESOURCE_TEMPLATE_URI: &str = "prism://event/{eventId}";
const MEMORY_RESOURCE_TEMPLATE_URI: &str = "prism://memory/{memoryId}";
const EDGE_RESOURCE_TEMPLATE_URI: &str = "prism://edge/{edgeId}";
const SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/{resourceKind}";
const TOOL_SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/tool/{toolName}";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

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
    #[arg(long = "daemon-log")]
    pub daemon_log: Option<PathBuf>,
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
        args.push("--http-bind".to_string());
        args.push(self.http_bind.clone());
        args.push("--http-path".to_string());
        args.push(self.http_path.clone());
        args.push("--health-path".to_string());
        args.push(self.health_path.clone());
        args.push("--http-uri-file".to_string());
        args.push(self.http_uri_file_path(root).display().to_string());
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
    pub fn from_workspace(root: impl AsRef<Path>) -> Result<Self> {
        Self::from_workspace_with_features(root, PrismMcpFeatures::default())
    }

    pub fn from_workspace_with_features(
        root: impl AsRef<Path>,
        features: PrismMcpFeatures,
    ) -> Result<Self> {
        let root = root.as_ref();
        info!(
            root = %root.display(),
            coordination = %features.mode_label(),
            "building prism-mcp workspace server"
        );
        let session = index_workspace_session_with_options(
            root,
            WorkspaceSessionOptions {
                coordination: features.coordination_layer_enabled(),
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
    next_task: Arc<AtomicU64>,
    default_limits: QueryLimits,
    worker_pool: Arc<JsWorkerPool>,
    query_log_store: Arc<QueryLogStore>,
    dashboard_state: Arc<DashboardState>,
    workspace: Option<Arc<WorkspaceSession>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    loaded_episodic_revision: Arc<AtomicU64>,
    loaded_inference_revision: Arc<AtomicU64>,
    loaded_coordination_revision: Arc<AtomicU64>,
    features: PrismMcpFeatures,
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceRefreshReport {
    refresh_path: &'static str,
    deferred: bool,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceSnapshotReloadStatus {
    Unchanged,
    Reloaded,
    DeferredBusy,
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
            next_event: Arc::new(AtomicU64::new(max_event_sequence(prism.as_ref()))),
            next_task: Arc::new(AtomicU64::new(max_task_sequence(prism.as_ref()))),
            default_limits: limits,
            worker_pool: Arc::new(worker_pool),
            query_log_store: Arc::new(QueryLogStore::default()),
            dashboard_state: Arc::new(DashboardState::default()),
            workspace: None,
            loaded_workspace_revision: Arc::new(AtomicU64::new(0)),
            loaded_episodic_revision: Arc::new(AtomicU64::new(0)),
            loaded_inference_revision: Arc::new(AtomicU64::new(0)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(0)),
            features: features.clone(),
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
        let revisions = workspace.snapshot_revisions().unwrap_or_default();
        let notes = workspace
            .load_episodic_snapshot()
            .ok()
            .flatten()
            .map(SessionMemory::from_snapshot)
            .unwrap_or_else(SessionMemory::new);
        let inferred_edges = workspace
            .load_inference_snapshot()
            .ok()
            .flatten()
            .map(InferenceStore::from_snapshot)
            .unwrap_or_else(InferenceStore::new);
        Self {
            prism: Arc::clone(&prism),
            notes: Arc::new(notes),
            inferred_edges: Arc::new(inferred_edges),
            next_event: Arc::new(AtomicU64::new(max_event_sequence(prism.as_ref()))),
            next_task: Arc::new(AtomicU64::new(max_task_sequence(prism.as_ref()))),
            default_limits: limits,
            worker_pool: Arc::new(worker_pool),
            query_log_store: Arc::new(QueryLogStore::default()),
            dashboard_state: Arc::new(DashboardState::default()),
            workspace: Some(Arc::clone(&workspace)),
            loaded_workspace_revision: workspace.loaded_workspace_revision_handle(),
            loaded_episodic_revision: Arc::new(AtomicU64::new(revisions.episodic)),
            loaded_inference_revision: Arc::new(AtomicU64::new(revisions.inference)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(revisions.coordination)),
            features,
        }
    }

    fn new_session_state(&self) -> Arc<SessionState> {
        Arc::new(SessionState::new(
            Arc::clone(&self.notes),
            Arc::clone(&self.inferred_edges),
            Arc::clone(&self.next_event),
            Arc::clone(&self.next_task),
            self.default_limits,
        ))
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
                || args.current_task_description.is_some()
                || args.current_task_tags.is_some())
        {
            return Err(anyhow!(
                "clearCurrentTask cannot be combined with currentTaskId, currentTaskDescription, or currentTaskTags"
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
        } else if let Some(task_id) = args.current_task_id {
            let task_id = TaskId::new(task_id);
            let (description, tags) = self.task_metadata(session, &task_id);
            session.set_current_task(
                task_id,
                args.current_task_description.or(description),
                args.current_task_tags.unwrap_or(tags),
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

        Ok(self.session_view_without_refresh(session))
    }

    fn current_prism(&self) -> Arc<Prism> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.prism_arc())
            .unwrap_or_else(|| Arc::clone(&self.prism))
    }

    pub(crate) fn refresh_workspace(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };

        let started = Instant::now();
        let revisions = workspace.snapshot_revisions()?;
        let mut refresh_path = "fast";
        if self.reload_workspace_snapshot_if_needed(workspace, revisions.workspace)? {
            refresh_path = "full";
        }
        let _ = workspace.refresh_fs()?;
        self.sync_workspace_revision_value(revisions.workspace);

        let episodic_reloaded =
            self.reload_episodic_snapshot_if_needed(workspace, revisions.episodic)?;
        let inference_reloaded =
            self.reload_inference_snapshot_if_needed(workspace, revisions.inference)?;
        let coordination_reloaded =
            self.reload_coordination_snapshot_if_needed(workspace, revisions.coordination)?;
        log_refresh_workspace(
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            started.elapsed().as_millis(),
        );
        self.dashboard_state.publish_value(
            "runtime.refreshed",
            json!({
                "refreshPath": refresh_path,
                "durationMs": started.elapsed().as_millis(),
                "coordinationReloaded": coordination_reloaded,
                "episodicReloaded": episodic_reloaded,
                "inferenceReloaded": inference_reloaded,
            }),
        );
        if coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        Ok(())
    }

    pub(crate) fn refresh_workspace_for_query(&self) -> Result<WorkspaceRefreshReport> {
        let Some(workspace) = &self.workspace else {
            return Ok(WorkspaceRefreshReport {
                refresh_path: "none",
                deferred: false,
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
            });
        };

        let started = Instant::now();
        let revisions = workspace.snapshot_revisions()?;
        let mut refresh_path = "fast";
        let deferred = match self
            .try_reload_workspace_snapshot_if_needed(workspace, revisions.workspace)?
        {
            WorkspaceSnapshotReloadStatus::Unchanged => match workspace.refresh_fs_nonblocking()? {
                FsRefreshStatus::Clean => false,
                FsRefreshStatus::Refreshed => {
                    refresh_path = "full";
                    false
                }
                FsRefreshStatus::DeferredBusy => {
                    refresh_path = "deferred";
                    true
                }
            },
            WorkspaceSnapshotReloadStatus::Reloaded => {
                refresh_path = "persisted";
                false
            }
            WorkspaceSnapshotReloadStatus::DeferredBusy => {
                refresh_path = "deferred";
                true
            }
        };
        let (episodic_reloaded, inference_reloaded, coordination_reloaded) = if deferred {
            (false, false, false)
        } else {
            self.sync_workspace_revision_value(revisions.workspace);
            (
                self.reload_episodic_snapshot_if_needed(workspace, revisions.episodic)?,
                self.reload_inference_snapshot_if_needed(workspace, revisions.inference)?,
                self.reload_coordination_snapshot_if_needed(workspace, revisions.coordination)?,
            )
        };
        log_refresh_workspace(
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            started.elapsed().as_millis(),
        );
        self.dashboard_state.publish_value(
            "runtime.refreshed",
            json!({
                "refreshPath": refresh_path,
                "durationMs": started.elapsed().as_millis(),
                "coordinationReloaded": coordination_reloaded,
                "deferred": deferred,
                "episodicReloaded": episodic_reloaded,
                "inferenceReloaded": inference_reloaded,
            }),
        );
        if coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        Ok(WorkspaceRefreshReport {
            refresh_path,
            deferred,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
        })
    }

    pub(crate) fn refresh_workspace_for_mutation(&self) -> Result<WorkspaceRefreshReport> {
        let Some(workspace) = &self.workspace else {
            return Ok(WorkspaceRefreshReport {
                refresh_path: "none",
                deferred: false,
                episodic_reloaded: false,
                inference_reloaded: false,
                coordination_reloaded: false,
            });
        };

        let started = Instant::now();
        let revisions = workspace.snapshot_revisions()?;
        let (workspace_reloaded, deferred) =
            match self.try_reload_workspace_snapshot_if_needed(workspace, revisions.workspace)? {
                WorkspaceSnapshotReloadStatus::Unchanged => (false, false),
                WorkspaceSnapshotReloadStatus::Reloaded => (true, false),
                WorkspaceSnapshotReloadStatus::DeferredBusy => (false, true),
            };
        if workspace_reloaded {
            self.sync_workspace_revision_value(revisions.workspace);
        }
        let (episodic_reloaded, inference_reloaded, coordination_reloaded) = if deferred {
            (false, false, false)
        } else {
            (
                self.reload_episodic_snapshot_if_needed(workspace, revisions.episodic)?,
                self.reload_inference_snapshot_if_needed(workspace, revisions.inference)?,
                self.reload_coordination_snapshot_if_needed(workspace, revisions.coordination)?,
            )
        };
        let refresh_path = if deferred {
            "deferred"
        } else if workspace_reloaded
            || episodic_reloaded
            || inference_reloaded
            || coordination_reloaded
        {
            "persisted"
        } else {
            "none"
        };
        log_refresh_workspace(
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            started.elapsed().as_millis(),
        );
        self.dashboard_state.publish_value(
            "runtime.refreshed",
            json!({
                "refreshPath": refresh_path,
                "durationMs": started.elapsed().as_millis(),
                "coordinationReloaded": coordination_reloaded,
                "deferred": deferred,
                "episodicReloaded": episodic_reloaded,
                "inferenceReloaded": inference_reloaded,
                "workspaceReloaded": workspace_reloaded,
            }),
        );
        if coordination_reloaded {
            let _ = self.publish_dashboard_coordination_update();
        }
        Ok(WorkspaceRefreshReport {
            refresh_path,
            deferred,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
        })
    }

    fn reload_workspace_snapshot_if_needed(
        &self,
        workspace: &WorkspaceSession,
        revision: u64,
    ) -> Result<bool> {
        let loaded = self.loaded_workspace_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        workspace.reload_persisted_prism()?;
        Ok(true)
    }

    fn try_reload_workspace_snapshot_if_needed(
        &self,
        workspace: &WorkspaceSession,
        revision: u64,
    ) -> Result<WorkspaceSnapshotReloadStatus> {
        let loaded = self.loaded_workspace_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(WorkspaceSnapshotReloadStatus::Unchanged);
        }

        if workspace.try_reload_persisted_prism()? {
            Ok(WorkspaceSnapshotReloadStatus::Reloaded)
        } else {
            Ok(WorkspaceSnapshotReloadStatus::DeferredBusy)
        }
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

    pub(crate) fn sync_inference_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let revision = workspace.inference_revision()?;
        self.sync_inference_revision_value(revision);
        Ok(())
    }

    pub(crate) fn sync_coordination_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
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

    fn reload_episodic_snapshot_if_needed(
        &self,
        workspace: &WorkspaceSession,
        revision: u64,
    ) -> Result<bool> {
        let loaded = self.loaded_episodic_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace
            .load_episodic_snapshot()?
            .unwrap_or(EpisodicMemorySnapshot {
                entries: Vec::new(),
            });
        self.notes.replace_from_snapshot(snapshot);
        self.sync_episodic_revision_value(revision);
        Ok(true)
    }

    fn reload_inference_snapshot_if_needed(
        &self,
        workspace: &WorkspaceSession,
        revision: u64,
    ) -> Result<bool> {
        let loaded = self.loaded_inference_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace.load_inference_snapshot()?.unwrap_or_default();
        self.inferred_edges.replace_from_snapshot(snapshot);
        self.sync_inference_revision_value(revision);
        Ok(true)
    }

    fn reload_coordination_snapshot_if_needed(
        &self,
        workspace: &WorkspaceSession,
        revision: u64,
    ) -> Result<bool> {
        let loaded = self.loaded_coordination_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace.load_coordination_snapshot()?.unwrap_or_default();
        workspace
            .prism_arc()
            .replace_coordination_snapshot(snapshot);
        self.sync_coordination_revision_value(revision);
        Ok(true)
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
        "  queryLog(options?: QueryLogOptions): QueryLogEntryView[];",
        "  slowQueries(options?: QueryLogOptions): QueryLogEntryView[];",
        "  queryTrace(id: string): QueryTraceView | null;",
        "  validationFeedback(options?: ValidationFeedbackOptions): ValidationFeedbackView[];",
    ];
    const TYPE_BLOCKS: &[&str] = &[
        "type QueryLogOptions = {",
        "type ValidationFeedbackOptions = {",
        "type RuntimeLogOptions = {",
        "type RuntimeTimelineOptions = {",
        "type RuntimeHealthView = {",
        "type RuntimeProcessView = {",
        "type RuntimeStatusView = {",
        "type RuntimeLogEventView = {",
        "type ValidationFeedbackView = {",
        "type QueryResultSummaryView = {",
        "type QueryPhaseView = {",
        "type QueryLogEntryView = {",
        "type QueryTraceView = {",
    ];
    const HEADING_SECTIONS: &[&str] = &[
        "### 7a. Inspect recent query behavior through PRISM itself",
        "### 7e. Inspect daemon status and recent runtime activity through PRISM",
        "### 7f. Inspect validation feedback recorded while dogfooding PRISM",
    ];
    const BULLET_PATTERNS: &[&str] = &[
        "workspace-backed runtime introspection through `prism.runtimeStatus()`",
        "a first-class query log through `prism.queryLog(...)`",
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
    workspace: &WorkspaceSession,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    duration_ms: u128,
) {
    let meaningful_refresh =
        refresh_path == "full" || episodic_reloaded || inference_reloaded || coordination_reloaded;
    if meaningful_refresh {
        if let Err(error) = record_workspace_refresh(
            workspace.root(),
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            duration_ms,
        ) {
            debug!(
                error = %error,
                root = %workspace.root().display(),
                "failed to update prism runtime state after workspace refresh"
            );
        }
    }
    if env::var_os("PRISM_MCP_REFRESH_LOG").is_none() {
        return;
    }

    debug!(
        refresh_path,
        fs_observed = workspace.observed_fs_revision(),
        fs_applied = workspace.applied_fs_revision(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
        duration_ms,
        "prism-mcp workspace refresh"
    );
}

#[cfg(test)]
mod query_replay_cases;

#[cfg(test)]
mod tests;
