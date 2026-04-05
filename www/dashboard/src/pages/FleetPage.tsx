import { useFleetData } from '../hooks/useFleetData'

type FleetPageProps = {
  onNavigate: (path: string) => void
}

export function FleetPage({ onNavigate }: FleetPageProps) {
  const fleet = useFleetData()

  if (!fleet) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">PRISM Fleet</p>
        <h2>Loading runtime utilization</h2>
        <p>Fetching the live runtime lanes and active task leases.</p>
      </section>
    )
  }

  const activeBars = fleet.bars.filter((bar) => bar.active)
  const staleBars = fleet.bars.filter((bar) => bar.stale)
  const idleLanes = fleet.lanes.filter((lane) => lane.idle)

  return (
    <div className="page-stack">
      <section className="hero-bar panel operator-hero">
        <div>
          <p className="eyebrow">Utilization</p>
          <h2>Runtime lanes and task leases</h2>
          <p className="lede">
            Track who is active, who is stale, and which runtimes have gone idle.
          </p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{fleet.lanes.length} runtimes</span>
          <span className="connection-pill">{activeBars.length} active leases</span>
          <span className="connection-pill">{staleBars.length} stale</span>
        </div>
      </section>

      <section className="status-grid">
        <article className="panel stat-card">
          <p className="stat-label">Active Lanes</p>
          <h3>{fleet.lanes.length - idleLanes.length}</h3>
          <p>Runtimes with current lease activity inside the observation window.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Idle Lanes</p>
          <h3>{idleLanes.length}</h3>
          <p>Runtimes with no visible task bars in the current utilization window.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Active Bars</p>
          <h3>{activeBars.length}</h3>
          <p>Leases still stretching to now and therefore worth watching for stalls.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Stale Bars</p>
          <h3>{staleBars.length}</h3>
          <p>Claims whose freshness window has elapsed and may need human intervention.</p>
        </article>
      </section>

      <section className="fleet-layout">
        <article className="panel fleet-lanes-panel">
          <div className="panel-header">
            <h3>Runtime Lanes</h3>
            <span>{fleet.lanes.length}</span>
          </div>
          <div className="panel-body fleet-lanes">
            {fleet.lanes.map((lane) => {
              const laneBars = fleet.bars.filter((bar) => bar.laneId === lane.id)
              return (
                <section key={lane.id} className="fleet-lane-card">
                  <div className="fleet-lane-header">
                    <div>
                      <p className="operation-kind">{lane.discoveryMode ?? 'runtime'}</p>
                      <h4>{lane.label}</h4>
                      <p className="operation-meta">
                        {lane.branchRef ?? lane.worktreeId ?? 'No branch metadata'}
                      </p>
                    </div>
                    <div className="fleet-lane-badges">
                      {lane.idle ? <span className="table-status">idle</span> : null}
                      {lane.staleBarCount > 0 ? <span className="table-status error">stale</span> : null}
                    </div>
                  </div>
                  <div className="fleet-lane-strip">
                    {laneBars.length === 0 ? (
                      <div className="fleet-strip-empty">No task leases in the current time window.</div>
                    ) : (
                      laneBars.map((bar) => (
                        <button
                          key={bar.id}
                          type="button"
                          className={[
                            'fleet-bar-chip',
                            bar.active ? 'fleet-bar-active' : '',
                            bar.stale ? 'fleet-bar-stale' : '',
                          ].filter(Boolean).join(' ')}
                          onClick={() => {
                            if (bar.taskId) {
                              onNavigate(`/plans?task=${encodeURIComponent(bar.taskId)}`)
                            }
                          }}
                        >
                          <strong>{bar.taskTitle}</strong>
                          <span>{formatDuration(bar.durationSeconds)}</span>
                        </button>
                      ))
                    )}
                  </div>
                </section>
              )
            })}
          </div>
        </article>

        <aside className="panel fleet-inspector-panel">
          <div className="panel-header">
            <h3>Utilization Notes</h3>
            <span>v1</span>
          </div>
          <div className="panel-body signal-list">
            <div className="signal-row">
              <div>
                <p className="operation-kind">Focus</p>
                <h4>Watch continuous bars</h4>
              </div>
              <span className="runtime-metric">{activeBars.length}</span>
            </div>
            <div className="signal-row">
              <div>
                <p className="operation-kind">Idle detection</p>
                <h4>Empty swimlanes stand out immediately</h4>
              </div>
              <span className="runtime-metric">{idleLanes.length}</span>
            </div>
            <div className="signal-row">
              <div>
                <p className="operation-kind">Next step</p>
                <h4>Full horizontal gantt layout lands in the fleet implementation task</h4>
              </div>
              <span className="runtime-metric">pending</span>
            </div>
          </div>
        </aside>
      </section>
    </div>
  )
}

function formatDuration(durationSeconds: number | null | undefined) {
  if (!durationSeconds || durationSeconds < 60) {
    return `${durationSeconds ?? 0}s`
  }
  if (durationSeconds < 3600) {
    return `${Math.round(durationSeconds / 60)}m`
  }
  return `${(durationSeconds / 3600).toFixed(1)}h`
}
