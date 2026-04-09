# Canonical Runtime Replacement Follow-Through Phase 6

Status: completed
Audience: coordination, query, runtime-state, MCP, and watch-loop maintainers
Scope: remove legacy snapshot-only runtime replacement helpers from active paths so runtime mutation and reload code must carry canonical coordination state explicitly when replacing live coordination runtime state

---

## 1. Summary

Phase 6 already made canonical coordination state first-class in the read model, publish path,
materialization envelope, assisted overlay, and startup checkpoint path.

One runtime-shaped compatibility seam still remained:

- live `Prism` replacement helpers still accepted only `CoordinationSnapshot`
- `MaterializedCoordinationRuntime` still exposed snapshot-only replace and persist helpers
- several runtime and test callers still relied on those helpers and silently re-derived v2 state

That is the wrong direction for the settled model.

The active runtime path may still carry the continuity snapshot internally where event replay and
legacy runtime mutation machinery still need it, but replacement of live runtime state should no
longer pretend canonical coordination state is optional.

This slice removes that compatibility seam.

## 2. Required changes

- delete `Prism::replace_coordination_snapshot(...)`
- delete `Prism::replace_coordination_runtime_with_snapshot_v2(...)`
- make `Prism::replace_coordination_runtime(...)` require both:
  - `CoordinationSnapshot`
  - `CoordinationSnapshotV2`
  - runtime descriptors
- delete snapshot-only replace and persist helpers on `MaterializedCoordinationRuntime`
- keep runtime rollback explicit by rebuilding canonical v2 state from the final rollback snapshot
- update active runtime callers and affected tests to pass canonical state explicitly
- keep `WorkspaceRuntimeState::replace_coordination_runtime(...)` aligned with the same contract

## 3. Non-goals

- do not remove the continuity snapshot from `CoordinationRuntimeState` in this slice
- do not rewrite constructor-heavy test setup that still seeds continuity snapshots inline
- do not change persisted schema or authority formats in this slice

## 4. Outcome

Completed.

The active runtime replacement seam now requires canonical state explicitly:

- `Prism::replace_coordination_runtime(...)` is the only live replacement helper left
- `MaterializedCoordinationRuntime` no longer exposes snapshot-only replace or persist helpers
- transaction rollback now restores the runtime through an explicit rollback snapshot plus explicit
  canonical projection rebuild
- watch-loop overlay publication, authority-sync runtime replacement, recurring-plan event-engine
  tests, and runtime-only reload tests now pass canonical state explicitly when replacing the live
  coordination runtime

The continuity snapshot still exists internally where the mutation engine and replay machinery
still depend on it, but the replacement seam no longer treats canonical coordination state as a
derived afterthought.
