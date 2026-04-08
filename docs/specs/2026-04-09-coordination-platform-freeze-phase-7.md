# Coordination Platform Freeze Phase 7

Status: in progress
Audience: coordination, service, runtime, query, MCP, CLI, UI, storage, and testing maintainers
Scope: complete roadmap Phase 7 by deleting the remaining transitional coordination shims, aligning the live docs with the implemented seams, and treating coordination as the base platform for the native spec engine

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 7:

- freeze the service-backed coordination platform after Phases 1 through 6
- delete transitional compatibility shims that still exist only to bridge old runtime-owned or
  pre-seam behavior
- align live docs and test coverage with the implemented authority, materialization, query,
  mutation, and trust seams

This phase is not another architecture rewrite.
It is the stabilization pass that turns the current coordination implementation into the platform
the native spec engine can depend on without reopening the last six phases.

## 2. Status

Current state:

- [x] authority, materialized, query, mutation, and trust seams exist
- [x] service-backed coordination ownership is live
- [x] product-facing coordination reads and writes route through the new seams
- [x] the remaining transitional seams are explicit and named
- [ ] the remaining transitional seams are not yet fully deleted or narrowed to intentional local
  overlays only
- [ ] tests are not yet consistently written against the new seam boundaries
- [ ] docs do not yet consistently describe the current code as implemented reality instead of
  recent migration history

Current slice notes:

- Phase 5 intentionally left a small set of explicit cleanup targets for this phase:
  - local assisted-lease overlay republish in the watch layer
  - protected-state continuity fallback helpers
  - MCP coordination-surface fallback for not-yet-hydrated session state
- Phase 6 centralized the live trust-family semantics, which means the remaining work is now
  mostly deletion, narrowing, and test/doc cleanup rather than one more semantic migration
- the first Phase 7 slice narrows `prism-mcp` coordination-surface loading so a workspace-backed
  coordination surface no longer silently substitutes the live in-memory `Prism` coordination
  snapshot when service-backed coordination state is absent; live runtime coordination remains an
  explicit overlay only where the contracts already allow it

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 7: freeze coordination as the base platform

## 4. Related contracts and ADRs

This spec depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-authority-sync-role.md](../contracts/service-authority-sync-role.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/authorization-and-capabilities.md](../contracts/authorization-and-capabilities.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/signing-and-verification.md](../contracts/signing-and-verification.md)
- [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

## 5. Scope

This phase includes:

- deleting or sharply narrowing the remaining explicit transitional coordination shims
- moving tests away from legacy direct runtime-state assumptions and toward the new seams
- tightening the live docs so they describe the settled coordination platform as implemented
  reality
- establishing a clean starting point for the native spec engine phases

This phase does not include:

- spec-engine implementation
- remaining richer PRISM Service role implementation
- DB-backed authority implementation work beyond what is needed to keep the docs and seams honest
- introducing a new coordination semantic model

## 6. Non-goals

This phase should not:

- reopen service-owned coordination ownership
- reopen the coordination mutation protocol
- reintroduce runtime-owned coordination persistence or fallback writes
- invent another temporary compatibility layer to avoid deleting an old one
- absorb spec-engine concepts into coordination before the spec-engine phases begin

## 7. Design

### 7.1 Freeze rule

After this phase, coordination should be treated as the stable substrate for the next layer.
If a coordination behavior still needs a compatibility shim, that shim must either:

- be deleted in this phase, or
- be explicitly justified as an intentional long-term local overlay or transport boundary

### 7.2 Transitional-shim rule

Named transitional seams left by Phases 5 and 6 should not silently become permanent.
This phase exists to decide which ones are:

- deleted
- narrowed to intentional local overlays
- or promoted into explicit long-term boundaries with contract-backed rationale

### 7.3 Test rule

Coordination tests should increasingly validate:

- authority-store behavior
- service-owned materialization behavior
- query-engine behavior
- mutation-protocol behavior
- trust-surface behavior

instead of validating old runtime snapshot side effects or hidden fallback paths.

### 7.4 Docs rule

Docs touched in this phase should describe:

- what is now implemented reality
- what remains intentionally transitional
- what future phases start from

They should not keep speaking in migration-tense once the platform is frozen.

## 8. Implementation slices

### Slice 1: Transitional seam audit and deletion

- inventory the remaining explicit coordination shims still referenced in runtime, MCP, and watch
  paths
- delete the ones that are only migration leftovers
- narrow the surviving ones to intentional local-overlay or transport-boundary responsibilities

Expected targets:

- assisted-lease overlay republish in the watch layer
- protected-state continuity fallback helpers
- MCP coordination-surface fallback for not-yet-hydrated session state

Exit criteria:

- migration-only shims are deleted or reduced to clearly intentional boundaries

Current progress:

- `prism-mcp` `coordination_surface` no longer treats the live in-memory `Prism` coordination
  snapshot as a hidden service-backed fallback whenever a workspace session exists
- persisted coordination read models, when present, now flow through the named coordination
  surface instead of always being recomputed from the snapshot
- live runtime coordination remains available only through the explicit overlay inputs used by the
  runtime-status surface
- protected-state runtime recovery and workspace indexer startup now also default missing
  service-backed coordination state to empty coordination state instead of inheriting the live
  in-memory coordination snapshot as a hidden fallback

### Slice 2: Test retargeting

- update targeted tests that still assert old runtime-owned or hidden fallback behavior
- add or refine targeted tests around the current seams where this phase deletes transitional
  behavior
- keep the validation focus on changed crates and direct downstream dependents only

Exit criteria:

- changed coordination tests validate the frozen seam behavior rather than the migration path

### Slice 3: Docs and platform freeze pass

- update the relevant specs, roadmap entries, and architecture notes after the code cleanup lands
- explicitly mark coordination as the base platform for the spec-engine phases
- remove outdated wording that still implies recent migration state where that is no longer true

Exit criteria:

- the docs match the implemented frozen coordination platform at the checked-out SHA

## 9. Validation

Minimum validation for this phase:

- targeted tests in changed crates for the exact seam or shim being cleaned up
- direct downstream dependent tests when a shared seam or public shared helper changes
- `git diff --check`

Important regression checks for this phase:

- no deleted shim removes real local-overlay behavior that the current contracts still allow
- product-facing coordination reads and writes still route through the named seams
- service-owned eventual-state behavior remains intact
- runtime-local telemetry overlays remain local and clearly non-authoritative

## 10. Completion criteria

Phase 7 is complete only when:

- the remaining migration-only coordination shims are deleted or explicitly narrowed to
  intentional long-term boundaries
- tests primarily target the frozen coordination seams rather than transitional fallback behavior
- the roadmap and current specs describe coordination as the new base platform

## 11. Implementation checklist

- [x] Audit the remaining explicit transitional coordination seams
- [ ] Delete or narrow migration-only compatibility shims
- [ ] Retarget tests to the frozen seam behavior
- [ ] Update docs to describe the frozen coordination platform accurately
- [ ] Validate changed crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land

## 12. Current implementation status

Coordination is now close to platform status, but not quite there yet.

The remaining work is no longer foundational seam invention.
It is the cleanup needed so the next subsystem does not inherit:

- compatibility layers as accidental architecture
- tests that still encode old ownership assumptions
- docs that still talk like the migration is the product
