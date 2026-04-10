## Concept Packs

- Treat concept packs as a reusable repo vocabulary layer, not as a taxonomy exercise.
- Use semantic pathfinding to move from high-level abstractions down to concrete code handles instead of repeatedly searching or guessing file locations.
- Use `verbosity` intentionally:
  - `summary` for discovery
  - `standard` for balanced architectural context
  - `full` only when deep reasoning or historical detail is required
- When entering an unfamiliar subsystem, decode the governing concept with `lens: "memory"` before writing code.
- Prefer carrying forward an existing concept handle when it matches the task instead of rediscovering the same cluster.
- Request binding detail with `includeBindingMetadata` when you need to inspect lineage-backed member bindings, drift, or rebinding behavior.
- Promote a concept candidate when real task work has resolved a broad or fuzzy term into a stable multi-artifact cluster that future agents would likely reuse.
- Prefer concepts that match how future agents will naturally think and speak about the repo, such as `validation pipeline`, `runtime surface`, `session lifecycle`, `memory system`, `compact tools`, or `task continuity`.
- Use the same `local -> session -> repo` promotion ladder for concepts.
- Use `prism_code` and the concept SDK methods when the concept lifecycle matters, such as promote, update, or retire operations.
- Do not let concepts silently rot. If a concept no longer matches the live subsystem, update or retire it.
