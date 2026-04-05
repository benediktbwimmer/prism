import { TaskDetailDrawer } from '../components/tasks/TaskDetailDrawer'
import { FleetTimeline } from '../components/fleet/FleetTimeline'
import { useFleetData } from '../hooks/useFleetData'

type FleetPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function FleetPage({ search, onNavigate }: FleetPageProps) {
  const fleet = useFleetData()
  const query = new URLSearchParams(search)
  const selectedTaskId = query.get('task')

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

  function navigateWithPatch(patch: Record<string, string | null>) {
    const next = new URLSearchParams(search)
    for (const [key, value] of Object.entries(patch)) {
      if (value && value.trim().length > 0) {
        next.set(key, value)
      } else {
        next.delete(key)
      }
    }
    const serialized = next.toString()
    onNavigate(serialized ? `/fleet?${serialized}` : '/fleet')
  }

  return (
    <div className="page-stack">
      <section className="hero-bar panel operator-hero">
        <div>
          <p className="eyebrow">Utilization</p>
          <h2>Runtime lanes and task leases</h2>
          <p className="lede">
            Spot stalled agents, idle runtimes, and long-running claims without leaving the operator console.
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

      <div className="fleet-console">
        <FleetTimeline
          fleet={fleet}
          selectedTaskId={selectedTaskId}
          onSelectTask={(taskId) => navigateWithPatch({ task: taskId })}
        />

        <aside className="panel fleet-inspector-panel">
          <div className="panel-header">
            <h3>Operational Cues</h3>
            <span>{selectedTaskId ? 'task selected' : 'live'}</span>
          </div>
          <div className="panel-body signal-list">
            <div className="signal-row">
              <div>
                <p className="operation-kind">Stuck detection</p>
                <h4>Long bars deserve human attention first</h4>
              </div>
              <span className="runtime-metric">{staleBars.length}</span>
            </div>
            <div className="signal-row">
              <div>
                <p className="operation-kind">Idle capacity</p>
                <h4>Empty lanes indicate available execution slots</h4>
              </div>
              <span className="runtime-metric">{idleLanes.length}</span>
            </div>
            <div className="signal-row">
              <div>
                <p className="operation-kind">Task drill-down</p>
                <h4>Click any bar to open the same task intervention drawer</h4>
              </div>
              <span className="runtime-metric">{selectedTaskId ? 'open' : 'ready'}</span>
            </div>
          </div>
        </aside>
      </div>

      <TaskDetailDrawer
        taskId={selectedTaskId}
        onClose={() => navigateWithPatch({ task: null })}
      />
    </div>
  )
}
