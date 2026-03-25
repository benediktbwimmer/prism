use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use deno_ast::{
    parse_program, EmitOptions, MediaType, ModuleSpecifier, ParseParams, TranspileModuleOptions,
    TranspileOptions,
};
use prism_agent::{EdgeId, InferenceStore, InferredEdgeScope};
use prism_core::{index_workspace_session, WorkspaceSession};
use prism_ir::{
    AnchorRef, Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta, NodeId, NodeKind, TaskId,
};
use prism_js::{
    api_reference_markdown, runtime_prelude, ChangeImpactView, CoChangeView, EdgeView,
    LineageEventView, LineageStatus, LineageView, MemoryEntryView, NodeIdView, QueryDiagnostic,
    QueryEnvelope, RelationsView, ScoredMemoryView, SubgraphView, SymbolView, ValidationCheckView,
    ValidationRecipeView, API_REFERENCE_URI,
};
use prism_memory::{
    EpisodicMemory, MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource, OutcomeEvent,
    OutcomeEvidence, OutcomeKind, OutcomeResult, RecallQuery, ScoredMemory,
};
use prism_query::{
    ChangeImpact, CoChange, Prism, QueryLimits, Symbol, ValidationCheck, ValidationRecipe,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use rquickjs::{prelude::Func, Context, Runtime};
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const ENTRYPOINTS_URI: &str = "prism://entrypoints";
const SYMBOL_RESOURCE_TEMPLATE_URI: &str = "prism://symbol/{crateName}/{kind}/{path}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}";

struct SessionState {
    notes: EpisodicMemory,
    inferred_edges: InferenceStore,
    current_task: Mutex<Option<TaskId>>,
    next_event: AtomicU64,
    next_task: AtomicU64,
    limits: QueryLimits,
}

impl SessionState {
    fn with_limits(
        prism: &Prism,
        notes: EpisodicMemory,
        inferred_edges: InferenceStore,
        limits: QueryLimits,
    ) -> Self {
        Self::with_snapshots(prism, notes, inferred_edges, limits)
    }

    fn with_snapshots(
        prism: &Prism,
        notes: EpisodicMemory,
        inferred_edges: InferenceStore,
        limits: QueryLimits,
    ) -> Self {
        Self {
            notes,
            inferred_edges,
            current_task: Mutex::new(None),
            next_event: AtomicU64::new(max_event_sequence(prism)),
            next_task: AtomicU64::new(max_task_sequence(prism)),
            limits,
        }
    }

    fn next_event_id(&self, prefix: &str) -> EventId {
        let sequence = self.next_event.fetch_add(1, Ordering::Relaxed) + 1;
        EventId::new(format!("{prefix}:{sequence}"))
    }

    fn current_task(&self) -> Option<TaskId> {
        self.current_task
            .lock()
            .expect("session task lock poisoned")
            .clone()
    }

    fn set_current_task(&self, task: TaskId) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = Some(task);
    }

    fn start_task(&self, description: &str, _tags: &[String]) -> TaskId {
        let sequence = self.next_task.fetch_add(1, Ordering::Relaxed) + 1;
        let mut slug = description
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>();
        while slug.contains("--") {
            slug = slug.replace("--", "-");
        }
        slug = slug.trim_matches('-').to_owned();
        let prefix = if slug.is_empty() { "task" } else { &slug };
        let task = TaskId::new(format!("task:{prefix}:{sequence}"));
        self.set_current_task(task.clone());
        task
    }

    fn task_for_mutation(&self, explicit: Option<TaskId>) -> TaskId {
        if let Some(task) = explicit {
            return task;
        }
        if let Some(task) = self.current_task() {
            return task;
        }
        self.start_task("session", &[])
    }

    fn limits(&self) -> QueryLimits {
        self.limits
    }
}

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "prism-mcp")]
#[command(about = "MCP server for programmable PRISM queries")]
pub struct PrismMcpCli {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Clone)]
pub struct PrismMcpServer {
    tool_router: ToolRouter<PrismMcpServer>,
    host: Arc<QueryHost>,
}

impl PrismMcpServer {
    pub fn from_workspace(root: impl AsRef<Path>) -> Result<Self> {
        let session = index_workspace_session(root)?;
        Ok(Self::with_session(session))
    }

    pub fn new(prism: Prism) -> Self {
        Self::new_with_limits(prism, QueryLimits::default())
    }

    pub fn new_with_limits(prism: Prism, limits: QueryLimits) -> Self {
        Self {
            tool_router: Self::tool_router(),
            host: Arc::new(QueryHost::new_with_limits(prism, limits)),
        }
    }

    pub fn with_session(session: WorkspaceSession) -> Self {
        Self::with_session_limits(session, QueryLimits::default())
    }

