import { useEffect, useState } from 'react'

import type { PrismUiSessionBootstrapView } from '../types'

const DEFAULT_POLL_INTERVAL_MS = 2000

export function useSessionBootstrap() {
  const [bootstrap, setBootstrap] = useState<PrismUiSessionBootstrapView | null>(null)
  const [connection, setConnection] = useState<'connecting' | 'open' | 'closed'>('connecting')

  useEffect(() => {
    let cancelled = false
    let timeoutHandle: number | null = null

    async function poll() {
      try {
        const response = await fetch('/api/v1/session')
        if (!response.ok) {
          throw new Error(`session bootstrap ${response.status}`)
        }
        const next = (await response.json()) as PrismUiSessionBootstrapView
        if (cancelled) {
          return
        }
        setBootstrap(next)
        setConnection('open')
        timeoutHandle = window.setTimeout(
          poll,
          next.pollingIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
        )
      } catch {
        if (cancelled) {
          return
        }
        setConnection((current) => (current === 'connecting' ? 'connecting' : 'closed'))
        timeoutHandle = window.setTimeout(poll, DEFAULT_POLL_INTERVAL_MS)
      }
    }

    void poll()

    return () => {
      cancelled = true
      if (timeoutHandle !== null) {
        window.clearTimeout(timeoutHandle)
      }
    }
  }, [])

  return { bootstrap, connection }
}
