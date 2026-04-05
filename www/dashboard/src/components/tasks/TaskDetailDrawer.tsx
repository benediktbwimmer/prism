import type { ReactNode } from 'react'

import { useTaskDetail } from '../../hooks/useTaskDetail'

type TaskDetailDrawerProps = {
  taskId: string | null
  onClose: () => void
}

export function TaskDetailDrawer({ taskId, onClose }: TaskDetailDrawerProps) {
  const { detail, status } = useTaskDetail(taskId)
  const open = Boolean(taskId)

  return (
    <aside className={`operation-drawer task-detail-drawer ${open ? 'open' : ''}`}>
      <div className="drawer-header">
        <div>
          <p className="eyebrow">Task Detail</p>
          <h2>{detail?.task.title ?? taskId ?? 'Nothing selected'}</h2>
        </div>
        <button type="button" className="ghost-button" onClick={onClose}>
          Close
        </button>
      </div>

      {!open ? (
        <p className="drawer-empty">Select a concrete task node to inspect its full execution context.</p>
      ) : null}

      {open && status === 'loading' && !detail ? (
        <p className="drawer-empty">Loading task detail.</p>
      ) : null}

      {open && status === 'error' ? (
        <p className="drawer-empty">The task detail request failed. The polling loop will retry automatically.</p>
      ) : null}

      {detail ? (
        <div className="drawer-section">
          <section className="detail-block task-detail-hero">
            <p className="eyebrow">{formatLabel(detail.task.kind ?? 'task')}</p>
            <h3>{detail.task.title}</h3>
            <p>{detail.editable.description ?? detail.task.summary ?? 'No authored description is attached to this task yet.'}</p>
            <div className="drawer-metrics">
              <Metric label="Status" value={formatLabel(detail.task.status)} />
              <Metric label="Priority" value={detail.editable.priority?.toString() ?? 'unset'} />
              <Metric label="Assignee" value={detail.task.assignee ?? 'unassigned'} />
              <Metric label="Blockers" value={String(detail.blockers.length)} />
            </div>
          </section>

          <ReadOnlySection
            empty="No claim history has been recorded for this task yet."
            title="Claim History"
          >
            {detail.claimHistory.map((claim) => (
              <article key={claim.id} className="task-detail-row">
                <div>
                  <strong>{claim.holder}</strong>
                  <p>{formatLabel(claim.status)} · {formatLabel(claim.capability)}</p>
                </div>
                <div className="task-detail-row-meta">
                  <span>{formatDuration(claim.durationSeconds)}</span>
                  <span>{claim.branchRef ?? claim.worktreeId ?? 'no branch'}</span>
                </div>
              </article>
            ))}
          </ReadOnlySection>

          <ReadOnlySection
            empty="No validation outcomes are attached to this task yet."
            title="Outcomes"
          >
            {detail.outcomes.map((outcome) => (
              <article key={`${outcome.ts}-${outcome.summary}`} className="task-detail-row">
                <div>
                  <strong>{outcome.summary}</strong>
                  <p>{formatLabel(outcome.kind)} · {formatLabel(outcome.result)}</p>
                </div>
                <div className="task-detail-row-meta">
                  <span>{formatTimestamp(outcome.ts)}</span>
                </div>
              </article>
            ))}
          </ReadOnlySection>

          <ReadOnlySection
            empty="No recent commits are associated with this task yet."
            title="Recent Commits"
          >
            {detail.recentCommits.map((commit) => (
              <article key={`${commit.kind}-${commit.commit}`} className="task-detail-row">
                <div>
                  <strong>{commit.label}</strong>
                  <p>{formatLabel(commit.kind)} · {shortCommit(commit.commit)}</p>
                </div>
                <div className="task-detail-row-meta">
                  <span>{commit.reference ?? 'local lineage'}</span>
                </div>
              </article>
            ))}
          </ReadOnlySection>

          <ReadOnlySection
            empty="This task is not currently blocked by any upstream dependency."
            title="Blockers"
          >
            {detail.blockers.map((entry) => (
              <article
                key={`${entry.blocker.summary}-${entry.relatedTask?.id ?? 'none'}`}
                className="task-detail-row"
              >
                <div>
                  <strong>{entry.blocker.summary}</strong>
                  <p>{formatLabel(entry.blocker.kind)}</p>
                </div>
                <div className="task-detail-row-meta">
                  <span>{entry.relatedTask?.title ?? entry.relatedTask?.id ?? 'external blocker'}</span>
                  <span>{entry.relatedTask ? formatLabel(entry.relatedTask.status) : 'unresolved'}</span>
                </div>
              </article>
            ))}
          </ReadOnlySection>
        </div>
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

function ReadOnlySection({
  children,
  empty,
  title,
}: {
  children: ReactNode
  empty: string
  title: string
}) {
  const items = Array.isArray(children) ? children : [children]
  return (
    <section className="detail-block">
      <h3>{title}</h3>
      <div className="task-detail-list">
        {items.filter(Boolean).length > 0 ? children : <p>{empty}</p>}
      </div>
    </section>
  )
}

function formatLabel(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase())
}

function formatDuration(durationSeconds: number | null | undefined) {
  if (!durationSeconds) {
    return 'n/a'
  }
  if (durationSeconds < 60) {
    return `${durationSeconds}s`
  }
  if (durationSeconds < 3600) {
    return `${Math.round(durationSeconds / 60)}m`
  }
  return `${(durationSeconds / 3600).toFixed(1)}h`
}

function formatTimestamp(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleString()
}

function shortCommit(commit: string) {
  return commit.slice(0, 8)
}
