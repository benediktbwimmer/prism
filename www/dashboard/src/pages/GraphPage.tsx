import { useState, type ReactNode } from 'react'

import { PrismFlowCanvas } from '../components/graph/PrismFlowCanvas'
import {
  buildConceptFlow,
  focusNodeId,
  planNodeIdFor,
  relatedConceptNodeId,
  relationEdgeId,
} from '../graph/conceptFlowModel'
import { useGraphData } from '../hooks/useGraphData'
import type { ConceptRelationView, GraphPlanTouchpointView } from '../types'

type GraphPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function GraphPage({ search, onNavigate }: GraphPageProps) {
  const requestedConceptHandle = new URLSearchParams(search).get('concept')
  const graph = useGraphData(requestedConceptHandle)
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null)
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null)
  const [hoveredEdgeId, setHoveredEdgeId] = useState<string | null>(null)

  if (!graph) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Graph</p>
        <h2>Loading architecture explorer</h2>
        <p>Fetching the focus concept, typed neighborhood, and active plan touchpoints.</p>
      </section>
    )
  }

  const relationByEdgeId = new Map(
    graph.focus.relations.map((relation) => [relationEdgeId(relation), relation]),
  )
  const planByNodeId = new Map(graph.relatedPlans.map((plan) => [planNodeIdFor(plan), plan]))
  const relationByNodeId = new Map(
    graph.focus.relations.map((relation) => [relatedConceptNodeId(relation), relation]),
  )
  const flow = buildConceptFlow(graph, {
    hoveredNodeId,
    selectedEdgeId,
    hoveredEdgeId,
  })

  const activeRelation = selectedEdgeId
    ? relationByEdgeId.get(selectedEdgeId)
    : hoveredEdgeId
      ? relationByEdgeId.get(hoveredEdgeId)
      : null
  const hoveredPlan = hoveredNodeId ? planByNodeId.get(hoveredNodeId) ?? null : null
  const hoveredRelationNode = hoveredNodeId ? relationByNodeId.get(hoveredNodeId) ?? null : null
  const hoveringFocus = hoveredNodeId === focusNodeId(graph.focus)

  return (
    <div className="page-stack flow-page">
      <section className="hero-bar panel flow-hero">
        <div>
          <p className="eyebrow">Prism Graph</p>
          <h2>{graph.focus.canonicalName}</h2>
          <p className="lede">{graph.focus.summary}</p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{graph.focus.relations.length} relations</span>
          <span className="connection-pill">{graph.relatedPlans.length} plan overlays</span>
          <span className="connection-pill">{graph.focus.evidence.length} evidence</span>
        </div>
      </section>

      <section className="panel flow-selector-panel">
        <div className="panel-header">
          <h3>Semantic Zoom</h3>
          <span>{graph.entryConcepts.length}</span>
        </div>
        <div className="panel-body flow-selector-row">
          {graph.entryConcepts.map((concept) => (
            <button
              key={concept.handle}
              type="button"
              className={`flow-selector ${concept.handle === graph.selectedConceptHandle ? 'flow-selector-active' : ''}`}
              onClick={() => onNavigate(`/graph?concept=${encodeURIComponent(concept.handle)}`)}
            >
              <span className="flow-selector-title">{concept.canonicalName}</span>
              <span className="flow-selector-meta">
                <strong>{concept.relations.length}</strong> relations
              </span>
              <span className="flow-selector-meta">
                <strong>{concept.evidence.length}</strong> evidence
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="flow-layout">
        <section className="panel flow-stage-panel">
          <div className="panel-header">
            <h3>Architecture Graph</h3>
            <span>{flow.nodes.length} nodes / {flow.edges.length} edges</span>
          </div>
          <div className="flow-stage-meta">
            <span>Concept-first neighborhood</span>
            <span>Click concept nodes to focus</span>
            <span>Hover nodes for preview</span>
          </div>
          <div className="flow-stage">
            <PrismFlowCanvas
              nodes={flow.nodes}
              edges={flow.edges}
              onNodeActivate={(node) => {
                if (node.id === focusNodeId(graph.focus)) {
                  return
                }
                const plan = planByNodeId.get(node.id)
                if (plan) {
                  onNavigate(`/plans?plan=${encodeURIComponent(plan.plan.planId)}`)
                  return
                }
                const relation = relationByNodeId.get(node.id)
                if (relation) {
                  onNavigate(`/graph?concept=${encodeURIComponent(relation.relatedHandle)}`)
                }
              }}
              onEdgeActivate={(edge) => {
                setSelectedEdgeId(edge.id)
              }}
              onNodeHoverChange={setHoveredNodeId}
              onEdgeHoverChange={setHoveredEdgeId}
              onPaneActivate={() => setSelectedEdgeId(null)}
            />
          </div>
        </section>

        <aside className="panel flow-inspector">
          <div className="panel-header">
            <h3>Inspector</h3>
            <span>{activeRelation ? 'edge' : hoveredPlan ? 'plan' : hoveredRelationNode ? 'concept' : 'focus'}</span>
          </div>
          <div className="panel-body flow-inspector-body">
            {activeRelation ? (
              <RelationInspector relation={activeRelation} />
            ) : hoveredPlan ? (
              <PlanOverlayInspector plan={hoveredPlan} onNavigate={onNavigate} />
            ) : hoveredRelationNode ? (
              <ConceptPreviewInspector relation={hoveredRelationNode} />
            ) : hoveringFocus ? (
              <FocusConceptInspector graph={graph} />
            ) : (
              <FocusConceptInspector graph={graph} />
            )}
          </div>
        </aside>
      </section>

      <section className="flow-support-grid">
        <CompactGraphPanel
          title="Evidence"
          count={graph.focus.evidence.length}
          emptyMessage="No evidence is attached to this focus concept."
          items={graph.focus.evidence.slice(0, 4).map((item) => (
            <article key={item} className="compact-item">
              <strong>Evidence</strong>
              <p>{item}</p>
            </article>
          ))}
        />
        <CompactGraphPanel
          title="Plan Touchpoints"
          count={graph.relatedPlans.length}
          emptyMessage="No active plans currently touch this concept."
          items={graph.relatedPlans.slice(0, 4).map((plan) => (
            <button
              key={plan.plan.planId}
              type="button"
              className="compact-item"
              onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(plan.plan.planId)}`)}
            >
              <strong>{plan.plan.title}</strong>
              <p>{plan.touchedNodes.length} touched nodes</p>
            </button>
          ))}
        />
        <CompactGraphPanel
          title="Concept Payload"
          count={graph.focus.aliases.length}
          emptyMessage="No aliases are attached to this concept."
          items={graph.focus.aliases.slice(0, 4).map((alias) => (
            <article key={alias} className="compact-item">
              <strong>Alias</strong>
              <p>{alias}</p>
            </article>
          ))}
        />
      </section>
    </div>
  )
}

function FocusConceptInspector({ graph }: { graph: NonNullable<ReturnType<typeof useGraphData>> }) {
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Focus Concept</p>
        <h3>{graph.focus.canonicalName}</h3>
        <p>{graph.focus.summary}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Relations" value={graph.focus.relations.length} />
        <StatPill label="Evidence" value={graph.focus.evidence.length} />
        <StatPill label="Plans" value={graph.relatedPlans.length} />
        <StatPill label="Aliases" value={graph.focus.aliases.length} />
      </div>
      <InspectorSection title="How to use this graph">
        <ul className="inspector-list">
          <li>Hover a related concept node to preview it.</li>
          <li>Click a concept node to drill into that neighborhood.</li>
          <li>Click an edge to inspect the typed architectural relation.</li>
        </ul>
      </InspectorSection>
      <InspectorSection title="Risk hint">
        <p>{graph.focus.riskHint ?? 'No explicit risk hint is attached to this concept right now.'}</p>
      </InspectorSection>
    </div>
  )
}

function ConceptPreviewInspector({ relation }: { relation: ConceptRelationView }) {
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Hover Preview</p>
        <h3>{relation.relatedCanonicalName ?? relation.relatedHandle}</h3>
        <p>{relation.relatedSummary ?? 'No summary is available for this related concept yet.'}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Relation" value={formatLabel(relation.kind)} />
        <StatPill label="Direction" value={formatLabel(relation.direction)} />
        <StatPill label="Scope" value={relation.scope} />
        <StatPill label="Confidence" value={`${Math.round(relation.confidence * 100)}%`} />
      </div>
      <InspectorSection title="Interaction">
        <p>Click this node to refocus the graph on this concept and load its own neighborhood.</p>
      </InspectorSection>
    </div>
  )
}

function RelationInspector({ relation }: { relation: ConceptRelationView }) {
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Typed Relation</p>
        <h3>{formatLabel(relation.kind)}</h3>
        <p>{relation.relatedCanonicalName ?? relation.relatedHandle}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Direction" value={formatLabel(relation.direction)} />
        <StatPill label="Scope" value={relation.scope} />
        <StatPill label="Confidence" value={`${Math.round(relation.confidence * 100)}%`} />
        <StatPill label="Evidence" value={relation.evidence.length} />
      </div>
      <InspectorSection title="Relation evidence">
        {relation.evidence.length > 0 ? (
          <ul className="inspector-list">
            {relation.evidence.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        ) : (
          <p>No explicit evidence snippets were attached to this relation.</p>
        )}
      </InspectorSection>
    </div>
  )
}

function PlanOverlayInspector({
  plan,
  onNavigate,
}: {
  plan: GraphPlanTouchpointView
  onNavigate: (path: string) => void
}) {
  return (
    <div className="inspector-stack">
      <section className="inspector-hero">
        <p className="eyebrow">Plan Overlay</p>
        <h3>{plan.plan.title}</h3>
        <p>{plan.plan.goal}</p>
      </section>
      <div className="inspector-stat-grid">
        <StatPill label="Ready" value={plan.plan.summary.actionableNodes} />
        <StatPill label="Blocked" value={plan.plan.summary.executionBlockedNodes} />
        <StatPill label="Touched" value={plan.touchedNodes.length} />
        <StatPill label="Status" value={formatLabel(plan.plan.status)} />
      </div>
      <InspectorSection title="Touched nodes">
        <ul className="inspector-list">
          {plan.touchedNodes.map((node) => (
            <li key={node.nodeId}>{node.title} · {formatLabel(node.status)}</li>
          ))}
        </ul>
      </InspectorSection>
      <button
        type="button"
        className="ghost-button"
        onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(plan.plan.planId)}`)}
      >
        Open plan
      </button>
    </div>
  )
}

function CompactGraphPanel({
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
