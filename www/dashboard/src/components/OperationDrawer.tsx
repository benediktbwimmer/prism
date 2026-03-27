import type { DashboardOperationDetailView } from '../types'

type Props = {
  detail: DashboardOperationDetailView | null
  status: 'idle' | 'loading' | 'error'
  selectedId: string | null
  onClose: () => void
}

export function OperationDrawer({ detail, status, selectedId, onClose }: Props) {
  const open = Boolean(selectedId)

  return (
    <aside className={`operation-drawer ${open ? 'open' : ''}`}>
      <div className="drawer-header">
        <div>
          <p className="eyebrow">Operation Detail</p>
          <h2>{selectedId ?? 'Nothing selected'}</h2>
        </div>
        <button type="button" className="ghost-button" onClick={onClose}>
          Close
        </button>
      </div>

      {!open ? (
        <p className="drawer-empty">Select an active operation, query, or mutation to inspect it.</p>
      ) : null}

      {open && status === 'loading' ? (
        <p className="drawer-empty">Loading operation detail.</p>
      ) : null}

      {open && status === 'error' ? (
        <p className="drawer-empty">The operation detail request failed.</p>
      ) : null}

      {detail?.kind === 'active' ? (
        <section className="drawer-section">
          <div className="drawer-metrics">
            <Metric label="Kind" value={detail.operation.kind} />
            <Metric label="Status" value={detail.operation.phase ?? detail.operation.status} />
            <Metric label="Session" value={detail.operation.sessionId} />
            <Metric label="Task" value={detail.operation.taskId ?? 'none'} />
          </div>
          <ListSection title="Touched" values={detail.operation.touched} empty="No touched anchors yet." />
          {detail.operation.error ? (
            <div className="detail-block detail-error">
              <h3>Error</h3>
              <p>{detail.operation.error}</p>
            </div>
          ) : null}
        </section>
      ) : null}

      {detail?.kind === 'query' ? (
        <section className="drawer-section">
          <div className="drawer-metrics">
            <Metric label="Duration" value={`${detail.trace.entry.durationMs} ms`} />
            <Metric label="Result" value={detail.trace.entry.result.kind} />
            <Metric label="Task" value={detail.trace.entry.taskId ?? 'none'} />
            <Metric label="Session" value={detail.trace.entry.sessionId} />
          </div>
          <div className="detail-block">
            <h3>Query</h3>
            <pre>{detail.trace.entry.queryText}</pre>
          </div>
          <div className="detail-block">
            <h3>Phases</h3>
            {detail.trace.phases.length === 0 ? (
              <p>No phase trace captured.</p>
            ) : (
              <div className="phase-list">
                {detail.trace.phases.map((phase) => (
                  <article key={`${phase.operation}-${phase.startedAt}`} className="phase-card">
                    <div className="phase-header">
                      <strong>{phase.operation}</strong>
                      <span>{phase.durationMs} ms</span>
                    </div>
                    <p>{phase.success ? 'ok' : 'error'}</p>
                    {phase.touched.length ? <p>{phase.touched.join(', ')}</p> : null}
                    {phase.argsSummary ? <pre>{JSON.stringify(phase.argsSummary, null, 2)}</pre> : null}
                    {phase.error ? <p className="detail-error-text">{phase.error}</p> : null}
                  </article>
                ))}
              </div>
            )}
          </div>
          <ListSection
            title="Diagnostics"
            values={detail.trace.entry.diagnostics.map((diagnostic) => `${diagnostic.code}: ${diagnostic.message}`)}
            empty="No diagnostics."
          />
        </section>
      ) : null}

      {detail?.kind === 'mutation' ? (
        <section className="drawer-section">
          <div className="drawer-metrics">
            <Metric label="Action" value={detail.trace.entry.action} />
            <Metric label="Duration" value={`${detail.trace.entry.durationMs} ms`} />
            <Metric label="Violations" value={String(detail.trace.entry.violationCount)} />
            <Metric label="Task" value={detail.trace.entry.taskId ?? 'none'} />
          </div>
          <ListSection
            title="Result IDs"
            values={detail.trace.entry.resultIds}
            empty="No result identifiers were recorded."
          />
          {detail.trace.entry.error ? (
            <div className="detail-block detail-error">
              <h3>Error</h3>
              <p>{detail.trace.entry.error}</p>
            </div>
          ) : null}
          <div className="detail-block">
            <h3>Result Payload</h3>
            <pre>{JSON.stringify(detail.trace.result, null, 2)}</pre>
          </div>
        </section>
      ) : null}
    </aside>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-chip">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function ListSection({
  title,
  values,
  empty,
}: {
  title: string
  values: string[]
  empty: string
}) {
  return (
    <div className="detail-block">
      <h3>{title}</h3>
      {values.length === 0 ? (
        <p>{empty}</p>
      ) : (
        <ul className="detail-list">
          {values.map((value) => (
            <li key={value}>{value}</li>
          ))}
        </ul>
      )}
    </div>
  )
}
