import { useEffect, useState } from 'react'

import type { PrismPlansView } from '../types'

const POLL_INTERVAL_MS = 2000

type PlansQueryOptions = {
  agent?: string | null
  planId?: string | null
  search?: string | null
  sort?: string | null
  status?: string | null
}

export function usePlansData(options: PlansQueryOptions) {
  const [plans, setPlans] = useState<PrismPlansView | null>(null)
  const {
    agent = null,
    planId = null,
    search = null,
    sort = null,
    status = null,
  } = options

  useEffect(() => {
    let cancelled = false
    let timeoutHandle: number | null = null

    async function loadPlans() {
      const params = new URLSearchParams()
      if (planId) {
        params.set('planId', planId)
      }
      if (status) {
        params.set('status', status)
      }
      if (search) {
        params.set('search', search)
      }
      if (sort) {
        params.set('sort', sort)
      }
      if (agent) {
        params.set('agent', agent)
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
  }, [agent, planId, search, sort, status])

  return plans
}
