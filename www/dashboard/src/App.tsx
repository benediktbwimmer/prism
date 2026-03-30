import { useEffect, useState } from 'react'

import { PRISM_ROUTES, resolveRoute } from './appRoutes'
import { AppFrame } from './components/AppFrame'
import { useDashboardData } from './hooks/useDashboardData'
import { useThemeChoice } from './hooks/useThemeChoice'
import { DashboardPage } from './pages/DashboardPage'
import { OverviewPage } from './pages/OverviewPage'
import { PlaceholderPage } from './pages/PlaceholderPage'

export function App() {
  const [pathname, setPathname] = useState(() => window.location.pathname)
  const route = resolveRoute(pathname)
  const dashboardState = useDashboardData()
  const { themeChoice, setThemeChoice } = useThemeChoice()

  useEffect(() => {
    document.title = route.title
  }, [route])

  useEffect(() => {
    function handlePopState() {
      setPathname(window.location.pathname)
    }

    window.addEventListener('popstate', handlePopState)
    return () => {
      window.removeEventListener('popstate', handlePopState)
    }
  }, [])

  function navigate(path: string) {
    if (path === window.location.pathname) {
      return
    }
    window.history.pushState({}, '', path)
    setPathname(window.location.pathname)
  }

  let page = (
    <OverviewPage
      dashboard={dashboardState.dashboard}
      connection={dashboardState.connection}
      onNavigate={navigate}
    />
  )

  if (route.key === 'dashboard') {
    page = <DashboardPage {...dashboardState} />
  } else if (route.key === 'plans') {
    page = (
      <PlaceholderPage
        title="Plans View"
        eyebrow="Prism Plans"
        description="The route and shell are now live. The next implementation step is the graph-native plan surface with blockers, ready nodes, claims, validations, and manual interventions."
        highlights={[
          `${dashboardState.dashboard?.coordination.activePlanCount ?? 0} active plans visible in current dashboard bootstrap`,
          `${dashboardState.dashboard?.coordination.readyTaskCount ?? 0} ready coordination tasks already available for the first focused view`,
          'Manual editing should stay structured: inspect, propose, confirm, then mutate through PRISM actions.',
        ]}
        ctaLabel="Open Dashboard"
        ctaPath="/dashboard"
        onNavigate={navigate}
      />
    )
  } else if (route.key === 'graph') {
    page = (
      <PlaceholderPage
        title="Architecture Graph"
        eyebrow="Prism Graph"
        description="This route establishes the future explorer surface. The first implementation pass should start with subsystem-level navigation, typed relations, and evidence-backed drill-downs instead of freeform editing."
        highlights={[
          'Semantic zoom matters more than showing the whole graph at once.',
          'The best early overlays are plan touchpoints, health, risk, and recent changes.',
          'Concept updates will land after the UI is real, so the graph can reflect the new product shape honestly.',
        ]}
        ctaLabel="Back To Overview"
        ctaPath="/"
        onNavigate={navigate}
      />
    )
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
