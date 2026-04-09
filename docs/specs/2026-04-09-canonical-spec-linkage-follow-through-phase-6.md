# Canonical Spec Linkage Follow-Through Phase 6

Status: completed
Audience: coordination, query, spec, CLI, and MCP maintainers
Scope: make spec linkage first-class on the canonical coordination model and remove the remaining
live spec-materialization dependency on the legacy snapshot

---

## 1. Summary

The spec materialization path is still using `CoordinationSnapshot` for one reason:

- canonical plan/task records do not yet carry `spec_refs`

That leaves a real migration-era hole in the current v2 model. Spec links are part of the live
coordination graph, but today they only survive on the legacy plan/task structs.

This slice closes that hole by:

- adding spec linkage to canonical plan and task records
- projecting legacy spec linkage into `CoordinationSnapshotV2`
- switching spec materialization and workspace-backed spec query entrypoints to
  `CoordinationSnapshotV2`

## 2. Required changes

- add `spec_refs` to `CanonicalPlanRecord`
- add `spec_refs` to `CanonicalTaskRecord`
- populate those fields in `CoordinationSnapshot::to_canonical_snapshot_v2()`
- make `SpecMaterializedReplaceRequest` carry `Option<CoordinationSnapshotV2>`
- make `refresh_spec_materialization(...)` and `WorkspaceSpecSurface` take canonical snapshots
- make MCP and CLI spec query entrypoints pass `coordination_snapshot_v2()`
- update spec-materialization coverage and sync-provenance derivation to read canonical records

## 3. Non-goals

- do not expose new spec-ref fields on JS/MCP task/plan response views in this slice
- do not remove the deeper legacy runtime mutation engine in this slice
- do not implement Postgres

## 4. Exit criteria

- production spec materialization no longer accepts `CoordinationSnapshot`
- workspace-backed spec queries use `CoordinationSnapshotV2`
- spec coverage and sync-provenance derivation read canonical plan/task records
- canonical plan/task records preserve plan/task spec linkage across legacy projection
- targeted tests for `prism-coordination`, `prism-spec`, `prism-cli`, and `prism-mcp` pass

## 5. Outcome

- canonical plan and task records now carry spec linkage directly
- legacy-to-canonical projection preserves plan/task `spec_refs`
- spec materialization request types and workspace-backed spec query entrypoints now consume
  `CoordinationSnapshotV2`
- CLI and MCP spec reads now pass canonical coordination snapshots instead of the legacy snapshot
