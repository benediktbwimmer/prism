# Coordination V2 Breaking Compatibility Removal Phase 6

Status: in progress
Audience: coordination, query, MCP, CLI, JS surface, and authority-backend maintainers
Scope: remove the remaining migration-era compatibility code so SQLite-first coordination and v2 plan/task/artifact/review surfaces are the only supported product model

---

## 1. Summary

The authority and coordination cleanup has reached the point where compatibility code is now more
harmful than useful.

This repo does not need to preserve:

- legacy shared-ref response fields
- deprecated shared-ref helper aliases
- old plan/task compatibility view types
- MCP/query surfaces that expose both old and v2 coordination payloads side by side

The target outcome of this phase is a real breaking cut:

- `coordinationAuthority` replaces the runtime-status shared-ref field
- deprecated shared-ref diagnostics aliases are deleted
- live plan and task query surfaces return only v2 payloads
- inbox/task-context/UI coordination surfaces stop carrying old plan/task projections
- compatibility mapping helpers that translate v2 states back into legacy plan/task payloads are removed

This phase starts with the breaking product-surface cut, but it does not stop there. The same
phase also owns the follow-through to remove legacy coordination-model dependencies that remain on
the active runtime path under `CoordinationSnapshot` after the surface break lands.

The first internal follow-through slice under this phase is:

- [2026-04-09-coordination-query-reader-v2-follow-through-phase-6.md](./2026-04-09-coordination-query-reader-v2-follow-through-phase-6.md)
- [2026-04-09-canonical-task-handoff-follow-through-phase-6.md](./2026-04-09-canonical-task-handoff-follow-through-phase-6.md)
- [2026-04-09-canonical-read-surface-follow-through-phase-6.md](./2026-04-09-canonical-read-surface-follow-through-phase-6.md)
- [2026-04-09-canonical-read-model-follow-through-phase-6.md](./2026-04-09-canonical-read-model-follow-through-phase-6.md)
- [2026-04-09-canonical-spec-linkage-follow-through-phase-6.md](./2026-04-09-canonical-spec-linkage-follow-through-phase-6.md)
- [2026-04-09-canonical-runtime-publish-state-follow-through-phase-6.md](./2026-04-09-canonical-runtime-publish-state-follow-through-phase-6.md)
- [2026-04-09-canonical-materialization-envelope-follow-through-phase-6.md](./2026-04-09-canonical-materialization-envelope-follow-through-phase-6.md)
- [2026-04-09-canonical-assisted-overlay-follow-through-phase-6.md](./2026-04-09-canonical-assisted-overlay-follow-through-phase-6.md)

## 2. Required changes

### 2.1 Authority edge

- delete deprecated `shared_coordination_ref_diagnostics(...)` aliases
- delete deprecated `shared_coordination_ref_diagnostics_with_provider(...)` aliases
- rename runtime status payloads from `sharedCoordinationRef` to `coordinationAuthority`
- rename the corresponding JS/MCP view types accordingly
- update CLI and MCP status/reporting code to use the new field only

### 2.2 Coordination surface

- delete `PlanView`
- delete `CoordinationTaskView`
- remove dual `plan`/`planV2` and `task`/`taskV2` surface duplication
- make `prism.plan(...)` return `CoordinationPlanV2View`
- make `prism.task(...)` return `CoordinationTaskV2View`
- make `prism.readyTasks(...)` return `CoordinationTaskV2View[]`
- remove old task/plan compatibility mapping helpers in `crates/prism-mcp/src/views.rs`

### 2.3 Aggregate UI/query payloads

- remove `plan_v2` and `task_v2` duplication from aggregate payloads
- make inbox/task-context/plan-resource surfaces carry only v2 plan/task payloads
- update MCP UI models and query runtime dispatch accordingly

### 2.4 Legacy coordination-model purge

- stop treating `Plan`, `CoordinationTask`, `Artifact`, `ArtifactReview`, and `CoordinationSnapshot`
  as the primary live coordination model in `prism-coordination`
- move runtime/query/authority code toward `CoordinationSnapshotV2` as the authoritative
  coordination state shape
- make spec linkage first-class on canonical plan/task records instead of keeping it only on the
  legacy plan/task projection
- cut reader-side query and MCP task/plan metadata consumers over to canonical v2 task and plan
  helpers before touching deeper mutation or watch paths
- move pending-handoff semantics into canonical task records so assisted-lease and task-brief
  readers do not need legacy task projections for that state
- rebuild read-model derivation and materialization from `CoordinationSnapshotV2` so broker and
  overview surfaces stop carrying legacy plan/task payloads only to maintain summary queues
- make the published runtime path keep canonical coordination state first-class instead of
  re-deriving it from the legacy continuity snapshot on every read
- make materialization and checkpoint envelopes require canonical coordination state instead of
  treating it as an optional fallback derived from the legacy continuity snapshot
- make local assisted overlay republishes preserve the live canonical coordination snapshot instead
  of re-deriving it from the legacy continuity snapshot
- delete legacy-only translation helpers once no active runtime path depends on them
- keep Git-shared-ref-specific compatibility only where it is part of the explicit Git backend,
  not in backend-neutral coordination code

## 3. Non-goals

- do not implement Postgres in this phase
- do not preserve source compatibility for older MCP/UI payload consumers
- do not keep deprecated aliases for convenience

## 4. Exit criteria

- no production code references `shared_coordination_ref_diagnostics(...)`
- no production response schema uses `shared_coordination_ref`
- no production response schema exposes `PlanView` or `CoordinationTaskView`
- no live plan/task response path in MCP/query code uses compatibility status mappers
- reader-side query and MCP task/plan metadata consumers use canonical v2 task and plan lookups
  where legacy-only fields are not required
- the roadmap clearly tracks any remaining `CoordinationSnapshot`-backed active-path dependencies
  still to be removed inside this phase
- targeted tests for `prism-core`, `prism-js`, and `prism-mcp` pass
