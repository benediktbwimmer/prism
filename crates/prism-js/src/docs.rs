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
- Ordinary multi-statement snippets are supported, including top-level `await`.
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

Tool-level failures from `prism_query` now separate the main query failure classes:

- `query_parse_failed` for TypeScript parse/transpile errors
- `query_runtime_failed` for runtime exceptions from the snippet itself
- `query_result_not_serializable` when the final returned value cannot be JSON-serialized
- `query_result_decode_failed` when PRISM itself fails to decode the JS result envelope

When PRISM can map a failure back to the submitted snippet, the MCP error payload includes
`line`, `column`, and `nextAction`.

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
  module?: string;
  taskId?: string;
  pathMode?: "contains" | "exact";
  strategy?: "direct" | "behavioral";
  structuredPath?: string;
  topLevelOnly?: boolean;
  preferCallableCode?: boolean;
  preferEditableTargets?: boolean;
  preferBehavioralOwners?: boolean;
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

type NextReadsOptions = {
  limit?: number;
};

type WhereUsedOptions = {
  mode?: "direct" | "behavioral";
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

type QueryLogOptions = {
  limit?: number;
  since?: number;
  target?: string;
  operation?: string;
  taskId?: string;
  minDurationMs?: number;
};

type ValidationFeedbackOptions = {
  limit?: number;
  since?: number;
  taskId?: string;
  verdict?: string;
  category?: string;
  contains?: string;
  correctedManually?: boolean;
};

type RuntimeLogOptions = {
  limit?: number;
  level?: string;
  target?: string;
  contains?: string;
};

type RuntimeTimelineOptions = {
  limit?: number;
  contains?: string;
};

type ChangedFilesOptions = {
  since?: number;
  limit?: number;
  taskId?: string;
  path?: string;
};

type RecentPatchesOptions = {
  target?: SymbolView | NodeId;
  since?: number;
  limit?: number;
  taskId?: string;
  path?: string;
};

type DiffForOptions = {
  since?: number;
  limit?: number;
  taskId?: string;
};

type SearchBundleOptions = SearchOptions & {
  includeDiscovery?: boolean;
  suggestedReadLimit?: number;
};

type SymbolBundleOptions = {
  includeDiscovery?: boolean;
  suggestedReadLimit?: number;
};

type TextSearchBundleOptions = SearchTextOptions & {
  semanticQuery?: string;
  semanticLimit?: number;
  semanticKind?: string;
  ownerKind?: string;
  strategy?: "direct" | "behavioral";
  includeDiscovery?: boolean;
  includeInferred?: boolean;
  aroundBefore?: number;
  aroundAfter?: number;
  aroundMaxChars?: number;
  suggestedReadLimit?: number;
};

type TargetBundleOptions = DiffForOptions & {
  includeDiscovery?: boolean;
  suggestedReadLimit?: number;
};

type PrismApi = {
  symbol(query: string): SymbolView | null;
  symbolBundle(query: string, options?: SymbolBundleOptions): SymbolBundleView;
  symbols(query: string): SymbolView[];
  search(query: string, options?: SearchOptions): SymbolView[];
  searchText(query: string, options?: SearchTextOptions): TextSearchMatchView[];
  textSearchBundle(query: string, options?: TextSearchBundleOptions): TextSearchBundleView;
  tools(): ToolCatalogEntryView[];
  tool(name: string): ToolSchemaView | null;
  entrypoints(): SymbolView[];
  file(path: string): FileView;
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
  full(target: QueryTarget): string | null;
  excerpt(target: QueryTarget, options?: SourceExcerptOptions): SourceExcerptView | null;
  editSlice(target: QueryTarget, options?: EditSliceOptions): SourceSliceView | null;
  focusedBlock(target: QueryTarget, options?: EditSliceOptions): FocusedBlockView | null;
  lineage(target: QueryTarget): LineageView | null;
  coChangeNeighbors(target: QueryTarget): CoChangeView[];
  relatedFailures(target: QueryTarget): OutcomeEvent[];
  blastRadius(target: QueryTarget): ChangeImpactView | null;
  validationRecipe(target: QueryTarget): ValidationRecipeView | null;
  readContext(target: QueryTarget): ReadContextView | null;
  editContext(target: QueryTarget): EditContextView | null;
  validationContext(target: QueryTarget): ValidationContextView | null;
  recentChangeContext(target: QueryTarget): RecentChangeContextView | null;
  discovery(target: QueryTarget): DiscoveryBundleView | null;
  searchBundle(query: string, options?: SearchBundleOptions): SearchBundleView;
  targetBundle(target: QueryTarget | SearchBundleView | DiscoveryBundleView, options?: TargetBundleOptions): TargetBundleView | null;
  nextReads(target: QueryTarget, options?: NextReadsOptions): OwnerCandidateView[];
  whereUsed(target: QueryTarget, options?: WhereUsedOptions): SymbolView[];
  entrypointsFor(target: QueryTarget, options?: NextReadsOptions): SymbolView[];
  specFor(target: QueryTarget): SymbolView[];
  implementationFor(target: QueryTarget, options?: ImplementationOptions): SymbolView[];
  owners(target: QueryTarget, options?: OwnerLookupOptions): OwnerCandidateView[];
  driftCandidates(limit?: number): DriftCandidateView[];
  specCluster(target: QueryTarget): SpecImplementationClusterView | null;
  explainDrift(target: QueryTarget): SpecDriftExplanationView | null;
  resumeTask(taskId: string): TaskReplay;
  taskJournal(taskId: string, options?: TaskJournalOptions): TaskJournalView;
  changedFiles(options?: ChangedFilesOptions): ChangedFileView[];
  changedSymbols(path: string, options?: ChangedFilesOptions): ChangedSymbolView[];
  recentPatches(options?: RecentPatchesOptions): PatchEventView[];
  diffFor(target: QueryTarget, options?: DiffForOptions): DiffHunkView[];
  taskChanges(taskId: string, options?: ChangedFilesOptions): PatchEventView[];
  connectionInfo(): ConnectionInfoView;
  runtimeStatus(): RuntimeStatusView;
  runtimeLogs(options?: RuntimeLogOptions): RuntimeLogEventView[];
  runtimeTimeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];
  validationFeedback(options?: ValidationFeedbackOptions): ValidationFeedbackView[];
  connection: {
    info(): ConnectionInfoView;
  };
  runtime: {
    status(): RuntimeStatusView;
    logs(options?: RuntimeLogOptions): RuntimeLogEventView[];
    timeline(options?: RuntimeTimelineOptions): RuntimeLogEventView[];
  };
  memory: {
    recall(options?: MemoryRecallOptions): ScoredMemoryView[];
    outcomes(options?: MemoryOutcomeOptions): OutcomeEvent[];
  };
  curator: {
    jobs(options?: CuratorJobQueryOptions): CuratorJobView[];
    job(id: string): CuratorJobView | null;
  };
  queryLog(options?: QueryLogOptions): QueryLogEntryView[];
  slowQueries(options?: QueryLogOptions): QueryLogEntryView[];
  queryTrace(id: string): QueryTraceView | null;
  diagnostics(): QueryDiagnostic[];
};

