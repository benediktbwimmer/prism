# SQL-Only Coordination Authority Cutover

Status: implemented
Audience: coordination, storage, runtime, query, MCP, CLI, and service maintainers
Scope: remove the shared-ref coordination authority backend entirely, tighten the authority abstraction around SQL-backed implementations only, and migrate all product call sites and tests to the new assumption

---

## 1. Summary

PRISM should stop pretending that one authority abstraction must cleanly span two fundamentally
different persistence families.

That experiment is over.

The shared-ref coordination authority backend no longer matches where PRISM is going:

- it is not the default mode
- it distorts the authority abstraction
- it preserves snapshot-shaped and publication-shaped semantics that do not fit the target system
- it makes Postgres harder instead of easier

This spec hard-cuts PRISM to a SQL-only coordination authority model.

After this change:

- `git_shared_refs` is no longer a supported coordination authority backend
- the authority abstraction is shaped only around DB-backed implementations
- SQLite remains the active implementation
- Postgres is the next backend to add behind the same SQL-oriented contract

This is allowed to be a breaking change inside the repo while it lands.

There is no deprecation phase and no compatibility layer that keeps `git_shared_refs` viable as an
authority backend.

## 2. Status

Checklist:

- [x] replace the current Phase 2 target with this hard-cut spec
- [x] remove `git_shared_refs` from the supported coordination authority backend taxonomy
- [x] delete the shared-ref coordination authority store implementation
- [x] remove shared-ref backend selection from CLI, MCP, service config, and runtime views
- [x] tighten `CoordinationAuthorityStore` around SQL-backed semantics only
- [x] remove backend-kind branching that exists only to preserve shared-ref compatibility
- [x] preserve an explicit recovery/export snapshot path only where still necessary
- [x] keep SQLite green behind the tightened seam
- [x] leave Postgres as a stub on the same SQL-only contract
- [x] update tests and fixtures to stop assuming shared-ref authority support

Progress note (2026-04-09):

- the previous refactor improved the authority seam but still preserved escape hatches and backend
  branching to keep `git_shared_refs` alive
- this spec replaces that compromise with the actual desired direction: one logical authority model,
  two SQL implementations
- implementation landed by removing the shared-ref authority backend from core runtime taxonomy,
  deleting its store implementation, collapsing the normal authority mutation path to append-only
  semantics, and updating CLI, MCP, and tests to fail clearly on old `git_shared_refs` config

## 3. Problem statement

Three problems remain after the first authority refactor.

### 3.1 Shared-ref authority support still leaks into the abstraction

The current code still knows that `git_shared_refs` exists as a valid authority backend.

That means:

- backend enums still name it
- backend selection code still supports it
- runtime and diagnostics views still describe it
- tests still provision it
- persistence still contains backend-specific branches

This is architectural drag, not useful flexibility.

### 3.2 Snapshot replacement remains too visible because shared refs still need it

A full-state replacement path is still acceptable for:

- recovery
- export/import
- bootstrap helpers

It is not acceptable as something the normal coordination authority flow keeps reaching for just to
support an obsolete backend.

The shared-ref backend is the main reason the current seam still bends around full-state
publication semantics.

### 3.3 The abstraction is still broader than the real target system

The actual target system is:

- SQLite now
- Postgres later
- one authoritative event-first storage model
- typed read projections

If that is true, the abstraction should be optimized for that target instead of remaining
backend-family-neutral in theory.

## 4. Goals

This cutover must make the following true.

### 4.1 PRISM has one supported coordination authority family

That family is SQL-backed authority storage.

Immediately this means SQLite.

Soon this should mean SQLite and Postgres behind one coherent seam.

### 4.2 The authority abstraction is shaped for SQL semantics

The authority contract should assume:

- transactional append semantics
- DB-backed concurrency control
- typed current-state projections
- shaped current-state reads
- explicit history access

It should not assume:

- git ref publication
- ref-head freshness as the main concurrency story
- summary-vs-shard publication families
- snapshot publication as the normal mutation mode

### 4.3 Shared-ref authority code is gone, not dormant

This refactor should remove support directly.

It should not:

- leave a dead backend in enums and switch statements
- preserve a compatibility adapter for later
- keep tests around for a backend we do not intend to revive

### 4.4 Postgres becomes a straightforward backend follow-on

After this cutover, Postgres work should be “implement the SQL backend contract,” not “first unwind
shared-ref assumptions and then implement Postgres.”

## 5. Non-goals

This spec does not attempt to:

- fully implement Postgres now
- design every final projection table in this slice
- implement every future query-shaped read in one pass
- remove the shared coordination ref subsystem entirely where it still serves non-authority roles
- redesign coordination-domain semantics such as task, artifact, or review rules

Important distinction:

- removing shared refs as an authority backend is in scope
- deleting every shared-ref utility unrelated to authority is not required in this slice

## 6. Related docs

Roadmap:

- [../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md](../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md)

Superseded implementation target:

- [2026-04-09-coordination-authority-abstraction-and-storage-refactor.md](./2026-04-09-coordination-authority-abstraction-and-storage-refactor.md)

Existing contracts and implementation context:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-db-coordination-authority-seam-phase-2.md](./2026-04-09-db-coordination-authority-seam-phase-2.md)
- [2026-04-09-sqlite-coordination-authority-phase-3.md](./2026-04-09-sqlite-coordination-authority-phase-3.md)

## 7. Design

### 7.1 Supported backend family after the cutover

