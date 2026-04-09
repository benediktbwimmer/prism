# Event Execution Record Model Phase 15

Status: completed
Audience: service, coordination, storage, query, and future event-engine maintainers
Scope: define the first concrete event-execution domain model in code so later scheduling, hook evaluation, and authoritative event storage can build on stable types instead of ad hoc metadata

---

## 1. Summary

This spec is the next concrete Phase 15 target after the event-engine foundation slice.

The goal is to introduce the native event-execution record model in the domain crates before PRISM
starts implementing recurring execution or hook-trigger loops.

This slice should:

- add stable event-execution identity and lifecycle enums
- add concrete execution-record types with ownership, timing, and trigger metadata
- place those types in the correct crates so later authority-store and mutation work can build on
  them cleanly

This slice should not:

- implement authoritative storage or mutation of event records yet
- add recurring-plan scheduling loops
- add hook SDK execution
- invent service-local event semantics that bypass the coordination and authority seams

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts

This spec depends on:

- [../contracts/event-engine.md](../contracts/event-engine.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/identity-model.md](../contracts/identity-model.md)

It follows:

- [2026-04-09-event-engine-foundation-phase-15.md](2026-04-09-event-engine-foundation-phase-15.md)

## 4. Scope

This slice includes:

- `prism-ir` identity and lifecycle primitives for event execution
- `prism-coordination` record and trigger types for event execution
- targeted tests that lock the lifecycle vocabulary and basic serde/domain shape

This slice does not include:

- authority-store persistence
- service mutation wiring
- query-engine exposure
- UI, CLI, or MCP event-engine views

## 5. Design constraints

- Event execution records are authoritative facts eventually, but this slice only settles the
  model, not the persistence backend.
- The lifecycle vocabulary must match the settled contract:
  - `claimed`
  - `running`
  - `succeeded`
  - `failed`
  - `expired`
  - `abandoned`
- Event identity must be transition-oriented rather than predicate-only.
- The model must be reusable by both DB-backed and Git-backed authority implementations later.

## 6. Implementation slices

### Slice 1: Add event-execution identity and lifecycle primitives

- add native event-execution ids and lifecycle enums in `prism-ir`
- add any minimal trigger vocabulary needed to make execution records meaningful

Exit criteria:

- downstream crates can name and serialize event-execution identities and lifecycle states without
  inventing local strings

### Slice 2: Add execution-record domain types

- add concrete record types in `prism-coordination`
- include trigger, owner, timing, and status fields
- keep the model backend-neutral and storage-neutral

Exit criteria:

- later authority-store and mutation work can consume one settled record model

## 7. Validation

Minimum validation for this slice:

- targeted `prism-ir` tests for lifecycle and serde shape
- targeted `prism-coordination` tests for record model shape
- direct downstream compile/test coverage where the new exported types are re-exported
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- event-execution identity and lifecycle primitives exist in `prism-ir`
- execution-record types exist in `prism-coordination`
- the record model is stable enough that later storage and mutation slices can reference it

## 9. Implementation checklist

- [x] Add event-execution identity and lifecycle primitives
- [x] Add execution-record domain types
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
