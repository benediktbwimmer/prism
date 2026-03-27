import {
  startTransition,
  useEffect,
  useEffectEvent,
  useState,
} from 'react'

import type {
  ActiveOperationView,
  DashboardBootstrapView,
  MutationLogEntryView,
  QueryLogEntryView,
  ThemeChoice,
} from './types'

const THEME_KEY = 'prism-dashboard-theme'

export function App() {
  const [dashboard, setDashboard] = useState<DashboardBootstrapView | null>(null)
  const [connection, setConnection] = useState<'connecting' | 'open' | 'closed'>('connecting')
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
    async function load() {
      const response = await fetch('/dashboard/api/bootstrap')
      const next = (await response.json()) as DashboardBootstrapView
      if (!cancelled) {
        startTransition(() => setDashboard(next))
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [])

  const refreshSummary = useEffectEvent(async () => {
    const response = await fetch('/dashboard/api/summary')
    const summary = (await response.json()) as DashboardBootstrapView['summary']
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
  })

  const handleActiveEvent = useEffectEvent((operation: ActiveOperationView) => {
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
  })

  const handleFinishedQuery = useEffectEvent((query: QueryLogEntryView) => {
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
  })

  const handleFinishedMutation = useEffectEvent((mutation: MutationLogEntryView) => {
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
  })

  useEffect(() => {
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
    source.addEventListener('runtime.refreshed', () => {
      void refreshSummary()
    })

    return () => {
      source.close()
    }
  }, [handleActiveEvent, handleFinishedMutation, handleFinishedQuery, refreshSummary])

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

  const { summary, operations } = dashboard
  const querySparkline = operations.recentQueries.slice(0, 12).reverse()

  return (
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
          <p>Current in-flight work visible through SSE.</p>
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
                <div key={operation.id} className="operation-row">
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
              ))
            )}
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Recent Query Latency</h3>
            <span>{operations.recentQueries.length}</span>
          </div>
          <div className="sparkline">
            {querySparkline.map((query) => (
              <span
                key={query.id}
                className={`spark-bar ${query.success ? 'spark-ok' : 'spark-error'}`}
                style={{ height: `${Math.max(10, Math.min(64, query.durationMs))}px` }}
                title={`${query.querySummary} (${query.durationMs} ms)`}
              />
            ))}
          </div>
          <p className="sparkline-caption">
            First-pass sparkline view. D3 latency histograms and richer traces can land next.
          </p>
        </article>

        <article className="panel wide-panel">
          <div className="panel-header">
            <h3>Recent Queries</h3>
            <span>{operations.recentQueries.length}</span>
          </div>
          <div className="table-list">
            {operations.recentQueries.map((query) => (
              <div key={query.id} className="table-row">
                <div className="table-primary">
                  <h4>{query.querySummary}</h4>
                  <p>{query.operations.join(', ') || query.kind}</p>
                </div>
                <div className="table-metric">{query.durationMs} ms</div>
                <div className={`table-status ${query.success ? 'ok' : 'error'}`}>
                  {query.success ? 'ok' : 'error'}
                </div>
              </div>
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
                <div key={mutation.id} className="table-row">
                  <div className="table-primary">
                    <h4>{mutation.action}</h4>
                    <p>{mutation.resultIds.join(', ') || mutation.sessionId}</p>
                  </div>
                  <div className="table-metric">{mutation.durationMs} ms</div>
                  <div className={`table-status ${mutation.success ? 'ok' : 'error'}`}>
                    {mutation.success ? 'ok' : 'error'}
                  </div>
                </div>
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
  )
}
