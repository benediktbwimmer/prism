# Repo-Authored PRISM Code Compiled To PRISM Execution IR

Status: proposed design  
Audience: coordination, service, runtime, query, MCP, CLI, UI, and extension maintainers  
Scope: authoring PRISM code in JS or TS, compiling it into PRISM Execution IR or transaction ops, and preserving a thin service with DB-backed authority

---

## 1. Summary

PRISM should move toward:

- authoring PRISM code in JS or TS
- compiling that authoring form into explicit PRISM Execution IR or explicit native transaction ops
- persisting, querying, rendering, and executing native results rather than executing authored code directly in the service

The core rule is:

- PRISM code is authored as JS/TS
- plans and runtime state are executed as native compiled or lowered results

This gives PRISM:

- high-level authoring UX
- explicit inspectable graph structure
- deterministic execution semantics
- strong provenance over compiled artifacts
- PRISM Execution IR as the reviewable and operational truth
- lightweight service behavior
- no arbitrary plan-authoring code running in the service hot path

## 2. Why this direction fits PRISM

PRISM already wants all of the following:

- reusable plan definitions
- branching and fan-out or fan-in structure
- typed inputs and outputs
- graph-visible plans, tasks, actions, reviews, and evidence
- good UI rendering and queryability
- strong provenance and auditability
- a thin service that does not become a user-code interpreter

Compiled PRISM Execution IR is a strong answer because it resolves the tension between:

- expressive authoring
- explicit runtime inspectability

without forcing PRISM to execute arbitrary user plan-authoring code in the service.

## 3. Core stance

### 3.1 PRISM Execution IR remains runtime truth, not live guest code

PRISM plans should remain explicit persisted PRISM Execution IR artifacts.

They should not become opaque runtime code objects.

### 3.2 PRISM authoring can still be code-first

Humans and agents should be able to author PRISM code in a higher-level form such as:

- JS or TS plan-definition code
- JS or TS Action, validator, or helper code
- generated PRISM code

That authoring layer is flexible and ergonomic.

### 3.3 The service should remain native-result-driven

The PRISM Service should not:

- interpret arbitrary authored plan code directly
- compile user-authored plan code in its hot path
- become a generic guest-code runtime

The service should consume compiled IR or lowered native mutation ops plus associated provenance.

## 4. Proposed architecture

The long-term model should have three clear layers.

### 4.1 Repo-authored PRISM code layer

This is what humans or agents write.

Examples:

- reusable plan definitions under `.prism/code/plans/`
- Action or validator source under `.prism/code/actions/` or `.prism/code/validators/`
- shared helper modules under `.prism/code/libraries/`
- generated plan source derived from specs or templates

### 4.2 Plan compiler

This runs in a runtime or another trusted compile environment.

It should:

- parse or evaluate the authoring form
- validate it
- compile it into explicit PRISM Execution IR
- emit source provenance
- emit compiler version and artifact hash

The compiler implementation should primarily live in Rust so it can integrate directly with
PRISM-native types, IR lowering, validation, and provenance machinery.

That does **not** mean PRISM should fall back to a tiny declarative authoring DSL.

The intended authoring experience should stay close to ordinary JS or TS control flow. The compiler
should therefore aim to support near-full JS or TS evaluation at compile time, not just static
parsing of a restricted syntax subset.

PRISM already has a strong starting point for this through the existing `prism-js` stack:

- TS parsing and transpilation support
- embedded JS execution
- host-call plumbing between JS and Rust

The long-term target should therefore be:

- JS or TS plan-definition code as the authored source
- Rust-hosted evaluation of that source against a plan-authoring SDK
- Rust capture and lowering into explicit PRISM Execution IR

This preserves the ergonomic “just write control flow” authoring model while keeping the compiled
target explicit and service-safe.

The point is to keep the authoring surface highly expressive:

- arbitrary control flow is the compact way to describe the DAG that should exist
- the compiler then turns that authored control flow into explicit PRISM Execution IR

### 4.3 Compiler hosting model

PRISM should support the same compiler core through multiple front doors.

#### 4.3.1 CLI-fronted compile path

The first delivery path should include a local CLI-fronted compile command over the shared compiler.

That path is the simplest way to:

- prove the compiler model
- iterate on authored plan UX
- generate and inspect compiled IR locally
- integrate with agent-driven planning before distributed compilation exists

#### 4.3.2 Runtime-hosted compile path

