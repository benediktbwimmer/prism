import { PlansPortfolio } from '../components/plans/PlansPortfolio'
import { PlanWorkspace } from '../components/plans/PlanWorkspace'
import { TaskDetailDrawer } from '../components/tasks/TaskDetailDrawer'
import { usePlansData } from '../hooks/usePlansData'

type PlansPageProps = {
  search: string
  onNavigate: (path: string) => void
}

export function PlansPage({ search, onNavigate }: PlansPageProps) {
  const query = new URLSearchParams(search)
  const requestedPlanId = query.get('plan')
  const requestedTaskId = query.get('task')
  const status = query.get('status')
  const searchText = query.get('search')
  const sort = query.get('sort')
  const agent = query.get('agent')
  const plansView = usePlansData({
    planId: requestedPlanId,
    status,
    search: searchText,
    sort,
    agent,
  })

  if (!plansView) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Strategic Console</p>
        <h2>Loading plan portfolio</h2>
        <p>Fetching the active plans, filters, and selected dependency graph.</p>
      </section>
    )
  }

  if (plansView.plans.length === 0) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Strategic Console</p>
        <h2>No plans match the current filters.</h2>
        <p>Clear the filters or create new work in PRISM to populate the operator portfolio.</p>
      </section>
    )
  }

  const selectedPlan = plansView.selectedPlan

  if (!selectedPlan) {
    return (
      <section className="panel hero-panel">
        <p className="eyebrow">Strategic Console</p>
        <h2>Selected plan detail is unavailable.</h2>
        <p>Choose another plan from the portfolio to restore the strategic workspace.</p>
      </section>
    )
  }

  function navigateWithPatch(patch: Record<string, string | null>) {
    const next = new URLSearchParams(search)
    for (const [key, value] of Object.entries(patch)) {
      if (value && value.trim().length > 0) {
        next.set(key, value)
      } else {
        next.delete(key)
      }
    }
    const serialized = next.toString()
    onNavigate(serialized ? `/plans?${serialized}` : '/plans')
  }

  return (
    <div className="strategic-console">
      <PlansPortfolio
        filters={plansView.filters}
        plans={plansView.plans}
        selectedPlanId={plansView.selectedPlanId ?? null}
        stats={plansView.stats}
        onAgentChange={(value) => navigateWithPatch({
          agent: value || null,
          plan: null,
          task: null,
        })}
        onSearchChange={(value) => navigateWithPatch({
          search: value || null,
          plan: null,
          task: null,
        })}
        onSelectPlan={(planId) => navigateWithPatch({
          plan: planId,
          task: null,
        })}
        onSortChange={(value) => navigateWithPatch({
          sort: value || null,
          plan: null,
          task: null,
        })}
        onStatusChange={(value) => navigateWithPatch({
          status: value || null,
          plan: null,
          task: null,
        })}
      />

      <PlanWorkspace
        plan={selectedPlan}
        selectedTaskId={requestedTaskId}
        onTaskSelect={(taskId) => {
          navigateWithPatch({
            task: taskId,
          })
        }}
      />

      <TaskDetailDrawer
        taskId={requestedTaskId}
        onClose={() => navigateWithPatch({ task: null })}
      />
    </div>
  )
}
