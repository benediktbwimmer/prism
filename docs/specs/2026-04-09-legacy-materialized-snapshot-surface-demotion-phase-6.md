# Legacy Materialized Snapshot Surface Demotion Phase 6

Status: completed
Audience: core, coordination materialization, authority-store, and runtime reload maintainers
Scope: make the materialized-store continuity snapshot paths explicitly legacy so canonical v2
remains the only normal materialized coordination surface

---

## 1. Summary

Phase 6 already demoted the public legacy session and `Prism` snapshot accessors.

The next matching cleanup is the materialized-store seam. It still exposed a normal-sounding
`read_snapshot()` API and request/type fields named simply `snapshot`, even when those values were
specifically the continuity projection rather than the canonical v2 coordination model.

This slice renames those materialized continuity surfaces to `legacy_*` names and updates the
active persistence/session/authority callsites accordingly.

## 2. Required changes

- rename `CoordinationMaterializedStore::read_snapshot(...)` to
  `CoordinationMaterializedStore::read_legacy_snapshot(...)`
- rename `CoordinationMaterializedState.snapshot` to `legacy_snapshot`
- rename `CoordinationStartupCheckpointWriteRequest.snapshot` to `legacy_snapshot`
- rename `CoordinationCompactionWriteRequest.snapshot` to `legacy_snapshot`
- update session, persistence, checkpoint materialization, and SQLite authority follow-through
  callsites to use the explicit legacy names
- align the materialized-store contract wording so unqualified materialized coordination reads are
  canonical/v2-oriented and continuity access is explicitly marked legacy

## 3. Non-goals

- do not remove the continuity snapshot payload from materialized persistence yet
- do not redesign startup checkpoint storage
- do not remove the canonical v2 snapshot materialization path

## 4. Exit criteria

- no production code calls `CoordinationMaterializedStore::read_snapshot(...)`
- no public materialized-store write request uses an unqualified `snapshot` field for continuity
  payloads
- targeted `prism-core` tests and downstream compile checks pass
