# PRISM Service Architecture

Status: normative contract  
Audience: coordination, runtime, MCP, CLI, UI, storage, auth, and future service maintainers  
Scope: one PRISM Service host process, its internal role model, shared dependencies, anti-bypass rules, and deployment modes

---

## 1. Goal

PRISM must define one explicit **PRISM Service Architecture** contract before the service is
implemented concretely.

This contract exists so that:

- the service is specified as an orchestration host rather than a vague blob
- service responsibilities are sharply separated without introducing a microservice swarm
- the service remains an optimization around the authority plane rather than becoming a hidden
  authority plane itself

## 2. Core rule

The PRISM Service is:

- one host process
- an optimization and orchestration shell around the authority plane
- composed from several narrow internal role engines

The PRISM Service is not:

- the authority plane
- a microservice swarm
- a singleton correctness dependency

If the service disappears, PRISM may become slower or less convenient, but correctness must still
be recoverable through the authority backend and local fallback behavior.

## 3. Service shell responsibilities

The service shell owns:

- lifecycle and process management
- configuration
- transport endpoints
- auth and trust plumbing
- role wiring
- observability and resource management

The service shell does not own coordination domain semantics directly.

## 4. Internal role model

The service should be composed from explicit internal roles:

- [service-authority-sync-role.md](./service-authority-sync-role.md)
- [service-read-broker.md](./service-read-broker.md)
- [service-mutation-broker.md](./service-mutation-broker.md)
- [service-runtime-gateway.md](./service-runtime-gateway.md)
- [event-engine.md](./event-engine.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)

Additional roles such as archive or export may come later, but they should remain explicit role
modules rather than ad hoc appendages.

## 5. Shared kernel beneath the roles

Service roles should compose around a small shared kernel:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [coordination-query-engine.md](./coordination-query-engine.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)

## 6. Hard anti-bypass rule

No service role may bypass the authority store, query engine, or materialized-store seams to reach
directly into Git, SQLite, or ad hoc local state "just for a shortcut."

Concretely:

- all authoritative coordination access goes through the authority store
- all deterministic coordination evaluation goes through the query engine
- all local persisted eventual-read state goes through the materialized store
- all runtime-local packets or diagnostics go through the runtime-gateway or observability seams

This rule exists to prevent the service from accumulating secret side paths that turn it into a god
object or a hidden authority layer.

## 7. Deployment modes

The same service shape should support:

- local edge mode
- repo-scoped leader mode
- org- or hosted-leader mode later
- follower mode later

These are deployment modes of one service shape, not separate products.

## 8. Initial implementation phases

Phase 1 service core:

- authority sync role
- read broker
- mutation broker

Phase 2:

- runtime gateway

Phase 3:

- event engine role

This phase ordering keeps the first service slice focused on sync, reads, and writes before it
absorbs richer runtime or scheduling behavior.

## 9. Litmus tests

For any new behavior proposed for the service, PRISM should ask:

1. If the service vanished, would PRISM still be correct?
2. Is this behavior about authority, transport, scheduling, or deterministic evaluation?

The intended answers are:

- authority -> authority store or kernel contract
- deterministic coordination evaluation -> query engine
- local persisted eventual state -> materialized store
- orchestration, coalescing, or scheduling -> service role
- transport and runtime connectivity -> runtime gateway

## 10. Minimum implementation bar

This contract is considered implemented only when:

- the service is modeled as one host process with explicit internal roles
- role boundaries reference the lower-level contracts instead of inventing their own semantics
- the service is not required for correctness
- the anti-bypass rule is upheld in code structure and tests
