# Trust Family Centralization Phase 6

Status: in progress
Audience: coordination, service, runtime, identity, authz, provenance, diagnostics, and UI maintainers
Scope: complete roadmap Phase 6 by centralizing the trust-family contracts into the live coordination and service-backed runtime paths

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 6:

- centralize the trust-family contracts in code where coordination and service-backed runtime
  behavior already depends on them:
  - identity
  - authorization and capabilities
  - provenance
  - signing and verification
  - consistency and trust metadata

Phase 5 finished the service-backed coordination cutover.
Phase 6 now makes the trust model coherent across those new seams.

The goal is not to redesign trust semantics.
The goal is to remove duplicated or ad hoc trust interpretation from coordination reads, writes,
descriptor publication, diagnostics, and service-facing response surfaces.

## 2. Status

Current state:

- [x] trust-family contracts exist
- [x] authority-store and mutation-protocol result shapes already carry some trust metadata
- [ ] trust metadata is not yet centralized behind one shared code path
- [ ] capability checks still rely on scattered call-site interpretation
- [ ] provenance envelope construction is not yet shared enough across coordination surfaces
- [ ] verification and freshness state are not yet surfaced consistently through the same boundary

Current slice notes:

- Phase 4 and Phase 5 already converged a lot of write/read plumbing onto the authority,
  materialized, query, and mutation seams
- that convergence makes it practical to centralize trust-family enforcement and output now,
  because there are fewer old bypasses left to special-case
- the next implementation work should focus on shared trust envelopes and shared check/evaluation
  helpers, not on inventing new policy

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 6: trust-family cleanup and centralization

## 4. Related contracts and ADRs

This spec depends on:

- [../contracts/identity-model.md](../contracts/identity-model.md)
- [../contracts/authorization-and-capabilities.md](../contracts/authorization-and-capabilities.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/signing-and-verification.md](../contracts/signing-and-verification.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/runtime-identity-and-descriptors.md](../contracts/runtime-identity-and-descriptors.md)
- [../contracts/service-capability-and-authz.md](../contracts/service-capability-and-authz.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)

## 5. Scope

This phase includes:

- centralizing identity and capability evaluation that already participates in coordination and
  service-backed runtime paths
- centralizing provenance envelope construction for meaningful coordination mutations and
  descriptor publication where it is still duplicated
- making verification and freshness/trust metadata surface through shared result shapes rather
  than call-site-specific interpretation
- converging diagnostics and service-facing read/write surfaces on the same trust vocabulary

This phase does not include:

- introducing a new auth system or external identity provider flow
- implementing the remaining richer PRISM Service roles
- redesigning signing semantics or changing the accepted trust contracts
- broader platform freeze work reserved for Phase 7

## 6. Non-goals

This phase should not:

- reopen the authority backend decision
- redesign the coordination mutation protocol again
- add speculative hosted-service login flows
- add public-internet peer trust or remote execution semantics beyond the current contracts

## 7. Design

### 7.1 Centralization rule

If two coordination or service-backed surfaces need the same trust-family interpretation, they
should share one helper, type, or boundary module rather than re-deriving it inline.

### 7.2 Trust-family target

After this phase:

- identity context should be assembled through shared code paths
- capability checks should be grouped into a small number of shared evaluators
- provenance envelopes should be constructed through shared builders where possible
- freshness and verification posture should flow through shared response metadata

### 7.3 Surface rule

This phase should improve:

- authority-store results
- mutation results
- runtime descriptor publication/discovery
- diagnostics and status surfaces
- service-backed query or mutation responses

without creating a second parallel trust vocabulary.

## 8. Implementation slices

### Slice 1: Shared trust metadata and envelope audit

- inventory the live trust/freshness/provenance metadata shapes already used by:
  - authority-store reads
  - mutation results
  - runtime descriptor flows
  - diagnostics/status surfaces
- identify duplicated or conflicting field construction
- converge on one shared internal envelope or adapter family where appropriate

Exit criteria:

- live trust-related outputs no longer assemble the same metadata in several incompatible ways

### Slice 2: Capability and authorization centralization

- group capability checks that are currently repeated across service/runtime/coordination entry
  points
- move repeated repo/runtime/service authorization decisions behind shared evaluators
- preserve behavior while deleting duplicate call-site policy interpretation

Exit criteria:

- capability-gated coordination and service surfaces no longer depend on scattered local checks

### Slice 3: Provenance and verification convergence

- centralize provenance construction where mutations and descriptor publication still build it
  ad hoc
- centralize verification/freshness interpretation where response surfaces still translate it
  themselves
- ensure indeterminate, verified-current, verified-stale, and unavailable stories stay aligned

Exit criteria:

- provenance and verification semantics are carried by shared code paths across coordination and
  service-backed runtime surfaces

### Slice 4: Diagnostics and service-surface cleanup

- route diagnostics/status and related service-facing read/write surfaces through the shared
  trust-family helpers introduced above
- delete residual compatibility wrappers or duplicate formatting paths when they are no longer
  needed

Exit criteria:

- trust-family semantics are visible in one coherent shape across the main coordination and
  service-backed runtime surfaces

## 9. Validation

Minimum validation for this phase:

- targeted tests in changed crates for:
  - mutation result metadata
  - capability-gated coordination/service flows
  - runtime descriptor publication/discovery
  - diagnostics/status surfaces touched by the cleanup
- downstream validation for crates affected by shared trust-family type changes
- `git diff --check`

Important regression checks for this phase:

- trust metadata remains stable for existing coordination and MCP callers
- capability checks do not silently weaken during centralization
- provenance remains attributable and consistent across retries and rejections
- verified-current, verified-stale, indeterminate, and unavailable states remain distinguishable

## 10. Completion criteria

Phase 6 is complete only when:

- shared trust-family helpers or boundary modules exist where live coordination/service paths were
  previously duplicating identity, capability, provenance, or verification logic
- coordination and service-backed runtime surfaces speak the same trust vocabulary
- the Phase 6 spec and roadmap are updated to reflect landed slices

## 11. Implementation checklist

- [ ] Audit live trust-family metadata and duplicate builders
- [ ] Centralize repeated capability checks
- [ ] Centralize provenance and verification/freshness construction
- [ ] Cut diagnostics/service surfaces over to the shared trust-family boundary
- [ ] Validate changed crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land

## 12. Current implementation status

Phase 6 starts from a better place than earlier coordination phases:

- the core coordination seams now exist
- service-backed coordination ownership is in place
- read/write paths are already far more centralized than before

That means this phase should mostly be:

- convergence
- deletion of duplicated trust logic
- shared envelope and checker introduction

not another broad architectural rewrite.
