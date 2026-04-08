# Coordination Materialized Store Phase 2

Status: in progress
Audience: coordination, runtime, query, MCP, CLI, UI, storage, and authority-backend maintainers
Scope: complete Phase 2 implementation of the `CoordinationMaterializedStore` seam so local coordination materialization routes through one explicit non-authoritative storage boundary

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 2:

- implement the `CoordinationMaterializedStore` fully enough that all local coordination
  materialization can go through it

The goal of this phase is not to finish the full coordination cutover yet.
The goal is to finish the local materialization seam itself so later phases can migrate query,
runtime, and product call sites onto a real non-authoritative storage interface instead of more
ad hoc SQLite and checkpoint helpers.

This phase should leave PRISM with:

- one concrete materialized-store interface
- one initial SQLite-backed implementation behind it
- explicit eventual-read, checkpoint, metadata, and invalidation families
- no new product code added against direct coordination SQLite or checkpoint helpers

The materialized store is local, persistent, disposable, and non-authoritative.
It exists to serve eventual reads honestly and to persist restart continuity, not to redefine
coordination truth.

## 2. Status

Current state:

- [x] `CoordinationMaterializedStore` trait and type family finalized in code
- [x] SQLite-backed coordination materialized-store implementation extracted behind the seam
- [x] eventual coordination snapshot and plan-state read families implemented through the new seam
- [ ] startup checkpoint persistence and restore implemented through the new seam
- [x] materialization metadata, revision, and authority-key access implemented through the new seam
- [ ] invalidation, replace, and clear families implemented through the new seam
- [ ] direct product-facing coordination SQLite and checkpoint calls removed or redirected

Current slice notes:

- coordination eventual reads currently live in `crates/prism-core/src/coordination_reads.rs`
- startup checkpoint load and save currently live in
  `crates/prism-core/src/coordination_startup_checkpoint.rs`
- coordination persistence, session, watch, and checkpoint materialization still call those local
  storage paths directly
- coordination SQLite access is still spread across product-facing code and should become the
  materialized-store implementation concern in this phase
- the backend-neutral materialized-store module now exists in
  `crates/prism-core/src/coordination_materialized_store/`
- the stable type family and SQLite-backed store shell compile and are exported from `prism-core`
- the seam currently exposes eventual snapshot, plan-state, read-model, queue-model, startup
  checkpoint, and metadata read families while downstream call-site cutover remains for later
  slices
- the `coordination_reads` helper layer and protected-state eventual plan-state load now delegate
  through the materialized-store seam instead of reading checkpoints directly

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 2: Implement the Coordination Materialized Store

## 4. Related contracts

This spec depends on:

- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/local-materialization.md](../contracts/local-materialization.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/signing-and-verification.md](../contracts/signing-and-verification.md)
- [../contracts/provenance.md](../contracts/provenance.md)

## 5. Scope

This phase includes:

- defining the materialized-store trait and stable type families in code
- implementing the current SQLite-backed eventual coordination storage behind that seam
- routing eventual coordination snapshot, canonical snapshot, and hydrated plan-state reads through
  the seam
- routing coordination startup checkpoint persistence and restore through the seam
- exposing materialization metadata such as authority stamp, coordination revision, schema version,
  and materialized-at through the seam
- extracting or demoting direct coordination SQLite and checkpoint helpers so the seam becomes the
  only product-facing materialization boundary

This phase does not include:

- centralizing coordination evaluation into the query engine
- redesigning eventual versus strong read semantics
- redesigning startup checkpoint contents
- full broad cutover of every product surface in roadmap Phase 5
- the spec engine
- the PRISM service

## 6. Non-goals

This phase should not:

- collapse authority and materialization into one abstraction
- accept speculative local intent as materialized coordination truth
- redesign the coordination domain model again
- leave new product-facing direct SQLite reads or writes in place “for now”
- solve spec-state materialization in the same seam

## 7. Design

### 7.1 Target module boundary

Add or complete the materialized-store module tree under:

```text
crates/prism-core/src/coordination_materialized_store/
  mod.rs
  traits.rs
  types.rs
  sqlite.rs
  checkpoints.rs
```

The exact file split may vary, but the architectural rule should remain:

- backend-neutral local materialization API at the top
- SQLite-backed implementation beneath it
- no raw store-table knowledge leaking upward into product-facing modules

### 7.2 Public semantic families

The code should expose one seam for:

- eventual coordination snapshot reads
- eventual canonical snapshot and hydrated plan-state reads
- startup checkpoint persistence and restore
- materialization metadata and revision reads
- invalidate, replace, and clear operations for local coordination materialization

The seam must preserve:

- non-authoritative local semantics
- explicit authority-stamp and revision tracking
- rebuildability and disposability
- enough metadata for freshness reporting to remain honest

### 7.3 SQLite-backed implementation

The initial implementation is the current local SQLite and checkpoint behavior.

That implementation should remain first-class, but after this phase it should be:

- invoked through `CoordinationMaterializedStore`
- treated as one local storage implementation
- no longer treated as a product-facing utility surface

### 7.4 Product-facing boundary rule

After this phase:

- product code should depend on `CoordinationMaterializedStore` for eventual coordination storage
- only the materialized-store implementation code should depend directly on coordination SQLite and
  checkpoint mechanics

This rule applies to:

- session
- watch
- checkpoint materializer
- coordination persistence
- protected-state runtime sync
- any future CLI, MCP, or UI eventual-read path that serves local coordination materialization

### 7.5 Relationship to authority and freshness

The materialized-store seam must preserve the semantics already defined in contracts:

- authority still determines truth
- materialization determines speed and restart continuity
- eventual answers may be served from previously materialized local authority-backed state
- strong reads may refresh or bypass local materialization without changing what the materialized
  store is allowed to claim

This does not mean the materialized store owns coordination evaluation.
It means it owns the local persisted storage and metadata needed for eventual reads and restart
continuity to remain honest.

### 7.6 Materialized-store adoption rule

This phase may use compatibility wrappers or adapters internally to preserve behavior while the
seam is being introduced.

However:

- product-facing modules must not gain new direct dependencies on coordination SQLite or startup
  checkpoint mechanics
- any temporary compatibility wrapper must live below the materialized-store seam
- any newly discovered product-facing coordination materialization access path encountered during
  this phase must either be migrated here or be documented explicitly as deferred with a reason

This rule exists to stop new local-storage bypasses from appearing while the seam is being created.

## 8. Implementation slices

### Slice 1: Trait and type family

- finalize the materialized-store trait
- finalize eventual-read, checkpoint, metadata, and invalidation request and response types
- finalize local materialization metadata envelopes

Exit criteria:

- code outside the backend can compile against the trait without direct SQLite or checkpoint
  imports

### Slice 2: SQLite backend extraction

- move or wrap local coordination SQLite and checkpoint logic behind the new implementation
- demote ad hoc eventual-read and checkpoint helpers from product-facing modules
- preserve current SQLite and checkpoint behavior behind the seam

Exit criteria:

- SQLite-backed coordination materialization is reachable through the backend implementation only

### Slice 3: Eventual-read cutover

- route eventual coordination snapshot, canonical snapshot, and hydrated plan-state reads through
  the materialized-store seam
- expose materialization metadata and freshness inputs consistently

Exit criteria:

- product-facing eventual coordination reads no longer call checkpoint or SQLite helpers directly

### Slice 4: Checkpoint and replace-path cutover

- route startup checkpoint persistence and restore through the materialized-store seam
- route authoritative materialization replacement or advancement through shared types
- expose explicit invalidation and clear families

Exit criteria:

- startup checkpoint persistence and restore are reachable through the seam
- local coordination materialization advances only from committed authority or explicitly allowed
  local operational inputs

### Slice 5: Metadata and diagnostics cleanup

- route materialization metadata and revision access through the seam
- route any remaining coordination-local checkpoint or metadata helpers through the seam
- make the local boundary explicit enough for Phase 3 query work to build on

Exit criteria:

- eventual read, checkpoint, and materialization metadata families are all reachable through the
  materialized-store interface

## 9. Migration targets

This phase should prioritize the high-value call sites in this order:

1. `crates/prism-core/src/coordination_reads.rs`
2. `crates/prism-core/src/coordination_startup_checkpoint.rs`
3. `crates/prism-core/src/coordination_persistence.rs`
4. `crates/prism-core/src/session.rs`
5. `crates/prism-core/src/watch.rs`
6. `crates/prism-core/src/checkpoint_materializer.rs`
7. `crates/prism-core/src/protected_state/runtime_sync.rs`
8. any remaining direct coordination checkpoint or SQLite materialization helpers in
   product-facing code

This phase does not need to finish the full broad runtime and product cutover, but it must make
those migrations possible without further seam redesign.

## 10. Validation

Minimum validation for this phase:

- targeted tests for `prism-core`
- direct downstream tests for crates whose eventual coordination read surface depends on
  `prism-core`
- focused tests for:
  - eventual coordination snapshot reads
  - hydrated plan-state eventual reads
  - startup checkpoint persistence and restore
  - materialization metadata and revision behavior
  - checkpoint invalidation or rebuild behavior where touched

Representative commands should likely include at least:

- `cargo test -p prism-core --no-run`
- targeted `prism-core` tests covering eventual coordination reads and startup checkpoints
- targeted downstream `prism-mcp` or `prism-cli` tests when public eventual coordination read
  behavior changes

This phase does not require a full workspace suite by default.

## 11. Completion criteria

This phase is complete when:

- `CoordinationMaterializedStore` is a real code seam, not only a docs concept
- SQLite-backed eventual coordination state is fully reachable through that seam
- eventual coordination reads, startup checkpoint persistence and restore, and materialization
  metadata all route through it
- the rest of the app no longer needs direct checkpoint or coordination SQLite helpers to perform
  local coordination materialization access

## 12. Follow-on phases

This phase intentionally prepares:

- Phase 3: Coordination Query Engine
- Phase 4: Transactional Coordination Mutation Protocol
- Phase 5: full coordination cutover

The expected outcome is a stable local materialization foundation that later phases can depend on
without another interface rewrite.
