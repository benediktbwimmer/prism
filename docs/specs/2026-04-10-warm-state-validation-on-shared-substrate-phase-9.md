# Warm-State Validation On Shared Substrate Phase 9

Status: in progress  
Audience: prism-core, prism-mcp, prism-js, CLI, runtime, validation, and repo-policy maintainers  
Scope: move warm-state validation from policy-and-refs-only semantics onto the shared execution substrate without collapsing validation into generic machine work

---

## 1. Goal

Phase 9 exists to make warm-state validation an actual substrate-backed execution family.

After Phase 8, PRISM has:

- shared execution substrate types
- a shared execution adapter over the current durable execution store
- event-trigger execution as the first substrate consumer

Phase 9 should use that substrate for validation while preserving the semantics that make
validation special:

- validation is part of task correctness
- validation can gate completion
- validation is warm-state aware
- validation is normally runtime-local to the claiming runtime

This phase should not flatten validation into `Action`.

---

## 2. Required outcomes

Phase 9 is complete only when all of the following are true:

- PRISM can persist and query validation executions as `validation` family substrate records
- validation runner identity, capability class, target runtime, and compact result are durable and
  queryable
- task or artifact validation refs are backed by substrate execution ids rather than ad hoc opaque
  ids
- warm-state validation execution can be requested against the claiming runtime using the shared
  substrate, not a bespoke validation-only path
- query and MCP surfaces can distinguish pending, passed, failed, or missing validation based on
  substrate execution state rather than only on manually attached refs

This phase does not require:

- first-class `Action`
- richer graph dataflow bindings
- full repo-local validator authoring or compilation from `.prism/code/validators`

---

## 3. Hard rules

The implementation must preserve these architectural constraints:

- validation remains semantically distinct from `Action` and event jobs
- validation execution uses the shared substrate mechanics instead of inventing a second runner or
  execution-record system
- the preferred runtime target is the runtime currently holding the relevant task claim whenever
  such a runtime is available and healthy
- validation writes must remain provenance-rich and attributable to the concrete runtime session
  that executed them
- validation refs and validation queries must stay source-level and user-meaningful; they must not
  expose backend row ids or incidental storage details

---

## 4. Required model additions

At minimum this phase should add or settle the following concepts.

### 4.1 Validation execution subject

Validation executions need an explicit subject model, for example:

- task validation for `task_id`
- artifact validation for `artifact_id`
- review-gate validation for `review_id` or `review_requirement_id`

That subject model should map onto the shared substrate without pretending validation is
event-trigger work.

### 4.2 Validation runner identity

Validation executions must record at least:

- validation runner kind
- capability class
- runtime target
- execution status
- compact result envelope

### 4.3 Validation refs

`ValidationRef.id` should resolve to a durable validation substrate execution record.

The public model should remain simple:

- tasks, artifacts, and reviews refer to validation ids
- query surfaces resolve those ids into structured validation state

### 4.4 Runtime-local execution

The preferred execution target remains the warm claiming runtime for the relevant task.

This phase should therefore define:

- how the service selects that runtime target
- how a validation request is represented on the substrate
- how the runtime returns compact structured results

---

## 5. Implementation slices

Phase 9 should land in bounded slices.

### Slice 1: Generic shared execution storage seam for non-event families

Deliver:

- extend the current shared execution storage seam so validation executions are not forced through
  event-trigger-specific source assumptions
- add durable substrate storage support for `validation` family records
- keep event-trigger execution working on the same seam

Success condition:

- validation no longer depends on pretending to be an event-trigger record

### Slice 2: Validation execution lifecycle and linking

Deliver:

- validation subject types
- validation execution request and result mapping
- linkage from task, artifact, or review validation refs to durable substrate execution ids
- query shaping for pending, passed, failed, or missing validation state

Success condition:

- validation refs are durable substrate-backed execution refs rather than opaque manual labels

### Slice 3: Warm runtime dispatch

Deliver:

- route validation requests to the preferred claiming runtime when available
- record runtime target and provenance through the substrate
- return compact structured validation results

Success condition:

- warm-state validation actually executes on the substrate rather than only being modeled as policy

---

## 6. Repo policy and validator source alignment

This phase must stay aligned with the `prism_code` and repo-authored code architecture.

That means:

- validator code should converge on `.prism/code/validators/`
- built-in validators may still exist in the binary
- repo-local validator authoring and compilation may arrive later, but the runner identity and
  execution model introduced here must already be compatible with that destination

This phase is about substrate-backed execution and durable state, not yet about the full validator
authoring experience.

---

## 7. Validation

Minimum validation for this phase:

- targeted `cargo test -p prism-core ...` coverage for durable shared-execution storage and
  validation linking
- targeted `cargo test -p prism-mcp ...` coverage for warm runtime routing and validation-state
  read models
- downstream `cargo test -p prism-cli` if public core surfaces or CLI-facing validation views
  change

Run the full workspace only if this phase crosses the repo’s Tier 3 threshold.

---

## 8. Exit note

Phase 9 should be judged by whether validation has actually moved onto the shared substrate:

- durable execution state is substrate-backed
- runtime routing is substrate-backed
- validation refs resolve through substrate records
- validation queries reason over substrate results

If validation still behaves like a special side channel with only loosely attached refs, then
Phase 9 is not complete.
