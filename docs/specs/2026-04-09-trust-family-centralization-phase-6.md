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
- `prism-mcp` now has an explicit `trust_surface` helper boundary for:
  - stable `mutation_capability_denied` payload shaping
  - authority-error to protocol-state conversion
  - authority-stamp attachment for coordination mutation result state
- `server_surface.rs` principal-auth mutation gating now goes through one shared
  `authenticate_principal_mutation_common(...)` flow for both traced and untraced entry points
- the top-level mutation-auth dispatcher now also goes through one shared
  `authenticate_mutation_common(...)` path for:
  - principal credentials
  - bridge execution bindings
  - missing-auth rejection shaping
- provenance selection for ordinary mutation surfaces versus coordination-authority mutation
  surfaces now goes through `MutationProvenance::for_execution(...)` and
  `MutationProvenanceMode`, instead of keeping that actor/execution-context precedence logic as
  ad hoc local functions in `host_mutations.rs`
- knowledge-side concept, contract, and concept-relation packet provenance now also goes through
  shared `MutationProvenance` helpers for:
  - scope-to-origin mapping
  - manual packet provenance construction
  - default provenance filling on update/retire flows
- that first slice reduces duplicated trust metadata shaping in `server_surface.rs` and
  `host_mutations.rs`, but broader provenance and verification/freshness convergence still remain

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

Current progress:

- a new `prism-mcp` `trust_surface` module now centralizes:
  - mutation capability-denial payload construction
  - authority mutation error to coordination protocol-state mapping
  - authority-stamp attachment for committed coordination mutation result state
- `server_surface.rs` now uses the shared capability-denial helper in both traced and untraced
  authentication paths
- `host_mutations.rs` now uses the shared trust-surface helper for authority protocol-state and
  authority-stamp shaping instead of open-coding those trust envelopes inline
- `host_mutations.rs` now also delegates provenance-mode selection to the shared provenance module
  instead of locally deciding how authenticated principals and worktree mutator slots should be
  prioritized
- `host_mutations.rs` no longer hand-builds repeated concept/contract/relation packet provenance
  envelopes inline; that policy now lives in shared `MutationProvenance` helpers
- update and retire concept flows now share the same default-provenance fill rule instead of
  repeating `ConceptProvenance::default()` checks inline
- protected-state verification semantics now have shared report predicates in `prism-core`, and
  the MCP protected-state resource now uses those predicates instead of reinterpreting string
  status labels locally
- `trust_surface` now also owns the shared builders for structured coordination protocol mutation
  results when a mutation fails before any authoritative commit:
  - query-stage protocol rejections
  - authority conflicts
  - authority indeterminate outcomes
- `host_mutations.rs` no longer assembles those rejected/conflict/indeterminate result envelopes
  inline; it now delegates to the shared trust-surface builders

### Slice 2: Capability and authorization centralization

- group capability checks that are currently repeated across service/runtime/coordination entry
  points
- move repeated repo/runtime/service authorization decisions behind shared evaluators
- preserve behavior while deleting duplicate call-site policy interpretation

Exit criteria:

- capability-gated coordination and service surfaces no longer depend on scattered local checks

Current progress:

- `server_surface.rs` no longer keeps separate traced and untraced implementations of:
  - workspace-backed principal credential verification
  - registered-worktree enforcement
  - worktree mutator-slot acquisition
  - mutation capability gating
- `server_surface.rs` no longer keeps separate traced and untraced dispatchers for:
  - credential-vs-bridge selection
  - bridge binding verification tracing
  - missing-auth rejection shaping
- the shared flow preserves mutation-trace phase recording when a trace is present, while the
  untraced path now depends on the same authorization logic instead of a parallel copy
- mutation auth and worktree-gating payload shaping now also routes through `trust_surface` for:
  - missing auth
  - credential rejection
  - registered-worktree requirements
  - worktree-mode mismatch
  - mutator-slot conflicts and takeover rejection
  - bridge execution worktree mismatch and mode requirements
- `server_surface.rs` now delegates those service-facing trust errors to shared builders instead
  of hand-assembling their JSON envelopes inline

### Slice 3: Provenance and verification convergence

- centralize provenance construction where mutations and descriptor publication still build it
  ad hoc
- centralize verification/freshness interpretation where response surfaces still translate it
  themselves
- ensure indeterminate, verified-current, verified-stale, and unavailable stories stay aligned

Exit criteria:

- provenance and verification semantics are carried by shared code paths across coordination and
  service-backed runtime surfaces

Current progress:

- `prism-core::ProtectedStateStreamReport` now owns shared trust predicates for:
  - verified
  - conflict
  - truncated
- core protected-state verify/quarantine/repair/reconcile flows now use those predicates instead
  of open-coded string comparisons
- the MCP protected-state resource now counts non-verified streams from the shared report
  semantics instead of reinterpreting `verification_status` after view projection
- `prism-mcp` now has a dedicated `runtime_freshness_surface` helper boundary for:
  - top-level runtime freshness status classification
  - projection freshness-state interpretation
  - projection materialization-state interpretation
- `runtime_views.rs` and `serving_projection_models.rs` now consume the same freshness semantics
  instead of independently reinterpreting raw status strings
- coordination mutation result shaping for pre-commit protocol failures now also flows through the
  same trust boundary used for authority protocol-state translation, instead of being rebuilt
  separately in `host_mutations.rs`

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
- [x] Centralize the first shared MCP trust-surface helpers
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
