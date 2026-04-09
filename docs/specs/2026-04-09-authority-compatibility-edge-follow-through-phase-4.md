# Authority Compatibility Edge Follow-Through Phase 4

Status: completed
Audience: MCP, UI, runtime-status, and operator-guidance maintainers
Scope: keep the legacy `shared_coordination_ref` response field only as a compatibility edge while
moving internal runtime-status plumbing and operator-facing copy to coordination-authority wording

---

## 1. Summary

SQLite is already the default authority backend, but a few MCP and UI surfaces still talk as if the
main product concept were "the shared coordination ref".

The remaining leak is not primarily backend implementation logic. It is a compatibility-boundary
problem:

- the external runtime-status schema still exposes `shared_coordination_ref`
- some internal caches and UI read models still reach into that field directly
- session guidance still tells operators to repair task completion so "the shared coordination ref"
  records completion

This slice keeps the legacy field for compatibility while making the internal and operator-facing
story authority-neutral.

## 2. Goals

- centralize access to the legacy `shared_coordination_ref` runtime-status field behind one
  authority-named helper
- remove remaining shared-ref-shaped wording from backend-neutral session repair guidance
- keep the external MCP/runtime-status schema stable
- update the roadmap and contract trail to make the compatibility-boundary rule explicit

## 3. Non-goals

- no change to the external `RuntimeStatusView` field name in this slice
- no change to Git-backend diagnostics payload contents
- no Postgres work

## 4. Implementation

This slice updates:

- `trust_surface.rs`
- `diagnostics_state.rs`
- `ui_read_models.rs`
- `host_resources.rs`
- targeted MCP tests for session task guidance

The settled rule is:

- `shared_coordination_ref` remains an outward compatibility field
- internal caches, read models, and repair guidance refer to coordination authority
- direct field access should be isolated behind `runtime_status_coordination_authority_view(...)`

## 5. Exit criteria

- backend-neutral operator guidance no longer tells users that completion authority is specifically
  "the shared coordination ref"
- runtime-status compatibility-field access is centralized behind one helper
- the external runtime-status response stays backward compatible
- targeted MCP validation passes

## 6. Validation

- `cargo test -p prism-mcp git_execution_completion_trace_records_subphases_without_ui_publish`
- `cargo test -p prism-mcp coordination_mutation_trace_records_persistence_subphases`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`
- `cargo test -p prism-mcp session_resource_surfaces_publish_failed_repair_action`
