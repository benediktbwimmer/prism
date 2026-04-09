# PRISM Local Materialization Contract

Status: normative contract
Audience: coordination, runtime, service, query, MCP, CLI, UI, and storage maintainers
Scope: durable service-local coordination materialization plus runtime-local operational materialization, acceleration, invalidation, rebuild, and downstream read-model behavior

---

## 1. Goal

PRISM must define one explicit contract for **local materialization**.

This document defines the broader behavioral rules for local materialization as a persistence
plane. The concrete storage seam for persisted coordination state lives in
[coordination-materialized-store.md](./coordination-materialized-store.md).

This contract exists so that:

- service-local coordination SQLite, checkpoints, and read models are clearly downstream of
  authority
- product code can rely on materialization semantics without treating local state as authority
- backend substitution does not force a redesign of local acceleration behavior

Canonical ownership:

- this document defines materialization behavior, invalidation, rebuild, and product-surface rules
- [coordination-materialized-store.md](./coordination-materialized-store.md) defines the concrete
  service-owned persisted coordination storage seam for that behavior
- [spec-engine.md](./spec-engine.md) defines the local spec-state materialization boundary for
  native specs

This contract is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [coordination-query-engine.md](./coordination-query-engine.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 2. Core invariants

The local materialization layer must preserve these rules:

1. Local materialization is never an authority backend.
2. Local materialization may accelerate reads, but must not silently author new truth.
3. Coordination materialization is owned by the PRISM Service, not by participating runtimes.
4. Local materialization advances only from authoritative inputs or explicit local operational
   sources defined by contract.
5. Materialized state must be disposable and rebuildable.
6. Invalid or stale local materialization must degrade honestly rather than pretending to be
   current authority.

## 3. What local materialization may contain

Local materialization may include:

- service-local coordination read models
- repo- or project-scoped imported coordination checkpoints
- service-owned coordination checkpoints
- derived summaries and serving indexes
- query acceleration structures
- runtime-local caches for hot serving
- runtime-local operational continuity state such as activity sensing, command history, and local
  diagnostics that are not authoritative coordination state

Service-local coordination materialization and runtime-local operational materialization are
different families and must not blur together.

When native specs are enabled, local materialization may also include spec-derived local indexes and
status views, but those remain branch-local and non-authoritative.

## 4. What local materialization may not do

Local materialization must not:

- accept authoritative commits directly
- become the sole cross-runtime source of coordination truth
- hide freshness loss from callers
- invent state transitions that were not committed through the mutation protocol
- let runtimes behave as mini coordination databases

## 5. Materialization inputs

Local materialization may advance from:

- authoritative current-state reads
- authoritative committed transaction results
- authoritative history reconstruction when explicitly requested
- explicit local runtime operational sources such as activity summaries,
  when those sources are not being represented as authority

The source of each materialized update must be traceable.

Authority-sensitive materialization should be based on verified authoritative inputs rather than
unverified candidates.

## 6. Invalidation contract

Materialization must key invalidation on explicit authority or runtime inputs, not vague drift.

At minimum, invalidation keys should include:

- coordination root identity
- authority backend kind
- authority stamp, version, or equivalent
- materialization schema version
- service scope for coordination materialization
- worktree or runtime scope only for runtime-local operational materialization

If any required input changes incompatibly, the dependent materialized state must be refreshed,
discarded, or marked stale.

## 7. Rebuild guarantees

Materialization must be rebuildable from authoritative inputs plus allowed local operational inputs.

That means:

- deleting materialized state may make PRISM slower
- deleting materialized state must not make PRISM less correct

## 8. Read semantics

Eventual reads may serve from materialization according to
[consistency-and-freshness.md](./consistency-and-freshness.md).

Strong reads may update or bypass materialization as needed, but the caller-facing semantics remain:

- authority determines truth
- materialization determines speed

## 9. Checkpoints

Coordination checkpoints are part of service-local materialization.

They exist to:

- accelerate warm restart
- avoid unnecessary recomputation
- preserve service continuity across daemon restarts

They do not exist to redefine authority.

Checkpoint restore is valid only when its contractually required inputs still match or when the
checkpoint contract explicitly allows a bounded replay over the restored snapshot.

## 10. Product-surface rules

Product surfaces may depend on local materialization for performance, but they must not hardcode
local materialization as if it were the authority boundary.

That includes:

- MCP reads
- SSR console and API views
- CLI status and inspection
- future PRISM Service read broker

Runtimes may depend on runtime-local operational materialization, but interactive coordination
participation requires a reachable PRISM Service rather than a runtime-owned coordination store.

## 11. Minimum implementation bar

This contract is considered implemented only when:

- local read models and checkpoints are treated as downstream of authority
- invalidation keys are explicit
- stale or missing materialization degrades honestly
- deleting materialization preserves correctness
