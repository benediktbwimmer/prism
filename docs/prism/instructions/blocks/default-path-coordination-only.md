## Default Path

- Prefer the reduced coordination-first path in this mode:
  - orient with `prism://session`, `prism://capabilities`, and `prism://vocab`
  - in truncation-prone harnesses, use `prism://capabilities/{section}`, `prism://shape/...`, `prism://example/...`, and `prism://recipe/...` to confirm the reduced surface before reaching for larger payloads
  - use `prism_task_brief` for compact task state, blockers, and recent outcome context
  - use read-only `prism_code` for supported coordination reads such as plans, readiness, blockers, claims, artifacts, and runtime diagnostics that remain enabled in this mode
  - use `prism_code` for declared work, coordination updates, claims, artifacts, and other durable state changes that the reduced runtime allows
- Prefer reduced coordination reads over hidden assumptions about plan state, ownership, or runtime readiness.
- If the task requires repo exploration, symbol lookup, concept enrichment, or bounded edit targeting, switch to a full-runtime PRISM session instead of guessing from `coordination_only` mode.
