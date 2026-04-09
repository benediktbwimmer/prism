# Execution Substrate And Compiled Plan Rollout

Status: in progress
Audience: coordination, execution, validation, runtime, MCP, CLI, UI, and workflow-authoring maintainers
Scope: sequencing the artifact/review model, the shared execution substrate, validation, actions, graph dataflow, native reusable plans, and finally JS/TS-authored compiled plans

---

## 1. Summary

PRISM now has several tightly related but still distinct bodies of work:

- implement the coordination artifact/review model
- hard-cut PRISM to a SQL-only coordination authority model
- implement one shared execution substrate
- move warm-state validation onto that substrate
- add `Action` as a first-class machine-work leaf
- add graph-wide dataflow and bindings
- add reusable native plan definitions and instantiation
- add JS/TS-authored workflows compiled into PRISM-native IR

This roadmap exists to sequence those items so that:

- foundational semantics land before authoring ergonomics
- execution machinery stabilizes before the compiler targets it
- graph dataflow lands before reusable plan authoring depends on it
- event-trigger work is explicitly postponed instead of muddying the critical path

The ordering principle is:

- stabilize the coordination and execution model first
- remove shared-ref authority support and snapshot-shaped authority coupling before more systems build on it
- then stabilize graph dataflow and reusable native plan semantics
- only then build the JS/TS compiler and SDK on top of that settled target

This roadmap depends on:

- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)
- [../designs/2026-04-09-shared-execution-substrate.md](../designs/2026-04-09-shared-execution-substrate.md)
- [../designs/2026-04-09-actions-and-machine-work.md](../designs/2026-04-09-actions-and-machine-work.md)
- [../designs/2026-04-09-warm-state-validation-feedback.md](../designs/2026-04-09-warm-state-validation-feedback.md)
- [../designs/2026-04-09-graph-dataflow-and-parameterization.md](../designs/2026-04-09-graph-dataflow-and-parameterization.md)
- [../designs/2026-04-09-compiled-workflows-to-prism-ir.md](../designs/2026-04-09-compiled-workflows-to-prism-ir.md)

## 2. Status

Current phase checklist:

- [x] Phase 0: freeze sequencing and spec boundaries
- [x] Phase 1: implement the coordination artifact/review model
- [x] Phase 2: refactor the coordination authority abstraction and persistence contract
- [ ] Phase 3: implement the shared execution substrate core
- [ ] Phase 4: move warm-state validation onto the shared execution substrate
- [ ] Phase 5: add `Action` as a first-class graph leaf on the shared execution substrate
- [ ] Phase 6: implement graph-wide typed inputs, outputs, and bindings
- [ ] Phase 7: implement reusable native plan definitions and instantiation
- [ ] Phase 8: implement JS/TS-authored compiled plans and the workflow-authoring SDK
- [ ] Phase 9: evaluate whether fast runtime-executed machine-only subgraphs are warranted

Current active phase:

- Phase 3: implement the shared execution substrate core

Current implementation note (2026-04-09):

- Phase 1 is landed across `prism-coordination`, `prism-query`, `prism-core`, `prism-mcp`, and `prism-js`
- Phase 2 is landed with the SQL-only authority cutover, removing `git_shared_refs` as a supported authority backend and collapsing the normal authority mutation path to DB append semantics
- the next blocking work is Phase 3: the shared execution substrate core

## 3. Ordering thesis

This work should not start with the compiler.

The compiler is an authoring layer and should target a stable native model. If it arrives too early,
it will churn every time the graph, execution, evidence, or dataflow semantics shift.

The right order is:

1. land durable artifact and review semantics first
2. remove the shared-ref authority backend and replace the remaining authority contract with a SQL-only seam
3. build the shared execution substrate next
4. prove that substrate with warm-state validation
5. widen it to `Action`
6. add graph-wide dataflow and bindings once the node set is real
7. add reusable native plan definitions and instantiation against that settled graph model
8. only then add JS/TS authoring and compilation to PRISM-native IR

Event-trigger work is intentionally out of the critical path for this roadmap.

It can return later once the shared substrate, Actions, and compiled plan authoring are in place.

## 4. Parallel-work guidance

This roadmap is mostly sequential at the phase level, but some prep and supporting work can proceed
in parallel once the dependency edge is clear.

### 4.1 Strictly sequential phase boundaries

These phase transitions are blocking and should not be inverted:

- Phase 1 before Phase 2
- Phase 2 before Phase 3
- Phase 3 before Phases 4 and 5
- Phases 4 and 5 before Phase 6
- Phase 6 before Phase 7
- Phase 7 before Phase 8

### 4.2 Safe parallel lanes

The following work can proceed in parallel without violating the architecture:

- During Phase 1:
  - artifact/review storage work and artifact/review query-surface work can proceed in parallel once the contract mapping is frozen
