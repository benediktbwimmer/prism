# PRISM Coordination Authority Store Migration Spec

Status: companion implementation spec  
Audience: PRISM coordination, runtime, MCP, CLI, and storage maintainers  
Scope: complete migration of coordination-only authority access to the new `CoordinationAuthorityStore` interface

---

## 1. Goal

Migrate the current coordination-only implementation so that **all authoritative coordination
access, present and historic, flows through one backend-neutral `CoordinationAuthorityStore`
interface**.

This spec is grounded in the current repo snapshot after the v2 coordination graph rewrite and the
legacy graph purge. It intentionally ignores cognition and knowledge surfaces except where they leak
into coordination-only code paths.

The migration is complete when:

- no product code outside the authority backend implementation calls shared-ref helpers directly
- no product code outside the authority backend implementation reconstructs authoritative
  coordination history directly from `.git`
- the session, watch, MCP, CLI, and docs-export layers read/write coordination state only through
  `CoordinationAuthorityStore`
- the Git shared-ref implementation is the first backend behind that interface
- the interface remains capable of hosting a later PostgreSQL authority backend
- the rest of PRISM no longer depends on Git-specific concepts like summary refs, shard refs,
  manifest digests, or atomic push mechanics

---

## 2. Current problems this migration must solve

The current implementation still spreads authority responsibilities across several modules.

### 2.1 Shared-ref semantics are not encapsulated

Git shared-ref operations currently leak through:

- `shared_coordination_ref.rs`
- `published_plans.rs`
- `coordination_persistence.rs`
- `watch.rs`
- `coordination_startup_checkpoint.rs`
- MCP and CLI runtime status surfaces

### 2.2 Current-state reads and historical reads are split incorrectly

Current-state reads mostly flow through session methods, but history/diagnostics/runtime descriptor
reads still depend on shared-ref-specific helpers.

That means the app does not yet have one true authority seam.

### 2.3 Mutation persistence and local materialization are still too entangled

`coordination_persistence.rs` currently mixes:

- local event-journal persistence
- authoritative shared-ref publication
- startup checkpoint writes
- read-model writes
- compaction
- derived publication

This is too much responsibility in one place, and it makes backend substitution harder.

---

## 3. End-state architecture

### 3.1 New module boundary

Add a new module tree in `crates/prism-core/src/coordination_authority_store/`:

```text
coordination_authority_store/
  mod.rs
  types.rs
  traits.rs
  git_shared_refs.rs
  history.rs
  diagnostics.rs
```

Minimum responsibilities:

- define the backend-neutral authority interface and types
- implement the Git shared-ref backend there
- expose no Git-specific details above this layer

### 3.2 Existing modules after migration

#### Backend-private or backend-owned

These should become implementation details of the Git backend:

- `shared_coordination_ref.rs`
- `shared_coordination_schema.rs`
- any shared-ref-specific history reconstruction helpers
- any shard-manifest verification helpers

#### Local materialization only

These remain local-runtime concerns and should consume authority results instead of performing
authority reads/writes directly:

- `coordination_reads.rs`
- `coordination_startup_checkpoint.rs`
- `checkpoint_materializer.rs`
- local coordination read-model persistence in SQLite

#### Coordination kernel / session surface

These should call `CoordinationAuthorityStore` only:

- `session.rs`
- `watch.rs`
- `indexer.rs`
- `published_plans.rs`
- `prism_doc/repo_state.rs`
- runtime descriptor/detection surfaces in MCP and CLI

---

## 4. New interface responsibilities

The new `CoordinationAuthorityStore` must own these operations.

### 4.1 Current state

- eventual current-state read
- strong current-state read
- runtime descriptor query
- authority metadata / startup authority query

### 4.2 Transactions

- apply transactional coordination mutation intent
- publish or clear runtime descriptor transactionally
- return post-commit authority metadata and committed snapshot information

### 4.3 Retained history

- historical snapshot query
- object timeline query
- transaction/provenance query
- retention / archive / compaction visibility

### 4.4 Diagnostics

- backend-neutral coordination authority diagnostics
- backend-specific details nested inside a backend detail field

---

