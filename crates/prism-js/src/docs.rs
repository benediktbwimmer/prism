use std::sync::OnceLock;

use crate::prism_api_declaration_block;

pub const API_REFERENCE_URI: &str = "prism://api-reference";

pub fn api_reference_markdown() -> &'static str {
    static DOCS: OnceLock<String> = OnceLock::new();
    DOCS.get_or_init(|| {
        API_REFERENCE_TEMPLATE.replace(
            "__PRISM_API_DECLARATION_BLOCK__",
            prism_api_declaration_block(),
        )
    })
}

const API_REFERENCE_TEMPLATE: &str = r#"# PRISM Agent API

PRISM exposes a compact staged agent ABI over the existing semantic/query engine.

Target default agent path:

- `prism_locate`
- `prism_gather` for exact text/config/schema slices when a symbol handle is not the right first hop
- `prism_open`
- `prism_workset`
- `prism_expand`
- `prism_concept` for repo-native concept packets and decode lenses
- `prism_task_brief` for compact coordination-task reads when you already have a task id
- `prism_concept` for broad repo concepts when symbol-like first hops are the wrong unit

`prism_workset` is the comparison baseline for future agent-facing views. A useful workset should already give you:

- a primary target
- 1 to 3 supporting reads biased toward same-file and direct-graph follow-through
- likely tests when the repo exposes them
- `why`, `nextAction`, and `suggestedActions` that let you continue without reconstructing context manually

Use `prism_workset` after `prism_locate` and `prism_open` when you want the next bounded reads or validations before inventing a dedicated view.
- `prism_query` only when the compact surface cannot express the need

Compact-tool note:

- the compact staged tools are available as top-level MCP tools
- the rich semantic escape hatch is still `prism_query`
- this reference documents that rich query surface honestly so agents still have the escape hatch when they need it

The MCP transport surface currently includes:

- `prism_query` as the rich semantic read surface and escape hatch
- `prism_session` for task and session-context mutations
- `prism_mutate` for all other state changes

## Mental model

Treat the current query surface as the rich semantic escape hatch, not the long-term default first
hop.

- TypeScript is for composition.
- Prism is where semantic meaning should live.
- Return the final value with `return ...`.
- Ordinary multi-statement snippets are supported, including top-level `await`.
- The returned value must be JSON-serializable.
- `language` currently supports only `"ts"`.
- `prism_query` is read-only in this implementation.

Design principle for the future compact ABI:

- return the minimum sufficient answer for the next likely agent action

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
- `query_typecheck_failed` for pre-execution PRISM API shape errors on the stable `prism.*` surface
- `query_runtime_failed` for runtime exceptions from the snippet itself
- `query_result_not_serializable` when the final returned value cannot be JSON-serialized
- `query_result_decode_failed` when PRISM itself fails to decode the JS result envelope

When PRISM can map a failure back to the submitted snippet, the MCP error payload includes
`line`, `column`, and `nextAction`.

For stable `prism.*` helpers, PRISM now preflights common API mistakes before execution,
including misspelled helpers, wrong record shapes, and misspelled option keys, and includes
repair data such as `didYouMean` when it can confidently suggest the intended key.

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

type ConceptQueryOptions = {
  limit?: number;
  verbosity?: "summary" | "standard" | "full";
  includeBindingMetadata?: boolean;
};

type ConceptPacketTruncationView = {
  coreMembersOmitted: number;
  supportingMembersOmitted: number;
  likelyTestsOmitted: number;
  evidenceOmitted: number;
  relationsOmitted: number;
  relationEvidenceOmitted: number;
};

type ConceptCurationHintsView = {
  inspectFirst?: NodeId;
  supportingRead?: NodeId;
  likelyTest?: NodeId;
  nextAction: string;
};

