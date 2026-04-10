# PRISM Coordination Query Engine

Status: normative contract  
Audience: coordination, query, runtime, MCP, CLI, UI, and future service maintainers  
Scope: deterministic evaluation of authoritative coordination state into bounded read answers

---

## 1. Goal

PRISM must have one backend-neutral **Coordination Query Engine** that evaluates coordination
state into deterministic, agent-usable answers.

The engine exists so that:

- plan and task reasoning is implemented once
- MCP, CLI, UI, local runtime, and future service layers do not reimplement workflow logic
- backend choice does not change read semantics
- local materialization changes speed, not meaning

This engine is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [coordination-artifact-review-model.md](./coordination-artifact-review-model.md)
- [spec-engine.md](./spec-engine.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)

Canonical ownership:

- this document defines deterministic evaluation of authoritative coordination state
- [spec-engine.md](./spec-engine.md) defines native spec parsing, dependency evaluation, and local
  spec query semantics
- coordination surfaces may join to spec-derived local state, but must not absorb spec semantics
  into authoritative coordination reasoning silently

## 2. Non-goals

The Coordination Query Engine is not:

- the authority backend
- the mutation broker
- the watch or sync loop
- a UI-specific view layer
- a transport-specific API surface

It evaluates coordination state. It does not decide how that state is fetched, persisted, or
mutated.

The canonical programmable public surface that reaches this engine is `prism_code`.
Separate read-only public tools are not part of the target architecture.

## 3. Core invariants

The engine must preserve these rules:

1. Equal authoritative input plus equal query parameters must produce equal results.
2. Backend choice must not change evaluation semantics.
3. Local materialization may serve reads faster, but must not change the result meaning.
4. Every result must carry enough freshness metadata to explain how current it is.
5. Query outputs must be bounded and structured; the engine must not require callers to infer
   workflow semantics from raw records.
6. Artifact, review, blocker, and lifecycle reasoning must come from this engine or one of its
   direct submodules, not from UI- or transport-specific ad hoc code.
7. Local spec-derived enrichment may be attached to coordination answers, but the answer must remain
   able to distinguish authoritative coordination conclusions from local spec context.

## 4. Required responsibilities

The Coordination Query Engine must evaluate at least:

- task lifecycle state
- task actionability
- task blockers and blocker reasons
- plan-derived status and rollups
- required artifact satisfaction
- required review satisfaction
- review scope resolution
- task evidence status
- pending review work
- stale, reclaimable, or review-pending work
- dependency and dependent views

When a product surface asks for coordination-plus-spec context, the coordination engine may expose
explicit join families that incorporate local spec summaries from the spec engine.
Those joins remain coordination reads with local enrichment, not a new source of authoritative task
truth.

## 5. Canonical query families

The engine should expose query families rather than UI-specific projections.

### 5.1 Object reads

- `plan(plan_id)`
- `task(task_id)`
- `artifact(artifact_id)`
- `review(review_id)`

These are canonical object reads with coordination semantics attached where needed.

### 5.2 Task evaluation reads

- `task_status(task_id)`
- `task_blockers(task_id)`
- `task_actionability(task_id)`
- `task_evidence_status(task_id)`
- `task_review_scope(task_id)`
- `task_review_targets(task_id)`
- `task_review_status(task_id)`

`task_evidence_status(task_id)` is the preferred composite read for explaining why a task is
blocked, complete, reopened, or waiting on review.

It should include:

- declared artifact requirements
- declared review requirements
- active artifact per requirement lineage
- latest primitive review records on the active artifact
- unsatisfied evidence requirements
- blockers caused by `changes_requested` or `rejected` review outcomes

### 5.3 Plan evaluation reads

- `plan_summary(plan_id)`
- `plan_actionable_tasks(plan_id)`
- `plan_pending_reviews(plan_id)`
- `plan_rollup(plan_id)`

### 5.4 Portfolio and queue reads

- `actionable_tasks(scope)`
- `pending_reviews(scope)`
- `stale_work(scope)`
- `reclaimable_work(scope)`

### 5.5 Local join reads

When native specs are enabled, the engine may also expose join families such as:

- `task_linked_specs(task_id)`
- `task_spec_status(task_id)`
- `plan_spec_coverage(plan_id)`

These joins must:

- identify the authoritative coordination object clearly
- identify the local spec source clearly
- avoid treating branch-local spec posture as authoritative blocker truth unless an explicit sync
  action has already materialized that change into coordination state

## 6. Required inputs

The engine may read:

- canonical coordination snapshot state
- runtime descriptors when they affect coordination reasoning
- local materialized projections only as acceleration
- local spec summaries through the spec engine when a caller requested an explicit coordination-plus-spec join

The engine must not require:

- direct Git inspection by callers
- transport-specific context to evaluate semantics
- UI-specific view structs as inputs

The engine must not treat repo-local spec files as direct authoritative coordination input.

## 7. Evaluation boundaries

The engine must draw a clear line between:

- authoritative facts
- derived workflow conclusions
- freshness or availability limits

Examples:

- a task's declared dependencies are authoritative facts
- "task is actionable" is a derived conclusion
- "answer is based on eventual materialization from authority stamp X" is freshness metadata

## 8. Freshness contract

Every query result must be wrapped in a consistency/freshness envelope defined by
[consistency-and-freshness.md](./consistency-and-freshness.md).

At minimum, the envelope must tell the caller:

- whether the read was `eventual` or `strong`
- what authority stamp or version it was evaluated against
- whether that authority input is verified current, verified stale, or unavailable

## 9. Review and evidence semantics

The engine must implement the artifact and review rules from
[coordination-artifact-review-model.md](./coordination-artifact-review-model.md).

That includes:

- active artifact lineage selection
- review satisfaction against the active artifact
- review-pass aggregation over many primitive review records
- reopened and rejected task consequences
- yield checkpoint visibility when it affects task posture

## 10. Result shape rules

Query results should prefer:

- explicit status enums
- explicit blocker arrays with reasons
- explicit evidence status fields
- stable object identifiers

Query results should avoid:

- requiring consumers to diff raw records manually
- embedding presentation-only formatting as the primary output
- hiding important uncertainty in free-form text

## 11. Relationship to product surfaces

Product surfaces must consume the Coordination Query Engine rather than reimplement coordination
logic.

That includes:

- MCP query handlers
- SSR console and API read models
- CLI status and inspection commands
- future PRISM Service read broker

Surface-specific formatting and truncation are allowed. Surface-specific workflow semantics are not.

If a surface also exposes native spec context, it should obtain that through the documented
spec-engine seam or an explicit join family rather than by parsing spec files ad hoc in handlers.

## 12. Minimum implementation bar

Before PRISM Service is specified concretely, the Coordination Query Engine contract is considered
implemented only when:

- MCP, CLI, and UI can answer plan/task/evidence/review questions without reaching around the
  engine for coordination semantics
- blocker and actionability logic is centralized
- artifact and review requirement evaluation is centralized
- result freshness is surfaced through the shared consistency contract
- local spec joins, when exposed, preserve the authoritative-versus-local boundary explicitly
