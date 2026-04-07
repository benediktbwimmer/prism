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
  bridgeIdentity?: BridgeIdentityView | null
  limits?: SessionLimitsView
  features?: FeatureFlagsView
}

export type BridgeIdentityView = {
  status: string
  profile?: string | null
  principalId?: string | null
  credentialId?: string | null
  error?: string | null
  nextAction?: string | null
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

export type CoordinationTaskView = {
  id: string
  planId: string
  kind?: string
  title: string
  summary?: string | null
  status: string
  publishedTaskStatus?: string | null
  assignee?: string | null
  pendingHandoffTo?: string | null
  anchors?: AnchorRef[]
  bindings?: PlanBindingView
  dependsOn: string[]
  coordinationDependsOn?: string[]
  integratedDependsOn?: string[]
  lifecycle?: {
    completed: boolean
    coordinationPublished: boolean
    integratedToTarget: boolean
    publishedToBranch: boolean
  }
  validationRefs?: ValidationRefView[]
  isAbstract?: boolean
  baseRevision?: WorkspaceRevisionView
  priority?: number | null
  tags?: string[]
  gitExecution?: {
    status?: string | null
    sourceRef?: string | null
    publishRef?: string | null
  }
}

export type PrismUiSessionBootstrapView = {
  session: SessionView
  runtime: RuntimeStatusView
  pollingIntervalMs: number
}

export type PrismUiFleetLaneView = {
  id: string
  runtimeId?: string | null
  label: string
  principalId?: string | null
  worktreeId?: string | null
  branchRef?: string | null
  discoveryMode?: string | null
  lastSeenAt?: number | null
  activeBarCount: number
  staleBarCount: number
  idle: boolean
}

export type PrismUiFleetBarView = {
  id: string
  laneId: string
  runtimeId?: string | null
  taskId?: string | null
  taskTitle: string
  taskStatus: string
  claimId?: string | null
  claimStatus?: string | null
  holder?: string | null
  agent?: string | null
  capability?: string | null
  mode?: string | null
  branchRef?: string | null
  startedAt: number
  endedAt?: number | null
  durationSeconds?: number | null
  active: boolean
  stale: boolean
}

export type PrismUiFleetView = {
  generatedAt: number
  windowStart: number
  windowEnd: number
  lanes: PrismUiFleetLaneView[]
  bars: PrismUiFleetBarView[]
}

export type PrismUiTaskEditableMetadataView = {
  title: string
  description?: string | null
  priority?: number | null
  assignee?: string | null
  status: string
  validationRefs: ValidationRefView[]
  validationGuidance: string[]
  statusOptions: string[]
}

export type PrismUiTaskClaimHistoryEntryView = {
  id: string
  holder: string
  agent?: string | null
  status: string
  capability: string
  mode: string
  startedAt: number
  refreshedAt?: number | null
  staleAt?: number | null
  expiresAt: number
  durationSeconds?: number | null
  branchRef?: string | null
  worktreeId?: string | null
}

export type PrismUiTaskBlockerEntryView = {
  blocker: {
    kind: string
    summary: string
    relatedTaskId?: string | null
    relatedArtifactId?: string | null
    riskScore?: number | null
    validationChecks: string[]
  }
  relatedTask?: CoordinationTaskView | null
}

export type PrismUiTaskCommitView = {
  kind: string
  commit: string
  reference?: string | null
  label: string
}

export type PrismUiTaskDetailView = {
  task: CoordinationTaskView
  editable: PrismUiTaskEditableMetadataView
  claimHistory: PrismUiTaskClaimHistoryEntryView[]
  blockers: PrismUiTaskBlockerEntryView[]
  outcomes: OutcomeSummaryView[]
  recentCommits: PrismUiTaskCommitView[]
  artifacts: ArtifactView[]
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
  scheduling: {
    importance: number
    urgency: number
    manualBoost: number
    dueAt?: number | null
  }
  gitExecutionPolicy: {
    startMode: string
    completionMode: string
    integrationMode: string
    targetRef?: string | null
    targetBranch: string
    requireTaskBranch: boolean
    maxCommitsBehindTarget: number
    maxFetchAgeSeconds?: number | null
  }
  summary: string
  planSummary: PlanSummaryView
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

export type NodeRefView = {
  kind: string
  id: string
}

export type BlockerCauseView = {
  source: string
  code?: string | null
  acceptanceLabel?: string | null
  thresholdMetric?: string | null
  thresholdValue?: number | null
  observedValue?: number | null
}

export type CoordinationPlanV2View = {
  id: string
  parentPlanId?: string | null
  title: string
  goal: string
  scope: string
  kind: string
  operatorState: string
  status: string
  scheduling: PlanListEntryView['scheduling']
  tags: string[]
  createdFrom?: string | null
  metadata?: unknown
  children: NodeRefView[]
  dependencies: NodeRefView[]
  dependents: NodeRefView[]
  estimatedMinutesTotal: number
  remainingEstimatedMinutes: number
}

export type TaskExecutorPolicyView = {
  executorClass: string
  targetLabel?: string | null
  allowedPrincipals: string[]
}

export type TaskGitExecutionView = {
  status: string
  pendingTaskStatus?: string | null
  sourceRef?: string | null
  targetRef?: string | null
  publishRef?: string | null
  targetBranch?: string | null
}

export type CoordinationTaskV2View = {
  id: string
  parentPlanId: string
  title: string
  summary?: string | null
  lifecycleStatus: string
  status: string
  graphActionable: boolean
  estimatedMinutes: number
  executor: TaskExecutorPolicyView
  assignee?: string | null
  session?: string | null
  worktreeId?: string | null
  branchRef?: string | null
  anchors: AnchorRef[]
  bindings: PlanBindingView
  validationRefs: ValidationRefView[]
  baseRevision: WorkspaceRevisionView
  priority?: number | null
  tags: string[]
  metadata?: unknown
  gitExecution: TaskGitExecutionView
  blockerCauses: BlockerCauseView[]
  dependencies: NodeRefView[]
  dependents: NodeRefView[]
}

export type OutcomeSummaryView = {
  ts: number
  kind: string
  result: string
  summary: string
}

export type PrismPlanDetailView = {
  plan: PlanListEntryView
  summary: PlanSummaryView
  children: NodeRefView[]
  childPlans: CoordinationPlanV2View[]
  childTasks: CoordinationTaskV2View[]
  readyTasks: CoordinationTaskView[]
  pendingReviews: ArtifactView[]
  pendingHandoffs: CoordinationTaskView[]
  recentViolations: PolicyViolationRecordView[]
  recentOutcomes: OutcomeSummaryView[]
}

export type PrismUiPlansFiltersView = {
  status: string
  search?: string | null
  sort: string
  agent?: string | null
}

export type PrismUiPlansStatsView = {
  totalPlans: number
  visiblePlans: number
  activePlans: number
  completedPlans: number
  archivedPlans: number
}

export type PrismPlansView = {
  filters: PrismUiPlansFiltersView
  stats: PrismUiPlansStatsView
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
