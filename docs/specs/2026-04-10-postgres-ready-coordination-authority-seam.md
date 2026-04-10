# Postgres-Ready Coordination Authority Seam

Status: implemented
Audience: coordination, storage, runtime, query, MCP, CLI, and service maintainers
Scope: harden the SQL-only coordination authority abstraction so the primary seam is the real future Postgres contract rather than a lightly cleaned-up SQLite-era contract

---

## 1. Summary

The SQL-only cutover removed the shared-ref backend, but it did not finish the job.

The current authority seam is still too broad for a clean Postgres implementation because it still
mixes:

- hot-path authority operations
- snapshot-oriented recovery reads
- full-state replacement
- transaction results that can carry whole-state payloads

That is not the contract we want Postgres to inherit.

This spec hardens the seam so:

- the primary authority trait is event-first and SQL-shaped
- snapshot reads and full-state replacement move behind an explicit secondary seam
- hot authority writes return commit metadata, not caller-visible full state
- the Postgres stub targets the same split contract SQLite targets

This is the last persistence-contract cleanup that should happen before the shared execution
substrate starts building on the authority layer.

## 2. Goals

Progress note (2026-04-10):

- implementation landed by splitting the hot authority trait from snapshot/recovery operations,
  adding explicit provider access for the secondary snapshot seam, removing full-state payloads
  from normal authority transaction results, and migrating snapshot consumers onto the explicit
  secondary interface
- SQLite and the Postgres stub now compile against the same split contract

### 2.1 Make the main authority trait the real future backend contract

The primary `CoordinationAuthorityStore` should contain only the operations a real SQL backend
should implement for day-to-day product traffic:

- shaped current-state reads
- event appends
- runtime descriptor operations
- event execution record operations
- retained history reads
- diagnostics

### 2.2 Move recovery and snapshot operations off the hot seam

Snapshot-oriented operations should exist only on an explicit secondary interface.

That secondary seam is allowed to support:

- `read_snapshot`
- `read_snapshot_v2`
- `replace_current_state`

Those operations remain useful for:

- recovery
- bootstrap helpers
- targeted migration or import/export utilities

They should not be part of the primary authority contract anymore.

### 2.3 Remove full-state payloads from normal authority write results

Normal authority transaction results should carry:

- status
- committed flag
- authority stamp
- persistence metadata
- conflict information
- diagnostics

They should not carry:

- `CoordinationCurrentState`
- caller-visible full snapshots

### 2.4 Make SQLite and Postgres target the same split contract

SQLite remains the implemented backend.

Postgres may still be a stub after this change, but the stub should compile against the same split
contract rather than the old SQLite-shaped monolith.

## 3. Non-goals

This spec does not:

- implement the full Postgres backend
- redesign every future query-shaped read API
- remove all snapshot-oriented code from the repo
- remove all shared-coordination-ref utilities that still serve non-authority roles
- change coordination-domain semantics

## 4. Design

### 4.1 Primary authority trait

Keep `CoordinationAuthorityStore` as the hot-path interface, but narrow it to:

- `capabilities`
- `read_plan_state`
- `read_summary`
- `append_events`
- `publish_runtime_descriptor`
- `clear_runtime_descriptor`
- `list_runtime_descriptors`
- `read_event_execution_records`
- `upsert_event_execution_record`
- `apply_event_execution_transition`
- `read_history`
- `diagnostics`

Remove from the primary trait:

- `read_snapshot`
- `read_snapshot_v2`
- `replace_current_state`

### 4.2 Snapshot/recovery trait

Add an explicit secondary trait, for example `CoordinationAuthoritySnapshotStore`, that extends the
primary authority trait and contains:

- `read_snapshot`
- `read_snapshot_v2`
- `replace_current_state`

This makes the separation visible at the type level.

### 4.3 Provider/opening split

The store provider should expose distinct entrypoints:

- `open(...)` for the primary authority trait
- `open_snapshot(...)` for the snapshot/recovery trait

Code that needs snapshot-oriented operations should opt into that secondary seam explicitly.

### 4.4 Transaction result tightening

Remove the full-state payload from `CoordinationTransactionResult`.

After this change, callers should not expect an authority mutation to hand back the authoritative
current state blob.

If some helper genuinely needs to reload state after a committed transaction, it should issue a
read explicitly.

### 4.5 DB trait split

Mirror the same split inside `coordination_authority_store/db`:

- `CoordinationAuthorityDb` for the hot-path surface
- `CoordinationAuthoritySnapshotDb` for snapshot/recovery operations

The wrapper store should implement the corresponding public traits from those narrower DB traits.

### 4.6 Call-site migration

Move the following callers onto the explicit snapshot seam:

- `published_plans.rs` snapshot loaders and `replace_current_state` use
- any tests that seed authority state with `replace_current_state`
- any runtime helper that reads full snapshots directly from authority

Keep hot path callers on the primary seam only.

## 5. Implementation plan

1. Update the roadmap to make this phase explicit.
2. Add the new spec and doc index entries.
3. Split the public authority traits.
4. Split the DB-side authority traits.
5. Add provider helpers for opening the primary versus snapshot/recovery seam.
6. Remove snapshot payloads from `CoordinationTransactionResult`.
7. Update SQLite implementations to the split contract.
8. Update the Postgres stub to the split contract.
9. Migrate snapshot and recovery call sites.
10. Run targeted and full validation.

## 6. Validation

Minimum validation for this seam refactor:

- `cargo test -p prism-core -p prism-mcp -p prism-cli`

Required broader validation because this changes the shared authority contract:

- `cargo test`

## 7. Exit criteria

- the main authority trait no longer exposes snapshot replacement
- the main authority trait no longer exposes broad snapshot reads
- snapshot reads and full-state replacement are available only through an explicit secondary seam
- normal authority transaction results no longer contain full state
- SQLite passes behind the split contract
- the Postgres stub compiles against the same split contract
