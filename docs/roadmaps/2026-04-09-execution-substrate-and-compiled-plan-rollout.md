# Execution Substrate, `prism_code`, And Repo-Authored Plans Rollout

Status: in progress
Audience: coordination, execution, validation, runtime, MCP, CLI, UI, compiler, and plan-authoring maintainers
Scope: sequencing the artifact/review model, the `prism_code` hard cutover, the shared execution substrate, validation, Actions, graph dataflow, reusable repo-authored PRISM code, and later richer plan compilation

---

## 1. Summary

PRISM now has several tightly related but still distinct bodies of work:

- implement the coordination artifact/review model
- hard-cut PRISM to a SQL-only coordination authority model
- harden the SQL-only authority seam so it is genuinely Postgres-ready
- finalize the SQL authority contract so Postgres inherits narrow SQL-native seams instead of residual snapshot-era coupling
- narrow the SQL authority query/provider seams so hot callers stop opening coarse stores or reading full current state
- replace generic authority read stores with sharp read-model ports and a final SQL command seam
- hard-cut to `prism_code` and one unified JS/TS SDK
- land the public `prism_code` transport cutover
- land the first real native `prism_code` builder/compiler slice
- establish PRISM Execution IR as the compiled executable target for authored code
- implement one shared execution substrate
- move warm-state validation onto that substrate
- add `Action` as a first-class machine-work leaf
- add graph-wide dataflow and bindings
- add reusable repo-authored PRISM code modules, libraries, and plan instantiation
- extend the same compiler to richer plan compilation and composition

This roadmap exists to sequence those items so that:

- foundational semantics land before richer repo-authored plan authoring
- the canonical interaction model and minimum compiler land before more runtime surface area is built on the old query or mutate split
- graph dataflow lands before richer reusable plan semantics depend on it
- the final authority contract settles before Postgres and before the execution substrate build on it
- event-trigger work is explicitly postponed instead of muddying the critical path

The ordering principle is:

- stabilize the coordination and execution model first
- remove shared-ref authority support and snapshot-shaped authority coupling before more systems build on it
- remove recovery or snapshot APIs from the hot SQL authority seam before Postgres is implemented
- remove the last snapshot-shaped current-state read and storage-policy leaks from the SQL authority contract before the execution substrate builds on it
- replace coarse provider opens with explicit responsibility-scoped openings before the execution substrate builds on the authority layer
- replace generic projection reads with caller-shaped read-model ports before the execution substrate or Postgres build on the wrong query seam
- hard-cut to `prism_code` and land the minimum compiler before more runtime or authoring work lands on `prism_query` and `prism_mutate`
- do not treat a `prism.mutate(...)` bridge as the finished native `prism_code` authoring model
- keep the service as a broker or consumer of compiled results rather than letting it drift into a generic code-execution host
- then stabilize graph dataflow and richer reusable plan semantics
- later extend the already-landed compiler to richer repo-authored plan composition

This roadmap depends on:

- [../adrs/2026-04-10-prism-code-canonical-surface.md](../adrs/2026-04-10-prism-code-canonical-surface.md)
- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)
- [../designs/2026-04-10-prism-code-and-unified-js-sdk.md](../designs/2026-04-10-prism-code-and-unified-js-sdk.md)
- [../specs/2026-04-10-prism-code-hard-cutover-phase-7.md](../specs/2026-04-10-prism-code-hard-cutover-phase-7.md)
- [../specs/2026-04-10-native-prism-code-builder-and-compiler-phase-7b.md](../specs/2026-04-10-native-prism-code-builder-and-compiler-phase-7b.md)
- [../designs/2026-04-09-shared-execution-substrate.md](../designs/2026-04-09-shared-execution-substrate.md)
- [../specs/2026-04-10-shared-execution-substrate-core-phase-8.md](../specs/2026-04-10-shared-execution-substrate-core-phase-8.md)
- [../specs/2026-04-10-warm-state-validation-on-shared-substrate-phase-9.md](../specs/2026-04-10-warm-state-validation-on-shared-substrate-phase-9.md)
- [../designs/2026-04-09-actions-and-machine-work.md](../designs/2026-04-09-actions-and-machine-work.md)
- [../designs/2026-04-09-warm-state-validation-feedback.md](../designs/2026-04-09-warm-state-validation-feedback.md)
- [../designs/2026-04-09-graph-dataflow-and-parameterization.md](../designs/2026-04-09-graph-dataflow-and-parameterization.md)
- [../designs/2026-04-09-compiled-workflows-to-prism-ir.md](../designs/2026-04-09-compiled-workflows-to-prism-ir.md)