## 5. Concrete migration inventory

The following call sites and modules must change.

## 5.1 Core authority and read-path migration

### A. `crates/prism-core/src/session.rs`

This is the most important consumer and must become the primary client of the new interface.

#### Current call sites to replace

- `1427`: `load_repo_protected_plan_state(&self.root, &mut *store)?`
- `1961`: `read_coordination_snapshot_with_consistency(...)`
- `1972`: `read_coordination_snapshot_v2_with_consistency(...)`
- `2054`: `read_coordination_plan_state_with_consistency(...)`
- `2147`: `refresh_coordination_materialization_for_strong_read(...)`
- `2223`: `persist_current_coordination(...)`
- `2320`: `mutate_coordination_with_session_guarded_with_options(...)`

#### Required change

`WorkspaceSession` should hold an `Arc<dyn CoordinationAuthorityStore>` and use it for:

- eventual and strong reads
- obtaining the current authoritative stamp/version
- committing mutations
- publishing current full coordination state when needed
- reading runtime descriptors and startup authority metadata

#### Expected refactor

Replace the current pattern:

- session → local store helpers → shared-ref helpers

with:

- session → `CoordinationAuthorityStore`
- session → local materialization helpers only for checkpoint/read-model persistence

#### Specific notes

- `read_coordination_with_consistency(...)` should delegate to the authority store instead of
  calling local store functions and then manually triggering a strong-refresh path.
- `refresh_coordination_materialization_for_strong_read(...)` should become a local materialization
  update driven by an authority-store strong read result, not by direct watch/sync helper calls.
- `persist_current_coordination(...)` and `mutate_coordination_with_session_guarded_with_options(...)`
  should stop calling `CoordinationPersistenceBackend` directly for authoritative publication.

### B. `crates/prism-core/src/coordination_reads.rs`

#### Current role

This module is effectively the current-state eventual read adapter over the local checkpoint store.

#### Required change

Keep it as a **local materialization** helper only.
It should no longer pretend to be an authority boundary.

It should either:

- become an internal helper under a local materialization module, or
- stay where it is but only serve local eventual reads from checkpoint/read-model state

Strong reads must move entirely to the authority store.

### C. `crates/prism-core/src/indexer.rs`

#### Current call site to replace

- `491`: `load_repo_protected_plan_state(&root, &mut store)?`

#### Required change

Indexer/bootstrap should not load coordination plan state through protected-state / store helpers.
It should call the authority store for the required coordination seed view.

In coordination-only mode, this is the place where the bootstrap path should start feeling truly
backend-neutral.

### D. `crates/prism-core/src/protected_state/runtime_sync.rs`

#### Current call site to replace

- `155`: `load_repo_protected_plan_state(...)`

#### Required change

This helper should stop being the back door for loading eventual coordination plan state.
Either:

- delete it and move callers to the authority store, or
- rename/re-scope it so it is clearly a local materialization loader only

It should not remain the apparent source of coordination truth.

---

## 5.2 Shared-ref backend extraction

### E. `crates/prism-core/src/shared_coordination_ref.rs`

#### Current external API surface to move behind the backend

- `924`: `sync_shared_coordination_ref_state(...)`
- `984`: `sync_live_runtime_descriptor(...)`
- `1453`: `load_shared_coordination_ref_state_authoritative(...)`
- `1588`: `shared_coordination_startup_authority(...)`
- `1975`: `shared_coordination_ref_diagnostics(...)`
- `2134`: `shared_coordination_ref_status_summary(...)`
- internal history/compaction helpers such as `ref_history_depth(...)`

#### Required change

This file should become backend-private or be split into Git-backend submodules under
`coordination_authority_store/git_shared_refs.rs`.

No production module outside the Git backend should call these functions directly after migration.

### F. `crates/prism-core/src/published_plans.rs`

#### Current call sites to replace

- `17`: direct imports from `shared_coordination_ref`
- `93`: `sync_repo_published_plans(...)`
- `114`: `load_authoritative_coordination_snapshot_v2(root)?`
- `185`: `load_authoritative_coordination_snapshot(...)`
- `191`: `load_authoritative_coordination_snapshot_v2(...)`
- `197`: `load_authoritative_coordination_plan_state(...)`

