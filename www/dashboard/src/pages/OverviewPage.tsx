import type { DashboardBootstrapView } from '../types'

type OverviewPageProps = {
  dashboard: DashboardBootstrapView | null
  connection: 'connecting' | 'open' | 'closed'
  onNavigate: (path: string) => void
}

export function OverviewPage({ dashboard, connection, onNavigate }: OverviewPageProps) {
  if (!dashboard) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">PRISM Overview</p>
        <h2>Connecting the control plane</h2>
        <p>Loading the live runtime snapshot so the overview can summarize work, coordination, and system health.</p>
      </section>
    )
  }

  const { summary, operations, task, coordination } = dashboard

  return (
    <div className="page-stack">
      <section className="hero-bar panel overview-hero">
        <div>
          <p className="eyebrow">PRISM Overview</p>
          <h2>Start from the system, then drill into the surface you need.</h2>
          <p className="lede">
            {summary.session.workspaceRoot ?? 'Unknown workspace'}
          </p>
        </div>
        <div className="hero-actions">
          <span className={`connection-pill connection-${connection}`}>{connection}</span>
        </div>
      </section>

      <section className="overview-grid">
        <article className="panel stat-card">
          <p className="stat-label">System Health</p>
          <h3>{summary.runtime.health.ok ? 'Healthy' : 'Needs attention'}</h3>
          <p>{summary.runtime.health.detail}</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Current Intent</p>
          <h3>{task.session.currentTask?.description ?? 'No active session task'}</h3>
          <p>{task.session.currentTask?.taskId ?? 'Session is idle'}</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Coordination</p>
          <h3>{coordination.activePlanCount} plans / {coordination.readyTaskCount} ready</h3>
          <p>{coordination.activeClaimCount} active claims and {coordination.pendingReviewCount} pending reviews.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Live Runtime</p>
          <h3>{summary.activeQueryCount + summary.activeMutationCount} active operations</h3>
          <p>{summary.runtime.daemonCount} daemon processes and {summary.recentQueryErrorCount} recent query errors.</p>
        </article>
      </section>

      <section className="route-grid">
        <RouteCard
          eyebrow="Operational"
          title="Dashboard"
          description="Inspect live MCP activity, runtime refreshes, task focus, and operation traces."
          metric={`${operations.active.length} active`}
          path="/dashboard"
          onNavigate={onNavigate}
        />
        <RouteCard
          eyebrow="Execution"
          title="Plans"
          description="Track blockers, ready nodes, handoffs, validations, and human interventions."
          metric={`${coordination.activePlanCount} plans`}
          path="/plans"
          onNavigate={onNavigate}
        />
        <RouteCard
          eyebrow="Architecture"
          title="Graph"
          description="Explore subsystems, typed relations, evidence, and future overlays."
          metric={`${coordination.taskCount} linked tasks`}
          path="/graph"
          onNavigate={onNavigate}
        />
      </section>

      <section className="content-grid">
        <article className="panel callout-card">
          <div className="panel-header">
            <h3>Current Task</h3>
            <span>{task.journal?.active ? 'active' : 'context'}</span>
          </div>
          <div className="task-focus">
            <h4>{task.journal?.description ?? task.session.currentTask?.description ?? 'No current task'}</h4>
            <p className="operation-meta">{task.session.currentTask?.taskId ?? 'No session task is selected right now.'}</p>
            {task.journal ? (
              <div className="metric-grid">
                <OverviewMetric label="Tests" value={task.journal.summary.testCount} />
                <OverviewMetric label="Patches" value={task.journal.summary.patchCount} />
                <OverviewMetric label="Failures" value={task.journal.summary.failureCount} />
                <OverviewMetric label="Validation" value={task.journal.summary.validationCount} />
              </div>
            ) : (
              <p className="empty-state">Attach a task to surface journal state, validations, and outcomes here.</p>
            )}
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Recent Signals</h3>
            <span>{operations.recentQueries.length + operations.recentMutations.length}</span>
          </div>
          <div className="signal-list">
            {operations.recentQueries.slice(0, 3).map((query) => (
              <div key={query.id} className="signal-row">
                <div>
                  <p className="operation-kind">query</p>
                  <h4>{query.querySummary}</h4>
                </div>
                <span className="runtime-metric">{query.durationMs} ms</span>
              </div>
            ))}
            {operations.recentMutations.slice(0, 2).map((mutation) => (
              <div key={mutation.id} className="signal-row">
                <div>
                  <p className="operation-kind">mutation</p>
                  <h4>{mutation.action}</h4>
                </div>
                <span className={`table-status ${mutation.success ? 'ok' : 'error'}`}>
                  {mutation.success ? 'ok' : 'error'}
                </span>
              </div>
            ))}
          </div>
        </article>
      </section>
    </div>
  )
}

type RouteCardProps = {
  eyebrow: string
  title: string
  description: string
  metric: string
  path: string
  onNavigate: (path: string) => void
}

function RouteCard({ eyebrow, title, description, metric, path, onNavigate }: RouteCardProps) {
  return (
    <article className="panel route-card">
      <p className="eyebrow">{eyebrow}</p>
      <h3>{title}</h3>
      <p>{description}</p>
      <div className="route-card-footer">
        <span className="metric-chip">
          <span>Now</span>
          <strong>{metric}</strong>
        </span>
        <button type="button" className="ghost-button" onClick={() => onNavigate(path)}>
          Open {title}
        </button>
      </div>
    </article>
  )
}

function OverviewMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}
