# Runtime Gateway Authority Diagnostics Follow-Through Phase 4

Status: completed
Audience: MCP, runtime-routing, authority-surface, and service maintainers
Scope: remove direct shared-ref diagnostics usage and shared-ref-shaped authority error wording
from `runtime_gateway.rs`

---

## 1. Summary

`peer_runtime_router.rs` already stopped treating Git diagnostics as the normal descriptor lookup
surface.

`runtime_gateway.rs` still has the same migration-era mismatch:

- it calls shared-ref diagnostics helpers directly from backend-neutral runtime-routing code
- it still reports authority failures as if "shared coordination refs" were the default product
  surface

That is wrong on the SQLite-default path.

This slice finishes that cleanup for the runtime gateway.

## 2. Goals

- remove direct `shared_coordination_ref_diagnostics(...)` usage from `runtime_gateway.rs`
- source degraded-verification gating through authority diagnostics instead of a Git helper entrypoint
- keep runtime descriptor discovery authority-store-driven
- make backend-neutral remote-runtime errors talk about coordination authority and published runtime
  descriptors instead of shared refs
- add focused runtime-gateway tests for the SQLite-default path

## 3. Non-goals

- no Postgres implementation work
- no redesign of the external runtime-status schema
- no change to Git backend internals beyond how backend-neutral routing consults them

## 4. Implementation plan

1. Switch `runtime_gateway.rs` to `coordination_authority_diagnostics(...)` and
   `coordination_authority_diagnostics_with_provider(...)`.
2. Gate degraded routing only when diagnostics confirm the selected backend is Git and its
   verification state is degraded.
3. Keep runtime descriptor lookup on `CoordinationAuthorityStore::list_runtime_descriptors(...)`.
4. Rename runtime-gateway error codes/messages away from shared-ref wording where the path is
   backend-neutral.
5. Add focused runtime-gateway tests for SQLite-default descriptor lookup and empty-authority
   behavior.

## 5. Exit criteria

- `runtime_gateway.rs` no longer imports direct shared-ref diagnostics helpers
- SQLite-default runtime-gateway failures no longer talk about shared coordination refs
- runtime-gateway descriptor lookup still succeeds on the SQLite-default path
- targeted `prism-mcp` validation for the runtime-gateway slice passes

## 6. Result

Completed.

`runtime_gateway.rs` now reads backend diagnostics through the coordination-authority surface,
uses the authority store for runtime descriptor lookup, and reports SQLite-default failures in
coordination-authority language instead of shared-ref language.

Focused validation run:

- `cargo test -p prism-mcp runtime_gateway::tests::`
- `cargo test -p prism-mcp peer_runtime_query_executes_prism_query`
- `cargo test -p prism-mcp execute_remote_prism_query_resolves_runtime_id_from_published_runtime_descriptor`
