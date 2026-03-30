export type ThemeChoice = 'system' | 'light' | 'dark'

export type SessionTaskView = {
  taskId: string
  description: string | null
  tags: string[]
}

export type SessionLimitsView = {
  maxResultNodes: number
  maxCallGraphDepth: number
  maxOutputJsonBytes: number
}

export type CoordinationFeaturesView = {
  workflow: boolean
  claims: boolean
  artifacts: boolean
}

export type FeatureFlagsView = {
  mode: string
  coordination: CoordinationFeaturesView
  internalDeveloper: boolean
}

export type SessionView = {
  workspaceRoot: string | null
  currentTask: SessionTaskView | null
  currentAgent?: string | null
  limits?: SessionLimitsView
  features?: FeatureFlagsView
}

export type RuntimeHealthView = {
  ok: boolean
  detail: string
}

export type RuntimeProcessView = {
  pid: number
  rssKb?: number
  rssMb: number
  elapsed: string
  kind: string
  command: string
  healthPath?: string | null
}

export type RuntimeLogEventView = {
  timestamp?: string | null
  level?: string | null
  message: string
  target?: string | null
  file?: string | null
  lineNumber?: number | null
  fields?: Record<string, unknown> | null
}

export type RuntimeStatusView = {
  root?: string
  uri?: string | null
  uriFile?: string
  logPath?: string
  cachePath?: string
  healthPath?: string
  health: RuntimeHealthView
  daemonCount: number
  bridgeCount: number
  cacheBytes?: number | null
  logBytes?: number | null
  processError?: string | null
  processes: RuntimeProcessView[]
}

export type QueryDiagnostic = {
  code: string
  message: string
  data?: unknown
}

export type QueryResultSummaryView = {
  kind: string
  jsonBytes: number
  itemCount?: number | null
  truncated: boolean
  outputCapHit?: boolean
  resultCapHit?: boolean
}

export type QueryPhaseView = {
  operation: string
  startedAt: number
  durationMs: number
  argsSummary?: unknown
  touched: string[]
  success: boolean
  error?: string | null
}

export type QueryLogEntryView = {
  id: string
  kind: string
  querySummary: string
  queryText: string
  startedAt: number
  durationMs: number
  success: boolean
  error?: string | null
  taskId?: string | null
  sessionId: string
  operations: string[]
  touched: string[]
  diagnostics: QueryDiagnostic[]
  result: QueryResultSummaryView
}

export type QueryTraceView = {
  entry: QueryLogEntryView
  phases: QueryPhaseView[]
}

export type MutationLogEntryView = {
  id: string
  action: string
  startedAt: number
  durationMs: number
  taskId?: string | null
  sessionId: string
  success: boolean
  error?: string | null
  violationCount: number
  resultIds: string[]
}

export type MutationTraceView = {
  entry: MutationLogEntryView
  phases: QueryPhaseView[]
  result: unknown
}

export type ActiveOperationView = {
  id: string
  kind: 'query' | 'mutation' | string
  label: string
  startedAt: number
  sessionId: string
  taskId?: string | null
  status: string
  phase?: string | null
  touched: string[]
  error?: string | null
}

export type TaskLifecycleSummaryView = {
  planCount: number
  patchCount: number
  buildCount: number
  testCount: number
  failureCount: number
  validationCount: number
  noteCount: number
  startedAt?: number | null
  lastUpdatedAt?: number | null
  finalSummary?: string | null
}

export type TaskJournalView = {
  taskId: string
  description?: string | null
  tags: string[]
  disposition: string
  active: boolean
  summary: TaskLifecycleSummaryView
}

export type AnchorRef = unknown

export type WorkspaceRevisionView = {
  graphVersion: number
  gitCommit?: string | null
}

export type ArtifactView = {
  id: string
  taskId: string
  status: string
  anchors?: AnchorRef[]
  baseRevision?: WorkspaceRevisionView
  diffRef?: string | null
  requiredValidations: string[]
  validatedChecks: string[]
  riskScore?: number | null
}

