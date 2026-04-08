# PRISM Event Engine Contract

Status: normative contract  
Audience: coordination, runtime, event-engine, MCP, CLI, and future service maintainers  
Scope: recurring execution, hook evaluation, event identity, authoritative execution ownership, retry, retention, and service-role interaction

---

## 1. Goal

PRISM must define one explicit **event engine** contract before the event engine is embedded inside
the future PRISM Service.

This contract exists so that:

- event execution is a client of the authority, query, and mutation seams
- exactly-once execution ownership is defined concretely
- hook execution and retries are not hidden inside service implementation detail
- the event engine remains a role inside the service, not the service itself

## 2. Core invariants

The event engine must preserve these rules:

1. Event execution ownership is authoritative.
2. Exactly-once ownership does not imply universal exactly-once side effects.
3. Event execution must read verified current state before acting when correctness requires it.
4. Event execution mutations commit through the coordination mutation protocol.
5. Retry and expiry behavior must be explicit.
6. The preferred execution plane may be the PRISM Service, but correctness must not depend on one
   singleton service instance.
7. Event execution records must not pollute the hot coordination summary state.

## 3. Required responsibilities

The event engine contract must define:

- recurring execution and continue-as-new semantics
- hook execution boundaries
- event identity
- execution ownership acquisition
- execution record model
- retry lifecycle
- expiry and abandonment lifecycle
- authoritative mutation interaction
- relationship to the PRISM Service roles

## 4. Relationship to other contracts

The event engine is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-query-engine.md](./coordination-query-engine.md)
- [coordination-mutation-protocol.md](./coordination-mutation-protocol.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [identity-model.md](./identity-model.md)
- [service-read-broker.md](./service-read-broker.md)
- [service-mutation-broker.md](./service-mutation-broker.md)

It must not bypass those seams.

## 5. Exactly-once ownership

Exactly-once ownership means:

- one owner is authoritative for one execution attempt at a time
- duplicate owners must be prevented by authoritative coordination

It does not mean:

- every downstream side effect in the outside world is automatically exactly once

Those side effects still require idempotence or explicit external compensation where needed.

## 6. Continue-as-new

Recurring work should be modeled at the plan boundary.

The event engine should support continue-as-new semantics where completion of one plan instance can
authoritatively create the next instance from a template while archiving the previous instance.

This keeps recurring execution bounded rather than reopening old tasks in place.

## 7. Hook execution boundary

Hooks should run against verified coordination state, not whatever local SQLite state happens to
exist.

When the PRISM Service is present, the preferred execution input is the verified view served
through the service's read path.

## 8. Event identity

Event identity must be transition-based rather than predicate-only.

The deterministic event id should be derived from at least:

- trigger kind
- target identity
- authoritative revision or transition marker
- hook identity
- hook version or digest when relevant

## 9. Execution record model

The event engine must use authoritative execution records rather than a one-bit processed
tombstone.

The minimum lifecycle should support:

- `claimed`
- `running`
- `succeeded`
- `failed`
- `expired`
- `abandoned`

## 10. Event authority namespace

Event execution records are authoritative, but they are not coordination graph state.

They should therefore live in a dedicated authoritative event namespace rather than being embedded
into the hot coordination summary state.

## 11. Relationship to the PRISM Service

The event engine is a service role, not the service shell itself.

The intended relationship is:

- service read path provides verified current inputs
- event engine evaluates triggers and execution candidates
- service mutation path commits event ownership and resulting authoritative mutations

The service is the preferred execution plane for efficiency.
The authority backend remains the correctness plane for ownership and durable results.

## 12. Retention and compaction

Event execution records must be retained long enough for:

- deduplication
- failure visibility
- audit

But they should still be compactable according to explicit retention policy.

## 13. Minimum implementation bar

This contract is considered implemented only when:

- event execution ownership is explicit and authoritative
- event retries and expiry are modeled explicitly
- event-triggered mutations route through the coordination mutation protocol
- the event engine reads through the service or authority/query seams rather than bypassing them
