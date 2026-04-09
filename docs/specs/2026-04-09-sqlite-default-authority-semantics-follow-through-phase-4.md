# SQLite Default Authority Semantics Follow-Through Phase 4

Status: completed
Audience: coordination, authority, persistence, and prism-core maintainers
Scope: reconcile legacy shared-ref and persistence assumptions with the SQLite-default `CoordinationAuthorityStore` path so the full workspace suite matches the intended steady-state semantics

---

## 1. Summary

Phase 3 made SQLite the default coordination authority backend.

The remaining mismatch is no longer the backend implementation itself. It is the expectation surface
around it:

- shared-ref tests still assume the Git backend is the implicit authority default
- several `prism-core` tests still assume authoritative coordination only exists when shared refs
  are published
- several persistence tests still read read-models and startup checkpoints from the old local
  store locations instead of the repo-shared coordination materialization store

This slice fixes those mismatches without weakening the architecture.

The rule is:

- shared-ref behavior stays covered, but only under an explicit Git authority configuration
- backend-neutral tests should now assert the SQLite-default authority semantics directly
- coordination materialization assertions should target the settled materialization store

## 2. Goals

- make shared-ref tests explicit about requiring the Git authority backend
- update backend-neutral persistence tests to the SQLite-first authority model
- stop tests from asserting that strong reads are unavailable merely because shared-ref authority
  is absent
- align startup-checkpoint and read-model assertions with the repo-shared coordination
  materialization DB
- preserve the distinction between authoritative state and materialized derived state

## 3. Non-goals

- no Postgres implementation work
- no new product-facing authority contract changes
- no restoration of Git shared refs as the default authority backend

## 4. Implementation

1. Add a shared-ref test helper that explicitly configures the Git authority backend for the test
   repo/worktree surface.
2. Update shared-ref tests to rely on that explicit configuration instead of the historical default.
3. Update generic coordination tests so authoritative reads expect SQLite-backed state by default.
4. Update persistence and checkpoint tests to read from the coordination materialization store
   rather than the old worktree-local cache assumptions.
5. Re-run the previously failing targeted tests and then the full workspace suite.

Implemented result:

- shared-ref tests now opt into the Git authority backend explicitly through repo-local test
  configuration
- backend-neutral persistence tests now assert SQLite-default authority behavior directly
- coordination materialization assertions now target the settled repo-shared materialization store
  or the strong authority-read path instead of legacy shared-runtime assumptions
- the full `cargo test` workspace suite passes again on the SQLite-default authority branch state

## 5. Exit criteria

- all shared-ref tests pass while using an explicit Git authority configuration
- the SQLite-default read and materialization semantics are asserted directly in backend-neutral
  tests
- the full workspace `cargo test` suite passes again on the SQLite-default branch state