#### Required change

This module should stop loading authority directly from shared refs.
Instead it should consume a `CoordinationAuthorityStore` read result or a current committed
materialization provided by the session.

Two concrete rules:

- **authoritative loads** of current coordination state move to `CoordinationAuthorityStore`
- **derived branch publication** remains here, but it must consume already-read authority results

This file should become a consumer of authority, not part of the authority backend.

### G. `crates/prism-core/src/coordination_startup_checkpoint.rs`

#### Current call sites to replace

- `62`: `save_coordination_startup_checkpoint(...)`
- `141`: `resolve_coordination_startup_checkpoint_authority(...)`

#### Required change

This module should remain local-checkpoint-focused, but authority metadata should come from the
backend-neutral authority stamp / startup authority query.

The startup checkpoint must stop depending on `shared_coordination_startup_authority(...)`
directly and instead resolve authority provenance through the backend-neutral startup-checkpoint
authority helper path.

### H. `crates/prism-core/src/watch.rs`

#### Current call sites to replace

- `1137`: `load_repo_protected_plan_state(root, &mut *local_store)?`
- `1203`: `sync_shared_coordination_ref_watch_update(...)`
- `1236`: `apply_shared_coordination_ref_watch_state(...)`

#### Required change

The watch path must stop knowing about shared-ref polling directly.
It should depend on one authority-store refresh or poll surface.

Suggested target shape:

- watch asks the authority store for current strong authority metadata or a strong summary read
- the store returns either the same authority stamp or a refreshed committed state bundle
- watch applies local materialization and published-generation updates from that bundle

The watch layer should not call shared-ref polling helpers or interpret shared-ref live-sync enums
itself.
If a dedicated refresh or poll helper is added later, it should still be expressed as an
authority-store surface rather than a second hidden protocol.

---

## 5.3 Mutation-path migration

### I. `crates/prism-core/src/coordination_persistence.rs`

This file is the largest architectural cleanup target.

#### Current functions to replace or dissolve

- `104`: `sync_authoritative_shared_coordination_ref_observed(...)`
- `259`: `persist_coordination_authoritative_state_for_root(...)`
- `304-322`: direct authoritative loader wrappers
- `364`: `persist_coordination_authoritative_mutation_state_for_root_with_session_observed(...)`
- `339-359`: inline authoritative persist + read-model writes
- `560-618`: local event compaction coupled to authority persistence

#### Required change

Split this file into two responsibilities:

1. **authority backend implementation**
   - transactional authority commit and authoritative current/history reads
2. **local materialization helper**
   - read-model persistence
   - startup checkpoint persistence
   - local event compaction where still needed

`CoordinationPersistenceBackend` should not remain the apparent authority API.
The new `CoordinationAuthorityStore` should replace it.

#### Specific design note

The future transactional coordination mutation action should commit through this new authority
store, not through a store trait that still mixes SQLite event persistence and shared-ref writes.

### J. `crates/prism-core/src/checkpoint_materializer.rs`

#### Current call site to review

- `462`: `persist_coordination_materialization(...)`

#### Required change

This function can remain, but it must consume a committed authority result or materialization
bundle from the new interface boundary.

It must not perform or imply authority publication itself.

The materializer should be explicitly downstream of:

- an already-committed authority transaction result
- or an already-verified authority read/refresh result

### K. `crates/prism-core/src/session.rs` mutation entrypoints

These are already listed above, but deserve one extra rule:

- all coordination mutation entrypoints should eventually commit through one backend-neutral
  transaction request, not through ad hoc persistence helpers plus local snapshot surgery

This is where the upcoming transactional coordination mutation action should land.

---

## 5.4 Runtime descriptor and diagnostics migration

### L. `crates/prism-mcp/src/lib.rs`

#### Current call sites to replace

- `524`: runtime descriptor publication helper call
- `855`: runtime descriptor publication helper call

#### Required change

Workspace server / host startup should publish runtime descriptors through the authority store,
not via a direct shared-ref helper. The product-facing helper should be authority-neutral, for
example `publish_local_runtime_descriptor(...)`.

