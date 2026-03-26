use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use clap::{ArgAction, ValueEnum};
use prism_agent::InferenceStore;
use prism_core::{index_workspace_session_with_options, WorkspaceSession, WorkspaceSessionOptions};
use prism_ir::TaskId;
use prism_js::{api_reference_markdown, CuratorJobView, API_REFERENCE_URI};
use prism_memory::{EpisodicMemorySnapshot, OutcomeEvent, SessionMemory};
use prism_query::{Prism, QueryLimits};
use rmcp::{handler::server::router::tool::ToolRouter, transport::stdio, ServiceExt};

mod capabilities_resource;
mod common;
mod daemon_mode;
mod diagnostics;
mod discovery_bundle;
mod discovery_helpers;
mod features;
mod host_mutations;
mod host_resources;
mod js_runtime;
mod memory_metadata;
mod proxy_server;
mod query_helpers;
mod query_runtime;
mod query_types;
mod resource_schemas;
mod resources;
mod schema_examples;
mod semantic_contexts;
mod server_surface;
mod session_state;
mod spec_insights;
mod suggested_queries;
mod task_journal;
mod tool_args;
mod tool_schemas;
mod views;

use capabilities_resource::*;
use common::*;
pub use daemon_mode::serve_with_mode;
use diagnostics::*;
use discovery_bundle::*;
use discovery_helpers::*;
pub use features::{CoordinationFeatureFlag, PrismMcpFeatures};
use js_runtime::JsWorker;
use memory_metadata::*;
use query_helpers::*;
use query_runtime::*;
use query_types::*;
use resource_schemas::*;
use resources::*;
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
    "prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&includeInferred={includeInferred}";
const LINEAGE_RESOURCE_TEMPLATE_URI: &str =
    "prism://lineage/{lineageId}?limit={limit}&cursor={cursor}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}?limit={limit}&cursor={cursor}";
const EVENT_RESOURCE_TEMPLATE_URI: &str = "prism://event/{eventId}";
const MEMORY_RESOURCE_TEMPLATE_URI: &str = "prism://memory/{memoryId}";
const EDGE_RESOURCE_TEMPLATE_URI: &str = "prism://edge/{edgeId}";
const SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/{resourceKind}";
const TOOL_SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/tool/{toolName}";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

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
}

