# Spec Sync Actions Phase 13

Status: in progress
Audience: spec-engine, coordination, query, MCP, CLI, and agent-execution maintainers
Scope: implement explicit spec-to-coordination sync actions that let agents and users create or extend authoritative coordination state from native specs while preserving stable sync provenance

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 13:

- add explicit sync actions from spec intent into authoritative coordination
- keep sync agentic and visible, not a deterministic markdown compiler
- preserve spec revision and checklist coverage provenance on created or updated coordination
  objects

This phase builds on:

- typed authoritative spec links
- local sync provenance
- real `SpecCoverageView`

This phase does not make local specs authoritative coordination truth.

## 2. Status

Current state:

- [x] Phase 12 is complete
- [x] specs can now expose linked coordination provenance and coverage posture
- [ ] there is still no first-class action for creating coordination from a spec
- [ ] coordination objects linked to specs are still authored only through lower-level mutation
  inputs
- [x] a canonical sync brief now exists for agent-facing sync decisions

Current slice notes:

- sync actions must remain explicit and reviewable
- PRISM should structure the input and persist the provenance, but the graph synthesis remains an
  agent or human decision
- the first cut should prefer small, composable actions over one giant “compile spec” mutation
- Slice 1 is complete on this branch: `SpecQueryEngine` now exposes `sync_brief(spec_id)` with
  spec metadata, required checklist items, coverage posture, and linked coordination refs

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 13: implement explicit spec-to-coordination sync actions

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)

This spec builds on:

- [2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md](2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md)
- [2026-04-09-spec-coverage-phase-12.md](2026-04-09-spec-coverage-phase-12.md)

## 5. Scope

This phase includes:

- a deterministic sync-brief query for one spec
- explicit mutation/action surfaces for creating or extending coordination from a spec
- sync provenance capture for the created or updated coordination objects
- stable linkage to exact checklist items and optional sections where applicable

This phase does not include:

- automatic or background sync
- silent plan/task regeneration after every markdown edit
- making specs authoritative blockers for coordination readiness

## 6. Non-goals

This phase should not:

- compile a flat markdown checklist into a full coordination DAG without an acting agent or user
- infer task hierarchy solely from heading depth
- mutate markdown checkboxes automatically on task completion
- bypass the canonical coordination transaction protocol

## 7. Design

### 7.1 Explicit-sync rule

All spec-to-coordination sync must happen through explicit actions.

Initial target families:

- `spec_sync_brief(spec_id)`
- `create_plan_from_spec(...)`
- `create_tasks_from_spec_items(...)`
- `extend_plan_from_spec(...)`

The exact action names may vary, but the semantics must remain explicit.

### 7.2 Agentic-synthesis rule

PRISM may parse, validate, and summarize spec structure.
It must not pretend that plan topology, review-gate placement, dependency layering, or artifact
requirements can always be derived mechanically from markdown alone.

The caller remains responsible for choosing the coordination graph shape.

### 7.3 Provenance rule

Every sync action must record:

- `spec_id`
- `source_path`
- `source_revision`
- exact covered checklist-item ids where applicable
- the created or updated authoritative coordination refs

### 7.4 Coverage-preservation rule

Sync actions must produce coordination links that the existing coverage and sync-provenance layers
can understand without extra side channels.

## 8. Implementation slices

### Slice 1: Add sync-brief query surface

- provide a deterministic spec sync brief that exposes:
  - spec metadata
  - required checklist items
  - current coverage posture
  - current linked plans/tasks

Exit criteria:

- an agent can request one bounded sync-oriented view instead of manually stitching together
  several spec queries

Status:

- [x] complete

### Slice 2: Add explicit plan/task sync actions

- add initial explicit mutation/action surfaces for creating linked plans/tasks from spec intent
- keep the actions narrow and provenance-rich

Exit criteria:

- callers can create authoritative coordination with spec linkage without manually hand-building
  every link field

### Slice 3: Validate sync-action behavior end to end

- verify the actions preserve typed spec links
- verify the resulting coverage and sync provenance update correctly after refresh

Exit criteria:

- sync actions participate cleanly in the existing native spec loop

## 9. Validation

Minimum validation for this phase:

- targeted `prism-spec` tests for sync-brief query behavior
- targeted coordination/query tests for created link fields and provenance
- targeted downstream `prism-core` tests if shared transaction surfaces change
- `git diff --check`

Important regression checks:

- sync actions never bypass the canonical transaction protocol
- resulting plans/tasks retain exact checklist coverage links
- repeated sync does not silently duplicate links without caller intent
- coverage and sync-provenance refresh remain deterministic after sync

## 10. Completion criteria

This phase is complete only when:

- one bounded sync-brief query exists
- explicit spec-to-coordination sync actions exist
- resulting coordination objects carry the right typed links and provenance
- later CLI/MCP/UI exposure can build on those actions without redesign

## 11. Implementation checklist

- [x] Add a deterministic sync-brief query surface
- [ ] Add explicit plan/task sync actions
- [ ] Preserve exact checklist-item and revision provenance
- [ ] Validate end-to-end coverage/provenance refresh after sync
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