## 2. Status

Current phase checklist:

- [x] Phase 0: freeze sequencing and spec boundaries
- [x] Phase 1: implement the coordination artifact/review model
- [x] Phase 2: refactor the coordination authority abstraction and persistence contract
- [x] Phase 3: harden the SQL authority seam for Postgres
- [x] Phase 4: finalize the SQL authority contract for Postgres
- [x] Phase 5: narrow the SQL authority query/provider seams
- [x] Phase 6: replace generic authority reads with sharp SQL read-model and command ports
- [x] Phase 7: hard-cut to `prism_code` and the unified JS/TS SDK
- [ ] Phase 7b: implement the native `prism_code` builder/compiler cutover
- [x] Phase 8: implement the shared execution substrate core
- [ ] Phase 9: move warm-state validation onto the shared execution substrate
- [ ] Phase 10: add `Action` as a first-class graph leaf on the shared execution substrate
- [ ] Phase 11: implement graph-wide typed inputs, outputs, and bindings
- [ ] Phase 12: implement reusable repo-authored PRISM code modules, libraries, and plan instantiation
- [ ] Phase 13: extend the compiler to richer plan compilation and composition
- [ ] Phase 14: evaluate whether fast runtime-executed machine-only subgraphs are warranted

Current active phase:

- Phase 7b: implement the native `prism_code` builder/compiler cutover

Current implementation note (2026-04-10):

- Phase 1 is landed across `prism-coordination`, `prism-query`, `prism-core`, `prism-mcp`, and `prism-js`
- Phase 2 is landed with the SQL-only authority cutover, removing `git_shared_refs` as a supported authority backend and collapsing the normal authority mutation path to DB append semantics
- Phase 3 is landed by splitting the hot authority contract from snapshot and recovery operations, adding an explicit secondary snapshot seam, and removing full-state payloads from normal authority write results
- Phase 4 is landed by replacing the misleading hot `read_plan_state` authority read with a direct current-state read, splitting the SQL authority seam into smaller traits, removing derived-state persistence policy from the public append contract, and replacing `prism_store` persist-result leakage with backend-neutral commit receipts
- Phase 5 is landed by removing the public hot-path full-current-state read, switching the provider to explicit responsibility-scoped openings, and moving hot query callers onto narrower projection/runtime/diagnostics surfaces
- Phase 6 is landed by replacing the generic projection seam with exact authority-stamp and coordination-surface read ports, moving hot callers off canonical-snapshot reads, and keeping broad snapshot/current-state assembly on explicit secondary seams
- Phase 7 is landed only as the public-surface cutover: `prism_code` is now the canonical public programmable surface, public MCP/self-description/schema surfaces no longer advertise `prism_query` or `prism_mutate`, and the transport cutover is complete
- Phase 7b is now the blocking compiler/builder gap: authenticated writes still route through a transitional `prism.mutate(...)` bridge rather than the finished native handle-oriented authoring surface
- Phase 8 is landed with the authority-backed shared execution substrate adapter in `prism-core`, shared execution family, runner, target, and result vocabulary, and migration of the workspace event engine onto substrate-native execution records
- the next blocking work is Phase 7b: replace the public `prism.mutate(...)` bridge with the first real native `prism_code` builder/compiler slice
- the already-landed Phase 8 substrate work remains valid, but later execution-substrate phases should now build on the native builder/compiler seam rather than the transitional bridge
- the implementation target for Phase 9 is now frozen in `docs/specs/2026-04-10-warm-state-validation-on-shared-substrate-phase-9.md`, including the required storage generalization away from event-only durable execution assumptions

