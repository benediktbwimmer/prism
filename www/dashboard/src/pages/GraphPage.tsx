import { useGraphData } from '../hooks/useGraphData'
import type { ConceptPacketView, ConceptRelationView, GraphPlanTouchpointView } from '../types'

type GraphPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function GraphPage({ search, onNavigate }: GraphPageProps) {
  const requestedConceptHandle = new URLSearchParams(search).get('concept')
  const graph = useGraphData(requestedConceptHandle)

  if (!graph) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Prism Graph</p>
        <h2>Loading architecture explorer</h2>
        <p>Fetching the focus concept, typed neighborhood, and active plan touchpoints.</p>
      </section>
    )
  }

  return (
    <div className="page-stack">
      <section className="hero-bar panel">
        <div>
          <p className="eyebrow">Prism Graph</p>
          <h2>{graph.focus.canonicalName}</h2>
          <p className="lede">{graph.focus.summary}</p>
        </div>
        <div className="hero-actions">
          <span className="connection-pill">{graph.focus.relations.length} relations</span>
          <span className="connection-pill">{graph.relatedPlans.length} plan touchpoints</span>
        </div>
      </section>

      <section className="plans-layout">
        <aside className="panel plans-sidebar">
          <div className="panel-header">
            <h3>Semantic Zoom</h3>
            <span>{graph.entryConcepts.length}</span>
          </div>
          <div className="panel-body plans-list">
            {graph.entryConcepts.map((concept) => (
              <button
                key={concept.handle}
                type="button"
                className={`plan-list-button ${concept.handle === graph.selectedConceptHandle ? 'plan-list-button-active' : ''}`}
                onClick={() => onNavigate(`/graph?concept=${encodeURIComponent(concept.handle)}`)}
              >
                <div className="plan-list-topline">
                  <strong>{concept.canonicalName}</strong>
                  <span className="status-chip status-warn">{concept.relations.length} edges</span>
                </div>
                <p>{concept.summary}</p>
              </button>
            ))}
          </div>
        </aside>

        <div className="plans-main">
          <section className="status-grid">
            <article className="panel stat-card">
              <p className="stat-label">Focus Concept</p>
              <h3>{graph.focus.canonicalName}</h3>
              <p>{graph.focus.handle}</p>
            </article>
            <article className="panel stat-card">
              <p className="stat-label">Evidence</p>
              <h3>{graph.focus.evidence.length}</h3>
              <p>{graph.focus.coreMembers.length} core members and {graph.focus.likelyTests.length} likely tests.</p>
            </article>
            <article className="panel stat-card">
              <p className="stat-label">Relations</p>
              <h3>{graph.focus.relations.length}</h3>
              <p>Typed edges drive navigation instead of a whole-repo hairball.</p>
            </article>
            <article className="panel stat-card">
              <p className="stat-label">Plan Overlay</p>
              <h3>{graph.relatedPlans.length} plans</h3>
              <p>Active work touching this concept appears as a first overlay.</p>
            </article>
          </section>

          <section className="plans-main-grid">
            <article className="panel">
              <div className="panel-header">
                <h3>Neighborhood</h3>
                <span>{graph.focus.relations.length}</span>
              </div>
              <div className="panel-body signal-list">
                {graph.focus.relations.length === 0 ? (
                  <p className="empty-state">No typed relations are available for this concept yet.</p>
                ) : (
                  graph.focus.relations.map((relation) => (
                    <RelationCard
                      key={`${relation.kind}-${relation.relatedHandle}`}
                      relation={relation}
                      onNavigate={onNavigate}
                    />
                  ))
                )}
              </div>
            </article>

            <article className="panel">
              <div className="panel-header">
                <h3>Plans Touching This Concept</h3>
                <span>{graph.relatedPlans.length}</span>
              </div>
              <div className="panel-body signal-list">
                {graph.relatedPlans.length === 0 ? (
                  <p className="empty-state">No active plans are currently bound to this concept.</p>
                ) : (
                  graph.relatedPlans.map((touchpoint) => (
                    <TouchpointCard
                      key={touchpoint.plan.planId}
                      touchpoint={touchpoint}
                      onNavigate={onNavigate}
                    />
                  ))
                )}
              </div>
            </article>

            <article className="panel">
              <div className="panel-header">
                <h3>Evidence</h3>
                <span>{graph.focus.evidence.length}</span>
              </div>
              <div className="panel-body signal-list">
                {graph.focus.evidence.length === 0 ? (
                  <p className="empty-state">No evidence snippets are attached to this concept yet.</p>
                ) : (
                  graph.focus.evidence.map((item) => (
                    <article key={item} className="plan-card">
                      <p>{item}</p>
                    </article>
                  ))
                )}
              </div>
            </article>

            <article className="panel">
              <div className="panel-header">
                <h3>Core Members</h3>
                <span>{graph.focus.coreMembers.length}</span>
              </div>
              <div className="panel-body signal-list">
                <MemberSection
                  members={graph.focus.coreMembers}
                  emptyMessage="No core members are attached to this concept."
                />
              </div>
            </article>

            <article className="panel">
              <div className="panel-header">
                <h3>Supporting Members</h3>
                <span>{graph.focus.supportingMembers.length}</span>
              </div>
              <div className="panel-body signal-list">
                <MemberSection
                  members={graph.focus.supportingMembers}
                  emptyMessage="No supporting members are attached to this concept."
                />
              </div>
            </article>

            <article className="panel">
              <div className="panel-header">
                <h3>Likely Tests</h3>
                <span>{graph.focus.likelyTests.length}</span>
              </div>
              <div className="panel-body signal-list">
                <MemberSection
                  members={graph.focus.likelyTests}
                  emptyMessage="No likely tests are attached to this concept."
                />
              </div>
            </article>
          </section>
        </div>
      </section>
    </div>
  )
}

