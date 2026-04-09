# Event Execution Authority Storage Phase 15

Status: partially implemented
Audience: service, coordination, storage, authority-store, query, and future event-engine maintainers
Scope: add the first authoritative event-execution storage namespace beneath the settled authority and store seams so later event claiming, retries, and scheduling build on durable records rather than ad hoc service state

---

## 1. Summary

This spec is the next concrete Phase 15 target after the event-execution record model slice.

The goal is to make event-execution records durably storable and readable through the existing
storage and authority layers before PRISM starts implementing authoritative claiming, retry, or
recurring scheduling behavior.

This slice should:

- add one authoritative event-execution persistence seam in `prism-store`
- implement SQLite-backed storage for the event-execution record model
- wire the SQLite coordination authority backend to use that storage seam
- keep event-execution records in a dedicated namespace rather than the hot coordination summary
  state

This slice should not:

- implement event-engine claiming or scheduler loops
- define event-triggered coordination mutation semantics
- add UI, CLI, or MCP event-engine views yet
- leak SQLite-specific event tables upward into service or product code

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/event-engine.md](../contracts/event-engine.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/identity-model.md](../contracts/identity-model.md)

It follows:

- [2026-04-09-event-engine-foundation-phase-15.md](2026-04-09-event-engine-foundation-phase-15.md)
- [2026-04-09-event-execution-record-model-phase-15.md](2026-04-09-event-execution-record-model-phase-15.md)

## 4. Scope

This slice includes:

- one `prism-store` persistence surface for event-execution records
- SQLite schema and codecs for authoritative event-execution rows
- load and persist operations for event-execution records in the SQLite store
- SQLite authority-store wiring for event-execution record reads and writes
- targeted tests for record round-tripping and authority-backed storage behavior

This slice does not include:

- record claiming and conflict semantics
- query-engine exposure of event-execution records
- retention and compaction policy implementation
- Postgres or Git-backed event-execution storage

## 5. Design constraints

- Event-execution records are authoritative facts, but they must live in a dedicated event
  namespace instead of being embedded into the hot coordination summary snapshot.
- The first persistence seam should be stable enough that later SQLite, Postgres, and Git-backed
  implementations can conform to one record-storage shape.
- Product and service code must not reach around `prism-store` into SQLite tables directly.
- The SQLite authority backend may use the new store surface internally, but upper layers should
  still speak in backend-neutral authority/event record types.

## 6. Implementation slices

### Slice 1: Add the event-execution persistence seam in `prism-store`

- add backend-neutral store types and trait methods for event-execution records
- keep the seam narrow and record-focused
- avoid coupling it to scheduling or service-loop behavior

Exit criteria:

- `prism-store` exposes one authoritative event-execution record persistence surface

### Slice 2: Implement SQLite-backed event-execution storage

- add the SQLite schema for event-execution records
- implement insert or replace and read paths on `SqliteStore`
- add targeted store tests for round-tripping records and ordering behavior

Exit criteria:

- SQLite can durably persist and load authoritative event-execution records through the settled
  store seam

### Slice 3: Wire the SQLite authority backend to the new seam

- add authority-backend helpers that use the new store surface
- keep the event-execution namespace below the authority boundary
- avoid exposing raw SQLite operations upward

Exit criteria:

- the SQLite authority backend uses the shared event-execution store seam rather than ad hoc local
  storage code

## 7. Validation

Minimum validation for this slice:

- targeted `prism-store` tests for SQLite event-execution record round-tripping
- targeted `prism-core` tests for SQLite authority-backed event record persistence helpers
- direct downstream compile or test coverage where new store exports are used
- `git diff --check`

Important regression checks:

- event-execution records remain separate from coordination snapshot state
- the new store seam does not force service-specific logic into `prism-store`
- the SQLite authority backend uses the shared store seam instead of raw table access

## 8. Completion criteria

This spec is complete when:

- `prism-store` has a stable event-execution persistence seam
- SQLite storage for event-execution records exists behind that seam
- the SQLite authority backend uses that seam for authoritative event record storage
- later event-engine claiming and scheduling work can build on durable records instead of in-memory
  placeholders

## 9. Implementation checklist

- [x] Add the event-execution persistence seam in `prism-store`
- [x] Implement SQLite-backed event-execution storage
- [ ] Wire the SQLite authority backend to the new seam
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap and spec status as slices land
