export type PrismRouteKey = 'plans' | 'fleet'

export type PrismRoute = {
  key: PrismRouteKey
  path: string
  label: string
  title: string
  summary: string
}

export const PRISM_ROUTES: PrismRoute[] = [
  {
    key: 'plans',
    path: '/plans',
    label: 'Strategic',
    title: 'PRISM Operator Console',
    summary: 'Plans, blockers, graph state, and human intervention.',
  },
  {
    key: 'fleet',
    path: '/fleet',
    label: 'Utilization',
    title: 'PRISM Fleet Timeline',
    summary: 'Runtime lanes, task leases, and stuck or idle agents.',
  },
]

export function resolveRoute(pathname: string): PrismRoute {
  const normalized = pathname.endsWith('/') && pathname !== '/'
    ? pathname.slice(0, -1)
    : pathname
  if (normalized === '/') {
    return PRISM_ROUTES[0]
  }
  return PRISM_ROUTES.find((route) => route.path === normalized) ?? PRISM_ROUTES[0]
}
