# Runtime Descriptor Publication Follow-Through Phase 4

Status: completed
Audience: coordination, runtime, MCP, CLI, and authority-surface maintainers
Scope: finish the next authority-surface cleanup slice by moving production runtime descriptor
publication to backend-neutral terminology

---

## 1. Summary

The authority-store migration already moved runtime descriptor publication behind
`CoordinationAuthorityStore`.

What is still misleading is the product-facing helper name:

- production code still calls `sync_live_runtime_descriptor(...)`

That name is Git shared-ref shaped even though the implementation now performs an authority-backed
runtime descriptor publish on SQLite by default.

This slice renames the production publication entrypoint and call sites to authority-neutral terms,
while leaving compatibility bridges only where they are still useful during migration.

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-sqlite-default-service-follow-through-phase-4.md](./2026-04-09-sqlite-default-service-follow-through-phase-4.md)

## 3. Goals

This slice must:

- introduce an authority-neutral runtime descriptor publication helper
- migrate production CLI and MCP call sites to that helper
- update logs and operator-facing wording away from shared-ref language where publication is now
  authority-backed
- keep targeted tests and peer-runtime flows working

## 4. Non-goals

This slice does not yet:

- redesign the full runtime-status schema
- remove every shared-ref-era type name from MCP read models
- remove the Git backend implementation
- eliminate every compatibility alias in one pass

## 5. Design

### 5.1 Publication rule

Publishing the current runtime descriptor is an authority write.

The product-facing helper for that operation should describe the operation itself, not the first
backend that once implemented it.

### 5.2 Compatibility rule

If a compatibility alias remains temporarily, production code should still migrate to the new
authority-neutral helper immediately.

## 6. Implementation scope

This slice includes:

- a new authority-neutral runtime descriptor publication helper in `prism-core`
- production call-site migration in CLI and MCP service code
- test call-site migration where needed
- contract/spec wording updates for the new helper name

## 7. Exit criteria

This slice is complete when:

- production code no longer calls the legacy shared-ref-shaped helper name for runtime descriptor
  publication
- authority-backed runtime descriptor publication still works for CLI and MCP flows
- targeted peer-runtime and runtime-status tests still pass

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-cli mcp`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`
- `cargo test -p prism-mcp peer_runtime_query_executes_prism_query`

## 9. Status updates

Current status:

- phase-4 runtime descriptor publication follow-through spec written
- production CLI and MCP call sites now use `publish_local_runtime_descriptor(...)`
- the legacy shared-ref-shaped helper remains only as a compatibility alias
- targeted runtime-status and peer-runtime validation passed for the authority-neutral helper
