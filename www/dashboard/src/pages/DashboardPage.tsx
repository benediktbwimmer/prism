import { LatencySparkline } from '../components/LatencySparkline'
import { OperationDrawer } from '../components/OperationDrawer'
import { useDashboardData } from '../hooks/useDashboardData'

type DashboardPageProps = ReturnType<typeof useDashboardData> & {
  search: string
}

export function DashboardPage({
  clearSelectedOperation,
  connection,
  dashboard,
  detailStatus,
  selectOperation,
  search,
  selectedOperation,
  selectedOperationId,
}: DashboardPageProps) {
  const focusSection = new URLSearchParams(search).get('section')

  if (!dashboard) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">PRISM Dashboard</p>
        <h1>Connecting to the daemon</h1>
        <p>Loading the first dashboard snapshot and subscribing to live events.</p>
      </section>
    )
  }

  const { summary, operations, task, coordination } = dashboard

  return (
    <>
      <div className="page-stack dashboard-page">
        <section className="hero-bar panel page-hero">
          <div>
            <p className="eyebrow">PRISM Dashboard</p>
            <h2>Live server activity</h2>
            <p className="lede">
              {summary.session.workspaceRoot ?? 'Unknown workspace'}
            </p>
            {focusSection ? (
              <p className="focus-note">Focused section: {focusSection}</p>
            ) : null}
          </div>
          <div className="hero-actions">
            <span className={`connection-pill connection-${connection}`}>{connection}</span>
          </div>
        </section>

        <section className="status-grid">
          <article className="panel stat-card">
            <p className="stat-label">Health</p>
            <h3>{summary.runtime.health.ok ? 'Healthy' : 'Degraded'}</h3>
            <p>{summary.runtime.health.detail}</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Processes</p>
            <h3>{summary.runtime.daemonCount} daemon / {summary.runtime.bridgeCount} bridges</h3>
            <p>Shared live state across attached agent sessions.</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Active Operations</p>
            <h3>{summary.activeQueryCount} queries / {summary.activeMutationCount} mutations</h3>
            <p>Streaming over SSE with replay support.</p>
          </article>
          <article className="panel stat-card">
            <p className="stat-label">Current Task</p>
            <h3>{summary.session.currentTask?.description ?? 'No active session task'}</h3>
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
      </div>

      <OperationDrawer
        detail={selectedOperation}
        status={detailStatus}
        selectedId={selectedOperationId}
        onClose={clearSelectedOperation}
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