    pub fn with_session_limits(session: WorkspaceSession, limits: QueryLimits) -> Self {
        Self {
            tool_router: Self::tool_router(),
            host: Arc::new(QueryHost::with_session_and_limits(session, limits)),
        }
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum QueryLanguage {
    Ts,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismQueryArgs {
    #[schemars(description = "TypeScript snippet evaluated with a global `prism` object.")]
    code: String,
    #[schemars(description = "Query language. Only `ts` is currently supported.")]
    language: Option<QueryLanguage>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismSymbolArgs {
    #[schemars(description = "Best-effort symbol lookup query.")]
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismSearchArgs {
    #[schemars(description = "Full-text or symbol search query.")]
    query: String,
    #[schemars(description = "Maximum number of results to return.")]
    limit: Option<usize>,
    #[schemars(description = "Optional node kind filter.")]
    kind: Option<String>,
    #[schemars(description = "Optional path fragment filter.")]
    path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct NodeIdInput {
    #[serde(alias = "crate_name")]
    #[serde(alias = "crateName")]
    crate_name: String,
    path: String,
    kind: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnchorRefInput {
    Node {
        #[serde(alias = "crate_name")]
        #[serde(alias = "crateName")]
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        #[serde(rename = "lineageId", alias = "lineage_id")]
        lineage_id: String,
    },
    File {
        #[serde(rename = "fileId", alias = "file_id")]
        file_id: u32,
    },
    Kind {
        kind: String,
    },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OutcomeKindInput {
    NoteAdded,
    HypothesisProposed,
    PlanCreated,
    BuildRan,
    TestRan,
    ReviewFeedback,
    FailureObserved,
    RegressionObserved,
    FixValidated,
    RollbackPerformed,
    MigrationRequired,
    IncidentLinked,
    PerfSignalObserved,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OutcomeResultInput {
    Success,
    Failure,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutcomeEvidenceInput {
    Commit { sha: String },
    Test { name: String, passed: bool },
    Build { target: String, passed: bool },
    Reviewer { author: String },
    Issue { id: String },
    StackTrace { hash: String },
    DiffSummary { text: String },
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum InferredEdgeScopeInput {
    SessionOnly,
    Persisted,
    Rejected,
    Expired,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismOutcomeArgs {
    kind: OutcomeKindInput,
    anchors: Vec<AnchorRefInput>,
    summary: String,
    result: Option<OutcomeResultInput>,
    evidence: Option<Vec<OutcomeEvidenceInput>>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismNoteArgs {
    anchors: Vec<AnchorRefInput>,
    content: String,
    trust: Option<f32>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismStartTaskArgs {
    description: String,
    tags: Option<Vec<String>>,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
struct PrismStartTaskResult {
    task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismInferEdgeArgs {
    source: NodeIdInput,
    target: NodeIdInput,
    kind: String,
    confidence: f32,
    scope: Option<InferredEdgeScopeInput>,
    evidence: Option<Vec<String>>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismTestRanArgs {
    anchors: Vec<AnchorRefInput>,
    test: String,
    passed: bool,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismFailureObservedArgs {
    anchors: Vec<AnchorRefInput>,
    summary: String,
    trace: Option<String>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismFixValidatedArgs {
    anchors: Vec<AnchorRefInput>,
    summary: String,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        description = "Create and activate a task context for subsequent mutations in this session.",
        annotations(
            title = "Start PRISM Task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_start_task(
        &self,
        Parameters(args): Parameters<PrismStartTaskArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.description.trim().is_empty() {
            return Err(McpError::invalid_params(
                "task description cannot be empty",
                Some(json!({ "field": "description" })),
            ));
        }

        let task = self
            .host
            .start_task(args.description, args.tags.unwrap_or_default())
            .map_err(map_query_error)?;
        structured_tool_result(PrismStartTaskResult {
            task_id: task.0.to_string(),
        })
    }

    #[tool(
        name = "prism_query",
        description = "Execute a read-only TypeScript query against the live PRISM graph. Read prism://api-reference for the available prism API.",
        annotations(title = "Programmable PRISM Query", read_only_hint = true)
    )]
    fn prism_query(
        &self,
        Parameters(args): Parameters<PrismQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.code.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query code cannot be empty",
                Some(json!({ "field": "code" })),
            ));
        }

        let language = args.language.unwrap_or(QueryLanguage::Ts);
        let envelope = self
            .host
            .execute(&args.code, language)
            .map_err(map_query_error)?;
        structured_tool_result(envelope)
    }

    #[tool(
        description = "Convenience lookup for the best matching symbol. Returns the same structured query envelope as prism_query.",
        annotations(title = "Lookup PRISM Symbol", read_only_hint = true)
    )]
    fn prism_symbol(
        &self,
        Parameters(args): Parameters<PrismSymbolArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let envelope = self
            .host
            .symbol_query(&args.query)
            .map_err(map_query_error)?;
        structured_tool_result(envelope)
    }

    #[tool(
        description = "Convenience search lookup. Returns the same structured query envelope as prism_query.",
        annotations(title = "Search PRISM Graph", read_only_hint = true)
    )]
    fn prism_search(
        &self,
        Parameters(args): Parameters<PrismSearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.query.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query cannot be empty",
                Some(json!({ "field": "query" })),
            ));
        }

        let envelope = self
            .host
            .search_query(SearchArgs {
                query: args.query,
                limit: args.limit,
                kind: args.kind,
                path: args.path,
                include_inferred: None,
            })
            .map_err(map_query_error)?;
        structured_tool_result(envelope)
    }

    #[tool(
        description = "Write a structured outcome event for the current task or symbol anchors.",
        annotations(
            title = "Record Outcome Event",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_outcome(
        &self,
        Parameters(args): Parameters<PrismOutcomeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let event_id = self.host.store_outcome(args).map_err(map_query_error)?;
        structured_tool_result(json!({ "eventId": event_id.0 }))
    }

    #[tool(
        description = "Store an agent note anchored to nodes or lineages.",
        annotations(
            title = "Store Agent Note",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_note(
        &self,
        Parameters(args): Parameters<PrismNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let memory_id = self.host.store_note(args).map_err(map_query_error)?;
        structured_tool_result(json!({ "memoryId": memory_id.0 }))
    }

    #[tool(
        description = "Persist an inferred edge into the session overlay or a promoted scope.",
        annotations(
            title = "Store Inferred Edge",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_infer_edge(
        &self,
        Parameters(args): Parameters<PrismInferEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let edge_id = self
            .host
            .store_inferred_edge(args)
            .map_err(map_query_error)?;
        structured_tool_result(json!({ "edgeId": edge_id.0 }))
    }

    #[tool(
        description = "Convenience outcome for a test run.",
        annotations(
            title = "Record Test Run",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_test_ran(
        &self,
        Parameters(args): Parameters<PrismTestRanArgs>,
    ) -> Result<CallToolResult, McpError> {
        let summary = format!(
            "test `{}` {}",
            args.test,
            if args.passed { "passed" } else { "failed" }
        );
        let event_id = self
            .host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::TestRan,
                anchors: args.anchors,
                summary,
                result: Some(if args.passed {
                    OutcomeResultInput::Success
                } else {
                    OutcomeResultInput::Failure
                }),
                evidence: Some(vec![OutcomeEvidenceInput::Test {
                    name: args.test,
                    passed: args.passed,
                }]),
                task_id: args.task_id,
            })
            .map_err(map_query_error)?;
        structured_tool_result(json!({ "eventId": event_id.0 }))
    }

    #[tool(
        description = "Convenience outcome for an observed failure.",
        annotations(
            title = "Record Observed Failure",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_failure_observed(
        &self,
        Parameters(args): Parameters<PrismFailureObservedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let evidence = args
            .trace
            .map(|trace| vec![OutcomeEvidenceInput::StackTrace { hash: trace }]);
        let event_id = self
            .host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::FailureObserved,
                anchors: args.anchors,
                summary: args.summary,
                result: Some(OutcomeResultInput::Failure),
                evidence,
                task_id: args.task_id,
            })
            .map_err(map_query_error)?;
        structured_tool_result(json!({ "eventId": event_id.0 }))
    }

    #[tool(
        description = "Convenience outcome for a validated fix.",
        annotations(
            title = "Record Validated Fix",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    fn prism_fix_validated(
        &self,
        Parameters(args): Parameters<PrismFixValidatedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let event_id = self
            .host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::FixValidated,
                anchors: args.anchors,
                summary: args.summary,
                result: Some(OutcomeResultInput::Success),
                evidence: None,
                task_id: args.task_id,
            })
            .map_err(map_query_error)?;
        structured_tool_result(json!({ "eventId": event_id.0 }))
    }
}

#[tool_handler]
impl ServerHandler for PrismMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(
            "Start with prism://api-reference for the typed query contract. Use prism_query for programmable read-only graph queries, prism_symbol or prism_search for direct lookups, prism://entrypoints for a quick workspace overview, prism://symbol/{crateName}/{kind}/{path} for an exact symbol snapshot, and the prism_* mutation tools to record outcomes, notes, inferred edges, and task context.",
        )
        .with_protocol_version(ProtocolVersion::LATEST)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                RawResource::new(API_REFERENCE_URI, "PRISM API Reference")
                    .with_description(
                        "TypeScript query surface, d.ts-style contract, and usage recipes",
                    )
                    .with_mime_type("text/markdown")
                    .no_annotation(),
                RawResource::new(ENTRYPOINTS_URI, "PRISM Entrypoints")
                    .with_description(
                        "Workspace entrypoints and top-level starting symbols in structured JSON",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = request.uri.as_str();
        let contents = if uri == API_REFERENCE_URI {
            ResourceContents::text(api_reference_markdown(), request.uri.clone())
                .with_mime_type("text/markdown")
        } else if uri == ENTRYPOINTS_URI {
            json_resource_contents(
                self.host
                    .entrypoints_resource_value()
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(id) = parse_symbol_resource_uri(uri)? {
            json_resource_contents(
                self.host
                    .symbol_resource_value(&id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(task_id) = parse_task_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .task_resource_value(&task_id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else {
            return Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({ "uri": request.uri })),
            ));
        };

        Ok(ReadResourceResult::new(vec![contents]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            next_cursor: None,
            resource_templates: vec![
                RawResourceTemplate::new(
                    SYMBOL_RESOURCE_TEMPLATE_URI,
                    "PRISM Symbol Snapshot",
                )
                .with_description(
                    "Read a structured snapshot for an exact symbol, including relations, lineage, validation recipe, blast radius, and related failures",
                )
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(TASK_RESOURCE_TEMPLATE_URI, "PRISM Task Replay")
                    .with_description(
                        "Read the outcome-event timeline recorded for a specific task context",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
            ],
            meta: None,
        })
    }
}

fn map_query_error(error: anyhow::Error) -> McpError {
    McpError::internal_error(
        "prism query failed",
        Some(json!({
            "code": "query_execution_failed",
            "error": error.to_string(),
        })),
    )
}

fn structured_tool_result<T: serde::Serialize>(value: T) -> Result<CallToolResult, McpError> {
    let value = serde_json::to_value(value).map_err(|err| {
        McpError::internal_error(
            "failed to serialize structured tool result",
            Some(json!({ "error": err.to_string() })),
        )
    })?;
    Ok(CallToolResult::structured(value))
}

fn json_resource_contents<T: serde::Serialize>(
    value: T,
    uri: impl Into<String>,
) -> Result<ResourceContents, McpError> {
    let text = serde_json::to_string_pretty(&value).map_err(|err| {
        McpError::internal_error(
            "failed to serialize resource payload",
            Some(json!({ "error": err.to_string() })),
        )
    })?;
    Ok(ResourceContents::text(text, uri).with_mime_type("application/json"))
}

fn parse_symbol_resource_uri(uri: &str) -> Result<Option<NodeId>, McpError> {
    let Some(rest) = uri.strip_prefix("prism://symbol/") else {
        return Ok(None);
    };
    let mut segments = rest.splitn(3, '/');
    let Some(crate_name) = segments.next() else {
        return Ok(None);
    };
    let Some(kind) = segments.next() else {
        return Ok(None);
    };
    let Some(path) = segments.next() else {
        return Ok(None);
    };
    let kind = parse_node_kind(kind).map_err(|err| {
        McpError::invalid_params(
            "invalid symbol resource uri",
            Some(json!({
                "uri": uri,
                "error": err.to_string(),
            })),
        )
    })?;
    Ok(Some(NodeId::new(crate_name, path, kind)))
}

fn parse_task_resource_uri(uri: &str) -> Option<TaskId> {
    uri.strip_prefix("prism://task/").map(TaskId::new)
}

#[derive(Clone)]
struct QueryHost {
    prism: Arc<Prism>,
    session: Arc<SessionState>,
    worker: Arc<JsWorker>,
    workspace: Option<Arc<WorkspaceSession>>,
}

impl QueryHost {
    #[cfg(test)]
    fn new(prism: Prism) -> Self {
        Self::new_with_limits(prism, QueryLimits::default())
    }

    fn new_with_limits(prism: Prism, limits: QueryLimits) -> Self {
        let prism = Arc::new(prism);
        let session = Arc::new(SessionState::with_limits(
            prism.as_ref(),
            EpisodicMemory::new(),
            InferenceStore::new(),
            limits,
        ));
        Self {
            prism: prism.clone(),
            session,
            worker: Arc::new(JsWorker::spawn()),
            workspace: None,
        }
    }

    #[cfg(test)]
    fn with_session(workspace: WorkspaceSession) -> Self {
        Self::with_session_and_limits(workspace, QueryLimits::default())
    }

    fn with_session_and_limits(workspace: WorkspaceSession, limits: QueryLimits) -> Self {
        let workspace = Arc::new(workspace);
        let prism = workspace.prism_arc();
        let notes = workspace
            .load_episodic_snapshot()
            .ok()
            .flatten()
            .map(EpisodicMemory::from_snapshot)
            .unwrap_or_else(EpisodicMemory::new);
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
        }
    }

    fn execute(&self, code: &str, language: QueryLanguage) -> Result<QueryEnvelope> {
        match language {
            QueryLanguage::Ts => self.execute_typescript(code),
        }
    }

    fn symbol_query(&self, query: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let result = serde_json::to_value(execution.best_symbol(query)?)?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn search_query(&self, args: SearchArgs) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let result = serde_json::to_value(execution.search(args)?)?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn entrypoints_resource_value(&self) -> Result<Value> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism);
        Ok(json!({
            "entrypoints": execution.entrypoints()?,
            "diagnostics": execution.diagnostics(),
        }))
    }

    fn symbol_resource_value(&self, id: &NodeId) -> Result<Value> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism.clone());
        let symbol = symbol_for(prism.as_ref(), id)?;
        Ok(json!({
            "symbol": symbol_view(prism.as_ref(), &symbol)?,
            "relations": relations_view(prism.as_ref(), self.session.as_ref(), id)?,
            "lineage": lineage_view(prism.as_ref(), id)?,
            "coChangeNeighbors": prism
                .co_change_neighbors(id, 8)
                .into_iter()
                .map(co_change_view)
                .collect::<Vec<_>>(),
            "relatedFailures": prism.related_failures(id),
            "blastRadius": blast_radius_view(prism.as_ref(), self.session.as_ref(), id),
            "validationRecipe": validation_recipe_view_with(
                prism.as_ref(),
                self.session.as_ref(),
                id,
            ),
            "diagnostics": execution.diagnostics(),
        }))
    }

    fn task_resource_value(&self, task_id: &TaskId) -> Result<Value> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        Ok(json!({
            "task": prism.resume_task(task_id),
        }))
    }

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let source = format!(
            "(function() {{\n  const __prismUserQuery = () => {{\n{}\n  }};\n  const __prismResult = __prismUserQuery();\n  return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n}})();\n",
            code
        );
        let transpiled = transpile_typescript(&source)?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let raw_result = self.worker.execute(transpiled, execution.clone())?;
        let mut result =
            serde_json::from_str(&raw_result).context("failed to decode query result JSON")?;
        let limits = self.session.limits();
        if raw_result.len() > limits.max_output_json_bytes {
            execution.push_diagnostic(
                "result_truncated",
                format!(
                    "Query output exceeded the {} byte session cap.",
                    limits.max_output_json_bytes
                ),
                Some(json!({
                    "applied": limits.max_output_json_bytes,
                    "observed": raw_result.len(),
                })),
            );
            result = Value::Null;
        }
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn co_change_neighbors_value(&self, id: &NodeId) -> Result<Value> {
        let prism = self.current_prism();
        serde_json::to_value(
            prism
                .co_change_neighbors(id, 8)
                .into_iter()
                .map(co_change_view)
                .collect::<Vec<_>>(),
        )
        .map_err(Into::into)
    }

    fn start_task(&self, description: String, tags: Vec<String>) -> Result<TaskId> {
        self.refresh_workspace()?;
        let task = self.session.start_task(&description, &tags);
        let event = OutcomeEvent {
            meta: EventMeta {
                id: self.session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task.clone()),
                causation: None,
            },
            anchors: Vec::new(),
            kind: OutcomeKind::PlanCreated,
            result: OutcomeResult::Success,
            summary: description,
            evidence: Vec::new(),
            metadata: json!({ "tags": tags }),
        };
        if let Some(workspace) = &self.workspace {
            let _ = workspace.append_outcome(event)?;
        } else {
            let prism = self.current_prism();
            prism.apply_outcome_event_to_projections(&event);
            let _ = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
        }
        Ok(task)
    }

    fn store_outcome(&self, args: PrismOutcomeArgs) -> Result<EventId> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
        let task_id = self
            .session
            .task_for_mutation(args.task_id.map(TaskId::new));
        let event = OutcomeEvent {
            meta: EventMeta {
                id: self.session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task_id),
                causation: None,
            },
            anchors,
            kind: convert_outcome_kind(args.kind),
            result: args
                .result
                .map(convert_outcome_result)
                .unwrap_or(OutcomeResult::Unknown),
            summary: args.summary,
            evidence: args
                .evidence
                .unwrap_or_default()
                .into_iter()
                .map(convert_outcome_evidence)
                .collect(),
            metadata: Value::Null,
        };
        if let Some(workspace) = &self.workspace {
            workspace.append_outcome(event)
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            Ok(id)
        }
    }

    fn store_note(&self, args: PrismNoteArgs) -> Result<MemoryId> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
        let task_id = self
            .session
            .task_for_mutation(args.task_id.map(TaskId::new));
        let mut entry = MemoryEntry::new(MemoryKind::Episodic, args.content);
        entry.anchors = anchors;
        entry.source = MemorySource::Agent;
        entry.trust = args.trust.unwrap_or(0.5).clamp(0.0, 1.0);
        entry.metadata = json!({ "task_id": task_id.0.clone() });
        let note_anchors = entry.anchors.clone();
        let note_content = entry.content.clone();
        let memory_id = self.session.notes.store(entry)?;
        let note_event = OutcomeEvent {
            meta: EventMeta {
                id: self.session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: Some(task_id),
                causation: None,
            },
            anchors: note_anchors,
            kind: OutcomeKind::NoteAdded,
            result: OutcomeResult::Success,
            summary: note_content,
            evidence: Vec::new(),
            metadata: Value::Null,
        };
        if let Some(workspace) = &self.workspace {
            let _ = workspace.append_outcome(note_event)?;
        } else {
            prism.apply_outcome_event_to_projections(&note_event);
            let _ = prism.outcome_memory().store_event(note_event)?;
            self.persist_outcomes()?;
        }
        self.persist_notes()?;
        Ok(memory_id)
    }

    fn store_inferred_edge(&self, args: PrismInferEdgeArgs) -> Result<EdgeId> {
        let task = self
            .session
            .task_for_mutation(args.task_id.map(TaskId::new));
        let edge = Edge {
            kind: parse_edge_kind(&args.kind)?,
            source: convert_node_id(args.source)?,
            target: convert_node_id(args.target)?,
            origin: EdgeOrigin::Inferred,
            confidence: args.confidence.clamp(0.0, 1.0),
        };
        let scope = args
            .scope
            .map(convert_inferred_scope)
            .unwrap_or(InferredEdgeScope::SessionOnly);
        let id = self.session.inferred_edges.store_edge(
            edge,
            scope,
            Some(task),
            args.evidence.unwrap_or_default(),
        );
        if scope != InferredEdgeScope::SessionOnly {
            self.persist_inferred_edges()?;
        }
        Ok(id)
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
}

fn change_impact_view(impact: ChangeImpact) -> ChangeImpactView {
    ChangeImpactView {
        direct_nodes: impact.direct_nodes.into_iter().map(node_id_view).collect(),
        lineages: impact
            .lineages
            .into_iter()
            .map(|lineage| lineage.0.to_string())
            .collect(),
        likely_validations: impact.likely_validations,
        validation_checks: impact
            .validation_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        co_change_neighbors: impact
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        risk_events: impact.risk_events,
    }
}

fn validation_recipe_view(recipe: ValidationRecipe) -> ValidationRecipeView {
    ValidationRecipeView {
        target: node_id_view(recipe.target),
        checks: recipe.checks,
        scored_checks: recipe
            .scored_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        related_nodes: recipe.related_nodes.into_iter().map(node_id_view).collect(),
        co_change_neighbors: recipe
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        recent_failures: recipe.recent_failures,
    }
}

fn scored_memory_view(memory: ScoredMemory) -> ScoredMemoryView {
    ScoredMemoryView {
        id: memory.id.0,
        entry: memory_entry_view(memory.entry),
        score: memory.score,
        source_module: memory.source_module,
        explanation: memory.explanation,
    }
}

fn memory_entry_view(entry: MemoryEntry) -> MemoryEntryView {
    MemoryEntryView {
        id: entry.id.0,
        anchors: entry.anchors,
        kind: format!("{:?}", entry.kind),
        content: entry.content,
        metadata: entry.metadata,
        created_at: entry.created_at,
        source: format!("{:?}", entry.source),
        trust: entry.trust,
    }
}

fn validation_check_view(check: ValidationCheck) -> ValidationCheckView {
    ValidationCheckView {
        label: check.label,
        score: check.score,
        last_seen: check.last_seen,
    }
}

fn co_change_view(value: CoChange) -> CoChangeView {
    CoChangeView {
        lineage: value.lineage.0.to_string(),
        count: value.count,
        nodes: value.nodes.into_iter().map(node_id_view).collect(),
    }
}

struct JsWorker {
    tx: mpsc::Sender<JsWorkerMessage>,
}

struct JsWorkerRequest {
    script: String,
    execution: QueryExecution,
    reply: mpsc::Sender<Result<String>>,
}

enum JsWorkerMessage {
    Execute(JsWorkerRequest),
}

impl JsWorker {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<JsWorkerMessage>();
        thread::spawn(move || {
            if let Err(error) = run_js_worker(rx) {
                eprintln!("prism-mcp js worker failed: {error}");
            }
        });
        Self { tx }
    }

    fn execute(&self, script: String, execution: QueryExecution) -> Result<String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(JsWorkerMessage::Execute(JsWorkerRequest {
                script,
                execution,
                reply: reply_tx,
            }))
            .map_err(|_| anyhow!("js worker is unavailable"))?;

        reply_rx
            .recv()
            .map_err(|_| anyhow!("js worker dropped the query response"))?
    }
}