export type PolicyViolationView = {
  code: string
  summary: string
  planId?: string | null
  taskId?: string | null
  claimId?: string | null
  artifactId?: string | null
  details?: unknown
}

export type PolicyViolationRecordView = {
  eventId: string
  ts: number
  summary: string
  planId?: string | null
  taskId?: string | null
  claimId?: string | null
  artifactId?: string | null
  violations: PolicyViolationView[]
}

export type DashboardSummaryView = {
  session: SessionView
  runtime: RuntimeStatusView
  activeQueryCount: number
  activeMutationCount: number
  recentQueryErrorCount: number
  lastRuntimeEvent?: RuntimeLogEventView | null
}

export type DashboardTaskSnapshotView = {
  session: SessionView
  journal?: TaskJournalView | null
}

export type CoordinationTaskView = {
  id: string
  planId: string
  title: string
  status: string
  assignee?: string | null
  pendingHandoffTo?: string | null
  anchors?: AnchorRef[]
  dependsOn: string[]
  baseRevision?: WorkspaceRevisionView
}

export type DashboardCoordinationSummaryView = {
  enabled: boolean
  activePlanCount: number
  taskCount: number
  readyTaskCount: number
  inReviewTaskCount: number
  activeClaimCount: number
  pendingReviewCount: number
  proposedArtifactCount: number
  recentPendingReviews: ArtifactView[]
  recentViolations: PolicyViolationRecordView[]
}

export type DashboardOperationsView = {
  active: ActiveOperationView[]
  recentQueries: QueryLogEntryView[]
  recentMutations: MutationLogEntryView[]
}

export type DashboardBootstrapView = {
  summary: DashboardSummaryView
  operations: DashboardOperationsView
  task: DashboardTaskSnapshotView
  coordination: DashboardCoordinationSummaryView
}

export type DashboardOperationDetailView =
  | {
      kind: 'active'
      operation: ActiveOperationView
    }
  | {
      kind: 'query'
      trace: QueryTraceView
    }
  | {
      kind: 'mutation'
      trace: MutationTraceView
    }

export type RuntimeRefreshEvent = {
  refreshPath?: string
  durationMs?: number
  coordinationReloaded?: boolean
  episodicReloaded?: boolean
  inferenceReloaded?: boolean
}

export type PlanSummaryView = {
  planId: string
  status: string
  totalNodes: number
  completedNodes: number
  abandonedNodes: number
  inProgressNodes: number
  actionableNodes: number
  executionBlockedNodes: number
  completionGatedNodes: number
  reviewGatedNodes: number
  validationGatedNodes: number
  staleNodes: number
  claimConflictedNodes: number
}

export type PlanListEntryView = {
  planId: string
  title: string
  goal: string
  status: string
  scope: string
  kind: string
  rootNodeIds: string[]
  summary: PlanSummaryView
}

export type ValidationRefView = {
  id: string
}

export type PlanBindingView = {
  anchors: AnchorRef[]
  conceptHandles: string[]
  artifactRefs: string[]
  memoryRefs: string[]
  outcomeRefs: string[]
}

export type PlanAcceptanceCriterionView = {
  label: string
  anchors: AnchorRef[]
  requiredChecks: ValidationRefView[]
  evidencePolicy: string
}

export type PlanNodeView = {
  id: string
  planId: string
  kind?: string
  title: string
  summary?: string | null
  status: string
  bindings: PlanBindingView
  acceptance?: PlanAcceptanceCriterionView[]
  validationRefs?: ValidationRefView[]
  isAbstract?: boolean
  assignee?: string | null
  baseRevision?: WorkspaceRevisionView
  priority?: number | null
  tags?: string[]
  metadata?: unknown
}