### M. `crates/prism-cli/src/mcp.rs`

#### Current call sites to replace

- `320`: `shared_coordination_ref_diagnostics(root)?`
- `374`: runtime descriptor publication helper call
- `381`: runtime descriptor publication helper call

#### Required change

CLI diagnostics and public URL update flows should call:

- `authority_store.diagnostics(...)`
- `authority_store.publish_runtime_descriptor(...)`

No CLI command should need to know that the current backend is Git shared refs.

### N. `crates/prism-mcp/src/runtime_views.rs`

#### Current call sites to replace

- `401`: `shared_coordination_ref_diagnostics(inputs.root)?`
- `409`: `runtime_shared_coordination_ref_view(...)`
- `516-587`: `runtime_freshness_from_inputs(...)` depends on direct workspace revision and local
  read-model revision helpers

#### Required change

Runtime status should source authority diagnostics from the authority store.

Two sub-rules:

- the top-level shared-coordination diagnostic object becomes a backend-neutral authority-diagnostic
  view
- Git-specific fields such as manifest digests or compaction history stay nested in backend detail
  fields
- non-Git default backends must not surface the shared-ref diagnostic object as if it were a
  backend-neutral authority status view

`runtime_freshness_from_inputs(...)` may continue to compare local materialization lag, but its
**authoritative** side must come from the authority store’s current authority metadata rather than
from direct revision helpers alone.

### O. `crates/prism-mcp/src/peer_runtime_router.rs`

#### Current call site to replace

- `423-453`: `resolve_runtime_descriptor(...)` calls `shared_coordination_ref_diagnostics(root)?`
  and extracts runtime descriptors from diagnostics

#### Required change

Peer runtime routing should query runtime descriptors through the authority store directly.
It should not depend on a diagnostics object as an accidental descriptor registry.

That means splitting two concerns cleanly:

- diagnostics
- runtime descriptor discovery

---

## 5.5 Docs/export and repo-state readers

### P. `crates/prism-core/src/prism_doc/repo_state.rs`

#### Current call site to replace

- `46`: `load_authoritative_coordination_plan_state(root)?`

#### Required change

Repo-state export should use the authority store to load current authoritative coordination state.
This keeps docs/export paths backend-neutral too.

---

## 5.6 MCP mutation surface migration

### Q. `crates/prism-mcp/src/host_mutations.rs`

This file is not yet using the future transactional coordination mutation action, but it is the
main call site family that must eventually rely on it.

#### Current mutation entrypoints that must converge on the new transaction surface

- `2695`, `2783`, `3875`, `4280`, `4297`, `4374`, `4464`, `4554`: calls to
  `workspace.mutate_coordination_with_session_wait_observed(...)` and related variants
- `4645`: direct `prism.bootstrap_native_plan(...)`
- multiple direct task-authoritative updates via `prism.update_native_task_authoritative_only(...)`
  at lines such as `383`, `706`, `780`, `3912`, `4032`, `4087`, `4142`, `4207`, `5020`

#### Required change

After the transactional coordination mutation action lands, these entrypoints should become
clients of that action and therefore, transitively, clients of `CoordinationAuthorityStore`.

This spec does **not** require rewriting the entire mutation surface immediately, but it does
require that the final cutover leaves no coordination authority writes outside the new transaction
surface.

---

## 6. New code to add

At minimum, the migration should add the following new code.

### 6.1 Interface and types

- `crates/prism-core/src/coordination_authority_store/mod.rs`
- `crates/prism-core/src/coordination_authority_store/types.rs`
- `crates/prism-core/src/coordination_authority_store/traits.rs`

### 6.2 Git backend adapter

- `crates/prism-core/src/coordination_authority_store/git_shared_refs.rs`

This backend should wrap the existing shared-ref implementation and become the **only** place that
knows about:

- summary refs
- shard refs
- manifest verification
- `git push --atomic`
- shared-ref history traversal
- compaction / archive-boundary metadata

### 6.3 Optional mock/in-memory backend for tests

A simple in-memory backend will make it much easier to unit-test session, query, and MCP surfaces
without going through the full shared-ref protocol.

