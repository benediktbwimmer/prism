# Spec Coordination Linking And Sync Provenance Phase 11

Status: in progress
Audience: coordination, spec-engine, query, MCP, CLI, and storage maintainers
Scope: complete roadmap Phase 11 by adding explicit typed spec-link fields to authoritative coordination objects and deriving local spec sync-provenance records from those links plus local spec state

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 11:

- make spec-to-coordination linkage explicit in the authoritative coordination model
- stop treating spec linkage as an implied convention or opaque metadata blob
- populate queryable local sync provenance from those explicit authoritative links

This phase is the bridge between:

- local branch-sensitive spec intent
- authoritative shared coordination objects

It does not yet implement full coverage computation or high-level sync actions.
It establishes the typed linkage and provenance substrate those later phases require.

## 2. Status

Current state:

- [x] Phase 10 `SpecQueryEngine` is complete
- [x] plans and tasks now carry explicit typed spec links
- [x] coordination replay and mutation surfaces preserve typed plan/task spec links
- [ ] high-level transaction mutation families do not yet accept authored spec links
- [ ] local sync-provenance storage exists but is not populated from coordination
- [ ] no authoritative-to-local derivation path exists for spec sync provenance
- [ ] coordination-plus-spec join queries still lack real linkage data

Current slice notes:

- this phase must touch the coordination domain model, not just the spec crate
- the key design goal is one explicit authoritative linkage shape, not a second hidden metadata
  channel
- local sync provenance should be derived from authoritative links where possible rather than
  mutated independently as a parallel source of truth
- Slice 1 is complete on this branch: the coordination domain, replay path, and direct mutation
  inputs now all understand typed plan/task spec links
- the next remaining work is Slice 2 completion for authored transaction inputs, followed by Slice 3
  sync-provenance derivation in `prism-spec`

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 11: implement explicit spec-coordination linking and sync provenance

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/provenance.md](../contracts/provenance.md)

This spec builds on:

- [2026-04-09-spec-query-engine-phase-10.md](2026-04-09-spec-query-engine-phase-10.md)

## 5. Scope

This phase includes:

- explicit typed spec-link fields on authoritative coordination plans and tasks
- source spec revision tracking on those links
- checklist-item and optional section identity tracking for task-level coverage links
- derivation of local spec sync-provenance records from authoritative coordination links
- query visibility for linked specs on coordination objects and linked coordination refs on specs

This phase does not include:

- agent-facing high-level create-plan-from-spec or create-tasks-from-spec actions
- full `SpecCoverageView` computation
- task readiness changes driven by local spec posture
- automatic back-sync from task completion to markdown checkboxes

## 6. Non-goals

This phase should not:

- hide spec links inside untyped `metadata`
- create a second local-only mutable provenance channel parallel to authoritative coordination data
- infer coordination links from fuzzy text matching
- make local spec dependencies authoritative blockers by themselves

## 7. Design

### 7.1 Authoritative-link rule

Spec linkage must be an explicit typed part of authoritative coordination objects.

Initial target:

- plans gain `spec_refs`
- tasks gain `spec_refs`

These fields must be first-class serialized coordination data, not opaque JSON in `metadata`.

### 7.2 Link-shape rule

The authoritative link family should distinguish:

- generic linkage to one spec
- task-level coverage of exact checklist items and optional section identities

Initial target shape:

- `CoordinationSpecRef`
  - `spec_id`
  - `source_path`
  - `source_revision`
- `TaskSpecRef` or equivalent
  - the base spec-ref fields
  - `sync_kind`
  - `covered_checklist_items`
  - optional `covered_sections`

Plans may use generic `spec_refs`.
Tasks must carry the exact checklist-item coverage they represent when created from or synced to a
spec.

### 7.3 Provenance-derivation rule

The local spec materialized store should not become a second writable provenance authority.

Instead:

- authoritative plans/tasks carry the explicit spec links
- local spec sync-provenance records are derived from those authoritative objects plus local spec
  state during materialization or refresh

This keeps the truth model clean:

- coordination remains authoritative for coordination linkage
- local spec storage remains a derived local query surface

### 7.4 Source-revision rule

Every explicit authoritative spec link must capture the source spec revision used when the link was
created or updated.

Minimum field:

- `source_revision`

This is required for later drift detection.

### 7.5 Query-join rule

Once explicit links exist, query seams may expose stable joins such as:

- `task_linked_specs(task_id)`
- `task_spec_status(task_id)`
- `plan_spec_coverage(plan_id)`

This phase only establishes the data needed for those joins.
Phase 12 computes the richer coverage story on top of it.

## 8. Implementation slices

### Slice 1: Add typed spec-link domain types

- add typed coordination-domain structs for plan/task spec links
- add serialized fields to plans and tasks
- preserve backward compatibility through `#[serde(default)]`

Exit criteria:

- authoritative coordination objects can carry explicit typed spec links

Status:

- [x] complete

### Slice 2: Thread spec links through mutation surfaces

- extend canonical transaction inputs and convenience mutation adapters to accept spec links
- preserve existing behavior when spec links are absent

Exit criteria:

- explicit plan/task creation and update flows can write authoritative spec links

Status:

- [ ] lower-level coordination mutation inputs are updated
- [ ] high-level transaction mutation families still need explicit spec-link fields

### Slice 3: Derive local sync provenance from authoritative links

- populate spec sync-provenance records during local spec materialization refresh or equivalent
- derive records from linked plans/tasks instead of direct local writes

Exit criteria:

- local spec queries can explain which authoritative objects came from which spec revision and
  checklist identities

## 9. Validation

Minimum validation for this phase:

- targeted coordination-domain tests for serialization, replay, and mutation preservation of spec
  links
- targeted `prism-spec` tests for sync-provenance derivation from linked coordination objects
- targeted downstream `prism-query` / `prism-core` tests if public transaction inputs or snapshot
  shapes change
- `git diff --check`

Important regression checks:

- old coordination snapshots deserialize cleanly with default empty spec links
- new links survive event replay and read-model generation
- local sync-provenance derivation does not invent links when coordination has none
- direct downstream `prism-query` and `prism-core` suites stay green after the new required fields
  are added

## 10. Completion criteria

This phase is complete only when:

- plans and tasks can carry typed explicit spec links
- task links can identify exact covered checklist items
- local sync-provenance records are derived from authoritative coordination links
- later coverage and join phases can build on those stable fields without redesign

## 11. Implementation checklist

- [ ] Add typed coordination-domain spec link fields
- [ ] Thread spec links through transaction and convenience mutation inputs
- [ ] Preserve backward-compatible serialization defaults
- [ ] Derive local sync-provenance records from authoritative links
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
