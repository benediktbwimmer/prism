# Coordination Authority Family Hardening Phase 1

Status: completed
Audience: coordination, storage, service, MCP, CLI, and authority-backend maintainers
Scope: harden the public `CoordinationAuthorityStore` seam so it cleanly hosts a backend family instead of leaking Git-shaped taxonomy and diagnostics into upper layers

---

## 1. Summary

PRISM already has a real public coordination authority seam:

- `CoordinationAuthorityStore`

What it does not yet have is a fully hardened authority-family surface.

The current trait is usable, but several surrounding types and helpers still encode the first
backend too directly:

- backend taxonomy is incomplete for the intended family
- diagnostics still expose Git-only detail shapes at the authority-store type boundary
- several product surfaces still route through `shared_coordination_ref_diagnostics(...)`
  instead of the authority-store diagnostics surface

This slice fixes that seam before the internal DB family lands.

The result of this phase should be:

- one public authority taxonomy that names Git, SQLite, and Postgres coherently
- one authority diagnostics surface whose top-level shape is backend-neutral
- one explicit place for Git-specific diagnostics detail to live without pretending it is the
  entire backend detail model
- fewer direct product-surface dependencies on shared-ref diagnostics helpers

This slice is the opening implementation phase of:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

## 2. Related contracts and prior specs

This slice hardens:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)

This slice follows:

- [2026-04-09-db-backed-service-foundation-phase-15.md](./2026-04-09-db-backed-service-foundation-phase-15.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

Current checkpoint:

- [x] phase spec written
- [x] authority-store backend taxonomy widened to include `Sqlite`
- [x] authority-store backend details widened to host Git, SQLite, and Postgres variants
- [x] CLI and MCP runtime-status consumers now load backend details through
  `CoordinationAuthorityStore::diagnostics(...)`
- [x] main product-facing diagnostics consumers no longer bypass the authority-store seam

## 3. Problems to solve now

### 3.1 Backend taxonomy is incomplete

The intended authority family is:

- Git shared refs
- SQLite
- Postgres

But the current public backend kind only names:

- `GitSharedRefs`
- `Postgres`

That mismatch makes the steady-state family look unfinished before implementation even starts.

### 3.2 Backend details are still Git-shaped

The current authority diagnostics type effectively says:

- backend details are either `GitSharedRefs(...)` or `Unavailable`

That is not the right contract for a backend family where SQLite and Postgres are meant to be
first-class implementations.

### 3.3 Product surfaces still bypass the diagnostics seam

Several MCP, CLI, and core status surfaces still call:

- `shared_coordination_ref_diagnostics(...)`

directly.

That keeps Git-specific diagnostic loading alive as a product-facing pattern, even though the
authority-store contract already has `diagnostics(...)`.

## 4. Goals

This slice must:

- make backend taxonomy match the intended authority family
- make authority diagnostics top-level types backend-neutral
- keep Git-specific detail available without making it the only backend detail story
- move the most obvious product-facing diagnostics consumers onto the authority-store surface
- reduce the amount of public API surface that still re-exports shared-ref diagnostics as if they
  were backend-neutral authority data

## 5. Non-goals

This slice does not yet:

- implement the DB authority seam itself
- implement SQLite transactional authority behavior
- make SQLite the default backend yet
- delete the Git backend
- finish every remaining shared-ref consumer in one pass

## 6. Implementation scope

### 6.1 Public type hardening

Update the authority-store type family so that it can describe the intended backend family
cleanly:

- add `Sqlite` to `CoordinationAuthorityBackendKind`
- replace the Git-only backend-details enum with a backend-family shape
- keep top-level diagnostics generic and put Git-specific detail behind a nested backend-specific
  variant

### 6.2 Product-surface diagnostics hardening

Move the first round of obvious diagnostics consumers from:

- `shared_coordination_ref_diagnostics(...)`

to:

- `CoordinationAuthorityStore::diagnostics(...)`

This includes the product-facing status and trust surfaces that should already think in terms of
the authority-store seam.

### 6.3 Public API cleanup

Where the repo still exposes direct shared-ref diagnostics helpers as a top-level authority API,
narrow or demote those surfaces so the preferred path is clearly the authority-store contract.

## 7. Exit criteria

This slice is complete when:

- the public backend kind includes `Sqlite`
- backend details no longer imply that Git detail is the only meaningful authority detail shape
- the main diagnostics consumers use the authority-store diagnostics path
- contracts, roadmap, and this phase spec all describe the same hardened seam

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-core coordination_authority_store`

If public `prism-core` types used by downstream crates change, also run:

- `cargo test -p prism-cli`
- `cargo test -p prism-mcp`

## 9. Status updates

Current status:

- contract updates landed
- Rust seam hardening landed
- remaining Git-specific public facade cleanup is deferred to later cleanup slices so the DB
  authority family can land next