## 3. Ordering thesis

This work should no longer postpone the compiler until the end.

The old rationale for delaying compilation was sound when the question was “should reusable plan
authoring arrive before the native target stabilizes?” The answer to that is still no.

But PRISM is now making a different move:

- land the minimum compiler and the canonical `prism_code` surface now
- then extend that same compiler through the later phases

The right order is:

1. land durable artifact and review semantics first
2. remove the shared-ref authority backend and replace the remaining authority contract with a SQL-only seam
3. harden that SQL-only seam so the primary authority contract is the real future Postgres contract
4. finalize that SQL-only contract so the hot reads, writes, and receipts are genuinely SQL-native
5. narrow the remaining query/provider seams so hot callers use explicit responsibility-scoped openings and narrower reads
6. replace generic authority read stores with final caller-shaped SQL read-model ports and a sharp command seam
7. hard-cut to `prism_code` at the public surface
8. land the first real native `prism_code` builder/compiler slice
9. build the shared execution substrate next
10. prove that substrate with warm-state validation
11. widen it to `Action`
12. add graph-wide dataflow and bindings once the node set is real
13. add reusable repo-authored PRISM code modules, libraries, and plan instantiation against that settled graph model
14. then extend the same compiler to richer plan composition and compiled-plan authoring

Event-trigger work is intentionally out of the critical path for this roadmap.

It can return later once the shared substrate, Actions, and richer repo-authored plan composition
are in place.

## 4. Parallel-work guidance

This roadmap is mostly sequential at the phase level, but some prep and supporting work can proceed
in parallel once the dependency edge is clear.

### 4.1 Strictly sequential phase boundaries

These phase transitions are blocking and should not be inverted:

- Phase 1 before Phase 2
- Phase 2 before Phase 3
- Phase 3 before Phase 4
- Phase 4 before Phase 5
- Phase 5 before Phase 6
- Phase 6 before Phase 7
- Phase 7 before Phase 7b
- Phase 7b before Phase 8
- Phase 8 before Phases 9 and 10
- Phases 9 and 10 before Phase 11
- Phase 11 before Phase 12
- Phase 12 before Phase 13

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
  - split the primary authority trait from snapshot/recovery-only operations
  - remove snapshot payloads from hot write-result contracts
  - move snapshot reads behind an explicit secondary seam
  - migrate snapshot consumers onto that secondary seam
  - tighten the Postgres stub against the new contract
  can proceed in parallel if they converge on one SQL-first authority surface
- During Phase 4:
  - replace the hot `read_plan_state` contract with a direct current-state read
  - split the primary SQL authority surface into smaller current-state, mutation, runtime, execution, history, and diagnostics traits
  - remove derived-state persistence policy from the public append contract
  - replace `prism_store` persist-result leakage with backend-neutral commit metadata
  - keep snapshot/recovery access on an explicit standalone seam
  can proceed in parallel if they converge on one final SQL authority contract
- During Phase 5:
  - replace generic provider `open(...)` calls with explicit responsibility-scoped openings
  - remove the public hot-path `read_current_state` contract
  - move hot query callers onto narrower projection-oriented reads
  - keep full snapshot assembly only in explicit snapshot-plus-runtime helpers
  can proceed in parallel if they converge on one final authority query/provider surface
- During Phase 6:
  - define caller-shaped read ports for task queues, review queues, plan detail, runtime leases, execution records, and authority diagnostics
  - define the final SQL command seam for event append and related concurrency semantics
  - move existing callers off generic projection reads and onto exact DTO-returning read ports
  - restrict snapshot/export/recovery access to explicit non-hot admin seams
  - plan SQLite local-first implementations and Postgres production implementations against those same ports
  can proceed in parallel if they converge on one final read-model/command surface
