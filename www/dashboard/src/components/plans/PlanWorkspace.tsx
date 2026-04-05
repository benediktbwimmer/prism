import type { ReactNode } from 'react'

import { PrismFlowCanvas } from '../graph/PrismFlowCanvas'
import { buildPlanFlow } from '../../graph/planFlowModel'
import type {
  CoordinationTaskView,
  OutcomeSummaryView,
  PlanEdgeView,
  PlanExecutionOverlayView,
  PlanNodeRecommendationView,
  PlanNodeView,
  PrismPlanDetailView,
} from '../../types'

type PlanWorkspaceProps = {
  hoveredEdgeId: string | null
  hoveredNodeId: string | null
  plan: PrismPlanDetailView
  selectedEdgeId: string | null
  selectedNodeId: string | null
  selectedTaskId: string | null
  onClearSelection: () => void
  onEdgeHoverChange: (edgeId: string | null) => void
  onEdgeSelect: (edgeId: string) => void
  onNodeHoverChange: (nodeId: string | null) => void
  onNodeSelect: (nodeId: string) => void
}

export function PlanWorkspace({
  hoveredEdgeId,
  hoveredNodeId,
  plan,
  selectedEdgeId,
  selectedNodeId,
  selectedTaskId,
  onClearSelection,
  onEdgeHoverChange,
  onEdgeSelect,
  onNodeHoverChange,
  onNodeSelect,
}: PlanWorkspaceProps) {
  const nodeById = new Map(plan.graph.nodes.map((node) => [node.id, node]))
  const edgeById = new Map(plan.graph.edges.map((edge) => [edge.id, edge]))
  const recommendationById = new Map(plan.nextNodes.map((item) => [item.node.id, item]))
  const overlayById = new Map(plan.execution.map((item) => [item.nodeId, item]))
  const selectedNode = selectedNodeId ? nodeById.get(selectedNodeId) ?? null : null
  const selectedEdge = selectedEdgeId ? edgeById.get(selectedEdgeId) ?? null : null
  const hoveredNode = hoveredNodeId ? nodeById.get(hoveredNodeId) ?? null : null
  const hoveredEdge = hoveredEdgeId ? edgeById.get(hoveredEdgeId) ?? null : null

  const flow = buildPlanFlow(plan.graph, plan.nextNodes, plan.execution, {
    selectedNodeId,
    hoveredNodeId,
    selectedEdgeId,
    hoveredEdgeId,
  })

  return (
    <section className="plans-workspace">
      <section className="hero-bar panel operator-hero strategic-hero">
        <div>
          <p className="eyebrow">Dependency Graph</p>
          <h2>{plan.plan.title}</h2>
          <p className="lede">{plan.plan.summary}</p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{plan.summary.actionableNodes} ready</span>
          <span className="connection-pill">{plan.summary.inProgressNodes} active</span>
          <span className="connection-pill">{plan.summary.executionBlockedNodes} blocked</span>
          {selectedTaskId ? <span className="connection-pill">Task target armed</span> : null}
        </div>
      </section>

      <section className="status-grid strategic-status-grid">
        <article className="panel stat-card">
          <p className="stat-label">Review Pressure</p>
          <h3>{plan.pendingReviews.length}</h3>
          <p>Pending review artifacts waiting on a human or another runtime.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Pending Handoffs</p>
          <h3>{plan.pendingHandoffs.length}</h3>
          <p>Nodes where execution context is waiting to change hands.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Validation Gates</p>
          <h3>{plan.summary.validationGatedNodes}</h3>
          <p>Nodes whose next move is blocked by a required validation signal.</p>
        </article>
        <article className="panel stat-card">
          <p className="stat-label">Stale Nodes</p>
          <h3>{plan.summary.staleNodes}</h3>
          <p>Nodes that may need human judgment because the active work looks stalled.</p>
        </article>
      </section>

      <section className="strategic-workspace-grid">
        <section className="panel flow-stage-panel strategic-graph-panel">
          <div className="panel-header">
            <h3>Execution Graph</h3>
            <span>{plan.graph.nodes.length} nodes / {plan.graph.edges.length} edges</span>
          </div>
          <div className="flow-stage-meta">
            <span>{plan.summary.reviewGatedNodes} review-gated</span>
            <span>{plan.summary.validationGatedNodes} validation-gated</span>
            <span>{plan.summary.completionGatedNodes} completion-gated</span>
          </div>
          <div className="flow-stage strategic-flow-stage">
            <PrismFlowCanvas
              nodes={flow.nodes}
              edges={flow.edges}
              onNodeActivate={(node) => onNodeSelect(node.id)}
              onEdgeActivate={(edge) => onEdgeSelect(edge.id)}
              onNodeHoverChange={onNodeHoverChange}
              onEdgeHoverChange={onEdgeHoverChange}
              onPaneActivate={onClearSelection}
            />
          </div>
        </section>

        <aside className="panel flow-inspector strategic-inspector">
          <div className="panel-header">
            <h3>Task Focus</h3>
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
              <PlanSummaryInspector plan={plan} />
            )}
          </div>
        </aside>
      </section>

      <section className="flow-support-grid strategic-support-grid">
        <CompactListPanel
          title="Ready Now"
          count={plan.nextNodes.length}
          emptyMessage="No next-node recommendations are available."
          items={plan.nextNodes.slice(0, 4).map((recommendation) => (
            <button
              key={recommendation.node.id}
              type="button"
              className="compact-item"
              onClick={() => onNodeSelect(recommendation.node.id)}
            >
              <strong>{recommendation.node.title}</strong>
              <p>{recommendation.reasons[0] ?? recommendation.node.summary ?? 'Actionable now.'}</p>
            </button>
          ))}
        />
        <CompactListPanel
          title="Ready Tasks"
          count={plan.readyTasks.length}
          emptyMessage="No ready coordination tasks are exposed right now."
          items={plan.readyTasks.slice(0, 4).map((task) => (
            <CompactTaskCard key={task.id} task={task} />
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
      <InspectorSection title="How to use this view">
        <p>Select a node to target the future task drawer, inspect blockers, and decide whether the current plan ordering still makes sense.</p>
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
