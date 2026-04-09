# Spec Coverage Phase 12

Status: draft
Audience: spec-engine, coordination, query, MCP, CLI, and UI maintainers
Scope: implement the first full `SpecCoverageView` by deriving stable local checklist-coverage posture from explicit coordination spec links plus local spec state

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 12:

- build the first real `SpecCoverageView`
- compute checklist-item coverage from explicit authoritative task links
- expose the resulting coverage posture through the `SpecQueryEngine`

This phase closes the loop from:

- local branch-sensitive implementation intent
- explicit authoritative task and plan links
- queryable local understanding of what the spec is already covered by coordination

This phase does not yet add high-level create-plan-from-spec actions or automatic back-sync into
markdown.

## 2. Status

Current state:

- [x] Phase 11 is complete
- [x] plans and tasks now carry explicit typed spec links
- [x] local sync provenance is derived from authoritative coordination links
- [ ] coverage records remain empty scaffolding only
- [ ] no checklist-level uncovered versus represented posture is computed yet
- [ ] no drift posture is computed yet

Current slice notes:

- this phase should compute coverage from authoritative coordination links, not from local-only
  annotations
- initial coverage should stay deterministic and structural
- richer drift and review-backed rollups may start in this phase, but must not force speculative
  heuristics

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 12: implement `SpecCoverageView` fully

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)

This spec builds on:

- [2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md](2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md)

## 5. Scope

This phase includes:

- deriving coverage records from explicit authoritative task spec links
- representing at least:
  - uncovered checklist items
  - checklist items represented by one or more coordination tasks
  - checklist items represented only by stale spec revisions
- preserving stable task references in coverage records
- exposing populated coverage views through the `SpecQueryEngine`

This phase may also include, if cheap and structurally clean:

- review-backed coverage posture based on linked task evidence state
- plan-level coverage rollups derived from checklist coverage

This phase does not include:

- agentic create-plan-from-spec or create-tasks-from-spec actions
- automatic markdown checkbox mutation
- making local spec coverage authoritative for task readiness

## 6. Non-goals

This phase should not:

- infer coverage from fuzzy task titles or descriptions
- treat plan-level links alone as checklist-item coverage
- make branch-local coverage posture block authoritative coordination execution
- add speculative NLP matching between specs and coordination objects

## 7. Design

### 7.1 Checklist-coverage rule

Checklist-item coverage must be derived from explicit task-level `covered_checklist_items`.

Plan-level `spec_refs` may support plan summaries later, but they are not enough to claim a
checklist item is covered.

### 7.2 Coverage-kind rule

The materialized coverage record family should begin with a small explicit vocabulary:

- `represented`
- `stale_revision`
- `uncovered`

Additional richer kinds can be added later only if they remain deterministic.

### 7.3 Uncovered-rule

For every required checklist item in a materialized spec:

- if no authoritative task link covers it, emit `uncovered`
- informational items may be omitted from uncovered records or explicitly marked informational-only,
  but they must not degrade the required coverage posture

### 7.4 Stale-revision rule

If a task covers a checklist item but references an older `source_revision` than the local spec
materialization, coverage should preserve that distinction explicitly as `stale_revision`.

### 7.5 Query rule

`SpecQueryEngine::coverage(spec_id)` must return real populated coverage records once a spec has
linked coordination state.

The query layer should not recompute checklist coverage ad hoc in readers.

## 8. Implementation slices

### Slice 1: Define checklist-coverage derivation inputs

- finalize the minimal coverage-kind vocabulary
- define how local spec source revision and authoritative link revision interact
- make any needed store-type adjustments for stable task/checklist coverage rows

Exit criteria:

- the coverage derivation model is explicit and testable

### Slice 2: Populate materialized coverage records

- derive coverage rows during canonical spec materialization refresh
- emit uncovered rows for required unchecked coverage gaps
- emit represented or stale-revision rows from authoritative task links

Exit criteria:

- local materialization writes real coverage rows instead of leaving the table empty

### Slice 3: Expose coverage through the query engine

- extend query tests to assert stable `SpecCoverageView` behavior
- keep not-found behavior explicit and deterministic

Exit criteria:

- spec queries can answer what checklist items are covered, stale, or uncovered

## 9. Validation

Minimum validation for this phase:

- targeted `prism-spec` tests for coverage derivation from authoritative task links
- targeted `prism-spec` tests for uncovered and stale-revision posture
- targeted downstream `prism-core` tests if public spec surfaces change
- `git diff --check`

Important regression checks:

- plan-only spec links do not falsely mark checklist items as covered
- informational items do not incorrectly degrade required coverage posture
- multiple task links to one checklist item remain stable and deterministic
- absent coordination links still yield explicit uncovered posture for required items

## 10. Completion criteria

This phase is complete only when:

- spec materialization writes real populated coverage rows
- `SpecCoverageView` is backed by derived data, not empty scaffolding
- checklist items can be distinguished as represented, stale, or uncovered
- later sync-action phases can build on the coverage surface without redesign

## 11. Implementation checklist

- [ ] Define coverage derivation inputs and vocabulary
- [ ] Populate materialized coverage records from explicit task links
- [ ] Preserve explicit uncovered posture for required items
- [ ] Expose populated coverage through the query engine
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
