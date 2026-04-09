# SQLite Default Service Follow-Through Phase 4

Status: completed
Audience: coordination, service, MCP, CLI, and runtime-session maintainers
Scope: remove the most obvious Git-shaped service and runtime plumbing mismatches after switching
the default `CoordinationAuthorityStore` backend to SQLite

---

## 1. Summary

Phase 3 made SQLite the real default coordination authority backend.

Phase 4 follows through on that by cleaning up service-shell and runtime-session behavior that
still assumes Git shared-ref live sync is the normal path.

The first cleanup target in this slice is the authority watch path:

- session startup should only create an authority live-sync watch when the selected backend
  actually needs one
- runtime/session naming should stop calling that watch a shared-ref watch in backend-neutral code
- logs and helper names should describe coordination authority sync rather than shared-ref sync

## 2. Related roadmap and contracts

This slice implements:

- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

This slice depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)
- [2026-04-09-sqlite-coordination-authority-phase-3.md](./2026-04-09-sqlite-coordination-authority-phase-3.md)

## 3. Goals

This slice must:

- stop spawning a Git-shaped coordination live-sync watch on SQLite-default sessions
- rename the runtime/session watch ownership surface to coordination-authority terms
- keep Git live-sync behavior working when the Git shared-refs backend is explicitly selected
- preserve the existing authority-store-based runtime descriptor publication path

## 4. Non-goals

This slice does not yet:

- redesign every MCP runtime-status field
- remove the Git backend
- implement Postgres behavior
- eliminate every legacy helper name in one pass

## 5. Design

### 5.1 Live-sync support rule

Authority live sync is backend-specific behavior, not a mandatory session primitive.

Session startup should ask the selected authority backend whether live sync is active. If the
backend does not support live sync, startup should not allocate a polling watch thread for it.

### 5.2 Naming rule

Backend-neutral session, watch, and service code should use coordination-authority language rather
than shared-ref language.

Git-specific terminology may remain inside the Git backend implementation and its dedicated tests.

## 6. Implementation scope

This slice includes:

- a backend-neutral authority live-sync enablement check
- conditional authority-watch spawning in workspace-session startup
- renaming the watch/session ownership path away from `shared_coordination_ref_watch`
- targeted test follow-through for the renamed authority-watch path

## 7. Exit criteria

This slice is complete when:

- SQLite-default sessions do not spawn a pointless shared-ref authority polling watch
- session/watch naming is coordination-authority-shaped in backend-neutral code
- targeted tests still cover authority-watch synchronization and SQLite-default runtime behavior

## 8. Validation

Minimum validation for this slice:

- `python3 scripts/update_doc_indices.py --check`
- `cargo test -p prism-core coordination_authority_store`
- `cargo test -p prism-core watch`
- `cargo test -p prism-cli mcp`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`

## 9. Status updates

Current status:

- phase-4 spec written
- backend-neutral authority live-sync enablement is implemented
- SQLite-default sessions now skip authority-watch thread startup entirely
- backend-neutral session and watch ownership now use coordination-authority naming instead of
  shared-ref naming
- targeted validation for the first Phase 4 cleanup slice is complete
