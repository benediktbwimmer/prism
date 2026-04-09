# Runtime Status Authority View Follow-Through Phase 4

Status: completed
Audience: coordination, MCP, UI-read-model, and trust-surface maintainers
Scope: finish the next authority-surface cleanup slice by reducing shared-ref-era naming inside
runtime-status caching and trust-surface shaping

---

## 1. Summary

The authority-store migration has cleaned up backend selection, runtime descriptor publication, and
peer-runtime descriptor discovery.

One obvious migration seam still remains in the MCP status path:

- internal runtime-status caching and trust-surface helpers still use shared-ref-era names such as
  `shared_coordination_ref` and `runtime_shared_coordination_ref_view(...)`

Even when the external schema still needs compatibility, the internal ownership and shaping code
should move toward authority-neutral language.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-git-diagnostics-demotion-follow-through-phase-4.md](./2026-04-09-git-diagnostics-demotion-follow-through-phase-4.md)

## 3. Goals

This slice must:

- reduce shared-ref-era naming in backend-neutral runtime-status caching and trust-surface helpers
- preserve external compatibility where the status schema still requires it
- keep authority diagnostics and trust shaping aligned with the SQLite-first default path

## 4. Non-goals

This slice does not yet:

- redesign the full runtime-status response contract
- remove the Git backend
- remove every legacy UI field in one pass

## 5. Design

### 5.1 Internal naming rule

Backend-neutral runtime-status plumbing should use authority-oriented naming even when compatibility
layers still expose a shared-ref-shaped field outward.

### 5.2 Compatibility rule

If the external response still carries a compatibility field, the internal cache and trust-surface
code should nonetheless be written around authority-neutral concepts and adapt only at the edge.

## 6. Implementation scope

This slice includes:

- diagnostics-state cache naming cleanup
- runtime-view helper naming cleanup
- trust-surface helper naming cleanup where it is backend-neutral

## 7. Exit criteria

This slice is complete when:

- backend-neutral MCP status plumbing no longer uses shared-ref-era names internally where
  authority-neutral equivalents are available
- targeted runtime-status tests still pass

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`

## 9. Status updates

Current status:

- phase-4 runtime-status authority-view follow-through spec written
- implementation complete
- diagnostics-state runtime-status caching now uses authority-oriented internal naming
- runtime-view shaping now treats the cached status/trust payload as a coordination-authority view
  internally and only adapts to the legacy shared-ref field at the response edge
- trust-surface shaping now uses `runtime_coordination_authority_view(...)` as the primary
  backend-neutral helper name, with the legacy shared-ref helper name retained only as a
  compatibility alias
