import { useEffect, useState } from 'react'

import type { ThemeChoice } from '../types'

const THEME_KEY = 'prism-dashboard-theme'

export function useThemeChoice() {
  const [themeChoice, setThemeChoice] = useState<ThemeChoice>(() => {
    const stored = window.localStorage.getItem(THEME_KEY)
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      return stored
    }
    return 'system'
  })

  useEffect(() => {
    window.localStorage.setItem(THEME_KEY, themeChoice)
    const root = document.documentElement
    const resolvedDark =
      themeChoice === 'dark' ||
      (themeChoice === 'system' &&
        window.matchMedia('(prefers-color-scheme: dark)').matches)
    root.dataset.theme = resolvedDark ? 'dark' : 'light'
  }, [themeChoice])

  return {
    setThemeChoice,
    themeChoice,
  }
}
