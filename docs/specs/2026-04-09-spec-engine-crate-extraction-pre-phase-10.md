# Spec Engine Crate Extraction Pre-Phase 10

Status: draft
Audience: spec-engine, core, query, MCP, CLI, and storage maintainers
Scope: extract the native spec-engine parser and materialized-store seams from `prism-core` into a dedicated crate so Phase 10 can add a clean `SpecQueryEngine` without cross-crate layering violations

---

## 1. Summary

This spec is a prerequisite implementation target before Phase 10 query work proceeds.

The current code shape is functionally correct but crate ownership is wrong:

- spec parsing and materialization live in `prism-core`
- the next phase wants a shared deterministic `SpecQueryEngine`
- `prism-query` cannot cleanly depend on `prism-core`

If left in place, Phase 10 would force one of two bad outcomes:

- keep spec query logic in `prism-core`, muddying long-term ownership
- or duplicate/adapt spec query logic in MCP and CLI surfaces

The right fix is a small targeted extraction:

- introduce a dedicated `prism-spec` crate
- move spec parser, materialized-store seam, and related types into it
- keep `prism-core` as the repo/runtime integration layer that provides paths and wiring

## 2. Status

Current state:

- [x] Phase 8 parser and identity layer exists
- [x] Phase 9 local spec materialization layer exists
- [ ] spec-engine code still lives in `prism-core`
- [ ] no dedicated spec-engine crate exists
- [ ] Phase 10 query work would currently cross crate boundaries awkwardly

Current slice notes:

- this is intentionally a small pre-Phase-10 restructure, not a broad workspace reorganization
- the extraction should preserve behavior while improving ownership

## 3. Related roadmap

This spec is a prerequisite for:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 10: implement the `SpecQueryEngine` fully

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)

This spec builds on:

- [2026-04-09-spec-engine-source-parser-identity-phase-8.md](2026-04-09-spec-engine-source-parser-identity-phase-8.md)
- [2026-04-09-spec-engine-local-materialization-phase-9.md](2026-04-09-spec-engine-local-materialization-phase-9.md)

## 5. Scope

This extraction includes:

- creating a dedicated `crates/prism-spec`
- moving spec discovery, parsing, and materialized-store code into that crate
- keeping the new crate independent of `prism-core`
- switching path-dependent APIs to accept plain `&Path` or SQLite db paths instead of `PrismPaths`
- rewiring `prism-core` to re-export or adapt the extracted functionality where needed

This extraction does not include:

- implementing the full `SpecQueryEngine`
- adding CLI or MCP spec query surfaces
- changing spec semantics, parser rules, or store behavior

## 6. Non-goals

This work should not:

- trigger a large repo-wide crate split
- move unrelated coordination or runtime code out of `prism-core`
- redesign Phase 8 or Phase 9 semantics
- couple `prism-spec` back to `prism-core`

## 7. Design

### 7.1 Ownership rule

The dedicated spec crate should own:

- spec source discovery
- spec parsing and diagnostics
- spec materialized-store traits, types, and SQLite implementation
- later `SpecQueryEngine`

`prism-core` should own only integration responsibilities such as:

- repo/worktree path derivation
- session/bootstrap wiring
- higher-level product integration

### 7.2 Dependency rule

`prism-spec` must not depend on `prism-core`.

Instead:

- repo-root-dependent operations should take `&Path`
- SQLite-backed store operations should take the resolved db path directly
- any `PrismPaths` usage should stay in `prism-core`

### 7.3 Behavior-preservation rule

This extraction is structural, not semantic.

The extracted crate should preserve:

- default and override spec-root behavior
- parser output and diagnostics
- materialized-store schema and lifecycle
- local status derivation

## 8. Implementation slices

### Slice 1: Create `prism-spec` crate and move parser/discovery types

- add the new workspace crate
- move the current `spec_engine` module and exports into it
- replace `PrismPaths` usage with plain repo-root-relative helpers

Exit criteria:

- spec discovery and parsing compile from `prism-spec`

### Slice 2: Move the spec materialized-store seam

- move the current `spec_materialized_store` module and exports into `prism-spec`
- replace `PrismPaths`-based db resolution with plain path-based constructors

Exit criteria:

- spec materialization compiles and tests from `prism-spec`

### Slice 3: Rewire `prism-core`

- add the new dependency
- replace old local module ownership with re-exports or adapters
- keep external call sites stable where practical

Exit criteria:

- `prism-core` no longer owns spec parser/materialization logic directly

## 9. Validation

Minimum validation for this extraction:

- targeted tests for the new `prism-spec` crate
- targeted downstream tests in `prism-core` if its public surface changes
- `git diff --check`

Important regression checks:

- spec discovery behavior is unchanged
- parser diagnostics are unchanged
- materialized-store replace/clear semantics are unchanged

## 10. Completion criteria

This extraction is complete only when:

- spec-engine code lives in a dedicated crate
- `prism-spec` has no dependency on `prism-core`
- `prism-core` no longer owns the spec parser/materialization implementation directly
- Phase 10 can add a clean `SpecQueryEngine` without cross-crate layering problems

## 11. Implementation checklist

- [ ] Create `prism-spec`
- [ ] Move spec discovery/parser/types into `prism-spec`
- [ ] Move spec materialized-store seam into `prism-spec`
- [ ] Rewire `prism-core` to the new crate
- [ ] Validate changed crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
