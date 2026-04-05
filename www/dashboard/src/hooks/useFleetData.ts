import { useEffect, useState } from 'react'

import type { PrismUiFleetView } from '../types'

const POLL_INTERVAL_MS = 2000

export function useFleetData() {
  const [fleet, setFleet] = useState<PrismUiFleetView | null>(null)

  useEffect(() => {
    let cancelled = false
    let timeoutHandle: number | null = null

    async function loadFleet() {
      try {
        const response = await fetch('/api/v1/fleet')
        if (!response.ok) {
          throw new Error(`fleet ${response.status}`)
        }
        const next = (await response.json()) as PrismUiFleetView
        if (!cancelled) {
          setFleet(next)
        }
      } finally {
        if (!cancelled) {
          timeoutHandle = window.setTimeout(loadFleet, POLL_INTERVAL_MS)
        }
      }
    }

    void loadFleet()

    return () => {
      cancelled = true
      if (timeoutHandle !== null) {
        window.clearTimeout(timeoutHandle)
      }
    }
  }, [])

  return fleet
}
