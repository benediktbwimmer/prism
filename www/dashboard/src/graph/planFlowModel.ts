import { MarkerType } from '@xyflow/react'

import type {
  PlanExecutionOverlayView,
  PlanGraphView,
  PlanNodeRecommendationView,
  PlanNodeView,
} from '../types'
import type { PrismFlowEdge, PrismFlowNode, PrismFlowNodeTone } from '../components/graph/PrismFlowCanvas'

type PlanFlowSelection = {
  selectedNodeId: string | null
  hoveredNodeId: string | null
  selectedEdgeId: string | null
  hoveredEdgeId: string | null
}

export function buildPlanFlow(
  graph: PlanGraphView,
  recommendations: PlanNodeRecommendationView[],
  execution: PlanExecutionOverlayView[],
  selection: PlanFlowSelection,
) {
  const recommendationById = new Map(recommendations.map((item) => [item.node.id, item]))
  const overlayById = new Map(execution.map((item) => [item.nodeId, item]))

  const nodes: PrismFlowNode[] = graph.nodes.map((node) => {
    const recommendation = recommendationById.get(node.id)
    const overlay = overlayById.get(node.id)
    const blockerCount = recommendation?.blockers?.length ?? 0
    const validationCount = node.validationRefs?.length ?? 0

    return {
      id: node.id,
      type: 'prismCard',
      position: { x: 0, y: 0 },
      style: {
        width: 268,
        height: 168,
      },
      data: {
        title: node.title,
        eyebrow: [formatLabel(node.kind ?? 'node'), overlay?.effectiveAssignee ?? node.assignee]
          .filter(Boolean)
          .join(' · '),
        body: node.summary ?? recommendation?.reasons[0] ?? 'No summary recorded yet.',
        badge: formatLabel(node.status),
        footerLeft: blockerCount > 0 ? `${blockerCount} blockers` : `${validationCount} checks`,
        footerRight: recommendation?.actionable ? 'next up' : `${node.bindings.conceptHandles.length} concepts`,
        kind: node.id === graph.rootNodeIds[0] ? 'focus' : 'plan',
        tone: planTone(node, recommendation),
        selected: node.id === selection.selectedNodeId,
        hovered: node.id === selection.hoveredNodeId,
      },
    }
  })

  const edges: PrismFlowEdge[] = graph.edges.map((edge) => {
    const active = edge.id === selection.selectedEdgeId || edge.id === selection.hoveredEdgeId
    return {
      id: edge.id,
      source: edge.from,
      target: edge.to,
      type: 'smoothstep',
      animated: active,
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: active ? 'var(--accent)' : 'var(--edge)',
      },
      label: edge.kind === 'DependsOn' ? '' : formatLabel(edge.kind),
      style: {
        stroke: active ? 'var(--accent)' : 'var(--edge)',
        strokeWidth: active ? 2.4 : 1.6,
      },
      labelStyle: {
        fill: 'var(--muted)',
        fontSize: 11,
      },
      labelBgStyle: {
        fill: 'var(--panel-strong)',
        stroke: 'var(--border)',
      },
    }
  })

  return { nodes, edges }
}

function planTone(
  node: PlanNodeView,
  recommendation: PlanNodeRecommendationView | undefined,
): PrismFlowNodeTone {
  if (node.status === 'Completed') {
    return 'success'
  }
  if (node.status === 'Blocked' || node.status === 'Abandoned') {
    return 'danger'
  }
  if (node.status === 'InProgress' || node.status === 'Validating') {
    return 'accent'
  }
  if (recommendation?.actionable || node.status === 'Ready') {
    return 'warn'
  }
  return 'default'
}

function formatLabel(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase())
}
