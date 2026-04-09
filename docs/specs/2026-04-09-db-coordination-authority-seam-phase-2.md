# DB Coordination Authority Seam Phase 2

Status: completed
Audience: coordination, storage, service, MCP, CLI, and authority-backend maintainers
Scope: introduce the internal DB authority abstraction beneath `CoordinationAuthorityStore` so SQLite and Postgres can share one authority adapter family without changing product-facing code

---

## 1. Summary

Phase 1 hardened the public authority seam so it can describe a real backend family.

This slice introduces the first internal structure beneath that seam:

- one internal DB authority trait family
- one shared DB-backed authority-store adapter
- one SQLite implementation stub behind that internal DB seam
- one Postgres implementation stub behind that internal DB seam

This slice is intentionally structural.

It does not yet make SQLite functional or default.
Its job is to ensure the next implementation work lands beneath an already-real DB authority
boundary instead of scattering SQLite-specific logic upward into product code.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-db-backed-service-foundation-phase-15.md](./2026-04-09-db-backed-service-foundation-phase-15.md)

This slice follows:

- [2026-04-09-coordination-authority-family-hardening-phase-1.md](./2026-04-09-coordination-authority-family-hardening-phase-1.md)

## 3. Goals

This slice must:

- create one internal DB authority interface below `CoordinationAuthorityStore`
- create one shared `DbCoordinationAuthorityStore` adapter that delegates the public authority
  contract to a DB implementation
- keep SQLite-specific and Postgres-specific code below that DB seam
- route authority-store factory selection for SQL backends through the new DB family seam
- preserve current explicit failure behavior for unimplemented SQL backends until the SQLite
  implementation slice lands

## 4. Non-goals

This slice does not yet:

- implement working SQLite reads, writes, history, or diagnostics
- switch the default backend away from Git
- add schema or migration files
- change service deployment behavior

## 5. Design

### 5.1 Internal layering

The intended internal layering is:

- `CoordinationAuthorityStore`
  - `DbCoordinationAuthorityStore`
    - `SqliteCoordinationAuthorityDb`
    - `PostgresCoordinationAuthorityDb`
  - `GitSharedRefsCoordinationAuthorityStore`

The DB trait is internal-only.
Upper layers must keep depending on `CoordinationAuthorityStore`.

### 5.2 Delegation rule

The shared DB authority adapter should own:

- delegation from the public authority-store contract to the internal DB trait
- any future semantic shaping common to both SQL backends

The concrete DB implementations should own:

- backend-specific opening/config interpretation
- backend-specific query and transaction execution
- backend-specific diagnostics payload construction

### 5.3 Safety rule for the transition

Until SQLite is implemented, SQL backend selection should remain explicit and fail closed.

The code should route through the new DB seam already, but it should not pretend SQL backends are
working before they are.

## 6. Implementation scope

This slice includes:

- a new internal `coordination_authority_store/db/` module family
- an internal DB trait mirroring the settled authority-store behavior
- a shared DB authority-store adapter that implements `CoordinationAuthorityStore`
- SQLite and Postgres internal implementation stubs
- authority-store factory routing through the new DB seam for SQL backend configs

## 7. Exit criteria

This slice is complete when:

- SQL backend configs no longer fail directly from `factory.rs`
- the failure path for unimplemented SQL backends now flows through the DB authority family seam
- there is one obvious internal place to implement SQLite authority behavior next

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-core coordination_authority_store`

If public authority-store behavior changes while landing this slice, also run downstream tests for:

- `prism-cli`
- `prism-mcp`

## 9. Status updates

Current status:

- phase-2 spec written
- DB seam scaffold landed under `crates/prism-core/src/coordination_authority_store/db/`
- SQL backend selection in `factory.rs` now routes through the internal DB authority seam
- current SQL backends still fail closed explicitly until the SQLite implementation slice lands
