import type { ReactNode } from 'react'
import { useEffect, useMemo, useState } from 'react'

import { useTaskDetail } from '../../hooks/useTaskDetail'
import { useUiMutationQueue } from '../../hooks/useUiMutationQueue'
import type { PrismUiTaskDetailView, ValidationRefView } from '../../types'

type TaskDetailDrawerProps = {
  taskId: string | null
  onClose: () => void
}

export function TaskDetailDrawer({ taskId, onClose }: TaskDetailDrawerProps) {
  const { detail, status } = useTaskDetail(taskId)
  const {
    pendingActions,
    queueMutation,
    resolvePendingAction,
  } = useUiMutationQueue()
  const open = Boolean(taskId)
  const [titleDraft, setTitleDraft] = useState('')
  const [descriptionDraft, setDescriptionDraft] = useState('')
  const [priorityDraft, setPriorityDraft] = useState('')
  const [statusDraft, setStatusDraft] = useState('')
  const [validationDraft, setValidationDraft] = useState('')
  const [mutationError, setMutationError] = useState<string | null>(null)

  const taskPendingActions = useMemo(
    () => pendingActions.filter((action) => action.taskId === taskId),
    [pendingActions, taskId],
  )
  const drawerSyncing = taskPendingActions.length > 0

  useEffect(() => {
    if (!detail || drawerSyncing) {
      return
    }
    setTitleDraft(detail.editable.title)
    setDescriptionDraft(detail.editable.description ?? '')
    setPriorityDraft(detail.editable.priority?.toString() ?? '')
    setStatusDraft(detail.editable.status.toLowerCase())
    setValidationDraft(detail.editable.validationRefs.map((ref) => ref.id).join('\n'))
  }, [detail, drawerSyncing])

  useEffect(() => {
    if (!detail) {
      return
    }
    for (const action of taskPendingActions) {
      if (pendingActionSatisfied(detail, action.target)) {
        resolvePendingAction(action.id)
      }
    }
  }, [detail, resolvePendingAction, taskPendingActions])

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
          <section className={`detail-block task-detail-hero ${drawerSyncing ? 'detail-block-syncing' : ''}`}>
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

          <section className={`detail-block ${drawerSyncing ? 'detail-block-syncing' : ''}`}>
            <h3>Editable Metadata</h3>
            <div className="task-form-grid">
              <label className="plans-filter-field">
                <span>Title</span>
                <input
                  type="text"
                  value={titleDraft}
                  disabled={drawerSyncing}
                  onChange={(event) => setTitleDraft(event.target.value)}
                />
              </label>
              <label className="plans-filter-field">
                <span>Priority</span>
                <select
                  value={priorityDraft}
                  disabled={drawerSyncing}
                  onChange={(event) => setPriorityDraft(event.target.value)}
                >
                  <option value="">Unset</option>
                  {Array.from({ length: 10 }, (_, index) => index + 1).map((value) => (
                    <option key={value} value={value.toString()}>{value}</option>
                  ))}
                </select>
              </label>
              <label className="plans-filter-field task-form-span">
                <span>Description / Goal</span>
                <textarea
                  rows={4}
                  value={descriptionDraft}
                  disabled={drawerSyncing}
                  onChange={(event) => setDescriptionDraft(event.target.value)}
                />
              </label>
              <label className="plans-filter-field">
                <span>Status Override</span>
                <select
                  value={statusDraft}
                  disabled={drawerSyncing}
                  onChange={(event) => setStatusDraft(event.target.value)}
                >
                  {detail.editable.statusOptions.map((option) => (
                    <option key={option} value={option}>
                      {formatLabel(option)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="plans-filter-field task-form-span">
                <span>Validation Requirements</span>
                <textarea
                  rows={5}
                  value={validationDraft}
                  disabled={drawerSyncing}
                  onChange={(event) => setValidationDraft(event.target.value)}
                  placeholder="One validation ref per line"
                />
              </label>
            </div>
            <div className="drawer-action-row">
              <button
                type="button"
                className="table-button"
                disabled={drawerSyncing || !metadataChanged(detail, {
                  titleDraft,
                  descriptionDraft,
                  priorityDraft,
                  statusDraft,
                  validationDraft,
                })}
                onClick={() => void submitMetadataMutation({
                  descriptionDraft,
                  detail,
                  priorityDraft,
                  queueMutation,
                  setMutationError,
                  statusDraft,
                  taskId,
                  titleDraft,
                  validationDraft,
                })}
              >
                {drawerSyncing ? 'Syncing…' : 'Save metadata'}
              </button>
              {mutationError ? <span className="detail-error-text">{mutationError}</span> : null}
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

function metadataChanged(
  detail: PrismUiTaskDetailView,
  drafts: {
    titleDraft: string
    descriptionDraft: string
    priorityDraft: string
    statusDraft: string
    validationDraft: string
  },
) {
  const normalizedValidation = normalizeValidationRefs(drafts.validationDraft)
  return (
    drafts.titleDraft.trim() !== detail.editable.title
    || drafts.descriptionDraft !== (detail.editable.description ?? '')
    || drafts.priorityDraft !== (detail.editable.priority?.toString() ?? '')
    || drafts.statusDraft !== detail.editable.status.toLowerCase()
    || normalizedValidation.join('\n') !== detail.editable.validationRefs.map((ref) => ref.id).join('\n')
  )
}

async function submitMetadataMutation(args: {
  descriptionDraft: string
  detail: PrismUiTaskDetailView
  priorityDraft: string
  queueMutation: ReturnType<typeof useUiMutationQueue>['queueMutation']
  setMutationError: (value: string | null) => void
  statusDraft: string
  taskId: string | null
  titleDraft: string
  validationDraft: string
}) {
  const {
    descriptionDraft,
    detail,
    priorityDraft,
    queueMutation,
    setMutationError,
    statusDraft,
    taskId,
    titleDraft,
    validationDraft,
  } = args
  if (!taskId) {
    return
  }

  const trimmedTitle = titleDraft.trim()
  if (!trimmedTitle) {
    setMutationError('Title cannot be empty.')
    return
  }

  const normalizedValidation = normalizeValidationRefs(validationDraft)
  const target = {
    description: descriptionDraft,
    priority: priorityDraft ? Number(priorityDraft) : null,
    status: statusDraft,
    title: trimmedTitle,
    validationRefs: normalizedValidation,
  }

  try {
    setMutationError(null)
    await queueMutation({
      fields: ['title', 'description', 'priority', 'status', 'validationRefs'],
      label: `Sync task metadata: ${trimmedTitle}`,
      request: {
        action: 'coordination',
        input: {
          kind: 'update',
          payload: {
            id: taskId,
            title: trimmedTitle,
            status: statusDraft,
            summary: descriptionDraft.length > 0
              ? descriptionDraft
              : { op: 'clear' },
            priority: priorityDraft
              ? Number(priorityDraft)
              : { op: 'clear' },
            validationRefs: normalizedValidation.map((id) => ({ id })),
          },
        },
      },
      target,
      taskId,
    })
  } catch (error) {
    setMutationError(error instanceof Error ? error.message : 'Mutation failed.')
  }
}

function normalizeValidationRefs(value: string): string[] {
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
}

function pendingActionSatisfied(
  detail: PrismUiTaskDetailView,
  target: Record<string, unknown>,
) {
  const description = typeof target.description === 'string' ? target.description : null
  const priority = typeof target.priority === 'number' ? target.priority : null
  const status = typeof target.status === 'string' ? target.status : null
  const title = typeof target.title === 'string' ? target.title : null
  const validationRefs = Array.isArray(target.validationRefs)
    ? target.validationRefs.filter((value): value is string => typeof value === 'string')
    : null

  return (
    (title === null || detail.editable.title === title)
    && (description === null || (detail.editable.description ?? '') === description)
    && (priority === null || detail.editable.priority === priority)
    && (status === null || detail.editable.status.toLowerCase() === status)
    && (
      validationRefs === null
      || detail.editable.validationRefs.map((ref: ValidationRefView) => ref.id).join('\n')
        === validationRefs.join('\n')
    )
  )
}