fn run_js_worker(rx: mpsc::Receiver<JsWorkerMessage>) -> Result<()> {
    let runtime = Runtime::new().context("failed to create JS runtime")?;
    let context = Context::full(&runtime).context("failed to create JS context")?;
    let active_execution = Arc::new(Mutex::new(None::<QueryExecution>));

    context.with(|ctx| -> Result<()> {
        let current = active_execution.clone();
        ctx.globals().set(
            "__prismHostCall",
            Func::from(move |operation: String, args_json: String| {
                let execution = {
                    let guard = current.lock().expect("active execution lock poisoned");
                    guard.clone()
                };
                let Some(execution) = execution else {
                    return json!({
                        "ok": false,
                        "error": "no active prism query execution"
                    })
                    .to_string();
                };
                execution.dispatch_enveloped(&operation, &args_json)
            }),
        )?;
        ctx.eval::<(), _>(runtime_prelude())
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })?;

    while let Ok(message) = rx.recv() {
        match message {
            JsWorkerMessage::Execute(request) => {
                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = Some(request.execution.clone());
                }

                let result = context.with(|ctx| -> Result<String> {
                    ctx.eval::<String, _>(request.script.as_str())
                        .map_err(|err| anyhow!(err.to_string()))
                });

                let cleanup_result = context.with(|ctx| -> Result<()> {
                    ctx.eval::<(), _>("__prismCleanupGlobals()")
                        .map_err(|err| anyhow!(err.to_string()))
                });

                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = None;
                }

                let final_result = match (result, cleanup_result) {
                    (Ok(value), Ok(())) => Ok(value),
                    (Err(error), _) => Err(error),
                    (Ok(_), Err(error)) => Err(error),
                };

                let _ = request.reply.send(final_result);
            }
        }
    }

    Ok(())
}

