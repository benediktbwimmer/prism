# ADR: DB-Backed Coordination Authority First

Status: accepted
Date: 2026-04-08
Scope: coordination authority backend release order, backend layering, and deployment defaults

---

## Context

PRISM now has a real backend-neutral coordination authority seam, but the repo still contains older
assumptions that the first production release should ride primarily on the Git shared-ref backend.

That assumption no longer matches the intended launch path.

The shared-ref backend remains valuable and implemented, but it still implies extra work in areas
such as:

- concurrency hardening
- service-side coalescing and retry behavior
- retention and compaction policy
- more complex deployment and failure-mode reasoning
- larger protocol and validation surface before the first robust release

At the same time, PRISM's main product value does not depend on shipping Git authority first.
The more important differentiators are:

- the coordination model itself
- deterministic bounded coordination queries
- the artifact and review model
- the service and runtime split
- native specs as a feature-intent layer
- provenance, signing, and verification

## Decision

PRISM adopts the following release ordering and backend layering:

- the first production-grade coordination authority path is **DB-backed**
- **Postgres** is the first multi-instance or hosted production backend
- **SQLite** is also supported as the DB backend for single-instance or local-service deployments
- the current **Git shared-ref backend** remains a serious long-term and advanced backend, but it
  is not the release-critical path

The intended layering is:

- `CoordinationAuthorityStore`
  - `DbCoordinationAuthorityStore`
    - `SqliteCoordinationDb`
    - `PostgresCoordinationDb`
  - later or parallel `GitCoordinationAuthorityStore`

This does not change the core truth rule:

- each coordination root has exactly one active authority backend at a time

## Consequences

### Positive

- the first release path can rely on mature transaction and concurrency semantics
- service behavior becomes simpler and easier to harden
- local single-instance and hosted multi-instance deployments share one authority model
- Git shared refs can continue to mature without blocking launch

### Tradeoffs

- some older docs must stop describing Git authority as the default release path
- the service and authority contracts must describe DB-backed authority as the initial shipping
  family
- Git-backed coordination becomes an advanced or later backend rather than the main launch story

## Follow-through requirements

The following docs should reflect this ADR:

- `docs/contracts/coordination-authority-store.md`
- `docs/contracts/service-architecture.md`
- `docs/contracts/service-capability-and-authz.md`
- `docs/designs/PRISM_COORDINATION_TARGET_ARCHITECTURE.md`
- `docs/PRISM_SHARED_COORDINATION_REFS.md`
- `docs/designs/PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md`
- `docs/designs/PRISM_CROSS_REPO_FUTURE_DESIGN.md`
- `docs/roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md`
