import { area, curveMonotoneX, line, max, scaleLinear } from 'd3'

import type { QueryLogEntryView } from '../types'

const WIDTH = 320
const HEIGHT = 108
const PADDING_X = 10
const PADDING_Y = 12

type Props = {
  queries: QueryLogEntryView[]
}

export function LatencySparkline({ queries }: Props) {
  if (queries.length === 0) {
    return <p className="empty-state chart-empty">No completed queries yet.</p>
  }

  const ordered = [...queries].slice(0, 18).reverse()
  const yMax = Math.max(max(ordered, (query: QueryLogEntryView) => query.durationMs) ?? 1, 1)
  const xScale = scaleLinear()
    .domain([0, Math.max(ordered.length - 1, 1)])
    .range([PADDING_X, WIDTH - PADDING_X])
  const yScale = scaleLinear()
    .domain([0, yMax])
    .range([HEIGHT - PADDING_Y, PADDING_Y])

  const latencyArea = area<QueryLogEntryView>()
    .x((_: QueryLogEntryView, index: number) => xScale(index))
    .y0(() => HEIGHT - PADDING_Y)
    .y1((query) => yScale(query.durationMs))
    .curve(curveMonotoneX)

  const latencyLine = line<QueryLogEntryView>()
    .x((_: QueryLogEntryView, index: number) => xScale(index))
    .y((query) => yScale(query.durationMs))
    .curve(curveMonotoneX)

  return (
    <div className="latency-chart">
      <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label="Recent query latency sparkline">
        <path className="latency-area" d={latencyArea(ordered) ?? ''} />
        <path className="latency-line" d={latencyLine(ordered) ?? ''} />
        {ordered.map((query, index) => (
          <circle
            key={query.id}
            className={query.success ? 'latency-point' : 'latency-point latency-point-error'}
            cx={xScale(index)}
            cy={yScale(query.durationMs)}
            r={3.5}
          >
            <title>
              {query.querySummary} • {query.durationMs} ms
            </title>
          </circle>
        ))}
      </svg>
      <div className="chart-scale">
        <span>0 ms</span>
        <span>{Math.round(yMax)} ms</span>
      </div>
    </div>
  )
}
