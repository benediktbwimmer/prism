export type ThemeChoice = 'system' | 'light' | 'dark'

export type SessionTaskView = {
  taskId: string
  description: string | null
  tags: string[]
}

export type SessionView = {
  workspaceRoot: string | null
  currentTask: SessionTaskView | null
}

export type RuntimeHealthView = {
  ok: boolean
  detail: string
}

export type RuntimeProcessView = {
  pid: number
  rssMb: number
  elapsed: string
  kind: string
  command: string
}

export type RuntimeLogEventView = {
  timestamp?: string | null
  level?: string | null
  message: string
  target?: string | null
  fields?: Record<string, unknown> | null
}

export type RuntimeStatusView = {
  health: RuntimeHealthView
  daemonCount: number
  bridgeCount: number
  cacheBytes?: number | null
  logBytes?: number | null
  processes: RuntimeProcessView[]
}

export type QueryDiagnostic = {
  code: string
  message: string
}

export type QueryLogEntryView = {
  id: string
  kind: string
  querySummary: string
  startedAt: number
  durationMs: number
  success: boolean
  error?: string | null
  taskId?: string | null
  sessionId: string
  operations: string[]
  touched: string[]
  diagnostics: QueryDiagnostic[]
  result: {
    truncated: boolean
  }
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

export type DashboardSummaryView = {
  session: SessionView
  runtime: RuntimeStatusView
  activeQueryCount: number
  activeMutationCount: number
  recentQueryErrorCount: number
  lastRuntimeEvent?: RuntimeLogEventView | null
}

export type DashboardOperationsView = {
  active: ActiveOperationView[]
  recentQueries: QueryLogEntryView[]
  recentMutations: MutationLogEntryView[]
}

export type DashboardBootstrapView = {
  summary: DashboardSummaryView
  operations: DashboardOperationsView
}
