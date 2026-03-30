import { MarkerType } from '@xyflow/react'

import type {
  ConceptPacketView,
  ConceptRelationView,
  GraphPlanTouchpointView,
  PrismGraphView,
} from '../types'
import type { PrismFlowEdge, PrismFlowNode, PrismFlowNodeTone } from '../components/graph/PrismFlowCanvas'

type ConceptFlowSelection = {
  hoveredNodeId: string | null
  selectedEdgeId: string | null
  hoveredEdgeId: string | null
}

export function buildConceptFlow(
  graph: PrismGraphView,
  selection: ConceptFlowSelection,
) {
  const nodes: PrismFlowNode[] = []
  const edges: PrismFlowEdge[] = []

  nodes.push(makeConceptNode(graph.focus, 'focus', selection.hoveredNodeId))

  for (const relation of graph.focus.relations) {
    nodes.push(makeRelationNode(relation, selection.hoveredNodeId))
    edges.push(makeRelationEdge(graph.focus, relation, selection))
  }

  for (const plan of graph.relatedPlans) {
    const planNodeId = planNodeIdFor(plan)
    nodes.push({
      id: planNodeId,
      type: 'prismCard',
      position: { x: 0, y: 0 },
      style: {
        width: 240,
        height: 148,
      },
      data: {
        title: plan.plan.title,
        eyebrow: `Plan overlay · ${formatLabel(plan.plan.status)}`,
        body: plan.plan.goal,
        badge: `${plan.touchedNodes.length} touchpoints`,
        footerLeft: `${plan.plan.summary.actionableNodes} ready`,
        footerRight: `${plan.plan.summary.executionBlockedNodes} blocked`,
        kind: 'plan_overlay',
        tone: plan.plan.status === 'Active' ? 'accent' : 'default',
        hovered: selection.hoveredNodeId === planNodeId,
      },
    })
    edges.push({
      id: `plan-overlay:${graph.focus.handle}:${plan.plan.planId}`,
      source: focusNodeId(graph.focus),
      target: planNodeId,
      type: 'smoothstep',
      label: 'active plan',
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: 'var(--edge-soft)',
      },
      style: {
        stroke: 'var(--edge-soft)',
        strokeDasharray: '8 6',
        strokeWidth: 1.8,
      },
      labelStyle: {
        fill: 'var(--muted)',
        fontSize: 11,
      },
      labelBgStyle: {
        fill: 'var(--panel-strong)',
        stroke: 'var(--border)',
      },
    })
  }

  return { nodes, edges }
}

function makeConceptNode(
  concept: ConceptPacketView,
  kind: 'focus' | 'concept',
  hoveredNodeId: string | null,
): PrismFlowNode {
  const nodeId = focusNodeId(concept)
  return {
    id: nodeId,
    type: 'prismCard',
    position: { x: 0, y: 0 },
    style: {
      width: kind === 'focus' ? 312 : 252,
      height: kind === 'focus' ? 168 : 152,
    },
    data: {
      title: concept.canonicalName,
      eyebrow:
        kind === 'focus'
          ? `Focus concept · ${concept.handle}`
          : `${concept.relations.length} linked relations`,
      body: concept.summary,
      badge: kind === 'focus' ? 'focus' : 'drill down',
      footerLeft: `${concept.evidence.length} evidence`,
      footerRight: `${concept.relations.length} relations`,
      kind,
      tone: kind === 'focus' ? 'accent' : conceptTone(concept),
      hovered: hoveredNodeId === nodeId,
    },
  }
}

function makeRelationNode(
  relation: ConceptRelationView,
  hoveredNodeId: string | null,
): PrismFlowNode {
  const nodeId = relatedConceptNodeId(relation)
  return {
    id: nodeId,
    type: 'prismCard',
    position: { x: 0, y: 0 },
    style: {
      width: 252,
      height: 152,
    },
    data: {
      title: relation.relatedCanonicalName ?? relation.relatedHandle,
      eyebrow: `${formatLabel(relation.direction)} · ${relation.scope}`,
      body: relation.relatedSummary ?? 'No related concept summary recorded yet.',
      badge: formatLabel(relation.kind),
      footerLeft: `${Math.round(relation.confidence * 100)}% confidence`,
      footerRight: 'click to focus',
      kind: 'concept',
      tone: relationTone(relation),
      hovered: hoveredNodeId === nodeId,
    },
  }
}

function makeRelationEdge(
  focus: ConceptPacketView,
  relation: ConceptRelationView,
  selection: ConceptFlowSelection,
): PrismFlowEdge {
  const id = relationEdgeId(relation)
  const active = selection.selectedEdgeId === id || selection.hoveredEdgeId === id
  const source =
    relation.direction === 'Incoming'
      ? relatedConceptNodeId(relation)
      : focusNodeId(focus)
  const target =
    relation.direction === 'Incoming'
      ? focusNodeId(focus)
      : relatedConceptNodeId(relation)

  return {
    id,
    source,
    target,
    type: 'smoothstep',
    animated: active,
    label: formatLabel(relation.kind),
    markerEnd: {
      type: MarkerType.ArrowClosed,
      color: active ? 'var(--accent)' : 'var(--edge)',
    },
    style: {
      stroke: active ? 'var(--accent)' : 'var(--edge)',
      strokeWidth: active ? 2.4 : 1.7,
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
}

function conceptTone(concept: ConceptPacketView): PrismFlowNodeTone {
  if (concept.riskHint) {
    return 'warn'
  }
  if (concept.evidence.length > 2) {
    return 'accent'
  }
  return 'default'
}

function relationTone(relation: ConceptRelationView): PrismFlowNodeTone {
  if (relation.confidence >= 0.97) {
    return 'accent'
  }
  if (relation.confidence >= 0.9) {
    return 'warn'
  }
  return 'default'
}

export function focusNodeId(concept: ConceptPacketView) {
  return `concept:${concept.handle}`
}

export function relatedConceptNodeId(relation: ConceptRelationView) {
  return `concept:${relation.relatedHandle}`
}

export function relationEdgeId(relation: ConceptRelationView) {
  return `relation:${relation.kind}:${relation.direction}:${relation.relatedHandle}`
}

export function planNodeIdFor(plan: GraphPlanTouchpointView) {
  return `plan:${plan.plan.planId}`
}

function formatLabel(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase())
}
