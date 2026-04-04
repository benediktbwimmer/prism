## Default Path

- Prefer the staged PRISM-first path for normal agent work:
  - orient with `prism://session`, `prism://capabilities`, and `prism://vocab`
  - use `prism_locate`, `prism_gather`, `prism_open`, `prism_workset`, and `prism_expand` for bounded context and edit targeting
  - use `prism_concept` when the unit of thought is a broad repo-native subsystem or multi-artifact cluster
  - use a `memory` lens on concept reads before substantial work in an unfamiliar subsystem
  - use typed query views when the task is semantic guidance rather than raw code lookup
  - use ad hoc read-only `prism_query` snippets only when the compact surface and typed views cannot express the needed read
- Use `prism_gather` for bounded exact-text slices when you know the text to inspect and a symbol handle is not the right first hop.
- Use `prism_concept` when the task is framed as a broad repo-native term such as `validation`, `runtime`, `session`, `memory`, `status`, `compact tools`, or `task continuity`.
- Prefer concept retrieval before symbol or text search when the likely unit is a multi-artifact repo concept rather than one file, symbol, or exact string match.