fn symbol_view(prism: &Prism, symbol: &Symbol<'_>) -> Result<SymbolView> {
    let node = symbol.node();
    Ok(SymbolView {
        id: node_id_view(symbol.id().clone()),
        name: symbol.name().to_owned(),
        kind: node.kind,
        signature: symbol.signature(),
        file_path: prism
            .graph()
            .file_path(node.file)
            .map(|path| path.to_string_lossy().into_owned()),
        span: node.span,
        language: node.language,
        lineage_id: prism
            .lineage_of(symbol.id())
            .map(|lineage| lineage.0.to_string()),
    })
}

fn node_id_view(node: NodeId) -> NodeIdView {
    NodeIdView {
        crate_name: node.crate_name.to_string(),
        path: node.path.to_string(),
        kind: node.kind,
    }
}

fn edge_view(edge: Edge) -> EdgeView {
    EdgeView {
        kind: edge.kind,
        source: node_id_view(edge.source),
        target: node_id_view(edge.target),
        origin: edge.origin,
        confidence: edge.confidence,
    }
}

fn symbol_views_for_ids(prism: &Prism, ids: Vec<NodeId>) -> Result<Vec<SymbolView>> {
    ids.into_iter()
        .map(|id| symbol_for(prism, &id).and_then(|symbol| symbol_view(prism, &symbol)))
        .collect()
}

fn symbol_for<'a>(prism: &'a Prism, id: &NodeId) -> Result<Symbol<'a>> {
    let node = prism
        .graph()
        .node(id)
        .ok_or_else(|| anyhow!("unknown symbol `{}`", id.path))?;
    let matching = prism.search(
        &node.id.path,
        prism.graph().nodes.len().max(1),
        Some(node.kind),
        None,
    );
    matching
        .into_iter()
        .find(|symbol| symbol.id() == id)
        .ok_or_else(|| anyhow!("symbol `{}` is no longer queryable", id.path))
}

fn relations_view(prism: &Prism, session: &SessionState, id: &NodeId) -> Result<RelationsView> {
    let relations = symbol_for(prism, id)?.relations();
    Ok(RelationsView {
        contains: symbol_views_for_ids(
            prism,
            prism
                .graph()
                .edges_from(id, Some(EdgeKind::Contains))
                .into_iter()
                .map(|edge| edge.target.clone())
                .collect(),
        )?,
        callers: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.incoming_calls,
                session
                    .inferred_edges
                    .edges_to(id, Some(EdgeKind::Calls))
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
        )?,
        callees: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_calls,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Calls))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        references: symbol_views_for_ids(
            prism,
            merge_node_ids(
                prism
                    .graph()
                    .edges_from(id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|edge| edge.target.clone())
                    .collect(),
                prism
                    .graph()
                    .edges_to(id, Some(EdgeKind::References))
                    .into_iter()
                    .map(|edge| edge.source.clone()),
            ),
        )?,
        imports: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_imports,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Imports))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
        implements: symbol_views_for_ids(
            prism,
            merge_node_ids(
                relations.outgoing_implements,
                session
                    .inferred_edges
                    .edges_from(id, Some(EdgeKind::Implements))
                    .into_iter()
                    .map(|record| record.edge.target),
            ),
        )?,
    })
}

fn lineage_view(prism: &Prism, id: &NodeId) -> Result<Option<LineageView>> {
    let Some(lineage) = prism.lineage_of(id) else {
        return Ok(None);
    };
    let current = symbol_for(prism, id)?;
    let events = prism.lineage_history(&lineage);
    let status = if events
        .iter()
        .any(|event| matches!(event.kind, prism_ir::LineageEventKind::Ambiguous))
    {
        LineageStatus::Ambiguous
    } else if events
        .last()
        .is_some_and(|event| matches!(event.kind, prism_ir::LineageEventKind::Died))
    {
        LineageStatus::Dead
    } else {
        LineageStatus::Active
    };
    Ok(Some(LineageView {
        lineage_id: lineage.0.to_string(),
        current: symbol_view(prism, &current)?,
        status,
        history: events
            .into_iter()
            .map(|event| LineageEventView {
                event_id: event.meta.id.0.to_string(),
                ts: event.meta.ts,
                kind: format!("{:?}", event.kind),
                confidence: event.confidence,
            })
            .collect(),
    }))
}

fn blast_radius_view(prism: &Prism, session: &SessionState, id: &NodeId) -> ChangeImpactView {
    let mut impact = prism.blast_radius(id);
    for record in session.inferred_edges.edges_from(id, None) {
        impact.direct_nodes.push(record.edge.target);
    }
    for record in session.inferred_edges.edges_to(id, None) {
        impact.direct_nodes.push(record.edge.source);
    }
    impact.direct_nodes = merge_node_ids(impact.direct_nodes, std::iter::empty());
    change_impact_view(impact)
}

fn validation_recipe_view_with(
    prism: &Prism,
    session: &SessionState,
    id: &NodeId,
) -> ValidationRecipeView {
    let mut recipe = prism.validation_recipe(id);
    recipe.related_nodes = merge_node_ids(
        recipe.related_nodes,
        session
            .inferred_edges
            .edges_from(id, None)
            .into_iter()
            .map(|record| record.edge.target)
            .chain(
                session
                    .inferred_edges
                    .edges_to(id, None)
                    .into_iter()
                    .map(|record| record.edge.source),
            ),
    );
    validation_recipe_view(recipe)
}

#[derive(Clone)]
struct QueryExecution {
    host: QueryHost,
    prism: Arc<Prism>,
    diagnostics: Arc<Mutex<Vec<QueryDiagnostic>>>,
}

