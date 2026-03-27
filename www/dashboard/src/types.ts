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

export type ArtifactView = {
  id: string
  taskId: string
  status: string
  requiredValidations: string[]
  validatedChecks: string[]
  riskScore?: number | null
}

export type PolicyViolationView = {
  code: string
  summary: string
}

export type PolicyViolationRecordView = {
  eventId: string
  ts: number
  summary: string
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
