import { usePlansData } from '../hooks/usePlansData'
import type {
  ArtifactView,
  CoordinationTaskView,
  OutcomeSummaryView,
  PlanExecutionOverlayView,
  PlanListEntryView,
  PlanNodeRecommendationView,
  PlanNodeView,
  PolicyViolationRecordView,
} from '../types'

type PlansPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function PlansPage({ search, onNavigate }: PlansPageProps) {
  const requestedPlanId = new URLSearchParams(search).get('plan')
  const plansView = usePlansData(requestedPlanId)

  if (!plansView) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Plans</p>
        <h2>Loading native plan runtime state</h2>
        <p>Fetching the current plan list, selected graph, blockers, and coordination queues.</p>
      </section>
    )
  }

  if (plansView.plans.length === 0) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Plans</p>
        <h2>No plans are active yet.</h2>
        <p>The shell is ready. Once plans exist in Prism, this view will surface their graph, blockers, validations, and handoffs.</p>
      </section>
    )
  }

  const selectedPlan = plansView.selectedPlan
  const selectedPlanId = plansView.selectedPlanId ?? plansView.plans[0]?.planId ?? null
  const executionByNodeId = Object.fromEntries(
    (selectedPlan?.execution ?? []).map((overlay) => [overlay.nodeId, overlay]),
  )

  return (
    <div className="page-stack plans-page">
      <section className="hero-bar panel">
        <div>
          <p className="eyebrow">Prism Plans</p>
          <h2>{selectedPlan?.plan.title ?? 'Plan control plane'}</h2>
          <p className="lede">{selectedPlan?.plan.goal ?? 'Select a plan to inspect blockers, readiness, and execution pressure.'}</p>
        </div>
        {selectedPlan ? (
          <div className="hero-actions">
            <span className="connection-pill">{selectedPlan.summary.actionableNodes} ready</span>
            <span className="connection-pill">{selectedPlan.summary.executionBlockedNodes} blocked</span>
            <span className="connection-pill">{selectedPlan.graph.nodes.length} nodes</span>
          </div>
        ) : null}
      </section>

      <section className="plans-layout">
        <aside className="panel plans-sidebar">
          <div className="panel-header">
            <h3>Plans</h3>
            <span>{plansView.plans.length}</span>
          </div>
          <div className="panel-body plans-list">
            {plansView.plans.map((plan) => (
              <button
                key={plan.planId}
                type="button"
                className={`plan-list-button ${plan.planId === selectedPlanId ? 'plan-list-button-active' : ''}`}
                onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(plan.planId)}`)}
              >
                <div className="plan-list-topline">
                  <strong>{plan.title}</strong>
                  <span className={`status-chip status-${statusTone(plan.status)}`}>{formatLabel(plan.status)}</span>
                </div>
                <p>{plan.goal}</p>
                <div className="metric-grid">
                  <MetricCard label="Ready" value={plan.summary.actionableNodes} />
                  <MetricCard label="Blocked" value={plan.summary.executionBlockedNodes} />
                </div>
              </button>
            ))}
          </div>
        </aside>

        <div className="plans-main">
          {!selectedPlan ? (
            <section className="panel hero-panel">
              <p className="eyebrow">Plan Selection</p>
              <h2>Plan detail is unavailable.</h2>
              <p>Select another plan from the sidebar or refresh the page to reload the current plan graph.</p>
            </section>
          ) : (
            <>
              <section className="status-grid">
                <article className="panel stat-card">
                  <p className="stat-label">Execution</p>
                  <h3>{selectedPlan.summary.actionableNodes} ready / {selectedPlan.summary.executionBlockedNodes} blocked</h3>
                  <p>{selectedPlan.summary.inProgressNodes} nodes are currently in progress.</p>
                </article>
                <article className="panel stat-card">
                  <p className="stat-label">Completion Gates</p>
                  <h3>{selectedPlan.summary.completionGatedNodes}</h3>
                  <p>{selectedPlan.summary.reviewGatedNodes} review-gated and {selectedPlan.summary.validationGatedNodes} validation-gated.</p>
                </article>
                <article className="panel stat-card">
                  <p className="stat-label">Reviews</p>
                  <h3>{selectedPlan.pendingReviews.length} pending</h3>
                  <p>{selectedPlan.recentViolations.length} recent policy rejections for this plan.</p>
                </article>
                <article className="panel stat-card">
                  <p className="stat-label">Runtime Overlay</p>
                  <h3>{selectedPlan.pendingHandoffs.length} handoffs</h3>
                  <p>{selectedPlan.execution.length} nodes currently carry execution overlay data.</p>
                </article>
              </section>

              <section className="plans-main-grid">
                <article className="panel">
                  <div className="panel-header">
                    <h3>Recommended Next Nodes</h3>
                    <span>{selectedPlan.nextNodes.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.nextNodes.length === 0 ? (
                      <p className="empty-state">No next-node recommendations are available yet.</p>
                    ) : (
                      selectedPlan.nextNodes.map((recommendation) => (
                        <RecommendationCard key={recommendation.node.id} recommendation={recommendation} />
                      ))
                    )}
                  </div>
                </article>

                <article className="panel">
                  <div className="panel-header">
                    <h3>Plan Graph</h3>
                    <span>{selectedPlan.graph.edges.length} edges</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.graph.nodes.map((node) => (
                      <NodeCard
                        key={node.id}
                        node={node}
                        overlay={executionByNodeId[node.id] as PlanExecutionOverlayView | undefined}
                      />
                    ))}
                  </div>
                </article>

                <article className="panel">
                  <div className="panel-header">
                    <h3>Ready Tasks</h3>
                    <span>{selectedPlan.readyTasks.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.readyTasks.length === 0 ? (
                      <p className="empty-state">No ready coordination tasks are exposed for this plan right now.</p>
                    ) : (
                      selectedPlan.readyTasks.map((task) => (
                        <TaskCard key={task.id} task={task} />
                      ))
                    )}
                  </div>
                </article>

                <article className="panel">
                  <div className="panel-header">
                    <h3>Pending Reviews</h3>
                    <span>{selectedPlan.pendingReviews.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.pendingReviews.length === 0 ? (
                      <p className="empty-state">No reviewable artifacts are pending for this plan.</p>
                    ) : (
                      selectedPlan.pendingReviews.map((artifact) => (
                        <ArtifactCard key={artifact.id} artifact={artifact} />
                      ))
                    )}
                  </div>
                </article>

                <article className="panel">
                  <div className="panel-header">
                    <h3>Pending Handoffs</h3>
                    <span>{selectedPlan.pendingHandoffs.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.pendingHandoffs.length === 0 ? (
                      <p className="empty-state">No handoffs are waiting on acceptance for this plan.</p>
                    ) : (
                      selectedPlan.pendingHandoffs.map((task) => (
                        <TaskCard key={task.id} task={task} />
                      ))
                    )}
                  </div>
                </article>

                <article className="panel">
                  <div className="panel-header">
                    <h3>Recent Outcomes</h3>
                    <span>{selectedPlan.recentOutcomes.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.recentOutcomes.length === 0 ? (
                      <p className="empty-state">No recent recorded outcomes are tied to the currently visible plan work.</p>
                    ) : (
                      selectedPlan.recentOutcomes.map((outcome) => (
                        <OutcomeCard key={`${outcome.ts}-${outcome.summary}`} outcome={outcome} />
                      ))
                    )}
                  </div>
                </article>

                <article className="panel wide-panel">
                  <div className="panel-header">
                    <h3>Policy Violations</h3>
                    <span>{selectedPlan.recentViolations.length}</span>
                  </div>
                  <div className="panel-body signal-list">
                    {selectedPlan.recentViolations.length === 0 ? (
                      <p className="empty-state">No recent policy rejections have been recorded for this plan.</p>
                    ) : (
                      selectedPlan.recentViolations.map((record) => (
                        <ViolationCard key={record.eventId} record={record} />
                      ))
                    )}
                  </div>
                </article>
              </section>
            </>
          )}
        </div>
      </section>
    </div>
  )
}

function RecommendationCard({ recommendation }: { recommendation: PlanNodeRecommendationView }) {
  const blockerSummary = recommendation.blockers?.[0]?.summary

  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{recommendation.node.title}</strong>
        <span className={`status-chip status-${recommendation.actionable ? 'ok' : 'warn'}`}>
          {recommendation.actionable ? 'Actionable' : formatLabel(recommendation.node.status)}
        </span>
      </div>
      <p>{recommendation.reasons[0] ?? recommendation.node.summary ?? 'No recommendation reason recorded.'}</p>
      {recommendation.effectiveAssignee ? (
        <p className="operation-meta">Owner: {recommendation.effectiveAssignee}</p>
      ) : null}
      {blockerSummary ? (
        <div className="plan-inline-list">
          <span className="status-chip status-warn">Blocker</span>
          <span>{blockerSummary}</span>
        </div>
      ) : null}
    </article>
  )
}

function NodeCard({
  node,
  overlay,
}: {
  node: PlanNodeView
  overlay?: PlanExecutionOverlayView
}) {
  const summary = overlay?.pendingHandoffTo
    ? `Pending handoff to ${overlay.pendingHandoffTo}`
    : overlay?.effectiveAssignee
      ? `Effective assignee: ${overlay.effectiveAssignee}`
      : node.summary ?? `${node.bindings.conceptHandles.length} concept links`

  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{node.title}</strong>
        <span className={`status-chip status-${statusTone(node.status)}`}>{formatLabel(node.status)}</span>
      </div>
      <p>{summary}</p>
      <div className="plan-inline-list">
        <span>{node.kind ?? 'node'}</span>
        <span>{node.bindings.conceptHandles.length} concepts</span>
        <span>{node.validationRefs?.length ?? 0} validations</span>
      </div>
    </article>
  )
}

function TaskCard({ task }: { task: CoordinationTaskView }) {
  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{task.title}</strong>
        <span className={`status-chip status-${statusTone(task.status)}`}>{formatLabel(task.status)}</span>
      </div>
      <p>{task.pendingHandoffTo ? `Pending handoff to ${task.pendingHandoffTo}` : task.id}</p>
      <div className="plan-inline-list">
        <span>{task.assignee ?? 'Unassigned'}</span>
        <span>{task.dependsOn.length} dependencies</span>
      </div>
    </article>
  )
}

function ArtifactCard({ artifact }: { artifact: ArtifactView }) {
  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{artifact.id}</strong>
        <span className={`status-chip status-${statusTone(artifact.status)}`}>{formatLabel(artifact.status)}</span>
      </div>
      <p>{artifact.taskId}</p>
      <div className="plan-inline-list">
        <span>{artifact.requiredValidations.length} required checks</span>
        <span>{artifact.validatedChecks.length} validated</span>
        {typeof artifact.riskScore === 'number' ? <span>risk {artifact.riskScore.toFixed(2)}</span> : null}
      </div>
    </article>
  )
}

function OutcomeCard({ outcome }: { outcome: OutcomeSummaryView }) {
  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{outcome.summary}</strong>
        <span className={`status-chip status-${statusTone(outcome.result)}`}>{formatLabel(outcome.result)}</span>
      </div>
      <div className="plan-inline-list">
        <span>{formatLabel(outcome.kind)}</span>
        <span>{new Date(outcome.ts * 1000).toLocaleString()}</span>
      </div>
    </article>
  )
}

function ViolationCard({ record }: { record: PolicyViolationRecordView }) {
  return (
    <article className="plan-card">
      <div className="plan-list-topline">
        <strong>{record.summary}</strong>
        <span className="status-chip status-error">{record.violations.length} violations</span>
      </div>
      <p>{record.planId ?? record.taskId ?? record.eventId}</p>
      <div className="signal-list">
        {record.violations.map((violation) => (
          <div key={`${record.eventId}-${violation.code}`} className="plan-inline-list">
            <span className="status-chip status-warn">{violation.code}</span>
            <span>{violation.summary}</span>
          </div>
        ))}
      </div>
    </article>
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

function formatLabel(value: string) {
  return value.replaceAll('_', ' ')
}

function statusTone(value: string) {
  const normalized = value.toLowerCase()
  if (
    normalized.includes('fail') ||
    normalized.includes('reject') ||
    normalized.includes('blocked') ||
    normalized.includes('abandoned')
  ) {
    return 'error'
  }
  if (
    normalized.includes('ready') ||
    normalized.includes('complete') ||
    normalized.includes('approved') ||
    normalized.includes('success')
  ) {
    return 'ok'
  }
  return 'warn'
}