- During Phase 2:
  - shared-ref authority removal
  - authority trait and type redesign for SQL backends only
  - SQLite event-log plus projection schema design
  - Postgres-oriented schema and read-shape planning
  - authority call-site migration
  can proceed in parallel if they converge on one SQL-only authority contract
- During Phase 3:
  - execution-record storage
  - runner contract definition
  - runtime routing plumbing
  - capability-class plumbing
  can proceed in parallel if they share one settled substrate spec
- During late Phase 3 / early Phase 4:
  - the Phase 4 validation spec and the Phase 5 Actions spec can be prepared in parallel
- During Phase 6:
  - task/action/review binding semantics
  - typed value/reference representation
  - query/UI read-model shaping for bound inputs and outputs
  can proceed in parallel once the dataflow contract is frozen
- During Phase 7:
  - native reusable-plan definition semantics
  - plan-instance lineage and provenance work
  can proceed in parallel once plan-definition versus plan-instance boundaries are fixed
- During Phase 8:
  - CLI-first compiler surface
  - workflow-authoring SDK surface
  - compiler provenance and artifact-pin metadata
  can proceed in parallel once the compiled IR target is frozen

### 4.3 Explicitly deferred work

The following should remain out of scope until this roadmap reaches the appropriate phase:

- event-trigger execution rollout beyond whatever substrate hooks are already needed
- compiler-first workflow authoring before native dataflow and reusable plan semantics exist
- fast runtime-executed machine-only subgraph execution as a required v1 path

## 5. Phases

### Phase 0: Freeze sequencing and spec boundaries

Create or update the implementation-target specs that this roadmap will drive.

This includes:

- one spec for the coordination artifact/review model implementation slice
- one spec for the SQL-only coordination authority cutover
- one spec for the shared execution substrate core
- one spec for warm-state validation on the substrate
- one spec for first-class `Action`
- one spec for graph-wide dataflow and bindings
- one spec for reusable native plan definitions and instantiation
- one spec for JS/TS-authored compiled plans and the workflow-authoring SDK

Exit criteria:

- each roadmap phase has a concrete target spec
- the current sequencing is reflected in those specs instead of staying only in prose

### Phase 1: Implement the coordination artifact/review model

Implement the contract in:

- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)

This phase should settle:

- artifact identity and storage shape
- review identity and storage shape
- lifecycle and query surfaces for artifact and review facts
- provenance and evidence attachment points used by later validation and action work

Exit criteria:

- artifact and review state is durable, queryable, and no longer needs placeholder result shapes
- later validation and action execution can attach evidence to the real model

Status note (2026-04-09):

- complete
- declared artifact and review requirements now round-trip through coordination mutations, canonical/shared-query surfaces, MCP payloads, and JS-facing views

### Phase 2: Hard-cut to a SQL-only coordination authority model

Implement the SQL-only cutover described in:

- [../specs/2026-04-09-sql-only-coordination-authority-cutover.md](../specs/2026-04-09-sql-only-coordination-authority-cutover.md)

This phase should settle:

- removal of the `git_shared_refs` coordination authority backend from supported product paths
- a SQL-only authority contract shared by SQLite and future Postgres implementations
- authoritative append semantics for coordination mutations on the DB-backed default path
- explicit recovery-only snapshot replacement paths kept out of the hot mutation API
- shaped current-state authority reads and retention-aware history reads aimed at SQL projections
- migration of core product call sites, CLI surfaces, MCP surfaces, and tests off shared-ref authority assumptions

This phase should explicitly exclude:

- implementing the shared execution substrate itself
- shipping Postgres as a production backend
- changing coordination-domain semantics unrelated to the authority contract

Exit criteria:

- no supported product path can configure or select `git_shared_refs` as the coordination authority backend
- no product-facing authority write path requires a caller-built full coordination snapshot on the SQL-backed path
- no hot current-state product read path depends on loading the full coordination snapshot by default
- SQLite remains functional behind the tightened seam
- Postgres can be implemented later without emulating shared-ref or snapshot-shaped authority semantics

### Phase 3: Implement the shared execution substrate core

Implement the common lower layer described in:

- [../designs/2026-04-09-shared-execution-substrate.md](../designs/2026-04-09-shared-execution-substrate.md)

This phase should settle:

- execution record model
- capability classes and runtime capability posture
- runner contract and result envelope
- runtime routing and service orchestration
- retries, timeout, budget, and provenance semantics

This phase should explicitly exclude:

- event-trigger feature rollout
- first-class Action graph semantics
- validation-specific lifecycle semantics

Exit criteria:

- PRISM has one shared execution core that can host multiple semantic entrypoints
- validation and Actions can both target it without inventing parallel stacks

### Phase 4: Move warm-state validation onto the shared execution substrate

Implement the validation-specific integration described in:

- [../designs/2026-04-09-warm-state-validation-feedback.md](../designs/2026-04-09-warm-state-validation-feedback.md)

This phase should settle:

- validation execution records on the substrate
- validation runner family
- capability and routing rules for validation execution
- validation result mapping back into task correctness and completion gating

