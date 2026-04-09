# Canonical Runtime Publish State Follow-Through Phase 6

Status: completed
Audience: coordination, query, core runtime, and service-sync maintainers
Scope: make the published runtime path treat `CoordinationSnapshotV2` as first-class state instead
of repeatedly re-deriving it from the legacy continuity snapshot

---

## 1. Summary

The broker and persisted read-model paths now consume canonical coordination state directly, but
the published runtime path is still lagging behind.

Today:

- `WorkspaceRuntimeState` stores a legacy `CoordinationSnapshot` as its coordination payload
- `Prism` materialized runtime stores the same legacy snapshot and derives
  `coordination_snapshot_v2()` on demand
- service-backed coordination sync already has canonical state available, but it republishes the
  runtime through the legacy snapshot first

That leaves an unnecessary active-path dependency on the old snapshot model in the runtime publish
loop even when the canonical state is already known.

This slice moves the published runtime path toward the steady-state model by:

- storing canonical coordination state alongside the continuity snapshot in runtime state
- storing canonical coordination state alongside the continuity runtime in `Prism`
- publishing canonical state through workspace runtime generation and service-backed sync entrypoints

The legacy continuity snapshot still remains in this slice because the mutation engine has not been
fully cut over yet, but it becomes explicitly secondary to the canonical runtime view.

## 2. Required changes

- add canonical coordination state storage to `MaterializedCoordinationRuntime`
- make `Prism::coordination_snapshot_v2()` return the stored canonical state
- add runtime constructors and replacement paths that accept both the continuity snapshot and the
  canonical snapshot
- add canonical coordination state storage to `WorkspaceRuntimeState`
- make service-backed coordination refresh and runtime republish paths pass the canonical snapshot
  they already have instead of forcing recomputation from the legacy snapshot

## 3. Non-goals

- do not rewrite `CoordinationRuntimeState` to operate natively on canonical records in this slice
- do not remove persisted legacy snapshot payloads from checkpoint or authority storage yet
- do not implement Postgres

## 4. Exit criteria

- published `Prism` instances keep a first-class canonical coordination snapshot
- service-backed coordination sync publishes runtime state with canonical coordination data
- runtime `coordination_snapshot_v2()` no longer re-derives v2 from the legacy snapshot on every
  read
- the remaining legacy continuity snapshot usage is narrowed to the mutation engine and explicitly
  tracked as follow-up work

## 5. Outcome

- `MaterializedCoordinationRuntime` now stores `CoordinationSnapshotV2` directly beside the
  continuity runtime
- `WorkspaceRuntimeState` now carries canonical coordination state directly beside the continuity
  snapshot
- runtime publish and republish paths now pass through the canonical snapshot they already have
  instead of forcing re-derivation
- the shared coordination-transaction path now refreshes the cached canonical snapshot before
  releasing the runtime lock, so immediate post-mutation v2 reads stay synchronized
