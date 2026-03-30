import type { Edge, Node } from '@xyflow/react'
import ELK from 'elkjs/lib/elk.bundled.js'

const elk = new ELK()

export type FlowDirection = 'RIGHT' | 'DOWN'

export async function layoutFlowGraph<TNode extends Node, TEdge extends Edge>(
  nodes: TNode[],
  edges: TEdge[],
  direction: FlowDirection,
) {
  if (nodes.length === 0) {
    return { nodes, edges }
  }

  try {
    const layout = await elk.layout({
      id: 'prism-root',
      layoutOptions: {
        'elk.algorithm': 'layered',
        'elk.direction': direction,
        'elk.layered.spacing.nodeNodeBetweenLayers': '120',
        'elk.spacing.nodeNode': '56',
        'elk.padding': '[top=32,left=32,bottom=32,right=32]',
      },
      children: nodes.map((node) => ({
        id: node.id,
        width: typeof node.style?.width === 'number' ? node.style.width : 260,
        height: typeof node.style?.height === 'number' ? node.style.height : 160,
      })),
      edges: edges.map((edge) => ({
        id: edge.id,
        sources: [edge.source],
        targets: [edge.target],
      })),
    })

    const positionById = new Map(
      (layout.children ?? []).map((child) => [
        child.id,
        {
          x: child.x ?? 0,
          y: child.y ?? 0,
        },
      ]),
    )

    return {
      nodes: nodes.map((node) => ({
        ...node,
        position: positionById.get(node.id) ?? node.position,
      })),
      edges,
    }
  } catch {
    return {
      nodes: nodes.map((node, index) => ({
        ...node,
        position: {
          x: (index % 3) * 320,
          y: Math.floor(index / 3) * 220,
        },
      })),
      edges,
    }
  }
}
