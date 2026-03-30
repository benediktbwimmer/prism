import { useEffect, useState } from 'react'

import { PRISM_ROUTES, resolveRoute } from './appRoutes'
import { AppFrame } from './components/AppFrame'
import { useDashboardData } from './hooks/useDashboardData'
import { useThemeChoice } from './hooks/useThemeChoice'
import { DashboardPage } from './pages/DashboardPage'
import { GraphPage } from './pages/GraphPage'
import { OverviewPage } from './pages/OverviewPage'
import { PlansPage } from './pages/PlansPage'

export function App() {
  const [locationState, setLocationState] = useState(() => ({
    pathname: window.location.pathname,
    search: window.location.search,
  }))
  const route = resolveRoute(locationState.pathname)
  const dashboardState = useDashboardData()
  const { themeChoice, setThemeChoice } = useThemeChoice()

  useEffect(() => {
    document.title = route.title
  }, [route])

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

  let page = (
    <OverviewPage
      dashboard={dashboardState.dashboard}
      connection={dashboardState.connection}
      search={locationState.search}
      onNavigate={navigate}
    />
  )

  if (route.key === 'dashboard') {
    page = <DashboardPage {...dashboardState} search={locationState.search} />
  } else if (route.key === 'plans') {
    page = <PlansPage search={locationState.search} onNavigate={navigate} />
  } else if (route.key === 'graph') {
    page = <GraphPage search={locationState.search} onNavigate={navigate} />
  }

  return (
    <AppFrame
      connection={dashboardState.connection}
      currentPath={route.path}
      routes={PRISM_ROUTES}
      themeChoice={themeChoice}
      workspaceRoot={dashboardState.dashboard?.summary.session.workspaceRoot ?? null}
      onNavigate={navigate}
      onThemeChange={setThemeChoice}
    >
      {page}
    </AppFrame>
  )
}
