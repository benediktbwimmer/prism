# Query And Spec Platform Shortcuts Phase 3

Status: partially implemented
Audience: query, spec-engine, MCP, CLI, and service maintainers
Scope: remove the most obvious remaining platform shortcuts in query and spec surfaces so those crates consume shared platform owners instead of rebuilding storage and refresh workflows inline

---

## 1. Summary

This spec is the concrete implementation target for Phase 3 of:

- [../roadmaps/2026-04-09-platform-seam-follow-through.md](../roadmaps/2026-04-09-platform-seam-follow-through.md)

The previous phase made the service shell and initial role owners explicit. The next cleanup is
smaller but important: some surface crates still act like platform owners.

The clearest remaining shortcuts are:

- CLI and MCP spec surfaces still choose the spec materialized-db path themselves
- CLI and MCP spec surfaces still refresh spec materialization themselves
- CLI and MCP spec surfaces still construct the query engine directly for every call
- some coordination query entrypoints still reach for broad `Prism` access when a narrower shared
  seam would be cleaner

This phase starts by removing the duplicated spec-surface composition path, then tightens the
remaining query shortcuts.

## 2. Status

Current state:

- [x] service-shell and role-owner seams are explicit in the current host process
- [x] `prism-spec` owns source discovery, parsing, materialization, refresh, and query semantics
- [x] CLI and MCP no longer rebuild the same spec-surface path and refresh flow inline
- [ ] some query entrypoints are still broader than they need to be

Current phase notes:

- start with the spec-surface path because it is duplicated and low-risk
- do not move spec semantics back out of `prism-spec`
- keep coordination/spec joins explicit
- Slice 1 is now landed:
  - `WorkspaceSpecSurface` is the named `prism-spec` workspace-facing owner for default
    materialized-db path selection, refresh-on-read, and query-engine composition
  - CLI `specs` commands now consume that owner instead of spelling out store path, refresh, and
    engine construction inline
  - MCP native spec surfaces now consume that owner instead of spelling out store path, refresh,
    and engine construction inline
- Slice 2 is now partially landed:
  - linked-spec enrichment for plan/task query reads is now centralized in shared `spec_surface`
    helpers
  - `query_runtime` and `host_resources` no longer open-code the same plan/task plus linked-spec
    join path inline
  - the next targeted shortcut cluster is now landed:
    - plan-shaped MCP resource reads now go through one shared `plan_surface` boundary
    - `host_resources` no longer open-codes coordination snapshot filtering and linked-plan
      summary composition inline for plan list/detail resources
  - the next targeted shortcut cluster is now landed:
    - UI plan list/detail entry generation now also goes through the shared `plan_surface`
    - `ui_read_models` no longer rebuilds the all-plans list from broad `Prism` access for those
      plan-shaped views
  - the next targeted shortcut cluster is now landed:
    - `task_journal` now owns the shared replay-plus-journal load path for surface reads
    - `host_resources` task resources and UI current-task journal reads no longer rebuild that
      replay-plus-journal composition inline
  - the next targeted shortcut cluster is now landed:
    - task-shaped MCP resource assembly now goes through one shared `task_surface` boundary
    - `host_resources` no longer owns inline task metadata and task resource payload assembly

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)

This spec follows:

- [2026-04-09-service-shell-and-role-owners-phase-3.md](2026-04-09-service-shell-and-role-owners-phase-3.md)

## 4. Scope

This phase includes:

- one explicit `prism-spec` workspace-facing owner for:
  - default spec materialized-db path resolution
  - refresh-on-read materialization
  - query-engine construction
- moving CLI and MCP spec surfaces onto that owner
- tightening the most obvious remaining query helpers that still over-own platform composition

This phase does not include:

- changing spec semantics
- making spec materialization service-owned yet
- changing spec storage format
- redesigning the coordination query engine

## 5. Non-goals

This phase should not:

- spread spec path conventions across more surface crates
- introduce a second spec query stack beside `prism-spec`
- collapse spec queries into coordination queries
- turn this cleanup into a large crate-graph rewrite

## 6. Design

### 6.1 Spec workspace owner rule

`prism-spec` should own the default workspace-facing composition for:

- selecting the default materialized-db path
- refreshing materialization against repo-local specs
- constructing the materialized spec query engine

CLI and MCP should consume that owner instead of spelling out those steps themselves.

### 6.2 Surface-consumer rule

Surface crates may still format results for humans or transport views, but they should not own:

- spec storage-path policy
- spec materialization refresh orchestration
- spec query-engine construction

### 6.3 Query-shortcut rule

After the shared spec surface lands, remaining query cleanup in this phase should prefer:

- shared narrow query helpers
- explicit join surfaces

over adding new direct `Prism`-shaped shortcuts in UI or transport layers.

## 7. Implementation slices

### Slice 1: Extract a shared spec workspace surface

- add one `prism-spec` owner for default workspace-backed refresh and query composition
- move CLI and MCP spec surfaces onto it

Exit criteria:

- CLI and MCP no longer choose spec DB paths or run refresh/query composition inline

### Slice 2: Tighten remaining query convenience shortcuts

- identify the highest-leverage broad query helpers still living in surface crates
- move them onto narrower shared helpers where practical

Exit criteria:

- the worst remaining query shortcuts are gone or clearly bounded

## 8. Validation

Minimum validation for this phase:

- targeted `prism-spec` tests for the new workspace-facing owner
- targeted `prism-cli` tests for `specs` commands
- targeted `prism-mcp` tests for spec query surfaces
- `git diff --check`

Important regression checks:

- spec root resolution still respects repo-local config
- spec refresh still derives coverage and sync provenance correctly
- CLI and MCP spec reads still return the same visible results

## 9. Completion criteria

This phase is complete only when:

- duplicated spec-surface composition is gone from CLI and MCP
- the remaining query shortcuts addressed by this phase are either removed or clearly bounded

## 10. Implementation checklist

- [x] Add a shared `prism-spec` workspace-facing surface
- [x] Move CLI spec commands onto that surface
- [x] Move MCP spec queries onto that surface
- [x] Tighten the first targeted query shortcut cluster around linked-spec enrichment
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