impl QueryExecution {
    fn new(host: QueryHost, prism: Arc<Prism>) -> Self {
        Self {
            host,
            prism,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn diagnostics(&self) -> Vec<QueryDiagnostic> {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .clone()
    }

    fn push_diagnostic(&self, code: &str, message: impl Into<String>, data: Option<Value>) {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .push(QueryDiagnostic {
                code: code.to_owned(),
                message: message.into(),
                data,
            });
    }

    fn dispatch_enveloped(&self, operation: &str, args_json: &str) -> String {
        match self.dispatch(operation, args_json) {
            Ok(value) => json!({ "ok": true, "value": value }).to_string(),
            Err(error) => json!({ "ok": false, "error": error.to_string() }).to_string(),
        }
    }

    fn dispatch(&self, operation: &str, args_json: &str) -> Result<Value> {
        let args = if args_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(args_json).context("failed to parse host-call arguments")?
        };

        match operation {
            "symbol" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.best_symbol(&args.query)?)?)
            }
            "symbols" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.symbols(&args.query)?)?)
            }
            "search" => {
                let args: SearchArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.search(args)?)?)
            }
            "entrypoints" => Ok(serde_json::to_value(self.entrypoints()?)?),
            "full" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(
                    symbol_for(self.prism.as_ref(), &id)?.full(),
                )?)
            }
            "relations" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(relations_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                )?)?)
            }
            "callGraph" => {
                let args: CallGraphArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.call_graph(args)?)?)
            }
            "lineage" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                let lineage = lineage_view(self.prism.as_ref(), &id)?;
                if lineage
                    .as_ref()
                    .is_some_and(|view| view.history.iter().any(|event| event.kind == "Ambiguous"))
                {
                    self.push_diagnostic(
                        "lineage_uncertain",
                        format!("Lineage for `{}` contains ambiguous history.", id.path),
                        Some(json!({ "id": id.path })),
                    );
                }
                Ok(serde_json::to_value(lineage)?)
            }
            "relatedFailures" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                serde_json::to_value(self.prism.related_failures(&id)).map_err(Into::into)
            }
            "coChangeNeighbors" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                self.host.co_change_neighbors_value(&id)
            }
            "blastRadius" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(blast_radius_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "validationRecipe" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let id = convert_node_id(args.id)?;
                Ok(serde_json::to_value(validation_recipe_view_with(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &id,
                ))?)
            }
            "resumeTask" => {
                let args: TaskTargetArgs = serde_json::from_value(args)?;
                serde_json::to_value(self.prism.resume_task(&args.task_id)).map_err(Into::into)
            }
            "memoryRecall" => {
                let args: MemoryRecallArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.memory_recall(args)?)?)
            }
            "diagnostics" => Ok(serde_json::to_value(self.diagnostics())?),
            other => {
                self.push_diagnostic(
                    "unknown_method",
                    format!("Unknown Prism host operation `{other}`."),
                    Some(json!({ "operation": other })),
                );
                Err(anyhow!("unsupported host operation `{other}`"))
            }
        }
    }

    fn best_symbol(&self, query: &str) -> Result<Option<SymbolView>> {
        let matches = self.symbols(query)?;
        if matches.is_empty() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No symbol matched `{query}`."),
                Some(json!({ "query": query })),
            );
            return Ok(None);
        }
        if matches.len() > 1 {
            self.push_diagnostic(
                "ambiguous_symbol",
                format!(
                    "`{query}` matched {} symbols; returning the first best match.",
                    matches.len()
                ),
                Some(json!({
                    "query": query,
                    "matches": matches
                        .iter()
                        .map(|symbol| symbol.id.path.to_string())
                        .collect::<Vec<_>>(),
                })),
            );
        }
        Ok(matches.into_iter().next())
    }

    fn search(&self, args: SearchArgs) -> Result<Vec<SymbolView>> {
        let _include_inferred = args.include_inferred.unwrap_or(true);
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);

        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut results = self
            .prism
            .search(
                &args.query,
                applied.saturating_add(1),
                kind,
                args.path.as_deref(),
            )
            .iter()
            .map(|symbol| symbol_view(self.prism.as_ref(), symbol))
            .collect::<Result<Vec<_>>>()?;

        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search results for `{}` were truncated at {} entries.",
                    args.query, applied
                ),
                Some(json!({
                    "query": args.query,
                    "applied": applied,
                })),
            );
        }

        Ok(results)
    }

    fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        let limits = self.host.session.limits();
        let mut results = self.symbols_from(self.prism.entrypoints())?;
        if results.len() > limits.max_result_nodes {
            results.truncate(limits.max_result_nodes);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Entrypoints were truncated at {} entries.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        Ok(results)
    }

    fn call_graph(&self, args: CallGraphArgs) -> Result<SubgraphView> {
        let limits = self.host.session.limits();
        let id = convert_node_id(args.id)?;
        let requested = args.depth.unwrap_or(DEFAULT_CALL_GRAPH_DEPTH);
        let applied = requested.min(limits.max_call_graph_depth);
        if requested > limits.max_call_graph_depth {
            self.push_diagnostic(
                "depth_limited",
                format!(
                    "Call-graph depth was capped at {} instead of {requested}.",
                    limits.max_call_graph_depth
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        let mut graph = symbol_for(self.prism.as_ref(), &id)?.call_graph(applied);
        let mut queue = vec![(id.clone(), 0usize)];
        let mut seen = std::collections::HashSet::from([id.clone()]);

        while let Some((current, depth)) = queue.pop() {
            if depth >= applied {
                continue;
            }
            for record in self
                .host
                .session
                .inferred_edges
                .edges_from(&current, Some(EdgeKind::Calls))
            {
                graph.edges.push(record.edge.clone());
                graph.nodes.push(record.edge.target.clone());
                if seen.insert(record.edge.target.clone()) {
                    queue.push((record.edge.target, depth + 1));
                }
            }
        }

        graph.nodes = merge_node_ids(graph.nodes, std::iter::empty());
        graph.edges.sort_by(|left, right| {
            left.source
                .path
                .cmp(&right.source.path)
                .then_with(|| left.target.path.cmp(&right.target.path))
                .then_with(|| edge_kind_label(left.kind).cmp(edge_kind_label(right.kind)))
        });
        graph.edges.dedup_by(|left, right| {
            left.kind == right.kind && left.source == right.source && left.target == right.target
        });
        if graph.nodes.len() > limits.max_result_nodes {
            let keep = graph
                .nodes
                .iter()
                .take(limits.max_result_nodes)
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            graph.nodes.truncate(limits.max_result_nodes);
            graph
                .edges
                .retain(|edge| keep.contains(&edge.source) && keep.contains(&edge.target));
            graph.truncated = true;
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Call graph for `{}` was truncated at {} nodes.",
                    id.path, limits.max_result_nodes
                ),
                Some(json!({
                    "query": id.path,
                    "applied": limits.max_result_nodes,
                })),
            );
        }
        graph.max_depth_reached = Some(applied);
        Ok(SubgraphView {
            nodes: symbol_views_for_ids(self.prism.as_ref(), graph.nodes)?,
            edges: graph.edges.into_iter().map(edge_view).collect(),
            truncated: graph.truncated,
            max_depth_reached: graph.max_depth_reached,
        })
    }

    fn memory_recall(&self, args: MemoryRecallArgs) -> Result<Vec<ScoredMemoryView>> {
        let requested = args.limit.unwrap_or(5);
        let limits = self.host.session.limits();
        let applied = requested.min(limits.max_result_nodes);
        if requested > limits.max_result_nodes {
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Memory recall limit was capped at {} instead of {requested}.",
                    limits.max_result_nodes
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut focus = Vec::new();
        if let Some(ids) = args.focus {
            for id in ids {
                focus.push(AnchorRef::Node(convert_node_id(id)?));
            }
        }
        let focus = self.prism.anchors_for(&focus);
        let results = self
            .host
            .session
            .notes
            .recall(&RecallQuery {
                focus,
                text: args.text,
                limit: applied,
                kinds: Some(vec![MemoryKind::Episodic]),
                since: None,
            })?
            .into_iter()
            .map(scored_memory_view)
            .collect();
        Ok(results)
    }

    fn symbols(&self, query: &str) -> Result<Vec<SymbolView>> {
        self.symbols_from(self.prism.symbol(query))
    }

    fn symbols_from<'a, I>(&self, symbols: I) -> Result<Vec<SymbolView>>
    where
        I: IntoIterator<Item = Symbol<'a>>,
    {
        symbols
            .into_iter()
            .map(|symbol| symbol_view(self.prism.as_ref(), &symbol))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SymbolQueryArgs {
    query: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
    kind: Option<String>,
    path: Option<String>,
    #[serde(alias = "includeInferred")]
    include_inferred: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SymbolTargetArgs {
    id: NodeIdInput,
}

#[derive(Debug, Deserialize)]
struct CallGraphArgs {
    id: NodeIdInput,
    depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct TaskTargetArgs {
    task_id: TaskId,
}

#[derive(Debug, Deserialize)]
struct MemoryRecallArgs {
    focus: Option<Vec<NodeIdInput>>,
    text: Option<String>,
    limit: Option<usize>,
}

fn convert_node_id(input: NodeIdInput) -> Result<NodeId> {
    Ok(NodeId::new(
        input.crate_name,
        input.path,
        parse_node_kind(&input.kind)?,
    ))
}

fn convert_anchors(inputs: Vec<AnchorRefInput>) -> Result<Vec<AnchorRef>> {
    inputs
        .into_iter()
        .map(|input| match input {
            AnchorRefInput::Node {
                crate_name,
                path,
                kind,
            } => Ok(AnchorRef::Node(NodeId::new(
                crate_name,
                path,
                parse_node_kind(&kind)?,
            ))),
            AnchorRefInput::Lineage { lineage_id } => {
                Ok(AnchorRef::Lineage(prism_ir::LineageId::new(lineage_id)))
            }
            AnchorRefInput::File { file_id } => Ok(AnchorRef::File(prism_ir::FileId(file_id))),
            AnchorRefInput::Kind { kind } => Ok(AnchorRef::Kind(parse_node_kind(&kind)?)),
        })
        .collect()
}

fn convert_outcome_kind(kind: OutcomeKindInput) -> OutcomeKind {
    match kind {
        OutcomeKindInput::NoteAdded => OutcomeKind::NoteAdded,
        OutcomeKindInput::HypothesisProposed => OutcomeKind::HypothesisProposed,
        OutcomeKindInput::PlanCreated => OutcomeKind::PlanCreated,
        OutcomeKindInput::BuildRan => OutcomeKind::BuildRan,
        OutcomeKindInput::TestRan => OutcomeKind::TestRan,
        OutcomeKindInput::ReviewFeedback => OutcomeKind::ReviewFeedback,
        OutcomeKindInput::FailureObserved => OutcomeKind::FailureObserved,
        OutcomeKindInput::RegressionObserved => OutcomeKind::RegressionObserved,
        OutcomeKindInput::FixValidated => OutcomeKind::FixValidated,
        OutcomeKindInput::RollbackPerformed => OutcomeKind::RollbackPerformed,
        OutcomeKindInput::MigrationRequired => OutcomeKind::MigrationRequired,
        OutcomeKindInput::IncidentLinked => OutcomeKind::IncidentLinked,
        OutcomeKindInput::PerfSignalObserved => OutcomeKind::PerfSignalObserved,
    }
}

fn convert_outcome_result(result: OutcomeResultInput) -> OutcomeResult {
    match result {
        OutcomeResultInput::Success => OutcomeResult::Success,
        OutcomeResultInput::Failure => OutcomeResult::Failure,
        OutcomeResultInput::Partial => OutcomeResult::Partial,
        OutcomeResultInput::Unknown => OutcomeResult::Unknown,
    }
}

fn convert_outcome_evidence(evidence: OutcomeEvidenceInput) -> OutcomeEvidence {
    match evidence {
        OutcomeEvidenceInput::Commit { sha } => OutcomeEvidence::Commit { sha },
        OutcomeEvidenceInput::Test { name, passed } => OutcomeEvidence::Test { name, passed },
        OutcomeEvidenceInput::Build { target, passed } => OutcomeEvidence::Build { target, passed },
        OutcomeEvidenceInput::Reviewer { author } => OutcomeEvidence::Reviewer { author },
        OutcomeEvidenceInput::Issue { id } => OutcomeEvidence::Issue { id },
        OutcomeEvidenceInput::StackTrace { hash } => OutcomeEvidence::StackTrace { hash },
        OutcomeEvidenceInput::DiffSummary { text } => OutcomeEvidence::DiffSummary { text },
    }
}

fn convert_inferred_scope(scope: InferredEdgeScopeInput) -> InferredEdgeScope {
    match scope {
        InferredEdgeScopeInput::SessionOnly => InferredEdgeScope::SessionOnly,
        InferredEdgeScopeInput::Persisted => InferredEdgeScope::Persisted,
        InferredEdgeScopeInput::Rejected => InferredEdgeScope::Rejected,
        InferredEdgeScopeInput::Expired => InferredEdgeScope::Expired,
    }
}

fn parse_edge_kind(value: &str) -> Result<EdgeKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "contains" => EdgeKind::Contains,
        "calls" => EdgeKind::Calls,
        "references" => EdgeKind::References,
        "implements" => EdgeKind::Implements,
        "defines" => EdgeKind::Defines,
        "imports" => EdgeKind::Imports,
        "dependson" | "depends-on" => EdgeKind::DependsOn,
        other => return Err(anyhow!("unknown edge kind `{other}`")),
    };
    Ok(kind)
}

fn edge_kind_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Contains => "contains",
        EdgeKind::Calls => "calls",
        EdgeKind::References => "references",
        EdgeKind::Implements => "implements",
        EdgeKind::Defines => "defines",
        EdgeKind::Imports => "imports",
        EdgeKind::DependsOn => "depends-on",
    }
}

fn merge_node_ids<I>(mut base: Vec<NodeId>, extra: I) -> Vec<NodeId>
where
    I: IntoIterator<Item = NodeId>,
{
    base.extend(extra);
    base.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    base.dedup();
    base
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn max_event_sequence(prism: &Prism) -> u64 {
    prism
        .outcome_snapshot()
        .events
        .into_iter()
        .filter_map(|event| event.meta.id.0.rsplit(':').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn max_task_sequence(prism: &Prism) -> u64 {
    prism
        .outcome_snapshot()
        .events
        .into_iter()
        .filter_map(|event| event.meta.correlation)
        .map(|task| task.0.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64
}

fn parse_node_kind(value: &str) -> Result<NodeKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "workspace" => NodeKind::Workspace,
        "package" => NodeKind::Package,
        "document" => NodeKind::Document,
        "module" => NodeKind::Module,
        "function" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "impl" => NodeKind::Impl,
        "method" => NodeKind::Method,
        "field" => NodeKind::Field,
        "typealias" | "type-alias" => NodeKind::TypeAlias,
        "markdownheading" | "markdown-heading" => NodeKind::MarkdownHeading,
        "jsonkey" | "json-key" => NodeKind::JsonKey,
        "yamlkey" | "yaml-key" => NodeKind::YamlKey,
        other => return Err(anyhow!("unknown node kind `{other}`")),
    };
    Ok(kind)
}

fn transpile_typescript(source: &str) -> Result<String> {
    let specifier = ModuleSpecifier::parse("file:///prism/query.ts")?;
    let parsed = parse_program(ParseParams {
        specifier,
        text: source.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    })
    .map_err(|err| anyhow!(err.to_string()))?;
    let transpiled = parsed
        .transpile(
            &TranspileOptions::default(),
            &TranspileModuleOptions::default(),
            &EmitOptions::default(),
        )
        .map_err(|err| anyhow!(err.to_string()))?
        .into_source();
    Ok(transpiled.text)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rmcp::{
        model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
        transport::{IntoTransport, Transport},
        ServiceExt,
    };

    use super::*;
    use prism_core::index_workspace_session;
    use prism_history::HistoryStore;
    use prism_ir::{
        AnchorRef, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId,
        NodeKind, Span, TaskId,
    };
    use prism_memory::{OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult};
    use prism_store::Graph;
    use std::collections::HashMap;

    fn host_with_node(node: Node) -> QueryHost {
        let mut graph = Graph::default();
        graph.nodes.insert(node.id.clone(), node);
        graph.adjacency = HashMap::new();
        graph.reverse_adjacency = HashMap::new();
        QueryHost::new(Prism::new(graph))
    }

    fn host_with_prism(prism: Prism) -> QueryHost {
        QueryHost::new(prism)
    }

    fn temp_workspace() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("prism-mcp-test-{suffix}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { beta(); }\npub fn beta() {}\n",
        )
        .unwrap();
        root
    }

    fn demo_node() -> Node {
        Node {
            id: NodeId::new("demo", "demo::main", NodeKind::Function),
            name: "main".into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::new(1, 3),
            language: Language::Rust,
        }
    }

    fn server_with_node(node: Node) -> PrismMcpServer {
        let mut graph = Graph::default();
        graph.nodes.insert(node.id.clone(), node);
        graph.adjacency = HashMap::new();
        graph.reverse_adjacency = HashMap::new();
        PrismMcpServer::new(Prism::new(graph))
    }

    fn client_message(raw: &str) -> ClientJsonRpcMessage {
        serde_json::from_str(raw).expect("invalid client json-rpc message")
    }

    fn initialize_request() -> ClientJsonRpcMessage {
        client_message(
            r#"{
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "prism-mcp-test", "version": "0.0.1" }
                }
            }"#,
        )
    }

    fn initialized_notification() -> ClientJsonRpcMessage {
        client_message(r#"{ "jsonrpc": "2.0", "method": "notifications/initialized" }"#)
    }

    fn list_tools_request(id: u64) -> ClientJsonRpcMessage {
        client_message(&format!(
            r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "tools/list" }}"#
        ))
    }

    fn list_resources_request(id: u64) -> ClientJsonRpcMessage {
        client_message(&format!(
            r#"{{ "jsonrpc": "2.0", "id": {id}, "method": "resources/list" }}"#
        ))
    }

    fn read_resource_request(id: u64, uri: &str) -> ClientJsonRpcMessage {
        serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "resources/read",
            "params": { "uri": uri },
        }))
        .expect("resources/read request should deserialize")
    }

    fn call_tool_request(
        id: u64,
        name: &str,
        arguments: serde_json::Map<String, Value>,
    ) -> ClientJsonRpcMessage {
        serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        }))
        .expect("tools/call request should deserialize")
    }

    async fn initialize_client(client: &mut impl Transport<rmcp::RoleClient>) -> serde_json::Value {
        client.send(initialize_request()).await.unwrap();
        let response = client.receive().await.unwrap();
        serde_json::to_value(response).expect("initialize response should serialize")
    }

    fn response_json(response: ServerJsonRpcMessage) -> serde_json::Value {
        serde_json::to_value(response).expect("response should serialize")
    }

    fn first_tool_content_json(response: ServerJsonRpcMessage) -> serde_json::Value {
        let response = response_json(response);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result should contain json text");
        serde_json::from_str(text).expect("tool content should decode as json")
    }

    #[test]
    fn executes_symbol_query() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result["path"], "demo::main");
        assert_eq!(result.result["kind"], "Function");
        assert!(result.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn mcp_server_advertises_tools_and_api_reference_resource() {
        let server = server_with_node(demo_node());
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move { server.serve(server_transport).await });
        let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

        let initialize = initialize_client(&mut client).await;
        assert_eq!(
            initialize["result"]["protocolVersion"],
            ProtocolVersion::LATEST.as_str()
        );
        assert!(initialize["result"]["capabilities"]["tools"].is_object());
        assert!(initialize["result"]["capabilities"]["resources"].is_object());

        client.send(initialized_notification()).await.unwrap();
        let running = server_task
            .await
            .expect("server join should succeed")
            .expect("server should initialize");

        client.send(list_tools_request(2)).await.unwrap();
        let tools = response_json(client.receive().await.unwrap());
        let tool_names = tools["result"]["tools"]
            .as_array()
            .expect("tools/list should return an array")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(tool_names.contains(&"prism_query"));
        assert!(tool_names.contains(&"prism_symbol"));
        assert!(tool_names.contains(&"prism_search"));
        assert!(tool_names.contains(&"prism_outcome"));
        assert!(tool_names.contains(&"prism_start_task"));

        client.send(list_resources_request(3)).await.unwrap();
        let resources = response_json(client.receive().await.unwrap());
        assert_eq!(
            resources["result"]["resources"][0]["uri"],
            API_REFERENCE_URI
        );
        assert_eq!(
            resources["result"]["resources"][0]["name"],
            "PRISM API Reference"
        );

        client
            .send(read_resource_request(4, API_REFERENCE_URI))
            .await
            .unwrap();
        let resource = response_json(client.receive().await.unwrap());
        let api_reference = resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("api reference should be text");
        assert!(api_reference.contains("PRISM Query API"));
        assert!(api_reference.contains("prism_query"));

        running.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_server_executes_prism_query_tool_round_trip() {
        let server = server_with_node(demo_node());
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move { server.serve(server_transport).await });
        let mut client = IntoTransport::<rmcp::RoleClient, _, _>::into_transport(client_transport);

        let _ = initialize_client(&mut client).await;
        client.send(initialized_notification()).await.unwrap();
        let running = server_task
            .await
            .expect("server join should succeed")
            .expect("server should initialize");

        client
            .send(call_tool_request(
                2,
                "prism_query",
                json!({
                    "code": r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
                    "language": "ts",
                })
                .as_object()
                .expect("tool args should be an object")
                .clone(),
            ))
            .await
            .unwrap();

        let envelope = first_tool_content_json(client.receive().await.unwrap());
        assert_eq!(envelope["result"]["path"], "demo::main");
        assert_eq!(envelope["result"]["kind"], "Function");
        assert_eq!(
            envelope["diagnostics"]
                .as_array()
                .map(|diagnostics| diagnostics.len()),
            Some(0)
        );

        running.cancel().await.unwrap();
    }

    #[test]
    fn js_views_use_camel_case_and_enriched_nested_symbols() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

        let mut graph = Graph::new();
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: alpha.clone(),
            target: beta,
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone()]);

        let host = host_with_prism(Prism::with_history(graph, history));
        let result = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
