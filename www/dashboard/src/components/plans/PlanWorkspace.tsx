import type { ReactNode } from 'react'

import type {
  ArtifactView,
  CoordinationPlanV2View,
  CoordinationTaskV2View,
  CoordinationTaskView,
  OutcomeSummaryView,
  PrismPlanDetailView,
} from '../../types'

type PlanWorkspaceProps = {
  plan: PrismPlanDetailView
  selectedTaskId: string | null
  onTaskSelect: (taskId: string) => void
}

export function PlanWorkspace({
  plan,
  selectedTaskId,
  onTaskSelect,
}: PlanWorkspaceProps) {
  const selectedTask =
    plan.childTasks.find((task) => task.id === selectedTaskId)
    ?? plan.readyTasks.find((task) => task.id === selectedTaskId)
    ?? plan.pendingHandoffs.find((task) => task.id === selectedTaskId)
    ?? null

  return (
    <section className="plans-workspace">
      <section className="hero-bar panel operator-hero strategic-hero">
        <div>
          <p className="eyebrow">Plan Workspace</p>
          <h2>{plan.plan.title}</h2>
          <p className="lede">{plan.plan.summary}</p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{plan.summary.actionableNodes} ready</span>
          <span className="connection-pill">{plan.childTasks.length} child tasks</span>
          <span className="connection-pill">{plan.childPlans.length} child plans</span>
          {selectedTaskId ? <span className="connection-pill">Task selected</span> : null}
        </div>
      </section>

      <section className="status-grid strategic-status-grid">
        <article className="panel stat-card">
          <p className="stat-label">Contained Work</p>
          <h3>{plan.children.length}</h3>
          <p>Direct child plans and tasks linked to this plan in the canonical coordination graph.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Ready Tasks</p>
          <h3>{plan.readyTasks.length}</h3>
          <p>Tasks currently actionable for the current executor view.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Pending Reviews</p>
          <h3>{plan.pendingReviews.length}</h3>
          <p>Artifacts still waiting for review before work can close cleanly.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Pending Handoffs</p>
          <h3>{plan.pendingHandoffs.length}</h3>
          <p>Tasks where execution context is waiting to move to another operator or runtime.</p>
        </article>
      </section>

      <section className="strategic-workspace-grid">
        <section className="panel flow-stage-panel strategic-graph-panel">
          <div className="panel-header">
            <h3>Contained Work</h3>
            <span>{plan.children.length} children</span>
          </div>
          <div className="flow-stage-meta">
            <span>{plan.childTasks.length} tasks</span>
            <span>{plan.childPlans.length} plans</span>
            <span>{plan.summary.executionBlockedNodes} blocked</span>
          </div>
          <div className="panel-body compact-panel-body">
            <CompactListPanel
              title="Child Tasks"
              count={plan.childTasks.length}
              emptyMessage="No direct child tasks are attached to this plan."
              items={plan.childTasks.map((task) => (
                <button
                  key={task.id}
                  type="button"
                  className={`compact-item ${selectedTaskId === task.id ? 'compact-item-active' : ''}`}
                  onClick={() => onTaskSelect(task.id)}
                >
                  <strong>{task.title}</strong>
                  <p>{task.summary ?? task.id}</p>
                  <span className="compact-meta">
                    {formatLabel(task.status)} · {formatLabel(task.executor.executorClass)}
                  </span>
                </button>
              ))}
            />
            <CompactListPanel
              title="Child Plans"
              count={plan.childPlans.length}
              emptyMessage="No direct child plans are attached to this plan."
              items={plan.childPlans.map((child) => (
                <ChildPlanCard key={child.id} plan={child} />
              ))}
            />
          </div>
        </section>

        <aside className="panel flow-inspector strategic-inspector">
          <div className="panel-header">
            <h3>Focus</h3>
            <span>{selectedTask ? 'task' : 'plan'}</span>
          </div>
          <div className="panel-body flow-inspector-body">
            {selectedTask ? (
              <TaskFocusInspector task={selectedTask} />
            ) : (
              <PlanSummaryInspector plan={plan} />
            )}
          </div>
        </aside>
      </section>

      <section className="flow-support-grid strategic-support-grid">
        <CompactListPanel
          title="Ready Tasks"
          count={plan.readyTasks.length}
          emptyMessage="No ready coordination tasks are exposed right now."
          items={plan.readyTasks.slice(0, 4).map((task) => (
            <button
              key={task.id}
              type="button"
              className={`compact-item ${selectedTaskId === task.id ? 'compact-item-active' : ''}`}
              onClick={() => onTaskSelect(task.id)}
            >
              <strong>{task.title}</strong>
              <p>{task.summary ?? task.id}</p>
              <span className="compact-meta">{formatLabel(task.status)}</span>
            </button>
          ))}
        />
        <CompactListPanel
          title="Pending Reviews"
          count={plan.pendingReviews.length}
          emptyMessage="No pending review artifacts are tied to this plan."
          items={plan.pendingReviews.slice(0, 4).map((artifact) => (
            <CompactArtifactCard key={artifact.id} artifact={artifact} />
          ))}
        />
        <CompactListPanel
          title="Recent Outcomes"
          count={plan.recentOutcomes.length}
          emptyMessage="No recent recorded outcomes are tied to this plan."
          items={plan.recentOutcomes.slice(0, 4).map((outcome) => (
            <CompactOutcomeCard key={`${outcome.ts}-${outcome.summary}`} outcome={outcome} />
          ))}
        />
      </section>
    </section>
  )
}

