import { useOverviewData } from '../hooks/useOverviewData'
import type { DashboardBootstrapView } from '../types'

type OverviewPageProps = {
  dashboard: DashboardBootstrapView | null
  connection: 'connecting' | 'open' | 'closed'
  search: string
  onNavigate: (path: string) => void
}

export function OverviewPage({ dashboard, connection, onNavigate }: OverviewPageProps) {
  const overview = useOverviewData()
  const activeOverview = overview ?? (dashboard ? {
    summary: dashboard.summary,
    task: dashboard.task,
    coordination: dashboard.coordination,
    planSignals: {
      blockedNodes: 0,
      reviewGatedNodes: dashboard.coordination.inReviewTaskCount,
      validationGatedNodes: 0,
      claimConflictedNodes: 0,
    },
    spotlightPlans: [],
    hotConcepts: [],
    recentOutcomes: [],
    pendingHandoffs: [],
  } : null)

  if (!activeOverview) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">PRISM Overview</p>
        <h2>Connecting the control plane</h2>
        <p>Loading the live runtime snapshot so the overview can summarize work, coordination, and system health.</p>
      </section>
    )
  }

  const { summary, task, coordination, planSignals, spotlightPlans, hotConcepts, recentOutcomes, pendingHandoffs } = activeOverview
  const operations = dashboard?.operations

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
          <p className="stat-label">Execution Pressure</p>
          <h3>{planSignals.blockedNodes} blocked / {coordination.readyTaskCount} ready</h3>
          <p>{planSignals.reviewGatedNodes} review-gated, {planSignals.validationGatedNodes} validation-gated.</p>
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
          metric={`${operations?.active.length ?? 0} active`}
          path="/dashboard?section=operations"
          onNavigate={onNavigate}
        />
        <RouteCard
          eyebrow="Execution"
          title="Plans"
          description="Track blockers, ready nodes, handoffs, validations, and human interventions."
          metric={`${coordination.activePlanCount} plans`}
          path={spotlightPlans[0] ? `/plans?plan=${encodeURIComponent(spotlightPlans[0].planId)}` : '/plans'}
          onNavigate={onNavigate}
        />
        <RouteCard
          eyebrow="Architecture"
          title="Graph"
          description="Explore subsystems, typed relations, evidence, and future overlays."
          metric={`${coordination.taskCount} linked tasks`}
          path={hotConcepts[0] ? `/graph?concept=${encodeURIComponent(hotConcepts[0].handle)}` : '/graph'}
          onNavigate={onNavigate}
        />
      </section>

      <section className="spotlight-grid">
        {spotlightPlans.map((plan) => (
          <article key={plan.planId} className="panel spotlight-card">
            <div className="panel-header">
              <h3>{plan.title}</h3>
              <span>{plan.summary.actionableNodes} ready</span>
            </div>
            <div className="panel-body spotlight-body">
              <p>{plan.goal}</p>
              <div className="metric-grid spotlight-metrics">
                <OverviewMetric label="Blocked" value={plan.summary.executionBlockedNodes} />
                <OverviewMetric label="In Progress" value={plan.summary.inProgressNodes} />
                <OverviewMetric label="Completed" value={plan.summary.completedNodes} />
                <OverviewMetric label="Review" value={plan.summary.reviewGatedNodes} />
              </div>
              <div className="spotlight-next">
                <h4>Next nodes</h4>
                {plan.nextNodes.length === 0 ? (
                  <p className="empty-state">No recommended next nodes yet.</p>
                ) : (
                  plan.nextNodes.map((node) => (
                    <button
                      key={node.node.id}
                      type="button"
                      className="table-button spotlight-next-button"
                      onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(plan.planId)}`)}
                    >
                      <div className="table-row">
                        <div className="table-primary">
                          <h4>{node.node.title}</h4>
                          <p>{node.reasons[0] ?? node.node.status}</p>
                        </div>
                        <div className="table-status ok">{node.node.status}</div>
                      </div>
                    </button>
                  ))
                )}
              </div>
            </div>
          </article>
        ))}
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
            <button type="button" className="ghost-button" onClick={() => onNavigate('/dashboard?section=task')}>
              Inspect task in dashboard
            </button>
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Recent Outcomes</h3>
            <span>{recentOutcomes.length}</span>
          </div>
          <div className="signal-list">
            {recentOutcomes.length === 0 ? (
              <p className="panel-body empty-state">No recent outcomes recorded yet.</p>
            ) : recentOutcomes.map((outcome) => (
              <div key={`${outcome.ts}-${outcome.summary}`} className="signal-row">
                <div>
                  <p className="operation-kind">{outcome.kind}</p>
                  <h4>{outcome.summary}</h4>
                </div>
                <span className="runtime-metric">{outcome.result}</span>
              </div>
            ))}
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Hot Concepts</h3>
            <span>{hotConcepts.length}</span>
          </div>
          <div className="panel-body concept-list">
            {hotConcepts.length === 0 ? (
              <p className="empty-state">No concept-linked active work yet.</p>
            ) : hotConcepts.map((concept) => (
              <button
                key={concept.handle}
                type="button"
                className="concept-button"
                onClick={() => onNavigate(`/graph?concept=${encodeURIComponent(concept.handle)}`)}
              >
                <strong>{concept.canonicalName}</strong>
                <span>{concept.summary}</span>
              </button>
            ))}
          </div>
        </article>

        <article className="panel">
          <div className="panel-header">
            <h3>Coordination Queue</h3>
            <span>{pendingHandoffs.length}</span>
          </div>
          <div className="panel-body">
            {pendingHandoffs.length === 0 ? (
              <p className="empty-state">No pending handoffs.</p>
            ) : pendingHandoffs.map((taskItem) => (
              <button
                key={taskItem.id}
                type="button"
                className="table-button"
                onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(taskItem.planId)}`)}
              >
                <div className="table-row">
                  <div className="table-primary">
                    <h4>{taskItem.title}</h4>
                    <p>{taskItem.pendingHandoffTo ?? taskItem.status}</p>
                  </div>
                  <div className="table-metric">{taskItem.planId}</div>
                </div>
              </button>
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
