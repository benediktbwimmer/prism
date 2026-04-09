# Service-Owned Coordination Materialization Slice 1

Status: completed
Audience: coordination, service, storage, runtime, MCP, CLI, and path-layout maintainers
Scope: move coordination materialization off the generic worktree-local cache DB path onto a dedicated repo-shared coordination materialization path, with legacy migration

---

## 1. Summary

This spec is the concrete implementation target for Phase 2 of:

- [../roadmaps/2026-04-09-abstraction-adoption-and-service-state-cleanup.md](../roadmaps/2026-04-09-abstraction-adoption-and-service-state-cleanup.md)

The contracts already say coordination materialization is service-owned.
The current implementation still stores it through the generic worktree cache path.

This slice makes the implementation follow the contract more closely by:

- introducing a dedicated repo-shared coordination materialization DB path
- moving `SqliteCoordinationMaterializedStore` to that path
- migrating legacy coordination materialization state forward from the worktree cache when needed

This slice does not try to finish the entire service shell or runtime gateway story.

## 2. Status

Current state:

- [x] coordination materialization already goes through `CoordinationMaterializedStore`
- [x] the SQLite materialized-store backend no longer uses `PrismPaths::worktree_cache_db_path()`
- [x] coordination materialization path ownership is no longer mixed with generic worktree cache state
- [x] legacy coordination materialization state is explicitly migrated into a repo-shared path

Current slice notes:

- this is a storage-owner correction, not a semantic redesign
- runtime-local telemetry and generic worktree cache data remain worktree-local
- only coordination materialization should move in this slice
- live coordination materialization writers now target the repo-shared store directly
- the old store-backed coordination materialization adapter has been removed

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/local-materialization.md](../contracts/local-materialization.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)

This spec follows:

- [2026-04-08-service-backed-coordination-cutover-phase-5.md](2026-04-08-service-backed-coordination-cutover-phase-5.md)
- [2026-04-09-db-backed-service-foundation-phase-15.md](2026-04-09-db-backed-service-foundation-phase-15.md)

## 4. Scope

This slice includes:

- a dedicated `PrismPaths` accessor for repo-shared coordination materialization storage
- `SqliteCoordinationMaterializedStore` opening that dedicated path instead of the worktree cache
  DB path
- one bounded migration path for:
  - startup checkpoint
  - read model
  - queue read model
  - compaction snapshot metadata
- narrow updates to diagnostics or status surfaces that need to report the new path

This slice does not include:

- moving runtime-local telemetry or operator auth state
- changing the `CoordinationMaterializedStore` public contract
- introducing Postgres-backed coordination materialization
- full service-shell extraction

## 5. Non-goals

This slice should not:

- leave two active coordination materialization locations in normal operation
- copy unrelated worktree-local runtime state into the repo-shared coordination materialization DB
- redesign `PrismPaths` wholesale
- pretend service-owned materialization means hosted-only; local single-process service remains
  valid

## 6. Design

### 6.1 Path-owner rule

Coordination materialization should live in repo-shared PRISM home storage, not the worktree cache
DB.

The path should be:

- repo-shared
- deterministic per repo
- distinct from runtime-local worktree cache state

### 6.2 Migration rule

If the dedicated coordination materialization store does not yet contain coordination state, the
implementation may migrate forward bounded coordination materialization artifacts from the legacy
worktree cache DB.

The migration should only move:

- coordination startup checkpoint
- coordination read model
- coordination queue read model
- coordination compaction snapshot

It should not move unrelated runtime-local state.

### 6.3 Steady-state rule

After migration:

- `SqliteCoordinationMaterializedStore` should read and write only the dedicated repo-shared path
- runtime-local cache reporting should remain about worktree-local runtime state, not coordination
  authority projections

## 7. Implementation slices

### Slice 1: Add dedicated path support

- add a repo-shared coordination materialization DB path accessor to `PrismPaths`
- keep existing worktree cache path behavior unchanged for non-coordination callers

Exit criteria:

- coordination materialization has a named path owner distinct from `worktree_cache_db_path`
- status: complete

### Slice 2: Cut `SqliteCoordinationMaterializedStore` over

- make the SQLite coordination materialized store use the new path
- add bounded migration from legacy worktree cache coordination artifacts

Exit criteria:

- live coordination materialized-store operations no longer open the generic worktree cache DB path
- status: complete

### Slice 3: Update affected surfaces

- update any runtime status or diagnostics views that need to surface the new path or owner
- keep the distinction between runtime-local cache and coordination materialization explicit

Exit criteria:

- operator-facing surfaces do not mislabel the new store as generic worktree cache state
- status: complete

## 8. Validation

Minimum validation for this slice:

- targeted `prism-core` tests for `PrismPaths` and `SqliteCoordinationMaterializedStore`
- targeted `prism-mcp` or `prism-cli` tests only where path-reporting surfaces change
- `git diff --check`

Important regression checks:

- legacy coordination materialization still becomes visible after migration
- runtime-local cache behavior remains intact
- coordination materialized-store writes are now isolated from the generic worktree cache DB path

## 9. Completion criteria

This slice is complete only when:

- coordination materialization uses a dedicated repo-shared path
- legacy coordination materialization is migrated forward in bounded form
- path-reporting surfaces that matter no longer imply worktree-local ownership

## 10. Implementation checklist

- [x] Add a dedicated repo-shared coordination materialization path
- [x] Cut `SqliteCoordinationMaterializedStore` over to that path
- [x] Migrate bounded legacy coordination materialization data
- [x] Update affected diagnostics or status surfaces
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
