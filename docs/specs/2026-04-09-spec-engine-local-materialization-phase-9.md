# Spec Engine Local Materialization Phase 9

Status: draft
Audience: spec-engine, storage, query, MCP, CLI, and docs maintainers
Scope: complete roadmap Phase 9 by implementing the local persistent materialization layer for parsed spec state, including spec records, checklist items, dependency edges, derived local status state, source metadata, and the persistence scaffolding that later phases will use for coverage and sync provenance

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 9:

- persist parsed spec state from Phase 8 into one local rebuildable store
- stop treating parsed spec state as transient in-memory data only
- establish the storage seam that later spec query, coverage, and sync phases will use

This phase is about storage discipline, not user-facing query breadth.

The result should be:

- one local spec materialization seam
- one persistent representation of parsed spec state
- deterministic replace and clear behavior
- derived local status inputs available from storage

This phase does not yet make the full spec engine user-facing.

## 2. Status

Current state:

- [x] Phase 8 parser and identity layer is complete
- [ ] no spec materialized-store seam exists in code
- [ ] parsed specs are not yet persisted locally
- [ ] checklist items are not yet persisted locally
- [ ] dependency edges are not yet persisted locally
- [ ] derived local spec status is not yet persisted locally
- [ ] source metadata is not yet persisted locally
- [ ] coverage and sync provenance storage scaffolding is not yet present

Current slice notes:

- Phase 8 established deterministic parsed spec records with structured diagnostics
- Phase 9 should persist those records without mixing query logic or coordination joins into the
  storage seam

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 9: implement spec local materialization

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/local-materialization.md](../contracts/local-materialization.md)

This spec builds directly on:

- [2026-04-09-spec-engine-source-parser-identity-phase-8.md](2026-04-09-spec-engine-source-parser-identity-phase-8.md)

## 5. Scope

This phase includes:

- one local spec materialized-store seam
- persistent spec records
- persistent checklist-item records
- persistent dependency-edge records
- persistent source metadata
- persistent derived local status inputs and outputs
- clear and replace lifecycle for rebuilding spec state from repo files
- storage scaffolding for later coverage and sync provenance records

This phase does not include:

- rich CLI or MCP spec query surfaces
- full dependency-graph query APIs
- coverage computation against coordination
- sync provenance population from explicit spec-to-coordination actions
- authoritative coordination integration

## 6. Non-goals

This phase should not:

- make spec state authoritative coordination truth
- embed query logic directly into storage helpers
- infer coordination coverage from local spec state alone
- block on a final service-hosted story for specs before local storage exists
- couple spec storage to one hardcoded docs path

## 7. Design

### 7.1 Store-ownership rule

Phase 9 should introduce one explicit local spec materialized-store seam.

Initial target:

- spec materialization is local and branch-sensitive
- v1 ownership should remain worktree-local
- storage should live behind a named store boundary rather than ad hoc SQLite calls

This differs from coordination materialization deliberately:

- coordination materialization is service-owned
- spec materialization remains local because spec state is derived from the checked-out branch and
  is not shared coordination truth

### 7.2 Replace-and-clear rule

The store must support deterministic rebuild behavior.

At minimum:

- replace materialized spec state for one repo/worktree from one parsed batch
- clear local materialized spec state
- track whether the current materialization is empty, partial, or current with respect to the last
  parsed batch

Phase 9 should prefer whole-batch replacement over incremental mutation.

### 7.3 Persistent record families

The materialized store should persist at least:

- spec records
- checklist-item records
- dependency-edge records
- source metadata
- derived local status records

It should also establish storage families for:

- coverage records
- sync provenance records

Those two may remain empty until later phases populate them, but the storage seam should not need a
second redesign just to add them.

### 7.4 Derived-status rule

Phase 9 should persist one local derived status view per spec.

The derived view should at least separate:

- declared status
- checklist posture
- dependency posture
- overall local status

The mapping may stay simple in this phase, but it must be deterministic and documented in code.

### 7.5 Query-boundary rule

Phase 9 may expose minimal read helpers needed for verification and rebuilds, but it must not
become the final query API.

The clean split is:

- Phase 9: storage and rebuild seam
- Phase 10: deterministic query engine

### 7.6 Source-metadata rule

Materialized spec state must retain the Phase 8 source metadata needed for later drift and sync
work:

- repo-relative source path
- deterministic content digest
- nullable git revision

That metadata must survive store reload and rebuild.

## 8. Implementation slices

### Slice 1: Spec materialized-store seam and schema

- add a named local spec materialized-store module
- define the persistent record families and schema
- expose clear and replace entry points

Exit criteria:

- one store seam exists for local spec materialization

### Slice 2: Parsed-batch replacement and source metadata persistence

- persist parsed spec records, checklist items, dependency edges, and source metadata
- replace previous local spec state from one parsed batch
- preserve deterministic ordering and stable identifiers

Exit criteria:

- parsed Phase 8 output can be materialized and reloaded deterministically

### Slice 3: Derived local status persistence

- compute and persist one local derived status view per spec
- persist checklist and dependency posture inputs needed for later querying

Exit criteria:

- local status state is available from storage without reparsing raw markdown

### Slice 4: Coverage and sync-provenance scaffolding

- add empty or placeholder storage families for coverage and sync provenance
- ensure clear and replace lifecycle handles them coherently

Exit criteria:

- later phases can populate coverage and sync provenance without redesigning the store boundary

## 9. Validation

Minimum validation for this phase:

- targeted tests in the crate that owns spec materialization
- downstream tests only if a changed public API is consumed elsewhere immediately
- `git diff --check`

Important regression checks for this phase:

- replacing a parsed batch yields deterministic stored state
- clearing removes all local spec materialization cleanly
- source metadata survives store reload
- duplicate or malformed parser outputs do not corrupt persisted state
- derived local status is recomputed from persisted inputs deterministically

## 10. Completion criteria

Phase 9 is complete only when:

- parsed spec state is persisted through one named local materialized-store seam
- spec records, checklist items, dependency edges, source metadata, and derived local status are
  reloadable from storage
- clear and replace lifecycle is deterministic
- later coverage and sync-provenance phases have a stable storage boundary to build on

## 11. Implementation checklist

- [ ] Add a local spec materialized-store seam
- [ ] Add persistent spec-record storage
- [ ] Add persistent checklist-item storage
- [ ] Add persistent dependency-edge storage
- [ ] Add persistent source metadata storage
- [ ] Add derived local status persistence
- [ ] Add coverage storage scaffolding
- [ ] Add sync-provenance storage scaffolding
- [ ] Validate changed crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land

## 12. Current implementation status

This phase should leave PRISM with one reliable answer to:

- what parsed spec state is currently materialized locally?
- what stable checklist and dependency data is already persisted?
- what local derived spec status exists without reparsing raw markdown?

Phase 10 can then build the query surface on top of a real store instead of inventing a second
parallel cache.
