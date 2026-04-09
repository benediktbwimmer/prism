# Authority Sync And Legacy Alias Demotion Follow-Through Phase 4

Status: completed
Audience: coordination, public-surface, CLI, and MCP maintainers
Scope: finish a small Phase 4 cleanup slice by removing the last shared-ref-shaped wording from the
backend-neutral authority-sync path and demoting legacy shared-ref diagnostics aliases behind
explicit Git-named exports

---

## 1. Summary

Most of the SQLite-default authority cleanup is complete, but two migration leftovers remain:

- the authority live-sync path still reports a "shared coordination ref refresh lock"
- public diagnostics exports still keep legacy shared-ref helper names alive without a stronger
  signal that they are only compatibility aliases

This slice tightens both surfaces without changing runtime behavior.

## 2. Goals

- rename the backend-neutral authority-sync lock wording away from shared-ref language
- add an explicit `git_shared_coordination_ref_diagnostics_with_provider(...)` helper
- demote the legacy shared-ref diagnostics helpers to deprecated compatibility aliases
- keep downstream CLI and MCP code working unchanged

## 3. Non-goals

- no change to Git-backend diagnostics payload contents
- no removal of compatibility aliases in this slice
- no Postgres work

## 4. Implementation

This slice updates:

- `coordination_authority_api.rs`
- `coordination_authority_sync.rs`
- `lib.rs`

The settled public-surface rule is:

- backend-neutral callers use coordination-authority diagnostics
- Git-specific callers use explicit `git_shared_...` helper names
- legacy shared-ref helper names remain only as deprecated compatibility aliases

## 5. Exit criteria

- backend-neutral authority-sync wording no longer mentions shared coordination refs
- the preferred Git-specific diagnostics surface is explicit in both the API module and the crate
  facade
- legacy shared-ref diagnostics aliases are visibly demoted without breaking compatibility

## 6. Validation

- `cargo test -p prism-core coordination_authority_api`
- `cargo test -p prism-cli mcp`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`
- `python3 scripts/update_doc_indices.py --check`