const graph = sym?.callGraph(1);
const lineage = sym?.lineage();
return {
  crateName: sym?.id.crateName,
  callees: sym?.relations().callees.map((node) => node.id.path) ?? [],
  graphNodes: graph?.nodes.map((node) => node.id.path) ?? [],
  graphDepth: graph?.maxDepthReached ?? null,
  lineageId: lineage?.lineageId ?? null,
  lineageStatus: lineage?.status ?? null,
  currentPath: lineage?.current.id.path ?? null,
};
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert_eq!(result.result["crateName"], "demo");
        assert_eq!(result.result["callees"][0], "demo::beta");
        assert_eq!(result.result["graphNodes"][0], "demo::alpha");
        assert_eq!(result.result["graphNodes"][1], "demo::beta");
        assert_eq!(result.result["graphDepth"], 1);
        assert!(result.result["lineageId"]
            .as_str()
            .unwrap_or_default()
            .starts_with("lineage:"));
        assert_eq!(result.result["lineageStatus"], "active");
        assert_eq!(result.result["currentPath"], "demo::alpha");
    }

    #[test]
    fn custom_query_limits_apply_per_host() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: NodeId::new("demo", "demo::beta", NodeKind::Function),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
            target: NodeId::new("demo", "demo::beta", NodeKind::Function),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let host = QueryHost::new_with_limits(
            Prism::new(graph),
            QueryLimits {
                max_result_nodes: 1,
                max_call_graph_depth: 1,
                max_output_json_bytes: 512,
            },
        );

        let search = host
            .execute(
                r#"
return prism.search("a", { limit: 10 }).map((sym) => sym.id.path);
"#,
                QueryLanguage::Ts,
            )
            .expect("search should succeed");
        assert_eq!(search.result.as_array().map(|items| items.len()), Some(1));
        assert_eq!(search.diagnostics[0].code, "result_truncated");

        let depth = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