function RelationCard({
  relation,
  onNavigate,
}: {
  relation: ConceptRelationView
  onNavigate: (path: string) => void
}) {
  return (
    <button
      type="button"
      className="plan-card graph-relation-card"
      onClick={() => onNavigate(`/graph?concept=${encodeURIComponent(relation.relatedHandle)}`)}
    >
      <div className="plan-list-topline">
        <strong>{relation.relatedCanonicalName ?? relation.relatedHandle}</strong>
        <span className="status-chip status-warn">{formatLabel(relation.kind)}</span>
      </div>
      <p>{relation.relatedSummary ?? 'No summary is available for this related concept yet.'}</p>
      <div className="plan-inline-list">
        <span>{formatLabel(relation.direction)}</span>
        <span>{relation.scope}</span>
        <span>{Math.round(relation.confidence * 100)}%</span>
      </div>
    </button>
  )
}

function TouchpointCard({
  touchpoint,
  onNavigate,
}: {
  touchpoint: GraphPlanTouchpointView
  onNavigate: (path: string) => void
}) {
  return (
    <button
      type="button"
      className="plan-card graph-relation-card"
      onClick={() => onNavigate(`/plans?plan=${encodeURIComponent(touchpoint.plan.planId)}`)}
    >
      <div className="plan-list-topline">
        <strong>{touchpoint.plan.title}</strong>
        <span className={`status-chip status-${statusTone(touchpoint.plan.status)}`}>
          {formatLabel(touchpoint.plan.status)}
        </span>
      </div>
      <p>{touchpoint.plan.goal}</p>
      <div className="signal-list">
        {touchpoint.touchedNodes.map((node) => (
          <div key={node.nodeId} className="plan-inline-list">
            <span>{node.title}</span>
            <span className={`status-chip status-${statusTone(node.status)}`}>{formatLabel(node.status)}</span>
          </div>
        ))}
      </div>
    </button>
  )
}

function MemberSection({
  members,
  emptyMessage,
}: {
  members: ConceptPacketView['coreMembers']
  emptyMessage: string
}) {
  if (members.length === 0) {
    return <p className="empty-state">{emptyMessage}</p>
  }

  return (
    <>
      {members.map((member) => (
        <article key={member.path} className="plan-card">
          <div className="plan-list-topline">
            <strong>{member.path}</strong>
            {member.kind ? <span className="status-chip status-warn">{member.kind}</span> : null}
          </div>
        </article>
      ))}
    </>
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
    normalized.includes('success') ||
    normalized.includes('active')
  ) {
    return 'ok'
  }
  return 'warn'
}
