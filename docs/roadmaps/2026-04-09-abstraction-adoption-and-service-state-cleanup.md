# Abstraction Adoption And Service-State Cleanup

Status: in progress
Audience: coordination, service, runtime, storage, MCP, CLI, and spec-engine maintainers
Scope: finish adopting the new coordination abstractions in product code, move coordination materialization toward service-owned state, and remove the remaining architectural mismatches called out by the post-cutover review

---

## 1. Summary

The coordination abstractions and native spec engine are now real in code, but the repo still has a
set of visible transitional mismatches:

- some backend and authority concerns still leak outward through compatibility facades
- coordination materialization is still implemented on the worktree-local cache path
- the service host is not yet explicit enough about role ownership
- the query and spec layers still carry a few request-driven or `Prism`-centric shortcuts

This roadmap exists to make the architecture match the seams we already defined.

The point is not to redesign the contracts again.
The point is to fully use them.

## 2. Status

Current phase checklist:

- [ ] Phase 1: finish authority-store adoption and remove remaining backend leakage
- [x] Phase 2: move coordination materialization to service-owned repo-shared storage
- [ ] Phase 3: make the service shell and initial roles explicit
- [ ] Phase 4: reduce remaining `Prism`-centric query shortcuts and refresh-driven spec shortcuts
- [ ] Phase 5: close residual compatibility wrappers and document the new steady state

Current active phase:

- Phase 3: make the service shell and initial roles explicit

Current phase spec:

- [../specs/2026-04-09-service-shell-and-role-owners-phase-3.md](../specs/2026-04-09-service-shell-and-role-owners-phase-3.md)

Current phase assessment:

- Phase 2 is complete:
  - repo-shared coordination materialization path and bounded legacy migration are implemented
  - live coordination materialization writers now target the repo-shared store directly
  - runtime status and CLI/MCP path surfaces now distinguish runtime-local cache from
    coordination materialization
- next up is Phase 3: make the service shell and initial roles explicit

## 3. Ordering thesis

This cleanup should proceed in the same foundation-first spirit as the earlier coordination work.

The ordering is:

1. finish dependency inversion around the authority seam
2. stop treating worktree-local SQLite as the coordination materialization home
3. then tighten the service-host ownership model around those settled seams
4. only then sand away remaining convenience shortcuts in query and spec surfaces

Doing this in the opposite order would risk hiding the real architectural problems behind prettier
surface code.

## 4. Phases

### Phase 1: Finish authority-store adoption and remove backend leakage

This phase closes the gap between:

- having a real `CoordinationAuthorityStore` contract
and
- actually depending on it everywhere

This includes:

- product and runtime code no longer constructing concrete backend stores directly
- a shared authority-store opener or provider boundary
- shrinking Git-specific compatibility façades in product-facing layers

Exit criteria:

- live product code does not hardcode concrete authority backends
- backend selection lives behind one shared owner boundary

Current assessment:

- partially satisfied by the new authority-store opener seam
- remaining work is mostly compatibility-surface cleanup rather than direct constructor use

### Phase 2: Move coordination materialization to service-owned repo-shared storage

This phase makes the implementation match the service-owned coordination-materialization contract.

This includes:

- a dedicated repo-shared coordination materialization path
- migration from legacy worktree-local coordination materialization state
- coordination materialized-store code no longer reusing the generic worktree cache DB path
- diagnostics and status surfaces distinguishing worktree-local runtime cache from coordination
  materialization state

Exit criteria:

- coordination materialization is repo-shared and no longer rooted in the worktree cache path
- legacy worktree-local coordination materialization can be read or migrated forward without data
  loss

### Phase 3: Make the service shell and initial roles explicit

This phase makes the service host structure match the service contracts.

This includes:

- one explicit service shell
- explicit authority-sync, read-broker, and mutation-broker owners
- clearer host-module boundaries in the current service process

Exit criteria:

- the service host is no longer just a broad daemon surface with implicit role ownership

### Phase 4: Reduce remaining query and spec-engine shortcuts

This phase addresses the remaining review feedback on platform ergonomics.

This includes:

- reducing `CoordinationQueryEngine` dependence on direct `Prism`-object access where practical
- making spec refresh/materialization ownership more explicit instead of fully request-driven in
  surface crates
- keeping joins explicit while reducing repeated adapter logic

Exit criteria:

- the query and spec surfaces feel like true platform consumers rather than convenience side paths

### Phase 5: Close residual compatibility wrappers and document the new steady state

This phase removes the last obvious transitional helpers and updates the docs accordingly.

This includes:

- narrowing or deleting compatibility wrappers that only existed for the migration
- updating contracts, specs, and architecture docs to reflect the new steady state

Exit criteria:

- the code and docs tell the same architecture story without major transitional caveats

## 5. Dependency logic

This ordering is driven by the review findings:

- authority dependency inversion should settle before more backend work lands
- service-owned materialization should settle before the service shell grows around the wrong path
- service-role extraction should happen after the materialization owner is correct
- query/spec shortcut cleanup should happen after the storage and service owners are correct

## 6. Anti-patterns to avoid

Do not:

- introduce a second ad hoc provider path beside the authority-store opener
- keep coordination materialization in the worktree cache path “for now”
- hide service ownership problems behind more surface adapters
- let query or spec refresh surfaces grow new persistence logic
- leave old compatibility wrappers undocumented once the real seam exists

## 7. Short form

The cleanup order to stake the repo on is:

1. fully adopt the authority seam
2. move coordination materialization to repo-shared service-owned storage
3. make the service shell and initial roles explicit
4. reduce remaining query/spec shortcuts
5. delete or narrow transitional wrappers
