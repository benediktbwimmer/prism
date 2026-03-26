use prism_coordination::BlockerKind;
use prism_ir::{
    AnchorRef, ArtifactStatus, Capability, ClaimMode, ClaimStatus, ConflictSeverity,
    CoordinationTaskStatus, EdgeKind, EdgeOrigin, Language, NodeKind, PlanStatus, Span,
};
use prism_memory::OutcomeEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_REFERENCE_URI: &str = "prism://api-reference";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdView {
    pub crate_name: String,
    pub path: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SymbolView {
    pub id: NodeIdView,
    pub name: String,
    pub kind: NodeKind,
    pub signature: String,
    pub file_path: Option<String>,
    pub span: Span,
    pub language: Language,
    pub lineage_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelationsView {
    pub contains: Vec<SymbolView>,
    pub callers: Vec<SymbolView>,
    pub callees: Vec<SymbolView>,
    pub references: Vec<SymbolView>,
    pub imports: Vec<SymbolView>,
    pub implements: Vec<SymbolView>,
    pub specifies: Vec<SymbolView>,
    pub specified_by: Vec<SymbolView>,
    pub validates: Vec<SymbolView>,
    pub validated_by: Vec<SymbolView>,
    pub related: Vec<SymbolView>,
    pub related_by: Vec<SymbolView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageView {
    pub lineage_id: String,
    pub current: SymbolView,
    pub status: LineageStatus,
    pub history: Vec<LineageEventView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineageStatus {
    Active,
    Dead,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageEventView {
    pub event_id: String,
    pub ts: u64,
    pub kind: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgeView {
    pub kind: EdgeKind,
    pub source: NodeIdView,
    pub target: NodeIdView,
    pub origin: EdgeOrigin,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphView {
    pub nodes: Vec<SymbolView>,
    pub edges: Vec<EdgeView>,
    pub truncated: bool,
    pub max_depth_reached: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeImpactView {
    pub direct_nodes: Vec<NodeIdView>,
    pub lineages: Vec<String>,
    pub likely_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationCheckView {
    pub label: String,
    pub score: f32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoChangeView {
    pub lineage: String,
    pub count: u32,
    pub nodes: Vec<NodeIdView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRecipeView {
    pub target: NodeIdView,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskValidationRecipeView {
    pub task_id: String,
    pub checks: Vec<String>,
    pub scored_checks: Vec<ValidationCheckView>,
    pub related_nodes: Vec<NodeIdView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRiskView {
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale_task: bool,
    pub has_approved_artifact: bool,
    pub likely_validations: Vec<String>,
    pub missing_validations: Vec<String>,
    pub validation_checks: Vec<ValidationCheckView>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
    pub approved_artifact_ids: Vec<String>,
    pub stale_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRiskView {
    pub artifact_id: String,
    pub task_id: String,
    pub risk_score: f32,
    pub review_required: bool,
    pub stale: bool,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub missing_validations: Vec<String>,
    pub co_change_neighbors: Vec<CoChangeView>,
    pub risk_events: Vec<OutcomeEvent>,
    pub promoted_summaries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftCandidateView {
    pub spec: NodeIdView,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub reasons: Vec<String>,
    pub recent_failures: Vec<OutcomeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIntentView {
    pub task_id: String,
    pub specs: Vec<NodeIdView>,
    pub implementations: Vec<NodeIdView>,
    pub validations: Vec<NodeIdView>,
    pub related: Vec<NodeIdView>,
    pub drift_candidates: Vec<DriftCandidateView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRevisionView {
    pub graph_version: u64,
    pub git_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub id: String,
    pub goal: String,
    pub status: PlanStatus,
    pub root_task_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationTaskView {
    pub id: String,
    pub plan_id: String,
    pub title: String,
    pub status: CoordinationTaskStatus,
    pub assignee: Option<String>,
    pub anchors: Vec<AnchorRef>,
    pub depends_on: Vec<String>,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimView {
    pub id: String,
    pub holder: String,
    pub task_id: Option<String>,
    pub capability: Capability,
    pub mode: ClaimMode,
    pub status: ClaimStatus,
    pub anchors: Vec<AnchorRef>,
    pub expires_at: u64,
    pub base_revision: WorkspaceRevisionView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    pub severity: ConflictSeverity,
    pub summary: String,
    pub anchors: Vec<AnchorRef>,
    pub blocking_claim_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockerView {
    pub kind: BlockerKind,
    pub summary: String,
    pub related_task_id: Option<String>,
    pub related_artifact_id: Option<String>,
    pub risk_score: Option<f32>,
    pub validation_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub task_id: String,
    pub status: ArtifactStatus,
    pub anchors: Vec<AnchorRef>,
    pub base_revision: WorkspaceRevisionView,
    pub diff_ref: Option<String>,
    pub required_validations: Vec<String>,
    pub validated_checks: Vec<String>,
    pub risk_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntryView {
    pub id: String,
    pub anchors: Vec<AnchorRef>,
    pub kind: String,
    pub content: String,
    pub metadata: Value,
    pub created_at: u64,
    pub source: String,
    pub trust: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoredMemoryView {
    pub id: String,
    pub entry: MemoryEntryView,
    pub score: f32,
    pub source_module: String,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorProposalView {
    pub index: usize,
    pub kind: String,
    pub disposition: String,
    pub payload: Value,
    pub decided_at: Option<u64>,
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CuratorJobView {
    pub id: String,
    pub trigger: String,
    pub status: String,
    pub task_id: Option<String>,
    pub focus: Vec<AnchorRef>,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub proposals: Vec<CuratorProposalView>,
    pub diagnostics: Vec<QueryDiagnostic>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryEnvelope {
    pub result: Value,
    pub diagnostics: Vec<QueryDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

pub fn api_reference_markdown() -> &'static str {
    r#"# PRISM Query API

`prism_query` executes a TypeScript snippet against a live in-memory PRISM graph.

Secondary convenience MCP tools are also available for the most common lookups:

- `prism_symbol { query }`
- `prism_search { query, limit?, kind?, path? }`

## Mental model

Treat this like a repo-specific read-only query shell.

- TypeScript is for composition.
- Prism is where semantic meaning should live.
- Return the final value with `return ...`.
- The returned value must be JSON-serializable.
- `language` currently supports only `"ts"`.
- `prism_query` is read-only in this implementation.

## Result shape

```ts
interface QueryResult {
  result: unknown;
  diagnostics: QueryDiagnostic[];
}

interface QueryDiagnostic {
  code: string;
  message: string;
  data?: Record<string, unknown>;
}
```

Diagnostics are how the server tells you a query was ambiguous, truncated, or capped.

## Type surface

```ts
type NodeId = {
  crateName: string;
  path: string;
  kind: string;
};

type SearchOptions = {
  limit?: number;
  kind?: string;
  path?: string;
  includeInferred?: boolean;
};

type MemoryRecallOptions = {
  focus?: Array<SymbolView | NodeId>;
  text?: string;
  limit?: number;
};

type CuratorJobQueryOptions = {
  status?: string;
  trigger?: string;
  limit?: number;
};

type PrismApi = {
  symbol(query: string): SymbolView | null;
  symbols(query: string): SymbolView[];
  search(query: string, options?: SearchOptions): SymbolView[];
  entrypoints(): SymbolView[];
  plan(planId: string): PlanView | null;
  task(taskId: string): CoordinationTaskView | null;
  readyTasks(planId: string): CoordinationTaskView[];
  claims(target: SymbolView | NodeId | AnchorRef | Array<SymbolView | NodeId | AnchorRef>): ClaimView[];
  conflicts(target: SymbolView | NodeId | AnchorRef | Array<SymbolView | NodeId | AnchorRef>): ConflictView[];
  blockers(taskId: string): BlockerView[];
  pendingReviews(planId?: string): ArtifactView[];
  artifacts(taskId: string): ArtifactView[];
  taskBlastRadius(taskId: string): ChangeImpactView | null;
  taskValidationRecipe(taskId: string): TaskValidationRecipeView | null;
  taskRisk(taskId: string): TaskRiskView | null;
  artifactRisk(artifactId: string): ArtifactRiskView | null;
  taskIntent(taskId: string): TaskIntentView | null;
  coordinationInbox(planId: string): CoordinationInboxView;
  taskContext(taskId: string): TaskContextView;
  claimPreview(input: {
    anchors: Array<SymbolView | NodeId | AnchorRef>;
    capability: string;
    mode?: string;
    taskId?: string;
  }): ClaimPreviewView;
  simulateClaim(input: {
    anchors: Array<SymbolView | NodeId | AnchorRef>;
    capability: string;
    mode?: string;
    taskId?: string;
  }): ConflictView[];
  lineage(target: SymbolView | NodeId): LineageView | null;
  coChangeNeighbors(target: SymbolView | NodeId): CoChangeView[];
  relatedFailures(target: SymbolView | NodeId): OutcomeEvent[];
  blastRadius(target: SymbolView | NodeId): ChangeImpactView | null;
  validationRecipe(target: SymbolView | NodeId): ValidationRecipeView | null;
  specFor(target: SymbolView | NodeId): SymbolView[];
  implementationFor(target: SymbolView | NodeId): SymbolView[];
  driftCandidates(limit?: number): DriftCandidateView[];
  resumeTask(taskId: string): TaskReplay;
  memory: {
    recall(options?: MemoryRecallOptions): ScoredMemoryView[];
  };
  curator: {
    jobs(options?: CuratorJobQueryOptions): CuratorJobView[];
    job(id: string): CuratorJobView | null;
  };
  diagnostics(): QueryDiagnostic[];
};

type SymbolView = {
  id: NodeId;
  name: string;
  kind: string;
  signature: string;
  filePath?: string;
  span: { start: number; end: number };
  language: string;
  lineageId?: string;
  full(): string;
  relations(): RelationsView;
  callGraph(depth?: number): Subgraph;
  lineage(): LineageView | null;
};

type RelationsView = {
  contains: SymbolView[];
  callers: SymbolView[];
  callees: SymbolView[];
  references: SymbolView[];
  imports: SymbolView[];
  implements: SymbolView[];
  specifies: SymbolView[];
  specifiedBy: SymbolView[];
  validates: SymbolView[];
  validatedBy: SymbolView[];
  related: SymbolView[];
  relatedBy: SymbolView[];
};

type LineageView = {
  lineageId: string;
  current: SymbolView;
  status: "active" | "dead" | "ambiguous";
  history: Array<{
    eventId: string;
    ts: number;
    kind: string;
    confidence: number;
  }>;
};

type Subgraph = {
  nodes: SymbolView[];
  edges: Array<{
    kind: string;
    source: NodeId;
    target: NodeId;
    origin: string;
    confidence: number;
  }>;
  truncated: boolean;
  maxDepthReached?: number;
};

type ChangeImpactView = {
  directNodes: NodeId[];
  lineages: string[];
  likelyValidations: string[];
  validationChecks: ValidationCheckView[];
  coChangeNeighbors: CoChangeView[];
  riskEvents: OutcomeEvent[];
  promotedSummaries: string[];
};

type ValidationRecipeView = {
  target: NodeId;
  checks: string[];
  scoredChecks: ValidationCheckView[];
  relatedNodes: NodeId[];
  coChangeNeighbors: CoChangeView[];
  recentFailures: OutcomeEvent[];
};

type DriftCandidateView = {
  spec: NodeId;
  implementations: NodeId[];
  validations: NodeId[];
  related: NodeId[];
  reasons: string[];
  recentFailures: OutcomeEvent[];
};

type TaskIntentView = {
  taskId: string;
  specs: NodeId[];
  implementations: NodeId[];
  validations: NodeId[];
  related: NodeId[];
  driftCandidates: DriftCandidateView[];
};

type TaskValidationRecipeView = {
  taskId: string;
  checks: string[];
  scoredChecks: ValidationCheckView[];
  relatedNodes: NodeId[];
  coChangeNeighbors: CoChangeView[];
  recentFailures: OutcomeEvent[];
};

type TaskRiskView = {
  taskId: string;
  riskScore: number;
  reviewRequired: boolean;
  staleTask: boolean;
  hasApprovedArtifact: boolean;
  likelyValidations: string[];
  missingValidations: string[];
  validationChecks: ValidationCheckView[];
  coChangeNeighbors: CoChangeView[];
  riskEvents: OutcomeEvent[];
  promotedSummaries: string[];
  approvedArtifactIds: string[];
  staleArtifactIds: string[];
};

type ArtifactRiskView = {
  artifactId: string;
  taskId: string;
  riskScore: number;
  reviewRequired: boolean;
  stale: boolean;
  requiredValidations: string[];
  validatedChecks: string[];
  missingValidations: string[];
  coChangeNeighbors: CoChangeView[];
  riskEvents: OutcomeEvent[];
  promotedSummaries: string[];
};

type CoordinationInboxView = {
  readyTasks: CoordinationTaskView[];
  pendingReviews: ArtifactView[];
};

type TaskContextView = {
  task: CoordinationTaskView | null;
  blockers: BlockerView[];
  artifacts: ArtifactView[];
  claims: ClaimView[];
  conflicts: ConflictView[];
  blastRadius: ChangeImpactView | null;
  validationRecipe: TaskValidationRecipeView | null;
  risk: TaskRiskView | null;
};

type ClaimPreviewView = {
  conflicts: ConflictView[];
  blocked: boolean;
  warnings: ConflictView[];
};

type ValidationCheckView = {
  label: string;
  score: number;
  lastSeen: number;
};

type CoChangeView = {
  lineage: string;
  count: number;
  nodes: NodeId[];
};

type OutcomeEvent = {
  summary: string;
  result: string;
  kind: string;
};

type ScoredMemoryView = {
  id: string;
  entry: MemoryEntryView;
  score: number;
  sourceModule: string;
  explanation?: string;
};

type MemoryEntryView = {
  id: string;
  anchors: AnchorRef[];
  kind: string;
  content: string;
  metadata: unknown;
  createdAt: number;
  source: string;
  trust: number;
};

type AnchorRef =
  | { Node: NodeId }
  | { Lineage: string }
  | { File: number }
  | { Kind: string };

type TaskReplay = {
  task: string;
  events: OutcomeEvent[];
};

type WorkspaceRevisionView = {
  graphVersion: number;
  gitCommit?: string;
};

type PlanView = {
  id: string;
  goal: string;
  status: string;
  rootTaskIds: string[];
};

type CoordinationTaskView = {
  id: string;
  planId: string;
  title: string;
  status: string;
  assignee?: string;
  anchors: AnchorRef[];
  dependsOn: string[];
  baseRevision: WorkspaceRevisionView;
};

type ClaimView = {
  id: string;
  holder: string;
  taskId?: string;
  capability: string;
  mode: string;
  status: string;
  anchors: AnchorRef[];
  expiresAt: number;
  baseRevision: WorkspaceRevisionView;
};

type ConflictView = {
  severity: string;
  summary: string;
  anchors: AnchorRef[];
  blockingClaimIds: string[];
};

type BlockerView = {
  kind: string;
  summary: string;
  relatedTaskId?: string;
  relatedArtifactId?: string;
  riskScore?: number;
  validationChecks: string[];
};

type ArtifactView = {
  id: string;
  taskId: string;
  status: string;
  anchors: AnchorRef[];
  baseRevision: WorkspaceRevisionView;
  diffRef?: string;
  requiredValidations: string[];
  validatedChecks: string[];
  riskScore?: number;
};

type CuratorProposalView = {
  index: number;
  kind: string;
  disposition: "pending" | "applied" | "rejected";
  payload: unknown;
  decidedAt?: number;
  taskId?: string;
  note?: string;
  output?: string;
};

type CuratorJobView = {
  id: string;
  trigger: string;
  status: "queued" | "running" | "completed" | "failed" | "skipped";
  taskId?: string;
  focus: AnchorRef[];
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
  proposals: CuratorProposalView[];
  diagnostics: QueryDiagnostic[];
  error?: string;
};
```

## MCP Resources

Beyond `prism_query`, the MCP server exposes navigable `prism://...` resources.

- Static resources:
  - `prism://api-reference`
  - `prism://session`
  - `prism://entrypoints`
  - `prism://schemas`
- Parameterized resources:
  - `prism://schema/{resourceKind}`
  - `prism://search/{query}?limit={limit}&cursor={cursor}`
  - `prism://symbol/{crateName}/{kind}/{path}`
  - `prism://lineage/{lineageId}?limit={limit}&cursor={cursor}`
  - `prism://task/{taskId}?limit={limit}&cursor={cursor}`
  - `prism://event/{eventId}`
  - `prism://memory/{memoryId}`
  - `prism://edge/{edgeId}`

Every JSON resource payload now includes:

```ts
type ResourcePayloadBase = {
  uri: string;
  schemaUri: string;
  relatedResources: ResourceLink[];
};

type ResourceLink = {
  uri: string;
  name: string;
  description?: string;
};
```

Collection resources also expose cursor metadata:

```ts
type ResourcePage = {
  cursor?: string;
  nextCursor?: string;
  limit: number;
  returned: number;
  total: number;
  hasMore: boolean;
  limitCapped: boolean;
};
```

Clients should follow `schemaUri` and `relatedResources` instead of reconstructing adjacent URIs by hand.

## Limits and determinism

- Search results are capped at 500 nodes by default.
- Call graph depth is capped at 10 by default.
- Serialized query output is capped at 256 KiB per session.
- Results are deterministically ordered by Prism before they reach the JS layer.
- The graph and JS runtime both stay warm for the MCP session.

## Recipes

### 1. Find a symbol and show call graph plus lineage

```ts
const sym = prism.symbol("main");
return {
  symbol: sym,
  callGraph: sym?.callGraph(2),
  lineage: sym?.lineage(),
};
```

### 2. Search only functions

```ts
return prism.search("request", { limit: 5, kind: "function" });
```

### 3. Find callers of the best symbol match

```ts
const sym = prism.symbol("handle_request");
return {
  symbol: sym,
  callers: sym?.relations().callers ?? [],
};
```

### 4. Fall back from exact-ish lookup to search

```ts
const sym = prism.symbol("RequestContext") ?? prism.search("RequestContext", { limit: 1 })[0];
return sym;
```

### 5. Summarize entrypoints

```ts
return prism.entrypoints().map((sym) => ({
  path: sym.id.path,
  file: sym.filePath,
}));
```

### 6. Pull source plus relations in one round-trip

```ts
const sym = prism.symbol("main");
return {
  symbol: sym,
  source: sym?.full(),
  relations: sym?.relations(),
};
```

### 7. Inspect diagnostics after an ambiguous lookup

```ts
const sym = prism.symbol("parse");
return {
  symbol: sym,
  diagnostics: prism.diagnostics(),
};
```

### 8. Narrow by path fragment

```ts
return prism.search("config", {
  kind: "struct",
  path: "src/settings",
  limit: 10,
});
```

### 9. Compare two related symbols

```ts
const left = prism.symbol("handle_request");
const right = prism.symbol("handle_response");
return {
  left,
  right,
  sharedCallers:
    left && right
      ? left
          .relations()
          .callers
          .filter((caller) =>
            right.relations().callers.some((other) => other.id.path === caller.id.path)
          )
      : [],
};
```

### 10. Return both data and repair hints

```ts
const results = prism.search("parse", { limit: 1000 });
return {
  results,
  diagnostics: prism.diagnostics(),
};
```

### 11. Ask Prism for semantic blast radius directly

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.blastRadius(sym) : null;
```

### 12. Pull prior failures without reconstructing anchors manually

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.relatedFailures(sym) : [];
```

### 13. Ask for explicit co-change neighbors

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.coChangeNeighbors(sym) : [];
```

### 14. Ask for a validation recipe instead of rebuilding one in the snippet

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.validationRecipe(sym) : null;
```

### 15. Recall session memory for a symbol

```ts
const sym = prism.symbol("handle_request");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "regression",
  limit: 5,
});
```

### 16. Inspect recent curator proposals through `prism_query`

```ts
return prism.curator.jobs({ status: "completed", limit: 5 }).map((job) => ({
  id: job.id,
  trigger: job.trigger,
  proposals: job.proposals.map((proposal) => ({
    kind: proposal.kind,
    disposition: proposal.disposition,
  })),
}));
```

### 17. Fetch one curator job and keep only pending inferred-edge proposals

```ts
const job = prism.curator.job("curator:1");
return job?.proposals.filter(
  (proposal) => proposal.kind === "inferred_edge" && proposal.disposition === "pending"
);
```

### 18. See who is already working in an area

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.claims(sym) : [];
```

### 19. Ask PRISM for blockers on a coordination task

```ts
return prism.blockers("coord-task:12");
```

### 20. Simulate an edit claim before taking it

```ts
const sym = prism.symbol("handle_request");
return prism.simulateClaim({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

### 21. Pull a coordination inbox for one plan

```ts
return prism.coordinationInbox("plan:12");
```

### 22. Pull the full working context for one coordination task

```ts
return prism.taskContext("coord-task:12");
```

### 23. Preview a claim and tell whether it is blocked

```ts
const sym = prism.symbol("handle_request");
return prism.claimPreview({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

## Current implementation surface

- Available now: symbol lookup, search, entrypoints, relations, call graphs, source extraction, lineage history, related failures, blast radius, and task replay by id.
- Available now: session/workspace memory recall for notes and promoted curator memories.
- Available now: workspace-backed curator job inspection through `prism.curator.jobs()` and `prism.curator.job()`.
- Available now: coordination plans, tasks, claims, conflicts, blockers, review queues, claim simulation, and workflow helpers for inbox/task/claim preview.
- Keep query logic small. If you find yourself reconstructing semantics from raw low-level fields every time, that method probably belongs in Prism itself.

## Separate mutation tools

The query runtime is read-only. State changes happen through separate MCP tools:

- `prism_start_task`
- `prism_outcome`
- `prism_note`
- `prism_infer_edge`
- `prism_coordination`
- `prism_claim`
- `prism_artifact`
- `prism_curator_promote_edge`
- `prism_curator_promote_memory`
- `prism_curator_reject_proposal`
- `prism_test_ran`
- `prism_failure_observed`
- `prism_fix_validated`

Convenience query tools:

- `prism_symbol`
- `prism_search`

Patch observation is automatic. PRISM records file changes from `ObservedChangeSet` without requiring an explicit MCP call.
"#
}

pub fn runtime_prelude() -> &'static str {
    r#""use strict";

function __prismDecode(raw) {
  const envelope = JSON.parse(raw);
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.value;
}

function __prismHost(operation, args) {
  const payload = args === undefined ? "{}" : JSON.stringify(args);
  return __prismDecode(__prismHostCall(operation, payload));
}

function __prismNormalizeTarget(target) {
  if (target == null) {
    return null;
  }
  if (typeof target === "object" && target.id != null) {
    return target.id;
  }
  return target;
}

function __prismEnrichSymbol(raw) {
  if (raw == null) {
    return null;
  }

  return {
    ...raw,
    full() {
      return __prismHost("full", { id: this.id });
    },
    relations() {
      return __prismEnrichRelations(__prismHost("relations", { id: this.id }));
    },
    callGraph(depth = 3) {
      return __prismEnrichSubgraph(__prismHost("callGraph", { id: this.id, depth }));
    },
    lineage() {
      return __prismEnrichLineage(__prismHost("lineage", { id: this.id }));
    },
  };
}

function __prismEnrichSymbols(values) {
  return Array.isArray(values) ? values.map(__prismEnrichSymbol) : [];
}

function __prismEnrichRelations(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    contains: __prismEnrichSymbols(raw.contains),
    callers: __prismEnrichSymbols(raw.callers),
    callees: __prismEnrichSymbols(raw.callees),
    references: __prismEnrichSymbols(raw.references),
    imports: __prismEnrichSymbols(raw.imports),
    implements: __prismEnrichSymbols(raw.implements),
    specifies: __prismEnrichSymbols(raw.specifies),
    specifiedBy: __prismEnrichSymbols(raw.specifiedBy),
    validates: __prismEnrichSymbols(raw.validates),
    validatedBy: __prismEnrichSymbols(raw.validatedBy),
    related: __prismEnrichSymbols(raw.related),
    relatedBy: __prismEnrichSymbols(raw.relatedBy),
  };
}

function __prismEnrichSubgraph(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    nodes: __prismEnrichSymbols(raw.nodes),
  };
}

function __prismEnrichLineage(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    current: __prismEnrichSymbol(raw.current),
  };
}

function __prismNormalizeFocus(values) {
  if (!Array.isArray(values)) {
    return [];
  }
  return values
    .map(__prismNormalizeTarget)
    .filter((value) => value != null);
}

function __prismNormalizeAnchor(value) {
  if (value == null) {
    return null;
  }
  if (typeof value === "object" && value.id != null) {
    return {
      type: "node",
      crateName: value.id.crateName,
      path: value.id.path,
      kind: value.id.kind,
    };
  }
  if (typeof value === "object" && value.crateName != null && value.path != null) {
    return {
      type: "node",
      crateName: value.crateName,
      path: value.path,
      kind: value.kind,
    };
  }
  if (typeof value === "object" && value.Node != null) {
    const node = value.Node;
    return {
      type: "node",
      crateName: node.crateName ?? node.crate_name,
      path: node.path,
      kind: node.kind,
    };
  }
  if (typeof value === "object" && value.Lineage != null) {
    return {
      type: "lineage",
      lineageId: value.Lineage.lineageId ?? value.Lineage.lineage_id ?? value.Lineage,
    };
  }
  if (typeof value === "object" && value.File != null) {
    return {
      type: "file",
      fileId: value.File.fileId ?? value.File.file_id ?? value.File,
    };
  }
  if (typeof value === "object" && value.Kind != null) {
    return { type: "kind", kind: value.Kind.kind ?? value.Kind };
  }
  if (typeof value === "object" && typeof value.type === "string") {
    return value;
  }
  return null;
}

function __prismNormalizeAnchors(values) {
  const list = Array.isArray(values) ? values : [values];
  return list.map(__prismNormalizeAnchor).filter((value) => value != null);
}

function __prismCleanupGlobals() {
  for (const name of Object.getOwnPropertyNames(globalThis)) {
    if (__prismBaselineGlobals.includes(name)) {
      continue;
    }
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
    if (!descriptor || descriptor.configurable) {
      delete globalThis[name];
    }
  }
}

globalThis.prism = Object.freeze({
  symbol(query) {
    return __prismEnrichSymbol(__prismHost("symbol", { query }));
  },
  symbols(query) {
    return __prismEnrichSymbols(__prismHost("symbols", { query }));
  },
  search(query, options = {}) {
    return __prismEnrichSymbols(
      __prismHost("search", Object.assign({ query }, options))
    );
  },
  entrypoints() {
    return __prismEnrichSymbols(__prismHost("entrypoints", {}));
  },
  plan(planId) {
    return __prismHost("plan", { planId });
  },
  task(taskId) {
    return __prismHost("coordinationTask", { taskId });
  },
  readyTasks(planId) {
    return __prismHost("readyTasks", { planId });
  },
  claims(target) {
    return __prismHost("claims", { anchors: __prismNormalizeAnchors(target) });
  },
  conflicts(target) {
    return __prismHost("conflicts", { anchors: __prismNormalizeAnchors(target) });
  },
  blockers(taskId) {
    return __prismHost("blockers", { taskId });
  },
  pendingReviews(planId) {
    return __prismHost("pendingReviews", planId == null ? {} : { planId });
  },
  artifacts(taskId) {
    return __prismHost("artifacts", { taskId });
  },
  taskBlastRadius(taskId) {
    return __prismHost("taskBlastRadius", { taskId });
  },
  taskValidationRecipe(taskId) {
    return __prismHost("taskValidationRecipe", { taskId });
  },
  taskRisk(taskId) {
    return __prismHost("taskRisk", { taskId });
  },
  artifactRisk(artifactId) {
    return __prismHost("artifactRisk", { artifactId });
  },
  taskIntent(taskId) {
    return __prismHost("taskIntent", { taskId });
  },
  coordinationInbox(planId) {
    return {
      readyTasks: prism.readyTasks(planId),
      pendingReviews: prism.pendingReviews(planId),
    };
  },
  taskContext(taskId) {
    const task = prism.task(taskId);
    const target = task?.anchors ?? [];
    return {
      task,
      blockers: prism.blockers(taskId),
      artifacts: prism.artifacts(taskId),
      claims: target.length > 0 ? prism.claims(target) : [],
      conflicts: target.length > 0 ? prism.conflicts(target) : [],
      blastRadius: prism.taskBlastRadius(taskId),
      validationRecipe: prism.taskValidationRecipe(taskId),
      risk: prism.taskRisk(taskId),
    };
  },
  claimPreview(input) {
    const conflicts = prism.simulateClaim(input);
    return {
      conflicts,
      blocked: conflicts.some((conflict) => conflict.severity === "Block"),
      warnings: conflicts.filter((conflict) => conflict.severity !== "Info"),
    };
  },
  simulateClaim(input) {
    return __prismHost("simulateClaim", {
      anchors: __prismNormalizeAnchors(input?.anchors ?? input?.anchor ?? []),
      capability: input?.capability,
      mode: input?.mode,
      taskId: input?.taskId ?? input?.task_id,
    });
  },
  lineage(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichLineage(__prismHost("lineage", { id }));
  },
  coChangeNeighbors(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismHost("coChangeNeighbors", { id });
  },
  relatedFailures(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismHost("relatedFailures", { id });
  },
  blastRadius(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismHost("blastRadius", { id });
  },
  validationRecipe(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismHost("validationRecipe", { id });
  },
  specFor(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("specFor", { id }));
  },
  implementationFor(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("implementationFor", { id }));
  },
  driftCandidates(limit) {
    return __prismHost("driftCandidates", limit == null ? {} : { limit });
  },
  resumeTask(taskId) {
    return __prismHost("resumeTask", { taskId });
  },
  memory: Object.freeze({
    recall(options = {}) {
      return __prismHost("memoryRecall", {
        focus: __prismNormalizeFocus(options.focus),
        text: options.text,
        limit: options.limit,
      });
    },
  }),
  curator: Object.freeze({
    jobs(options = {}) {
      return __prismHost("curatorJobs", options);
    },
    job(id) {
      if (typeof id !== "string" || id.length === 0) {
        return null;
      }
      return __prismHost("curatorJob", { job_id: id });
    },
  }),
  diagnostics() {
    return __prismHost("diagnostics", {});
  },
});

const __prismBaselineGlobals = Object.getOwnPropertyNames(globalThis);
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_reference_mentions_primary_tool() {
        let docs = api_reference_markdown();
        assert!(docs.contains("prism_query"));
        assert!(docs.contains("type PrismApi"));
        assert!(
            docs.contains("### 12. Pull prior failures without reconstructing anchors manually")
        );
        assert!(docs.contains("coChangeNeighbors"));
        assert!(docs.contains("validationRecipe"));
        assert!(docs.contains("prism.memory.recall"));
        assert!(docs.contains("prism.curator.jobs"));
        assert!(docs.contains("prism_curator_promote_edge"));
        assert!(docs.contains("prism_curator_promote_memory"));
        assert!(docs.contains("prism_symbol"));
        assert!(docs.contains("prism_search"));
    }

    #[test]
    fn prelude_exposes_global_prism() {
        let prelude = runtime_prelude();
        assert!(prelude.contains("globalThis.prism"));
        assert!(prelude.contains("__prismHostCall"));
        assert!(prelude.contains("curator: Object.freeze"));
        assert!(prelude.contains("__prismCleanupGlobals"));
    }
}
