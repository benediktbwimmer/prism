# Canonical Startup Checkpoint Follow-Through Phase 6

Status: completed
Audience: core runtime, store, authority-backend, and startup maintainers
Scope: remove the remaining optional canonical checkpoint posture so startup checkpoints always
persist and load canonical coordination state directly

---

## 1. Summary

The runtime and materialization paths now treat canonical coordination as first-class state, but
the persisted startup-checkpoint contract still does not.

Today:

- `CoordinationStartupCheckpoint.canonical_snapshot_v2` is still optional
- startup and authority loaders still contain fallback logic for “checkpoint exists but v2
  snapshot is missing”
- that is explicit compatibility handling for the continuity-only coordination model

This slice removes that compatibility layer by making canonical coordination required in startup
checkpoints and deleting the fallback branches that re-derive canonical state from the legacy
snapshot.

## 2. Required changes

- make `CoordinationStartupCheckpoint.canonical_snapshot_v2` required
- make startup-checkpoint decode fail closed when canonical state is missing
- remove startup and authority-loader fallback derivation from legacy snapshots
- update checkpoint metadata and tests to reflect canonical state as a required persisted field

## 3. Non-goals

- do not remove the legacy continuity snapshot from the checkpoint payload in this slice
- do not rewrite the mutation engine away from `CoordinationSnapshot`
- do not implement Postgres

## 4. Exit criteria

- persisted startup checkpoints always contain canonical coordination state
- startup and authority reads no longer contain fallback derivation for missing canonical state
- checkpoint metadata surfaces report canonical-state presence unconditionally for persisted
  checkpoints
- targeted tests for `prism-store` and `prism-core` pass

## 5. Outcome

- `CoordinationStartupCheckpoint.canonical_snapshot_v2` is now required in the persisted contract
- startup-checkpoint decode now fails closed when canonical state is missing instead of silently
  accepting a legacy continuity-only checkpoint
- startup and SQLite authority loaders no longer branch on missing canonical checkpoint state
- the dead `WorkspaceRuntimeState::replace_coordination_runtime(...)` helper that only re-derived
  canonical state from the legacy snapshot has been removed
