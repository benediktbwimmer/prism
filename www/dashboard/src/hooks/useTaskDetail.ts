import { useEffect, useState } from 'react'

import type { PrismUiTaskDetailView } from '../types'

const POLL_INTERVAL_MS = 2000

export function useTaskDetail(taskId: string | null) {
  const [detail, setDetail] = useState<PrismUiTaskDetailView | null>(null)
  const [status, setStatus] = useState<'idle' | 'loading' | 'error'>('idle')

  useEffect(() => {
    if (!taskId) {
      setDetail(null)
      setStatus('idle')
      return
    }
    const activeTaskId = taskId

    let cancelled = false
    let timeoutHandle: number | null = null

    async function loadTaskDetail() {
      try {
        setStatus((current) => (current === 'idle' ? 'loading' : current))
        const response = await fetch(`/api/v1/tasks/${encodeURIComponent(activeTaskId)}`)
        if (!response.ok) {
          throw new Error(`task detail ${response.status}`)
        }
        const next = (await response.json()) as PrismUiTaskDetailView
        if (!cancelled) {
          setDetail(next)
          setStatus('idle')
        }
      } catch {
        if (!cancelled) {
          setStatus('error')
        }
      } finally {
        if (!cancelled) {
          timeoutHandle = window.setTimeout(loadTaskDetail, POLL_INTERVAL_MS)
        }
      }
    }

    void loadTaskDetail()

    return () => {
      cancelled = true
      if (timeoutHandle !== null) {
        window.clearTimeout(timeoutHandle)
      }
    }
  }, [taskId])

  return { detail, status }
}
