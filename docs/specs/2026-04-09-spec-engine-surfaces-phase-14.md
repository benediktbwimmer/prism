# Spec Engine Surfaces Phase 14

Status: completed
Audience: spec-engine, query, MCP, CLI, UI, and agent-execution maintainers
Scope: expose the existing native spec engine coherently through CLI, MCP, and UI-facing reads so specs become a first-class feature-intent layer in normal PRISM workflows

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 14:

- expose native spec queries through stable product surfaces
- surface linked spec summaries alongside coordination reads
- keep spec parsing and spec-coordination joins inside the existing spec and coordination seams

This phase builds on:

- deterministic spec source discovery, parsing, and local materialization
- `SpecQueryEngine`
- typed coordination `spec_refs`
- `SpecCoverageView`
- explicit spec-aware sync actions

This phase does not introduce new spec semantics. It exposes the already-implemented native spec
model cleanly.

## 2. Status

Current state:

- [x] native specs parse and materialize deterministically
- [x] native specs expose deterministic query surfaces
- [x] sync actions can create spec-linked authoritative coordination
- [x] CLI exposes native spec queries as a first-class surface
- [x] MCP exposes native spec queries and sync brief views directly
- [x] task and plan reads consistently surface linked spec summaries and basic drift posture

Current slice notes:

- this phase should reuse the existing `SpecQueryEngine` rather than re-parsing markdown in CLI or
  MCP handlers
- linked spec summaries should be additive read surfaces, not hidden blockers
- surface-level joins must preserve the distinction between authoritative coordination truth and
  local branch-bound spec state
- the MCP surface now exposes `specs`, `spec`, `specSyncBrief`, `specCoverage`, and
  `specSyncProvenance` through one shared `spec_surface` adapter over `SpecQueryEngine`
- the CLI now exposes the same read families through `prism specs ...` and reuses the native spec
  query engine rather than introducing a second parser or direct SQLite read path

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 14: expose the spec engine fully through CLI, MCP, and UI

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)

This spec builds on:

- [2026-04-09-spec-query-engine-phase-10.md](2026-04-09-spec-query-engine-phase-10.md)
- [2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md](2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md)
- [2026-04-09-spec-coverage-phase-12.md](2026-04-09-spec-coverage-phase-12.md)
- [2026-04-09-spec-sync-actions-phase-13.md](2026-04-09-spec-sync-actions-phase-13.md)

## 5. Scope

This phase includes:

- a first-class CLI surface for native spec listing, lookup, sync brief, coverage, and sync
  provenance
- stable MCP query/tool surfaces for native spec reads and sync-oriented inspection
- linked spec summaries in task and plan reads where the coordination object already carries typed
  `spec_refs`
- basic divergence or drift warnings where coordination links point at older spec revisions

This phase does not include:

- making local spec status authoritative over coordination
- editing spec markdown through MCP or CLI
- automatic background spec-to-coordination sync

## 6. Non-goals

This phase should not:

- add a second spec parser in product-surface crates
- bypass `SpecQueryEngine` with direct SQLite reads in CLI or MCP handlers
- collapse coordination and spec views into one ambiguous truth model

## 7. Design

### 7.1 Surface-owner rule

`prism-spec` remains the owner of spec parsing, materialization, and spec query semantics.
CLI, MCP, and UI-facing code may adapt and present those views, but they must not recreate the
underlying logic ad hoc.

### 7.2 Join-labeling rule

When a task or plan read includes linked spec information, the surface must make clear that:

- coordination task or plan state is authoritative
- spec summary, coverage, and drift posture are local spec-engine enrichments

### 7.3 Surface-priority rule

Initial surface priority order:

1. MCP read exposure
2. CLI read exposure
3. linked task and plan summaries for UI-facing reads

The first cut should focus on read-side usefulness, not writing a full spec-management UI.

## 8. Implementation slices

### Slice 1: Expose native spec reads through MCP

- add bounded MCP read surfaces for:
  - spec list
  - spec document
  - sync brief
  - coverage
  - sync provenance

Exit criteria:

- agents can inspect native specs through MCP without opening raw markdown or re-parsing specs in
  handler code
- status: completed

### Slice 2: Expose native spec reads through CLI

- add `prism specs ...` commands for:
  - list
  - show
  - sync-brief
  - coverage
  - sync-provenance

Exit criteria:

- humans can inspect native spec state directly from the CLI against the same underlying engine
- status: completed

### Slice 3: Add linked spec summaries to coordination-facing reads

- surface linked spec summaries and revision-drift posture in task and plan read views where typed
  `spec_refs` already exist

Exit criteria:

- normal task and plan reads expose enough linked spec context that humans and agents can see the
  intent-execution relationship without manual stitching
- status: completed

## 9. Validation

Minimum validation for this phase:

- targeted `prism-spec` tests where shared query behavior changes
- targeted `prism-mcp` tests for MCP spec reads and linked spec summaries
- targeted `prism-cli` tests for new `prism specs ...` surfaces
- `git diff --check`

Important regression checks:

- CLI and MCP reads use the same underlying native spec engine
- linked spec summaries never overwrite authoritative coordination fields
- missing local specs degrade clearly rather than pretending linked context still exists

## 10. Completion criteria

This phase is complete only when:

- specs are readable as first-class objects through CLI and MCP
- linked task and plan reads expose useful spec context
- surfaces preserve the authoritative-versus-local distinction cleanly

## 11. Implementation checklist

- [x] Expose native spec reads through MCP
- [x] Expose native spec reads through CLI
- [x] Add linked spec summaries to coordination-facing reads
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