type ConceptResolutionView = {
  score: number;
  reasons: string[];
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

type MemoryEventOptions = {
  memoryId?: string;
  focus?: Array<SymbolView | NodeId>;
  text?: string;
  limit?: number;
  kinds?: string[];
  actions?: string[];
  scope?: string;
  taskId?: string;
  since?: number;
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

type CuratorProposalQueryOptions = {
  status?: string;
  trigger?: string;
  kind?: string;
  disposition?: string;
  taskId?: string;
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

type McpLogOptions = {
  limit?: number;
  since?: number;
  callType?: string;
  name?: string;
  taskId?: string;
  sessionId?: string;
  success?: boolean;
  minDurationMs?: number;
  contains?: string;
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

type PlanListOptions = {
  status?: string;
  scope?: string;
  contains?: string;
  limit?: number;
};

type ContractListOptions = {
  status?: "candidate" | "active" | "deprecated" | "retired";
  scope?: "local" | "session" | "repo";
  contains?: string;
  kind?:
    | "interface"
    | "behavioral"
    | "data_shape"
    | "dependency_boundary"
    | "lifecycle"
    | "protocol"
    | "operational";
  limit?: number;
};

__PRISM_API_DECLARATION_BLOCK__

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

type ToolPayloadVariantSchemaView = {
  tag: string;
  schemaUri: string;
  requiredFields: string[];
  fields: ToolFieldSchemaView[];
  schema: unknown;
  exampleInput?: unknown;
  exampleInputs: unknown[];
};

type ToolActionSchemaView = {
  action: string;
  schemaUri: string;
  description?: string;
  requiredFields: string[];
  fields: ToolFieldSchemaView[];
  inputSchema: unknown;
  exampleInput?: unknown;
  exampleInputs: unknown[];
  payloadDiscriminator?: string;
  payloadVariants: ToolPayloadVariantSchemaView[];
};

type ToolSchemaView = {
  toolName: string;
  schemaUri: string;
  description: string;
  exampleInput: unknown;
  exampleInputs: unknown[];
  inputSchema: unknown;
  actions: ToolActionSchemaView[];
};

type ToolValidationIssueView = {
  code: string;
  path?: string;
  summary: string;
  allowedValues: string[];
  requiredFields: string[];
};

type ToolInputValidationView = {
  toolName: string;
  schemaUri: string;
  valid: boolean;
  normalizedInput: unknown;
  action?: string;
  actionSchemaUri?: string;
  summary: string;
  issues: ToolValidationIssueView[];
  exampleInputs: unknown[];
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

type RuntimeMaterializationItemView = {
  status: string;
  depth: string;
  loadedRevision: number;
  currentRevision?: number;
  coverage?: RuntimeMaterializationCoverageView;
  boundaries?: RuntimeBoundaryRegionView[];
};

type RuntimeMaterializationCoverageView = {
  knownFiles: number;
  knownDirectories: number;
  materializedFiles: number;
  materializedNodes: number;
  materializedEdges: number;
};

type RuntimeBoundaryRegionView = {
  id: string;
  path: string;
  provenance: string;
  materializationState: string;
  scopeState: string;
  knownFileCount: number;
  materializedFileCount: number;
};

type RuntimeMaterializationView = {
  workspace: RuntimeMaterializationItemView;
  episodic: RuntimeMaterializationItemView;
  inference: RuntimeMaterializationItemView;
  coordination: RuntimeMaterializationItemView;
};

type RuntimeFreshnessView = {
  fsObservedRevision: number;
  fsAppliedRevision: number;
  fsDirty: boolean;
  generationId?: number;
  parentGenerationId?: number;
  committedDeltaSequence?: number;
  lastRefreshPath?: string;
  lastRefreshTimestamp?: string;
  lastRefreshDurationMs?: number;
  lastRefreshLoadedBytes?: number;
  lastRefreshReplayVolume?: number;
  lastRefreshFullRebuildCount?: number;
  lastRefreshWorkspaceReloaded?: boolean;
  lastWorkspaceBuildMs?: number;
  lastDaemonReadyMs?: number;
  materialization: RuntimeMaterializationView;
  domains: RuntimeDomainFreshnessView[];
  activeCommand?: string;
  activeQueueClass?: string;
  queueDepth: number;
  queuedByClass: RuntimeQueueDepthView[];
  status: string;
  error?: string;
};

type RuntimeDomainFreshnessView = {
  domain: string;
  freshness: string;
  materializationDepth: string;
};

type RuntimeQueueDepthView = {
  queueClass: string;
  depth: number;
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
  mcpCallLogPath?: string;
  mcpCallLogBytes?: number;
  cachePath: string;
  cacheBytes?: number;
  healthPath: string;
  health: RuntimeHealthView;
  daemonCount: number;
  bridgeCount: number;
  connectedBridgeCount: number;
  orphanBridgeCount: number;
  processes: RuntimeProcessView[];
  processError?: string;
  freshness: RuntimeFreshnessView;
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

type QueryEvidenceView = {
  kind: string;
  detail: string;
  path?: string;
  line?: number;
  target?: NodeId;
};

type RepoPlaybookSectionView = {
  status: string;
  summary: string;
  commands: string[];
  why: string;
  provenance: QueryEvidenceView[];
};

type RepoPlaybookGotchaView = {
  summary: string;
  why: string;
  provenance: QueryEvidenceView[];
};

type RepoPlaybookView = {
  root: string;
  build: RepoPlaybookSectionView;
  test: RepoPlaybookSectionView;
  lint: RepoPlaybookSectionView;
  format: RepoPlaybookSectionView;
  workflow: RepoPlaybookSectionView;
  gotchas: RepoPlaybookGotchaView[];
};

type ValidationPlanSubjectView = {
  kind: string;
  taskId?: string;
  target?: NodeId;
  paths: string[];
  unresolvedPaths: string[];
};

type ValidationPlanCheckView = {
  label: string;
  why: string;
  provenance: QueryEvidenceView[];
  score?: number;
  lastSeen?: number;
};

type ValidationPlanView = {
  subject: ValidationPlanSubjectView;
  fast: ValidationPlanCheckView[];
  broader: ValidationPlanCheckView[];
  relatedTargets: NodeId[];
  notes: string[];
};

type QueryViewSubjectView = {
  kind: string;
  taskId?: string;
  target?: NodeId;
  paths: string[];
  unresolvedPaths: string[];
};

type QueryRecommendationView = {
  kind: string;
  label: string;
  why: string;
  provenance: QueryEvidenceView[];
  target?: NodeId;
  path?: string;
  score?: number;
  lastSeen?: number;
};

type QueryRiskHintView = {
  summary: string;
  why: string;
  provenance: QueryEvidenceView[];
};

type ImpactView = {
  subject: QueryViewSubjectView;
  downstream: QueryRecommendationView[];
  risks: QueryRiskHintView[];
  recommendedChecks: QueryRecommendationView[];
  contracts: ContractPacketView[];
  notes: string[];
};

type AfterEditView = {
  subject: QueryViewSubjectView;
  nextReads: QueryRecommendationView[];
  tests: QueryRecommendationView[];
  docs: QueryRecommendationView[];
  riskChecks: QueryRecommendationView[];
  contracts: ContractPacketView[];
  notes: string[];
};

type CommandMemoryCommandView = {
  command: string;
  confidence: number;
  why: string;
  provenance: QueryEvidenceView[];
  caveats: string[];
  lastSeen?: number;
};

type CommandMemoryView = {
  subject: QueryViewSubjectView;
  commands: CommandMemoryCommandView[];
  notes: string[];
};

type ConceptDecodeLensView = "open" | "workset" | "validation" | "timeline" | "memory";

type ConceptScopeView = "local" | "session" | "repo";

type ConceptPublicationStatusView = "active" | "retired";

type ConceptProvenanceView = {
  origin: string;
  kind: string;
  taskId?: string;
};

type ConceptPublicationView = {
  publishedAt: number;
  lastReviewedAt?: number;
  status: ConceptPublicationStatusView;
  supersedes: string[];
  retiredAt?: number;
  retirementReason?: string;
};

type AnchorRefView =
  | { type: "node"; crateName: string; path: string; kind: string; }
  | { type: "lineage"; lineageId: string; }
  | { type: "file"; path?: string; fileId?: number; }
  | { type: "kind"; kind: string; };

type ContractKindView =
  | "interface"
  | "behavioral"
  | "data_shape"
  | "dependency_boundary"
  | "lifecycle"
  | "protocol"
  | "operational";

type ContractStatusView = "candidate" | "active" | "deprecated" | "retired";

type ContractStabilityView =
  | "experimental"
  | "internal"
  | "public"
  | "deprecated"
  | "migrating";

type ContractGuaranteeStrengthView = "hard" | "soft" | "conditional";

type ContractHealthStatusView =
  | "healthy"
  | "watch"
  | "degraded"
  | "stale"
  | "superseded"
  | "retired";

type ContractTargetView = {
  anchors: AnchorRefView[];
  conceptHandles: string[];
};

type ContractGuaranteeView = {
  id: string;
  statement: string;
  scope?: string;
  strength?: ContractGuaranteeStrengthView;
  evidenceRefs: string[];
};

type ContractHealthSignalsView = {
  guaranteeCount: number;
  validationCount: number;
  consumerCount: number;
  validationCoverageRatio: number;
  guaranteeEvidenceRatio: number;
  staleValidationLinks: boolean;
};

type ContractHealthView = {
  status: ContractHealthStatusView;
  score: number;
  reasons: string[];
  signals: ContractHealthSignalsView;
  supersededBy: string[];
  nextAction?: string;
};

type ContractValidationView = {
  id: string;
  summary?: string;
  anchors: AnchorRefView[];
};

type ContractCompatibilityView = {
  compatible: string[];
  additive: string[];
  risky: string[];
  breaking: string[];
  migrating: string[];
};

type ContractResolutionView = {
  score: number;
  reasons: string[];
};

type ContractPacketView = {
  handle: string;
  name: string;
  summary: string;
  aliases: string[];
  kind: ContractKindView;
  subject: ContractTargetView;
  guarantees: ContractGuaranteeView[];
  assumptions: string[];
  consumers: ContractTargetView[];
  validations: ContractValidationView[];
  stability: ContractStabilityView;
  compatibility: ContractCompatibilityView;
  evidence: string[];
  status: ContractStatusView;
  health?: ContractHealthView;
  scope: ConceptScopeView;
  provenance: ConceptProvenanceView;
  publication?: ConceptPublicationView;
  resolution?: ContractResolutionView;
};

type ConceptPacketView = {
  handle: string;
  canonicalName: string;
  summary: string;
  aliases: string[];
  confidence: number;
  coreMembers: NodeId[];
  supportingMembers: NodeId[];
  likelyTests: NodeId[];
  evidence: string[];
  riskHint?: string;
  decodeLenses: ConceptDecodeLensView[];
  verbosityApplied: "summary" | "standard" | "full";
  truncation?: ConceptPacketTruncationView;
  curationHints: ConceptCurationHintsView;
  scope: ConceptScopeView;
  provenance: ConceptProvenanceView;
  publication?: ConceptPublicationView;
  resolution?: ConceptResolutionView;
  bindingMetadata?: {
    coreMemberLineages: Array<string | null>;
    supportingMemberLineages: Array<string | null>;
    likelyTestLineages: Array<string | null>;
  };
};

Concept packet default density:

- `prism.concepts(...)` defaults to `summary`
- `prism.concept(...)` defaults to `standard`
- `prism.conceptByHandle(...)` defaults to `standard`
- `prism.decodeConcept(...)` defaults to `standard`
- top-level `prism_concept` defaults to `summary`

When PRISM trims a concept packet for context, `verbosityApplied` tells you which density you got
and `truncation` reports what was omitted. Retry with `verbosity: "full"` only when you need the
complete packet. `curationHints` points at the first concrete member or likely test to inspect so
you do not have to reconstruct the next read manually.

type ConceptDecodeView = {
  concept: ConceptPacketView;
  lens: ConceptDecodeLensView;
  primary?: SymbolView;
  members: SymbolView[];
  supportingReads: SymbolView[];
  likelyTests: SymbolView[];
  recentFailures: OutcomeEvent[];
  relatedMemory: ScoredMemoryView[];
  recentPatches: PatchEventView[];
  validationRecipe?: ValidationRecipeView;
  evidence: string[];
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
  contracts: ContractPacketView[];
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
  contracts: ContractPacketView[];
  contractReviewNotes: string[];
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

type McpCallPayloadSummaryView = {
  kind: string;
  jsonBytes: number;
  itemCount?: number;
  truncated: boolean;
  excerpt?: unknown;
};

type McpCallLogEntryView = {
  id: string;
  callType: string;
  name: string;
  summary: string;
  startedAt: number;
  durationMs: number;
  sessionId?: string;
  taskId?: string;
  success: boolean;
  error?: string;
  operations: string[];
  touched: string[];
  diagnostics: QueryDiagnostic[];
  request: McpCallPayloadSummaryView;
  response: McpCallPayloadSummaryView;
  serverInstanceId: string;
  processId: number;
  workspaceRoot?: string;
  traceAvailable: boolean;
};

type McpCallTraceView = {
  entry: McpCallLogEntryView;
  phases: QueryPhaseView[];
  requestPayload?: unknown;
  requestPreview?: unknown;
  responsePreview?: unknown;
  metadata: unknown;
};

type McpCallStatsBucketView = {
  key: string;
  count: number;
  errorCount: number;
  averageDurationMs: number;
  maxDurationMs: number;
};

type McpCallStatsView = {
  totalCalls: number;
  successCount: number;
  errorCount: number;
  averageDurationMs: number;
  maxDurationMs: number;
  byCallType: McpCallStatsBucketView[];
  byName: McpCallStatsBucketView[];
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
  contracts: ContractPacketView[];
  contractReviewNotes: string[];
  promotedSummaries: string[];
};

type CoordinationInboxView = {
  plan: PlanView | null;
  planGraph: PlanGraphView | null;
  planExecution: PlanExecutionOverlayView[];
  planSummary: PlanSummaryView | null;
  planNext: PlanNodeRecommendationView[];
  readyTasks: CoordinationTaskView[];
  pendingReviews: ArtifactView[];
};

type TaskContextView = {
  task: CoordinationTaskView | null;
  taskNode: PlanNodeView | null;
  taskExecution: PlanExecutionOverlayView | null;
  planGraph: PlanGraphView | null;
  planSummary: PlanSummaryView | null;
  planNext: PlanNodeRecommendationView[];
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
  scope: string;
  content: string;
  metadata: unknown;
  createdAt: number;
  source: string;
  trust: number;
};

type MemoryEventView = {
  id: string;
  action: string;
  memoryId: string;
  scope: string;
  entry?: MemoryEntryView;
  recordedAt: number;
  taskId?: string;
  promotedFrom: string[];
  supersedes: string[];
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
  rootNodeIds: string[];
};

type PlanListEntryView = {
  planId: string;
  title: string;
  goal: string;
  status: string;
  scope: string;
  kind: string;
  rootNodeIds: string[];
  summary: PlanSummaryView;
};

type ValidationRefView = {
  id: string;
};

type PlanBindingView = {
  anchors: AnchorRef[];
  conceptHandles: string[];
  artifactRefs: string[];
  memoryRefs: string[];
  outcomeRefs: string[];
};

type PlanAcceptanceCriterionView = {
  label: string;
  anchors: AnchorRef[];
  requiredChecks: ValidationRefView[];
  evidencePolicy: string;
};

type PlanNodeView = {
  id: string;
  planId: string;
  kind: string;
  title: string;
  summary?: string;
  status: string;
  bindings: PlanBindingView;
  acceptance: PlanAcceptanceCriterionView[];
  validationRefs: ValidationRefView[];
  isAbstract: boolean;
  assignee?: string;
  baseRevision: WorkspaceRevisionView;
  priority?: number;
  tags: string[];
  metadata: Record<string, unknown>;
};

type PlanEdgeView = {
  id: string;
  planId: string;
  from: string;
  to: string;
  kind: string;
  summary?: string;
  metadata: Record<string, unknown>;
};

type PlanGraphView = {
  id: string;
  scope: string;
  kind: string;
  title: string;
  goal: string;
  status: string;
  revision: number;
  rootNodeIds: string[];
  tags: string[];
  createdFrom?: string;
  metadata: Record<string, unknown>;
  nodes: PlanNodeView[];
  edges: PlanEdgeView[];
};

type PlanExecutionOverlayView = {
  nodeId: string;
  pendingHandoffTo?: string;
  session?: string;
  effectiveAssignee?: string;
  awaitingHandoffFrom?: string;
};

type PlanNodeBlockerView = {
  kind: string;
  summary: string;
  relatedNodeId?: string;
  relatedArtifactId?: string;
  riskScore?: number;
  validationChecks: string[];
};

type PlanSummaryView = {
  planId: string;
  status: string;
  totalNodes: number;
  completedNodes: number;
  abandonedNodes: number;
  inProgressNodes: number;
  actionableNodes: number;
  executionBlockedNodes: number;
  completionGatedNodes: number;
  reviewGatedNodes: number;
  validationGatedNodes: number;
  staleNodes: number;
  claimConflictedNodes: number;
};

type PlanNodeRecommendationView = {
  node: PlanNodeView;
  actionable: boolean;
  effectiveAssignee?: string;
  score: number;
  reasons: string[];
  blockers: PlanNodeBlockerView[];
  unblocks: string[];
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

type CuratorProposalRecordView = {
  jobId: string;
  jobTrigger: string;
  jobStatus: "queued" | "running" | "completed" | "failed" | "skipped";
  jobTaskId?: string;
  focus: AnchorRef[];
  jobCreatedAt: number;
  jobStartedAt?: number;
  jobFinishedAt?: number;
  index: number;
  kind: string;
  disposition: "pending" | "applied" | "rejected";
  payload: unknown;
  decidedAt?: number;
  proposalTaskId?: string;
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
  - `prism://vocab`
  - `prism://session`
  - `prism://plans`
  - `prism://entrypoints`
  - `prism://schemas`
  - `prism://tool-schemas`
- Parameterized resources:
  - `prism://schema/{resourceKind}`
  - `prism://schema/tool/{toolName}`
  - `prism://plans?status={status}&scope={scope}&contains={contains}&limit={limit}&cursor={cursor}`
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

These recipes document the currently implemented rich query surface. They are useful when the
compact staged ABI is not available yet or when an agent genuinely needs the semantic escape hatch.

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

### 5c. Validate a tool payload before you call the tool

```ts
return prism.validateToolInput("prism_mutate", {
  action: "coordination",
  kind: "task_create",
  payload: { title: "Missing plan id" },
});
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

### 7a. Inspect recent MCP activity through PRISM itself

```ts
const recent = prism.mcpLog({ limit: 5 });
const trace = recent[0] ? prism.mcpTrace(recent[0].id) : null;
return {
  recent,
  trace,
  stats: prism.mcpStats({ limit: 50 }),
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

The flat aliases `prism.memoryRecall(...)`, `prism.memoryOutcomes(...)`, and
`prism.memoryEvents(...)` are also accepted for compatibility, but prefer the namespaced
`prism.memory.*(...)` form.

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

### 30a. Inspect pending curator proposals without walking whole jobs

```ts
return prism.curator.proposals({
  status: "completed",
  disposition: "pending",
  limit: 5,
});
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

### 33a. Discover plans by keyword before opening one

```ts
return prism.plans({
  contains: "persistence",
  limit: 5,
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

This is currently available, but it is not the long-term default agent path.

```ts
const search = prism.searchBundle("handle_request", { limit: 1 });
return prism.targetBundle(search);
```

### 37. Collapse direct symbol lookup into one consistent envelope

This is currently available, but it is not the long-term default agent path.

```ts
return prism.symbolBundle("handle_request", { includeDiscovery: true });
```

### 38. Collapse search plus top-target context into one helper

This is currently available, but it is not the long-term default agent path.

```ts
return prism.searchBundle("helper", { limit: 5 });
```

### 39. Opt into the slower full discovery bundle only when you need it

This is currently available, but it is not the long-term default agent path.

```ts
const search = prism.searchBundle("helper", {
  limit: 5,
  includeDiscovery: true,
});
return prism.targetBundle(search, { includeDiscovery: true, limit: 3 });
```

### 40. Collapse text search, raw file context, and semantic owner lookup into one helper

This is currently available, but it is not the long-term default agent path.

```ts
return prism.textSearchBundle("query_return_missing", {
  path: "crates/prism-mcp/src",
  semanticLimit: 3,
  aroundBefore: 2,
  aroundAfter: 8,
});
```

### 41. Use regex text search but still ask for semantic context with a separate query string

This is currently available, but it is not the long-term default agent path.

```ts
return prism.textSearchBundle("query_[a-z_]+", {
  regex: true,
  path: "crates/prism-mcp/src",
  semanticQuery: "query_return_missing",
  semanticLimit: 3,
});
```

### 42. Inspect the bundle summary flags directly

This is currently available, but it is not the long-term default agent path.

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

### 44. Ask `prism_expand` for a compact health lens on one concept handle

```json
{
  "handle": "concept://validation_pipeline",
  "kind": "health"
}
```

Expected shape: `status`, `score`, `signals`, `reasons`, and optional `repairTaskPayload`.

### 45. Ask `prism_expand` for a compact impact lens on one handle

```json
{
  "handle": "handle:1",
  "kind": "impact"
}
```

Expected shape: `likelyTouch`, `likelyTests`, `recentFailures`, and one short `riskHint`.

### 46. Ask `prism_expand` for a compact timeline lens on one handle

```json
{
  "handle": "handle:1",
  "kind": "timeline"
}
```

Expected shape: `recentEvents`, `recentPatches`, `lastFailure`, and `lastValidation`.

### 47. Ask `prism_expand` for compact memory recall on one handle

```json
{
  "handle": "handle:1",
  "kind": "memory"
}
```

Expected shape: `memories` with short summaries, memory kind/source/trust, and a one-line
`whyMatched`.

### 48. Ask `prism_task_brief` for a compact coordination read

```json
{
  "taskId": "coord-task:12"
}
```

Expected shape: task title/status/assignee, compact blockers and conflicts, recent outcomes,
likely validations, and 1 to 2 `nextReads`.

## Current implementation surface

- Target direction: a compact staged default agent ABI built around `prism_locate`, `prism_open`,
  `prism_gather`, `prism_workset`, `prism_expand`, `prism_task_brief`, and `prism_concept`, with
  `prism_query` retained as the semantic IR and escape hatch.
- Available now: symbol lookup, search, entrypoints, line-aware symbol locations, bounded source excerpts, focused local block retrieval, source extraction, relations, call graphs, lineage history, related failures, blast radius, and task replay by id.
- Available now: owner-biased discovery helpers through `prism.owners(...)`, `prism.nextReads(...)`, `prism.whereUsed(...)`, `prism.entrypointsFor(...)`, behavioral `prism.search(...)`, `prism.readContext(...)`, `prism.editContext(...)`, `prism.validationContext(...)`, `prism.recentChangeContext(...)`, and `implementationFor(..., { mode: "owners" })` without changing the direct primitive semantics.
- Available now: consistent eager bundle helpers through `prism.symbolBundle(...)`, `prism.searchBundle(...)`, `prism.textSearchBundle(...)`, and `prism.targetBundle(...)` with stable `summary`, `diagnostics`, and `suggestedReads` fields. These remain useful, but they are no longer the intended long-term default first hop for agent work.
- Available now: bounded workspace file reads through `prism.file(path).read(...)` and `prism.file(path).around(...)` for exact line-range and around-line inspection without leaving the PRISM query surface.
- Available now: bounded workspace text search through `prism.searchText(...)` with regex support, path/glob filters, exact match locations, and capped snippets, plus `prism.textSearchBundle(...)` to collapse text matches, one raw file window, and nearby semantic context into one helper.
- Available now: semantic recent-change inspection through `prism.changedFiles(...)`, `prism.changedSymbols(path, ...)`, `prism.recentPatches(...)`, `prism.diffFor(target, ...)`, and `prism.taskChanges(taskId, ...)` backed by recorded patch outcomes instead of raw diff dumps.
- Available now: direct daemon connection discovery through `prism.connectionInfo()` plus workspace-backed runtime introspection through `prism.runtimeStatus()`, `prism.runtimeLogs(...)`, and `prism.runtimeTimeline(...)` for daemon health, recent structured log events, startup/refresh diagnosis, and bridge upstream-resolution / connect latency without defaulting to shell status checks.
- Available now: a namespaced runtime alias through `prism.runtime.status()`, `prism.runtime.logs(...)`, and `prism.runtime.timeline(...)` so query authors do not have to remember only the flat runtime helpers.
- Available now: non-symbol repo coverage for markdown headings plus structured JSON, YAML, and TOML config keys through the normal PRISM search and relation surface.
- Available now: ambiguity-aware search narrowing through `path`, `module`, `taskId`, behavioral owner hints, and exact focused-block follow-ups surfaced directly from diagnostics and search resources.
- Available now: a durable canonical MCP call log through `prism.mcpLog(...)`, `prism.slowMcpCalls(...)`, `prism.mcpTrace(id)`, and `prism.mcpStats(...)` with per-runtime execution history, duration, diagnostics, payload summaries, previews, and phase breakdowns for tools and resources.
- Available now: spec-to-code clustering and drift explanations that group direct links with read/write/persistence/test owners for spec-like symbols.
- Available now: repo-exported curated concept packets that hydrate into the live concept layer and travel with the repo through `.prism/concepts/events.jsonl`.
- Available now: session/workspace memory recall for anchored memory entries, filtered outcome history, and promoted curator memories.
- Available now: workspace-backed curator job inspection through `prism.curator.jobs()`, flat proposal inspection through `prism.curator.proposals()`, and job detail through `prism.curator.job()`.
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
  - action `concept`
  - action `concept_relation`
  - action `infer_edge`
  - action `coordination`
  - action `claim`
  - action `artifact`
  - action `test_ran`
  - action `failure_observed`
  - action `fix_validated`
  - action `curator_apply_proposal`
  - action `curator_promote_edge`
  - action `curator_promote_concept`
  - action `curator_promote_memory`
  - action `curator_reject_proposal`
  - shorthand `{ action, ...fields }` is accepted in addition to the canonical `{ action, input: { ... } }`

Read current session state through `prism://session`.

Patch observation is automatic. PRISM records file changes from `ObservedChangeSet` without requiring an explicit MCP call.
"#;
