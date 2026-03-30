import { useEffect, useState } from 'react'

import type { PrismGraphView } from '../types'

export function useGraphData(selectedConceptHandle: string | null) {
  const [graph, setGraph] = useState<PrismGraphView | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadGraph() {
      const params = new URLSearchParams()
      if (selectedConceptHandle) {
        params.set('conceptHandle', selectedConceptHandle)
      }
      const query = params.toString()
      const response = await fetch(query ? `/api/graph?${query}` : '/api/graph')
      if (!response.ok) {
        return
      }
      const next = (await response.json()) as PrismGraphView
      if (!cancelled) {
        setGraph(next)
      }
    }

    void loadGraph()

    return () => {
      cancelled = true
    }
  }, [selectedConceptHandle])

  return graph
}