function PlanSummaryInspector({ plan }: { plan: PrismPlanDetailView }) {
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Plan Summary</p>
        <h3>{plan.plan.title}</h3>
        <p>{plan.plan.goal}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Ready" value={plan.summary.actionableNodes} />
        <StatPill label="Blocked" value={plan.summary.executionBlockedNodes} />
        <StatPill label="In Progress" value={plan.summary.inProgressNodes} />
        <StatPill label="Stale" value={plan.summary.staleNodes} />
      </div>
      <InspectorSection title="Current pressure">
        <ul className="inspector-list">
          <li>{plan.pendingHandoffs.length} pending handoffs</li>
          <li>{plan.pendingReviews.length} pending reviews</li>
          <li>{plan.recentViolations.length} recent policy violations</li>
        </ul>
      </InspectorSection>
      <InspectorSection title="Containment">
        <ul className="inspector-list">
          <li>{plan.childTasks.length} direct child tasks</li>
          <li>{plan.childPlans.length} direct child plans</li>
          <li>{plan.children.length} total direct children</li>
        </ul>
      </InspectorSection>
    </div>
  )
}

function TaskFocusInspector({
  task,
}: {
  task: CoordinationTaskV2View | CoordinationTaskView
}) {
  const dependencies = 'dependencies' in task ? task.dependencies.length : task.dependsOn.length
  const dependents = 'dependents' in task ? task.dependents.length : 0
  const validationCount = 'validationRefs' in task ? (task.validationRefs?.length ?? 0) : 0
  const assignee = task.assignee ?? ('session' in task ? task.session : undefined) ?? 'Unassigned'

  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">{formatLabel('kind' in task ? (task.kind ?? 'task') : 'task')}</p>
        <h3>{task.title}</h3>
        <p>{task.summary ?? 'No summary is attached to this task yet.'}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Status" value={formatLabel(task.status)} />
        <StatPill label="Dependencies" value={dependencies} />
        <StatPill label="Dependents" value={dependents} />
        <StatPill label="Checks" value={validationCount} />
      </div>
      <InspectorSection title="Runtime">
        <ul className="inspector-list">
          <li>Owner: {assignee}</li>
          {'executor' in task ? <li>Executor: {formatLabel(task.executor.executorClass)}</li> : null}
          {'worktreeId' in task && task.worktreeId ? <li>Worktree: {task.worktreeId}</li> : null}
          {'branchRef' in task && task.branchRef ? <li>Branch: {task.branchRef}</li> : null}
        </ul>
      </InspectorSection>
      {'blockerCauses' in task ? (
        <InspectorSection title="Blocker Causes">
          {task.blockerCauses.length > 0 ? (
            <ul className="inspector-list">
              {task.blockerCauses.map((cause, index) => (
                <li key={`${cause.source}-${cause.code ?? index}`}>
                  {formatLabel(cause.source)}
                  {cause.code ? ` · ${cause.code}` : ''}
                </li>
              ))}
            </ul>
          ) : (
            <p>No blocker causes are attached to this task.</p>
          )}
        </InspectorSection>
      ) : null}
    </div>
  )
}

function ChildPlanCard({ plan }: { plan: CoordinationPlanV2View }) {
  return (
    <article className="compact-item">
      <strong>{plan.title}</strong>
      <p>{plan.goal}</p>
      <span className="compact-meta">
        {formatLabel(plan.status)} · {plan.remainingEstimatedMinutes} min remaining
      </span>
    </article>
  )
}

function CompactArtifactCard({ artifact }: { artifact: ArtifactView }) {
  return (
    <article className="compact-item">
      <strong>{artifact.id}</strong>
      <p>{formatLabel(artifact.status)}</p>
      <span className="compact-meta">
        {artifact.validatedChecks.length} checks · {artifact.requiredValidations.length} required
      </span>
    </article>
  )
}

function CompactListPanel({
  title,
  count,
  emptyMessage,
  items,
}: {
  title: string
  count: number
  emptyMessage: string
  items: ReactNode[]
}) {
  return (
    <article className="panel compact-panel">
      <div className="panel-header">
        <h3>{title}</h3>
        <span>{count}</span>
      </div>
      <div className="panel-body compact-panel-body">
        {items.length > 0 ? items : <p className="empty-state">{emptyMessage}</p>}
      </div>
    </article>
  )
}

function CompactOutcomeCard({ outcome }: { outcome: OutcomeSummaryView }) {
  return (
    <article className="compact-item">
      <strong>{outcome.summary}</strong>
      <p>{formatLabel(outcome.kind)} · {formatLabel(outcome.result)}</p>
      <span className="compact-meta">{new Date(outcome.ts * 1000).toLocaleString()}</span>
    </article>
  )
}

function InspectorSection({
  title,
  children,
}: {
  title: string
  children: ReactNode
}) {
  return (
    <section className="inspector-section">
      <h4>{title}</h4>
      {children}
    </section>
  )
}

function StatPill({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="inspector-pill">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function formatLabel(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase())
}