After this change, the supported authority backend set is:

- `Sqlite`
- `Postgres`

`GitSharedRefs` is removed from:

- backend-name enums
- backend-kind enums
- backend-details enums
- CLI argument enums
- MCP-facing backend selections and views
- factory parsing and config loading

If old config still names `git_shared_refs`, startup should fail clearly instead of silently
falling back.

### 7.2 Authority write contract

The main authority mutation path should be:

- optimistic base
- session/provenance metadata
- appended coordination events
- projection update mode when needed

The main write path must not accept:

- full `CoordinationSnapshot`
- full `CoordinationSnapshotV2`

The only full-state write path allowed after this cutover is an explicit special-purpose operation,
kept separate from the normal mutation API.

That operation exists only for:

- recovery
- export/import
- targeted bootstrap helpers if truly required

Product mutation flows must not branch by backend kind to decide whether they use append or state
replacement.

### 7.3 Authority read contract

The public authority facade should keep explicit reads such as:

- `read_plan_state(...)`
- `read_snapshot(...)`
- `read_snapshot_v2(...)`
- `read_summary(...)`

But the design intent is now narrower:

- these reads exist for SQL-backed storage only
- snapshot reads are explicit and not the default hot path
- future narrowing toward more query-shaped reads is expected

There is no need to preserve read semantics that exist only to mirror the shared-ref backend.

### 7.4 Persistence-layer simplification

`coordination_persistence.rs` should stop branching on authority backend kind for normal product
mutations.

The default rule should be:

- normal coordination mutation flow uses authority append semantics

If some local helper still needs explicit state replacement, that helper should call the explicit
replacement API for a clearly documented reason.

There should be no “if shared refs then replace, else append” logic left in the main path.

### 7.5 SQLite implementation target

SQLite remains the working backend for this slice.

It may still internally:

- rebuild state from events plus checkpoint
- persist checkpoint material for recovery

But that is now an implementation detail of the SQL authority backend.

The important rule is that product code should no longer know or care about the removed shared-ref
authority family.

### 7.6 Postgres readiness target

Postgres remains unimplemented, but the contract should now assume that Postgres will be another
SQL authority backend with:

- the same append-oriented mutation contract
- the same authority stamp and revision semantics at the contract level
- the same family of current-state reads
- a different concrete DB implementation

The cutover is successful if Postgres can now be added without first undoing compatibility work for
shared refs.

## 8. Implementation slices

### Slice A: Docs and roadmap cutover

Update the roadmap and write this spec so the repo guidance is explicit:

- shared-ref authority support is being removed, not deprecated
- the target family is SQL-only

### Slice B: Backend taxonomy cleanup

Remove `GitSharedRefs` from:

- `CoordinationAuthorityBackendKind`
- `CoordinationAuthorityBackendConfig`
- related backend-name and backend-arg enums
- backend-detail view types when they exist only for authority selection

Adjust parsing, config, and UI surfaces accordingly.

### Slice C: Factory and provider cleanup

Update the authority factory so it:

- no longer imports the shared-ref store
- no longer constructs it
- rejects `git_shared_refs` config values
- defaults cleanly to SQLite or explicit Postgres

### Slice D: Delete shared-ref authority store support

Remove:

- [crates/prism-core/src/coordination_authority_store/git_shared_refs.rs](../../crates/prism-core/src/coordination_authority_store/git_shared_refs.rs)
- re-exports and tests that exist only for that store

Any remaining shared-ref utilities outside the authority store must stop being described or used as
an authority backend.

### Slice E: Tighten the authority seam

After the backend family is SQL-only:

- remove any trait methods, result fields, or helper flows that only exist to preserve shared-ref
  compatibility
- remove backend-kind branches in persistence and session/runtime code that only support the old
  backend
- keep explicit snapshot replacement only where it still has a recovery/export justification

### Slice F: Call-site and test migration

Update:

- `prism-core`
- `prism-mcp`
- `prism-cli`
- affected test helpers and fixtures

So they no longer:

- select `git_shared_refs`
- assert shared-ref authority behavior
- surface shared-ref authority diagnostics as a supported mode

### Slice G: Cleanup of stale docs and wording

Update active docs that still describe shared refs as a current authority option when those docs
are touched by this slice.

At minimum:

- roadmap references
- active spec references
- authority-facing CLI and MCP descriptions

## 9. Validation

Minimum validation for this cutover:

- targeted crate tests for changed crates
- direct downstream tests for `prism-core` public API changes
- full `cargo test` workspace run because backend taxonomy and SQLite authority behavior are
  touched

The validation bar for completion is:

- `cargo test`

Warnings may remain, but there must be no failing tests and no residual build break from removed
shared-ref authority paths.

## 10. Rollout

This is a hard-cut refactor.

Rollout order:

1. update roadmap and spec
2. remove shared-ref authority support from config and factory layers
3. delete the shared-ref authority store and migrate call sites
4. tighten persistence and runtime code so no shared-ref authority branches remain
5. fix the resulting test and fixture fallout
6. run full validation

There is no intermediate support promise for `git_shared_refs`.

If local fixtures or tests still rely on it, they should be updated or deleted in the same change.

## 11. Open questions

Open questions intentionally left for follow-on work, not blockers for this cutover:

- how far to split the SQL authority facade into narrower subtraits before Postgres starts
- which current-state reads should be promoted next from snapshot-level to narrower query-shaped
  reads
- how much checkpoint/export functionality should remain public versus internal once Postgres lands
