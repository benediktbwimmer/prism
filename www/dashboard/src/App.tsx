import { useEffect, useState } from 'react'

import { PRISM_ROUTES, resolveRoute } from './appRoutes'
import { AppFrame } from './components/AppFrame'
import { FleetPage } from './pages/FleetPage'
import { useSessionBootstrap } from './hooks/useSessionBootstrap'
import { UiMutationQueueProvider, useUiMutationQueue } from './hooks/useUiMutationQueue'
import { useThemeChoice } from './hooks/useThemeChoice'
import { PlansPage } from './pages/PlansPage'

export function App() {
  return (
    <UiMutationQueueProvider>
      <AppInner />
    </UiMutationQueueProvider>
  )
}

function AppInner() {
  const [locationState, setLocationState] = useState(() => ({
    pathname: window.location.pathname,
    search: window.location.search,
  }))
  const route = resolveRoute(locationState.pathname)
  const { bootstrap, connection } = useSessionBootstrap()
  const { pendingActions, pendingCount } = useUiMutationQueue()
  const { themeChoice, setThemeChoice } = useThemeChoice()

  useEffect(() => {
    document.title = route.title
  }, [route])

  useEffect(() => {
    if (window.location.pathname !== '/') {
      return
    }
    window.history.replaceState({}, '', '/plans')
    setLocationState({
      pathname: window.location.pathname,
      search: window.location.search,
    })
  }, [])

  useEffect(() => {
    function handlePopState() {
      setLocationState({
        pathname: window.location.pathname,
        search: window.location.search,
      })
    }

    window.addEventListener('popstate', handlePopState)
    return () => {
      window.removeEventListener('popstate', handlePopState)
    }
  }, [])

  function navigate(path: string) {
    const current = `${window.location.pathname}${window.location.search}`
    if (path === current) {
      return
    }
    window.history.pushState({}, '', path)
    setLocationState({
      pathname: window.location.pathname,
      search: window.location.search,
    })
  }

  let page = <PlansPage search={locationState.search} onNavigate={navigate} />

  if (route.key === 'fleet') {
    page = <FleetPage search={locationState.search} onNavigate={navigate} />
  }

  return (
    <AppFrame
      connection={connection}
      currentPath={route.path}
      operatorIdentity={bootstrap?.session.bridgeIdentity ?? null}
      pendingActionCount={pendingCount}
      pendingActionLabel={pendingActions[0]?.label ?? null}
      routes={PRISM_ROUTES}
      themeChoice={themeChoice}
      workspaceRoot={bootstrap?.session.workspaceRoot ?? null}
      onNavigate={navigate}
      onThemeChange={setThemeChoice}
    >
      {page}
    </AppFrame>
  )
}