type QueryTarget = SymbolView | NodeId | { lineageId: string };

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
  editSlice(options?: EditSliceOptions): SourceSliceView | null;
  relations(): RelationsView;
  callGraph(depth?: number): Subgraph;
  lineage(): LineageView | null;
};

type OwnerHintView = {
  kind: string;
  score: number;
  matchedTerms: string[];
  why: string;
  trustSignals: TrustSignalsView;
};

type ConfidenceLabel = "low" | "medium" | "high";

type EvidenceSourceKind = "direct_graph" | "inferred" | "memory" | "outcome";

type TrustSignalsView = {
  confidenceLabel: ConfidenceLabel;
  evidenceSources: EvidenceSourceKind[];
  why: string[];
};

type SourceLocationView = {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

type SearchTextOptions = {
  regex?: boolean;
  caseSensitive?: boolean;
  path?: string;
  glob?: string;
  limit?: number;
  contextLines?: number;
};

type FileReadOptions = {
  startLine?: number;
  endLine?: number;
  maxChars?: number;
};

type FileAroundOptions = {
  line: number;
  before?: number;
  after?: number;
  maxChars?: number;
};

type SourceExcerptOptions = {
  contextLines?: number;
  maxLines?: number;
  maxChars?: number;
};

type EditSliceOptions = {
  beforeLines?: number;
  afterLines?: number;
  maxLines?: number;
  maxChars?: number;
};

type SourceExcerptView = {
  text: string;
  startLine: number;
  endLine: number;
  truncated: boolean;
};

type SourceSliceView = {
  text: string;
  startLine: number;
  endLine: number;
  focus: SourceLocationView;
  relativeFocus: SourceLocationView;
  truncated: boolean;
};

type FocusedBlockView = {
  symbol: SymbolView;
  slice?: SourceSliceView;
  excerpt?: SourceExcerptView;
  strategy: string;
};

type ToolCatalogEntryView = {
  toolName: string;
  schemaUri: string;
  description: string;
  exampleInput: unknown;
};

type ToolFieldSchemaView = {
  name: string;
  required: boolean;
  description?: string;
  types: string[];
  enumValues: string[];
  nestedFields: ToolFieldSchemaView[];
  schema: unknown;
};

type ToolActionSchemaView = {
  action: string;
  requiredFields: string[];
  fields: ToolFieldSchemaView[];
  inputSchema: unknown;
  exampleInput?: unknown;
};

type ToolSchemaView = {
  toolName: string;
  schemaUri: string;
  description: string;
  exampleInput: unknown;
  inputSchema: unknown;
  actions: ToolActionSchemaView[];
};

type TextSearchMatchView = {
  path: string;
  location: SourceLocationView;
  excerpt: SourceExcerptView;
};

type ChangedSymbolView = {
  status: string;
  id?: NodeId;
  name: string;
  kind: string;
  filePath: string;
  location?: SourceLocationView;
  excerpt?: SourceExcerptView;
  lineageId?: string;
};

type ChangedFileView = {
  path: string;
  eventId: string;
  ts: number;
  taskId?: string;
  trigger?: string;
  summary: string;
  changedSymbolCount: number;
  addedCount: number;
  removedCount: number;
  updatedCount: number;
};

type PatchEventView = {
  eventId: string;
  ts: number;
  taskId?: string;
  trigger?: string;
  summary: string;
  files: string[];
  changedSymbols: ChangedSymbolView[];
};

type DiffHunkView = {
  eventId: string;
  ts: number;
  taskId?: string;
  trigger?: string;
  summary: string;
  symbol: ChangedSymbolView;
};

type RuntimeHealthView = {
  ok: boolean;
  detail: string;
};

type RuntimeProcessView = {
  pid: number;
  parentPid: number;
  rssKb: number;
  rssMb: number;
  elapsed: string;
  kind: string;
  command: string;
  healthPath?: string;
  bridgeState?: string;
};

type ConnectionInfoView = {
  root: string;
  mode: string;
  transport: string;
  uri?: string;
  uriFile: string;
  healthUri?: string;
  health: RuntimeHealthView;
  bridgeRole: string;
};

type RuntimeStatusView = {
  root: string;
  connection: ConnectionInfoView;
  uri?: string;
  uriFile: string;
  logPath: string;
  logBytes?: number;
  cachePath: string;
  cacheBytes?: number;
  healthPath: string;
  health: RuntimeHealthView;
  daemonCount: number;
  bridgeCount: number;
  connectedBridgeCount: number;
  idleBridgeCount: number;
  staleBridgeCount: number;
  orphanBridgeCount: number;
  processes: RuntimeProcessView[];
  processError?: string;
};

type RuntimeLogEventView = {
  timestamp?: string;
  level?: string;
  message: string;
  target?: string;
  file?: string;
  lineNumber?: number;
  fields?: Record<string, unknown>;
};

type FileView = {
  path: string;
  read(options?: FileReadOptions): SourceExcerptView;
  around(options: FileAroundOptions): SourceSliceView;
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
  summary: string;
  uncertainty: string[];
  history: Array<{
    eventId: string;
    ts: number;
    kind: string;
    confidence: number;
    before: NodeId[];
    after: NodeId[];
    evidence: string[];
    evidenceDetails: Array<{
      code: string;
      label: string;
      detail: string;
    }>;
    summary: string;
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

type SuggestedQueryView = {
  label: string;
  query: string;
  why: string;
};

type ReadContextView = {
  target: SymbolView;
  targetBlock: FocusedBlockView;
  directLinks: SymbolView[];
  directLinkBlocks: FocusedBlockView[];
  suggestedReads: OwnerCandidateView[];
  tests: OwnerCandidateView[];
  testBlocks: FocusedBlockView[];
  relatedMemory: ScoredMemoryView[];
  recentFailures: OutcomeEvent[];
  validationRecipe: ValidationRecipeView;
  why: string[];
  suggestedQueries: SuggestedQueryView[];
};

type EditContextView = {
  target: SymbolView;
  targetBlock: FocusedBlockView;
  directLinks: SymbolView[];
  directLinkBlocks: FocusedBlockView[];
  suggestedReads: OwnerCandidateView[];
  writePaths: OwnerCandidateView[];
  writePathBlocks: FocusedBlockView[];
  tests: OwnerCandidateView[];
  testBlocks: FocusedBlockView[];
  relatedMemory: ScoredMemoryView[];
  recentFailures: OutcomeEvent[];
  blastRadius: ChangeImpactView;
  validationRecipe: ValidationRecipeView;
  checklist: string[];
  suggestedQueries: SuggestedQueryView[];
};

type ValidationContextView = {
  target: SymbolView;
  targetBlock: FocusedBlockView;
  tests: OwnerCandidateView[];
  testBlocks: FocusedBlockView[];
  relatedMemory: ScoredMemoryView[];
  recentFailures: OutcomeEvent[];
  blastRadius: ChangeImpactView;
  validationRecipe: ValidationRecipeView;
  why: string[];
  suggestedQueries: SuggestedQueryView[];
};

type RecentChangeContextView = {
  target: SymbolView;
  recentEvents: OutcomeEvent[];
  recentFailures: OutcomeEvent[];
  coChangeNeighbors: CoChangeView[];
  relatedMemory: ScoredMemoryView[];
  promotedSummaries: string[];
  lineage: LineageView | null;
  why: string[];
  suggestedQueries: SuggestedQueryView[];
};

type DiscoveryBundleView = {
  target: SymbolView;
  suggestedReads: OwnerCandidateView[];
  readContext: ReadContextView;
  editContext: EditContextView;
  validationContext: ValidationContextView;
  recentChangeContext: RecentChangeContextView;
  entrypoints: SymbolView[];
  whereUsedDirect: SymbolView[];
  whereUsedBehavioral: SymbolView[];
  suggestedQueries: SuggestedQueryView[];
  relations: RelationsView;
  specCluster?: SpecImplementationClusterView;
  specDrift?: SpecDriftExplanationView;
  lineage?: LineageView;
  coChangeNeighbors: CoChangeView[];
  relatedFailures: OutcomeEvent[];
  blastRadius: ChangeImpactView;
  validationRecipe: ValidationRecipeView;
  trustSignals: TrustSignalsView;
  why: string[];
};

type BundleSummaryView = {
  kind: string;
  resultCount: number;
  empty: boolean;
  truncated: boolean;
  ambiguous: boolean;
  diagnosticCodes: string[];
};

type SearchBundleView = {
  query: string;
  results: SymbolView[];
  topResult?: SymbolView;
  discovery?: DiscoveryBundleView;
  focusedBlock?: FocusedBlockView;
  readContext?: ReadContextView;
  suggestedReads: OwnerCandidateView[];
  validationContext?: ValidationContextView;
  recentChangeContext?: RecentChangeContextView;
  summary: BundleSummaryView;
  diagnostics: QueryDiagnostic[];
};

type SymbolBundleView = {
  query: string;
  result?: SymbolView;
  candidates: SymbolView[];
  discovery?: DiscoveryBundleView;
  focusedBlock?: FocusedBlockView;
  readContext?: ReadContextView;
  suggestedReads: OwnerCandidateView[];
  summary: BundleSummaryView;
  diagnostics: QueryDiagnostic[];
};

type TextSearchBundleView = {
  query: string;
  matches: TextSearchMatchView[];
  topMatch?: TextSearchMatchView;
  rawContext?: SourceSliceView;
  semanticQuery?: string;
  semanticResults: SymbolView[];
  topSymbol?: SymbolView;
  discovery?: DiscoveryBundleView;
  focusedBlock?: FocusedBlockView;
  readContext?: ReadContextView;
  suggestedReads: OwnerCandidateView[];
  summary: BundleSummaryView;
  diagnostics: QueryDiagnostic[];
};

type TargetBundleView = {
  target: SymbolView;
  discovery?: DiscoveryBundleView;
  focusedBlock?: FocusedBlockView;
  diff: DiffHunkView[];
  editContext: EditContextView;
  readContext: ReadContextView;
  suggestedReads: OwnerCandidateView[];
  likelyTests: FocusedBlockView[];
  summary: BundleSummaryView;
  diagnostics: QueryDiagnostic[];
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
  trustSignals: TrustSignalsView;
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
  trustSignals: TrustSignalsView;
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

type QueryResultSummaryView = {
  kind: string;
  jsonBytes: number;
  itemCount?: number;
  truncated: boolean;
  outputCapHit: boolean;
  resultCapHit: boolean;
};

type QueryPhaseView = {
  operation: string;
  startedAt: number;
  durationMs: number;
  argsSummary?: unknown;
  touched: string[];
  success: boolean;
  error?: string;
};

type QueryLogEntryView = {
  id: string;
  kind: string;
  querySummary: string;
  queryText: string;
  startedAt: number;
  durationMs: number;
  sessionId: string;
  taskId?: string;
  success: boolean;
  error?: string;
  operations: string[];
  touched: string[];
  diagnostics: QueryDiagnostic[];
  result: QueryResultSummaryView;
};

type QueryTraceView = {
  entry: QueryLogEntryView;
  phases: QueryPhaseView[];
};

type ValidationFeedbackView = {
  id: string;
  recordedAt: number;
  taskId?: string;
  context: string;
  anchors: AnchorRef[];
  prismSaid: string;
  actuallyTrue: string;
  category: string;
  verdict: string;
  correctedManually: boolean;
  correction?: string;
  metadata: unknown;
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
  - `prism://capabilities`
  - `prism://session`
  - `prism://entrypoints`
  - `prism://schemas`
  - `prism://tool-schemas`
- Parameterized resources:
  - `prism://schema/{resourceKind}`
  - `prism://schema/tool/{toolName}`
  - `prism://search/{query}?limit={limit}&cursor={cursor}&strategy={strategy}&ownerKind={ownerKind}&kind={kind}&path={path}&module={module}&taskId={taskId}&pathMode={pathMode}&structuredPath={structuredPath}&topLevelOnly={topLevelOnly}&preferCallableCode={preferCallableCode}&preferEditableTargets={preferEditableTargets}&preferBehavioralOwners={preferBehavioralOwners}&includeInferred={includeInferred}`
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

### 4a. Use async-style composition when it keeps the query clearer

```ts
const results = await prism.search("handle_request", { limit: 3, kind: "function" });
const target = await prism.symbol("handle_request");
return {
  top: results[0],
  exact: target,
};
```

### 4b. If the query accidentally forgets its final return, PRISM warns instead of silently hiding it

```ts
const sym = prism.symbol("handle_request");
```

This returns `null` plus a `query_return_missing` diagnostic telling you to add a final
`return ...` if you intended the query to produce a result.

### 5. Summarize entrypoints

```ts
return prism.entrypoints().map((sym) => ({
  path: sym.id.path,
  file: sym.filePath,
}));
```

### 5a. Search raw workspace text with path filters

```ts
return prism.searchText("read context", {
  path: "src/recall.rs",
  limit: 3,
});
```

### 5b. Inspect tool payload requirements without leaving `prism_query`

```ts
const mutate = prism.tool("prism_mutate");
return mutate?.actions.find((action) => action.action === "validation_feedback");
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

### 6a. Ask for an edit-oriented slice with exact focus mapping

```ts
const sym = prism.symbol("handle_request");
return sym?.editSlice({
  beforeLines: 1,
  afterLines: 1,
  maxLines: 8,
});
```

### 6b. Ask for a focused local block with edit-slice fallback

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.focusedBlock(sym, { maxLines: 10, maxChars: 400 }) : null;
```

### 6c. Read an exact workspace file slice by path

```ts
return prism.file("src/main.rs").read({
  startLine: 10,
  endLine: 18,
});
```

### 6d. Read a bounded file slice around one line

```ts
return prism.file("src/main.rs").around({
  line: 14,
  before: 2,
  after: 2,
});
```

### 7. Inspect diagnostics after an ambiguous lookup

```ts
const sym = prism.symbol("parse");
return {
  symbol: sym,
  diagnostics: prism.diagnostics(),
};
```

### 7a. Inspect recent query behavior through PRISM itself

```ts
const recent = prism.queryLog({ limit: 5 });
const trace = recent[0] ? prism.queryTrace(recent[0].id) : null;
return {
  recent,
  trace,
};
```

### 7b. Inspect semantic recent changes without defaulting to git diff

```ts
return {
  files: prism.changedFiles({ limit: 5 }),
  patches: prism.recentPatches({ path: "crates/prism-mcp/src", limit: 3 }),
};
```

### 7c. Inspect semantic recent changes for one task or file

```ts
return {
  task: prism.taskChanges("task:123", { limit: 3 }),
  file: prism.changedSymbols("crates/prism-mcp/src/query_runtime.rs", { limit: 10 }),
};
```

### 7d. Inspect exact changed hunks for one semantic target

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.diffFor(sym, { limit: 5 }) : [];
```

### 7e. Inspect daemon status and recent runtime activity through PRISM

```ts
return {
  status: prism.runtime.status(),
  timeline: prism.runtime.timeline({ limit: 5 }),
  warnings: prism.runtime.logs({ level: "WARN", limit: 5 }),
};
```

### 7f. Inspect validation feedback recorded while dogfooding PRISM

```ts
return prism.validationFeedback({
  limit: 5,
  category: "projection",
  contains: "session",
});
```

### 8. Narrow by path fragment, module, or task context

```ts
return prism.search("config", {
  kind: "struct",
  path: "src/settings",
  module: "demo::settings",
  taskId: "coord-task:12",
  limit: 10,
});
```

### 8a. Search structured Cargo.toml keys without leaving PRISM

```ts
return prism.search("members", {
  path: "Cargo.toml",
  kind: "toml-key",
  limit: 5,
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

### 15. Pull a semantic read context before editing

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.readContext(sym) : null;
```

### 16. Pull an edit context with write paths and validations

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.editContext(sym) : null;
```

### 17. Pull a validation-focused semantic bundle

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.validationContext(sym) : null;
```

### 18. Pull recent changes, failures, and lineage context together

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.recentChangeContext(sym) : null;
```

### 19. Pull an implementation cluster for a spec heading

```ts
const spec = prism.search("Outcome Memory", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.specCluster(spec) : null;
```

### 20. Ask Prism to explain spec drift for one target

```ts
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.explainDrift(spec) : null;
```

### 21. Ask for owner-biased reads for one target

```ts
const sym = prism.symbol("memory_recall");
return sym ? prism.owners(sym, { kind: "read", limit: 5 }) : [];
```

### 22. Search with behavioral owner ranking instead of direct noun matches

```ts
return prism.search("memory recall", {
  strategy: "behavioral",
  ownerKind: "read",
  limit: 5,
});
```

### 23. Ask for implementation owners without changing direct implementationFor semantics

```ts
const spec = prism.search("Integration Points", {
  path: "docs/SPEC.md",
  kind: "markdown-heading",
  limit: 1,
})[0];
return spec ? prism.implementationFor(spec, { mode: "owners", ownerKind: "read" }) : [];
```

### 24. Ask for next reads directly instead of reconstructing owner lookups

```ts
const sym = prism.symbol("memory_recall");
return sym ? prism.nextReads(sym, { limit: 5 }) : [];
```

### 25. Ask where a target is used with either direct or behavioral semantics

```ts
const sym = prism.symbol("beta");
return sym
  ? {
      direct: prism.whereUsed(sym, { mode: "direct", limit: 5 }),
      behavioral: prism.whereUsed(sym, { mode: "behavioral", limit: 5 }),
    }
  : null;
```

### 26. Find entrypoints that reach a target

```ts
const sym = prism.symbol("beta");
return sym ? prism.entrypointsFor(sym, { limit: 5 }) : [];
```

### 27. Recall session memory for a symbol

```ts
const sym = prism.symbol("handle_request");
return prism.memory.recall({
  focus: sym ? [sym] : [],
  text: "regression",
  limit: 5,
});
```

### 28. Query outcome history with filters

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

### 29. Inspect recent curator proposals through `prism_query`

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

### 30. Fetch one curator job and keep only pending inferred-edge proposals

```ts
const job = prism.curator.job("curator:1");
return job?.proposals.filter(
  (proposal) => proposal.kind === "inferred_edge" && proposal.disposition === "pending"
);
```

### 31. See who is already working in an area

```ts
const sym = prism.symbol("handle_request");
return sym ? prism.claims(sym) : [];
```

### 32. Ask PRISM for blockers on a coordination task

```ts
return prism.blockers("coord-task:12");
```

### 33. Simulate an edit claim before taking it

```ts
const sym = prism.symbol("handle_request");
return prism.simulateClaim({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

### 34. Pull a coordination inbox for one plan

```ts
return prism.coordinationInbox("plan:12");
```

### 35. Pull the full working context for one coordination task

```ts
return prism.taskContext("coord-task:12");
```

### 36. Collapse target discovery into one helper

```ts
const search = prism.searchBundle("handle_request", { limit: 1 });
return prism.targetBundle(search);
```

### 37. Collapse direct symbol lookup into one consistent envelope

```ts
return prism.symbolBundle("handle_request", { includeDiscovery: true });
```

### 38. Collapse search plus top-target context into one helper

```ts
return prism.searchBundle("helper", { limit: 5 });
```

### 39. Opt into the slower full discovery bundle only when you need it

```ts
const search = prism.searchBundle("helper", {
  limit: 5,
  includeDiscovery: true,
});
return prism.targetBundle(search, { includeDiscovery: true, limit: 3 });
```

### 40. Collapse text search, raw file context, and semantic owner lookup into one helper

```ts
return prism.textSearchBundle("query_return_missing", {
  path: "crates/prism-mcp/src",
  semanticLimit: 3,
  aroundBefore: 2,
  aroundAfter: 8,
});
```

### 41. Use regex text search but still ask for semantic context with a separate query string

```ts
return prism.textSearchBundle("query_[a-z_]+", {
  regex: true,
  path: "crates/prism-mcp/src",
  semanticQuery: "query_return_missing",
  semanticLimit: 3,
});
```

### 42. Inspect the bundle summary flags directly

```ts
const bundle = prism.searchBundle("helper", { limit: 5 });
return {
  count: bundle.summary.resultCount,
  ambiguous: bundle.summary.ambiguous,
  truncated: bundle.summary.truncated,
  diagnosticCodes: bundle.summary.diagnosticCodes,
};
```

### 43. Preview a claim and tell whether it is blocked

```ts
const sym = prism.symbol("handle_request");
return prism.claimPreview({
  anchors: sym ? [sym] : [],
  capability: "Edit",
  mode: "SoftExclusive",
});
```

## Current implementation surface

- Available now: symbol lookup, search, entrypoints, line-aware symbol locations, bounded source excerpts, focused local block retrieval, source extraction, relations, call graphs, lineage history, related failures, blast radius, and task replay by id.
- Available now: owner-biased discovery helpers through `prism.owners(...)`, `prism.nextReads(...)`, `prism.whereUsed(...)`, `prism.entrypointsFor(...)`, behavioral `prism.search(...)`, `prism.readContext(...)`, `prism.editContext(...)`, `prism.validationContext(...)`, `prism.recentChangeContext(...)`, and `implementationFor(..., { mode: "owners" })` without changing the direct primitive semantics.
- Available now: consistent eager bundle helpers through `prism.symbolBundle(...)`, `prism.searchBundle(...)`, `prism.textSearchBundle(...)`, and `prism.targetBundle(...)` with stable `summary`, `diagnostics`, and `suggestedReads` fields.
- Available now: bounded workspace file reads through `prism.file(path).read(...)` and `prism.file(path).around(...)` for exact line-range and around-line inspection without leaving the PRISM query surface.
- Available now: bounded workspace text search through `prism.searchText(...)` with regex support, path/glob filters, exact match locations, and capped snippets, plus `prism.textSearchBundle(...)` to collapse text matches, one raw file window, and nearby semantic context into one helper.
- Available now: semantic recent-change inspection through `prism.changedFiles(...)`, `prism.changedSymbols(path, ...)`, `prism.recentPatches(...)`, `prism.diffFor(target, ...)`, and `prism.taskChanges(taskId, ...)` backed by recorded patch outcomes instead of raw diff dumps.
- Available now: direct daemon connection discovery through `prism.connectionInfo()` plus workspace-backed runtime introspection through `prism.runtimeStatus()`, `prism.runtimeLogs(...)`, and `prism.runtimeTimeline(...)` for daemon health, recent structured log events, startup/refresh diagnosis, and bridge upstream-resolution / connect latency without defaulting to shell status checks.
- Available now: a namespaced runtime alias through `prism.runtime.status()`, `prism.runtime.logs(...)`, and `prism.runtime.timeline(...)` so query authors do not have to remember only the flat runtime helpers.
- Available now: non-symbol repo coverage for markdown headings plus structured JSON, YAML, and TOML config keys through the normal PRISM search and relation surface.
- Available now: ambiguity-aware search narrowing through `path`, `module`, `taskId`, behavioral owner hints, and exact focused-block follow-ups surfaced directly from diagnostics and search resources.
- Available now: a first-class query log through `prism.queryLog(...)`, `prism.slowQueries(...)`, and `prism.queryTrace(id)` with duration, diagnostics, truncation metadata, and phase breakdowns.
- Available now: spec-to-code clustering and drift explanations that group direct links with read/write/persistence/test owners for spec-like symbols.
- Available now: session/workspace memory recall for anchored memory entries, filtered outcome history, and promoted curator memories.
- Available now: workspace-backed curator job inspection through `prism.curator.jobs()` and `prism.curator.job()`.
- Available now: a canonical capabilities resource at `prism://capabilities` plus tool input schema resources through `prism://tool-schemas` and `prism://schema/tool/{toolName}` for direct MCP introspection.
- Available now: coordination plans, tasks, claims, conflicts, blockers, review queues, claim simulation, and workflow helpers for inbox/task/claim preview.
- Keep query logic small. If you find yourself reconstructing semantics from raw low-level fields every time, that method probably belongs in Prism itself.

## Separate mutation tools

The query runtime is read-only. State changes happen through two coarse MCP mutation tools:

- `prism_session`
  - action `start_task`
  - action `configure`
  - shorthand `{ action, ...fields }` is accepted in addition to the canonical `{ action, input: { ... } }`
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
  - shorthand `{ action, ...fields }` is accepted in addition to the canonical `{ action, input: { ... } }`

Read current session state through `prism://session`.

Patch observation is automatic. PRISM records file changes from `ObservedChangeSet` without requiring an explicit MCP call.
"#
}
