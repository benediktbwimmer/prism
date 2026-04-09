# Compiled Workflows To PRISM IR

Status: proposed design  
Audience: coordination, service, runtime, query, MCP, CLI, UI, and extension maintainers  
Scope: authoring workflows in JS or TS, compiling them into PRISM-native IR, and preserving a thin service with DB-backed authority

---

## 1. Summary

PRISM should move toward:

- authoring workflows or plan definitions in JS or TS
- compiling that authoring form into explicit PRISM-native IR
- persisting, querying, rendering, and executing the compiled IR rather than executing authoring
  code directly in the service

The core rule is:

- workflows are authored as code
- plans are executed as compiled IR

This gives PRISM:

- high-level authoring UX
- explicit inspectable graph structure
- deterministic execution semantics
- strong provenance over compiled artifacts
- compiled IR as the reviewable and operational truth
- lightweight service behavior
- no arbitrary workflow code running in the service hot path

## 2. Why this direction fits PRISM

PRISM already wants all of the following:

- reusable workflow definitions
- branching and fan-out or fan-in structure
- typed inputs and outputs
- graph-visible plans, tasks, actions, reviews, and evidence
- good UI rendering and queryability
- strong provenance and auditability
- a thin service that does not become a user-code interpreter

Compiled workflow IR is a strong answer because it resolves the tension between:

- expressive authoring
- explicit runtime inspectability

without forcing PRISM to execute arbitrary user workflow code in the service.

## 3. Core stance

### 3.1 Plans remain IR artifacts, not live guest code

PRISM plans should remain explicit persisted IR artifacts.

They should not become opaque runtime code objects.

### 3.2 Workflow authoring can still be code-first

Humans and agents should eventually be able to author workflows in a higher-level form such as:

- JS or TS plan-definition code
- generated workflow code
- perhaps later a declarative DSL

That authoring layer is flexible and ergonomic.

### 3.3 The service should remain IR-driven

The PRISM Service should not:

- interpret arbitrary workflow code directly
- compile user workflow code in its hot path
- become a generic guest-code runtime

The service should consume compiled IR plus associated provenance.

## 4. Proposed architecture

The long-term model should have three clear layers.

### 4.1 Workflow authoring layer

This is what humans or agents write.

Examples:

- reusable workflow definitions
- plan-definition code
- generated workflow source derived from specs or templates

### 4.2 Workflow compiler

This runs in a runtime or another trusted compile environment.

It should:

- parse or evaluate the authoring form
- validate it
- compile it into explicit PRISM-native IR
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

- JS or TS workflow-definition code as the authored source
- Rust-hosted evaluation of that source against a workflow-authoring SDK
- Rust capture and lowering into explicit PRISM-native IR

This preserves the ergonomic “just write control flow” authoring model while keeping the compiled
target explicit and service-safe.

### 4.3 Compiler hosting model

PRISM should support the same compiler core through multiple front doors.

#### 4.3.1 CLI-first compile path

The first delivery path should be a local CLI compile command.

That path is the simplest way to:

- prove the compiler model
- iterate on authored workflow UX
- generate and inspect compiled IR locally
- integrate with agent-driven planning before distributed compilation exists

#### 4.3.2 Runtime-hosted compile path later

Later, runtimes may host the same compiler core as a capability.

That will be useful for:

- agent-driven authoring in remote or hosted setups
- compile environments that need repo-local toolchains or context
- team workflows where the service should ask a trusted runtime to compile authored definitions

#### 4.3.3 Never the service hot path

The service should not compile authored workflow code in its hot path.

The service may later accept compiled artifacts or ask a trusted runtime or compile environment to
produce them, but it should remain IR-driven rather than becoming the place where guest workflow
code is evaluated.

### 4.4 Executable PRISM IR

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

The compiled IR is the default operational surface that PRISM should persist, review, render,
query, and execute.

## 5. Definition, compiled artifact, and instance

This design should keep three concepts distinct.

### 5.1 Workflow definition

This is the authored source form.

Examples:

- JS or TS workflow-definition code
- generated plan-definition source
- later declarative workflow-definition input

### 5.2 Compiled artifact

This is the compiler output for a specific workflow definition revision and parameterization shape.

