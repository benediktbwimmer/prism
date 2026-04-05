import type { PrismUiFleetBarView, PrismUiFleetLaneView, PrismUiFleetView } from '../../types'

type FleetTimelineProps = {
  fleet: PrismUiFleetView
  selectedTaskId: string | null
  onSelectTask: (taskId: string | null) => void
}

const MIN_BAR_WIDTH_PERCENT = 3

export function FleetTimeline({
  fleet,
  onSelectTask,
  selectedTaskId,
}: FleetTimelineProps) {
  const totalWindowSeconds = Math.max(1, fleet.windowEnd - fleet.windowStart)
  const ticks = buildTicks(fleet.windowStart, fleet.windowEnd, 6)

  return (
    <section className="panel fleet-timeline-panel">
      <div className="panel-header fleet-panel-header">
        <div>
          <p className="eyebrow">Fleet Timeline</p>
          <h3>Lease activity across federated runtimes</h3>
        </div>
        <span>{formatWindowLabel(fleet.windowStart, fleet.windowEnd)}</span>
      </div>

      <div className="fleet-axis">
        <div className="fleet-axis-label">Runtime</div>
        <div className="fleet-axis-track">
          {ticks.map((tick) => (
            <div
              key={tick.label}
              className="fleet-axis-tick"
              style={{ left: `${tick.offsetPercent}%` }}
            >
              <span>{tick.label}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="fleet-timeline-grid">
        {fleet.lanes.map((lane) => (
          <FleetLaneRow
            key={lane.id}
            bars={fleet.bars.filter((bar) => bar.laneId === lane.id)}
            lane={lane}
            selectedTaskId={selectedTaskId}
            totalWindowSeconds={totalWindowSeconds}
            windowEnd={fleet.windowEnd}
            windowStart={fleet.windowStart}
            onSelectTask={onSelectTask}
          />
        ))}
      </div>
    </section>
  )
}

function FleetLaneRow({
  bars,
  lane,
  onSelectTask,
  selectedTaskId,
  totalWindowSeconds,
  windowEnd,
  windowStart,
}: {
  bars: PrismUiFleetBarView[]
  lane: PrismUiFleetLaneView
  selectedTaskId: string | null
  totalWindowSeconds: number
  windowStart: number
  windowEnd: number
  onSelectTask: (taskId: string | null) => void
}) {
  const sortedBars = [...bars].sort((left, right) => left.startedAt - right.startedAt)

  return (
    <div className="fleet-lane-row">
      <div className="fleet-lane-meta">
        <p className="operation-kind">{lane.discoveryMode ?? 'runtime'}</p>
        <h4>{lane.label}</h4>
        <p>{lane.branchRef ?? lane.worktreeId ?? 'No branch metadata'}</p>
        <div className="fleet-lane-meta-badges">
          {lane.idle ? <span className="table-status">idle</span> : null}
          {lane.staleBarCount > 0 ? <span className="table-status error">stale</span> : null}
          <span className="table-status">{lane.activeBarCount} active</span>
        </div>
      </div>

      <div className="fleet-lane-track">
        {sortedBars.length === 0 ? (
          <div className="fleet-track-empty">No leases in the visible time window.</div>
        ) : null}

        {sortedBars.map((bar) => {
          const endAt = bar.endedAt ?? windowEnd
          const clampedStart = Math.max(windowStart, bar.startedAt)
          const clampedEnd = Math.min(windowEnd, endAt)
          const leftPercent = ((clampedStart - windowStart) / totalWindowSeconds) * 100
          const rawWidthPercent = ((clampedEnd - clampedStart) / totalWindowSeconds) * 100
          const widthPercent = Math.max(MIN_BAR_WIDTH_PERCENT, rawWidthPercent)
          const selected = Boolean(selectedTaskId && bar.taskId === selectedTaskId)

          return (
            <button
              key={bar.id}
              type="button"
              className={[
                'fleet-timeline-bar',
                bar.active ? 'fleet-timeline-bar-active' : '',
                bar.stale ? 'fleet-timeline-bar-stale' : '',
                selected ? 'fleet-timeline-bar-selected' : '',
              ].filter(Boolean).join(' ')}
              style={{
                left: `${leftPercent}%`,
                width: `${Math.min(100 - leftPercent, widthPercent)}%`,
              }}
              onClick={() => onSelectTask(bar.taskId ?? null)}
            >
              <strong>{bar.taskTitle}</strong>
              <span>{formatBarMeta(bar)}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function buildTicks(windowStart: number, windowEnd: number, count: number) {
  const total = Math.max(1, windowEnd - windowStart)
  return Array.from({ length: count + 1 }, (_, index) => {
    const ts = windowStart + Math.round((total * index) / count)
    return {
      label: new Date(ts * 1000).toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit',
      }),
      offsetPercent: (index / count) * 100,
    }
  })
}

function formatWindowLabel(windowStart: number, windowEnd: number) {
  const start = new Date(windowStart * 1000)
  const end = new Date(windowEnd * 1000)
  return `${start.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })} - ${end.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
}

function formatBarMeta(bar: PrismUiFleetBarView) {
  const duration = formatDuration(bar.durationSeconds)
  const holder = bar.holder ?? bar.agent ?? 'unknown holder'
  return `${duration} · ${holder}`
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