Runtimes should host the same compiler core as a capability.

That will be useful for:

- agent-driven authoring in remote or hosted setups
- compile environments that need repo-local toolchains or context
- team workflows where the service should ask a trusted runtime to compile authored definitions

#### 4.3.3 Never long-lived guest code as service truth

The service should not treat authored JS/TS as the long-lived workflow truth.

The service may accept compiled artifacts, ask a trusted runtime to compile repo-authored code, or
evaluate bounded `prism_code` requests through the controlled runtime stack, but it should remain
native-result-driven rather than becoming a generic guest-code host.

### 4.4 Shared SDK family

PRISM should not invent a separate state interaction API just because code is running inside plan
authoring, an Action, a validation runner, or an event hook.

Instead, PRISM should expose one shared SDK family through:

- `prism_code`

That means the authored JS or TS environment should be built on the same native PRISM read and
write capabilities, views, and mutation shapes that already power agent-facing interpreted code.

This SDK family should cover at least:

- plan authoring
- Action runners
- validation runners
- event hooks or event jobs

The strongest form of this rule is:

- the authoring and execution SDK should verbatim reuse the same logical capability contracts that
  back `prism_code`
- PRISM should not invent a second equivalent of signals, workflow queries, or workflow-only state
  mutation channels
- Actions, validation runners, and event hooks should read and write PRISM state through those same
  underlying capability surfaces

These surfaces do not all need the same ergonomic helpers.

In particular:

- plan authoring needs graph-construction helpers and optional compile-time reads
- Action, validation, and event-hook authoring may need little or no extra DSL at all

But all of them should be able to import the same read and write capability surface rather than
learning a second PRISM-specific API.

The resulting SDK family should therefore be understood as:

- one native PRISM capability surface for reads and writes through `prism_code`
- a plan-authoring layer that adds graph-construction and compile-time helpers
- runtime-execution layers for Actions, validation runners, and event hooks that mostly just import
  those same capabilities directly

The public authoring model should use ordinary code bindings rather than mutation-payload client
ids.

That means:

- authored code refers to plans, tasks, actions, reviews, and intermediate values through local
  variables and lexical bindings
- the compiler may introduce temporary internal identifiers while lowering the authored program into
  PRISM Execution IR or transaction ops
- those internal identifiers must not leak into the user-facing programming model
- source-mapped validation and runtime errors should point back to authored bindings and control
  flow rather than internal lowering ids

### 4.5 Executable PRISM Execution IR

This is what the service and the query layers actually care about.

It should contain explicit PRISM-native structure such as:

- plans
- tasks
- actions
- bindings
- typed inputs and outputs
- policies
- execution metadata
- provenance to the workflow source and compiled artifact

PRISM Execution IR is the default operational surface that PRISM should persist, review, render,
query, and execute.

## 5. Definition, compiled artifact, and instance

This design should keep three concepts distinct.

### 5.1 Plan definition

This is the authored source form.

Examples:

- JS or TS plan-definition code
- generated plan-definition source
- later declarative plan-definition input

### 5.2 Compiled artifact

This is the compiler output for a specific plan definition revision and parameterization shape.

It contains:

- compiler metadata
- source provenance
- compiled artifact hash
- explicit PRISM-native IR

This is the reviewable and renderable plan truth that PRISM should pin and inspect by default.

### 5.3 Plan instance

This is a concrete instantiated graph produced from the compiled artifact.

It carries:

- bound inputs
- current execution state
- produced outputs and evidence
- lineage and provenance back to the compiled artifact and authored definition

## 6. Compile-time reads

Plan authoring should support compile-time reads from PRISM state as part of the intended v1 model.

That means authored plan code may be able to inspect current state such as:

- existing plans, tasks, artifacts, reviews, or validations
- current runtime capabilities
- current rollout or promotion posture
- current spec coverage or related plan state

Compile-time reads are powerful because they let the authored plan code describe the DAG compactly while
still adapting the compiled result to current authoritative state.

This also means the generated DAG may differ across compiles when compile-time reads are used.

That is acceptable, but it must be explicit and provenance-rich.

The compiled artifact should therefore capture not only source revision and compiler version, but
also the declared compile-time read inputs that influenced the emitted IR.

Two modes should remain available:

- pure compile from source plus explicit parameters only
- contextual compile from source plus explicit parameters plus declared compile-time PRISM reads

## 7. Control flow in compiled IR

