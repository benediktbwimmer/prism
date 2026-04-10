# PRISM Shared Execution Substrate

Status: proposed design  
Audience: coordination, service, runtime, query, MCP, CLI, UI, and extension maintainers  
Scope: one shared execution substrate beneath Actions, warm-state validation, and event jobs

---

## 1. Summary

PRISM should converge machine execution onto one shared execution substrate.

That substrate should sit beneath three semantically distinct families:

- `Action` execution
- warm-state validation execution
- event job execution

The substrate should unify:

- capability classes
- runner model
- service-side routing and orchestration
- runtime-side execution
- durable execution records
- result envelopes
- retries, timeout, and budget handling
- provenance

This does **not** mean that Actions, validation, and event jobs become the same object.

The intended split is:

- coordination semantics remain distinct
- execution mechanics become shared

## 2. Why this substrate should exist

PRISM is already evolving toward a workflow DAG that includes:

- human work
- agent work
- machine work

Without a shared execution substrate, PRISM will drift toward:

- one execution model for validation
- another for event jobs
- and later a third for build, deploy, rollout, or publish work

That would duplicate:

- runner configuration
- capability policy
- runtime routing
- result normalization
- provenance
- retry and timeout semantics

The shared execution substrate should prevent that fragmentation.

## 3. Semantic families above the substrate

The substrate is shared by several semantic entrypoints, but those entrypoints remain distinct.

### 3.1 Actions

Actions are explicit coordination-graph leaf nodes representing bounded machine-executed workflow
steps.

### 3.2 Validation executions

Validation remains semantically special because it is tied to:

- task correctness
- completion gating
- artifact evidence
- warm-state runtime posture

Validation execution should therefore share the substrate without being reduced to “just another
action.”

### 3.3 Event jobs

Event jobs remain orchestration-driven machine work that is usually created by:

- trigger evaluation
- recurring schedules
- service-side reactions

Event jobs do not need to be explicit coordination-graph nodes in the common case.

## 4. Core stance

### 4.1 Unify execution mechanics, not domain semantics

PRISM should unify:

- how machine work is routed
- how machine work is executed
- how results are recorded
- how provenance is modeled

PRISM should not flatten:

- `Action`
- validation
- event jobs

into one semantically generic workflow object.

### 4.2 Service routes, runtimes usually execute

The service should generally:

- evaluate state and policy
- select execution targets
- create durable execution records
- route work
- record results

Runtimes should generally execute the substantial work.

Service-executed work should remain a small, fixed set of built-in cases such as:

- webhook dispatch
- notifications
- tiny internal maintenance work

### 4.3 Tasks and Actions intentionally route differently

PRISM should preserve an intentional asymmetry between human or agent work and machine work.

- Tasks are claimed by runtimes because the runtime or agent knows best what it already knows,
  what context it has, and what work it is ready to take on
- Actions are routed by the service because the service can evaluate runtime descriptors, verified
  capability posture, policy, and current coordination state centrally

PRISM should not flatten these into one queue-centric execution model.

### 4.4 Typed runners, not arbitrary shell, are the semantic surface

The substrate should use typed runners plus structured input and output envelopes.

This does not forbid shell commands as an implementation detail of a runner. It does forbid making
“run arbitrary command X” the primary semantic API.

## 5. Shared substrate model

The shared substrate should provide at least these concepts.

### 5.1 Execution capability classes

One capability vocabulary should be shared across the substrate.

Examples:

- `cargo:test`
- `cargo:build`
- `pytest`
- `docker:publish`
- `k8s:deploy`
- `repo:script`

Capability classes should remain:

- runtime-defined or policy-defined data
- queryable
- extensible without recompiling PRISM

### 5.2 Runtime capability posture

Runtime posture should also be shared across execution families.

At minimum:

- `declared`
- `provisional`
- `verified`

An optional later `stale` posture can be added without changing the overall model.

### 5.3 Runner categories

The substrate should support one runner model with semantic categories such as:

- `validation_runner`
- `action_runner`
- `event_runner`

These categories share the same execution contract while remaining distinguishable in policy,
routing, and query/UI surfaces.

### 5.4 Execution records

Durable execution records should be first-class substrate objects.

At minimum they should capture:

- execution id
- semantic family
- runner category
- runner kind
- requested capability class
- target scope
- status
- attempt count
- created, started, and terminal timestamps
- timeout or budget
- compact structured result
- provenance

### 5.5 Structured result envelopes

All substrate executions should return compact structured results rather than only raw logs.

At minimum:

- `status`
- `summary`
- `duration_ms`
- optional typed detail fields
- optional evidence or artifact refs
- optional diagnostics refs

Raw stdout, stderr, or verbose implementation detail should remain runtime-local or artifact-backed
where appropriate.

### 5.6 Shared JS/TS capability surface

The shared execution substrate should not introduce a second state interaction model for machine
work.

