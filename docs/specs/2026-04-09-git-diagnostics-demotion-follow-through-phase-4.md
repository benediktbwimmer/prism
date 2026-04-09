# Git Diagnostics Demotion Follow-Through Phase 4

Status: completed
Audience: coordination, runtime, MCP, CLI, and public-surface maintainers
Scope: finish the next authority-surface cleanup slice by demoting the Git-only diagnostics helper
from the primary public surface

---

## 1. Summary

Production code already routes authority status through `coordination_authority_diagnostics(...)`.

What still remains from the migration era is a top-level Git-shaped helper:

- `shared_coordination_ref_diagnostics(...)`

That helper is now a backend-specific compatibility surface, not the primary architecture story.

This slice should demote it accordingly, so backend-neutral code and public exports emphasize the
authority-store path while Git-specific diagnostics remain available only as an explicitly
backend-specific helper.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-peer-runtime-descriptor-discovery-follow-through-phase-4.md](./2026-04-09-peer-runtime-descriptor-discovery-follow-through-phase-4.md)

## 3. Goals

This slice must:

- stop presenting `shared_coordination_ref_diagnostics(...)` as a primary public authority API
- make any remaining Git-specific diagnostics helper explicitly backend-specific in name and export
- preserve Git-backend diagnostics access for tests and backend-specific tooling
- keep backend-neutral call sites using `coordination_authority_diagnostics(...)`

## 4. Non-goals

This slice does not yet:

- redesign the MCP runtime-status schema
- remove the Git backend
- remove every compatibility shim in one pass
- change the underlying Git-backend diagnostics payload shape

## 5. Design

### 5.1 Public-surface rule

Backend-neutral authority status should be represented by the authority-store diagnostics surface.

Git-specific diagnostics helpers may remain, but they should read as Git-specific helpers rather
than as the default public coordination API.

## 6. Implementation scope

This slice includes:

- public-surface cleanup around the Git-only diagnostics helper
- any required helper renames or re-exports
- contract/spec wording updates for the demoted Git-specific surface

## 7. Exit criteria

This slice is complete when:

- the primary public authority diagnostics surface is backend-neutral
- Git-specific diagnostics remain reachable only through explicitly backend-specific naming
- targeted authority-diagnostics consumers still work

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-cli mcp`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`

## 9. Status updates

Current status:

- phase-4 Git diagnostics demotion spec written
- the primary public export now uses the explicit Git-specific helper name
  `git_shared_coordination_ref_diagnostics(...)`
- the legacy shared-ref diagnostics helper remains only as a compatibility alias
- targeted diagnostics consumers still pass after the public-surface demotion