- During Phase 7:
  - `prism_code` transport and tool contract
  - unified JS/TS SDK surface
  - auth and write-gating model for `prism_code`
  - migration of current query and mutation surfaces onto `prism_code`
  can proceed in parallel if they converge on one canonical programmable surface
- During Phase 7b:
  - staged write context for one `prism_code` invocation
  - native builder handles for plans, tasks, and dependency wiring
  - automatic dry-run or commit at end-of-invocation
  - removal of the public `prism.mutate(...)` requirement for normal coordination authoring
  can proceed in parallel if they converge on one native source-level builder/compiler core
- During Phase 8:
  - execution-record storage
  - runner contract definition
  - runtime routing plumbing
  - capability-class plumbing
  can proceed in parallel if they share one settled substrate spec
- During late Phase 8 / early Phase 9:
  - the Phase 9 validation spec and the Phase 10 Actions spec can be prepared in parallel
- During Phase 11:
  - task/action/review binding semantics
  - typed value/reference representation
  - query/UI read-model shaping for bound inputs and outputs
  can proceed in parallel once the dataflow contract is frozen
- During Phase 12:
  - repo-authored PRISM code module layout
  - reusable plan instantiation semantics
  - shared library import and composition rules
  - plan-instance lineage and provenance work
  can proceed in parallel once plan-definition versus plan-instance boundaries are fixed
- During Phase 13:
  - richer plan-authoring control-flow support
  - compiler provenance and artifact-pin metadata
  - repo-authored plan composition beyond the minimum v1 compiler
  can proceed in parallel once the richer compiled target is frozen

### 4.3 Explicitly deferred work

The following should remain out of scope until this roadmap reaches the appropriate phase:

- event-trigger execution rollout beyond whatever substrate hooks are already needed
- richer plan-authoring semantics before the `prism_code` cutover and native builder/compiler slice land
- fast runtime-executed machine-only subgraph execution as a required v1 path

## 5. Phases

### Phase 0: Freeze sequencing and spec boundaries

Create or update the implementation-target specs that this roadmap will drive.

This includes:

- one spec for the coordination artifact/review model implementation slice
- one spec for the SQL-only coordination authority cutover
- one spec for the Postgres-ready authority seam hardening
- one spec for finalizing the SQL authority contract for Postgres
- one spec for narrowing the SQL authority query/provider seams
- one spec for replacing generic authority reads with sharp SQL read-model and command ports
- one spec for the `prism_code` hard cutover and unified JS/TS SDK
- one spec for the shared execution substrate core
- one spec for warm-state validation on the substrate
- one spec for first-class `Action`
- one spec for graph-wide dataflow and bindings
- one spec for reusable repo-authored PRISM code modules, libraries, and plan instantiation
- one spec for richer plan compilation and composition on top of that compiler

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
- provenance and evidence attachment points used by later validation and Action work

Exit criteria:

- artifact and review state is durable, queryable, and no longer needs placeholder result shapes
- later validation and Action execution can attach evidence to the real model

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

Exit criteria:

- no supported product path can configure or select `git_shared_refs` as the coordination authority backend
- no product-facing authority write path requires a caller-built full coordination snapshot on the SQL-backed path
- no hot current-state product read path depends on loading the full coordination snapshot by default
- SQLite remains functional behind the tightened seam
- Postgres can be implemented later without emulating shared-ref or snapshot-shaped authority semantics

### Phase 3: Harden the SQL authority seam for Postgres

Implement the seam hardening described in:

- [../specs/2026-04-10-postgres-ready-coordination-authority-seam.md](../specs/2026-04-10-postgres-ready-coordination-authority-seam.md)

This phase should settle:

- separation of the primary SQL authority contract from snapshot and recovery-only operations
- removal of full-state payloads from normal authority transaction results
- explicit provider/opening paths for primary authority access versus snapshot/recovery access
- migration of product snapshot consumers onto the explicit secondary seam
- a Postgres stub that targets the same split contract instead of the old SQLite-shaped contract

Exit criteria:

