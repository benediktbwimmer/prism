# Event Trigger Execution Lifecycle Phase 15

Status: draft
Audience: service, coordination, event-engine, scheduling, MCP, and future automation maintainers
Scope: add the first explicit service-owned execution pass for claimed event records, including start, terminal transitions, and deterministic lifecycle outcomes

---

## 1. Summary

This spec is the next concrete Phase 15 target after the first authoritative claim loop.

The goal is to move from "the service can claim due triggers authoritatively" to "the service can
run one explicit execution pass over claimed records and persist lifecycle transitions cleanly."

This slice should:

- define one explicit event-engine execution-pass owner path inside the service
- load claimed execution records through the settled authority and read seams
- transition claimed executions into `running`
- persist deterministic terminal outcomes such as `succeeded`, `failed`, or `expired`

This slice should not:

- add a general external hook plugin system
- add hosted multi-instance scheduling policy beyond authoritative transition correctness
- add browser or CLI event-management UI
- add broad retry policy beyond the minimum transitionable lifecycle

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/event-engine.md](../contracts/event-engine.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)

It follows:

- [2026-04-09-event-trigger-claim-loop-phase-15.md](2026-04-09-event-trigger-claim-loop-phase-15.md)
- [2026-04-09-event-execution-authority-transitions-phase-15.md](2026-04-09-event-execution-authority-transitions-phase-15.md)

## 4. Scope

This slice includes:

- one explicit event-engine method for executing transitionable claimed records
- authoritative `claimed -> running` transitions through owner-checked preconditions
- authoritative terminal transitions for minimal execution outcomes
- deterministic outcome payloads that explain applied, conflicting, skipped, and terminal results

This slice does not include:

- generic retry scheduling
- broad recurrence policy rescheduling
- hosted distributed execution leases beyond authoritative owner checks
- full hook-registry ergonomics

## 5. Design constraints

- The event engine remains a client of the read, authority, and mutation seams; it does not bypass
  them.
- Execution lifecycle behavior must use explicit event-record transitions, not in-place record
  rewrites.
- The initial execution pass may support only the current recurring plan trigger family, but the
  lifecycle vocabulary must not be hardcoded to one trigger forever.
- Terminal outcome persistence must be deterministic enough that later retry and expiry policy can
  build on it cleanly.

## 6. Implementation slices

### Slice 1: Define execution-pass inputs and outcomes

- define the event-engine-facing input and result types for one execution pass
- keep transitionable versus skipped versus conflicted outcomes explicit

Exit criteria:

- the service has one named execution-pass vocabulary

### Slice 2: Load runnable claimed executions

- read claimed execution records through the authority seam
- determine which records are runnable, expired, or blocked by ownership or lifecycle state

Exit criteria:

- one event-engine owner can explain which claimed records are eligible to start

### Slice 3: Apply running and terminal transitions

- apply `start` transitions through authoritative owner-checked preconditions
- persist terminal `succeeded`, `failed`, or `expired` outcomes through authoritative transitions

Exit criteria:

- one execution pass can move claimed records through explicit lifecycle transitions without
  bypassing the authority seam

## 7. Validation

Minimum validation for this slice:

- targeted `prism-mcp` tests for execution-pass candidate loading, owner-checked `start`, and one
  terminal outcome
- targeted `prism-core` tests for any new authority-facing execution lifecycle inputs
- direct downstream compile or test coverage for changed event-engine owner APIs
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- the service has one named event-execution pass entry point
- runnable claimed records are read through settled seams
- `running` and terminal transitions use authoritative event-record transitions
- the event engine can explain applied, conflicting, skipped, and terminal execution outcomes

## 9. Implementation checklist

- [ ] Define event-engine execution-pass input and result types
- [ ] Load runnable claimed executions through settled seams
- [ ] Apply authoritative running and terminal transitions through the event-engine owner
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap and spec status as slices land