impl PrismMcpCli {
    pub fn features(&self) -> PrismMcpFeatures {
        let mut features = if self.no_coordination {
            PrismMcpFeatures::simple()
        } else {
            PrismMcpFeatures::full()
        };
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
            "--root".to_string(),
            root.display().to_string(),
        ];
        if self.no_coordination {
            args.push("--no-coordination".to_string());
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

#[derive(Clone)]
pub struct PrismMcpServer {
    tool_router: ToolRouter<PrismMcpServer>,
    host: Arc<QueryHost>,
}

impl PrismMcpServer {
    pub fn from_workspace(root: impl AsRef<Path>) -> Result<Self> {
        Self::from_workspace_with_features(root, PrismMcpFeatures::default())
    }

    pub fn from_workspace_with_features(
        root: impl AsRef<Path>,
        features: PrismMcpFeatures,
    ) -> Result<Self> {
        let session = index_workspace_session_with_options(
            root,
            WorkspaceSessionOptions {
                coordination: features.coordination_layer_enabled(),
            },
        )?;
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
        Self {
            tool_router: Self::build_tool_router(),
            host: Arc::new(QueryHost::new_with_limits_and_features(
                prism, limits, features,
            )),
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
        Self {
            tool_router: Self::build_tool_router(),
            host: Arc::new(QueryHost::with_session_and_limits_and_features(
                session, limits, features,
            )),
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
    session: Arc<SessionState>,
    worker: Arc<JsWorker>,
    workspace: Option<Arc<WorkspaceSession>>,
    loaded_workspace_revision: Arc<AtomicU64>,
    loaded_episodic_revision: Arc<AtomicU64>,
    loaded_inference_revision: Arc<AtomicU64>,
    loaded_coordination_revision: Arc<AtomicU64>,
    features: PrismMcpFeatures,
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
        let prism = Arc::new(prism);
        let session = Arc::new(SessionState::with_limits(
            prism.as_ref(),
            SessionMemory::new(),
            InferenceStore::new(),
            limits,
        ));
        Self {
            prism: prism.clone(),
            session,
            worker: Arc::new(JsWorker::spawn()),
            workspace: None,
            loaded_workspace_revision: Arc::new(AtomicU64::new(0)),
            loaded_episodic_revision: Arc::new(AtomicU64::new(0)),
            loaded_inference_revision: Arc::new(AtomicU64::new(0)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(0)),
            features,
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
        let workspace = Arc::new(workspace);
        let prism = workspace.prism_arc();
        let workspace_revision = workspace.workspace_revision().unwrap_or_default();
        let notes = workspace
            .load_episodic_snapshot()
            .ok()
            .flatten()
            .map(SessionMemory::from_snapshot)
            .unwrap_or_else(SessionMemory::new);
        let episodic_revision = workspace.episodic_revision().unwrap_or_default();
        let inferred_edges = workspace
            .load_inference_snapshot()
            .ok()
            .flatten()
            .map(InferenceStore::from_snapshot)
            .unwrap_or_else(InferenceStore::new);
        let inference_revision = workspace.inference_revision().unwrap_or_default();
        let coordination_revision = workspace.coordination_revision().unwrap_or_default();
        let session = Arc::new(SessionState::with_snapshots(
            prism.as_ref(),
            notes,
            inferred_edges,
            limits,
        ));
        Self {
            prism,
            session,
            worker: Arc::new(JsWorker::spawn()),
            workspace: Some(workspace),
            loaded_workspace_revision: Arc::new(AtomicU64::new(workspace_revision)),
            loaded_episodic_revision: Arc::new(AtomicU64::new(episodic_revision)),
            loaded_inference_revision: Arc::new(AtomicU64::new(inference_revision)),
            loaded_coordination_revision: Arc::new(AtomicU64::new(coordination_revision)),
            features,
        }
    }

    fn configure_session(&self, args: PrismConfigureSessionArgs) -> Result<SessionView> {
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
            let mut limits = self.session.limits();
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
            self.session.set_limits(limits);
        }

        if args.clear_current_task.unwrap_or(false) {
            self.session.clear_current_task();
        } else if let Some(task_id) = args.current_task_id {
            let task_id = TaskId::new(task_id);
            let (description, tags) = self.task_metadata(&task_id);
            self.session.set_current_task(
                task_id,
                args.current_task_description.or(description),
                args.current_task_tags.unwrap_or(tags),
            );
        } else if args.current_task_description.is_some() || args.current_task_tags.is_some() {
            if self.session.current_task_state().is_none() {
                return Err(anyhow!(
                    "no active task is set; use prism_session with action `start_task` or provide currentTaskId"
                ));
            }
            self.session.update_current_task_metadata(
                args.current_task_description.map(Some),
                args.current_task_tags,
            );
        }

        if args.clear_current_agent.unwrap_or(false) {
            self.session.clear_current_agent();
        } else if let Some(agent) = args.current_agent {
            self.session
                .set_current_agent(prism_ir::AgentId::new(agent));
        }

        self.session_view()
    }

    fn current_prism(&self) -> Arc<Prism> {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.prism_arc())
            .unwrap_or_else(|| Arc::clone(&self.prism))
    }

    fn refresh_workspace(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };

        let started = Instant::now();
        let mut refresh_path = "fast";
        if self.reload_workspace_snapshot_if_needed(workspace)? {
            refresh_path = "full";
        }
        let _ = workspace.refresh_fs()?;
        self.sync_workspace_revision(workspace)?;

        let episodic_reloaded = self.reload_episodic_snapshot_if_needed(workspace)?;
        let inference_reloaded = self.reload_inference_snapshot_if_needed(workspace)?;
        let coordination_reloaded = self.reload_coordination_snapshot_if_needed(workspace)?;
        log_refresh_workspace(
            refresh_path,
            workspace,
            episodic_reloaded,
            inference_reloaded,
            coordination_reloaded,
            started.elapsed().as_millis(),
        );
        Ok(())
    }

    fn reload_workspace_snapshot_if_needed(&self, workspace: &WorkspaceSession) -> Result<bool> {
        let revision = workspace.workspace_revision()?;
        let loaded = self.loaded_workspace_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        workspace.reload_persisted_prism()?;
        Ok(true)
    }

    fn sync_workspace_revision(&self, workspace: &WorkspaceSession) -> Result<()> {
        let revision = workspace.workspace_revision()?;
        self.loaded_workspace_revision
            .store(revision, Ordering::Relaxed);
        Ok(())
    }

    fn reload_episodic_snapshot_if_needed(&self, workspace: &WorkspaceSession) -> Result<bool> {
        let revision = workspace.episodic_revision()?;
        let loaded = self.loaded_episodic_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace
            .load_episodic_snapshot()?
            .unwrap_or(EpisodicMemorySnapshot {
                entries: Vec::new(),
            });
        self.session.notes.replace_from_snapshot(snapshot);
        self.loaded_episodic_revision
            .store(revision, Ordering::Relaxed);
        Ok(true)
    }

    fn reload_inference_snapshot_if_needed(&self, workspace: &WorkspaceSession) -> Result<bool> {
        let revision = workspace.inference_revision()?;
        let loaded = self.loaded_inference_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace.load_inference_snapshot()?.unwrap_or_default();
        self.session.inferred_edges.replace_from_snapshot(snapshot);
        self.loaded_inference_revision
            .store(revision, Ordering::Relaxed);
        Ok(true)
    }

    fn reload_coordination_snapshot_if_needed(&self, workspace: &WorkspaceSession) -> Result<bool> {
        let revision = workspace.coordination_revision()?;
        let loaded = self.loaded_coordination_revision.load(Ordering::Relaxed);
        if revision == loaded {
            return Ok(false);
        }

        let snapshot = workspace.load_coordination_snapshot()?.unwrap_or_default();
        workspace
            .prism_arc()
            .replace_coordination_snapshot(snapshot);
        self.loaded_coordination_revision
            .store(revision, Ordering::Relaxed);
        Ok(true)
    }

    fn persist_outcomes(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_outcomes()
    }

    fn persist_notes(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_episodic(&self.session.notes.snapshot())
    }

    fn persist_inferred_edges(&self) -> Result<()> {
        let Some(workspace) = &self.workspace else {
            return Ok(());
        };
        workspace.persist_inference(&self.session.inferred_edges.snapshot_persisted())
    }

    fn api_reference_markdown(&self) -> String {
        if self.features.mode_label() == "full" {
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
            "\nThe API reference below describes the full PRISM query surface. Disabled coordination groups stay hidden from `tools/list`, and their query helpers fail when called.\n\n---\n\n",
        );
        markdown.push_str(api_reference_markdown());
        markdown
    }
}

fn log_refresh_workspace(
    refresh_path: &str,
    workspace: &WorkspaceSession,
    episodic_reloaded: bool,
    inference_reloaded: bool,
    coordination_reloaded: bool,
    duration_ms: u128,
) {
    if env::var_os("PRISM_MCP_REFRESH_LOG").is_none() {
        return;
    }

    eprintln!(
        "prism-mcp refresh path={refresh_path} fs_observed={} fs_applied={} episodic_reloaded={} inference_reloaded={} coordination_reloaded={} duration_ms={duration_ms}",
        workspace.observed_fs_revision(),
        workspace.applied_fs_revision(),
        episodic_reloaded,
        inference_reloaded,
        coordination_reloaded,
    );
}

#[cfg(test)]
mod tests;