Compiled IR does not need to mean that every possible branch is flattened into only static leaf
nodes and plain edges.

The compiler may emit explicit IR-level control structures such as:

- typed conditions
- fan-out or fan-in constructs
- joins
- bounded map-like expansion constructs
- loop-like control constructs with explicit carried state
- explicit continue-as-new boundaries

The key rule is that these remain:

- explicit
- typed
- inspectable
- queryable
- free of arbitrary guest code at execution time

This keeps branching and richer plan structure available without turning the service into a
plan-code interpreter.

## 8. What PRISM borrows from Temporal, and what it rejects

The important idea PRISM should borrow is:

- author Plans as code because arbitrary control flow is a compact, expressive way to describe the
  DAG that should exist

PRISM should not import Temporal’s broader product model as-is.

PRISM should explicitly reject as core concepts:

- homogeneous workers that differ mostly by queue binding
- task queues as the primary routing model
- verbose worker configuration as the center of the execution model
- signals between running workflow instances as a required coordination primitive
- workflow queries as a separate special read channel

PRISM does not need those because:

- the service routes Actions intentionally using runtime descriptors and policy
- runtimes claim Tasks because they know their own context best
- Plans remain structural and compiled rather than long-lived actor objects
- the shared PRISM read and write capability surface already covers state interaction needs

## 9. Why compile to PRISM-native IR instead of reusing a workflow state machine directly

PRISM should borrow the compiler architecture and the code-authored workflow idea aggressively.

It should **not** simply reuse a Temporal-like executable state-machine IR as its primary product
model.

The reason is that PRISM’s center of gravity is still:

- shared coordination state
- plans, tasks, actions, artifacts, and reviews
- human and agent participation
- rich query and UI surfaces

So the compiler target should remain PRISM-native and graph-shaped rather than replacing the graph
with a generic workflow runtime abstraction.

## 10. Relationship to existing PRISM designs

This design builds directly on:

- [2026-04-09-actions-and-machine-work.md](./2026-04-09-actions-and-machine-work.md)
- [2026-04-09-graph-dataflow-and-parameterization.md](./2026-04-09-graph-dataflow-and-parameterization.md)
- [2026-04-09-shared-execution-substrate.md](./2026-04-09-shared-execution-substrate.md)
- [2026-04-09-event-job-runners.md](./2026-04-09-event-job-runners.md)
- [2026-04-09-warm-state-validation-feedback.md](./2026-04-09-warm-state-validation-feedback.md)

The intended stack is:

- graph-wide typed dataflow and parameterization
- Actions as explicit machine-work nodes
- shared execution substrate beneath Actions, validation, and event jobs
- JS or TS-authored plan definitions compiled into the native graph IR that these systems use

## 11. Compiler output requirements

The compiled artifact should be explicit, stable, and inspectable.

At minimum it should carry:

- plan definition id
- plan definition version
- compiler version
- source language
- source files
- source map or source-location mapping
- compiled artifact hash
- compiled IR payload
- declared compile-time read inputs when contextual compile is used

This gives PRISM:

- source-to-IR provenance
- stable instance pinning
- reviewable compiled output
- query and UI affordances that can refer back to authored source

## 12. Source and instance provenance

PRISM should be able to answer:

- which authored plan definition produced this plan instance
- which source revision it came from
- which compiler version produced the IR
- which compiled artifact hash is pinned to this running or persisted graph
- which compile-time reads affected the compiled result when contextual compile was used

This is especially important for:

- replay/debugging
- upgrade safety
- plan-template reuse
- spec-to-plan lineage

## 13. Relationship to the native spec engine

This direction makes the native spec engine stronger.

The future flow can become:

1. a spec defines intent
2. an agent reads structured spec data plus prose
3. the agent authors plan definition code or a higher-level plan definition
4. the compiler emits explicit PRISM IR
5. PRISM persists, queries, renders, and executes that IR
6. coverage and sync provenance still link back to the spec

That is a strong stack:

- spec as native intent
- plan code as ergonomic authoring
- PRISM IR as deterministic execution substrate

## 14. Why not replace the PRISM Service authority model with an append-log runtime

Compiled workflow authoring is highly compatible with PRISM.

A full append-log runtime as the primary PRISM authority model is not the right default target.

PRISM’s release path is centered on a DB-backed `CoordinationAuthorityStore` because it is better
suited for:

