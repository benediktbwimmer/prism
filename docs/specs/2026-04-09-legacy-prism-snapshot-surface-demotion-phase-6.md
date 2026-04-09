# Legacy Prism Snapshot Surface Demotion Phase 6

Status: completed
Audience: coordination, query, runtime, MCP, and watch-loop maintainers
Scope: remove the normal-sounding public `Prism::coordination_snapshot()` surface and replace it with an explicitly legacy continuity-snapshot name so the public coordination surface no longer advertises the continuity model as primary state

---

## 1. Summary

After the session-read cleanup, one public continuity-model escape hatch still remained:

- `Prism::coordination_snapshot()`

Even though the active coordination product surface is already v2-first, that method still made the
legacy continuity snapshot look like the canonical way to inspect coordination state.

That naming is no longer acceptable for Phase 6.

This slice demotes that surface explicitly:

- the old public name is deleted
- the remaining continuity-snapshot accessor is renamed to `legacy_coordination_snapshot()`
- production callers that still need continuity semantics now opt into that name explicitly

## 2. Required changes

- delete `Prism::coordination_snapshot()`
- expose the remaining continuity-snapshot accessor only as
  `Prism::legacy_coordination_snapshot()`
- update production callers in `prism-core` that still need continuity-snapshot semantics
- update test callers in `prism-query`, `prism-core`, and `prism-mcp`
- keep `coordination_snapshot_v2()` as the normal public coordination snapshot surface

## 3. Non-goals

- do not remove continuity-snapshot storage from the runtime engine in this slice
- do not rewrite persistence, mutation replay, or Git backend internals that still depend on the
  continuity snapshot
- do not change the canonical v2 read surface in this slice

## 4. Outcome

Completed.

The public `Prism` coordination surface no longer presents the continuity snapshot as normal state:

- `Prism::coordination_snapshot()` is gone
- callers that still need the continuity model now use `legacy_coordination_snapshot()`
- v2 remains the only normal public snapshot surface through `coordination_snapshot_v2()`
