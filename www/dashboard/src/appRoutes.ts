export type PrismRouteKey = 'overview' | 'plans' | 'graph'

export type PrismRoute = {
  key: PrismRouteKey
  path: string
  label: string
  title: string
  summary: string
}

export const PRISM_ROUTES: PrismRoute[] = [
  {
    key: 'overview',
    path: '/',
    label: 'Overview',
    title: 'PRISM Overview',
    summary: 'Orient to the repo, the runtime, and the current work.',
  },
  {
    key: 'plans',
    path: '/plans',
    label: 'Plans',
    title: 'PRISM Plans',
    summary: 'Track intent, blockers, and execution state.',
  },
  {
    key: 'graph',
    path: '/graph',
    label: 'Graph',
    title: 'PRISM Graph',
    summary: 'Explore architecture, evidence, and overlays.',
  },
]

export function resolveRoute(pathname: string): PrismRoute {
  const normalized = pathname.endsWith('/') && pathname !== '/'
    ? pathname.slice(0, -1)
    : pathname
  return PRISM_ROUTES.find((route) => route.path === normalized) ?? PRISM_ROUTES[0]
}
