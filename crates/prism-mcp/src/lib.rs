use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use clap::ArgAction;
use prism_agent::InferenceStore;
use prism_core::{index_workspace_session_with_options, WorkspaceSession, WorkspaceSessionOptions};
use prism_ir::TaskId;
use prism_js::{api_reference_markdown, CuratorJobView, API_REFERENCE_URI};
use prism_memory::{OutcomeEvent, SessionMemory};
use prism_query::{Prism, QueryLimits};
use rmcp::{handler::server::router::tool::ToolRouter, transport::stdio, ServiceExt};

mod common;
mod features;
mod host_mutations;
mod host_resources;
mod js_runtime;
mod query_helpers;
mod query_runtime;
mod query_types;
mod resource_schemas;
mod resources;
mod server_surface;
mod session_state;
mod tool_args;
mod views;

use common::*;
pub use features::{CoordinationFeatureFlag, PrismMcpFeatures};
use js_runtime::JsWorker;
use query_helpers::*;
use query_runtime::*;
use query_types::*;
use resource_schemas::*;
use resources::*;
use session_state::SessionState;
use tool_args::*;
use views::*;

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const DEFAULT_RESOURCE_PAGE_LIMIT: usize = 50;
const ENTRYPOINTS_URI: &str = "prism://entrypoints";
const SESSION_URI: &str = "prism://session";
const SCHEMAS_URI: &str = "prism://schemas";
const ENTRYPOINTS_RESOURCE_TEMPLATE_URI: &str = "prism://entrypoints?limit={limit}&cursor={cursor}";
const SYMBOL_RESOURCE_TEMPLATE_URI: &str = "prism://symbol/{crateName}/{kind}/{path}";
const SEARCH_RESOURCE_TEMPLATE_URI: &str = "prism://search/{query}?limit={limit}&cursor={cursor}";
const LINEAGE_RESOURCE_TEMPLATE_URI: &str =
    "prism://lineage/{lineageId}?limit={limit}&cursor={cursor}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}?limit={limit}&cursor={cursor}";
const EVENT_RESOURCE_TEMPLATE_URI: &str = "prism://event/{eventId}";
const MEMORY_RESOURCE_TEMPLATE_URI: &str = "prism://memory/{memoryId}";
const EDGE_RESOURCE_TEMPLATE_URI: &str = "prism://edge/{edgeId}";
const SCHEMA_RESOURCE_TEMPLATE_URI: &str = "prism://schema/{resourceKind}";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "prism-mcp")]
#[command(about = "MCP server for programmable PRISM queries")]
pub struct PrismMcpCli {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    #[arg(long = "no-coordination", alias = "simple", default_value_t = false)]
    pub no_coordination: bool,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub enable_coordination: Vec<CoordinationFeatureFlag>,
    #[arg(long, value_enum, value_delimiter = ',', action = ArgAction::Append)]
    pub disable_coordination: Vec<CoordinationFeatureFlag>,
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
                    "no active task is set; use prism_start_task or provide currentTaskId"
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
        if let Some(workspace) = &self.workspace {
            let _ = workspace.refresh_fs()?;
        }
        Ok(())
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

#[cfg(test)]
mod tests;
