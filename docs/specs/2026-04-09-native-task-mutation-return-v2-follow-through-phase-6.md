# Native Task Mutation Return V2 Follow-Through Phase 6

Status: completed
Audience: coordination, query, MCP, and runtime maintainers
Scope: convert transaction-backed native task mutation helpers in `prism-query` to return canonical v2 task records instead of re-reading legacy `CoordinationTask` projections after commit

---

## 1. Summary

After the host-mutation cutover, the next remaining legacy mutation boundary is inside
`crates/prism-query/src/lib.rs`.

Several transaction-backed native task helpers still:

- execute a mutation transaction
- then re-read the task through `coordination_task(...)`
- and return a legacy `CoordinationTask`

That keeps old task projections alive on the active mutation path even though canonical v2 task
records already exist and are what MCP now expects at the product surface.

This slice removes those legacy re-reads for the transaction-backed helper family:

- `create_native_task(...)`
- `update_native_task(...)`
- `request_native_handoff(...)`
- `accept_native_handoff(...)`
- `resume_native_task(...)`
- `reclaim_native_task(...)`

## 2. Required changes

- add a local canonical task reload helper in `prism-query`
- make the transaction-backed native task helper family return `CoordinationTaskV2`
- update downstream call sites and tests to use canonical task fields
- keep runtime mutation helpers that still return direct live-runtime legacy task values out of
  scope for this slice

## 3. Non-goals

- do not change `heartbeat_native_task(...)` in this slice
- do not change `update_native_task_authoritative_only(...)` in this slice
- do not rewrite the live runtime mutation engine to emit canonical task records in this slice
- do not implement Postgres work in this slice

## 4. Exit criteria

- transaction-backed native task mutation helpers in `prism-query` no longer re-read legacy
  `CoordinationTask` values after commit
- downstream callers that use those helper returns now consume canonical v2 task records
- remaining legacy mutation-return paths are explicitly narrowed to live-runtime helpers that are
  deferred for the deeper runtime-state cutover
- targeted `prism-query`, `prism-core`, and `prism-mcp` validation passes succeed
