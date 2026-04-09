# Canonical Task Lease And Live Runtime Mutation Follow-Through Phase 6

Status: completed
Audience: coordination, query, MCP, and runtime maintainers
Scope: move the remaining stale-holder and git-execution admissibility helpers onto canonical task records and make the last live-runtime native task helpers return canonical v2 task views

---

## 1. Summary

After the host-mutation reader cutover and the transaction-backed native helper cutover, the main
active-path legacy residue is now concentrated in two places:

- `crates/prism-mcp/src/host_mutations.rs` still uses legacy `CoordinationTask` lease-holder
  helpers for stale-same-holder auto-resume and git-execution admissibility
- `crates/prism-query/src/lib.rs` still returns legacy `CoordinationTask` values from the
  live-runtime helpers `update_native_task_authoritative_only(...)` and
  `heartbeat_native_task(...)`

That leaves the active mutation path relying on the old task projection even though canonical task
records already carry the same lease, holder, and git-execution state.

This slice removes that residue by making canonical task records the only task model used on the
active host mutation path.

## 2. Required changes

- promote canonical lease-holder helpers into `prism-coordination`
- make stale-same-holder auto-resume use canonical task views
- make git-execution admissibility use canonical task views and canonical plan lookup
- make `update_native_task_authoritative_only(...)` return `CoordinationTaskV2`
- make `heartbeat_native_task(...)` return `CoordinationTaskV2`
- update downstream callers and tests to use canonical task fields only

## 3. Non-goals

- do not remove all `CoordinationSnapshot` internals in this slice
- do not implement Postgres work in this slice
- do not rewrite the entire live runtime mutation engine around `CoordinationSnapshotV2` in this
  slice

## 4. Exit criteria

- `host_mutations.rs` no longer defines its own legacy task lease-holder helpers
- stale-holder auto-resume and git-execution admissibility both run on canonical task records
- the last live-runtime native task helper returns use `CoordinationTaskV2`
- downstream MCP/core/query tests no longer depend on legacy task return shapes from those helpers
- targeted `prism-coordination`, `prism-query`, `prism-core`, and `prism-mcp` validation passes
  succeed

## 5. Outcome

This slice is complete.

Completed follow-through:

- canonical lease-holder helpers now live in `prism-coordination` and are re-exported for
  backend-neutral consumers
- stale-same-holder auto-resume and git-execution admissibility in `host_mutations.rs` now operate
  on canonical task records rather than legacy `CoordinationTask`
- `update_native_task_authoritative_only(...)` and `heartbeat_native_task(...)` now return
  `CoordinationTaskV2`
- downstream MCP, query, and core tests now use canonical task fields for these live-runtime
  helper returns

Residual work after this slice:

- the remaining Phase 6 work is now below this mutation boundary: deeper runtime and
  `CoordinationSnapshot` internals still need follow-through so the legacy task model stops shaping
  the coordination engine itself
