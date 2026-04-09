# Authority Publication Operation Naming Follow-Through Phase 4

Status: completed
Audience: coordination, persistence, docs-export, MCP, and test-infrastructure maintainers
Scope: remove remaining shared-ref-shaped naming from backend-neutral authority-publication traces,
test opt-in markers, skip reasons, and repo-state export wording

---

## 1. Summary

SQLite is already the default coordination authority backend, but a small set of backend-neutral
surfaces still describe authority publication as if it were inherently a shared-ref operation.

The main remaining leaks were:

- trace operation names such as `syncSharedCoordinationRef`
- skip reasons such as `shared_ref_authority_only`
- the test-only opt-in marker `enable_shared_coordination_ref_publish`
- repo-state export wording that still says "shared coordination ref when present"

This slice cleans those up without changing Git-backend behavior.

## 2. Goals

- rename backend-neutral authority-publication trace operations to authority-oriented names
- rename the test-only publication opt-in marker and environment variable to coordination-authority
  wording, while preserving legacy compatibility
- replace shared-ref-shaped skip reasons in backend-neutral publication code
- update repo-state export wording to match the SQLite-default authority model
- keep focused `prism-core` and `prism-mcp` validation green

## 3. Non-goals

- no Postgres implementation work
- no rewrite of Git-backend internals or Git-specific diagnostics wording
- no bulk regeneration of archived plan-doc exports

## 4. Implementation

This slice updates:

- `coordination_persistence.rs`
- `published_plans.rs`
- `prism_doc/repo_state.rs`
- trace assertions in `prism-core` and `prism-mcp`
- the shared test helper marker path in `prism-mcp`

The settled backend-neutral names are:

- `mutation.coordination.authority.applyTransaction`
- `mutation.coordination.syncDerivedState`
- `enable_coordination_authority_publication`
- `PRISM_TEST_DISABLE_COORDINATION_AUTHORITY_PUBLICATION`

Legacy test marker and env-var names remain accepted as compatibility fallbacks inside
`coordination_persistence.rs`.

One additional trace boundary is now explicit:

- git-execution steps that deliberately use the no-materialization coordination path still emit a
  nested `scheduleMaterialization` marker, but that marker reflects deferred materialization
  scheduling rather than a suppressed inline-materialization attempt
- git-execution materialization trace assertions should treat `scheduleMaterialization` as a stable
  phase boundary and avoid depending on legacy payload shapes such as `suppressed: true`

## 5. Exit criteria

- backend-neutral authority-publication traces no longer use shared-ref-shaped names
- test-only authority-publication opt-in surfaces use authority-neutral names
- repo-state export no longer describes the default authority path as "shared coordination ref when
  present"
- git-execution trace assertions use stable phase-boundary checks instead of depending on stale
  materialization payload details
- focused `prism-core` and `prism-mcp` validation passes

## 6. Validation

- `cargo test -p prism-core --lib`
- `cargo test -p prism-core shared_coordination_ref::tests::shared_coordination_ref_pushes_to_origin_and_reloads_from_remote`
- `cargo test -p prism-mcp git_execution_completion_trace_records_subphases_without_ui_publish`
- `cargo test -p prism-mcp coordination_mutation_trace_records_persistence_subphases`
- `cargo test -p prism-mcp runtime_status_omits_shared_coordination_ref_diagnostics_on_sqlite_default`
