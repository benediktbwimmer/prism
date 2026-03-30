import { useEffect, useState } from 'react'

import type { PrismPlansView } from '../types'

export function usePlansData(selectedPlanId: string | null) {
  const [plans, setPlans] = useState<PrismPlansView | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadPlans() {
      const params = new URLSearchParams()
      if (selectedPlanId) {
        params.set('planId', selectedPlanId)
      }
      const query = params.toString()
      const response = await fetch(query ? `/api/plans?${query}` : '/api/plans')
      if (!response.ok) {
        return
      }
      const next = (await response.json()) as PrismPlansView
      if (!cancelled) {
        setPlans(next)
      }
    }

    void loadPlans()

    return () => {
      cancelled = true
    }
  }, [selectedPlanId])

  return plans
}
