import { useEffect, useState } from 'react'

import type { PrismPlansView } from '../types'

const POLL_INTERVAL_MS = 2000

export function usePlansData(selectedPlanId: string | null) {
  const [plans, setPlans] = useState<PrismPlansView | null>(null)

  useEffect(() => {
    let cancelled = false
    let timeoutHandle: number | null = null

    async function loadPlans() {
      const params = new URLSearchParams()
      if (selectedPlanId) {
        params.set('planId', selectedPlanId)
      }
      const query = params.toString()
      try {
        const response = await fetch(query ? `/api/v1/plans?${query}` : '/api/v1/plans')
        if (!response.ok) {
          throw new Error(`plans ${response.status}`)
        }
        const next = (await response.json()) as PrismPlansView
        if (!cancelled) {
          setPlans(next)
        }
      } finally {
        if (!cancelled) {
          timeoutHandle = window.setTimeout(loadPlans, POLL_INTERVAL_MS)
        }
      }
    }

    void loadPlans()

    return () => {
      cancelled = true
      if (timeoutHandle !== null) {
        window.clearTimeout(timeoutHandle)
      }
    }
  }, [selectedPlanId])

  return plans
}