- the main authority contract no longer exposes snapshot replacement on the hot path
- broad snapshot reads live behind an explicit secondary interface
- hot write flows and their results no longer carry caller-visible full-state payloads
- the Postgres backend stub compiles against the same split contract as SQLite

### Phase 4: Finalize the SQL authority contract for Postgres

Implement the final seam hardening described in:

- [../specs/2026-04-10-finalize-sql-coordination-authority-contract.md](../specs/2026-04-10-finalize-sql-coordination-authority-contract.md)

This phase should settle:

- replacement of the misleading hot-path `read_plan_state` API with a direct current-state read
- separation of the primary SQL authority surface into smaller current-state, mutation, runtime, execution, history, and diagnostics traits
- removal of derived-state persistence policy from the public authority append contract
- replacement of `prism_store::CoordinationPersistResult` leakage with backend-neutral authority commit metadata
- a standalone snapshot/recovery seam that no longer inherits the hot authority interface

Exit criteria:

- the hot authority contract no longer exposes `read_plan_state`
- the hot authority contract no longer leaks derived-state persistence policy
- normal authority write receipts no longer expose `prism_store` persistence result types
- the primary authority surface is split into smaller SQL-oriented traits, with one composite facade for callers that want the whole contract
- snapshot/recovery access remains available only through an explicit standalone seam
- SQLite and the Postgres stub compile against that final contract

Status note (2026-04-10):

- complete
- the hot read is now `read_current_state`, transaction receipts carry backend-neutral commit metadata, the SQL authority contract is split into smaller traits, and snapshot access no longer inherits the hot authority seam

### Phase 5: Narrow the SQL authority query/provider seams

Implement the query/provider refinement described in:

- [../specs/2026-04-10-narrow-sql-authority-query-and-provider-seams.md](../specs/2026-04-10-narrow-sql-authority-query-and-provider-seams.md)

This phase should settle:

- removal of the public hot-path `read_current_state` contract from the SQL authority seam
- replacement of coarse provider `open(...)` entrypoints with explicit responsibility-scoped openings
- a projection-oriented hot query surface that exposes only the hot authority reads callers actually need
- migration of product callers onto explicit projection, runtime, mutation, event-execution, diagnostics, history, and snapshot openings
- explicit helper assembly for the few flows that genuinely still need full snapshot-plus-runtime state

Exit criteria:

- the public hot SQL authority seam no longer exposes `read_current_state`
- the authority provider no longer exposes a coarse generic `open(...)` path
- hot query callers use explicit responsibility-scoped openings and narrower reads
- full current-state assembly exists only in explicit helper code that opts into snapshot-plus-runtime composition
- SQLite and the Postgres stub compile against the narrower provider/query surface

Status note (2026-04-10):

- complete
- the provider now opens explicit projection, mutation, runtime, event-execution, history, diagnostics, and snapshot seams, and hot callers no longer read full current state from the public SQL authority contract

### Phase 6: Replace generic authority reads with sharp SQL read-model and command ports

Implement the final authority-read redesign described in:

- [../specs/2026-04-10-sharp-sql-read-model-and-command-ports.md](../specs/2026-04-10-sharp-sql-read-model-and-command-ports.md)

This phase should settle:

- replacement of generic projection reads with caller-shaped read-model ports that return exact DTOs for hot use cases
- replacement of the remaining generic command or store vocabulary with a final SQL command seam centered on authoritative event append and concurrency control
- removal of hot-path dependence on broad payloads like canonical snapshots or generic current-state wrappers
- a backend-neutral query/command contract that SQLite can implement locally first and Postgres can implement for production without API drift
- strict separation between hot read-model/query ports and explicit admin/export/recovery seams

Exit criteria:

- no hot product path depends on generic authority reads like `read_canonical_snapshot_v2`
- hot authority queries are expressed as narrow read-model ports with exact view DTOs
- the SQL command seam is explicit and storage-agnostic, without leaking SQLite or Postgres details
- snapshot/export/recovery access is clearly separate from hot query/command paths
- SQLite and the Postgres stub compile against the new read-model/command surface

Status note (2026-04-10):

