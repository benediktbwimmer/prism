# Coordination Startup Authority Naming Follow-Through Phase 4

Status: completed
Audience: coordination, persistence, startup-checkpoint, and prism-core maintainers
Scope: replace remaining shared-ref-era startup authority helper naming with backend-neutral coordination-authority naming in the checkpoint and authority API layers

---

## 1. Summary

SQLite is now the default coordination authority backend, but the startup-checkpoint helper path in
`prism-core` still describes the authority source as if it were inherently a shared-ref concept.

That is no longer true.

The startup checkpoint needs an authority description regardless of whether the source is:

- Git shared refs
- SQLite
- a future Postgres authority
- or a local fallback when no external authority provenance is available

This slice removes the stale shared-ref naming from that path so the code and architecture story
match.

## 2. Goals

- rename startup-authority helpers so they describe coordination authority generally
- rename the coordination startup-checkpoint writer so it is not framed as a shared-ref-only path
- keep the fallback local authority semantics unchanged
- update direct callers and tests to the new names

## 3. Non-goals

- no behavior change to checkpoint contents
- no Postgres implementation work
- no public product-surface redesign beyond cleanup of internal naming

## 4. Implementation plan

1. Introduce backend-neutral helper names in `coordination_authority_api.rs`.
2. Rename the startup-checkpoint writer and authority selector in
   `coordination_startup_checkpoint.rs`.
3. Update the coordination materialized store and shared-ref tests to call the new helper names.
4. Re-run targeted `prism-core` tests for startup checkpoints and shared-ref startup authority.

## 5. Exit criteria

- no startup-checkpoint helper path in `prism-core` implies shared-ref-only authority semantics
- startup checkpoint persistence still records the same authority provenance as before
- targeted `prism-core` validation for startup checkpoint and shared-ref persistence passes

## 6. Result

Completed.

The startup-checkpoint helper path now uses backend-neutral coordination-authority naming in the
authority API, startup-checkpoint writer, and workspace-startup checkpoint plumbing.

Validation run:

- `cargo test -p prism-core --lib`
