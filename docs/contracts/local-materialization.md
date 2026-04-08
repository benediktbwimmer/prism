# PRISM Local Materialization Contract

Status: normative contract  
Audience: coordination, runtime, query, MCP, CLI, UI, storage, and future service maintainers  
Scope: durable local materialization, acceleration, invalidation, rebuild, and downstream read-model behavior

---

## 1. Goal

PRISM must define one explicit contract for **local materialization**.

This document defines the broader behavioral rules for local materialization as a persistence
plane. The concrete storage seam for persisted local coordination state lives in
[coordination-materialized-store.md](./coordination-materialized-store.md).

This contract exists so that:

- local SQLite, checkpoints, and read models are clearly downstream of authority
- product code can rely on materialization semantics without treating local state as authority
- backend substitution does not force a redesign of local acceleration behavior

Canonical ownership:

- this document defines materialization behavior, invalidation, rebuild, and product-surface rules
- [coordination-materialized-store.md](./coordination-materialized-store.md) defines the concrete
  persisted coordination storage seam for that behavior
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
3. Local materialization advances only from authoritative inputs or explicit local operational
   sources defined by contract.
4. Materialized state must be disposable and rebuildable.
5. Invalid or stale local materialization must degrade honestly rather than pretending to be
   current authority.

## 3. What local materialization may contain

Local materialization may include:

- worktree-local SQLite read models
- repo- or project-scoped imported coordination checkpoints
- startup checkpoints
- derived summaries and serving indexes
- query acceleration structures
- runtime-local caches for hot serving

It may also contain local operational continuity state that is not authoritative but is durable
enough to survive restart.

When native specs are enabled, local materialization may also include spec-derived local indexes and
status views, but those remain branch-local and non-authoritative.

## 4. What local materialization may not do

Local materialization must not:

- accept authoritative commits directly
- become the sole cross-runtime source of coordination truth
- hide freshness loss from callers
- invent state transitions that were not committed through the mutation protocol

## 5. Materialization inputs

Local materialization may advance from:

- authoritative current-state reads
- authoritative committed transaction results
- authoritative history reconstruction when explicitly requested
- explicit local runtime operational sources such as restart checkpoints or activity summaries,
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
- worktree or runtime scope when relevant

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

Startup checkpoints are part of local materialization.

They exist to:

- accelerate warm restart
- avoid unnecessary recomputation
- preserve local continuity across daemon restarts

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

## 11. Minimum implementation bar

This contract is considered implemented only when:

- local read models and checkpoints are treated as downstream of authority
- invalidation keys are explicit
- stale or missing materialization degrades honestly
- deleting materialization preserves correctness
