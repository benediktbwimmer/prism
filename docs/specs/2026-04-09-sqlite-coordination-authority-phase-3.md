# SQLite Coordination Authority Phase 3

Status: completed
Audience: coordination, storage, service, MCP, CLI, and authority-backend maintainers
Scope: implement the first functioning DB-backed `CoordinationAuthorityStore` backend on SQLite and switch the default authority path to it

---

## 1. Summary

This slice turns the internal DB authority seam into a real backend.

The implementation target is:

- use the repo-shared coordination SQLite DB as the authority source of truth
- implement strong current-state reads by replaying the authoritative coordination event stream
- implement eventual reads from the materialized checkpoint surface
- implement authority transactions through the existing SQLite coordination persist protocol
- persist startup checkpoint and read-model materialization from the backend so the default SQLite
  path stays coherent
- switch `CoordinationAuthorityStoreProvider::default()` to SQLite only after this backend works

This slice is the first point where `CoordinationAuthorityStore` should be truly usable without Git
shared refs as the default local authority path.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-db-coordination-authority-seam-phase-2.md](./2026-04-09-db-coordination-authority-seam-phase-2.md)
- [2026-04-09-db-backed-service-foundation-phase-15.md](./2026-04-09-db-backed-service-foundation-phase-15.md)

## 3. Goals

This slice must:

- make the SQLite authority backend actually read current state
- make the SQLite authority backend actually apply transactions with deterministic revision
  conflict handling
- persist runtime descriptors through the SQLite authority path
- surface retained history and diagnostics for the SQLite backend
- switch the default authority provider from Git shared refs to SQLite
- narrow obvious Git-only authority-sync assumptions enough that the SQLite default path does not
  run through Git live-sync behavior accidentally

## 4. Non-goals

This slice does not yet:

- implement Postgres behavior
- remove the Git authority backend
- finish every remaining Git-shaped product surface
- solve multi-instance authority coordination

## 5. Design

### 5.1 Source-of-truth rule

The repo-shared coordination SQLite DB is the authority source of truth for the SQLite backend.

The backend should not invent a second SQLite authority store beside the existing repo-shared
coordination DB path.

### 5.2 Read rule

- strong reads should load the authoritative coordination event stream from SQLite and replay the
  current snapshot
- eventual reads should use the materialized startup-checkpoint surface and surface stale/current
  freshness honestly

### 5.3 Transaction rule

Transactions should use the existing SQLite coordination persist protocol with expected revision
checks.

After a successful authority write, the backend should update the derived materialization surfaces
needed for eventual reads.

### 5.4 Runtime descriptor rule

Runtime descriptors remain authority data, but for the SQLite path they may be persisted via the
coordination startup-checkpoint materialization surface until a more specialized storage shape is
needed.

## 6. Implementation scope

This slice includes:

- a functioning `SqliteCoordinationAuthorityDb`
- authority stamp and diagnostics shaping for SQLite
- eventual and strong read behavior for SQLite
- transaction, publish-descriptor, and clear-descriptor behavior for SQLite
- SQLite retained-history loading
- default backend selection switched to SQLite
- authority live-sync helpers narrowed to no-op for non-Git defaults

## 7. Exit criteria

This slice is complete when:

- `CoordinationAuthorityStoreProvider::default()` opens a functioning SQLite authority backend
- the main local authority read and mutation paths work without Git shared refs
- targeted tests cover SQLite authority reads, writes, conflicts, and default backend selection

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-core coordination_authority_store`
- `cargo test -p prism-cli mcp`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`

## 9. Status updates

Current status:

- phase-3 spec written
- SQLite-backed authority reads, transactions, runtime descriptor persistence, retained history, and
  diagnostics are implemented through the DB authority seam
- SQLite is now the default authority backend
- non-Git authority live-sync defaults now no-op instead of routing through shared-ref polling
- downstream authority-surface validation has been updated around the new SQLite default