Suggested path:

- `crates/prism-core/src/coordination_authority_store/memory.rs`

---

## 7. Deletions and demotions after cutover

After the migration is complete, these responsibilities should no longer exist outside the backend.

### 7.1 Delete or demote public surfaces

- `shared_coordination_ref_diagnostics(...)` as a top-level public API
  demote it to an explicitly Git-specific helper such as
  `git_shared_coordination_ref_diagnostics(...)`
- `sync_live_runtime_descriptor(...)` as the primary product-facing publication helper
- direct authoritative loader helpers in `published_plans.rs`
- direct shared-ref polling helpers in `watch.rs`

### 7.2 Demote backend-specific helpers to backend-private

- shared-ref history-depth helpers
- shard summary reconstruction helpers
- startup-authority helpers tied directly to shared-ref manifests

### 7.3 Reduce `coordination_persistence.rs`

This file should no longer be the place where backend-neutral application code learns how
coordination authority is persisted.

---

## 8. Migration order

### Phase 1: define the authority interface and types

- add `CoordinationAuthorityStore`
- add backend-neutral read, transaction, history, descriptor, and diagnostics types
- add a Git backend adapter that delegates to the current shared-ref implementation
- harden backend taxonomy and backend-details shaping so SQLite and Postgres fit the public seam
  before the DB family lands

### Phase 2: cut over current-state reads in `session.rs`

- move eventual and strong current-state reads to the interface
- remove direct authoritative loaders from session logic
- make strong-read refresh use authority-store refresh results

### Phase 3: cut over watch/live-sync

- remove direct `poll_shared_coordination_ref_live_sync(...)` usage from `watch.rs`
- let the watch path consume authority-store refresh/poll results instead

### Phase 4: cut over diagnostics and runtime descriptors

- migrate MCP runtime status, peer routing, and CLI status/public-url commands
- delete direct external use of `shared_coordination_ref_diagnostics(...)` and
  the legacy shared-ref-shaped runtime descriptor publication helper

### Phase 5: cut over mutation persistence

- make `session.rs` coordination mutations commit through `CoordinationAuthorityStore`
- reduce `coordination_persistence.rs` to local materialization support or dissolve it entirely

At the end of this phase, existing mutation entrypoints may still exist, but their authoritative
commit step must already route through `CoordinationAuthorityStore`.

### Phase 6: cut over history access

- move retained authoritative history queries behind the authority store
- eliminate direct `.git` shared-ref history inspection from product-facing code

### Phase 7: land the transactional coordination mutation action on top of the new seam

- MCP mutation entrypoints become clients of the new transaction action
- the transaction action becomes the single coordination mutation surface above the authority store

This phase is mutation-surface convergence, not the first moment when writes become authority-store
backed.

---

## 9. Validation matrix for the migration

The migration should be considered complete only when the following are true.

### 9.1 Architecture validation

- no production module outside `coordination_authority_store/*` imports `shared_coordination_ref`
  helpers directly
- no production module outside the backend reconstructs shared-ref history directly from `.git`
- all coordination authority reads and writes flow through `CoordinationAuthorityStore`

### 9.2 Behavioral validation

- eventual reads still return local materialized state only
- strong reads still refresh against authority before answering
- mutations still respect transactionality and do not update local read models before commit
- runtime descriptor publication and discovery still work
- diagnostics still surface meaningful backend-specific detail
- retained history remains queryable through one backend-neutral contract

### 9.3 Test strategy

Required test updates:

- `shared_coordination_ref.rs` tests become Git-backend implementation tests
- `session.rs` tests should gain an authority-store-backed fixture path
- MCP runtime status / peer routing / CLI tests should run against the abstract interface
- a lightweight in-memory authority backend should cover non-Git logic where possible

---

## 10. Final rule

The migration is complete only when this statement becomes true in practice:

- **all coordination authority access, current and historical, goes through
  `CoordinationAuthorityStore`; all Git shared-ref details live only inside the Git backend
  implementation**

That is the point at which PRISM coordination becomes cleanly backend-pluggable while keeping the
current Git shared-ref design as its first-class default implementation.
