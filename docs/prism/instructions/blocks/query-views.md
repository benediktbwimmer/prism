## Query Views

- Treat the typed query views as first-class workflow tools:
  - `repoPlaybook()` for repo workflow, build, test, lint, format, and gotcha guidance
  - `validationPlan(...)` for fast and broader validation recommendations after a change
  - `impact(...)` for downstream blast radius, affected surfaces, and recommended checks
  - `afterEdit(...)` for immediate next reads, tests, docs, and risk follow-through after an edit
  - `commandMemory(...)` for recalled command evidence merged with current repo playbook guidance
- Treat custom `prism_query` snippets as the semantic escape hatch, not the default first hop.
- Keep `prism_query` read-only. Do not encode writes or side effects inside typed query views or custom query snippets.
- When a query needs another live runtime, use `prism.from("runtime-id")` inside `prism_query` instead of inventing a separate peer-read tool flow.
- Treat `prism.from("runtime-id")` results as peer-enriched context, not shared-authoritative repo truth.
