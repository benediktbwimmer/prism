# Spec Query Engine Phase 10

Status: completed
Audience: spec-engine, query, MCP, CLI, UI, and storage maintainers
Scope: complete roadmap Phase 10 by implementing the dedicated deterministic `SpecQueryEngine` seam in `prism-spec`, backed by the local spec materialized store and exposing stable query families for spec records, checklist items, dependencies, derived status, and the currently empty coverage and sync-provenance families

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 10:

- add one explicit `SpecQueryEngine` seam
- stop treating spec reads as ad hoc store access
- make deterministic spec queries available for later CLI, MCP, UI, and join-layer work

This phase is about query ownership and stable machine-facing results.

It should not wait for:

- explicit spec-to-coordination linking
- populated sync provenance
- computed `SpecCoverageView`
- user-facing CLI or MCP endpoints

Those arrive in later phases.
Phase 10 should still expose query families for coverage and sync provenance, but they may return
empty deterministic results until later phases populate them.

## 2. Status

Current state:

- [x] Phase 8 parser and identity layer is complete
- [x] Phase 9 local spec materialization layer is complete
- [x] the pre-Phase-10 crate extraction is complete
- [x] dedicated `SpecQueryEngine` seam exists
- [x] spec reads now flow through one query-owner boundary
- [x] coverage and sync-provenance query families are surfaced from the engine

Current slice notes:

- the new `prism-spec` crate now owns source discovery, parsing, and local materialization
- this phase should keep spec query logic in that crate rather than pushing it into `prism-core`,
  `prism-query`, or handler code
- the goal here is deterministic read semantics, not user-facing command or MCP fanout yet
- Phase 10 landed with a dedicated `SpecQueryEngine` trait, a materialized-store-backed
  implementation, explicit not-found lookups, spec-scoped checklist/dependency views, and stable
  empty read families for coverage and sync provenance

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 10: implement `SpecQueryEngine` fully

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)

This spec builds on:

- [2026-04-09-spec-engine-source-parser-identity-phase-8.md](2026-04-09-spec-engine-source-parser-identity-phase-8.md)
- [2026-04-09-spec-engine-local-materialization-phase-9.md](2026-04-09-spec-engine-local-materialization-phase-9.md)
- [2026-04-09-spec-engine-crate-extraction-pre-phase-10.md](2026-04-09-spec-engine-crate-extraction-pre-phase-10.md)

## 5. Scope

This phase includes:

- one dedicated `SpecQueryEngine` seam in `prism-spec`
- stable query result types for:
  - listing specs
  - fetching one spec by id
  - fetching checklist items
  - fetching dependency edges and posture
  - fetching derived status
  - fetching metadata summaries
  - fetching currently empty coverage records
  - fetching currently empty sync-provenance records
- deterministic query ordering and not-found behavior
- store reads for coverage and sync provenance so later phases can populate them without changing
  the query boundary

This phase does not include:

- explicit spec-to-coordination linking
- population of sync provenance from sync actions
- computed `SpecCoverageView` against coordination objects
- CLI or MCP query endpoints
- coordination join queries

## 6. Non-goals

This phase should not:

- move spec query ownership back into `prism-core`
- merge spec query semantics into the coordination query engine
- invent implicit coordination linkage before Phase 11
- fake coverage from checklist posture alone
- wait for later sync and coverage phases before establishing the query seam

## 7. Design

### 7.1 Ownership rule

Phase 10 must add a dedicated query owner in `prism-spec`.

Preferred shape:

- a `SpecQueryEngine` trait or equivalent documented seam
- a local materialized-store-backed implementation

The important rule is semantic ownership:

- spec query semantics live here
- consumers do not compose raw store reads into their own pseudo-query layer

### 7.2 Query-family rule

The engine should expose stable deterministic query families for:

- `list_specs`
- `spec(id)`
- `checklist_items(spec_id)`
- `dependencies(spec_id)`
- `status(spec_id)`
- `metadata()`
- `coverage(spec_id)`
- `sync_provenance(spec_id)`

Initial coverage and sync-provenance reads may return empty records from storage until later phases
populate them.

### 7.3 Determinism rule

Query results must be deterministic and explainable.

Initial requirements:

- stable ordering by `spec_id` or stored position where applicable
- explicit not-found behavior instead of silent `None`-like ambiguity in handler code
- no fallback to reparsing repo files during query execution

Phase 10 reads from materialized state only.
Rebuild belongs to earlier or later lifecycle code, not to query execution.

### 7.4 Coverage and provenance rule

Even though Phase 11 and Phase 12 have not landed yet, Phase 10 should still reserve and expose the
read path for:

- persisted coverage records
- persisted sync provenance records

This keeps the query boundary stable while allowing the actual meaning of those records to grow in
later phases.

Phase 10 must not pretend empty stored records already imply real coordination coverage.

### 7.5 Surface boundary rule

Phase 10 should keep the seam internal to library code.

Later phases will expose it through:

- CLI
- MCP
- UI
- explicit coordination join layers

This phase only establishes the stable programmatic query boundary.

## 8. Implementation slices

### Slice 1: Add query types and engine seam

- add query result types and request/lookup helpers
- define the engine boundary in `prism-spec`

Exit criteria:

- one named `SpecQueryEngine` seam exists

### Slice 2: Back basic queries with the materialized store

- implement spec, checklist, dependency, status, and metadata queries
- enforce deterministic ordering and explicit not-found handling

Exit criteria:

- basic spec reads go through the query engine instead of raw store calls

### Slice 3: Add coverage and sync-provenance read families

- extend the store seam to read coverage and sync-provenance records
- expose matching query-engine families
- keep current behavior structurally empty but deterministic

Exit criteria:

- later phases can populate coverage and sync provenance without changing the query boundary

## 9. Validation

Minimum validation for this phase:

- targeted `prism-spec` tests for query ordering, lookups, not-found behavior, and coverage /
  provenance empty-state behavior
- targeted downstream `prism-core` tests only if public re-exports or compile integration change
- `git diff --check`

Important regression checks:

- no query path reparses source files on demand
- query outputs preserve persisted ordering and identities
- empty coverage and sync-provenance reads are explicit and deterministic

## 10. Completion criteria

This phase is complete only when:

- `prism-spec` owns a dedicated `SpecQueryEngine` seam
- direct consumer logic no longer needs to build spec reads from raw store helpers
- basic spec reads are queryable through one deterministic boundary
- coverage and sync-provenance read families exist, even if still empty ahead of later phases

## 11. Implementation checklist

- [x] Add `SpecQueryEngine` seam and query result types
- [x] Implement list/spec/checklist/dependency/status/metadata queries
- [x] Add store read surfaces for coverage and sync provenance
- [x] Expose coverage and sync-provenance query families
- [x] Validate `prism-spec` and downstream integration
- [x] Update roadmap/spec status as slices land
