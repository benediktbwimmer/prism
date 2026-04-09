# Canonical Prism Constructor Surface Follow-Through Phase 6

Status: completed
Audience: query, core, MCP, and coordination-runtime maintainers
Scope: remove the remaining public `Prism` constructor aliases that quietly synthesize canonical
coordination state from legacy continuity snapshots

---

## 1. Summary

Phase 6 already removed the public legacy snapshot read surfaces.

The next matching cleanup is the constructor surface itself. `Prism` still exposed public builder
entrypoints that accepted only `CoordinationSnapshot` and silently derived canonical v2 state
inside the constructor. That keeps migration-era behavior on the public API even after the rest of
the product surface is explicitly v2-first.

This slice removes those constructor aliases and makes public coordination-bearing `Prism`
construction explicitly v2-aware.

## 2. Required changes

- delete the public `Prism::with_history_outcomes_coordination_and_projections(...)` constructor
- delete the public `Prism::with_shared_history_outcomes_coordination_projections_and_query_state(...)`
  alias
- keep only the explicit v2-aware public builders for coordination-bearing `Prism` construction
- update the remaining production caller in `prism-core` to pass canonical coordination state
  explicitly
- move unit-test construction onto a local helper that calls the v2-aware builder explicitly

## 3. Non-goals

- do not remove the legacy continuity runtime from the mutation engine in this slice
- do not redesign `MaterializedCoordinationRuntime`
- do not change SQLite materialization formats yet

## 4. Exit criteria

- no production code calls the deleted legacy `Prism` constructor aliases
- public coordination-bearing `Prism` builders require explicit canonical v2 state
- targeted `prism-query`, `prism-core`, and `prism-mcp` tests pass