- complete
- hot callers now use exact authority-stamp and coordination-surface read ports instead of the generic projection seam or canonical-snapshot reads

### Phase 7: Hard-cut to `prism_code` and the unified JS/TS SDK

Implement the public-surface and compiler pivot described in:

- [../adrs/2026-04-10-prism-code-canonical-surface.md](../adrs/2026-04-10-prism-code-canonical-surface.md)
- [../designs/2026-04-10-prism-code-and-unified-js-sdk.md](../designs/2026-04-10-prism-code-and-unified-js-sdk.md)
- [../specs/2026-04-10-prism-code-hard-cutover-phase-7.md](../specs/2026-04-10-prism-code-hard-cutover-phase-7.md)

This phase should settle:

- `prism_code` as the canonical programmable surface
- one-call, one-transaction semantics in v1
- the public read and transport cutover
- the shared SDK family for reads and writes
- the hard retirement of `prism_query` and `prism_mutate` as target architecture

Exit criteria:

- all new programmable reads and writes target `prism_code`
- later phases no longer need to choose between `prism_query` and `prism_mutate`

Current progress:

- Slice 1 is landed: `prism_code` is the canonical read transport across MCP docs, schemas, resources, and API reference.
- Slice 2 is landed in minimum form: authenticated `prism_code` now supports the first write-capable transport bridge plus `dryRun`.
- Slice 3 is landed: the public MCP transport is now `prism_code`-only, and the public schema/resource/test surface is no longer teaching `prism_query` or `prism_mutate` as target architecture.

Status note (2026-04-10):

- complete as a public-surface cutover
- not sufficient to claim the native builder/compiler experience is complete

### Phase 7b: Native `prism_code` builder/compiler cutover

Implement the native authoring slice described in:

- [../designs/2026-04-10-prism-code-and-unified-js-sdk.md](../designs/2026-04-10-prism-code-and-unified-js-sdk.md)
- [../specs/2026-04-10-native-prism-code-builder-and-compiler-phase-7b.md](../specs/2026-04-10-native-prism-code-builder-and-compiler-phase-7b.md)

This phase should settle:

- staged native write context per `prism_code` invocation
- source-level builders and handles for current coordination authoring
- one-call commit or dry-run semantics without a public `prism.mutate(...)` dependency
- the first real builder/compiler core that later phases extend

Exit criteria:

- plan and task authoring can happen natively through `prism_code`
- dependency wiring uses object handles rather than public mutation payload stitching
- later phases can extend the same native builder/compiler core rather than continue through the
  transitional bridge

Current progress:

- landed: native staged coordination transaction context inside `prism_code`
- landed: native plan creation, plan reopen, task creation, and dependency wiring through builder
  handles
- landed: first native lifecycle methods through task reopen, task update, and task completion
- remaining: extend the same builder/compiler core until normal coordination authoring no longer
  needs the legacy `prism.mutate(...)` escape hatch

### Phase 8: Implement the shared execution substrate core

Implement the common lower layer described in:

- [../designs/2026-04-09-shared-execution-substrate.md](../designs/2026-04-09-shared-execution-substrate.md)

This phase should settle:

- execution record model
- capability classes and runtime capability posture
- runner contract and result envelope
- runtime routing and service orchestration
- retries, timeout, budget, and provenance semantics

Exit criteria:

- PRISM has one shared execution core that can host multiple semantic entrypoints
- validation and Actions can both target it without inventing parallel stacks

### Phase 9: Move warm-state validation onto the shared execution substrate

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

### Phase 10: Add `Action` as a first-class graph leaf

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

### Phase 11: Implement graph-wide typed inputs, outputs, and bindings

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

### Phase 12: Implement reusable repo-authored PRISM code modules, libraries, and plan instantiation

Build on Phase 11 to add:

- repo-authored PRISM code under `.prism/code/**`
- plan-definition versus plan-instance separation
- plan input schemas
- shared library imports and composition
- instance binding of plan inputs into child nodes
- reusable instantiation and lineage
- later continue-as-new hooks if needed

