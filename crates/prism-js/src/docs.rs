pub const API_REFERENCE_URI: &str = "prism://api-reference";

pub fn api_reference_markdown() -> &'static str {
    r#"# PRISM Query API

`prism_query` executes a TypeScript snippet against a live in-memory PRISM graph.

The MCP transport surface is intentionally narrow:

- `prism_query` for all reads
- `prism_session` for task and session-context mutations
- `prism_mutate` for all other state changes

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
  strategy?: "direct" | "behavioral";
  ownerKind?: "read" | "write" | "persist" | "test" | "all";
  includeInferred?: boolean;
};

type ImplementationOptions = {
  mode?: "direct" | "owners";
  ownerKind?: "read" | "write" | "persist" | "test" | "all";
};

type OwnerLookupOptions = {
  kind?: "read" | "write" | "persist" | "test" | "all";
  limit?: number;
};

type MemoryRecallOptions = {
  focus?: Array<SymbolView | NodeId>;
  text?: string;
  limit?: number;
  kinds?: string[];
  since?: number;
};

type MemoryOutcomeOptions = {
  focus?: Array<SymbolView | NodeId>;
  taskId?: string;
  kinds?: string[];
  result?: string;
  actor?: string;
  since?: number;
  limit?: number;
};

type TaskJournalOptions = {
  eventLimit?: number;
  memoryLimit?: number;
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
  policyViolations(input?: { planId?: string; taskId?: string; limit?: number }): PolicyViolationRecordView[];
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
  implementationFor(target: SymbolView | NodeId, options?: ImplementationOptions): SymbolView[];
  owners(target: SymbolView | NodeId, options?: OwnerLookupOptions): OwnerCandidateView[];
  driftCandidates(limit?: number): DriftCandidateView[];
  specCluster(target: SymbolView | NodeId): SpecImplementationClusterView | null;
  explainDrift(target: SymbolView | NodeId): SpecDriftExplanationView | null;
  resumeTask(taskId: string): TaskReplay;
  taskJournal(taskId: string, options?: TaskJournalOptions): TaskJournalView;
  memory: {
    recall(options?: MemoryRecallOptions): ScoredMemoryView[];
    outcomes(options?: MemoryOutcomeOptions): OutcomeEvent[];
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
  location?: SourceLocationView;
  language: string;
  lineageId?: string;
  sourceExcerpt?: SourceExcerptView;
  ownerHint?: OwnerHintView;
  full(): string;
  excerpt(options?: SourceExcerptOptions): SourceExcerptView | null;
  relations(): RelationsView;
  callGraph(depth?: number): Subgraph;
  lineage(): LineageView | null;
};

type OwnerHintView = {
  kind: string;
  score: number;
  matchedTerms: string[];
  why: string;
};

type SourceLocationView = {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type SourceExcerptOptions = {
  contextLines?: number;
  maxLines?: number;
  maxChars?: number;
};

type SourceExcerptView = {
  text: string;
  startLine: number;
  endLine: number;
  truncated: boolean;
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
    before: NodeId[];
    after: NodeId[];
    evidence: string[];
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

type OwnerCandidateView = {
  symbol: SymbolView;
  kind: string;
  score: number;
  matchedTerms: string[];
  why: string;
};

type SpecImplementationClusterView = {
  spec: SymbolView;
  notes: string[];
  implementations: SymbolView[];
  validations: SymbolView[];
  related: SymbolView[];
  readPath: OwnerCandidateView[];
  writePath: OwnerCandidateView[];
  persistencePath: OwnerCandidateView[];
  tests: OwnerCandidateView[];
};

type SpecDriftExplanationView = {
  spec: SymbolView;
  notes: string[];
  driftReasons: string[];
  expectations: string[];
  observations: string[];
  gaps: string[];
  nextReads: OwnerCandidateView[];
  cluster: SpecImplementationClusterView;
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

type TaskLifecycleSummaryView = {
  planCount: number;
  patchCount: number;
  buildCount: number;
  testCount: number;
  failureCount: number;
  validationCount: number;
  noteCount: number;
  startedAt?: number;
  lastUpdatedAt?: number;
  finalSummary?: string;
};

type TaskJournalView = {
  taskId: string;
  description?: string;
  tags: string[];
  disposition: string;
  active: boolean;
  anchors: AnchorRef[];
  summary: TaskLifecycleSummaryView;
  diagnostics: QueryDiagnostic[];
  relatedMemory: ScoredMemoryView[];
  recentEvents: OutcomeEvent[];
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
  meta?: {
    id: string;
    ts: number;
    actor: unknown;
    correlation?: string;
    causation?: string;
  };
  anchors?: AnchorRef[];
  summary: string;
  result: string;
  kind: string;
  evidence?: unknown[];
  metadata?: unknown;
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
  pendingHandoffTo?: string;
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
  overlapKinds: string[];
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

type PolicyViolationView = {
  code: string;
  summary: string;
  planId?: string;
  taskId?: string;
  claimId?: string;
  artifactId?: string;
  details: Record<string, unknown>;
};

type PolicyViolationRecordView = {
  eventId: string;
  ts: number;
  summary: string;
  planId?: string;
  taskId?: string;
  claimId?: string;
  artifactId?: string;
  violations: PolicyViolationView[];
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
  - `prism://tool-schemas`
- Parameterized resources:
  - `prism://schema/{resourceKind}`
  - `prism://schema/tool/{toolName}`
  - `prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}`
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
  excerpt: sym?.excerpt(),
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

### 15. Pull an implementation cluster for a spec heading

```ts
const spec = prism.search("Outcome Memory", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.specCluster(spec) : null;
```

### 16. Ask Prism to explain spec drift for one target

```ts
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.explainDrift(spec) : null;
```

### 17. Ask for owner-biased reads for one target

```ts
const sym = prism.symbol("memory_recall");
return sym ? prism.owners(sym, { kind: "read", limit: 5 }) : [];
```

### 18. Search with behavioral owner ranking instead of direct noun matches

```ts
return prism.search("memory recall", {
  strategy: "behavioral",
  ownerKind: "read",
  limit: 5,
});
```

### 19. Ask for implementation owners without changing direct implementationFor semantics

```ts
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.implementationFor(spec, { mode: "owners", ownerKind: "read" }) : [];
```

### 20. Recall session memory for a symbol

```ts
const sym = prism.symbol("handle_request");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "regression",
  limit: 5,
});
```

### 21. Query outcome history with filters

```ts
const sym = prism.symbol("handle_request");
return prism.memory.outcomes({
  focus: sym ? [sym] : [],
  kinds: ["failure"],
  result: "failure",
  since: 1700000000,
  limit: 10,
});
```

### 22. Inspect recent curator proposals through `prism_query`

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

### 23. Fetch one curator job and keep only pending inferred-edge proposals

```ts
const job = prism.curator.job("curator:1");
return job?.proposals.filter(
  (proposal) => proposal.kind === "inferred_edge" && proposal.disposition === "pending"
);
```

### 24. See who is already working in an area

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.claims(sym) : [];
```

### 25. Ask PRISM for blockers on a coordination task

```ts
return prism.blockers("coord-task:12");
```

### 26. Simulate an edit claim before taking it

```ts
const sym = prism.symbol("handle_request");
return prism.simulateClaim({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

### 27. Pull a coordination inbox for one plan

```ts
return prism.coordinationInbox("plan:12");
```

### 28. Pull the full working context for one coordination task

```ts
return prism.taskContext("coord-task:12");
```

### 29. Preview a claim and tell whether it is blocked

```ts
const sym = prism.symbol("handle_request");
return prism.claimPreview({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

## Current implementation surface

- Available now: symbol lookup, search, entrypoints, line-aware symbol locations, bounded source excerpts, source extraction, relations, call graphs, lineage history, related failures, blast radius, and task replay by id.
- Available now: owner-biased discovery helpers through `prism.owners(...)`, behavioral `prism.search(...)`, and `implementationFor(..., { mode: "owners" })` without changing the direct primitive semantics.
- Available now: spec-to-code clustering and drift explanations that group direct links with read/write/persistence/test owners for spec-like symbols.
- Available now: session/workspace memory recall for anchored memory entries, filtered outcome history, and promoted curator memories.
- Available now: workspace-backed curator job inspection through `prism.curator.jobs()` and `prism.curator.job()`.
- Available now: tool input schema resources through `prism://tool-schemas` and `prism://schema/tool/{toolName}` for direct MCP introspection.
- Available now: coordination plans, tasks, claims, conflicts, blockers, review queues, claim simulation, and workflow helpers for inbox/task/claim preview.
- Keep query logic small. If you find yourself reconstructing semantics from raw low-level fields every time, that method probably belongs in Prism itself.

## Separate mutation tools

The query runtime is read-only. State changes happen through two coarse MCP mutation tools:

- `prism_session`
  - action `start_task`
  - action `configure`
- `prism_mutate`
  - action `outcome`
  - action `memory`
  - action `infer_edge`
  - action `coordination`
  - action `claim`
  - action `artifact`
  - action `test_ran`
  - action `failure_observed`
  - action `fix_validated`
  - action `curator_promote_edge`
  - action `curator_promote_memory`
  - action `curator_reject_proposal`

Read current session state through `prism://session`.

Patch observation is automatic. PRISM records file changes from `ObservedChangeSet` without requiring an explicit MCP call.
"#
}
