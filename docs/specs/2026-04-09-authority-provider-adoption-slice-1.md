# Authority Provider Adoption Slice 1

Status: completed
Audience: coordination, service, MCP, CLI, and runtime maintainers
Scope: introduce one named authority-access owner boundary for live product-facing code and move the first high-leverage callers onto it

---

## 1. Summary

This spec is the concrete implementation target for Phase 1 of:

- [../roadmaps/2026-04-09-platform-seam-follow-through.md](../roadmaps/2026-04-09-platform-seam-follow-through.md)

The repo already has a real `CoordinationAuthorityStore` abstraction and a backend opener seam.

What is still missing is a clean ownership rule for live product-facing callers.
Several important surfaces still reach directly for the default opener, which means backend-neutral
abstraction exists in principle but not yet in practice.

This slice introduces one named authority-access owner boundary and moves the first live
product-facing callers onto it.

## 2. Status

Current state:

- [x] `CoordinationAuthorityStore` and backend opener seam exist
- [x] repo-shared coordination materialization is in place
- [x] live product-facing code no longer calls the default authority opener directly
- [x] a named owner now exists for live authority access in product-facing code

Current phase notes:

- this slice is about ownership and dependency inversion, not new backend semantics
- backend selection still remains defaulted in this slice
- the landed win is that live callers no longer own default-opener details directly

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)

This spec follows:

- [2026-04-09-service-owned-coordination-materialization-slice-1.md](2026-04-09-service-owned-coordination-materialization-slice-1.md)
- [2026-04-09-service-shell-and-role-owners-phase-3.md](2026-04-09-service-shell-and-role-owners-phase-3.md)

## 4. Scope

This slice includes:

- a named authority-access owner boundary in code
- session-scoped or repo-scoped authority access through that owner
- migration of the first high-leverage live callers:
  - trust-surface authority stamp reads
  - peer runtime descriptor reads
  - any adjacent product-facing authority reads touched during this slice

This slice does not include:

- implementing SQLite or Postgres authority backends
- fully deleting all root-only authority helpers in `prism-core`
- full service-shell extraction

## 5. Non-goals

This slice should not:

- redesign coordination authority semantics
- introduce a second provider abstraction beside the chosen owner boundary
- hardcode more Git-shaped assumptions above the owner boundary
- fan out broad refactors before the owner boundary exists

## 6. Design

### 6.1 Named-owner rule

Live product-facing code should depend on one named authority-access owner rather than directly
calling the default authority opener.

For this slice, that owner may still internally use the default backend config, but callers must
no longer own that detail directly.

### 6.2 Session-first preference

Where a `WorkspaceSession` already exists, authority access should hang off that session-owned
boundary rather than rebuilding authority access from the root path in each caller.

### 6.3 Narrow migration rule

This slice should move the highest-leverage live product-facing callers first and leave deeper
root-only compatibility helpers for later cleanup phases.

## 7. Implementation slices

### Slice 1: Add the authority-access owner boundary

- add one named owner for live authority access
- place it below product-facing surfaces and above the backend opener

Exit criteria:

- there is one obvious owner boundary for live authority access in code

### Slice 2: Move trust and peer-runtime surfaces onto it

- migrate trust-surface authority-stamp reads
- migrate peer-runtime descriptor authority reads

Exit criteria:

- those live surfaces no longer call the default authority opener directly

### Slice 3: Update docs and leave follow-on notes for remaining root-only helpers

- update this spec and the roadmap after the slice lands
- explicitly note any remaining root-only helpers deferred to later cleanup phases

Exit criteria:

- the checked-out SHA documents what landed and what remains

## 8. Validation

Minimum validation for this slice:

- targeted `prism-core` tests for the new authority-access owner boundary
- targeted `prism-mcp` tests for trust and peer-runtime surfaces
- `git diff --check`

Important regression checks:

- authority reads still route through the authority-store seam
- trust surfaces still attach authority-stamp metadata
- peer runtime routing still resolves shared runtime descriptors correctly

## 9. Completion criteria

This slice is complete only when:

- one named authority-access owner boundary exists in code
- the highest-leverage live product-facing callers use it
- the default authority opener is no longer called directly from those migrated surfaces

## 10. Implementation checklist

- [x] Add a named authority-access owner boundary
- [x] Move trust-surface authority reads onto it
- [x] Move peer-runtime authority reads onto it
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status

## 11. Landed result

This slice landed:

- `CoordinationAuthorityStoreProvider` as the named live authority-access owner boundary
- migration of live product-facing authority callers away from
  `open_default_coordination_authority_store(...)`
- updated `prism-core` and `prism-mcp` call sites so the new checked-out SHA no longer exposes the
  old direct default-opener pattern outside the provider layer
