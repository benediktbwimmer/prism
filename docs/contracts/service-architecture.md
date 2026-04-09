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
- the service remains a coordination host around the authority plane rather than becoming a hidden
  authority plane itself
- coordination participation has one required host for authority access, reads, mutations, and
  service-scoped policy enforcement
- the service is a first-class product surface rather than an accidental side effect of the MCP
  daemon

## 2. Core rule

The PRISM Service is:

- one host process
- the required coordination host around the authority plane
- composed from several narrow internal role engines
- an explicit lifecycle surface
- the host that serves the browser UI

The PRISM Service is not:

- the authority plane
- a microservice swarm
- the durable source of coordination truth
- the MCP daemon

Interactive PRISM coordination participation requires a reachable PRISM Service.

If the service disappears:

- the authority backend remains the durable coordination truth
- offline tooling, export, migration, or diagnostics may still exist
- interactive runtime participation in coordination should fail clearly rather than half-working
  against runtime-owned coordination state

## 3. Service shell responsibilities

The service shell owns:

- lifecycle and process management
- configuration
- endpoint discovery state for machine-local mode
- transport endpoints
- bundled browser UI serving and browser-session plumbing
- auth and trust plumbing
- role wiring
- observability and resource management
- repo partitioning and multi-repo coordination-root hosting

The service shell does not own coordination domain semantics directly.

Service startup remains explicit.
The system must not implicitly boot the service from MCP or bridge flows.

## 4. Internal role model

The service should be composed from explicit internal roles:

- [service-authority-sync-role.md](./service-authority-sync-role.md)
- [service-read-broker.md](./service-read-broker.md)
- [service-mutation-broker.md](./service-mutation-broker.md)
- [service-runtime-gateway.md](./service-runtime-gateway.md)
- [event-engine.md](./event-engine.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [service-auth-and-session-model.md](./service-auth-and-session-model.md)

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

Within that kernel:

- the service owns coordination participation
- runtimes do not own coordination SQLite state
- runtimes remain clients of the service for coordination participation
- DB-backed authority may serve the default read path directly without separate coordination
  materialization

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

## 7. Deployment modes and endpoint selection

The same service shape should support:

- local service with SQLite authority
- local or hosted service with Postgres authority
- local or hosted service with Git authority later when that backend is selected
- follower or edge modes later

One service process may serve:

- many repos
- many coordination roots
- many worktrees
- many runtimes

Endpoint selection follows this rule:

1. explicit configured endpoint
2. otherwise machine-local service discovery
3. otherwise fail clearly

If an explicit service endpoint is configured, it must fail loudly if unavailable.
The system must not silently fall back to a local machine service in that case.

In hosted mode:

- runtimes and MCP clients connect directly to the hosted service by default
- a local service proxy is optional later, not required

In local mode:

- one machine-local PRISM Service is the intended topology

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

## 9. MCP and UI boundary

The MCP daemon is the worktree-local runtime and MCP surface only.

The MCP daemon may be bridge-launched or bridge-restarted, but it is not the PRISM Service.

The browser UI is served by the PRISM Service, not by the MCP daemon.

Bridge-assisted MCP launch or restart must not be treated as implicit service boot or implicit
authentication.

## 10. Litmus tests

For any new behavior proposed for the service, PRISM should ask:

1. If the service vanished, would PRISM still be correct?
2. Is this behavior about authority, transport, scheduling, or deterministic evaluation?

The intended answers are:

- authority -> authority store or kernel contract
- deterministic coordination evaluation -> query engine
- service-owned persisted eventual state -> materialized store
- orchestration, coalescing, or scheduling -> service role
- transport and runtime connectivity -> runtime gateway
- browser UI, browser session handling, and admin surfaces -> service shell plus service
  capability contract

## 11. Minimum implementation bar

This contract is considered implemented only when:

- the service is modeled as one host process with explicit internal roles and lifecycle
- role boundaries reference the lower-level contracts instead of inventing their own semantics
- interactive coordination participation is modeled as service-backed rather than runtime-owned
- UI serving is service-owned rather than MCP-owned
- the anti-bypass rule is upheld in code structure and tests
