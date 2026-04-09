# Event Engine Foundation Phase 15

Status: in progress
Audience: service, coordination, scheduling, MCP, runtime, and future automation maintainers
Scope: establish the first explicit event-engine role inside the service-hosted architecture without hiding event execution inside the service shell or generic host helpers

---

## 1. Summary

This spec is the next concrete Phase 15 implementation target after the runtime-gateway slice.

The goal is to make the event engine a named service role in code before PRISM starts routing
recurring execution, hook evaluation, or scheduling behavior through ad hoc shell or host helpers.

This slice should:

- give the service one named event-engine owner
- bind the event engine off the service shell rather than generic host state
- make its intended dependencies explicit: service read path, service mutation path, and later
  event execution records

This slice should not:

- finish recurring task scheduling
- implement the full authoritative event record namespace
- redesign event semantics or retry policy again
- hide event execution logic inside the service shell

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/event-engine.md](../contracts/event-engine.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)

It follows:

- [2026-04-09-runtime-gateway-foundation-phase-15.md](2026-04-09-runtime-gateway-foundation-phase-15.md)

## 4. Scope

This slice includes:

- one explicit event-engine owner in the live service host
- explicit service-shell binding for that owner
- explicit host accessors so later event execution code does not grow off broad service-shell
  reach-ins
- a named place for later event trigger evaluation and mutation routing to land

This slice does not include:

- full trigger scanning loops
- authoritative event claim or execution records
- continue-as-new plan generation
- browser-side event management UI

## 5. Design constraints

- The event engine is a service role, not the service shell itself.
- The event engine is a client of the read and mutation brokers; it does not bypass them.
- This slice should make the role boundary real in code without faking completed event semantics.
- Any temporary placeholder behavior must live under the event-engine owner, not in generic host
  helpers.

## 6. Implementation slices

### Slice 1: Name the event-engine owner

- introduce one explicit event-engine owner bound off the service shell
- expose a narrow host accessor for later event-engine entry points

Exit criteria:

- the service has a named event-engine role in code
- later scheduling or hook work has a real owner to land in

### Slice 2: Centralize event-engine entry points

- route any future event-engine-adjacent entry points through the named owner
- keep read and mutation dependencies explicit

Exit criteria:

- no new event-engine behavior lands in generic shell or host helpers

## 7. Validation

Minimum validation for this slice:

- targeted `prism-mcp` tests for host and service-shell construction paths
- targeted compile or construction tests for any new event-engine owner wiring
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- the service has one named event-engine owner in code
- service-shell wiring and host accessors are explicit
- later event-engine behavior has a dedicated landing zone instead of generic host helpers

## 9. Implementation checklist

- [x] Introduce the event-engine owner
- [ ] Centralize event-engine entry points
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
