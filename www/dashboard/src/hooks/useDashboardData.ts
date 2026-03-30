import {
  startTransition,
  useEffect,
  useRef,
  useState,
} from 'react'

import type {
  ActiveOperationView,
  DashboardBootstrapView,
  DashboardCoordinationSummaryView,
  DashboardOperationDetailView,
  DashboardSummaryView,
  DashboardTaskSnapshotView,
  MutationLogEntryView,
  QueryLogEntryView,
  RuntimeRefreshEvent,
} from '../types'

export function useDashboardData() {
  const [dashboard, setDashboard] = useState<DashboardBootstrapView | null>(null)
  const [connection, setConnection] = useState<'connecting' | 'open' | 'closed'>('connecting')
  const [selectedOperationId, setSelectedOperationId] = useState<string | null>(null)
  const [selectedOperation, setSelectedOperation] = useState<DashboardOperationDetailView | null>(null)
  const [detailStatus, setDetailStatus] = useState<'idle' | 'loading' | 'error'>('idle')
  const selectedOperationIdRef = useRef<string | null>(null)

  useEffect(() => {
    let cancelled = false

    async function loadDashboard() {
      const response = await fetch('/dashboard/api/bootstrap')
      const next = (await response.json()) as DashboardBootstrapView
      if (cancelled) {
        return
      }
      startTransition(() => setDashboard(next))
    }

    void loadDashboard()

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    selectedOperationIdRef.current = selectedOperationId
  }, [selectedOperationId])

  async function loadOperationDetail(id: string) {
    setDetailStatus('loading')
    setSelectedOperation(null)

    try {
      const response = await fetch(`/dashboard/api/operations/${encodeURIComponent(id)}`)
      if (!response.ok) {
        throw new Error(`operation detail ${response.status}`)
      }
      const detail = (await response.json()) as DashboardOperationDetailView
      startTransition(() => {
        setSelectedOperation(detail)
        setDetailStatus('idle')
      })
    } catch {
      startTransition(() => {
        setSelectedOperation(null)
        setDetailStatus('error')
      })
    }
  }

  function selectOperation(id: string) {
    setSelectedOperationId(id)
    void loadOperationDetail(id)
  }

  function clearSelectedOperation() {
    setSelectedOperationId(null)
    setSelectedOperation(null)
    setDetailStatus('idle')
  }

  useEffect(() => {
    async function refreshSummary() {
      const response = await fetch('/dashboard/api/summary')
      const summary = (await response.json()) as DashboardSummaryView
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                summary,
              }
            : current,
        )
      })
    }

    async function refreshCoordination() {
      const response = await fetch('/dashboard/api/coordination')
      const coordination = (await response.json()) as DashboardCoordinationSummaryView
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                coordination,
              }
            : current,
        )
      })
    }

    function handleActiveEvent(operation: ActiveOperationView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = [operation, ...current.operations.active.filter((item) => item.id !== operation.id)]
            .sort((left, right) => right.startedAt - left.startedAt)
            .slice(0, 30)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
            },
            operations: {
              ...current.operations,
              active,
            },
          }
        })
      })

      if (selectedOperationIdRef.current === operation.id) {
        void loadOperationDetail(operation.id)
      }
    }

    function handleFinishedQuery(query: QueryLogEntryView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = current.operations.active.filter((item) => item.id !== query.id)
          const recentQueries = [query, ...current.operations.recentQueries.filter((item) => item.id !== query.id)].slice(0, 20)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
              recentQueryErrorCount: recentQueries.filter((item) => !item.success).length,
            },
            operations: {
              ...current.operations,
              active,
              recentQueries,
            },
          }
        })
      })

      if (selectedOperationIdRef.current === query.id) {
        void loadOperationDetail(query.id)
      }
    }

    function handleFinishedMutation(mutation: MutationLogEntryView) {
      startTransition(() => {
        setDashboard((current) => {
          if (!current) return current
          const active = current.operations.active.filter((item) => item.id !== mutation.id)
          const recentMutations = [mutation, ...current.operations.recentMutations.filter((item) => item.id !== mutation.id)].slice(0, 20)
          return {
            ...current,
            summary: {
              ...current.summary,
              activeQueryCount: active.filter((item) => item.kind === 'query').length,
              activeMutationCount: active.filter((item) => item.kind === 'mutation').length,
            },
            operations: {
              ...current.operations,
              active,
              recentMutations,
            },
          }
        })
      })

      if (selectedOperationIdRef.current === mutation.id) {
        void loadOperationDetail(mutation.id)
      }
    }

    function handleTaskUpdate(task: DashboardTaskSnapshotView) {
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                task,
                summary: {
                  ...current.summary,
                  session: task.session,
                },
              }
            : current,
        )
      })
    }

    function handleCoordinationUpdate(coordination: DashboardCoordinationSummaryView) {
      startTransition(() => {
        setDashboard((current) =>
          current
            ? {
                ...current,
                coordination,
              }
            : current,
        )
      })
    }

    const source = new EventSource('/dashboard/events')
    setConnection('connecting')

    source.onopen = () => setConnection('open')
    source.onerror = () => setConnection('closed')

    source.addEventListener('query.started', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('query.phase', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('query.finished', (event) => {
      handleFinishedQuery(JSON.parse(event.data) as QueryLogEntryView)
    })
    source.addEventListener('mutation.started', (event) => {
      handleActiveEvent(JSON.parse(event.data) as ActiveOperationView)
    })
    source.addEventListener('mutation.finished', (event) => {
      handleFinishedMutation(JSON.parse(event.data) as MutationLogEntryView)
    })
    source.addEventListener('task.updated', (event) => {
      handleTaskUpdate(JSON.parse(event.data) as DashboardTaskSnapshotView)
    })
    source.addEventListener('coordination.updated', (event) => {
      handleCoordinationUpdate(JSON.parse(event.data) as DashboardCoordinationSummaryView)
    })
    source.addEventListener('runtime.refreshed', (event) => {
      const payload = JSON.parse(event.data) as RuntimeRefreshEvent
      void refreshSummary()
      if (payload.coordinationReloaded) {
        void refreshCoordination()
      }
    })

    return () => {
      source.close()
    }
  }, [])

  return {
    clearSelectedOperation,
    connection,
    dashboard,
    detailStatus,
    selectOperation,
    selectedOperation,
    selectedOperationId,
  }
}
