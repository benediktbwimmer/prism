# PRISM Coordination Materialized Store

Status: normative contract
Audience: coordination, service, runtime, query, MCP, CLI, UI, and storage maintainers
Scope: service-owned non-authoritative coordination read models, checkpoints, indexed eventual reads, and materialization metadata

---

## 1. Goal

PRISM must define one explicit **CoordinationMaterializedStore** abstraction for non-authoritative
coordination state owned by the PRISM Service.

This contract exists so that:

- service-owned coordination materialization lives behind one seam instead of leaking across call
  sites
- eventual coordination reads use one disciplined path
- checkpoints, indexed views, and local read models are updated through one protocol
- authority and materialization do not blur back together

This abstraction is distinct from:

- [coordination-authority-store.md](./coordination-authority-store.md)
  - authoritative current state, history, and transactions
- [coordination-query-engine.md](./coordination-query-engine.md)
  - deterministic domain evaluation

Canonical ownership:

- this document defines the service-owned persisted coordination storage seam
- [local-materialization.md](./local-materialization.md) defines the broader behavioral rules for
  local materialization as a layer
- [spec-engine.md](./spec-engine.md) defines local spec parsing and spec materialization semantics
  outside the coordination storage seam

The materialized store is the service-owned persisted projection of authority, not the authority
itself.

This contract relies on:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [signing-and-verification.md](./signing-and-verification.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)

## 2. Core invariants

The materialized store must preserve these rules:

1. It is never an authority backend.
2. It may serve eventual reads, but must not silently redefine what is true.
3. It advances only from committed authoritative state or explicitly allowed local operational
   inputs defined by contract.
4. It must surface which authority version or stamp it corresponds to.
5. It must remain disposable and rebuildable.
6. Runtimes do not own or mutate coordination materialization directly.

## 3. Responsibilities

The CoordinationMaterializedStore owns:

- reading service-local eventual coordination snapshots
- reading service-local indexed task, plan, artifact, and review views
- reading and writing service-owned coordination checkpoint bundles
- replacing or advancing service-local materialized state from authoritative snapshots or commit
  results
- invalidating or clearing stale service-owned coordination materialization
- storing materialization metadata such as schema version, authority stamp, and materialized-at

It may also own:

- repo- or project-scoped imported coordination checkpoints
- service-local diagnostics tables that are explicitly part of materialization metadata

It does not own native spec parsing or spec-state materialization unless another implementation
contract explicitly chooses to colocate storage behind a separate spec-engine seam.

## 4. Non-goals

The materialized store does not own:

- authoritative commit semantics
- conflict resolution
- CAS and retry behavior
- authoritative history reconstruction
- runtime descriptor truth
- mutation acceptance or rejection
- spec parsing and spec dependency evaluation
- runtime-local telemetry, command history, or worktree-local operational state

Those belong to the authority store, mutation protocol, and related contracts.

## 5. Canonical read families

The materialized store should expose service-local eventual read families such as:

- `read_eventual_snapshot(root_id)`
- `read_eventual_plan(plan_id)`
- `read_eventual_task(task_id)`
- `read_eventual_evidence_status(task_id)`
- `read_checkpoint_bundle(root_id, scope)`
- `read_materialization_metadata(root_id, scope)`

The exact function names may vary, but the semantic families should remain.

## 6. Canonical write and update families

The materialized store should expose update families such as:

- `replace_from_authoritative_snapshot(...)`
- `apply_committed_authority_result(...)`
- `write_checkpoint_bundle(...)`
- `invalidate_materialization(...)`
- `clear_materialization(...)`

The key rule is:

- materialization advances from committed authority
- it does not advance from speculative local intent

## 7. Required metadata

Every persisted materialization unit must be traceable to:

- coordination root identity
- authority backend kind
- authority stamp, version, or equivalent
- materialization schema version
- scope identity such as coordination root, repo, project, or service partition when relevant
- materialized-at timestamp
- completeness or degraded-state metadata when relevant

## 8. Relationship to eventual reads

The CoordinationMaterializedStore is the primary service-local storage seam for eventual
coordination reads.

That means:

- service roles and product surfaces should not reach directly into SQLite tables
- eventual-read call sites should depend on the materialized-store API
- consistency and freshness metadata must still be surfaced according to
  [consistency-and-freshness.md](./consistency-and-freshness.md)

## 9. Relationship to the query engine

The materialized store does not replace the query engine.

The intended flow is:

- authority store yields authoritative snapshot or commit result
- query engine evaluates coordination semantics
- service-owned materialized store persists the bounded derived state needed for fast eventual
  reads

If a materialized store contains precomputed evaluation results, those results are still derived
outputs of the query engine contract.

## 10. Relationship to checkpoints

Coordination checkpoints are part of the materialized-store boundary.

The materialized store should therefore own:

- checkpoint persistence
- checkpoint invalidation metadata
- checkpoint lookup by coordination root and service-local scope

Checkpoint validity rules themselves are governed by
[local-materialization.md](./local-materialization.md) and
[authority-sync.md](./authority-sync.md).

## 11. Testing and implementation discipline

The materialized-store abstraction should enforce:

- no direct SQLite reads for eventual coordination state outside the store implementation
- no direct SQLite writes for coordination materialization outside the store implementation
- testability with in-memory or simplified local-store implementations where useful

For runtime participants this also means:

- no runtime-owned SQLite coordination materialization
- no runtime-side direct reads or writes of coordination eventual state

The abstraction is justified by semantic boundary and call-site discipline, not by hypothetical
backend swapping alone.

## 12. Minimum implementation bar

This contract is considered implemented only when:

- eventual coordination reads no longer depend on ad hoc SQLite access
- service-owned coordination checkpoint persistence is routed through the materialized-store seam
- materialization metadata is explicit and queryable
- local materialization updates advance only from committed authority or explicitly allowed local
  operational inputs
