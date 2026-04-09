# Backend-Neutral Wording Residue Cleanup Phase 4

Status: completed
Audience: coordination, storage, and docs maintainers
Scope: remove the last backend-neutral shared-ref wording residue from non-Git comments and test
assertion text, then record that the remaining shared-ref terminology is intentional compatibility
or Git-specific behavior

---

## 1. Summary

After the authority-adoption cleanup slices, the remaining non-backend code is almost entirely in a
good state. The only leftover shared-ref wording in backend-neutral paths is minor residue:

- a tracked-snapshot comment still says coordination state "lives in shared refs"
- one SQLite-default strong-read assertion still frames the steady state in terms of avoiding
  "shared-ref publication"

This slice removes those last neutral-path wording leaks and clarifies that what remains elsewhere
is intentional compatibility or Git-specific coverage.

## 2. Goals

- remove shared-ref terminology from backend-neutral comments
- remove shared-ref terminology from backend-neutral SQLite-default assertion text
- leave intentional Git-specific and compatibility wording untouched

## 3. Non-goals

- no behavior change
- no Git-backend wording cleanup inside Git-specific tests or diagnostics
- no Postgres work

## 4. Implementation

This slice updates:

- `tracked_snapshot.rs`
- `tests.rs` in `prism-core`

It also updates the roadmap checkpoint trail so the remaining shared-ref terms are explicitly
understood as intentional compatibility or Git-specific surfaces.

## 5. Exit criteria

- backend-neutral code comments no longer describe coordination authority generically as shared refs
- backend-neutral SQLite-default assertions no longer describe the steady state in shared-ref terms
- roadmap notes make the remaining shared-ref terms easier to interpret correctly

## 6. Validation

- `cargo test -p prism-core coordination_authority_api`
- `python3 scripts/update_doc_indices.py --check`
