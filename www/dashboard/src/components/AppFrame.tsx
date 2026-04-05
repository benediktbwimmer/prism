import type { ReactNode } from 'react'

import type { PrismRoute } from '../appRoutes'
import type { BridgeIdentityView, ThemeChoice } from '../types'

type AppFrameProps = {
  children: ReactNode
  connection: 'connecting' | 'open' | 'closed'
  currentPath: string
  operatorIdentity: BridgeIdentityView | null
  routes: PrismRoute[]
  themeChoice: ThemeChoice
  workspaceRoot: string | null
  onNavigate: (path: string) => void
  onThemeChange: (theme: ThemeChoice) => void
}

export function AppFrame({
  children,
  connection,
  currentPath,
  operatorIdentity,
  routes,
  themeChoice,
  workspaceRoot,
  onNavigate,
  onThemeChange,
}: AppFrameProps) {
  return (
    <div className="app-shell">
      <header className="panel shell-header">
        <div className="brand-stack">
          <p className="eyebrow">PRISM Operator Console</p>
          <h1>Coordinate agents. Intervene when the graph needs a human.</h1>
          <p className="shell-copy">
            {workspaceRoot ?? 'Workspace not yet loaded'}
          </p>
        </div>

        <nav className="shell-nav" aria-label="Primary">
          {routes.map((route) => (
            <a
              key={route.path}
              href={route.path}
              className={route.path === currentPath ? 'nav-link nav-link-active' : 'nav-link'}
              onClick={(event) => {
                event.preventDefault()
                onNavigate(route.path)
              }}
            >
              <span>{route.label}</span>
              <small>{route.summary}</small>
            </a>
          ))}
        </nav>

        <div className="shell-controls">
          <div className="shell-identity">
            <p className="eyebrow">Operator</p>
            <strong>{operatorLabel(operatorIdentity)}</strong>
            <span>{operatorIdentity?.nextAction ?? 'Mutations from this console use the active local profile.'}</span>
          </div>
          <span className={`connection-pill connection-${connection}`}>{connection}</span>
          <label className="theme-picker">
            <span>Theme</span>
            <select value={themeChoice} onChange={(event) => onThemeChange(event.target.value as ThemeChoice)}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
        </div>
      </header>

      <div className="shell-content">
        {children}
      </div>
    </div>
  )
}

function operatorLabel(identity: BridgeIdentityView | null) {
  if (!identity) {
    return 'Loading local profile'
  }
  return identity.profile
    ?? identity.principalId
    ?? identity.status
}
