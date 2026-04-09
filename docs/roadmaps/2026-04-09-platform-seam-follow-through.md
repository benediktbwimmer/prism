# Platform Seam Follow-Through

Status: in progress
Audience: coordination, service, storage, MCP, CLI, and spec-engine maintainers
Scope: finish the remaining architecture-adoption work called out by the post-cutover code review so the codebase fully uses the seams it already defined

---

## 1. Summary

The coordination abstractions and native spec engine are now real in code, but the latest review
identified a narrower set of remaining platform-shape mismatches:

- authority-store dependency inversion is not complete in live product-facing code
- the service shell and initial role owners are still too implicit
- the coordination query engine is still more `Prism`-centric than store-centric
- spec refresh/materialization ownership is still mostly request-driven in surface crates
- transitional compatibility facades still blur the final steady-state shape

This roadmap exists to close those gaps without reopening settled semantics.

This roadmap follows:

- [2026-04-09-abstraction-adoption-and-service-state-cleanup.md](2026-04-09-abstraction-adoption-and-service-state-cleanup.md)

## 2. Status

Current phase checklist:

- [x] Phase 1: adopt a named authority-store owner in live product-facing code
- [x] Phase 2: make the service shell and role owners explicit
- [ ] Phase 3: reduce remaining query and spec-engine platform shortcuts
- [ ] Phase 4: delete or narrow transitional wrappers and document the steady state

Current active phase:

- Phase 3: reduce remaining query and spec-engine platform shortcuts

Current phase spec:

- [../specs/2026-04-09-query-and-spec-platform-shortcuts-phase-3.md](../specs/2026-04-09-query-and-spec-platform-shortcuts-phase-3.md)

Current phase assessment:

- Phase 1 is complete:
  - a named `CoordinationAuthorityStoreProvider` now owns live authority-store opening
  - live product-facing code no longer calls `open_default_coordination_authority_store(...)`
    directly
  - trust and peer-runtime surfaces now depend on the named provider boundary
- Phase 2 is complete:
  - Slice 1 is now landed:
    - `WorkspaceServiceShell` is the named workspace-backed service-shell owner
    - `QueryHost` no longer owns runtime binding and restored session-seed lifecycle directly
  - read-broker ownership is now landed:
    - `WorkspaceReadBroker` owns the workspace-backed coordination read surface
    - `QueryHost` now delegates coordination reads through that broker in workspace-backed mode
  - mutation-broker ownership is now landed:
    - `WorkspaceMutationBroker` owns the workspace-backed coordination mutation surface
    - `QueryHost` now delegates coordination mutations through that broker in workspace-backed
      mode
  - authority-sync ownership is now landed:
    - `WorkspaceAuthoritySyncOwner` owns the workspace-backed refresh and authority-sync
      orchestration surface
    - `QueryHost` now delegates workspace refresh and read/mutation refresh sync through that owner
    - startup descriptor publication no longer bypasses the service shell from `lib.rs`
- next up is Phase 3: reduce remaining query and spec-engine platform shortcuts
  - first target is now landed:
    - extract a shared `prism-spec` workspace-facing surface
    - move CLI and MCP spec reads onto it so those crates stop owning spec path and refresh
      composition inline
  - remaining target:
    - tighten the highest-leverage remaining broad query helpers so the worst platform shortcuts are
      gone or clearly bounded
  - next landed shortcut cluster:
    - linked-spec enrichment for plan/task reads is now centralized in shared helpers
    - `query_runtime` and resource reads no longer duplicate that join logic inline
  - next landed shortcut cluster:
    - MCP plan list/detail resource composition now goes through one shared `plan_surface`
      boundary
    - `host_resources` no longer open-codes snapshot filtering plus linked-plan summary
      composition inline for plan-shaped resources
  - next landed shortcut cluster:
    - UI plan list/detail entry generation now also goes through the shared `plan_surface`
    - `ui_read_models` no longer rebuilds the all-plans list from broad `Prism` access for those
      plan-shaped views
  - next landed shortcut cluster:
    - `task_journal` now owns the shared replay-plus-journal load path for surface reads
    - `host_resources` task resources and UI current-task journal reads no longer rebuild that
      replay-plus-journal composition inline
  - next landed shortcut cluster:
    - task-shaped MCP resource assembly now goes through one shared `task_surface` boundary
    - `host_resources` no longer owns inline task metadata and task resource payload assembly

## 3. Ordering thesis

The review findings have a clear order:

1. first, stop live product-facing code from reaching for the default authority opener directly
2. then make the service shell and initial role owners explicit on top of that settled access path
3. only then reduce remaining query/spec convenience shortcuts
4. finally, remove transitional compatibility layers and rewrite the docs to the new steady state

If the service shell grows before authority access ownership is explicit, the host will harden
around the wrong dependency shape again.

## 4. Phases

### Phase 1: Adopt a named authority-store owner in live product-facing code

This phase introduces one explicit owner boundary for live authority access and moves product-facing
callers onto it.

This includes:

- a named authority-store owner or provider abstraction
- session-scoped or repo-scoped access through that owner instead of direct default-opener calls
- migrating the highest-leverage live surfaces first:
  - trust
  - peer runtime routing
  - published-plan regeneration and sync helpers where practical

Exit criteria:

- live product-facing code does not call the default authority opener directly
- authority access ownership is explicit enough that later backend selection can land in one place

### Phase 2: Make the service shell and role owners explicit

This phase extracts one explicit service shell plus named authority-sync, read-broker, and
mutation-broker owners.

Exit criteria:

- the current host process has one obvious composition owner
- product surfaces depend on named role owners instead of broad session/runtime wiring where the
  role seam exists

### Phase 3: Reduce remaining query and spec-engine platform shortcuts

This phase addresses the remaining review feedback on platform ergonomics.

This includes:

- reducing `CoordinationQueryEngine` dependence on broad `Prism` access where practical
- making spec refresh/materialization ownership more explicit instead of fully request-driven in
  CLI and MCP surface crates
- keeping coordination/spec joins explicit while shrinking repeated surface adapters

Exit criteria:

- query and spec surfaces look like platform consumers rather than side-path materializers

### Phase 4: Delete or narrow transitional wrappers and document the steady state

This phase removes the last migration-only helpers and updates the docs to match the final shape.

Exit criteria:

- transitional compatibility facades are either gone or clearly bounded and documented
- contracts, specs, and architecture notes all tell the same steady-state story

## 5. Dependency logic

This ordering is driven by the review itself:

- authority dependency inversion comes before service-shell extraction
- service-shell extraction comes before query/spec cleanup
- query/spec cleanup comes before deleting the last compatibility bridges

## 6. Anti-patterns to avoid

Do not:

- add a second ad hoc authority-open path beside the named owner boundary
- let the service shell directly own coordination semantics
- leave CLI or MCP crates responsible for spec persistence logic long-term
- “fix” backend leakage by creating thin alias wrappers that still hardcode default backend access

## 7. Short form

The order to stake the cleanup on is:

1. named authority-store owner
2. explicit service shell and role owners
3. query/spec platform cleanup
4. delete transitional bridges and document the steady state
