import { useEffect, useState, type ReactNode } from 'react'

import { PrismFlowCanvas, type PrismFlowEdge, type PrismFlowNode } from '../components/graph/PrismFlowCanvas'
import { buildPlanFlow } from '../graph/planFlowModel'
import { usePlansData } from '../hooks/usePlansData'
import type {
  CoordinationTaskView,
  OutcomeSummaryView,
  PlanEdgeView,
  PlanExecutionOverlayView,
  PlanNodeRecommendationView,
  PlanNodeView,
  PrismPlanDetailView,
} from '../types'

type PlansPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function PlansPage({ search, onNavigate }: PlansPageProps) {
  const requestedPlanId = new URLSearchParams(search).get('plan')
  const plansView = usePlansData(requestedPlanId)
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null)
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null)
  const [hoveredEdgeId, setHoveredEdgeId] = useState<string | null>(null)

  useEffect(() => {
    const selectedPlan = plansView?.selectedPlan
    if (!selectedPlan) {
      setSelectedNodeId(null)
      setSelectedEdgeId(null)
      return
    }

    const nextRecommended = selectedPlan.nextNodes[0]?.node.id
    const nextRoot = selectedPlan.graph.rootNodeIds[0]
    const nextFallback = selectedPlan.graph.nodes[0]?.id ?? null
    setSelectedNodeId(nextRecommended ?? nextRoot ?? nextFallback)
    setSelectedEdgeId(null)
  }, [plansView?.selectedPlanId])

  if (!plansView) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Plans</p>
        <h2>Loading plan runtime graph</h2>
        <p>Fetching the current plan list, selected graph, and execution state.</p>
      </section>
    )
  }

  if (plansView.plans.length === 0) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Plans</p>
        <h2>No plans are active yet.</h2>
        <p>The graph surface is ready. Once plans exist in Prism, this page will render them as an execution graph instead of a task list.</p>
      </section>
    )
  }

  const selectedPlan = plansView.selectedPlan
  const selectedPlanId = plansView.selectedPlanId ?? plansView.plans[0]?.planId ?? null

  if (!selectedPlan) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Plans</p>
        <h2>Plan detail is unavailable.</h2>
        <p>Select another plan to render its execution graph and runtime inspector.</p>
      </section>
    )
  }

  const nodeById = new Map(selectedPlan.graph.nodes.map((node) => [node.id, node]))
  const edgeById = new Map(selectedPlan.graph.edges.map((edge) => [edge.id, edge]))
  const recommendationById = new Map(selectedPlan.nextNodes.map((item) => [item.node.id, item]))
  const overlayById = new Map(selectedPlan.execution.map((item) => [item.nodeId, item]))
  const selectedNode = selectedNodeId ? nodeById.get(selectedNodeId) ?? null : null
  const selectedEdge = selectedEdgeId ? edgeById.get(selectedEdgeId) ?? null : null
  const hoveredNode = hoveredNodeId ? nodeById.get(hoveredNodeId) ?? null : null
  const hoveredEdge = hoveredEdgeId ? edgeById.get(hoveredEdgeId) ?? null : null

  const flow = buildPlanFlow(selectedPlan.graph, selectedPlan.nextNodes, selectedPlan.execution, {
    selectedNodeId,
    hoveredNodeId,
    selectedEdgeId,
    hoveredEdgeId,
  })

  return (
    <div className="page-stack flow-page">
      <section className="hero-bar panel flow-hero">
        <div>
          <p className="eyebrow">Prism Plans</p>
          <h2>Execution graph for the active plan</h2>
          <p className="lede flow-hero-subtitle">{selectedPlan.plan.title}</p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{selectedPlan.summary.actionableNodes} ready</span>
          <span className="connection-pill">{selectedPlan.summary.executionBlockedNodes} blocked</span>
          <span className="connection-pill">{selectedPlan.summary.inProgressNodes} in progress</span>
        </div>
      </section>

      <section className="panel flow-selector-panel">
        <div className="panel-header">
          <h3>Active Plans</h3>
          <span>{plansView.plans.length}</span>
        </div>
        <div className="panel-body flow-selector-row">
          {plansView.plans.map((plan) => (
            <button
              key={plan.planId}
              type="button"
              title={plan.title}
              className={`flow-selector ${plan.planId === selectedPlanId ? 'flow-selector-active' : ''}`}
              onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(plan.planId)}`)}
            >
              <span className="flow-selector-title">{compactPlanLabel(plan.title)}</span>
              <span className="flow-selector-caption">{plan.title}</span>
              <span className="flow-selector-meta">
                <strong>{plan.summary.actionableNodes}</strong> ready
              </span>
              <span className="flow-selector-meta">
                <strong>{plan.summary.executionBlockedNodes}</strong> blocked
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="flow-layout">
        <section className="panel flow-stage-panel">
          <div className="panel-header">
            <h3>Execution Graph</h3>
            <span>{selectedPlan.graph.nodes.length} nodes / {selectedPlan.graph.edges.length} edges</span>
          </div>
          <div className="flow-stage-meta">
            <span>{selectedPlan.summary.reviewGatedNodes} review-gated</span>
            <span>{selectedPlan.summary.validationGatedNodes} validation-gated</span>
            <span>{selectedPlan.pendingHandoffs.length} pending handoffs</span>
          </div>
          <div className="flow-stage">
            <PrismFlowCanvas
              nodes={flow.nodes}
              edges={flow.edges}
              onNodeActivate={(node) => {
                setSelectedNodeId(node.id)
                setSelectedEdgeId(null)
              }}
              onEdgeActivate={(edge) => {
                setSelectedEdgeId(edge.id)
                setSelectedNodeId(null)
              }}
              onNodeHoverChange={setHoveredNodeId}
              onEdgeHoverChange={setHoveredEdgeId}
              onPaneActivate={() => {
                setSelectedEdgeId(null)
              }}
            />
          </div>
        </section>

        <aside className="panel flow-inspector">
          <div className="panel-header">
            <h3>Inspector</h3>
            <span>{selectedEdge ? 'edge' : selectedNode ? 'node' : 'plan'}</span>
          </div>
          <div className="panel-body flow-inspector-body">
            {selectedEdge ? (
              <PlanEdgeInspector edge={selectedEdge} nodeById={nodeById} />
            ) : selectedNode ? (
              <PlanNodeInspector
                node={selectedNode}
                recommendation={recommendationById.get(selectedNode.id)}
                overlay={overlayById.get(selectedNode.id)}
              />
            ) : hoveredEdge ? (
              <PlanEdgeInspector edge={hoveredEdge} nodeById={nodeById} />
            ) : hoveredNode ? (
              <PlanNodeInspector
                node={hoveredNode}
                recommendation={recommendationById.get(hoveredNode.id)}
                overlay={overlayById.get(hoveredNode.id)}
              />
            ) : (
              <PlanSummaryInspector plan={selectedPlan} />
            )}
          </div>
        </aside>
      </section>

      <section className="flow-support-grid">
        <CompactListPanel
          title="Next Up"
          count={selectedPlan.nextNodes.length}
          emptyMessage="No next-node recommendations are available."
          items={selectedPlan.nextNodes.slice(0, 4).map((recommendation) => (
            <button
              key={recommendation.node.id}
              type="button"
              className="compact-item"
              onClick={() => {
                setSelectedNodeId(recommendation.node.id)
                setSelectedEdgeId(null)
              }}
            >
              <strong>{recommendation.node.title}</strong>
              <p>{recommendation.reasons[0] ?? recommendation.node.summary ?? 'Actionable now.'}</p>
            </button>
          ))}
        />
        <CompactListPanel
          title="Ready Tasks"
          count={selectedPlan.readyTasks.length}
          emptyMessage="No ready coordination tasks are exposed right now."
          items={selectedPlan.readyTasks.slice(0, 4).map((task) => (
            <CompactTaskCard key={task.id} task={task} />
          ))}
        />
        <CompactListPanel
          title="Recent Outcomes"
          count={selectedPlan.recentOutcomes.length}
          emptyMessage="No recent recorded outcomes are tied to this plan."
          items={selectedPlan.recentOutcomes.slice(0, 4).map((outcome) => (
            <CompactOutcomeCard key={`${outcome.ts}-${outcome.summary}`} outcome={outcome} />
          ))}
        />
      </section>
    </div>
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
      <InspectorSection title="What this graph shows">
        <p>Click any node to inspect blockers, acceptance, bindings, and runtime ownership. Click an edge to inspect the authored relationship between milestones.</p>
      </InspectorSection>
      <InspectorSection title="Current pressure">
        <ul className="inspector-list">
          <li>{plan.pendingHandoffs.length} pending handoffs</li>
          <li>{plan.pendingReviews.length} pending reviews</li>
          <li>{plan.recentViolations.length} recent policy violations</li>
        </ul>
      </InspectorSection>
    </div>
  )
}

function PlanNodeInspector({
  node,
  recommendation,
  overlay,
}: {
  node: PlanNodeView
  recommendation?: PlanNodeRecommendationView
  overlay?: PlanExecutionOverlayView
}) {
  const blockers = recommendation?.blockers ?? []
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">{formatLabel(node.kind ?? 'node')}</p>
        <h3>{node.title}</h3>
        <p>{node.summary ?? recommendation?.reasons[0] ?? 'No authored summary is attached to this node yet.'}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Status" value={formatLabel(node.status)} />
        <StatPill label="Blockers" value={blockers.length} />
        <StatPill label="Checks" value={node.validationRefs?.length ?? 0} />
        <StatPill label="Concepts" value={node.bindings.conceptHandles.length} />
      </div>
      <InspectorSection title="Runtime">
        <ul className="inspector-list">
          <li>Owner: {overlay?.effectiveAssignee ?? node.assignee ?? 'Unassigned'}</li>
          <li>Pending handoff: {overlay?.pendingHandoffTo ?? 'None'}</li>
          <li>Awaiting handoff from: {overlay?.awaitingHandoffFrom ?? 'None'}</li>
        </ul>
      </InspectorSection>
      <InspectorSection title="Acceptance">
        {node.acceptance && node.acceptance.length > 0 ? (
          <ul className="inspector-list">
            {node.acceptance.map((criterion) => (
              <li key={criterion.label}>{criterion.label}</li>
            ))}
          </ul>
        ) : (
          <p>No explicit acceptance criteria are attached to this node.</p>
        )}
      </InspectorSection>
      <InspectorSection title="Blockers">
        {blockers.length > 0 ? (
          <ul className="inspector-list">
            {blockers.map((blocker) => (
              <li key={`${blocker.kind}-${blocker.summary}`}>{blocker.summary}</li>
            ))}
          </ul>
        ) : (
          <p>No blockers are currently attached to this node.</p>
        )}
      </InspectorSection>
    </div>
  )
}

function PlanEdgeInspector({
  edge,
  nodeById,
}: {
  edge: PlanEdgeView
  nodeById: Map<string, PlanNodeView>
}) {
  const from = nodeById.get(edge.from)
  const to = nodeById.get(edge.to)

  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Plan Edge</p>
        <h3>{formatLabel(edge.kind)}</h3>
        <p>{edge.summary ?? 'This authored edge defines how execution flows between two plan nodes.'}</p>
      </section>
      <InspectorSection title="Path">
        <ul className="inspector-list">
          <li>From: {from?.title ?? edge.from}</li>
          <li>To: {to?.title ?? edge.to}</li>
        </ul>
      </InspectorSection>
    </div>
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

function CompactTaskCard({ task }: { task: CoordinationTaskView }) {
  return (
    <article className="compact-item">
      <strong>{task.title}</strong>
      <p>{task.id}</p>
      <span className="compact-meta">{formatLabel(task.status)}</span>
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

function compactPlanLabel(title: string) {
  const words = title.trim().split(/\s+/)
  if (words.length <= 8) {
    return title
  }
  return `${words.slice(0, 8).join(' ')}…`
}
