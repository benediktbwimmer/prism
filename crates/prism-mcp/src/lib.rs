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
use prism_coordination::{
    AcceptanceCriterion, ArtifactProposeInput, ArtifactReviewInput, ArtifactSupersedeInput,
    ClaimAcquireInput, CoordinationPolicy, HandoffInput, PlanCreateInput, TaskCreateInput,
    TaskUpdateInput,
};
use prism_core::{index_workspace_session, WorkspaceSession};
use prism_curator::{
    CuratorJobId, CuratorJobRecord, CuratorProposal, CuratorProposalDisposition, CuratorTrigger,
};
use prism_ir::{
    AgentId, AnchorRef, ArtifactId, Capability, ClaimId, ClaimMode, CoordinationTaskId,
    CoordinationTaskStatus, Edge, EdgeKind, EdgeOrigin, EventActor, EventId, EventMeta, LineageId,
    NodeId, NodeKind, PlanId, ReviewVerdict, SessionId, TaskId, WorkspaceRevision,
};
use prism_js::{
    api_reference_markdown, runtime_prelude, ArtifactView, BlockerView, ChangeImpactView,
    ClaimView, CoChangeView, ConflictView, CoordinationTaskView, CuratorJobView,
    CuratorProposalView, EdgeView, LineageEventView, LineageStatus, LineageView, MemoryEntryView,
    NodeIdView, PlanView, QueryDiagnostic, QueryEnvelope, RelationsView, ScoredMemoryView,
    SubgraphView, SymbolView, ValidationCheckView, ValidationRecipeView, WorkspaceRevisionView,
    API_REFERENCE_URI,
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
const DEFAULT_RESOURCE_PAGE_LIMIT: usize = 50;
const ENTRYPOINTS_URI: &str = "prism://entrypoints";
const SESSION_URI: &str = "prism://session";
const ENTRYPOINTS_RESOURCE_TEMPLATE_URI: &str = "prism://entrypoints?limit={limit}&cursor={cursor}";
const SYMBOL_RESOURCE_TEMPLATE_URI: &str = "prism://symbol/{crateName}/{kind}/{path}";
const SEARCH_RESOURCE_TEMPLATE_URI: &str = "prism://search/{query}?limit={limit}&cursor={cursor}";
const LINEAGE_RESOURCE_TEMPLATE_URI: &str =
    "prism://lineage/{lineageId}?limit={limit}&cursor={cursor}";
const TASK_RESOURCE_TEMPLATE_URI: &str = "prism://task/{taskId}?limit={limit}&cursor={cursor}";
const EVENT_RESOURCE_TEMPLATE_URI: &str = "prism://event/{eventId}";
const MEMORY_RESOURCE_TEMPLATE_URI: &str = "prism://memory/{memoryId}";
const EDGE_RESOURCE_TEMPLATE_URI: &str = "prism://edge/{edgeId}";
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct SessionTaskState {
    id: TaskId,
    description: Option<String>,
    tags: Vec<String>,
}

struct SessionState {
    session_id: SessionId,
    notes: EpisodicMemory,
    inferred_edges: InferenceStore,
    current_task: Mutex<Option<SessionTaskState>>,
    next_event: AtomicU64,
    next_task: AtomicU64,
    limits: Mutex<QueryLimits>,
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
            session_id: SessionId::new(format!(
                "session:{}",
                NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            )),
            notes,
            inferred_edges,
            current_task: Mutex::new(None),
            next_event: AtomicU64::new(max_event_sequence(prism)),
            next_task: AtomicU64::new(max_task_sequence(prism)),
            limits: Mutex::new(limits),
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
            .as_ref()
            .map(|task| task.id.clone())
    }

    fn session_id(&self) -> SessionId {
        self.session_id.clone()
    }

    fn current_task_state(&self) -> Option<SessionTaskState> {
        self.current_task
            .lock()
            .expect("session task lock poisoned")
            .clone()
    }

    fn set_current_task(&self, task: TaskId, description: Option<String>, tags: Vec<String>) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = Some(SessionTaskState {
            id: task,
            description,
            tags,
        });
    }

    fn update_current_task_metadata(
        &self,
        description: Option<Option<String>>,
        tags: Option<Vec<String>>,
    ) {
        if let Some(task) = self
            .current_task
            .lock()
            .expect("session task lock poisoned")
            .as_mut()
        {
            if let Some(description) = description {
                task.description = description;
            }
            if let Some(tags) = tags {
                task.tags = tags;
            }
        }
    }

    fn clear_current_task(&self) {
        *self
            .current_task
            .lock()
            .expect("session task lock poisoned") = None;
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
        self.set_current_task(task.clone(), Some(description.to_string()), _tags.to_vec());
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
        *self.limits.lock().expect("session limits lock poisoned")
    }

    fn set_limits(&self, limits: QueryLimits) {
        *self.limits.lock().expect("session limits lock poisoned") = limits;
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
#[serde(rename_all = "camelCase")]
struct PrismStartTaskResult {
    task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EventMutationResult {
    event_id: String,
    task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct MemoryMutationResult {
    memory_id: String,
    task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EdgeMutationResult {
    edge_id: String,
    task_id: String,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CuratorProposalDecisionResult {
    job_id: String,
    proposal: Value,
    edge_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
struct QueryDiagnosticSchema {
    code: String,
    message: String,
    data: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
struct QueryEnvelopeSchema {
    result: Value,
    diagnostics: Vec<QueryDiagnosticSchema>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct QueryLimitsInput {
    max_result_nodes: Option<usize>,
    max_call_graph_depth: Option<usize>,
    max_output_json_bytes: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
struct PrismGetSessionArgs {}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismConfigureSessionArgs {
    limits: Option<QueryLimitsInput>,
    #[serde(alias = "current_task_id")]
    current_task_id: Option<String>,
    #[serde(alias = "current_task_description")]
    current_task_description: Option<String>,
    #[serde(alias = "current_task_tags")]
    current_task_tags: Option<Vec<String>>,
    clear_current_task: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SessionLimitsView {
    max_result_nodes: usize,
    max_call_graph_depth: usize,
    max_output_json_bytes: usize,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SessionTaskView {
    task_id: String,
    description: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SessionView {
    workspace_root: Option<String>,
    current_task: Option<SessionTaskView>,
    limits: SessionLimitsView,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourcePageView {
    cursor: Option<String>,
    next_cursor: Option<String>,
    limit: usize,
    returned: usize,
    total: usize,
    has_more: bool,
    limit_capped: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InferredEdgeRecordView {
    id: String,
    edge: EdgeView,
    scope: InferredEdgeScope,
    task_id: Option<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceLinkView {
    uri: String,
    name: String,
    description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResourcePayload {
    uri: String,
    workspace_root: Option<String>,
    current_task: Option<SessionTaskView>,
    limits: SessionLimitsView,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EntrypointsResourcePayload {
    uri: String,
    entrypoints: Vec<SymbolView>,
    page: ResourcePageView,
    truncated: bool,
    diagnostics: Vec<QueryDiagnostic>,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResourcePayload {
    uri: String,
    query: String,
    results: Vec<SymbolView>,
    page: ResourcePageView,
    truncated: bool,
    diagnostics: Vec<QueryDiagnostic>,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SymbolResourcePayload {
    uri: String,
    symbol: SymbolView,
    relations: RelationsView,
    lineage: Option<LineageView>,
    co_change_neighbors: Vec<CoChangeView>,
    related_failures: Vec<OutcomeEvent>,
    blast_radius: ChangeImpactView,
    validation_recipe: ValidationRecipeView,
    diagnostics: Vec<QueryDiagnostic>,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LineageResourcePayload {
    uri: String,
    lineage_id: String,
    status: LineageStatus,
    current_nodes: Vec<SymbolView>,
    current_nodes_truncated: bool,
    history: Vec<LineageEventView>,
    history_page: ResourcePageView,
    truncated: bool,
    co_change_neighbors: Vec<CoChangeView>,
    diagnostics: Vec<QueryDiagnostic>,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskResourcePayload {
    uri: String,
    task_id: String,
    events: Vec<OutcomeEvent>,
    page: ResourcePageView,
    truncated: bool,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EventResourcePayload {
    uri: String,
    event: OutcomeEvent,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryResourcePayload {
    uri: String,
    memory: MemoryEntryView,
    task_id: Option<String>,
    related_resources: Vec<ResourceLinkView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EdgeResourcePayload {
    uri: String,
    edge: InferredEdgeRecordView,
    related_resources: Vec<ResourceLinkView>,
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum CoordinationMutationKindInput {
    PlanCreate,
    TaskCreate,
    TaskUpdate,
    Handoff,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ClaimActionInput {
    Acquire,
    Renew,
    Release,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactActionInput {
    Propose,
    Supersede,
    Review,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismCoordinationArgs {
    kind: CoordinationMutationKindInput,
    payload: Value,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismClaimArgs {
    action: ClaimActionInput,
    payload: Value,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismArtifactArgs {
    action: ArtifactActionInput,
    payload: Value,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanTargetArgs {
    #[serde(alias = "plan_id")]
    plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoordinationTaskTargetArgs {
    #[serde(alias = "task_id")]
    task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnchorListArgs {
    anchors: Vec<AnchorRefInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingReviewsArgs {
    #[serde(alias = "plan_id")]
    plan_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimulateClaimArgs {
    anchors: Vec<AnchorRefInput>,
    capability: String,
    mode: Option<String>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanCreatePayload {
    goal: String,
    policy: Option<CoordinationPolicyPayload>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoordinationPolicyPayload {
    default_claim_mode: Option<String>,
    max_parallel_editors_per_anchor: Option<u16>,
    require_review_for_completion: Option<bool>,
    stale_after_graph_change: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptanceCriterionPayload {
    label: String,
    anchors: Option<Vec<AnchorRefInput>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskCreatePayload {
    plan_id: String,
    title: String,
    status: Option<String>,
    assignee: Option<String>,
    anchors: Option<Vec<AnchorRefInput>>,
    depends_on: Option<Vec<String>>,
    acceptance: Option<Vec<AcceptanceCriterionPayload>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskUpdatePayload {
    task_id: String,
    status: Option<String>,
    assignee: Option<String>,
    title: Option<String>,
    anchors: Option<Vec<AnchorRefInput>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HandoffPayload {
    task_id: String,
    to_agent: Option<String>,
    summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimAcquirePayload {
    anchors: Vec<AnchorRefInput>,
    capability: String,
    mode: Option<String>,
    ttl_seconds: Option<u64>,
    agent: Option<String>,
    coordination_task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRenewPayload {
    claim_id: String,
    ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimReleasePayload {
    claim_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactProposePayload {
    task_id: String,
    anchors: Option<Vec<AnchorRefInput>>,
    diff_ref: Option<String>,
    evidence: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactSupersedePayload {
    artifact_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactReviewPayload {
    artifact_id: String,
    verdict: String,
    summary: String,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CoordinationMutationResult {
    event_id: String,
    state: Value,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ClaimMutationResult {
    claim_id: Option<String>,
    conflicts: Vec<Value>,
    state: Value,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ArtifactMutationResult {
    artifact_id: Option<String>,
    review_id: Option<String>,
    state: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismCuratorPromoteEdgeArgs {
    #[serde(alias = "job_id")]
    job_id: String,
    #[serde(alias = "proposal_index")]
    proposal_index: usize,
    scope: Option<InferredEdgeScopeInput>,
    note: Option<String>,
    #[serde(alias = "task_id")]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct PrismCuratorRejectProposalArgs {
    #[serde(alias = "job_id")]
    job_id: String,
    #[serde(alias = "proposal_index")]
    proposal_index: usize,
    reason: Option<String>,
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
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<PrismStartTaskResult>()
            .unwrap()
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
        let task_id = task.0.to_string();
        structured_tool_result_with_links(
            PrismStartTaskResult {
                task_id: task_id.clone(),
            },
            vec![session_resource_link(), task_resource_link(&task_id)],
        )
    }

    #[tool(
        description = "Inspect the current MCP session state, including workspace root, active task, and runtime limits.",
        annotations(title = "Get PRISM Session", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<SessionView>().unwrap()
    )]
    fn prism_get_session(
        &self,
        Parameters(_args): Parameters<PrismGetSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.host.session_view().map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(task) = &session.current_task {
            links.push(task_resource_link(&task.task_id));
        }
        structured_tool_result_with_links(session, links)
    }

    #[tool(
        description = "Configure session-scoped limits and the active task context for subsequent mutations.",
        annotations(
            title = "Configure PRISM Session",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<SessionView>().unwrap()
    )]
    fn prism_configure_session(
        &self,
        Parameters(args): Parameters<PrismConfigureSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.host.configure_session(args).map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(task) = &session.current_task {
            links.push(task_resource_link(&task.task_id));
        }
        structured_tool_result_with_links(session, links)
    }

    #[tool(
        name = "prism_query",
        description = "Execute a read-only TypeScript query against the live PRISM graph. Read prism://api-reference for the available prism API.",
        annotations(title = "Programmable PRISM Query", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
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
        annotations(title = "Lookup PRISM Symbol", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
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
        let links = serde_json::from_value::<Option<SymbolView>>(envelope.result.clone())
            .ok()
            .flatten()
            .map(|symbol| symbol_links(&symbol))
            .unwrap_or_default();
        structured_tool_result_with_links(envelope, links)
    }

    #[tool(
        description = "Convenience search lookup. Returns the same structured query envelope as prism_query.",
        annotations(title = "Search PRISM Graph", read_only_hint = true),
        output_schema = rmcp::handler::server::tool::schema_for_output::<QueryEnvelopeSchema>()
            .unwrap()
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

        let query = args.query.clone();
        let envelope = self
            .host
            .search_query(SearchArgs {
                query: query.clone(),
                limit: args.limit,
                kind: args.kind,
                path: args.path,
                include_inferred: None,
            })
            .map_err(map_query_error)?;
        let mut links = vec![search_resource_link(&query)];
        if let Ok(symbols) = serde_json::from_value::<Vec<SymbolView>>(envelope.result.clone()) {
            for symbol in symbols.iter().take(8) {
                links.push(symbol_resource_link(symbol));
            }
        }
        structured_tool_result_with_links(envelope, links)
    }

    #[tool(
        description = "Write a structured outcome event for the current task or symbol anchors.",
        annotations(
            title = "Record Outcome Event",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_outcome(
        &self,
        Parameters(args): Parameters<PrismOutcomeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_outcome(args).map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Store an agent note anchored to nodes or lineages.",
        annotations(
            title = "Store Agent Note",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<MemoryMutationResult>()
            .unwrap()
    )]
    fn prism_note(
        &self,
        Parameters(args): Parameters<PrismNoteArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_note(args).map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                memory_resource_link(&result.memory_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Persist an inferred edge into the session overlay or a promoted scope.",
        annotations(
            title = "Store Inferred Edge",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EdgeMutationResult>()
            .unwrap()
    )]
    fn prism_infer_edge(
        &self,
        Parameters(args): Parameters<PrismInferEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .store_inferred_edge(args)
            .map_err(map_query_error)?;
        structured_tool_result_with_links(
            result.clone(),
            vec![
                edge_resource_link(&result.edge_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Mutate shared coordination state for plans, tasks, and handoffs.",
        annotations(
            title = "Mutate Coordination State",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CoordinationMutationResult>()
            .unwrap()
    )]
    fn prism_coordination(
        &self,
        Parameters(args): Parameters<PrismCoordinationArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .store_coordination(args)
            .map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Acquire, renew, or release shared work claims.",
        annotations(
            title = "Mutate Claims",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<ClaimMutationResult>()
            .unwrap()
    )]
    fn prism_claim(
        &self,
        Parameters(args): Parameters<PrismClaimArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_claim(args).map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Propose, supersede, or review shared artifacts.",
        annotations(
            title = "Mutate Artifacts",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<ArtifactMutationResult>()
            .unwrap()
    )]
    fn prism_artifact(
        &self,
        Parameters(args): Parameters<PrismArtifactArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self.host.store_artifact(args).map_err(map_query_error)?;
        structured_tool_result(result)
    }

    #[tool(
        description = "Convenience outcome for a test run.",
        annotations(
            title = "Record Test Run",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
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
        let result = self
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
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Convenience outcome for an observed failure.",
        annotations(
            title = "Record Observed Failure",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_failure_observed(
        &self,
        Parameters(args): Parameters<PrismFailureObservedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let evidence = args
            .trace
            .map(|trace| vec![OutcomeEvidenceInput::StackTrace { hash: trace }]);
        let result = self
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
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Convenience outcome for a validated fix.",
        annotations(
            title = "Record Validated Fix",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<EventMutationResult>()
            .unwrap()
    )]
    fn prism_fix_validated(
        &self,
        Parameters(args): Parameters<PrismFixValidatedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
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
        structured_tool_result_with_links(
            result.clone(),
            vec![
                event_resource_link(&result.event_id),
                task_resource_link(&result.task_id),
            ],
        )
    }

    #[tool(
        description = "Promote a completed curator inferred-edge proposal into the session overlay or persisted inference store.",
        annotations(
            title = "Promote Curator Edge",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CuratorProposalDecisionResult>()
            .unwrap()
    )]
    fn prism_curator_promote_edge(
        &self,
        Parameters(args): Parameters<PrismCuratorPromoteEdgeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .promote_curator_edge(args)
            .map_err(map_query_error)?;
        let mut links = vec![session_resource_link()];
        if let Some(edge_id) = &result.edge_id {
            links.push(edge_resource_link(edge_id));
        }
        structured_tool_result_with_links(result, links)
    }

    #[tool(
        description = "Reject a curator proposal without mutating the graph. Use this for risk summaries, validation recipes, or inferred edges you do not want to apply.",
        annotations(
            title = "Reject Curator Proposal",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        ),
        output_schema = rmcp::handler::server::tool::schema_for_output::<CuratorProposalDecisionResult>()
            .unwrap()
    )]
    fn prism_curator_reject_proposal(
        &self,
        Parameters(args): Parameters<PrismCuratorRejectProposalArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .host
            .reject_curator_proposal(args)
            .map_err(map_query_error)?;
        structured_tool_result_with_links(result, vec![session_resource_link()])
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
            "Start with prism://api-reference for the typed query contract. Use prism_get_session or prism://session to inspect the active workspace, task, and runtime limits, prism_configure_session to change them, prism_query for programmable read-only graph queries, prism_symbol or prism_search for direct lookups, prism://entrypoints for a quick workspace overview, prism://search/{query} for browseable search results, prism://symbol/{crateName}/{kind}/{path} for exact symbol snapshots, prism://lineage/{lineageId} for symbol history, prism://task/{taskId} for recorded task outcomes, prism://event/{eventId}, prism://memory/{memoryId}, and prism://edge/{edgeId} for mutation outputs, and the prism_* mutation tools to record outcomes, notes, inferred edges, task context, and curator proposal decisions.",
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
                RawResource::new(SESSION_URI, "PRISM Session")
                    .with_description(
                        "Active workspace root, current task context, and runtime query limits",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResource::new(ENTRYPOINTS_URI, "PRISM Entrypoints")
                    .with_description(
                        "Workspace entrypoints and top-level starting symbols in structured JSON, with optional cursor-based pagination",
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
        let (base_uri, _) = split_resource_uri(uri);
        let contents = if base_uri == API_REFERENCE_URI {
            ResourceContents::text(api_reference_markdown(), request.uri.clone())
                .with_mime_type("text/markdown")
        } else if base_uri == SESSION_URI {
            json_resource_contents(
                self.host
                    .session_resource_value()
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if base_uri == ENTRYPOINTS_URI {
            json_resource_contents(
                self.host
                    .entrypoints_resource_value(uri)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(query) = parse_search_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .search_resource_value(uri, &query)
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
        } else if let Some(lineage) = parse_lineage_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .lineage_resource_value(uri, &lineage)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(task_id) = parse_task_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .task_resource_value(uri, &task_id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(event_id) = parse_event_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .event_resource_value(&event_id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(memory_id) = parse_memory_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .memory_resource_value(&memory_id)
                    .map_err(map_query_error)?,
                request.uri.clone(),
            )?
        } else if let Some(edge_id) = parse_edge_resource_uri(uri) {
            json_resource_contents(
                self.host
                    .edge_resource_value(&edge_id)
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
                    ENTRYPOINTS_RESOURCE_TEMPLATE_URI,
                    "PRISM Entrypoints Page",
                )
                .with_description(
                    "Read workspace entrypoints with optional `limit` and opaque `cursor` pagination",
                )
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(SEARCH_RESOURCE_TEMPLATE_URI, "PRISM Search")
                    .with_description(
                        "Read structured PRISM search results and diagnostics for a query string with optional `limit` and opaque `cursor` pagination",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(
                    SYMBOL_RESOURCE_TEMPLATE_URI,
                    "PRISM Symbol Snapshot",
                )
                .with_description(
                    "Read a structured snapshot for an exact symbol, including relations, lineage, validation recipe, blast radius, and related failures",
                )
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(LINEAGE_RESOURCE_TEMPLATE_URI, "PRISM Lineage")
                    .with_description(
                        "Read structured lineage history, current nodes, and status for a lineage id with paged history",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(TASK_RESOURCE_TEMPLATE_URI, "PRISM Task Replay")
                    .with_description(
                        "Read the outcome-event timeline recorded for a specific task context with optional `limit` and opaque `cursor` pagination",
                    )
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(EVENT_RESOURCE_TEMPLATE_URI, "PRISM Event")
                    .with_description("Read a single recorded outcome event by id")
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(MEMORY_RESOURCE_TEMPLATE_URI, "PRISM Memory")
                    .with_description("Read a single episodic memory entry by id")
                    .with_mime_type("application/json")
                    .no_annotation(),
                RawResourceTemplate::new(EDGE_RESOURCE_TEMPLATE_URI, "PRISM Inferred Edge")
                    .with_description(
                        "Read a single inferred-edge record, including scope, task, and evidence",
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
    structured_tool_result_with_links(value, Vec::new())
}

fn structured_tool_result_with_links<T: serde::Serialize>(
    value: T,
    links: Vec<RawResource>,
) -> Result<CallToolResult, McpError> {
    let value = serde_json::to_value(value).map_err(|err| {
        McpError::internal_error(
            "failed to serialize structured tool result",
            Some(json!({ "error": err.to_string() })),
        )
    })?;
    let mut result = CallToolResult::structured(value);
    result
        .content
        .extend(links.into_iter().map(Content::resource_link));
    Ok(result)
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

#[derive(Debug, Clone, Copy)]
struct ResourcePageRequest {
    offset: usize,
    limit: usize,
    limit_capped: bool,
}

#[derive(Debug, Clone)]
struct PageSlice<T> {
    items: Vec<T>,
    page: ResourcePageView,
    truncated: bool,
}

fn split_resource_uri(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (uri, None),
    }
}

fn parse_resource_page(
    uri: &str,
    default_limit: usize,
    max_limit: usize,
) -> Result<ResourcePageRequest, McpError> {
    let (_, query) = split_resource_uri(uri);
    let mut requested_limit = None;
    let mut offset = None;

    if let Some(query) = query {
        for part in query.split('&').filter(|part| !part.is_empty()) {
            let (raw_key, raw_value) = part.split_once('=').unwrap_or((part, ""));
            let key = percent_decode_lossy(raw_key);
            let value = percent_decode_lossy(raw_value);
            match key.as_str() {
                "limit" => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid pagination limit",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?;
                    requested_limit = Some(parsed);
                }
                "cursor" => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid pagination cursor",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?;
                    offset = Some(parsed);
                }
                "offset" => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        McpError::invalid_params(
                            "invalid pagination offset",
                            Some(json!({ "uri": uri, "value": value })),
                        )
                    })?;
                    offset = Some(parsed);
                }
                _ => {}
            }
        }
    }

    let requested = requested_limit.unwrap_or(default_limit);
    let limit = requested.min(max_limit).max(1);
    Ok(ResourcePageRequest {
        offset: offset.unwrap_or(0),
        limit,
        limit_capped: requested > max_limit,
    })
}

fn paginate_items<T>(items: Vec<T>, request: ResourcePageRequest) -> PageSlice<T> {
    let total = items.len();
    let start = request.offset.min(total);
    let end = start.saturating_add(request.limit).min(total);
    let has_more = end < total;
    let next_cursor = has_more.then(|| end.to_string());
    let items = items.into_iter().skip(start).take(request.limit).collect();
    let page = ResourcePageView {
        cursor: (request.offset > 0).then(|| request.offset.to_string()),
        next_cursor,
        limit: request.limit,
        returned: end.saturating_sub(start),
        total,
        has_more,
        limit_capped: request.limit_capped,
    };
    PageSlice {
        truncated: page.has_more || page.limit_capped,
        items,
        page,
    }
}

fn parse_symbol_resource_uri(uri: &str) -> Result<Option<NodeId>, McpError> {
    let (base, _) = split_resource_uri(uri);
    let Some(rest) = base.strip_prefix("prism://symbol/") else {
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
    let crate_name = percent_decode_lossy(crate_name);
    let kind = percent_decode_lossy(kind);
    let path = percent_decode_lossy(path);
    let kind = parse_node_kind(&kind).map_err(|err| {
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

fn parse_search_resource_uri(uri: &str) -> Option<String> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://search/")
        .map(percent_decode_lossy)
        .filter(|query| !query.trim().is_empty())
}

fn parse_lineage_resource_uri(uri: &str) -> Option<LineageId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://lineage/")
        .map(percent_decode_lossy)
        .map(LineageId::new)
}

fn parse_task_resource_uri(uri: &str) -> Option<TaskId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://task/")
        .map(percent_decode_lossy)
        .map(TaskId::new)
}

fn parse_event_resource_uri(uri: &str) -> Option<EventId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://event/")
        .map(percent_decode_lossy)
        .map(EventId::new)
}

fn parse_memory_resource_uri(uri: &str) -> Option<MemoryId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://memory/")
        .map(percent_decode_lossy)
        .map(MemoryId)
}

fn parse_edge_resource_uri(uri: &str) -> Option<EdgeId> {
    let (base, _) = split_resource_uri(uri);
    base.strip_prefix("prism://edge/")
        .map(percent_decode_lossy)
        .map(EdgeId)
}

fn resource_link_view(
    uri: String,
    name: impl Into<String>,
    description: impl Into<String>,
) -> ResourceLinkView {
    ResourceLinkView {
        uri,
        name: name.into(),
        description: Some(description.into()),
    }
}

fn dedupe_resource_link_views(mut links: Vec<ResourceLinkView>) -> Vec<ResourceLinkView> {
    links.sort_by(|left, right| left.uri.cmp(&right.uri));
    links.dedup_by(|left, right| left.uri == right.uri);
    links
}

fn session_resource_uri() -> String {
    SESSION_URI.to_string()
}

fn task_resource_uri(task_id: &str) -> String {
    format!("prism://task/{}", percent_encode_component(task_id))
}

fn search_resource_uri(query: &str) -> String {
    format!("prism://search/{}", percent_encode_component(query))
}

fn lineage_resource_uri(lineage_id: &str) -> String {
    format!("prism://lineage/{}", percent_encode_component(lineage_id))
}

fn event_resource_uri(event_id: &str) -> String {
    format!("prism://event/{}", percent_encode_component(event_id))
}

fn memory_resource_uri(memory_id: &str) -> String {
    format!("prism://memory/{}", percent_encode_component(memory_id))
}

fn edge_resource_uri(edge_id: &str) -> String {
    format!("prism://edge/{}", percent_encode_component(edge_id))
}

fn session_resource_link() -> RawResource {
    RawResource::new(session_resource_uri(), "PRISM Session")
        .with_description("Active workspace root, current task context, and runtime query limits")
        .with_mime_type("application/json")
}

fn task_resource_link(task_id: &str) -> RawResource {
    RawResource::new(task_resource_uri(task_id), "PRISM Task Replay")
        .with_description("Task-scoped outcome timeline and correlated events")
        .with_mime_type("application/json")
}

fn search_resource_link(query: &str) -> RawResource {
    RawResource::new(search_resource_uri(query), format!("PRISM Search: {query}"))
        .with_description("Structured search results and diagnostics for this query")
        .with_mime_type("application/json")
}

fn symbol_resource_link(symbol: &SymbolView) -> RawResource {
    RawResource::new(
        symbol_resource_uri(&symbol.id),
        format!("PRISM Symbol: {}", symbol.id.path),
    )
    .with_description("Exact symbol snapshot with relations, lineage, and risk context")
    .with_mime_type("application/json")
}

fn lineage_resource_link(lineage_id: &str) -> RawResource {
    RawResource::new(
        lineage_resource_uri(lineage_id),
        format!("PRISM Lineage: {lineage_id}"),
    )
    .with_description("Structured lineage history and current nodes")
    .with_mime_type("application/json")
}

fn event_resource_link(event_id: &str) -> RawResource {
    RawResource::new(
        event_resource_uri(event_id),
        format!("PRISM Event: {event_id}"),
    )
    .with_description("Recorded outcome event and associated task metadata")
    .with_mime_type("application/json")
}

fn memory_resource_link(memory_id: &str) -> RawResource {
    RawResource::new(
        memory_resource_uri(memory_id),
        format!("PRISM Memory: {memory_id}"),
    )
    .with_description("Stored episodic memory entry and associated task metadata")
    .with_mime_type("application/json")
}

fn edge_resource_link(edge_id: &str) -> RawResource {
    RawResource::new(
        edge_resource_uri(edge_id),
        format!("PRISM Inferred Edge: {edge_id}"),
    )
    .with_description("Inferred-edge record with scope, evidence, and task metadata")
    .with_mime_type("application/json")
}

fn session_resource_view_link() -> ResourceLinkView {
    resource_link_view(
        session_resource_uri(),
        "PRISM Session",
        "Active workspace root, current task context, and runtime query limits",
    )
}

fn task_resource_view_link(task_id: &str) -> ResourceLinkView {
    resource_link_view(
        task_resource_uri(task_id),
        "PRISM Task Replay",
        "Task-scoped outcome timeline and correlated events",
    )
}

fn search_resource_view_link(query: &str) -> ResourceLinkView {
    resource_link_view(
        search_resource_uri(query),
        format!("PRISM Search: {query}"),
        "Structured search results and diagnostics for this query",
    )
}

fn symbol_resource_view_link(symbol: &SymbolView) -> ResourceLinkView {
    resource_link_view(
        symbol_resource_uri(&symbol.id),
        format!("PRISM Symbol: {}", symbol.id.path),
        "Exact symbol snapshot with relations, lineage, and risk context",
    )
}

fn symbol_resource_view_link_for_id(id: &NodeId) -> ResourceLinkView {
    resource_link_view(
        symbol_resource_uri_from_node_id(id),
        format!("PRISM Symbol: {}", id.path),
        "Exact symbol snapshot with relations, lineage, and risk context",
    )
}

fn lineage_resource_view_link(lineage_id: &str) -> ResourceLinkView {
    resource_link_view(
        lineage_resource_uri(lineage_id),
        format!("PRISM Lineage: {lineage_id}"),
        "Structured lineage history and current nodes",
    )
}

fn event_resource_view_link(event_id: &str) -> ResourceLinkView {
    resource_link_view(
        event_resource_uri(event_id),
        format!("PRISM Event: {event_id}"),
        "Recorded outcome event and associated task metadata",
    )
}

fn memory_resource_view_link(memory_id: &str) -> ResourceLinkView {
    resource_link_view(
        memory_resource_uri(memory_id),
        format!("PRISM Memory: {memory_id}"),
        "Stored episodic memory entry and associated task metadata",
    )
}

fn edge_resource_view_link(edge_id: &str) -> ResourceLinkView {
    resource_link_view(
        edge_resource_uri(edge_id),
        format!("PRISM Inferred Edge: {edge_id}"),
        "Inferred-edge record with scope, evidence, and task metadata",
    )
}

fn symbol_resource_uri(id: &NodeIdView) -> String {
    format!(
        "prism://symbol/{}/{}/{}",
        percent_encode_component(&id.crate_name),
        percent_encode_component(&id.kind.to_string()),
        percent_encode_component(&id.path),
    )
}

fn symbol_resource_uri_from_node_id(id: &NodeId) -> String {
    format!(
        "prism://symbol/{}/{}/{}",
        percent_encode_component(id.crate_name.as_str()),
        percent_encode_component(&id.kind.to_string()),
        percent_encode_component(id.path.as_str()),
    )
}

fn symbol_links(symbol: &SymbolView) -> Vec<RawResource> {
    let mut links = vec![symbol_resource_link(symbol)];
    if let Some(lineage_id) = &symbol.lineage_id {
        links.push(lineage_resource_link(lineage_id));
    }
    links
}

fn anchor_resource_view_links(anchors: &[AnchorRef]) -> Vec<ResourceLinkView> {
    let mut links = Vec::new();
    for anchor in anchors {
        match anchor {
            AnchorRef::Node(id) => links.push(symbol_resource_view_link_for_id(id)),
            AnchorRef::Lineage(lineage_id) => {
                links.push(lineage_resource_view_link(lineage_id.0.as_str()))
            }
            AnchorRef::File(_) | AnchorRef::Kind(_) => {}
        }
    }
    dedupe_resource_link_views(links)
}

fn task_resource_view_links_from_events(events: &[OutcomeEvent]) -> Vec<ResourceLinkView> {
    dedupe_resource_link_views(
        events
            .iter()
            .filter_map(|event| event.meta.correlation.as_ref())
            .map(|task_id| task_resource_view_link(task_id.0.as_str()))
            .collect(),
    )
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            decoded.push(b' ');
        } else {
            decoded.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
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

    fn session_view(&self) -> Result<SessionView> {
        self.refresh_workspace()?;
        let limits = self.session.limits();
        Ok(SessionView {
            workspace_root: self
                .workspace
                .as_ref()
                .map(|workspace| workspace.root().display().to_string()),
            current_task: self
                .session
                .current_task_state()
                .map(|task| SessionTaskView {
                    task_id: task.id.0.to_string(),
                    description: task.description,
                    tags: task.tags,
                }),
            limits: SessionLimitsView {
                max_result_nodes: limits.max_result_nodes,
                max_call_graph_depth: limits.max_call_graph_depth,
                max_output_json_bytes: limits.max_output_json_bytes,
            },
        })
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

        self.session_view()
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

    fn session_resource_value(&self) -> Result<SessionResourcePayload> {
        let session = self.session_view()?;
        let mut related_resources = vec![
            session_resource_view_link(),
            resource_link_view(
                ENTRYPOINTS_URI.to_string(),
                "PRISM Entrypoints",
                "Workspace entrypoints and top-level starting symbols",
            ),
        ];
        if let Some(task) = &session.current_task {
            related_resources.push(task_resource_view_link(&task.task_id));
        }
        Ok(SessionResourcePayload {
            uri: session_resource_uri(),
            workspace_root: session.workspace_root,
            current_task: session.current_task,
            limits: session.limits,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn task_metadata(&self, task_id: &TaskId) -> (Option<String>, Vec<String>) {
        if let Some(task) = self.session.current_task_state() {
            if task.id == *task_id {
                return (task.description, task.tags);
            }
        }

        let replay = self.current_prism().resume_task(task_id);
        let description = replay
            .events
            .iter()
            .find(|event| event.kind == OutcomeKind::PlanCreated)
            .map(|event| event.summary.clone());
        let tags = replay
            .events
            .iter()
            .find(|event| event.kind == OutcomeKind::PlanCreated)
            .and_then(|event| event.metadata.get("tags"))
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        (description, tags)
    }

    fn entrypoints_resource_value(&self, uri: &str) -> Result<EntrypointsResourcePayload> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism);
        let paged = paginate_items(
            execution.entrypoints()?,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
            )?,
        );
        let mut related_resources = vec![
            session_resource_view_link(),
            resource_link_view(
                uri.to_string(),
                "PRISM Entrypoints",
                "Workspace entrypoints and top-level starting symbols",
            ),
        ];
        related_resources.extend(paged.items.iter().take(8).map(symbol_resource_view_link));
        Ok(EntrypointsResourcePayload {
            uri: uri.to_string(),
            entrypoints: paged.items,
            page: paged.page,
            truncated: paged.truncated,
            diagnostics: execution.diagnostics(),
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn symbol_resource_value(&self, id: &NodeId) -> Result<SymbolResourcePayload> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let execution = QueryExecution::new(self.clone(), prism.clone());
        let symbol = symbol_for(prism.as_ref(), id)?;
        let symbol = symbol_view(prism.as_ref(), &symbol)?;
        let relations = relations_view(prism.as_ref(), self.session.as_ref(), id)?;
        let lineage = lineage_view(prism.as_ref(), id)?;
        let co_change_neighbors = prism
            .co_change_neighbors(id, 8)
            .into_iter()
            .map(co_change_view)
            .collect::<Vec<_>>();
        let related_failures = prism.related_failures(id);
        let blast_radius = blast_radius_view(prism.as_ref(), self.session.as_ref(), id);
        let validation_recipe =
            validation_recipe_view_with(prism.as_ref(), self.session.as_ref(), id);
        let mut related_resources = vec![
            session_resource_view_link(),
            symbol_resource_view_link(&symbol),
        ];
        if let Some(lineage) = &lineage {
            related_resources.push(lineage_resource_view_link(&lineage.lineage_id));
        }
        related_resources.extend(task_resource_view_links_from_events(
            &prism
                .outcome_memory()
                .outcomes_for(&[AnchorRef::Node(id.clone())], 16),
        ));
        related_resources.extend(
            related_failures
                .iter()
                .map(|event| event_resource_view_link(event.meta.id.0.as_str())),
        );
        Ok(SymbolResourcePayload {
            uri: symbol_resource_uri(&symbol.id),
            symbol,
            relations,
            lineage,
            co_change_neighbors,
            related_failures,
            blast_radius,
            validation_recipe,
            diagnostics: execution.diagnostics(),
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn search_resource_value(&self, uri: &str, query: &str) -> Result<SearchResourcePayload> {
        self.refresh_workspace()?;
        let execution = QueryExecution::new(self.clone(), self.current_prism());
        let paged = paginate_items(
            execution.search(SearchArgs {
                query: query.to_string(),
                limit: Some(self.session.limits().max_result_nodes),
                kind: None,
                path: None,
                include_inferred: None,
            })?,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
            )?,
        );
        let mut related_resources = vec![
            session_resource_view_link(),
            search_resource_view_link(query),
        ];
        related_resources.extend(paged.items.iter().take(8).map(symbol_resource_view_link));
        Ok(SearchResourcePayload {
            uri: uri.to_string(),
            query: query.to_string(),
            results: paged.items,
            page: paged.page,
            truncated: paged.truncated,
            diagnostics: execution.diagnostics(),
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn lineage_resource_value(
        &self,
        uri: &str,
        lineage: &LineageId,
    ) -> Result<LineageResourcePayload> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let history = prism.history_snapshot();
        let events = prism.lineage_history(lineage);
        let mut current_node_ids = history
            .node_to_lineage
            .into_iter()
            .filter_map(|(node, current)| (current == *lineage).then_some(node))
            .collect::<Vec<_>>();
        current_node_ids.sort_by(|left, right| {
            left.crate_name
                .cmp(&right.crate_name)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
        });
        let current_nodes_truncated =
            current_node_ids.len() > self.session.limits().max_result_nodes;
        current_node_ids.truncate(self.session.limits().max_result_nodes);
        let current_nodes = symbol_views_for_ids(prism.as_ref(), current_node_ids.clone())?;
        let co_change_neighbors = current_node_ids
            .first()
            .map(|node| {
                prism
                    .co_change_neighbors(node, 8)
                    .into_iter()
                    .map(co_change_view)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let paged_history = paginate_items(
            events
                .iter()
                .map(|event| LineageEventView {
                    event_id: event.meta.id.0.to_string(),
                    ts: event.meta.ts,
                    kind: format!("{:?}", event.kind),
                    confidence: event.confidence,
                })
                .collect::<Vec<_>>(),
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
            )?,
        );
        let mut related_resources = vec![
            session_resource_view_link(),
            lineage_resource_view_link(lineage.0.as_str()),
        ];
        related_resources.extend(current_nodes.iter().map(symbol_resource_view_link));
        Ok(LineageResourcePayload {
            uri: uri.to_string(),
            lineage_id: lineage.0.to_string(),
            status: lineage_status(&events),
            current_nodes,
            current_nodes_truncated,
            history: paged_history.items,
            history_page: paged_history.page,
            truncated: paged_history.truncated || current_nodes_truncated,
            co_change_neighbors,
            diagnostics: Vec::new(),
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn task_resource_value(&self, uri: &str, task_id: &TaskId) -> Result<TaskResourcePayload> {
        self.refresh_workspace()?;
        let prism = self.current_prism();
        let replay = prism.resume_task(task_id);
        let paged = paginate_items(
            replay.events,
            parse_resource_page(
                uri,
                DEFAULT_RESOURCE_PAGE_LIMIT,
                self.session.limits().max_result_nodes,
            )?,
        );
        let mut related_resources = vec![
            session_resource_view_link(),
            task_resource_view_link(replay.task.0.as_str()),
        ];
        related_resources.extend(
            paged
                .items
                .iter()
                .map(|event| event_resource_view_link(event.meta.id.0.as_str())),
        );
        related_resources.extend(
            paged
                .items
                .iter()
                .flat_map(|event| anchor_resource_view_links(&event.anchors)),
        );
        Ok(TaskResourcePayload {
            uri: uri.to_string(),
            task_id: replay.task.0.to_string(),
            events: paged.items,
            page: paged.page,
            truncated: paged.truncated,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn event_resource_value(&self, event_id: &EventId) -> Result<EventResourcePayload> {
        self.refresh_workspace()?;
        let event = self
            .current_prism()
            .outcome_memory()
            .event(event_id)
            .ok_or_else(|| anyhow!("unknown event `{}`", event_id.0))?;
        let mut related_resources = vec![
            session_resource_view_link(),
            event_resource_view_link(event_id.0.as_str()),
        ];
        if let Some(task_id) = &event.meta.correlation {
            related_resources.push(task_resource_view_link(task_id.0.as_str()));
        }
        related_resources.extend(anchor_resource_view_links(&event.anchors));
        Ok(EventResourcePayload {
            uri: event_resource_uri(event_id.0.as_str()),
            event,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn memory_resource_value(&self, memory_id: &MemoryId) -> Result<MemoryResourcePayload> {
        self.refresh_workspace()?;
        let entry = self
            .session
            .notes
            .entry(memory_id)
            .ok_or_else(|| anyhow!("unknown memory `{}`", memory_id.0))?;
        let task_id = entry
            .metadata
            .get("task_id")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let mut related_resources = vec![
            session_resource_view_link(),
            memory_resource_view_link(&memory_id.0),
        ];
        if let Some(task_id) = &task_id {
            related_resources.push(task_resource_view_link(task_id));
        }
        related_resources.extend(anchor_resource_view_links(&entry.anchors));
        Ok(MemoryResourcePayload {
            uri: memory_resource_uri(&memory_id.0),
            memory: memory_entry_view(entry),
            task_id,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn edge_resource_value(&self, edge_id: &EdgeId) -> Result<EdgeResourcePayload> {
        self.refresh_workspace()?;
        let record = self
            .session
            .inferred_edges
            .record(edge_id)
            .ok_or_else(|| anyhow!("unknown inferred edge `{}`", edge_id.0))?;
        let edge = inferred_edge_record_view(record);
        let mut related_resources = vec![
            session_resource_view_link(),
            edge_resource_view_link(&edge.id),
        ];
        if let Some(task_id) = &edge.task_id {
            related_resources.push(task_resource_view_link(task_id));
        }
        related_resources.push(symbol_resource_view_link_for_id(&convert_node_id(
            NodeIdInput {
                crate_name: edge.edge.source.crate_name.clone(),
                path: edge.edge.source.path.clone(),
                kind: edge.edge.source.kind.to_string(),
            },
        )?));
        related_resources.push(symbol_resource_view_link_for_id(&convert_node_id(
            NodeIdInput {
                crate_name: edge.edge.target.crate_name.clone(),
                path: edge.edge.target.path.clone(),
                kind: edge.edge.target.kind.to_string(),
            },
        )?));
        Ok(EdgeResourcePayload {
            uri: edge_resource_uri(&edge.id),
            edge,
            related_resources: dedupe_resource_link_views(related_resources),
        })
    }

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        self.refresh_workspace()?;
        let source = format!(
            "(function() {{\n  try {{\n    const __prismUserQuery = () => {{\n{}\n    }};\n    const __prismResult = __prismUserQuery();\n    return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n  }} catch (error) {{\n    const __prismMessage = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : error && typeof error === \"object\" && \"message\" in error && error.message\n        ? String(error.message)\n        : String(error);\n    throw new Error(__prismMessage);\n  }}\n}})();\n",
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

    fn store_outcome(&self, args: PrismOutcomeArgs) -> Result<EventMutationResult> {
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
                correlation: Some(task_id.clone()),
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
        let event_id = if let Some(workspace) = &self.workspace {
            workspace.append_outcome(event)?
        } else {
            prism.apply_outcome_event_to_projections(&event);
            let id = prism.outcome_memory().store_event(event)?;
            self.persist_outcomes()?;
            id
        };
        Ok(EventMutationResult {
            event_id: event_id.0.to_string(),
            task_id: task_id.0.to_string(),
        })
    }

    fn store_note(&self, args: PrismNoteArgs) -> Result<MemoryMutationResult> {
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
                correlation: Some(task_id.clone()),
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
            let _ = workspace.append_outcome_with_auxiliary(
                note_event,
                Some(self.session.notes.snapshot()),
                None,
            )?;
        } else {
            prism.apply_outcome_event_to_projections(&note_event);
            let _ = prism.outcome_memory().store_event(note_event)?;
            self.persist_outcomes()?;
            self.persist_notes()?;
        }
        Ok(MemoryMutationResult {
            memory_id: memory_id.0,
            task_id: task_id.0.to_string(),
        })
    }

    fn store_inferred_edge(&self, args: PrismInferEdgeArgs) -> Result<EdgeMutationResult> {
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
            Some(task.clone()),
            args.evidence.unwrap_or_default(),
        );
        if scope != InferredEdgeScope::SessionOnly {
            self.persist_inferred_edges()?;
        }
        Ok(EdgeMutationResult {
            edge_id: id.0,
            task_id: task.0.to_string(),
        })
    }

    fn store_coordination(
        &self,
        args: PrismCoordinationArgs,
    ) -> Result<CoordinationMutationResult> {
        self.refresh_workspace()?;
        let task = self
            .session
            .task_for_mutation(args.task_id.clone().map(TaskId::new));
        let event_id = self.session.next_event_id("coordination");
        let meta = EventMeta {
            id: event_id.clone(),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        let state = if let Some(workspace) = &self.workspace {
            workspace
                .mutate_coordination(|prism| self.apply_coordination_mutation(prism, args, meta))?
        } else {
            let prism = self.current_prism();
            self.apply_coordination_mutation(prism.as_ref(), args, meta)?
        };
        Ok(CoordinationMutationResult {
            event_id: event_id.0.to_string(),
            state,
        })
    }

    fn store_claim(&self, args: PrismClaimArgs) -> Result<ClaimMutationResult> {
        self.refresh_workspace()?;
        let task = self
            .session
            .task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: self.session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        let result = if let Some(workspace) = &self.workspace {
            workspace.mutate_coordination(|prism| self.apply_claim_mutation(prism, args, meta))?
        } else {
            let prism = self.current_prism();
            self.apply_claim_mutation(prism.as_ref(), args, meta)?
        };
        Ok(result)
    }

    fn store_artifact(&self, args: PrismArtifactArgs) -> Result<ArtifactMutationResult> {
        self.refresh_workspace()?;
        let task = self
            .session
            .task_for_mutation(args.task_id.clone().map(TaskId::new));
        let meta = EventMeta {
            id: self.session.next_event_id("coordination"),
            ts: current_timestamp(),
            actor: EventActor::Agent,
            correlation: Some(task),
            causation: None,
        };
        let result = if let Some(workspace) = &self.workspace {
            workspace
                .mutate_coordination(|prism| self.apply_artifact_mutation(prism, args, meta))?
        } else {
            let prism = self.current_prism();
            self.apply_artifact_mutation(prism.as_ref(), args, meta)?
        };
        Ok(result)
    }

    fn apply_coordination_mutation(
        &self,
        prism: &Prism,
        args: PrismCoordinationArgs,
        meta: EventMeta,
    ) -> Result<Value> {
        match args.kind {
            CoordinationMutationKindInput::PlanCreate => {
                let payload: PlanCreatePayload = serde_json::from_value(args.payload)?;
                let (_, plan) = prism.coordination().create_plan(
                    meta,
                    PlanCreateInput {
                        goal: payload.goal,
                        policy: convert_policy(payload.policy)?,
                    },
                )?;
                Ok(serde_json::to_value(plan_view(plan))?)
            }
            CoordinationMutationKindInput::TaskCreate => {
                let payload: TaskCreatePayload = serde_json::from_value(args.payload)?;
                let (_, task) = prism.coordination().create_task(
                    meta,
                    TaskCreateInput {
                        plan_id: PlanId::new(payload.plan_id),
                        title: payload.title,
                        status: payload
                            .status
                            .as_deref()
                            .map(parse_coordination_task_status)
                            .transpose()?,
                        assignee: payload.assignee.map(AgentId::new),
                        session: Some(self.session.session_id()),
                        anchors: convert_anchors(payload.anchors.unwrap_or_default())?,
                        depends_on: payload
                            .depends_on
                            .unwrap_or_default()
                            .into_iter()
                            .map(CoordinationTaskId::new)
                            .collect(),
                        acceptance: convert_acceptance(payload.acceptance)?,
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::TaskUpdate => {
                let payload: TaskUpdatePayload = serde_json::from_value(args.payload)?;
                let task = prism.coordination().update_task(
                    meta,
                    TaskUpdateInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        status: payload
                            .status
                            .as_deref()
                            .map(parse_coordination_task_status)
                            .transpose()?,
                        assignee: payload.assignee.map(|value| Some(AgentId::new(value))),
                        session: None,
                        title: payload.title,
                        anchors: payload.anchors.map(convert_anchors).transpose()?,
                        base_revision: Some(prism.workspace_revision()),
                    },
                    prism.workspace_revision(),
                    current_timestamp(),
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
            CoordinationMutationKindInput::Handoff => {
                let payload: HandoffPayload = serde_json::from_value(args.payload)?;
                let task = prism.coordination().handoff(
                    meta,
                    HandoffInput {
                        task_id: CoordinationTaskId::new(payload.task_id),
                        to_agent: payload.to_agent.map(AgentId::new),
                        summary: payload.summary,
                    },
                )?;
                Ok(serde_json::to_value(coordination_task_view(task))?)
            }
        }
    }

    fn apply_claim_mutation(
        &self,
        prism: &Prism,
        args: PrismClaimArgs,
        meta: EventMeta,
    ) -> Result<ClaimMutationResult> {
        match args.action {
            ClaimActionInput::Acquire => {
                let payload: ClaimAcquirePayload = serde_json::from_value(args.payload)?;
                let (claim_id, conflicts, state) = prism.coordination().acquire_claim(
                    meta,
                    self.session.session_id(),
                    ClaimAcquireInput {
                        task_id: payload.coordination_task_id.map(CoordinationTaskId::new),
                        anchors: convert_anchors(payload.anchors)?,
                        capability: parse_capability(&payload.capability)?,
                        mode: payload.mode.as_deref().map(parse_claim_mode).transpose()?,
                        ttl_seconds: payload.ttl_seconds,
                        base_revision: prism.workspace_revision(),
                        agent: payload.agent.map(AgentId::new),
                    },
                )?;
                Ok(ClaimMutationResult {
                    claim_id: claim_id.map(|claim_id| claim_id.0.to_string()),
                    conflicts: conflicts
                        .into_iter()
                        .map(conflict_view)
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    state: state
                        .map(claim_view)
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(Value::Null),
                })
            }
            ClaimActionInput::Renew => {
                let payload: ClaimRenewPayload = serde_json::from_value(args.payload)?;
                let claim = prism.coordination().renew_claim(
                    meta,
                    &ClaimId::new(payload.claim_id.clone()),
                    payload.ttl_seconds,
                )?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    conflicts: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
            ClaimActionInput::Release => {
                let payload: ClaimReleasePayload = serde_json::from_value(args.payload)?;
                let claim = prism
                    .coordination()
                    .release_claim(meta, &ClaimId::new(payload.claim_id.clone()))?;
                Ok(ClaimMutationResult {
                    claim_id: Some(payload.claim_id),
                    conflicts: Vec::new(),
                    state: serde_json::to_value(claim_view(claim))?,
                })
            }
        }
    }

    fn apply_artifact_mutation(
        &self,
        prism: &Prism,
        args: PrismArtifactArgs,
        meta: EventMeta,
    ) -> Result<ArtifactMutationResult> {
        match args.action {
            ArtifactActionInput::Propose => {
                let payload: ArtifactProposePayload = serde_json::from_value(args.payload)?;
                let task_id = CoordinationTaskId::new(payload.task_id.clone());
                let anchors = match payload.anchors {
                    Some(anchors) => convert_anchors(anchors)?,
                    None => prism
                        .coordination_task(&task_id)
                        .map(|task| task.anchors)
                        .unwrap_or_default(),
                };
                let (artifact_id, artifact) = prism.coordination().propose_artifact(
                    meta,
                    ArtifactProposeInput {
                        task_id,
                        anchors,
                        diff_ref: payload.diff_ref,
                        evidence: payload
                            .evidence
                            .unwrap_or_default()
                            .into_iter()
                            .map(EventId::new)
                            .collect(),
                        base_revision: prism.workspace_revision(),
                    },
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(artifact_id.0.to_string()),
                    review_id: None,
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Supersede => {
                let payload: ArtifactSupersedePayload = serde_json::from_value(args.payload)?;
                let artifact = prism.coordination().supersede_artifact(
                    meta,
                    ArtifactSupersedeInput {
                        artifact_id: ArtifactId::new(payload.artifact_id.clone()),
                    },
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: None,
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
            ArtifactActionInput::Review => {
                let payload: ArtifactReviewPayload = serde_json::from_value(args.payload)?;
                let (review_id, _, artifact) = prism.coordination().review_artifact(
                    meta,
                    ArtifactReviewInput {
                        artifact_id: ArtifactId::new(payload.artifact_id.clone()),
                        verdict: parse_review_verdict(&payload.verdict)?,
                        summary: payload.summary,
                    },
                    prism.workspace_revision(),
                )?;
                Ok(ArtifactMutationResult {
                    artifact_id: Some(payload.artifact_id),
                    review_id: Some(review_id.0.to_string()),
                    state: serde_json::to_value(artifact_view(artifact))?,
                })
            }
        }
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

    fn curator_jobs(&self, args: CuratorJobsArgs) -> Result<Vec<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(Vec::new());
        };
        let mut jobs = workspace
            .curator_snapshot()
            .records
            .into_iter()
            .filter(|record| {
                args.status
                    .as_deref()
                    .is_none_or(|status| curator_job_status_label(record) == status)
                    && args
                        .trigger
                        .as_deref()
                        .is_none_or(|trigger| curator_trigger_label(&record.job.trigger) == trigger)
            })
            .map(curator_job_view)
            .collect::<Result<Vec<_>>>()?;

        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if let Some(limit) = args.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    fn curator_job(&self, job_id: &str) -> Result<Option<CuratorJobView>> {
        self.refresh_workspace()?;
        let Some(workspace) = &self.workspace else {
            return Ok(None);
        };
        workspace
            .curator_snapshot()
            .records
            .into_iter()
            .find(|record| record.id.0 == job_id)
            .map(curator_job_view)
            .transpose()
    }

    fn promote_curator_edge(
        &self,
        args: PrismCuratorPromoteEdgeArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.refresh_workspace()?;
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot();
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }
        let proposal = curator_proposal(record, args.proposal_index)?;
        let CuratorProposal::InferredEdge(candidate) = proposal else {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is not an inferred edge",
                args.proposal_index,
                args.job_id
            ));
        };

        let task = self
            .session
            .task_for_mutation(args.task_id.map(TaskId::new));
        let scope =
            args.scope
                .map(convert_inferred_scope)
                .unwrap_or_else(|| match candidate.scope {
                    InferredEdgeScope::SessionOnly => InferredEdgeScope::Persisted,
                    scope => scope,
                });
        let edge_id = self.session.inferred_edges.store_edge(
            candidate.edge.clone(),
            scope,
            Some(task.clone()),
            candidate.evidence.clone(),
        );
        if scope != InferredEdgeScope::SessionOnly {
            self.persist_inferred_edges()?;
        }
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Applied,
            Some(task),
            args.note,
            Some(edge_id.0.clone()),
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("applied curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal: serde_json::to_value(proposal)?,
            edge_id: Some(edge_id.0),
        })
    }

    fn reject_curator_proposal(
        &self,
        args: PrismCuratorRejectProposalArgs,
    ) -> Result<CuratorProposalDecisionResult> {
        self.refresh_workspace()?;
        let workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| anyhow!("curator mutations require a workspace-backed session"))?;
        let job_id = CuratorJobId(args.job_id.clone());
        let snapshot = workspace.curator_snapshot();
        let record = snapshot
            .records
            .iter()
            .find(|record| record.id == job_id)
            .ok_or_else(|| anyhow!("unknown curator job `{}`", args.job_id))?;
        let proposal_state = curator_proposal_state(record, args.proposal_index)?;
        if proposal_state.disposition != CuratorProposalDisposition::Pending {
            return Err(anyhow!(
                "curator proposal {} for job `{}` is already {}",
                args.proposal_index,
                args.job_id,
                curator_disposition_label(proposal_state.disposition)
            ));
        }

        let task = self
            .session
            .task_for_mutation(args.task_id.map(TaskId::new));
        workspace.set_curator_proposal_state(
            &job_id,
            args.proposal_index,
            CuratorProposalDisposition::Rejected,
            Some(task),
            args.reason,
            None,
        )?;
        let proposal = self
            .curator_job(&args.job_id)?
            .and_then(|job| {
                job.proposals
                    .into_iter()
                    .find(|proposal| proposal.index == args.proposal_index)
            })
            .ok_or_else(|| anyhow!("rejected curator proposal could not be reloaded"))?;
        Ok(CuratorProposalDecisionResult {
            job_id: args.job_id,
            proposal: serde_json::to_value(proposal)?,
            edge_id: None,
        })
    }
}

fn curator_job_view(record: CuratorJobRecord) -> Result<CuratorJobView> {
    let id = record.id.0.clone();
    let trigger = curator_trigger_label(&record.job.trigger).to_owned();
    let status = curator_job_status_label(&record).to_owned();
    let task_id = record.job.task.as_ref().map(|task| task.0.to_string());
    let run = record.run.clone().unwrap_or_default();
    let mut proposals = Vec::with_capacity(run.proposals.len());
    for (index, proposal) in run.proposals.into_iter().enumerate() {
        let state = record
            .proposal_states
            .get(index)
            .cloned()
            .unwrap_or_default();
        proposals.push(curator_proposal_view(index, proposal, state)?);
    }
    Ok(CuratorJobView {
        id,
        trigger,
        status,
        task_id,
        focus: record.job.focus,
        created_at: record.created_at,
        started_at: record.started_at,
        finished_at: record.finished_at,
        proposals,
        diagnostics: run
            .diagnostics
            .into_iter()
            .map(|diagnostic| QueryDiagnostic {
                code: diagnostic.code,
                message: diagnostic.message,
                data: diagnostic.data,
            })
            .collect(),
        error: record.error,
    })
}

fn curator_proposal_view(
    index: usize,
    proposal: CuratorProposal,
    state: prism_curator::CuratorProposalState,
) -> Result<CuratorProposalView> {
    let (kind, payload) = match proposal {
        CuratorProposal::InferredEdge(candidate) => {
            ("inferred_edge", serde_json::to_value(candidate)?)
        }
        CuratorProposal::StructuralMemory(candidate) => {
            ("structural_memory", serde_json::to_value(candidate)?)
        }
        CuratorProposal::RiskSummary(candidate) => {
            ("risk_summary", serde_json::to_value(candidate)?)
        }
        CuratorProposal::ValidationRecipe(candidate) => {
            ("validation_recipe", serde_json::to_value(candidate)?)
        }
    };
    Ok(CuratorProposalView {
        index,
        kind: kind.to_owned(),
        disposition: curator_disposition_label(state.disposition).to_owned(),
        payload,
        decided_at: state.decided_at,
        task_id: state.task.map(|task| task.0.to_string()),
        note: state.note,
        output: state.output,
    })
}

fn curator_job_status_label(record: &CuratorJobRecord) -> &'static str {
    match record.status {
        prism_curator::CuratorJobStatus::Queued => "queued",
        prism_curator::CuratorJobStatus::Running => "running",
        prism_curator::CuratorJobStatus::Completed => "completed",
        prism_curator::CuratorJobStatus::Failed => "failed",
        prism_curator::CuratorJobStatus::Skipped => "skipped",
    }
}

fn curator_trigger_label(trigger: &CuratorTrigger) -> &'static str {
    match trigger {
        CuratorTrigger::Manual => "manual",
        CuratorTrigger::PostChange => "post_change",
        CuratorTrigger::TaskCompleted => "task_completed",
        CuratorTrigger::RepeatedFailure => "repeated_failure",
        CuratorTrigger::AmbiguousLineage => "ambiguous_lineage",
        CuratorTrigger::HotspotChanged => "hotspot_changed",
    }
}

fn curator_disposition_label(disposition: CuratorProposalDisposition) -> &'static str {
    match disposition {
        CuratorProposalDisposition::Pending => "pending",
        CuratorProposalDisposition::Applied => "applied",
        CuratorProposalDisposition::Rejected => "rejected",
    }
}

fn curator_proposal_state(
    record: &CuratorJobRecord,
    proposal_index: usize,
) -> Result<prism_curator::CuratorProposalState> {
    if record
        .run
        .as_ref()
        .and_then(|run| run.proposals.get(proposal_index))
        .is_none()
    {
        return Err(anyhow!("unknown curator proposal index {proposal_index}"));
    }
    Ok(record
        .proposal_states
        .get(proposal_index)
        .cloned()
        .unwrap_or_default())
}

fn curator_proposal(record: &CuratorJobRecord, proposal_index: usize) -> Result<&CuratorProposal> {
    record
        .run
        .as_ref()
        .and_then(|run| run.proposals.get(proposal_index))
        .ok_or_else(|| anyhow!("unknown curator proposal index {proposal_index}"))
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

fn workspace_revision_view(value: WorkspaceRevision) -> WorkspaceRevisionView {
    WorkspaceRevisionView {
        graph_version: value.graph_version,
        git_commit: value.git_commit.map(|commit| commit.to_string()),
    }
}

fn plan_view(value: prism_coordination::Plan) -> PlanView {
    PlanView {
        id: value.id.0.to_string(),
        goal: value.goal,
        status: value.status,
        root_task_ids: value
            .root_tasks
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
    }
}

fn coordination_task_view(value: prism_coordination::CoordinationTask) -> CoordinationTaskView {
    CoordinationTaskView {
        id: value.id.0.to_string(),
        plan_id: value.plan.0.to_string(),
        title: value.title,
        status: value.status,
        assignee: value.assignee.map(|agent| agent.0.to_string()),
        anchors: value.anchors,
        depends_on: value
            .depends_on
            .into_iter()
            .map(|task_id| task_id.0.to_string())
            .collect(),
        base_revision: workspace_revision_view(value.base_revision),
    }
}

fn claim_view(value: prism_coordination::WorkClaim) -> ClaimView {
    ClaimView {
        id: value.id.0.to_string(),
        holder: value.holder.0.to_string(),
        task_id: value.task.map(|task| task.0.to_string()),
        capability: value.capability,
        mode: value.mode,
        status: value.status,
        anchors: value.anchors,
        expires_at: value.expires_at,
        base_revision: workspace_revision_view(value.base_revision),
    }
}

fn conflict_view(value: prism_coordination::CoordinationConflict) -> ConflictView {
    ConflictView {
        severity: value.severity,
        summary: value.summary,
        anchors: value.anchors,
        blocking_claim_ids: value
            .blocking_claims
            .into_iter()
            .map(|claim_id| claim_id.0.to_string())
            .collect(),
    }
}

fn blocker_view(value: prism_coordination::TaskBlocker) -> BlockerView {
    BlockerView {
        kind: value.kind,
        summary: value.summary,
        related_task_id: value.related_task_id.map(|task_id| task_id.0.to_string()),
        related_artifact_id: value
            .related_artifact_id
            .map(|artifact_id| artifact_id.0.to_string()),
    }
}

fn artifact_view(value: prism_coordination::Artifact) -> ArtifactView {
    ArtifactView {
        id: value.id.0.to_string(),
        task_id: value.task.0.to_string(),
        status: value.status,
        anchors: value.anchors,
        base_revision: workspace_revision_view(value.base_revision),
        diff_ref: value.diff_ref,
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

fn inferred_edge_record_view(record: prism_agent::InferredEdgeRecord) -> InferredEdgeRecordView {
    InferredEdgeRecordView {
        id: record.id.0,
        edge: edge_view(record.edge),
        scope: record.scope,
        task_id: record.task.map(|task| task.0.to_string()),
        evidence: record.evidence,
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
    let status = lineage_status(&events);
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

fn lineage_status(events: &[prism_ir::LineageEvent]) -> LineageStatus {
    if events
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
    }
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
            "plan" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_plan(&PlanId::new(args.plan_id))
                        .map(plan_view),
                )?)
            }
            "coordinationTask" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .coordination_task(&CoordinationTaskId::new(args.task_id))
                        .map(coordination_task_view),
                )?)
            }
            "readyTasks" => {
                let args: PlanTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .ready_tasks(&PlanId::new(args.plan_id), current_timestamp())
                        .into_iter()
                        .map(coordination_task_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "claims" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .claims(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(claim_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "conflicts" => {
                let args: AnchorListArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .conflicts(&convert_anchors(args.anchors)?, current_timestamp())
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "blockers" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                let blockers = self.prism.blockers(
                    &CoordinationTaskId::new(args.task_id.clone()),
                    current_timestamp(),
                );
                if !blockers.is_empty() {
                    self.push_diagnostic(
                        "task_blocked",
                        format!(
                            "Coordination task `{}` currently has blockers.",
                            args.task_id
                        ),
                        Some(json!({ "taskId": args.task_id, "count": blockers.len() })),
                    );
                }
                if blockers
                    .iter()
                    .any(|blocker| blocker.kind == prism_coordination::BlockerKind::StaleRevision)
                {
                    self.push_diagnostic(
                        "stale_revision",
                        "The coordination task is based on a stale workspace revision.",
                        None,
                    );
                }
                Ok(serde_json::to_value(
                    blockers.into_iter().map(blocker_view).collect::<Vec<_>>(),
                )?)
            }
            "pendingReviews" => {
                let args: PendingReviewsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .pending_reviews(
                            args.plan_id
                                .as_ref()
                                .map(|plan_id| PlanId::new(plan_id.clone()))
                                .as_ref(),
                        )
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "artifacts" => {
                let args: CoordinationTaskTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .artifacts(&CoordinationTaskId::new(args.task_id))
                        .into_iter()
                        .map(artifact_view)
                        .collect::<Vec<_>>(),
                )?)
            }
            "simulateClaim" => {
                let args: SimulateClaimArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.prism
                        .simulate_claim(
                            &self.host.session.session_id(),
                            &convert_anchors(args.anchors)?,
                            parse_capability(&args.capability)?,
                            args.mode.as_deref().map(parse_claim_mode).transpose()?,
                            args.task_id
                                .as_ref()
                                .map(|task_id| CoordinationTaskId::new(task_id.clone()))
                                .as_ref(),
                            current_timestamp(),
                        )
                        .into_iter()
                        .map(conflict_view)
                        .collect::<Vec<_>>(),
                )?)
            }
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
            "curatorJobs" => {
                let args: CuratorJobsArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.curator_jobs(args)?)?)
            }
            "curatorJob" => {
                let args: CuratorJobArgs = serde_json::from_value(args)?;
                let job = self.host.curator_job(&args.job_id)?;
                if job.is_none() {
                    self.push_diagnostic(
                        "anchor_unresolved",
                        format!("No curator job matched `{}`.", args.job_id),
                        Some(json!({ "jobId": args.job_id })),
                    );
                }
                Ok(serde_json::to_value(job)?)
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

#[derive(Debug, Clone, Deserialize)]
struct CuratorJobsArgs {
    status: Option<String>,
    trigger: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct CuratorJobArgs {
    job_id: String,
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

fn parse_capability(value: &str) -> Result<Capability> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "observe" => Ok(Capability::Observe),
        "edit" => Ok(Capability::Edit),
        "review" => Ok(Capability::Review),
        "validate" => Ok(Capability::Validate),
        "merge" => Ok(Capability::Merge),
        other => Err(anyhow!("unknown capability `{other}`")),
    }
}

fn parse_claim_mode(value: &str) -> Result<ClaimMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "advisory" => Ok(ClaimMode::Advisory),
        "softexclusive" | "soft-exclusive" | "soft_exclusive" => Ok(ClaimMode::SoftExclusive),
        "hardexclusive" | "hard-exclusive" | "hard_exclusive" => Ok(ClaimMode::HardExclusive),
        other => Err(anyhow!("unknown claim mode `{other}`")),
    }
}

fn parse_coordination_task_status(value: &str) -> Result<CoordinationTaskStatus> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "proposed" => Ok(CoordinationTaskStatus::Proposed),
        "ready" => Ok(CoordinationTaskStatus::Ready),
        "inprogress" | "in-progress" => Ok(CoordinationTaskStatus::InProgress),
        "blocked" => Ok(CoordinationTaskStatus::Blocked),
        "inreview" | "in-review" => Ok(CoordinationTaskStatus::InReview),
        "validating" => Ok(CoordinationTaskStatus::Validating),
        "completed" => Ok(CoordinationTaskStatus::Completed),
        "abandoned" => Ok(CoordinationTaskStatus::Abandoned),
        other => Err(anyhow!("unknown coordination task status `{other}`")),
    }
}

fn parse_review_verdict(value: &str) -> Result<ReviewVerdict> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "approved" => Ok(ReviewVerdict::Approved),
        "changesrequested" | "changes-requested" | "changes_requested" => {
            Ok(ReviewVerdict::ChangesRequested)
        }
        "rejected" => Ok(ReviewVerdict::Rejected),
        other => Err(anyhow!("unknown review verdict `{other}`")),
    }
}

fn convert_policy(
    payload: Option<CoordinationPolicyPayload>,
) -> Result<Option<CoordinationPolicy>> {
    let Some(payload) = payload else {
        return Ok(None);
    };
    let mut policy = CoordinationPolicy::default();
    if let Some(mode) = payload.default_claim_mode {
        policy.default_claim_mode = parse_claim_mode(&mode)?;
    }
    if let Some(value) = payload.max_parallel_editors_per_anchor {
        policy.max_parallel_editors_per_anchor = value;
    }
    if let Some(value) = payload.require_review_for_completion {
        policy.require_review_for_completion = value;
    }
    if let Some(value) = payload.stale_after_graph_change {
        policy.stale_after_graph_change = value;
    }
    Ok(Some(policy))
}

fn convert_acceptance(
    payload: Option<Vec<AcceptanceCriterionPayload>>,
) -> Result<Vec<AcceptanceCriterion>> {
    payload
        .unwrap_or_default()
        .into_iter()
        .map(|criterion| {
            Ok(AcceptanceCriterion {
                label: criterion.label,
                anchors: convert_anchors(criterion.anchors.unwrap_or_default())?,
            })
        })
        .collect()
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
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rmcp::{
        model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
        transport::{IntoTransport, Transport},
        ServiceExt,
    };

    use super::*;
    use prism_core::{index_workspace_session, index_workspace_session_with_curator};
    use prism_curator::{
        CandidateEdge, CandidateRiskSummary, CuratorBackend, CuratorContext, CuratorJob,
        CuratorProposal, CuratorRun,
    };
    use prism_history::HistoryStore;
    use prism_ir::{
        AnchorRef, Edge, EdgeKind, EventActor, EventId, EventMeta, FileId, Language, Node, NodeId,
        NodeKind, Span, TaskId,
    };
    use prism_memory::{
        MemoryKind, OutcomeEvent, OutcomeEvidence, OutcomeKind, OutcomeMemory, OutcomeResult,
        RecallQuery,
    };
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

    fn wait_for_completed_curator_job(session: &WorkspaceSession) -> String {
        for _ in 0..200 {
            let snapshot = session.curator_snapshot();
            if let Some(record) = snapshot
                .records
                .iter()
                .find(|record| curator_job_status_label(record) == "completed")
            {
                return record.id.0.clone();
            }
            thread::sleep(Duration::from_millis(50));
        }
        let snapshot = session.curator_snapshot();
        panic!(
            "timed out waiting for completed curator job; statuses: {:?}",
            snapshot
                .records
                .iter()
                .map(|record| (record.id.0.clone(), curator_job_status_label(record)))
                .collect::<Vec<_>>()
        );
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
        if !response["error"].is_null() {
            panic!("tool call failed: {response}");
        }
        if !response["result"]["structuredContent"].is_null() {
            return response["result"]["structuredContent"].clone();
        }
        if let Some(content) = response["result"]["content"].as_array() {
            if let Some(json) = content.iter().find_map(|item| {
                let json = item.get("json")?;
                if json.is_null() {
                    None
                } else {
                    Some(json.clone())
                }
            }) {
                return json;
            }
        }
        let text = response["result"]["content"]
            .as_array()
            .and_then(|content| {
                content
                    .iter()
                    .find_map(|item| item.get("text").and_then(Value::as_str))
            })
            .unwrap_or_else(|| panic!("tool result should contain json text: {response}"));
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

    #[test]
    fn coordination_mutations_flow_through_query_runtime() {
        let host = host_with_node(demo_node());
        let plan = host
            .store_coordination(PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::PlanCreate,
                payload: json!({ "goal": "Ship coordination" }),
                task_id: None,
            })
            .unwrap();
        assert_eq!(plan.state["goal"], "Ship coordination");

        let task = host
            .store_coordination(PrismCoordinationArgs {
                kind: CoordinationMutationKindInput::TaskCreate,
                payload: json!({
                    "planId": plan.state["id"].as_str().unwrap(),
                    "title": "Edit main",
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }]
                }),
                task_id: None,
            })
            .unwrap();
        let task_id = task.state["id"].as_str().unwrap().to_string();

        let claim = host
            .store_claim(PrismClaimArgs {
                action: ClaimActionInput::Acquire,
                payload: json!({
                    "anchors": [{
                        "type": "node",
                        "crateName": "demo",
                        "path": "demo::main",
                        "kind": "function"
                    }],
                    "capability": "Edit",
                    "mode": "SoftExclusive",
                    "coordinationTaskId": task_id
                }),
                task_id: None,
            })
            .unwrap();
        assert!(claim.claim_id.is_some());

        let artifact = host
            .store_artifact(PrismArtifactArgs {
                action: ArtifactActionInput::Propose,
                payload: json!({
                    "taskId": task.state["id"].as_str().unwrap(),
                    "diffRef": "patch:1"
                }),
                task_id: None,
            })
            .unwrap();
        assert!(artifact.artifact_id.is_some());

        let execution = QueryExecution::new(host.clone(), host.current_prism());
        let plan_value = execution
            .dispatch("plan", r#"{ "planId": "plan:1" }"#)
            .unwrap();
        let ready_value = execution
            .dispatch("readyTasks", r#"{ "planId": "plan:1" }"#)
            .unwrap();
        let claims_value = execution
            .dispatch(
                "claims",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }] }"#,
            )
            .unwrap();
        let simulated_value = execution
            .dispatch(
                "simulateClaim",
                r#"{ "anchors": [{ "type": "node", "crateName": "demo", "path": "demo::main", "kind": "function" }], "capability": "Edit", "mode": "HardExclusive" }"#,
            )
            .unwrap_or_else(|error| panic!("simulateClaim dispatch failed: {error:#}"));
        let artifacts_value = execution
            .dispatch("artifacts", r#"{ "taskId": "coord-task:1" }"#)
            .unwrap();
        assert_eq!(plan_value["goal"], "Ship coordination");
        assert_eq!(ready_value.as_array().unwrap().len(), 1);
        assert_eq!(claims_value.as_array().unwrap().len(), 1);
        assert_eq!(artifacts_value.as_array().unwrap().len(), 1);
        assert!(simulated_value.as_array().unwrap().is_empty());
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
        assert!(tool_names.contains(&"prism_coordination"));
        assert!(tool_names.contains(&"prism_claim"));
        assert!(tool_names.contains(&"prism_artifact"));
        assert!(tool_names.contains(&"prism_curator_promote_edge"));
        assert!(tool_names.contains(&"prism_curator_reject_proposal"));

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

    #[tokio::test]
    async fn mcp_server_executes_coordination_mutations_and_reads_via_prism_query() {
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
                "prism_coordination",
                json!({
                    "kind": "plan_create",
                    "payload": { "goal": "Coordinate the main edit" }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let plan = first_tool_content_json(client.receive().await.unwrap());
        let plan_id = plan["state"]["id"].as_str().unwrap().to_string();

        client
            .send(call_tool_request(
                3,
                "prism_coordination",
                json!({
                    "kind": "task_create",
                    "payload": {
                        "planId": plan_id,
                        "title": "Edit main",
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }]
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let task = first_tool_content_json(client.receive().await.unwrap());
        let task_id = task["state"]["id"].as_str().unwrap().to_string();

        client
            .send(call_tool_request(
                4,
                "prism_claim",
                json!({
                    "action": "acquire",
                    "payload": {
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }],
                        "capability": "Edit",
                        "mode": "SoftExclusive",
                        "coordinationTaskId": task_id
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let claim = first_tool_content_json(client.receive().await.unwrap());
        assert!(claim["claimId"].as_str().is_some());

        client
            .send(call_tool_request(
                5,
                "prism_artifact",
                json!({
                    "action": "propose",
                    "payload": {
                        "taskId": task["state"]["id"].as_str().unwrap(),
                        "diffRef": "patch:1"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let artifact = first_tool_content_json(client.receive().await.unwrap());
        assert!(artifact["artifactId"].as_str().is_some());

        client
            .send(call_tool_request(
                6,
                "prism_query",
                json!({
                    "code": format!(
                        r#"
const sym = prism.symbol("main");
return {{
  plan: prism.plan("{plan_id}"),
  ready: prism.readyTasks("{plan_id}"),
  claims: sym ? prism.claims(sym) : [],
  artifacts: prism.artifacts("{task_id}"),
}};
"#
                    ),
                    "language": "ts",
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let envelope = first_tool_content_json(client.receive().await.unwrap());
        assert_eq!(
            envelope["result"]["plan"]["goal"],
            "Coordinate the main edit"
        );
        assert_eq!(envelope["result"]["ready"].as_array().unwrap().len(), 1);
        assert_eq!(envelope["result"]["claims"].as_array().unwrap().len(), 1);
        assert_eq!(envelope["result"]["artifacts"].as_array().unwrap().len(), 1);

        running.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_server_reports_review_queues_and_blockers_via_prism_query() {
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
                "prism_coordination",
                json!({
                    "kind": "plan_create",
                    "payload": {
                        "goal": "Review-gated change",
                        "policy": { "requireReviewForCompletion": true }
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let plan = first_tool_content_json(client.receive().await.unwrap());
        let plan_id = plan["state"]["id"].as_str().unwrap().to_string();

        client
            .send(call_tool_request(
                3,
                "prism_coordination",
                json!({
                    "kind": "task_create",
                    "payload": {
                        "planId": plan_id,
                        "title": "Patch main",
                        "anchors": [{
                            "type": "node",
                            "crateName": "demo",
                            "path": "demo::main",
                            "kind": "function"
                        }]
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let task = first_tool_content_json(client.receive().await.unwrap());
        let task_id = task["state"]["id"].as_str().unwrap().to_string();

        client
            .send(call_tool_request(
                4,
                "prism_artifact",
                json!({
                    "action": "propose",
                    "payload": {
                        "taskId": task_id,
                        "diffRef": "patch:review-gated"
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let artifact = first_tool_content_json(client.receive().await.unwrap());
        assert!(artifact["artifactId"].as_str().is_some());

        client
            .send(call_tool_request(
                5,
                "prism_query",
                json!({
                    "code": format!(
                        r#"
return {{
  blockers: prism.blockers("{task_id}"),
  pendingReviews: prism.pendingReviews("{plan_id}"),
}};
"#
                    ),
                    "language": "ts",
                })
                .as_object()
                .unwrap()
                .clone(),
            ))
            .await
            .unwrap();
        let envelope = first_tool_content_json(client.receive().await.unwrap());
        assert_eq!(
            envelope["result"]["blockers"][0]["kind"],
            Value::String("ReviewRequired".to_string())
        );
        assert_eq!(
            envelope["result"]["pendingReviews"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            envelope["diagnostics"][0]["code"],
            Value::String("task_blocked".to_string())
        );

        running.cancel().await.unwrap();
    }

    #[test]
    fn curator_reads_flow_through_prism_query_and_edge_promotion_is_explicit() {
        let root = temp_workspace();

        #[derive(Default)]
        struct FakeCurator;

        impl CuratorBackend for FakeCurator {
            fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
                Ok(CuratorRun {
                    proposals: vec![CuratorProposal::InferredEdge(CandidateEdge {
                        edge: Edge {
                            kind: EdgeKind::Calls,
                            source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                            target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                            origin: prism_ir::EdgeOrigin::Inferred,
                            confidence: 0.82,
                        },
                        scope: InferredEdgeScope::SessionOnly,
                        evidence: vec!["observed repeated edits".into()],
                        rationale: "alpha usually routes to beta after validation".into(),
                    })],
                    diagnostics: Vec::new(),
                })
            }
        }

        let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
        let alpha = session
            .prism()
            .symbol("alpha")
            .into_iter()
            .next()
            .unwrap()
            .id()
            .clone();
        session
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:validated"),
                    ts: 50,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha)],
                kind: OutcomeKind::FixValidated,
                result: OutcomeResult::Success,
                summary: "validated alpha change".into(),
                evidence: Vec::new(),
                metadata: Value::Null,
            })
            .unwrap();
        let job_id = wait_for_completed_curator_job(&session);
        let host = QueryHost::with_session(session);

        let jobs = host
            .execute(
                r#"
return prism.curator.jobs({ status: "completed", limit: 5 });
"#,
                QueryLanguage::Ts,
            )
            .unwrap();
        assert_eq!(jobs.result.as_array().map(|items| items.len()), Some(1));
        assert_eq!(jobs.result[0]["id"], job_id);
        assert_eq!(jobs.result[0]["proposals"][0]["kind"], "inferred_edge");
        assert_eq!(jobs.result[0]["proposals"][0]["disposition"], "pending");

        let promoted = host
            .promote_curator_edge(PrismCuratorPromoteEdgeArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                scope: Some(InferredEdgeScopeInput::Persisted),
                note: Some("accepted after review".into()),
                task_id: Some("task:promotion".into()),
            })
            .unwrap();
        assert!(promoted.edge_id.is_some());

        let proposal = host
            .execute(
                &format!(
                    r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
                ),
                QueryLanguage::Ts,
            )
            .unwrap();
        assert_eq!(proposal.result["disposition"], "applied");
        assert_eq!(proposal.result["output"], promoted.edge_id.unwrap());
    }

    #[test]
    fn curator_rejection_is_a_distinct_mutation() {
        let root = temp_workspace();

        #[derive(Default)]
        struct FakeCurator;

        impl CuratorBackend for FakeCurator {
            fn run(&self, _job: &CuratorJob, _ctx: &CuratorContext) -> anyhow::Result<CuratorRun> {
                Ok(CuratorRun {
                    proposals: vec![CuratorProposal::RiskSummary(CandidateRiskSummary {
                        anchors: Vec::new(),
                        summary: "alpha looks risky".into(),
                        severity: "medium".into(),
                        evidence_events: Vec::new(),
                    })],
                    diagnostics: Vec::new(),
                })
            }
        }

        let session = index_workspace_session_with_curator(&root, Arc::new(FakeCurator)).unwrap();
        let alpha = session
            .prism()
            .symbol("alpha")
            .into_iter()
            .next()
            .unwrap()
            .id()
            .clone();
        session
            .append_outcome(OutcomeEvent {
                meta: EventMeta {
                    id: EventId::new("outcome:failure"),
                    ts: 51,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:alpha")),
                    causation: None,
                },
                anchors: vec![AnchorRef::Node(alpha)],
                kind: OutcomeKind::FixValidated,
                result: OutcomeResult::Success,
                summary: "alpha follow-up validated".into(),
                evidence: Vec::new(),
                metadata: Value::Null,
            })
            .unwrap();
        let job_id = wait_for_completed_curator_job(&session);
        let host = QueryHost::with_session(session);

        let rejected = host
            .reject_curator_proposal(PrismCuratorRejectProposalArgs {
                job_id: job_id.clone(),
                proposal_index: 0,
                reason: Some("not enough evidence".into()),
                task_id: Some("task:review".into()),
            })
            .unwrap();
        assert!(rejected.edge_id.is_none());

        let proposal = host
            .execute(
                &format!(
                    r#"
return prism.curator.job("{job_id}")?.proposals[0];
"#
                ),
                QueryLanguage::Ts,
            )
            .unwrap();
        assert_eq!(proposal.result["disposition"], "rejected");
        assert_eq!(proposal.result["note"], "not enough evidence");
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
    fn persisted_notes_reload_with_workspace_session() {
        let root = temp_workspace();
        let host = QueryHost::with_session(index_workspace_session(&root).unwrap());

        host.store_note(PrismNoteArgs {
            anchors: vec![AnchorRefInput::Node {
                crate_name: "demo".to_string(),
                path: "demo::alpha".to_string(),
                kind: "function".to_string(),
            }],
            content: "alpha previously regressed".to_string(),
            trust: Some(0.9),
            task_id: Some("task:note".to_string()),
        })
        .expect("note should persist");

        let reloaded = QueryHost::with_session(index_workspace_session(&root).unwrap());
        let replay = reloaded
            .current_prism()
            .resume_task(&TaskId::new("task:note"));
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].kind, OutcomeKind::NoteAdded);

        let recalled = reloaded
            .session
            .notes
            .recall(&RecallQuery {
                focus: vec![AnchorRef::Node(NodeId::new(
                    "demo",
                    "demo::alpha",
                    NodeKind::Function,
                ))],
                text: Some("regressed".to_string()),
                limit: 5,
                kinds: Some(vec![MemoryKind::Episodic]),
                since: None,
            })
            .expect("recall should succeed");
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].entry.content, "alpha previously regressed");
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

        let memory = host
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

        assert!(memory.memory_id.starts_with("episodic:"));
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
        let event = host
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
        assert_eq!(replay.events[0].meta.id.0, event.event_id);
    }
}