This phase should build on the already-landed `prism_code` compiler rather than introducing a
second authoring model.

Exit criteria:

- repo-authored PRISM code modules and shared libraries exist as a first-class source family
- plan instantiation and lineage are stable enough for richer compiled plan composition later

### Phase 13: Extend the compiler to richer plan compilation and composition

Extend the compiler and authoring model described in:

- [../designs/2026-04-10-prism-code-and-unified-js-sdk.md](../designs/2026-04-10-prism-code-and-unified-js-sdk.md)
- [../designs/2026-04-09-compiled-workflows-to-prism-ir.md](../designs/2026-04-09-compiled-workflows-to-prism-ir.md)

This phase should settle:

- richer plan-authoring control flow and composition
- repo-authored plan-module compilation
- compiled artifact metadata, source maps, and artifact-pin provenance
- lowering into the now-stable native plan/dataflow/action IR from both inline and repo-authored source

Exit criteria:

- humans and agents can compose richer reusable plans out of repo-authored PRISM code modules
- service, query, and UI layers remain IR-driven rather than guest-code-driven

### Phase 14: Evaluate fast runtime-executed machine-only subgraphs

Only after the previous phases land should PRISM evaluate whether demand justifies:

- a fast runtime-executed lane for bounded machine-only subgraphs
- append-log plus snapshot local execution state for that runtime hot path

This phase is explicitly optional.

It should be skipped entirely unless real workloads justify it.

Exit criteria:

- either the optimization is explicitly declined as unnecessary
- or a separate roadmap is opened for the bounded runtime-fast-lane work

## 6. Practical dogfooding checkpoints

The roadmap should produce useful dogfooding value before richer plan composition lands.

### Checkpoint A: After Phase 9

PRISM should already have:

- the real artifact/review evidence model
- a Postgres-ready non-snapshot hot authority contract
- a narrowed responsibility-scoped authority provider/query surface
- sharp caller-shaped SQL read-model and command ports
- a `prism_code` hard cutover and minimum compiler
- a shared execution substrate
- warm-state validation on top of it

That is enough to dogfood validation and the new public interaction model seriously.

### Checkpoint B: After Phase 10

PRISM should additionally have:

- first-class `Action`

That is enough to dogfood machine-executed graph leaves without waiting for richer repo-authored
plan composition.

### Checkpoint C: After Phase 12

PRISM should additionally have:

- graph dataflow and bindings
- reusable repo-authored PRISM code modules, shared libraries, and plan instantiation

That is enough to dogfood the real native plan model before the richer compiler phase arrives.

## 7. Anti-patterns

Avoid the following:

- continuing to build new public surface area on `prism_query` and `prism_mutate` after the `prism_code` decision is made
- letting event-trigger work delay the shared execution substrate, validation, or Actions
- overloading `Task` to absorb machine-execution semantics that belong to `Action`
- inventing one-off validation execution plumbing after the shared substrate exists
- adding implicit dataflow through hidden shared state instead of explicit bindings
- letting richer plan authoring redefine the native graph model instead of targeting it

## 8. Recommendation

PRISM should execute this program in a foundation-first order:

1. artifact and review model
2. SQL-only coordination authority cutover
3. Postgres-ready authority seam hardening
4. final SQL authority contract hardening
5. narrow SQL authority query/provider seams
6. sharp SQL read-model and command ports
7. `prism_code` hard cutover and minimum compiler
8. shared execution substrate
9. warm-state validation
10. Actions
11. graph dataflow and bindings
12. reusable repo-authored PRISM code modules and plan instantiation
13. richer plan compilation and composition

That order gives PRISM the strongest result with the least churn:

- durable evidence first
- stable authority and persistence semantics second
- final Postgres-ready authority seam hardening third
- final SQL-contract hardening fourth
- narrow authority query/provider seams fifth
- sharp SQL read models and command ports sixth
- the canonical code surface and minimum compiler seventh
- one execution model eighth
- graph semantics before richer plan authoring ergonomics
- later plan composition extends the compiler instead of introducing it too late
