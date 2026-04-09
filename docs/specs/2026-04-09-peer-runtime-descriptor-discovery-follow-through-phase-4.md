# Peer Runtime Descriptor Discovery Follow-Through Phase 4

Status: completed
Audience: coordination, MCP, runtime-routing, and authority-surface maintainers
Scope: finish the next authority-surface cleanup slice by removing diagnostic-backed runtime
descriptor discovery from peer runtime routing

---

## 1. Summary

Peer runtime routing is already close to the desired authority-store shape, but one migration-era
mismatch remains:

- the router still reads runtime descriptors out of Git backend diagnostics before it falls back to
  `CoordinationAuthorityStore::list_runtime_descriptors(...)`

That keeps diagnostics acting as an accidental descriptor registry.

This slice removes that shortcut so runtime descriptor discovery is always authority-store-driven,
while diagnostics remain limited to degraded-verification context and operator hints.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-runtime-descriptor-publication-follow-through-phase-4.md](./2026-04-09-runtime-descriptor-publication-follow-through-phase-4.md)

## 3. Goals

This slice must:

- remove runtime descriptor discovery from authority diagnostics in peer runtime routing
- keep degraded-verification detection and repair hints available where needed
- make peer runtime routing rely on `CoordinationAuthorityStore::list_runtime_descriptors(...)`
  for descriptor lookup across backends
- preserve current peer-runtime query behavior

## 4. Non-goals

This slice does not yet:

- redesign the full MCP runtime-status schema
- remove every shared-ref-era read-model type name
- change Git backend diagnostics semantics
- implement Postgres behavior

## 5. Design

### 5.1 Discovery rule

Runtime descriptor discovery and diagnostics are separate concerns.

Descriptor lookup should come from the authority store’s runtime descriptor query surface, not from
backend diagnostic payloads.

### 5.2 Diagnostics rule

Diagnostics may still provide:

- degraded verification state
- repair hints
- backend-specific context

But they should not be the normal source of descriptor records.

## 6. Implementation scope

This slice includes:

- peer runtime router cleanup so descriptor lookup always uses the authority store
- preservation of degraded Git-backend verification handling without descriptor extraction from
  diagnostics
- contract/spec wording updates if the implementation shape changes

## 7. Exit criteria

This slice is complete when:

- `resolve_runtime_descriptor(...)` no longer treats diagnostics as a descriptor registry
- peer runtime routing still handles degraded authority verification honestly
- targeted peer-runtime tests still pass

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-mcp peer_runtime_query_executes_prism_query`
- `cargo test -p prism-mcp execute_remote_prism_query_resolves_runtime_id_from_published_runtime_descriptor`

## 9. Status updates

Current status:

- phase-4 peer-runtime descriptor discovery follow-through spec written
- peer runtime routing now uses `CoordinationAuthorityStore::list_runtime_descriptors(...)`
  for descriptor lookup across backends
- diagnostics remain limited to degraded-verification gating and operator hints
- targeted peer-runtime routing validation passed
