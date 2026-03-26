pub const API_REFERENCE_URI: &str = "prism://api-reference";

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
