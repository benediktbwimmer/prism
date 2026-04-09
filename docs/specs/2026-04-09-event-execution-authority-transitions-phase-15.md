# Event Execution Authority Transitions Phase 15

Status: partially implemented
Audience: service, coordination, storage, authority-store, MCP, and future event-engine maintainers
Scope: add explicit authoritative lifecycle transitions for event-execution records so the event engine can claim, start, finish, expire, and abandon work through named semantics instead of generic record replacement

---

## 1. Summary

This spec is the next concrete Phase 15 target after authoritative event-execution storage.

The goal is to move from “durable event records exist” to “event ownership transitions are explicit
and authoritative.”

This slice should:

- define one explicit authority-facing transition vocabulary for event-execution records
- make optimistic ownership transitions backend-neutral at the authority seam
- give `WorkspaceEventEngine` one narrow authority-backed owner path for event-record reads and
  writes
- preserve the rule that event-triggered coordination mutations still route through the settled
  coordination mutation protocol

This slice should not:

- implement recurring trigger scanning loops
- implement hook SDK execution
- add browser or CLI event-engine management UI
- reintroduce generic record upserts as the long-term event-engine API

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
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/provenance.md](../contracts/provenance.md)

It follows:

- [2026-04-09-event-engine-foundation-phase-15.md](2026-04-09-event-engine-foundation-phase-15.md)
- [2026-04-09-event-execution-record-model-phase-15.md](2026-04-09-event-execution-record-model-phase-15.md)
- [2026-04-09-event-execution-authority-storage-phase-15.md](2026-04-09-event-execution-authority-storage-phase-15.md)

## 4. Scope

This slice includes:

- authority-facing event-execution transition request/result types
- optimistic precondition support for event-record lifecycle changes
- SQLite-backed transition behavior for the initial DB authority backend
- `WorkspaceEventEngine` ownership of event-record authority access

This slice does not include:

- scheduling loops that discover new trigger candidates continuously
- mutation of coordination graph state as part of successful event completion
- retry policy tuning beyond the minimum transition vocabulary
- Postgres or Git-backed event transition support

## 5. Design constraints

- Event ownership transitions must be explicit named operations, not free-form whole-record
  replacement.
- The transition model must support at least:
  - `claim`
  - `start`
  - `succeed`
  - `fail`
  - `expire`
  - `abandon`
- Optimistic concurrency must be explicit so duplicate service instances cannot silently both own
  the same execution attempt.
- `WorkspaceEventEngine` may read and write event records through the authority seam, but any
  coordination graph mutations it triggers later must still flow through the mutation broker and
  the coordination mutation protocol.

## 6. Implementation slices

### Slice 1: Define authority-facing transition types

- add backend-neutral event-execution transition request/result types to the authority layer
- include explicit precondition support for current status and current owner where needed

Exit criteria:

- event lifecycle changes have one named authority-facing transition vocabulary

### Slice 2: Implement SQLite-backed transition behavior

- apply the new transition semantics in the SQLite authority backend
- reject stale or conflicting ownership transitions explicitly

Exit criteria:

- SQLite can claim and advance event-execution records through explicit authoritative transitions

### Slice 3: Centralize event-record authority access under `WorkspaceEventEngine`

- move event-record reads and transition entry points under the event-engine owner
- avoid spreading authority-store calls across generic host helpers

Exit criteria:

- future event-engine behavior has one real owner for authoritative event-record operations

## 7. Validation

Minimum validation for this slice:

- targeted `prism-core` tests for authority transition semantics and conflict handling
- targeted `prism-mcp` tests for `WorkspaceEventEngine` authority-backed event-record access
- direct downstream compile or test coverage for changed authority-layer public types
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- event lifecycle changes use explicit authority-facing transition types
- SQLite enforces those transitions authoritatively
- `WorkspaceEventEngine` owns event-record authority access in code
- later trigger scanning and scheduling work can build on authoritative ownership transitions

## 9. Implementation checklist

- [ ] Define authority-facing transition types
- [ ] Implement SQLite-backed transition behavior
- [x] Centralize raw event-record authority access under `WorkspaceEventEngine`
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap and spec status as slices land
