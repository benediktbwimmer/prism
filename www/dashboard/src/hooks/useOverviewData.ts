import { useEffect, useState } from 'react'

import type { PrismOverviewView } from '../types'

export function useOverviewData() {
  const [overview, setOverview] = useState<PrismOverviewView | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadOverview() {
      const response = await fetch('/api/overview')
      if (!response.ok) {
        return
      }
      const next = (await response.json()) as PrismOverviewView
      if (!cancelled) {
        setOverview(next)
      }
    }

    void loadOverview()

    return () => {
      cancelled = true
    }
  }, [])

  return overview
}