Instead, runners and authored JS or TS execution code should reuse the same underlying native PRISM
read and write capabilities exposed through:

- `prism_code`

That means:

- Actions can query or mutate PRISM state through the same logical capability contracts
- validation runners can do the same
- event hooks can do the same

The SDKs used by these execution contexts may provide small convenience wrappers, but they should
not introduce a separate equivalent of workflow queries, signals, or bespoke workflow-local state
channels.

### 5.7 Materialization policy

The shared execution substrate should support explicit materialization policy for machine-only
execution, especially for large fan-outs.

The key concept is the **materialization boundary**.

That boundary defines where runtime-executed machine work must be turned into durable authoritative
coordination truth.

Examples:

- every realized leaf can be a materialization boundary
- one bounded chunk can be a materialization boundary
- one aggregate rollup can be a materialization boundary

At minimum PRISM should be able to distinguish:

- full materialization
- batched materialization
- aggregated materialization

This is important because some action-dense plans want every realized node recorded, while others
only need:

- compact aggregate outcomes
- selected failures
- durable evidence or artifact refs

The substrate should therefore allow runtimes to execute bounded machine-only chunks in memory and
return batched or aggregated results to the service at explicit policy-defined boundaries.

Aggregation may compress success, but it must not erase information that later reasoning depends
on.

At minimum, policy should be able to require preservation of:

- failures
- policy violations
- retry exhaustion
- artifact-producing leaves
- externally visible side effects
- provenance anchors
- checkpoint or compaction boundaries when they matter for replay or audit

The substrate should also leave room for mixed materialization policy by outcome or action class.

Examples:

- aggregate successes but fully materialize failures
- fully materialize artifact-producing Actions while aggregating routine successes
- always retain policy-tagged or externally visible side effects individually

When batched or aggregated execution compresses detailed per-leaf results, PRISM should still be
able to persist:

- a compact durable summary in coordination state
- an artifact or export pointer to detailed per-leaf traces, logs, or result sets when needed

The service must still remain authoritative for what becomes durable coordination truth.

## 6. Service and runtime roles

### 6.1 Service role

The service should own:

- routing
- policy checks
- execution record creation
- retries and timeout policy
- durable lifecycle transitions
- compact result normalization
- provenance

### 6.2 Runtime role

Runtimes should own:

- actual execution of most substantial work
- local toolchain and warm-state access
- runner invocation
- local telemetry and detailed logs
- returning structured results to the service

### 6.3 Runtime-authenticated writes

When runtime-executed machine work writes PRISM state, it should normally do so under the delegated
runtime session that is already executing that work.

That means:

- Actions, validation runners, and event hooks write as `delegated_machine`
- the auth carrier is the runtime session delegated by the service
- provenance should bind each write to the concrete execution record, runtime identity, and
  delegated principal identity

These execution contexts should not gain hidden `human_attested` or `service_attested` authority
just because they are machine-executed.

### 6.4 Transport

The preferred target shape remains:

- long-lived authenticated runtime connections to the service
- low-latency dispatch
- structured progress and result reporting

The transport should remain an implementation detail beneath the substrate contract.

## 7. Provenance and trust

The substrate should preserve one coherent provenance model across execution families.

At minimum, every execution should be attributable to:

- who requested or authorized it
- which service routed it
- which runtime executed it when applicable
- which runner kind handled it
- which capability class or posture was relied upon
- which execution record and runtime session carried any resulting mutations

## 8. Relationship to Actions

Actions should be one semantic consumer of the substrate.

An Action:

- is a coordination-graph node
- uses the substrate to execute
- has graph-visible dependencies and lifecycle

The substrate does not define the graph semantics of Action; it only powers the execution beneath
them.

## 9. Relationship to warm-state validation

Warm-state validation semantics remain defined separately.

The correction here is:

- validation execution should use the shared substrate
- validation should not keep a bespoke one-off execution stack

Validation still remains special in semantics:

- tied to task correctness
- tied to completion gating
- tied to warm-state runtime ownership

## 10. Relationship to event jobs

Event jobs should also use the substrate.

The event engine should be understood primarily as:

- trigger evaluation
- recurring or reactive orchestration
- creation and routing of event-driven executions

It should not be understood as a separate general execution plane.

## 11. Query and UI expectations

The shared substrate should make it easy for query and UI surfaces to show:

- what kind of execution occurred
- whether it was validation, action, or event-driven work
- where it ran
- what capability class it used
- whether it succeeded, failed, timed out, or was retried
- what compact result or evidence it produced

## 12. Recommendation

PRISM should adopt one shared execution substrate beneath:

- Actions
- warm-state validation
- event jobs

The substrate should unify:

- execution routing
- capability classes and posture
- runner contracts
- durable execution records
- retries and timeout semantics
- structured result envelopes
- provenance

PRISM should preserve distinct semantics above that substrate rather than flattening everything into
generic jobs.
