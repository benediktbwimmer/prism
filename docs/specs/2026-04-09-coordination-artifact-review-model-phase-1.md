# Coordination Artifact And Review Model Phase 1

Status: in progress
Audience: coordination, query, MCP, UI, and workflow maintainers
Scope: implement declared artifact and review requirements on tasks, requirement-linked artifact and review records, lineage-head selection, and completion/query surfaces that honor those declared requirements

---

## 1. Summary

Phase 1 turns the artifact/review contract into real coordination state instead of leaving
artifacts and reviews as mostly free-floating records.

This slice introduces:

- explicit task-scoped artifact requirements
- explicit task-scoped review requirements
- artifact records linked to exactly one declared artifact requirement lineage
- review records linked to exactly one declared review requirement and exactly one artifact
- lineage-head and requirement-satisfaction helpers
- completion and query surfaces that use those declared requirements

This phase is intentionally about the evidence model only.

It does not yet implement the shared execution substrate, Actions, or graph-wide dataflow.

## 2. Status

Coarse checklist:

- [x] add native artifact requirement and review requirement types
- [x] thread requirement declarations through task create and update surfaces
- [x] require artifact proposals to target declared artifact requirements
- [x] require primitive reviews to target declared review requirements
- [x] add lineage-head and requirement-satisfaction helpers
- [x] update completion blockers and review queries to use declared requirements
- [ ] surface the new model through MCP and JS view types

Progress note (2026-04-09):

- coordination-core storage, mutation, replay, blocker, and pending-review semantics are implemented in `prism-coordination`
- task updates now reject requirement changes that would invalidate retained review requirements
- MCP, query-transaction, and JS/public view wiring is the remaining Phase 1 work

## 3. Non-goals

This phase does not attempt to:

- add Actions or the shared execution substrate
- add graph-wide typed input or output bindings
- redesign event-trigger execution
- implement multi-review diversity policies beyond the declared minimum count
- introduce a second graph node type for artifacts or reviews

## 4. Related contracts and roadmap

Primary contract:

- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)

Roadmap:

- [../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md](../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md)

Relevant follow-on designs:

- [../designs/2026-04-09-warm-state-validation-feedback.md](../designs/2026-04-09-warm-state-validation-feedback.md)
- [../designs/2026-04-09-actions-and-machine-work.md](../designs/2026-04-09-actions-and-machine-work.md)
- [../designs/2026-04-09-graph-dataflow-and-parameterization.md](../designs/2026-04-09-graph-dataflow-and-parameterization.md)

## 5. Current gap

Today PRISM already has:

- artifact records
- primitive review records
- artifact supersession
- review-oriented completion blockers

But the evidence model is still missing the declared requirement layer from the contract:

- tasks do not yet declare artifact requirements explicitly
- tasks do not yet declare review requirements explicitly
- artifacts are not linked to one declared requirement lineage
- primitive reviews are not linked to one declared review requirement
- completion and pending-review queries still reason mostly from broad artifact state rather than
  requirement satisfaction

That gap makes later execution work weaker, because validation and machine execution will have no
stable requirement identity to attach evidence against.

## 6. Design

### 6.1 Task-scoped requirement declarations

`CoordinationTask` should gain two explicit declaration sets:

- `artifact_requirements`
- `review_requirements`

These are durable task-owned declarations and should round-trip through:

- task create
- task update
- coordination snapshots
- canonical graph views
- MCP and JS-facing task views

### 6.2 Artifact requirement shape

Phase 1 should introduce a durable artifact requirement object with at least:

- `client_artifact_requirement_id`
- `kind`
- `min_count`
- `evidence_types`
- `stale_after_graph_change`
- `required_validations`

For this phase:

- `min_count` should be accepted but treated as a single-lineage requirement in behavior
- concurrent active heads for one requirement remain out of scope

### 6.3 Review requirement shape

Phase 1 should introduce a durable review requirement object with at least:

- `client_review_requirement_id`
- `artifact_requirement_ref`
- `allowed_reviewer_classes`
- `min_review_count`

For this phase:

- `min_review_count` should be accepted
- reviewer diversity is out of scope unless already required somewhere else

### 6.4 Artifact linkage

`Artifact` should be linked to exactly one declared artifact requirement lineage.

That means Phase 1 should add requirement linkage directly to artifact records, for example:

- `artifact_requirement_id`

Artifact proposals should target a declared requirement explicitly.

This phase should make that the normal product model rather than continuing to treat artifacts as
ad hoc task attachments.

### 6.5 Review linkage

`ArtifactReview` should be linked to:

- exactly one artifact
- exactly one declared review requirement

That means Phase 1 should add requirement linkage directly to review records, for example:

- `review_requirement_id`

### 6.6 Lineage-head selection

The coordination layer should expose helpers that can answer:

- all artifacts in one requirement lineage
- the latest non-superseded artifact for a requirement
- whether a review requirement is satisfied for that active artifact

This phase may implement those as coordination-store helper methods or internal derivation helpers.

### 6.7 Completion and query semantics

Completion blockers should move from broad “any approved artifact exists” logic toward requirement
logic:

- artifact-required completion should check declared artifact requirements
- review-required completion should check declared review requirements against the active lineage
  head
- superseded reviews should remain historical but should not satisfy the active requirement

Pending-review queries should also shift from:

- all proposed or in-review artifacts

to:

- active lineage heads for declared review requirements that are not yet satisfied

### 6.8 Public mutation and view surfaces

This phase should update the public coordination surfaces so the requirement model is not purely
internal.

That includes:

- task create and task update payloads
- artifact propose payloads
- artifact review payloads
- task views
- artifact views
- review views where applicable

## 7. Implementation slices

### Slice A: Types and storage

Add:

- requirement structs
- task fields
- artifact and review requirement linkage fields
- snapshot and replay support

Status:

- complete in `prism-coordination`

### Slice B: Mutation inputs

Add:

- task create and update payload support for declared requirements
- artifact propose linkage to declared artifact requirement
- artifact review linkage to declared review requirement

Status:

- complete in `prism-coordination`

### Slice C: Derivations and blockers

Add:

- active lineage-head helpers
- requirement satisfaction helpers
- completion blocker updates
- pending review query updates

Status:

- complete in `prism-coordination`

### Slice D: Public views

Add:

- MCP payload conversion
- JS-facing view types
- task and artifact views that expose the declared requirement model

Status:

- in progress

## 8. Validation

Tier 1:

- `cargo test -p prism-coordination`

Tier 2:

- `cargo test -p prism-mcp`

Targeted tests should cover at least:

- task creation with declared artifact and review requirements
- artifact proposal rejected when no matching requirement exists
- artifact lineage-head selection after supersession
- review satisfaction counting only against the active artifact for the linked requirement
- completion blockers for missing required artifact or review satisfaction
- MCP mutation payload round-tripping and surface views

## 9. Rollout and migration

This phase is allowed to be a hard cutover inside the coordination v2 model.

That means:

- task declarations should move to explicit requirement objects rather than trying to stretch the
  old generic acceptance shape forever
- artifact and review mutations should target declared requirement ids explicitly once this slice
  lands

If any remaining internal call sites still rely on the older ad hoc evidence shape, update them in
the same implementation program instead of preserving a long-lived compatibility branch.

## 10. Open questions

Questions intentionally deferred to later phases:

- richer reviewer diversity policy
- multi-head artifact requirement groups
- review-pass aggregation objects beyond primitive review records
- typed artifact and review outputs for graph-wide dataflow
