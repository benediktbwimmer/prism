import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react'

type UiMutationRequest = {
  action: string
  input: Record<string, unknown>
}

type PendingUiAction = {
  id: string
  fields: string[]
  label: string
  taskId: string
  target: Record<string, unknown>
}

type QueueMutationArgs = {
  fields: string[]
  label: string
  request: UiMutationRequest
  target: Record<string, unknown>
  taskId: string
}

type UiMutationQueueValue = {
  pendingActions: PendingUiAction[]
  pendingCount: number
  queueMutation: (args: QueueMutationArgs) => Promise<string>
  resolvePendingAction: (id: string) => void
}

const UiMutationQueueContext = createContext<UiMutationQueueValue | null>(null)

export function UiMutationQueueProvider({ children }: { children: ReactNode }) {
  const [pendingActions, setPendingActions] = useState<PendingUiAction[]>([])

  const value = useMemo<UiMutationQueueValue>(() => ({
    pendingActions,
    pendingCount: pendingActions.length,
    async queueMutation({ fields, label, request, target, taskId }) {
      const id = `ui-action-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
      setPendingActions((current) => [
        ...current,
        { id, fields, label, target, taskId },
      ])

      try {
        await postUiMutation({
          action: 'declare_work',
          input: {
            title: label,
          },
        })
        await postUiMutation(request)
        return id
      } catch (error) {
        setPendingActions((current) => current.filter((action) => action.id !== id))
        throw error
      }
    },
    resolvePendingAction(id) {
      setPendingActions((current) => current.filter((action) => action.id !== id))
    },
  }), [pendingActions])

  return (
    <UiMutationQueueContext.Provider value={value}>
      {children}
    </UiMutationQueueContext.Provider>
  )
}

export function useUiMutationQueue() {
  const context = useContext(UiMutationQueueContext)
  if (!context) {
    throw new Error('useUiMutationQueue must be used inside UiMutationQueueProvider')
  }
  return context
}

async function postUiMutation(request: UiMutationRequest) {
  const response = await fetch('/api/v1/mutate', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(request),
  })
  if (!response.ok) {
    const body = await response.text()
    throw new Error(body || `ui mutate ${response.status}`)
  }
  return response.json()
}
