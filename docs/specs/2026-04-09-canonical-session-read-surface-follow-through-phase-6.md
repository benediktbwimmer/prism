# Canonical Session Read Surface Follow-Through Phase 6

Status: completed
Audience: coordination, session, runtime, and Git-backend maintainers
Scope: remove the public legacy session read surface for `CoordinationSnapshot`, keep the remaining continuity-snapshot read path internal to the explicit Git backend, and move the affected assertions to canonical v2 state

---

## 1. Summary

After the runtime replacement cleanup, the remaining public legacy read surface was concentrated in
`WorkspaceSession`:

- `load_coordination_snapshot()`
- `read_coordination_snapshot_with_consistency(...)`

Production consumers were already effectively using canonical v2 state, but the continuity
snapshot remained publicly available as a compatibility read surface.

That is no longer the direction for Phase 6.

This slice removes the public session compatibility surface while still allowing the explicit Git
backend to perform its internal continuity-snapshot checks during live-sync tests.

## 2. Required changes

- delete the public `WorkspaceSession::load_coordination_snapshot()`
- demote strong/eventual continuity-snapshot reads to an internal helper
- rename that internal helper so it is clearly legacy/backend-specific
- update the explicit Git backend code to use the renamed internal helper
- update remaining tests to assert canonical v2 session state instead of the removed public legacy
  session API

## 3. Non-goals

- do not remove the continuity snapshot from the explicit Git backend in this slice
- do not remove `Prism::coordination_snapshot()` yet
- do not rewrite persistence internals that still need continuity snapshots

## 4. Outcome

Completed.

The session-side legacy compatibility read surface is gone:

- `WorkspaceSession::load_coordination_snapshot()` has been deleted
- `read_coordination_snapshot_with_consistency(...)` is now the internal
  `read_legacy_coordination_snapshot_with_consistency(...)`
- the explicit Git shared-ref code now uses that internal helper directly
- core tests that previously asserted through the removed public continuity-snapshot API now load
  canonical v2 state instead
