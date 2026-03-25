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
    api_reference_markdown, runtime_prelude, ChangeImpactView, CoChangeView, LineageView,
    QueryDiagnostic, QueryEnvelope, RelationsView, SymbolView, ValidationCheckView,
    ValidationRecipeView, API_REFERENCE_URI,
};
use prism_memory::{
    EpisodicMemory, MemoryEntry, MemoryId, MemoryKind, MemoryModule, MemorySource, OutcomeEvent,
    OutcomeEvidence, OutcomeKind, OutcomeResult,
};
use prism_query::{ChangeImpact, CoChange, Prism, Symbol, ValidationCheck, ValidationRecipe};
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
const MAX_SEARCH_LIMIT: usize = 50;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const MAX_CALL_GRAPH_DEPTH: usize = 5;
const MAX_ENTRYPOINTS: usize = 100;

struct SessionState {
    notes: EpisodicMemory,
    inferred_edges: InferenceStore,
    current_task: Mutex<Option<TaskId>>,
    next_event: AtomicU64,
}

impl SessionState {
    fn new(prism: &Prism) -> Self {
        Self::with_snapshots(prism, EpisodicMemory::new(), InferenceStore::new())
    }

    fn with_snapshots(prism: &Prism, notes: EpisodicMemory, inferred_edges: InferenceStore) -> Self {
        Self {
            notes,
            inferred_edges,
            current_task: Mutex::new(None),
            next_event: AtomicU64::new(max_event_sequence(prism)),
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

    fn remember_task(&self, task: Option<TaskId>) {
        if let Some(task) = task {
            *self
                .current_task
                .lock()
                .expect("session task lock poisoned") = Some(task);
        }
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
        Self {
            tool_router: Self::tool_router(),
            host: Arc::new(QueryHost::new(prism)),
        }
    }

    pub fn with_session(session: WorkspaceSession) -> Self {
        Self {
            tool_router: Self::tool_router(),
            host: Arc::new(QueryHost::with_session(session)),
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct NodeIdInput {
    crate_name: String,
    path: String,
    kind: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnchorRefInput {
    Node {
        crate_name: String,
        path: String,
        kind: String,
    },
    Lineage {
        lineage_id: String,
    },
    File {
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
struct PrismOutcomeArgs {
    kind: OutcomeKindInput,
    anchors: Vec<AnchorRefInput>,
    summary: String,
    result: Option<OutcomeResultInput>,
    evidence: Option<Vec<OutcomeEvidenceInput>>,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismNoteArgs {
    anchors: Vec<AnchorRefInput>,
    content: String,
    trust: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismInferEdgeArgs {
    source: NodeIdInput,
    target: NodeIdInput,
    kind: String,
    confidence: f32,
    scope: Option<InferredEdgeScopeInput>,
    evidence: Option<Vec<String>>,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismTestRanArgs {
    anchors: Vec<AnchorRefInput>,
    test: String,
    passed: bool,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismFailureObservedArgs {
    anchors: Vec<AnchorRefInput>,
    summary: String,
    trace: Option<String>,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismFixValidatedArgs {
    anchors: Vec<AnchorRefInput>,
    summary: String,
    task_id: Option<String>,
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        name = "prism_query",
        description = "Execute a read-only TypeScript query against the live PRISM graph. Read prism://api-reference for the available prism API."
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
        let content = Content::json(envelope).map_err(|err| {
            McpError::internal_error(
                "failed to serialize query result",
                Some(json!({ "error": err.to_string() })),
            )
        })?;
        Ok(CallToolResult::success(vec![content]))
    }

    #[tool(
        description = "Write a structured outcome event for the current task or symbol anchors."
    )]
    fn prism_outcome(
        &self,
        Parameters(args): Parameters<PrismOutcomeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let event_id = self.host.store_outcome(args).map_err(map_query_error)?;
        Ok(CallToolResult::success(vec![Content::json(event_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize outcome id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
    }

    #[tool(description = "Store an agent note anchored to nodes or lineages.")]
    fn prism_note(
        &self,
        Parameters(args): Parameters<PrismNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let memory_id = self.host.store_note(args).map_err(map_query_error)?;
        Ok(CallToolResult::success(vec![Content::json(memory_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize memory id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
    }

    #[tool(description = "Persist an inferred edge into the session overlay or a promoted scope.")]
    fn prism_infer_edge(
        &self,
        Parameters(args): Parameters<PrismInferEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let edge_id = self
            .host
            .store_inferred_edge(args)
            .map_err(map_query_error)?;
        Ok(CallToolResult::success(vec![Content::json(edge_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize edge id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
    }

    #[tool(description = "Convenience outcome for a test run.")]
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
        Ok(CallToolResult::success(vec![Content::json(event_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize outcome id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
    }

    #[tool(description = "Convenience outcome for an observed failure.")]
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
        Ok(CallToolResult::success(vec![Content::json(event_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize outcome id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
    }

    #[tool(description = "Convenience outcome for a validated fix.")]
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
        Ok(CallToolResult::success(vec![Content::json(event_id)
            .map_err(|err| {
                McpError::internal_error(
                    "failed to serialize outcome id",
                    Some(json!({ "error": err.to_string() })),
                )
            })?]))
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
            "Use the prism_query tool for read-only programmable graph queries and read prism://api-reference for the typed PRISM query surface.",
        )
        .with_protocol_version(ProtocolVersion::LATEST)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![RawResource::new(API_REFERENCE_URI, "PRISM API Reference")
                .with_description("TypeScript query surface, d.ts-style contract, and recipes")
                .no_annotation()],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri.as_str() != API_REFERENCE_URI {
            return Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({ "uri": request.uri })),
            ));
        }

        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            api_reference_markdown(),
            request.uri,
        )]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            next_cursor: None,
            resource_templates: Vec::new(),
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

#[derive(Clone)]
struct QueryHost {
    prism: Arc<Prism>,
    session: Arc<SessionState>,
    worker: Arc<JsWorker>,
    workspace: Option<Arc<WorkspaceSession>>,
}

impl QueryHost {
    fn new(prism: Prism) -> Self {
        let prism = Arc::new(prism);
        let session = Arc::new(SessionState::new(prism.as_ref()));
        Self {
            prism: prism.clone(),
            session,
            worker: Arc::new(JsWorker::spawn()),
            workspace: None,
        }
    }

    fn with_session(workspace: WorkspaceSession) -> Self {
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

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let source = format!(
            "(function() {{\n  const __prismUserQuery = () => {{\n{}\n  }};\n  const __prismResult = __prismUserQuery();\n  return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n}})();\n",
            code
        );
        let transpiled = transpile_typescript(&source)?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let raw_result = self.worker.execute(transpiled, execution.clone())?;
        let result =
            serde_json::from_str(&raw_result).context("failed to decode query result JSON")?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn co_change_neighbors_value(&self, id: &NodeId) -> Result<Value> {
        let prism = self.current_prism();
        serde_json::to_value(prism.co_change_neighbors(id, 8)).map_err(Into::into)
    }

    fn store_outcome(&self, args: PrismOutcomeArgs) -> Result<EventId> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
        let task_id = args
            .task_id
            .map(TaskId::new)
            .or_else(|| self.session.current_task());
        self.session.remember_task(task_id.clone());
        let event = OutcomeEvent {
            meta: EventMeta {
                id: self.session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: task_id,
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
        let id = prism.outcome_memory().store_event(event)?;
        self.persist_outcomes()?;
        Ok(id)
    }

    fn store_note(&self, args: PrismNoteArgs) -> Result<MemoryId> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let anchors = prism.anchors_for(&convert_anchors(args.anchors)?);
        let mut entry = MemoryEntry::new(MemoryKind::Episodic, args.content);
        entry.anchors = anchors;
        entry.source = MemorySource::Agent;
        entry.trust = args.trust.unwrap_or(0.5).clamp(0.0, 1.0);
        let note_anchors = entry.anchors.clone();
        let note_content = entry.content.clone();
        let memory_id = self.session.notes.store(entry)?;
        let _ = prism.outcome_memory().store_event(OutcomeEvent {
            meta: EventMeta {
                id: self.session.next_event_id("outcome"),
                ts: current_timestamp(),
                actor: EventActor::Agent,
                correlation: self.session.current_task(),
                causation: None,
            },
            anchors: note_anchors,
            kind: OutcomeKind::NoteAdded,
            result: OutcomeResult::Success,
            summary: note_content,
            evidence: Vec::new(),
            metadata: Value::Null,
        });
        self.persist_outcomes()?;
        self.persist_notes()?;
        Ok(memory_id)
    }

    fn store_inferred_edge(&self, args: PrismInferEdgeArgs) -> Result<EdgeId> {
        let task = args
            .task_id
            .map(TaskId::new)
            .or_else(|| self.session.current_task());
        self.session.remember_task(task.clone());
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
            task,
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
        direct_nodes: impact.direct_nodes,
        lineages: impact.lineages,
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
        target: recipe.target,
        checks: recipe.checks,
        scored_checks: recipe
            .scored_checks
            .into_iter()
            .map(validation_check_view)
            .collect(),
        related_nodes: recipe.related_nodes,
        co_change_neighbors: recipe
            .co_change_neighbors
            .into_iter()
            .map(co_change_view)
            .collect(),
        recent_failures: recipe.recent_failures,
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
        lineage: value.lineage,
        count: value.count,
        nodes: value.nodes,
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
        id: symbol.id().clone(),
        name: symbol.name().to_owned(),
        kind: node.kind,
        signature: symbol.signature(),
        file_path: prism
            .graph()
            .file_path(node.file)
            .map(|path| path.to_string_lossy().into_owned()),
        span: node.span,
        language: node.language,
        lineage_id: prism.lineage_of(symbol.id()),
    })
}

fn symbol_for<'a>(prism: &'a Prism, id: &NodeId) -> Result<Symbol<'a>> {
    let node = prism
        .graph()
        .node(id)
        .ok_or_else(|| anyhow!("unknown symbol `{}`", id.path))?;
    let matching = prism.search(&node.id.path, prism.graph().nodes.len().max(1), Some(node.kind), None);
    matching
        .into_iter()
        .find(|symbol| symbol.id() == id)
        .ok_or_else(|| anyhow!("symbol `{}` is no longer queryable", id.path))
}

fn relations_view(prism: &Prism, session: &SessionState, id: &NodeId) -> Result<RelationsView> {
    let relations = symbol_for(prism, id)?.relations();
    Ok(RelationsView {
        outgoing_calls: merge_node_ids(
            relations.outgoing_calls,
            session
                .inferred_edges
                .edges_from(id, Some(EdgeKind::Calls))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        incoming_calls: merge_node_ids(
            relations.incoming_calls,
            session
                .inferred_edges
                .edges_to(id, Some(EdgeKind::Calls))
                .into_iter()
                .map(|record| record.edge.source),
        ),
        outgoing_imports: merge_node_ids(
            relations.outgoing_imports,
            session
                .inferred_edges
                .edges_from(id, Some(EdgeKind::Imports))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        incoming_imports: merge_node_ids(
            relations.incoming_imports,
            session
                .inferred_edges
                .edges_to(id, Some(EdgeKind::Imports))
                .into_iter()
                .map(|record| record.edge.source),
        ),
        outgoing_implements: merge_node_ids(
            relations.outgoing_implements,
            session
                .inferred_edges
                .edges_from(id, Some(EdgeKind::Implements))
                .into_iter()
                .map(|record| record.edge.target),
        ),
        incoming_implements: merge_node_ids(
            relations.incoming_implements,
            session
                .inferred_edges
                .edges_to(id, Some(EdgeKind::Implements))
                .into_iter()
                .map(|record| record.edge.source),
        ),
    })
}

fn lineage_view(prism: &Prism, id: &NodeId) -> Result<Option<LineageView>> {
    let Some(lineage) = prism.lineage_of(id) else {
        return Ok(None);
    };
    Ok(Some(LineageView {
        events: prism.lineage_history(&lineage),
        lineage,
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
                Ok(serde_json::to_value(symbol_for(self.prism.as_ref(), &args.id)?.full())?)
            }
            "relations" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(relations_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &args.id,
                )?)?)
            }
            "callGraph" => {
                let args: CallGraphArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.call_graph(args)?)?)
            }
            "lineage" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                let lineage = lineage_view(self.prism.as_ref(), &args.id)?;
                if lineage
                    .as_ref()
                    .is_some_and(|view| {
                        view.events.iter().any(|event| {
                            matches!(event.kind, prism_ir::LineageEventKind::Ambiguous)
                        })
                    })
                {
                    self.push_diagnostic(
                        "lineage_uncertain",
                        format!("Lineage for `{}` contains ambiguous history.", args.id.path),
                        Some(json!({ "id": args.id.path })),
                    );
                }
                Ok(serde_json::to_value(lineage)?)
            }
            "relatedFailures" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                serde_json::to_value(self.prism.related_failures(&args.id)).map_err(Into::into)
            }
            "coChangeNeighbors" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                self.host.co_change_neighbors_value(&args.id)
            }
            "blastRadius" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(blast_radius_view(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &args.id,
                ))?)
            }
            "validationRecipe" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(validation_recipe_view_with(
                    self.prism.as_ref(),
                    self.host.session.as_ref(),
                    &args.id,
                ))?)
            }
            "resumeTask" => {
                let args: TaskTargetArgs = serde_json::from_value(args)?;
                serde_json::to_value(self.prism.resume_task(&args.task_id)).map_err(Into::into)
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
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(MAX_SEARCH_LIMIT);

        if requested > MAX_SEARCH_LIMIT {
            self.push_diagnostic(
                "result_truncated",
                format!("Search limit was capped at {MAX_SEARCH_LIMIT} instead of {requested}."),
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
        let mut results = self.symbols_from(self.prism.entrypoints())?;
        if results.len() > MAX_ENTRYPOINTS {
            results.truncate(MAX_ENTRYPOINTS);
            self.push_diagnostic(
                "result_truncated",
                format!("Entrypoints were truncated at {} entries.", MAX_ENTRYPOINTS),
                Some(json!({
                    "applied": MAX_ENTRYPOINTS,
                })),
            );
        }
        Ok(results)
    }

    fn call_graph(&self, args: CallGraphArgs) -> Result<prism_ir::Subgraph> {
        let requested = args.depth.unwrap_or(DEFAULT_CALL_GRAPH_DEPTH);
        let applied = requested.min(MAX_CALL_GRAPH_DEPTH);
        if requested > MAX_CALL_GRAPH_DEPTH {
            self.push_diagnostic(
                "depth_limited",
                format!(
                    "Call-graph depth was capped at {MAX_CALL_GRAPH_DEPTH} instead of {requested}."
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        let mut graph = symbol_for(self.prism.as_ref(), &args.id)?.call_graph(applied);
        let mut queue = vec![(args.id.clone(), 0usize)];
        let mut seen = std::collections::HashSet::from([args.id.clone()]);

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
        Ok(graph)
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

#[derive(Debug, Deserialize)]
struct SymbolQueryArgs {
    query: String,
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
    kind: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SymbolTargetArgs {
    id: NodeId,
}

#[derive(Debug, Deserialize)]
struct CallGraphArgs {
    id: NodeId,
    depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct TaskTargetArgs {
    task_id: TaskId,
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
    prism.outcome_snapshot()
        .events
        .into_iter()
        .filter_map(|event| event.meta.id.0.rsplit(':').next()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
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

    use prism_core::index_workspace_session;
    use super::*;
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
        fs::write(root.join("src/lib.rs"), "pub fn alpha() { beta(); }\npub fn beta() {}\n").unwrap();
        root
    }

    fn demo_node() -> Node {
        Node {
            id: NodeId::new("demo", "demo::main", NodeKind::Function),
            name: "main".into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::new(1, 1, 3, 1),
            language: Language::Rust,
        }
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
            result.result["blast"]["direct_nodes"][0]["path"],
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
            result.result["scored_checks"][0]["label"],
            Value::String("test:alpha_validation".to_string())
        );
        assert_eq!(
            result.result["recent_failures"][0]["summary"],
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
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(10), None, None),
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
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(1, Some(11), None, None),
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
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(20), None, None),
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
                        fingerprint: prism_ir::SymbolFingerprint::with_parts(2, Some(21), None, None),
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
return sym?.relations().outgoing_calls.map((node) => node.path) ?? [];
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
return sym?.relations().outgoing_calls.map((node) => node.path) ?? [];
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
return { path: sym?.id.path, callers: prism.symbol("alpha")?.relations().outgoing_calls.map((node) => node.path) ?? [] };
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
}
