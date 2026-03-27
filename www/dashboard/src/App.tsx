import {
  startTransition,
  useEffect,
  useRef,
  useState,
} from 'react'

import { LatencySparkline } from './components/LatencySparkline'
import { OperationDrawer } from './components/OperationDrawer'
import type {
  ActiveOperationView,
  DashboardBootstrapView,
  DashboardCoordinationSummaryView,
  DashboardOperationDetailView,
  DashboardSummaryView,
  DashboardTaskSnapshotView,
  MutationLogEntryView,
  QueryLogEntryView,
  RuntimeRefreshEvent,
  ThemeChoice,
} from './types'

const THEME_KEY = 'prism-dashboard-theme'

export function App() {
  const [dashboard, setDashboard] = useState<DashboardBootstrapView | null>(null)
  const [connection, setConnection] = useState<'connecting' | 'open' | 'closed'>('connecting')
  const [selectedOperationId, setSelectedOperationId] = useState<string | null>(null)
  const [selectedOperation, setSelectedOperation] = useState<DashboardOperationDetailView | null>(null)
  const [detailStatus, setDetailStatus] = useState<'idle' | 'loading' | 'error'>('idle')
  const selectedOperationIdRef = useRef<string | null>(null)
  const [themeChoice, setThemeChoice] = useState<ThemeChoice>(() => {
    const stored = window.localStorage.getItem(THEME_KEY)
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      return stored
    }
    return 'system'
  })

  useEffect(() => {
    window.localStorage.setItem(THEME_KEY, themeChoice)
    const root = document.documentElement
    const resolvedDark =
      themeChoice === 'dark' ||
      (themeChoice === 'system' &&
        window.matchMedia('(prefers-color-scheme: dark)').matches)
    root.dataset.theme = resolvedDark ? 'dark' : 'light'
  }, [themeChoice])

  useEffect(() => {
    let cancelled = false
    async function loadDashboard() {
      const response = await fetch('/dashboard/api/bootstrap')
      const next = (await response.json()) as DashboardBootstrapView
      if (cancelled) {
        return
      }
      startTransition(() => setDashboard(next))
    }
    void loadDashboard()
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    selectedOperationIdRef.current = selectedOperationId
  }, [selectedOperationId])

  async function loadOperationDetail(id: string) {
    setDetailStatus('loading')
    setSelectedOperation(null)
    try {
      const response = await fetch(`/dashboard/api/operations/${encodeURIComponent(id)}`)
      if (!response.ok) {
        throw new Error(`operation detail ${response.status}`)
      }
      const detail = (await response.json()) as DashboardOperationDetailView
      startTransition(() => {
        setSelectedOperation(detail)
        setDetailStatus('idle')
      })
    } catch {
      startTransition(() => {
        setSelectedOperation(null)
        setDetailStatus('error')
      })
    }
  }

  function selectOperation(id: string) {
    setSelectedOperationId(id)
    void loadOperationDetail(id)
  }

  useEffect(() => {
    async function refreshSummary() {
      const response = await fetch('/dashboard/api/summary')
      const summary = (await response.json()) as DashboardSummaryView
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                summary,
              }
            : current,
        )
      })
    }

    async function refreshCoordination() {
      const response = await fetch('/dashboard/api/coordination')
      const coordination = (await response.json()) as DashboardCoordinationSummaryView
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                coordination,
              }
            : current,
        )
      })
    }

    function handleActiveEvent(operation: ActiveOperationView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = [operation, ...current.operations.active.filter((item) => item.id !== operation.id)]
            .sort((left, right) => right.startedAt - left.startedAt)
            .slice(0, 30)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
            },
            operations: {
              ...current.operations,
              active,
            },
          }
        })
      })
      if (selectedOperationIdRef.current === operation.id) {
        void loadOperationDetail(operation.id)
      }
    }

    function handleFinishedQuery(query: QueryLogEntryView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = current.operations.active.filter((item) => item.id !== query.id)
          const recentQueries = [query, ...current.operations.recentQueries.filter((item) => item.id !== query.id)].slice(0, 20)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
              recentQueryErrorCount: recentQueries.filter((item) => !item.success).length,
            },
            operations: {
              ...current.operations,
              active,
              recentQueries,
            },
          }
        })
      })
      if (selectedOperationIdRef.current === query.id) {
        void loadOperationDetail(query.id)
      }
    }

    function handleFinishedMutation(mutation: MutationLogEntryView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = current.operations.active.filter((item) => item.id !== mutation.id)
          const recentMutations = [mutation, ...current.operations.recentMutations.filter((item) => item.id !== mutation.id)].slice(0, 20)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
            },
            operations: {
              ...current.operations,
              active,
              recentMutations,
            },
          }
        })
      })
      if (selectedOperationIdRef.current === mutation.id) {
        void loadOperationDetail(mutation.id)
      }
    }

    function handleTaskUpdate(task: DashboardTaskSnapshotView) {
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                task,
                summary: {
                  ...current.summary,
                  session: task.session,
                },
              }
            : current,
        )
      })
    }

    function handleCoordinationUpdate(coordination: DashboardCoordinationSummaryView) {
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                coordination,
              }
            : current,
        )
      })
    }

    const source = new EventSource('/dashboard/events')
    setConnection('connecting')

    source.onopen = () => setConnection('open')
    source.onerror = () => setConnection('closed')

    source.addEventListener('query.started', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('query.phase', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('query.finished', (event) => {
      handleFinishedQuery(JSON.parse(event.data) as QueryLogEntryView)
    })
    source.addEventListener('mutation.started', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('mutation.finished', (event) => {
      handleFinishedMutation(JSON.parse(event.data) as MutationLogEntryView)
    })
    source.addEventListener('task.updated', (event) => {
      handleTaskUpdate(JSON.parse(event.data) as DashboardTaskSnapshotView)
    })
    source.addEventListener('coordination.updated', (event) => {
      handleCoordinationUpdate(JSON.parse(event.data) as DashboardCoordinationSummaryView)
    })
    source.addEventListener('runtime.refreshed', (event) => {
      const payload = JSON.parse(event.data) as RuntimeRefreshEvent
      void refreshSummary()
      if (payload.coordinationReloaded) {
        void refreshCoordination()
      }
    })

    return () => {
      source.close()
    }
  }, [])

  if (!dashboard) {
    return (
      <main className="app-shell loading-shell">
        <section className="panel hero-panel">
          <p className="eyebrow">PRISM Dashboard</p>
          <h1>Connecting to the daemon</h1>
          <p>Loading the first dashboard snapshot and subscribing to live events.</p>
        </section>
      </main>
    )
  }

  const { summary, operations, task, coordination } = dashboard

  return (
    <>
      <main className="app-shell">
        <section className="hero-bar panel">
          <div>
            <p className="eyebrow">PRISM Dashboard</p>
            <h1>Live server activity</h1>
            <p className="lede">
              {summary.session.workspaceRoot ?? 'Unknown workspace'}
            </p>
          </div>
          <div className="hero-actions">
            <span className={`connection-pill connection-${connection}`}>{connection}</span>
            <label className="theme-picker">
              <span>Theme</span>
              <select value={themeChoice} onChange={(event) => setThemeChoice(event.target.value as ThemeChoice)}>
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
          </div>
        </section>

        <section className="status-grid">
          <article className="panel stat-card">
            <p className="stat-label">Health</p>
            <h2>{summary.runtime.health.ok ? 'Healthy' : 'Degraded'}</h2>
            <p>{summary.runtime.health.detail}</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Processes</p>
            <h2>{summary.runtime.daemonCount} daemon / {summary.runtime.bridgeCount} bridges</h2>
            <p>Shared live state across attached agent sessions.</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Active Operations</p>
            <h2>{summary.activeQueryCount} queries / {summary.activeMutationCount} mutations</h2>
            <p>Streaming over SSE with replay support.</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Current Task</p>
            <h2>{summary.session.currentTask?.description ?? 'No active session task'}</h2>
            <p>{summary.session.currentTask?.taskId ?? 'Session is idle'}</p>
          </article>
        </section>

        <section className="content-grid">
          <article className="panel">
            <div className="panel-header">
              <h3>Active Operations</h3>
              <span>{operations.active.length}</span>
            </div>
            <div className="operation-list">
              {operations.active.length === 0 ? (
                <p className="empty-state">No active operations right now.</p>
              ) : (
                operations.active.map((operation) => (
                  <button key={operation.id} type="button" className="operation-button" onClick={() => selectOperation(operation.id)}>
                    <div className="operation-row">
                      <div>
                        <p className="operation-kind">{operation.kind}</p>
                        <h4>{operation.label}</h4>
                        <p className="operation-meta">
                          {operation.phase ?? operation.status}
                          {operation.taskId ? ` • ${operation.taskId}` : ''}
                        </p>
                      </div>
                      <span className="operation-id">{operation.id}</span>
                    </div>
                  </button>
                ))
              )}
            </div>
          </article>

          <article className="panel">
            <div className="panel-header">
              <h3>Recent Query Latency</h3>
              <span>{operations.recentQueries.length}</span>
            </div>
            <LatencySparkline queries={operations.recentQueries} />
            <p className="sparkline-caption">
              D3-backed sparkline for the last completed queries, with failed runs marked in red.
            </p>
          </article>

          <article className="panel">
            <div className="panel-header">
              <h3>Task Focus</h3>
              <span>{task.session.currentTask ? 'active' : 'idle'}</span>
            </div>
            <div className="task-focus">
              <h4>{task.journal?.description ?? task.session.currentTask?.description ?? 'No current task'}</h4>
              <p className="operation-meta">{task.session.currentTask?.taskId ?? 'No task selected in this session.'}</p>
              {task.journal ? (
                <div className="metric-grid">
                  <MetricCard label="Tests" value={task.journal.summary.testCount} />
                  <MetricCard label="Patches" value={task.journal.summary.patchCount} />
                  <MetricCard label="Failures" value={task.journal.summary.failureCount} />
                  <MetricCard label="Validation" value={task.journal.summary.validationCount} />
                </div>
              ) : (
                <p className="empty-state">Start or attach a task to see its journal summary here.</p>
              )}
            </div>
          </article>

          <article className="panel">
            <div className="panel-header">
              <h3>Coordination</h3>
              <span>{coordination.enabled ? 'enabled' : 'disabled'}</span>
            </div>
            {!coordination.enabled ? (
              <p className="empty-state panel-body">Coordination features are disabled for this server.</p>
            ) : (
              <div className="coordination-panel">
                <div className="metric-grid">
                  <MetricCard label="Plans" value={coordination.activePlanCount} />
                  <MetricCard label="Tasks" value={coordination.taskCount} />
                  <MetricCard label="Ready" value={coordination.readyTaskCount} />
                  <MetricCard label="In Review" value={coordination.inReviewTaskCount} />
                  <MetricCard label="Claims" value={coordination.activeClaimCount} />
                  <MetricCard label="Pending Reviews" value={coordination.pendingReviewCount} />
                </div>
                <div className="mini-lists">
                  <div>
                    <h4>Pending Reviews</h4>
                    {coordination.recentPendingReviews.length === 0 ? (
                      <p className="empty-state">No pending review artifacts.</p>
                    ) : (
                      coordination.recentPendingReviews.map((artifact) => (
                        <div key={artifact.id} className="mini-row">
                          <strong>{artifact.id}</strong>
                          <span>{artifact.status}</span>
                        </div>
                      ))
                    )}
                  </div>
                  <div>
                    <h4>Recent Violations</h4>
                    {coordination.recentViolations.length === 0 ? (
                      <p className="empty-state">No recent policy violations.</p>
                    ) : (
                      coordination.recentViolations.map((record) => (
                        <div key={record.eventId} className="mini-row mini-row-stack">
                          <strong>{record.summary}</strong>
                          <span>{record.violations.map((violation) => violation.code).join(', ')}</span>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>
            )}
          </article>

          <article className="panel wide-panel">
            <div className="panel-header">
              <h3>Recent Queries</h3>
              <span>{operations.recentQueries.length}</span>
            </div>
            <div className="table-list">
              {operations.recentQueries.map((query) => (
                <button key={query.id} type="button" className="table-button" onClick={() => selectOperation(query.id)}>
                  <div className="table-row">
                    <div className="table-primary">
                      <h4>{query.querySummary}</h4>
                      <p>{query.operations.join(', ') || query.kind}</p>
                    </div>
                    <div className="table-metric">{query.durationMs} ms</div>
                    <div className={`table-status ${query.success ? 'ok' : 'error'}`}>
                      {query.success ? 'ok' : 'error'}
                    </div>
                  </div>
                </button>
              ))}
            </div>
          </article>

          <article className="panel">
            <div className="panel-header">
              <h3>Recent Mutations</h3>
              <span>{operations.recentMutations.length}</span>
            </div>
            <div className="table-list">
              {operations.recentMutations.length === 0 ? (
                <p className="empty-state">No recorded mutations yet in this session.</p>
              ) : (
                operations.recentMutations.map((mutation) => (
                  <button key={mutation.id} type="button" className="table-button" onClick={() => selectOperation(mutation.id)}>
                    <div className="table-row">
                      <div className="table-primary">
                        <h4>{mutation.action}</h4>
                        <p>{mutation.resultIds.join(', ') || mutation.sessionId}</p>
                      </div>
                      <div className="table-metric">{mutation.durationMs} ms</div>
                      <div className={`table-status ${mutation.success ? 'ok' : 'error'}`}>
                        {mutation.success ? 'ok' : 'error'}
                      </div>
                    </div>
                  </button>
                ))
              )}
            </div>
          </article>

          <article className="panel">
            <div className="panel-header">
              <h3>Runtime</h3>
              <span>{summary.runtime.processes.length} processes</span>
            </div>
            <div className="runtime-list">
              {summary.runtime.processes.slice(0, 6).map((process) => (
                <div key={`${process.kind}-${process.pid}`} className="runtime-row">
                  <div>
                    <h4>{process.kind} #{process.pid}</h4>
                    <p>{process.elapsed}</p>
                  </div>
                  <div className="runtime-metric">{process.rssMb.toFixed(1)} MB</div>
                </div>
              ))}
            </div>
            <p className="runtime-footnote">
              Last runtime event: {summary.lastRuntimeEvent?.message ?? 'No runtime events yet'}
            </p>
          </article>
        </section>
      </main>

      <OperationDrawer
        detail={selectedOperation}
        status={detailStatus}
        selectedId={selectedOperationId}
        onClose={() => {
          setSelectedOperationId(null)
          setSelectedOperation(null)
          setDetailStatus('idle')
        }}
      />
    </>
  )
}

function MetricCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}
