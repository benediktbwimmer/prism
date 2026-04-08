# ADR: Service-Owned Coordination Materialization

Status: accepted  
Date: 2026-04-08  
Scope: ownership of coordination materialization, coordination participation model, and runtime responsibilities

---

## Context

PRISM previously described a coordination architecture where local runtimes could own coordination
materialization in worktree-local SQLite state and the future PRISM Service was mainly an optional
optimization host around the authority plane.

That shape created a blurry ownership boundary:

- runtimes acted partly like execution shells and partly like mini coordination databases
- eventual coordination reads were spread across many local stores
- multi-worktree and cross-repo use implied duplicated coordination state
- the future hosted-service model inherited unnecessary runtime-local coordination complexity

## Decision

PRISM adopts the following target:

- interactive coordination participation requires a reachable PRISM Service
- the PRISM Service owns coordination materialization and coordination checkpoints
- runtimes do not own coordination SQLite state
- runtimes remain responsible for worktree-local operational and telemetry state only
- the active authority backend remains the durable source of coordination truth
- one service process may serve many repos, many coordination roots, many worktrees, and many
  runtimes

This applies regardless of authority backend:

- local service + Git authority
- hosted service + Git authority
- hosted service + PostgreSQL authority

The materialization backend is configurable by deployment and backend:

- SQLite
- in-memory only
- disabled where appropriate

## Consequences

### Positive

- coordination ownership becomes much clearer
- runtimes get lighter and simpler
- eventual-read ownership moves to one host instead of many worktree-local stores
- hosted-service and cross-repo architectures fit naturally
- PostgreSQL-backed authority can disable or simplify materialization where appropriate

### Tradeoffs

- the service is no longer merely an optional optimization for interactive coordination
- runtime startup and failure handling must be explicit about service reachability
- contracts and roadmaps must stop assuming runtime-owned coordination materialization

## Resulting ownership split

### Authority backend owns

- durable coordination truth
- current state
- retained history
- transactions

### PRISM Service owns

- coordination authority access
- coordination materialization
- strong and eventual read brokering
- coordination query evaluation
- mutation brokering
- runtime descriptor publication and discovery

### Runtimes own

- worktree-local telemetry
- local activity sensing
- command and execution history
- local MCP bridge behavior
- local hints, interventions, and other operational packets

## Follow-through requirements

The following docs must align with this ADR:

- `docs/contracts/coordination-materialized-store.md`
- `docs/contracts/local-materialization.md`
- `docs/contracts/service-architecture.md`
- `docs/contracts/service-read-broker.md`
- `docs/contracts/service-authority-sync-role.md`
- `docs/contracts/runtime-identity-and-descriptors.md`
- `docs/roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md`
