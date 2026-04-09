# Canonical Read Surface Follow-Through Phase 6

Status: completed
Audience: coordination, query, MCP, UI, and runtime-status maintainers
Scope: cut the active MCP/query read path over to canonical coordination snapshot v2 so plan lists,
ready-task reads, runtime overlays, and broker surfaces stop treating the legacy snapshot as the
default production read object

---

## 1. Summary

Phase 6 already removed the outward compatibility layer, but an important runtime/query residue
still remains:

- `prism-query` still exposes legacy ready-task and plan lookup helpers on the active path
- `prism-mcp` still threads `CoordinationSnapshot` through the read broker and runtime/UI plan
  surfaces
- plan lists, fleet overlays, and ready-task UI payloads still bounce through legacy snapshot
  helpers before converting back to v2 views

This slice finishes the next meaningful break:

- canonical ready-task views become the default query helper for active MCP reads
- plan-list and fleet/runtime overlay reads use `CoordinationSnapshotV2` directly
- the broker stops exposing the legacy snapshot as part of the current coordination surface

This is still not the full purge of legacy coordination internals. Persistence, spec
materialization, Git shared refs, and deeper mutation/runtime machinery may still need follow-on
work after this slice.

## 2. Required changes

### 2.1 Query helpers

- add canonical `ready_tasks_v2(...)` and `ready_tasks_for_executor_v2(...)` helpers in
  `prism-query`
- make plan discovery and plan activity derive from `CoordinationSnapshotV2` instead of pulling
  legacy plan/task records back through `CoordinationRuntimeState`

### 2.2 MCP read broker and plan surfaces

- remove the legacy `snapshot` field from `CurrentCoordinationSurface`
- stop exposing `current_coordination_snapshot()` from the broker/host when only canonical reads
  are needed
- make plan-resource list helpers read `CoordinationSnapshotV2`
- make runtime-status overlay views use canonical task records instead of converting legacy
  snapshots on the fly

### 2.3 UI and query-runtime ready-task reads

- switch overview, plan detail, and `readyTasks` query-runtime dispatch to canonical ready-task
  view helpers
- stop mapping legacy ready-task records back into `CoordinationTaskV2` inside MCP

## 3. Non-goals

- do not remove the remaining legacy coordination persistence or event-replay machinery in this
  slice
- do not rewrite spec materialization in `prism-spec` yet
- do not implement Postgres

## 4. Exit criteria

- production MCP plan list and fleet/runtime overlay reads consume `CoordinationSnapshotV2`
- production MCP ready-task reads consume canonical `CoordinationTaskV2` values directly
- `CurrentCoordinationSurface` no longer exposes the legacy snapshot
- `prism-query` plan discovery/activity reads no longer depend on legacy plan/task lookup helpers
- targeted `prism-query`, `prism-mcp`, and direct downstream `prism-core` tests pass
