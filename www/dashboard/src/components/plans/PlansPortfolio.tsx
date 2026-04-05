import type {
  PlanListEntryView,
  PrismUiPlansFiltersView,
  PrismUiPlansStatsView,
} from '../../types'

type PlansPortfolioProps = {
  filters: PrismUiPlansFiltersView
  plans: PlanListEntryView[]
  selectedPlanId: string | null
  stats: PrismUiPlansStatsView
  onAgentChange: (value: string) => void
  onSearchChange: (value: string) => void
  onSelectPlan: (planId: string) => void
  onSortChange: (value: string) => void
  onStatusChange: (value: string) => void
}

export function PlansPortfolio({
  filters,
  plans,
  selectedPlanId,
  stats,
  onAgentChange,
  onSearchChange,
  onSelectPlan,
  onSortChange,
  onStatusChange,
}: PlansPortfolioProps) {
  return (
    <aside className="plans-portfolio panel">
      <div className="plans-portfolio-head">
        <div>
          <p className="eyebrow">Strategic</p>
          <h2>Portfolio</h2>
          <p className="lede">
            Sort the active plan stack, filter for the work that matters now, and open a graph when you need to intervene.
          </p>
        </div>
        <div className="plans-portfolio-stats">
          <StatChip label="Visible" value={stats.visiblePlans} />
          <StatChip label="Active" value={stats.activePlans} />
          <StatChip label="Done" value={stats.completedPlans} />
          <StatChip label="Archived" value={stats.archivedPlans} />
        </div>
      </div>

      <div className="plans-filter-grid">
        <label className="plans-filter-field">
          <span>Search</span>
          <input
            type="search"
            placeholder="Plan title or goal"
            value={filters.search ?? ''}
            onChange={(event) => onSearchChange(event.target.value)}
          />
        </label>
        <label className="plans-filter-field">
          <span>Status</span>
          <select value={filters.status} onChange={(event) => onStatusChange(event.target.value)}>
            <option value="active">Active</option>
            <option value="all">All</option>
            <option value="completed">Completed</option>
            <option value="archived">Archived</option>
            <option value="blocked">Blocked</option>
            <option value="abandoned">Abandoned</option>
            <option value="draft">Draft</option>
          </select>
        </label>
        <label className="plans-filter-field">
          <span>Sort</span>
          <select value={filters.sort} onChange={(event) => onSortChange(event.target.value)}>
            <option value="priority">Priority</option>
            <option value="completion">Completion</option>
            <option value="title">Title</option>
          </select>
        </label>
        <label className="plans-filter-field">
          <span>Agent</span>
          <input
            type="search"
            placeholder="runtime or principal"
            value={filters.agent ?? ''}
            onChange={(event) => onAgentChange(event.target.value)}
          />
        </label>
      </div>

      <div className="plans-portfolio-list">
        {plans.map((plan) => {
          const completion = completionPercent(plan.planSummary)
          const statusClass = `portfolio-status-${String(plan.status).toLowerCase()}`
          return (
            <button
              key={plan.planId}
              type="button"
              className={[
                'plan-card',
                plan.planId === selectedPlanId ? 'plan-card-active' : '',
              ].filter(Boolean).join(' ')}
              onClick={() => onSelectPlan(plan.planId)}
            >
              <div className="plan-card-topline">
                <span className={`table-status ${statusClass}`}>{formatLabel(String(plan.status))}</span>
                <span className="plan-card-priority">
                  I{plan.scheduling.importance} · U{plan.scheduling.urgency}
                </span>
              </div>
              <div className="plan-card-copy">
                <h3>{plan.title}</h3>
                <p>{plan.summary}</p>
              </div>
              <div className="plan-card-progress">
                <div className="plan-card-progress-track">
                  <span style={{ width: `${completion}%` }} />
                </div>
                <div className="plan-card-progress-meta">
                  <strong>{completion}% complete</strong>
                  <span>{plan.planSummary.actionableNodes} ready</span>
                  <span>{plan.planSummary.inProgressNodes} active</span>
                  <span>{plan.planSummary.executionBlockedNodes} blocked</span>
                </div>
              </div>
            </button>
          )
        })}
      </div>
    </aside>
  )
}

function StatChip({ label, value }: { label: string; value: number }) {
  return (
    <div className="plans-stat-chip">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function completionPercent(summary: PlanListEntryView['planSummary']) {
  if (summary.totalNodes <= 0) {
    return 0
  }
  return Math.round((summary.completedNodes / summary.totalNodes) * 100)
}

function formatLabel(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (character) => character.toUpperCase())
}