return sym?.callGraph(9);
"#,
                QueryLanguage::Ts,
            )
            .expect("call graph should succeed");
        assert_eq!(depth.result["maxDepthReached"], 1);
        assert!(depth
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "depth_limited"));

        let capped = QueryHost::new_with_limits(
            Prism::new(Graph::new()),
            QueryLimits {
                max_result_nodes: 1,
                max_call_graph_depth: 1,
                max_output_json_bytes: 32,
            },
        )
        .execute(
            r#"
return "abcdefghijklmnopqrstuvwxyz0123456789";
"#,
            QueryLanguage::Ts,
        )
        .expect("query should succeed");
        assert_eq!(capped.result, Value::Null);
        assert!(capped
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "result_truncated"));
    }

    #[test]
    fn search_kind_filter_uses_cli_style_names() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
return prism.search("main", { kind: "function" });
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
    }

    #[test]
    fn reports_diagnostics_for_overbroad_searches() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
prism.search("main", { limit: 1000 });
return prism.diagnostics();
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "result_truncated");
    }

    #[test]
    fn reuses_warm_runtime_across_queries() {
        let host = host_with_node(demo_node());

        let first = host
            .execute(
                r#"
const sym = prism.symbol("main");
return sym?.id.path;
"#,
                QueryLanguage::Ts,
            )
            .expect("first query should succeed");
        let second = host
            .execute(
                r#"
return prism.entrypoints().map((sym) => sym.id.path);
"#,
                QueryLanguage::Ts,
            )
            .expect("second query should succeed");

        assert_eq!(first.result, Value::String("demo::main".to_owned()));
        assert_eq!(second.result.as_array().map(|items| items.len()), Some(1));
    }

    #[test]
    fn cleans_up_user_globals_between_queries() {
        let host = host_with_node(demo_node());

        host.execute(
            r#"
globalThis.__prismLeaked = 1;
return true;
"#,
            QueryLanguage::Ts,
        )
        .expect("first query should succeed");

        let second = host
            .execute(
                r#"
return typeof globalThis.__prismLeaked;
"#,
                QueryLanguage::Ts,
            )
            .expect("second query should succeed");

        assert_eq!(second.result, Value::String("undefined".to_owned()));
    }

    #[test]
    fn exposes_blast_radius_and_related_failures() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

        let mut graph = Graph::new();
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });
        graph.add_edge(Edge {
            kind: EdgeKind::Calls,
            source: alpha.clone(),
            target: beta.clone(),
            origin: prism_ir::EdgeOrigin::Static,
            confidence: 1.0,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:test"),
                    ts: 10,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha previously failed".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_unit".into(),
                    passed: false,
                }],
                metadata: Value::Null,
            })
            .expect("outcome event should store");

        let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
        let result = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
return {
  blast: sym ? prism.blastRadius(sym) : null,
  failures: sym ? prism.relatedFailures(sym) : [],
};
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert_eq!(
            result.result["blast"]["directNodes"][0]["path"],
            "demo::beta"
        );
        assert_eq!(
            result.result["failures"][0]["summary"],
            "alpha previously failed"
        );
    }

    #[test]
    fn exposes_validation_recipe() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);

        let mut graph = Graph::new();
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone()]);

        let outcomes = OutcomeMemory::new();
        outcomes
            .store_event(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:9"),
                    ts: 9,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha.clone())],
                kind: OutcomeKind::FailureObserved,
                result: OutcomeResult::Failure,
                summary: "alpha broke validation".into(),
                evidence: vec![OutcomeEvidence::Test {
                    name: "alpha_validation".into(),
                    passed: false,
                }],
                metadata: Value::Null,
            })
            .expect("outcome event should store");

        let host = host_with_prism(Prism::with_history_and_outcomes(graph, history, outcomes));
        let result = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