Exit criteria:

- warm-state validation no longer uses a bespoke execution stack
- validation semantics remain special, but execution plumbing is shared

### Phase 5: Add `Action` as a first-class graph leaf

Implement the graph and lifecycle model described in:

- [../designs/2026-04-09-actions-and-machine-work.md](../designs/2026-04-09-actions-and-machine-work.md)

This phase should settle:

- native `Action` identity and graph placement
- Action lifecycle
- Action execution over the shared substrate
- Action provenance, retry, timeout, and structured outputs

Exit criteria:

- PRISM has an explicit machine-work leaf type
- machine execution no longer has to hide behind task overloading or event-only semantics

### Phase 6: Implement graph-wide typed inputs, outputs, and bindings

Implement the graph dataflow model described in:

- [../designs/2026-04-09-graph-dataflow-and-parameterization.md](../designs/2026-04-09-graph-dataflow-and-parameterization.md)

This phase should settle:

- typed value and reference representation
- declared outputs for tasks, Actions, and reviews where appropriate
- bound inputs across plans, tasks, Actions, artifacts, validations, and reviews
- query and UI surfaces for bindings and produced outputs

Exit criteria:

- PRISM graph nodes can exchange typed data explicitly
- downstream execution no longer depends on ad hoc hidden shared-state lookup

### Phase 7: Implement reusable native plan definitions and instantiation

Build on Phase 5 to add:

- native plan-definition versus plan-instance separation
- plan input schemas
- instance binding of plan inputs into child nodes
- reusable instantiation and lineage
- later continue-as-new hooks if needed

This phase should remain native-IR-first.

It should not yet depend on JS/TS-authored workflow definitions.

Exit criteria:

- PRISM-native reusable plans exist before the compiler targets them
- the plan instance model is stable enough to pin compiled artifacts to it

### Phase 8: Implement JS/TS-authored compiled plans and the workflow-authoring SDK

Implement the authoring layer described in:

- [../designs/2026-04-09-compiled-workflows-to-prism-ir.md](../designs/2026-04-09-compiled-workflows-to-prism-ir.md)

This phase should settle:

- workflow-authoring SDK surface
- CLI-first compiler entrypoint
- Rust-hosted JS/TS evaluation using the `prism-js` stack
- compiled artifact metadata, source maps, and artifact-pin provenance
- lowering into the now-stable native plan/dataflow/action IR

Runtime-hosted compilation may begin here if the native target is already stable, but it should
remain secondary to the CLI-first path.

Exit criteria:

- humans and agents can author plans in JS/TS and compile them into PRISM-native IR
- service, query, and UI layers remain IR-driven rather than workflow-code-driven

### Phase 9: Evaluate fast runtime-executed machine-only subgraphs

Only after the previous phases land should PRISM evaluate whether demand justifies:

- a fast runtime-executed lane for bounded machine-only subgraphs
- append-log plus snapshot local execution state for that runtime hot path

This phase is explicitly optional.

It should be skipped entirely unless real workloads justify it.

Exit criteria:

- either the optimization is explicitly declined as unnecessary
- or a separate roadmap is opened for the bounded runtime-fast-lane work

## 6. Practical dogfooding checkpoints

The roadmap should produce useful dogfooding value before the compiler lands.

### Checkpoint A: After Phase 4

PRISM should already have:

- the real artifact/review evidence model
- a non-snapshot authority contract
- a shared execution substrate
- warm-state validation on top of it

That is enough to dogfood validation seriously.

### Checkpoint B: After Phase 5

PRISM should additionally have:

- first-class `Action`

That is enough to dogfood machine-executed graph leaves without waiting for JS/TS authoring.

### Checkpoint C: After Phase 7

PRISM should additionally have:

- graph dataflow and bindings
- reusable native plan definitions and instantiation

That is enough to dogfood the real native workflow model before the compiler arrives.

## 7. Anti-patterns

Avoid the following:

- starting with the JS/TS compiler before the native target stabilizes
- letting event-trigger work delay the shared execution substrate, validation, or Actions
- overloading `Task` to absorb machine-execution semantics that belong to `Action`
- inventing one-off validation execution plumbing after the shared substrate exists
- adding implicit dataflow through hidden shared state instead of explicit bindings
- letting compiled workflow authoring redefine the native graph model instead of targeting it

## 8. Recommendation

PRISM should execute this program in a foundation-first order:

1. artifact and review model
2. coordination authority abstraction and storage-contract refactor
3. shared execution substrate
4. warm-state validation
5. Actions
6. graph dataflow and bindings
7. native reusable plan definitions
8. JS/TS-authored compiled plans

That order gives PRISM the strongest result with the least churn:

- durable evidence first
- stable authority and persistence semantics second
- one execution model third
- graph semantics before authoring ergonomics
- compiler last, targeting a stable native IR instead of forcing the IR to chase the compiler