- shared current-state reads
- concurrent mutations
- multi-actor coordination
- service-side policy and auth
- rich query and UI surfaces

So the long-term model should be:

- compiled Plans are adopted
- DB-backed coordination authority remains the system truth

## 15. Selective materialization in v1 and future runtime-fast-lane optimization

PRISM v1 should include selective materialization for machine-only fan-outs and other action-dense
subgraphs.

The goal is to avoid overwhelming the coordination store with low-value per-leaf writes when policy
only needs:

- a summary
- selected failures
- durable evidence or aggregate outputs

The materialization policy should therefore be able to distinguish at least:

- full materialization
- batched materialization
- aggregated materialization

This should remain explicit and policy-driven.

The important lever here is the **materialization boundary**.

That boundary determines where a machine-only subgraph must be committed back into authoritative
coordination truth.

Depending on policy, the materialization boundary may be:

- each realized leaf
- a bounded chunk
- an aggregate summary

In particular:

- large machine-only fan-outs may execute mostly in runtime memory
- runtimes may report batched or aggregated results back to the service
- the service remains authoritative for the persisted coordination truth
- failures and policy-critical evidence should still be retained individually when required

Aggregation may compress success, but it must not erase information that later reasoning,
inspection, or policy enforcement depends on.

At minimum PRISM should leave room for preserving:

- failures
- policy violations
- retry exhaustion
- artifact-producing leaves
- externally visible side effects
- provenance anchors

PRISM should also leave room for mixed materialization by outcome or action class.

Examples:

- successes aggregated, failures fully materialized
- artifact-producing leaves fully materialized, routine successes aggregated
- certain policy-tagged Actions always retained individually

When detailed per-leaf results are compressed, the service should still be able to persist:

- the aggregate durable result in coordination state
- an artifact or export pointer to detailed batch traces or result sets when they are worth keeping

There is also an additional future optimization path that this design should preserve.

For plans or subgraphs that are:

- machine-only
- action-dense
- latency-sensitive
- free of human or agent decisions in the middle

PRISM may later support a faster runtime-executed subgraph lane.

The shape would be:

- the service remains thin and authoritative
- the service identifies an executable machine-only subgraph
- a runtime receives a compiled bounded execution bundle
- the runtime executes that bundle locally with hot state
- the service still records authoritative graph transitions and outputs

Any chunk eligible for selective materialization should be replay-safe at the chunk boundary, not
just at the individual leaf level.

In that future mode, an append-log plus snapshot runtime-local execution state may be a strong
optimization for the runtime hot path.

This fast lane is explicitly a **future optimization goal only if demand justifies it**.

It should not be a v1 requirement.

## 16. Recommended boundaries

PRISM should keep these boundaries strict:

### 12.1 The service stays thin

The service owns:

- authority
- policy
- routing
- provenance
- current graph truth

The service does not become a general workflow-code interpreter.

### 12.2 The compiler runs outside the service hot path

Compilation should happen in runtimes or another trusted compile environment.

### 12.3 The compiled target stays explicit

The compiled IR should remain:

- stable
- typed
- explicit
- graph-shaped
- queryable

### 12.4 Selective materialization is part of v1, fast lanes remain optional

Selective materialization for machine-only fan-outs should be part of the v1 execution model.

The later fast runtime-executed subgraph optimization should remain:

- explicit
- bounded
- optional
- non-authoritative

The service must still own coordination truth.

## 17. Recommended rollout

### Phase 1

Define the authoring-to-IR design and the compiled artifact metadata model.

### Phase 2

Add a JS or TS plan-definition authoring surface and compile it into PRISM-native IR.

### Phase 3

Teach PRISM to persist source-to-IR provenance and pin instances to compiled artifact hashes.

### Phase 4

Integrate spec-driven planning with plan-definition authoring and compilation.

### Phase 5

Only later, if demand justifies it, add a fast runtime-executed lane for bounded machine-only
subgraphs.

## 18. Recommendation

PRISM should evolve toward JS or TS-authored Plans compiled into explicit PRISM-native IR.

That gives PRISM:

- workflow-as-code ergonomics
- graph-level inspectability
- strong query and UI surfaces
- deterministic execution semantics
- strong provenance
- a lightweight IR-driven service

The append-log style runtime optimization should be preserved as a future option for machine-only
subgraphs, but it should not replace the DB-backed authority-centered service model that PRISM
needs for its broader coordination role.
