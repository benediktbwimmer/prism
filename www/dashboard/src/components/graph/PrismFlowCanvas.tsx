import {
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  ReactFlowProvider,
  type Edge,
  type Node,
  type NodeMouseHandler,
  type EdgeMouseHandler,
  type NodeProps,
} from '@xyflow/react'
import { useEffect, useState } from 'react'

import { layoutFlowGraph, type FlowDirection } from './flowLayout'

export type PrismFlowNodeTone = 'default' | 'accent' | 'success' | 'warn' | 'danger'
export type PrismFlowNodeKind = 'plan' | 'focus' | 'concept' | 'plan_overlay'

export type PrismFlowNodeData = {
  title: string
  eyebrow?: string | null
  body?: string | null
  badge?: string | null
  footerLeft?: string | null
  footerRight?: string | null
  kind: PrismFlowNodeKind
  tone: PrismFlowNodeTone
  selected?: boolean
  hovered?: boolean
}

export type PrismFlowNode = Node<PrismFlowNodeData>
export type PrismFlowEdge = Edge

type PrismFlowCanvasProps = {
  nodes: PrismFlowNode[]
  edges: PrismFlowEdge[]
  direction?: FlowDirection
  onNodeActivate: (node: PrismFlowNode) => void
  onEdgeActivate: (edge: PrismFlowEdge) => void
  onNodeHoverChange?: (nodeId: string | null) => void
  onEdgeHoverChange?: (edgeId: string | null) => void
  onPaneActivate?: () => void
}

type PrismFlowCanvasInnerProps = Omit<PrismFlowCanvasProps, 'direction'> & {
  direction: FlowDirection
}

const nodeTypes = {
  prismCard: PrismFlowCardNode,
}

export function PrismFlowCanvas({
  nodes,
  edges,
  direction = 'RIGHT',
  onNodeActivate,
  onEdgeActivate,
  onNodeHoverChange,
  onEdgeHoverChange,
  onPaneActivate,
}: PrismFlowCanvasProps) {
  return (
    <ReactFlowProvider>
      <PrismFlowCanvasInner
        nodes={nodes}
        edges={edges}
        direction={direction}
        onNodeActivate={onNodeActivate}
        onEdgeActivate={onEdgeActivate}
        onNodeHoverChange={onNodeHoverChange}
        onEdgeHoverChange={onEdgeHoverChange}
        onPaneActivate={onPaneActivate}
      />
    </ReactFlowProvider>
  )
}

function PrismFlowCanvasInner({
  nodes,
  edges,
  direction,
  onNodeActivate,
  onEdgeActivate,
  onNodeHoverChange,
  onEdgeHoverChange,
  onPaneActivate,
}: PrismFlowCanvasInnerProps) {
  const [layouted, setLayouted] = useState<{ nodes: PrismFlowNode[]; edges: PrismFlowEdge[] }>({
    nodes,
    edges,
  })

  useEffect(() => {
    let cancelled = false

    async function runLayout() {
      const next = await layoutFlowGraph(nodes, edges, direction)
      if (!cancelled) {
        setLayouted(next)
      }
    }

    void runLayout()

    return () => {
      cancelled = true
    }
  }, [nodes, edges, direction])

  const handleNodeEnter: NodeMouseHandler<PrismFlowNode> = (_, node) => {
    onNodeHoverChange?.(node.id)
  }

  const handleNodeLeave: NodeMouseHandler<PrismFlowNode> = () => {
    onNodeHoverChange?.(null)
  }

  const handleEdgeEnter: EdgeMouseHandler<PrismFlowEdge> = (_, edge) => {
    onEdgeHoverChange?.(edge.id)
  }

  const handleEdgeLeave: EdgeMouseHandler<PrismFlowEdge> = () => {
    onEdgeHoverChange?.(null)
  }

  return (
    <div className="prism-flow-shell">
      <ReactFlow
        fitView
        fitViewOptions={{ padding: 0.18, maxZoom: 1.2 }}
        nodes={layouted.nodes}
        edges={layouted.edges}
        nodeTypes={nodeTypes}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        minZoom={0.3}
        maxZoom={1.6}
        onNodeClick={(_, node) => onNodeActivate(node)}
        onEdgeClick={(_, edge) => onEdgeActivate(edge)}
        onNodeMouseEnter={handleNodeEnter}
        onNodeMouseLeave={handleNodeLeave}
        onEdgeMouseEnter={handleEdgeEnter}
        onEdgeMouseLeave={handleEdgeLeave}
        onPaneClick={onPaneActivate}
        proOptions={{ hideAttribution: true }}
        defaultEdgeOptions={{
          markerEnd: {
            type: MarkerType.ArrowClosed,
          },
        }}
      >
        <Background gap={20} size={1.2} color="var(--flow-grid)" />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  )
}

function PrismFlowCardNode({ data }: NodeProps<PrismFlowNode>) {
  return (
    <div
      className={[
        'prism-flow-node',
        `prism-flow-node-${data.kind}`,
        `prism-flow-tone-${data.tone}`,
        data.selected ? 'prism-flow-node-selected' : '',
        data.hovered ? 'prism-flow-node-hovered' : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <Handle className="prism-flow-handle" type="target" position={Position.Left} />
      <div className="prism-flow-node-header">
        <div>
          {data.eyebrow ? <p className="prism-flow-node-eyebrow">{data.eyebrow}</p> : null}
          <h3>{data.title}</h3>
        </div>
        {data.badge ? <span className="prism-flow-node-badge">{data.badge}</span> : null}
      </div>
      {data.body ? <p className="prism-flow-node-body">{data.body}</p> : null}
      {(data.footerLeft || data.footerRight) ? (
        <div className="prism-flow-node-footer">
          <span>{data.footerLeft}</span>
          <span>{data.footerRight}</span>
        </div>
      ) : null}
      <Handle className="prism-flow-handle" type="source" position={Position.Right} />
    </div>
  )
}
