# Canonical Materialization Envelope Follow-Through Phase 6

Status: completed
Audience: coordination, core runtime, persistence, and authority-backend maintainers
Scope: remove the remaining optional-canonical posture from coordination materialization and
checkpoint envelopes now that runtime publication already treats `CoordinationSnapshotV2` as
first-class state

---

## 1. Summary

The runtime publish path now carries canonical coordination state directly, but adjacent
materialization code still behaves as if canonical state might be absent.

Today:

- `CoordinationMaterialization` still stores `canonical_snapshot_v2` as `Option<_>`
- checkpoint persistence still falls back to deriving canonical state from the legacy snapshot
- that optionality leaks an outdated migration posture into the active persistence path even though
  current runtime and authority-backed producers already have canonical state available

This slice removes that false optionality so the materialization and startup-checkpoint path aligns
with the new runtime-state contract.

## 2. Required changes

- make `CoordinationMaterialization.canonical_snapshot_v2` required
- remove fallback v2 derivation in checkpoint materialization persistence
- update all production materialization producers to pass canonical state explicitly
- keep legacy `CoordinationSnapshot` only where the persistence format or mutation engine still
  needs it, not as a stand-in for missing canonical state

## 3. Non-goals

- do not rewrite stored compaction payloads away from the legacy continuity snapshot in this slice
- do not remove legacy `CoordinationSnapshot` from authority or startup-checkpoint persisted
  formats yet
- do not implement Postgres

## 4. Exit criteria

- production `CoordinationMaterialization` values always carry canonical coordination state
- checkpoint persistence no longer re-derives canonical state from the legacy snapshot
- all production coordination materialization producers compile against the required canonical
  envelope
- targeted tests for `prism-core` pass

## 5. Outcome

- `CoordinationMaterialization` now requires `canonical_snapshot_v2`
- checkpoint materialization persistence no longer falls back to deriving canonical state from the
  legacy continuity snapshot
- production SQLite authority and authority-sync materialization producers now pass canonical
  coordination state explicitly as part of the envelope