It contains:

- compiler metadata
- source provenance
- compiled artifact hash
- explicit PRISM-native IR

This is the reviewable and renderable workflow truth that PRISM should pin and inspect by default.

### 5.3 Plan instance

This is a concrete instantiated graph produced from the compiled artifact.

It carries:

- bound inputs
- current execution state
- produced outputs and evidence
- lineage and provenance back to the compiled artifact and authored definition

## 6. Control flow in compiled IR

Compiled IR does not need to mean that every possible branch is flattened into only static leaf
nodes and plain edges.

The compiler may emit explicit IR-level control structures such as:

- typed conditions
- fan-out or fan-in constructs
- joins
- bounded map-like expansion constructs

The key rule is that these remain:

- explicit
- typed
- inspectable
- queryable
- free of arbitrary guest code at execution time

This keeps branching and richer workflow structure available without turning the service into a
workflow-code interpreter.

## 7. Why compile to PRISM-native IR instead of reusing a workflow state machine directly

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

## 8. Relationship to existing PRISM designs

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
- JS or TS-authored workflow definitions compiled into the native graph IR that these systems use

## 9. Compiler output requirements

The compiled artifact should be explicit, stable, and inspectable.

At minimum it should carry:

- workflow definition id
- workflow definition version
- compiler version
- source language
- source files
- source map or source-location mapping
- compiled artifact hash
- compiled IR payload

This gives PRISM:

- source-to-IR provenance
- stable instance pinning
- reviewable compiled output
- query and UI affordances that can refer back to authored source

## 10. Source and instance provenance

PRISM should be able to answer:

- which authored workflow definition produced this plan instance
- which source revision it came from
- which compiler version produced the IR
- which compiled artifact hash is pinned to this running or persisted graph

This is especially important for:

- replay/debugging
- upgrade safety
- plan-template reuse
- spec-to-workflow lineage

## 11. Relationship to the native spec engine

This direction makes the native spec engine stronger.

The future flow can become:

1. a spec defines intent
2. an agent reads structured spec data plus prose
3. the agent authors workflow definition code or a higher-level plan definition
4. the compiler emits explicit PRISM IR
5. PRISM persists, queries, renders, and executes that IR
6. coverage and sync provenance still link back to the spec

That is a strong stack:

- spec as native intent
- workflow code as ergonomic authoring
- PRISM IR as deterministic execution substrate

## 12. Why not replace the PRISM Service authority model with an append-log runtime

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

- compiled workflows are adopted
- DB-backed coordination authority remains the system truth

## 13. Future optimization: runtime-executed machine-only subgraphs

There is one important future optimization path that this design should explicitly preserve.

For plans or subgraphs that are:

- machine-only
- action-dense
- latency-sensitive
- free of human or agent decisions in the middle

PRISM may later support a fast runtime-executed subgraph lane.

The shape would be:

- the service remains thin and authoritative
- the service identifies an executable machine-only subgraph
- a runtime receives a compiled bounded execution bundle
- the runtime executes that bundle locally with hot state
- the service still records authoritative graph transitions and outputs

In that future mode, an append-log plus snapshot runtime-local execution state may be a strong
optimization for the runtime hot path.

This is explicitly a **future optimization goal only if demand justifies it**.

It should not be a v1 requirement.

## 14. Recommended boundaries

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

### 12.4 Future fast lanes remain optional

Runtime-executed machine-only subgraph optimization should remain:

- explicit
- bounded
- optional
- non-authoritative

The service must still own coordination truth.

## 15. Recommended rollout

### Phase 1

Define the authoring-to-IR design and the compiled artifact metadata model.

### Phase 2

Add a JS or TS workflow-definition authoring surface and compile it into PRISM-native IR.

### Phase 3

Teach PRISM to persist source-to-IR provenance and pin instances to compiled artifact hashes.

### Phase 4

Integrate spec-driven planning with workflow-definition authoring and compilation.

### Phase 5

Only later, if demand justifies it, add a fast runtime-executed lane for bounded machine-only
subgraphs.

## 16. Recommendation

PRISM should evolve toward JS or TS-authored workflows compiled into explicit PRISM-native IR.

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