export type PlanEdgeView = {
  id: string
  planId: string
  from: string
  to: string
  kind: string
  summary?: string | null
  metadata?: unknown
}

export type PlanGraphView = {
  id: string
  scope: string
  kind: string
  title: string
  goal: string
  status: string
  revision: number
  rootNodeIds: string[]
  tags: string[]
  createdFrom?: string | null
  metadata?: unknown
  nodes: PlanNodeView[]
  edges: PlanEdgeView[]
}

export type PlanExecutionOverlayView = {
  nodeId: string
  pendingHandoffTo?: string | null
  session?: string | null
  effectiveAssignee?: string | null
  awaitingHandoffFrom?: string | null
}

export type PlanNodeBlockerView = {
  kind: string
  summary: string
  relatedNodeId?: string | null
  relatedArtifactId?: string | null
  riskScore?: number | null
  validationChecks: string[]
}

export type PlanNodeRecommendationView = {
  node: PlanNodeView
  actionable: boolean
  effectiveAssignee?: string | null
  score?: number
  reasons: string[]
  blockers?: PlanNodeBlockerView[]
  unblocks?: string[]
}

export type OverviewPlanSignalsView = {
  blockedNodes: number
  reviewGatedNodes: number
  validationGatedNodes: number
  claimConflictedNodes: number
}

export type OverviewPlanSpotlightView = {
  planId: string
  title: string
  goal: string
  summary: PlanSummaryView
  nextNodes: PlanNodeRecommendationView[]
}

export type OverviewConceptSpotlightView = {
  handle: string
  canonicalName: string
  summary: string
}

export type OutcomeSummaryView = {
  ts: number
  kind: string
  result: string
  summary: string
}

export type PrismOverviewView = {
  summary: DashboardSummaryView
  task: DashboardTaskSnapshotView
  coordination: DashboardCoordinationSummaryView
  planSignals: OverviewPlanSignalsView
  spotlightPlans: OverviewPlanSpotlightView[]
  hotConcepts: OverviewConceptSpotlightView[]
  recentOutcomes: OutcomeSummaryView[]
  pendingHandoffs: CoordinationTaskView[]
}

export type PrismPlanDetailView = {
  plan: PlanListEntryView
  summary: PlanSummaryView
  graph: PlanGraphView
  execution: PlanExecutionOverlayView[]
  nextNodes: PlanNodeRecommendationView[]
  readyTasks: CoordinationTaskView[]
  pendingReviews: ArtifactView[]
  pendingHandoffs: CoordinationTaskView[]
  recentViolations: PolicyViolationRecordView[]
  recentOutcomes: OutcomeSummaryView[]
}

export type PrismPlansView = {
  plans: PlanListEntryView[]
  selectedPlanId?: string | null
  selectedPlan?: PrismPlanDetailView | null
}

export type ConceptRelationView = {
  kind: string
  direction: string
  relatedHandle: string
  relatedCanonicalName?: string | null
  relatedSummary?: string | null
  confidence: number
  evidence: string[]
  scope: string
}

export type ConceptPacketView = {
  handle: string
  canonicalName: string
  summary: string
  aliases: string[]
  confidence: number
  coreMembers: Array<{ path: string; kind?: string | null }>
  supportingMembers: Array<{ path: string; kind?: string | null }>
  likelyTests: Array<{ path: string; kind?: string | null }>
  evidence: string[]
  riskHint?: string | null
  decodeLenses: string[]
  verbosityApplied: string
  relations: ConceptRelationView[]
}

export type GraphTouchedNodeView = {
  nodeId: string
  title: string
  status: string
}

export type GraphPlanTouchpointView = {
  plan: PlanListEntryView
  touchedNodes: GraphTouchedNodeView[]
}

export type PrismGraphView = {
  selectedConceptHandle: string
  focus: ConceptPacketView
  entryConcepts: ConceptPacketView[]
  relatedPlans: GraphPlanTouchpointView[]
}