return sym ? prism.validationRecipe(sym) : null;
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert_eq!(result.result["target"]["path"], "demo::alpha");
        assert_eq!(
            result.result["checks"][0],
            Value::String("test:alpha_validation".to_string())
        );
        assert_eq!(
            result.result["scoredChecks"][0]["label"],
            Value::String("test:alpha_validation".to_string())
        );
        assert_eq!(
            result.result["recentFailures"][0]["summary"],
            "alpha broke validation"
        );
    }

    #[test]
    fn exposes_co_change_neighbors() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

        let mut graph = Graph::new();
        for (id, line) in [(&alpha, 1), (&beta, 2)] {
            graph.add_node(Node {
                id: id.clone(),
                name: id.path.rsplit("::").next().unwrap().into(),
                kind: NodeKind::Function,
                file: FileId(1),
                span: Span::line(line),
                language: Language::Rust,
            });
        }

        let mut history = HistoryStore::new();
        history.seed_nodes([alpha.clone(), beta.clone()]);
        history.apply(&prism_ir::ObservedChangeSet {
            meta: EventMeta {
                id: EventId::new("observed:1"),
                ts: 1,
                actor: EventActor::System,
                correlation: None,
                causation: None,
            },
            trigger: prism_ir::ChangeTrigger::ManualReindex,
            files: vec![FileId(1)],
            added: Vec::new(),
            removed: Vec::new(),
            updated: vec![
                (
                    prism_ir::ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            1,
                            Some(10),
                            None,
                            None,
                        ),
                    },
                    prism_ir::ObservedNode {
                        node: Node {
                            id: alpha.clone(),
                            name: "alpha".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(1),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            1,
                            Some(11),
                            None,
                            None,
                        ),
                    },
                ),
                (
                    prism_ir::ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            2,
                            Some(20),
                            None,
                            None,
                        ),
                    },
                    prism_ir::ObservedNode {
                        node: Node {
                            id: beta.clone(),
                            name: "beta".into(),
                            kind: NodeKind::Function,
                            file: FileId(1),
                            span: Span::line(2),
                            language: Language::Rust,
                        },
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(
                            2,
                            Some(21),
                            None,
                            None,
                        ),
                    },
                ),
            ],
            edge_added: Vec::new(),
            edge_removed: Vec::new(),
        });

        let host = host_with_prism(Prism::with_history(graph, history));
        let result = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
return sym ? prism.coChangeNeighbors(sym) : [];
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert_eq!(result.result[0]["count"], 1);
        assert_eq!(result.result[0]["nodes"][0]["path"], "demo::beta");
    }

    #[test]
    fn inferred_edge_overlay_affects_relations_queries() {
        let alpha = NodeId::new("demo", "demo::alpha", NodeKind::Function);
        let beta = NodeId::new("demo", "demo::beta", NodeKind::Function);

        let mut graph = Graph::new();
        graph.add_node(Node {
            id: alpha.clone(),
            name: "alpha".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(1),
            language: Language::Rust,
        });
        graph.add_node(Node {
            id: beta.clone(),
            name: "beta".into(),
            kind: NodeKind::Function,
            file: FileId(1),
            span: Span::line(2),
            language: Language::Rust,
        });

        let host = host_with_prism(Prism::new(graph));
        host.store_inferred_edge(PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.9,
            scope: Some(InferredEdgeScopeInput::SessionOnly),
            evidence: Some(vec!["task-local inference".to_string()]),
            task_id: None,
        })
        .expect("inferred edge should store");

        let result = host
            .execute(
                r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert_eq!(result.result[0], "demo::beta");
    }

    #[test]
    fn persisted_inferred_edges_reload_with_workspace_session() {
        let root = temp_workspace();
        let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

        host.store_inferred_edge(PrismInferEdgeArgs {
            source: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            },
            target: NodeIdInput {
                crate_name: "demo".to_string(),
                path: "demo::beta".to_string(),
                kind: "function".to_string(),
            },
            kind: "calls".to_string(),
            confidence: 0.95,
            scope: Some(InferredEdgeScopeInput::Persisted),
            evidence: Some(vec!["persisted inference".to_string()]),
            task_id: Some("task:persist".to_string()),
        })
        .expect("inferred edge should persist");

        let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
        let result = reloaded
            .execute(
                r#"
const sym = prism.symbol("alpha");
return sym ? sym.relations().callees.map((node) => node.id.path) : [];
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");

        assert!(result
            .result
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "demo::beta"));
    }

    #[test]
    fn auto_refreshes_workspace_and_records_patch_events() {
        let root = temp_workspace();
        let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() { gamma(); }\npub fn gamma() {}\n",
        )
        .unwrap();

        let result = host
            .execute(
                r#"
const sym = prism.symbol("gamma");
const alpha = prism.symbol("alpha");
return {
  path: sym?.id.path,
  callers: alpha ? alpha.relations().callees.map((node) => node.id.path) : [],
};
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed after external edit");

        assert_eq!(result.result["path"], "demo::gamma");
        assert!(result.result["callers"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "demo::gamma"));

        let patch_events = host
            .current_prism()
            .outcome_memory()
            .outcomes_for(
                &[AnchorRef::Node(NodeId::new(
                    "demo",
                    "demo::gamma",
                    NodeKind::Function,
                ))],
                10,
            )
            .into_iter()
            .filter(|event| event.kind == OutcomeKind::PatchApplied)
            .collect::<Vec<_>>();
        assert_eq!(patch_events.len(), 1);
    }

    #[test]
    fn convenience_symbol_query_returns_diagnostics() {
        let host = host_with_node(demo_node());

        let envelope = host
            .symbol_query("missing")
            .expect("symbol query should succeed");
        assert!(envelope.result.is_object() || envelope.result.is_null());
        assert!(envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "anchor_unresolved"));
    }

    #[test]
    fn convenience_search_query_returns_structured_envelope() {
        let host = host_with_node(demo_node());

        let envelope = host
            .search_query(SearchArgs {
                query: "main".to_string(),
                limit: Some(1),
                kind: None,
                path: None,
                include_inferred: None,
            })
            .expect("search query should succeed");
        assert!(envelope.result.is_array());
        assert!(envelope.diagnostics.is_empty());
    }

    #[test]
    fn first_mutation_auto_creates_session_task() {
        let host = host_with_node(demo_node());

        let memory_id = host
            .store_note(PrismNoteArgs {
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                content: "remember this".to_string(),
                trust: Some(0.8),
                task_id: None,
            })
            .expect("note should store");

        assert!(memory_id.0.starts_with("episodic:"));
        let task = host.session.current_task().expect("task should be created");
        let replay = host.current_prism().resume_task(&task);
        assert_eq!(replay.task, task);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);
    }

    #[test]
    fn recalls_session_memory_for_symbol_focus() {
        let host = host_with_node(demo_node());

        host.store_note(PrismNoteArgs {
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::main".to_string(),
                kind: "function".to_string(),
            }],
            content: "main previously regressed on null handling".to_string(),
            trust: Some(0.9),
            task_id: None,
        })
        .expect("note should store");

        let result = host
            .execute(
                r#"
const sym = prism.symbol("main");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "null",
  limit: 5,
});
"#,
                QueryLanguage::Ts,
            )
            .expect("memory recall should succeed");

        assert_eq!(
            result.result[0]["entry"]["content"],
            "main previously regressed on null handling"
        );
        assert_eq!(result.result[0]["entry"]["kind"], "Episodic");
    }

    #[test]
    fn explicit_start_task_sets_session_default_and_logs_plan() {
        let host = host_with_node(demo_node());

        let task = host
            .start_task("Investigate main".to_string(), vec!["bug".to_string()])
            .expect("task should start");

        assert_eq!(host.session.current_task(), Some(task.clone()));
        let replay = host.current_prism().resume_task(&task);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].kind, OutcomeKind::PlanCreated);
        assert_eq!(replay.events[0].summary, "Investigate main");
        assert_eq!(replay.events[0].metadata["tags"][0], "bug");
    }

    #[test]
    fn explicit_task_override_does_not_replace_session_default() {
        let host = host_with_node(demo_node());
        let active = host
            .start_task("Primary task".to_string(), Vec::new())
            .expect("task should start");

        let explicit = TaskId::new("task:secondary:99");
        let event_id = host
            .store_outcome(PrismOutcomeArgs {
                kind: OutcomeKindInput::FailureObserved,
                anchors: vec![AnchorRefInput::Node {
                    crate_name: "demo".to_string(),
                    path: "demo::main".to_string(),
                    kind: "function".to_string(),
                }],
                summary: "secondary failure".to_string(),
                result: Some(OutcomeResultInput::Failure),
                evidence: None,
                task_id: Some(explicit.0.to_string()),
            })
            .expect("outcome should store");

        assert_eq!(host.session.current_task(), Some(active));
        let replay = host.current_prism().resume_task(&explicit);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].meta.id, event_id);
    }
}
