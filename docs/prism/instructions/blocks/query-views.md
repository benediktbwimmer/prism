## Query Views

- Treat the typed query views as first-class workflow tools:
  - `repoPlaybook()` for repo workflow, build, test, lint, format, and gotcha guidance
  - `validationPlan(...)` for fast and broader validation recommendations after a change
  - `impact(...)` for downstream blast radius, affected surfaces, and recommended checks
  - `afterEdit(...)` for immediate next reads, tests, docs, and risk follow-through after an edit
  - `commandMemory(...)` for recalled command evidence merged with current repo playbook guidance
- Treat custom `prism_query` snippets as the semantic escape hatch, not the default first hop.
- Keep `prism_query` read-only. Do not encode writes or side effects inside typed query views or custom query snippets.
