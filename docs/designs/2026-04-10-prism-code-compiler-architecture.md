# PRISM Code Compiler Architecture

Status: proposed design  
Audience: compiler, prism-js, prism-mcp, prism-core, prism-cli, coordination, query, runtime, UI, and service maintainers  
Scope: the full compiler and runtime architecture for `prism_code`, including interpreted reads, JIT-lowered writes, fully compiled reusable plans, the JS/TS language contract, structured control-flow IR, deterministic hosted inputs, diagnostics, provenance, and validation

---

## 1. Summary

PRISM should treat `prism_code` as a real programming model, not a transport wrapper around older
mutation payloads.

The target architecture is:

- one canonical authored surface: `prism_code`
- one JS/TS SDK family exposed through that surface
- interpreted execution for pure reads
- JIT lowering for write-capable inline `prism_code`
- full structural compilation for reusable repo-authored plans
- explicit persisted PRISM Execution IR as the durable truth for reusable orchestration structure
- explicit native authoritative transactions as the durable truth for direct transactional writes

The service must never execute authored source as the durable workflow truth.

The compiler must capture ordinary async JS/TS control flow and lower it into structured PRISM
control-flow and graph semantics.

The compiler must not flatten everything into a giant static DAG.

Instead, it must preserve explicit control structures such as:

- sequence
- parallel fan-out and fan-in
- conditional branches
- loops
- short-circuiting expressions
- try/catch/finally regions
- function and callback boundaries
- collection combinators such as `map`, `flatMap`, `filter`, `reduce`, `some`, `every`, and
  `find`

This design is intentionally ambitious. It aims to cover nearly all ordinary valid JS/TS used in
practical authored PRISM code, with a small explicitly documented set of disallowed dynamic
features.

This document describes the intended implementation target, not a compromise target.

## 2. Relationship to existing docs

This document is the detailed compiler architecture companion to:

- `docs/adrs/2026-04-10-prism-code-canonical-surface.md`
- `docs/designs/2026-04-10-prism-code-and-unified-js-sdk.md`
- `docs/designs/2026-04-09-compiled-workflows-to-prism-ir.md`
- `docs/specs/2026-04-10-full-prism-code-compiler-cutover-phase-7b.md`

Those documents establish the public surface and rollout direction.

This document defines the actual compiler and runtime model that should make that direction true.

It is also a living architecture reference.

As the roadmap advances and PRISM gains new native semantics, this document should be extended
rather than bypassed. In particular, later phases should add or refine compiler coverage for:

- Actions
- validators
- graph dataflow and parameter bindings
- reusable plan composition
- execution-intent and execution-result semantics
- selective materialization and partial hydration policies

## 3. Goals

The compiler architecture must satisfy all of the following.

- Compile ordinary async JS/TS authored code into PRISM-native semantics.
- Preserve one public programming surface for both reads and writes.
- Keep reads fast by interpreting pure read-only programs rather than emitting durable artifacts for
  them.
- Ensure every durable write goes through structured lowering rather than imperative state mutation.
- Support repo-authored reusable plans under `.prism/code/plans/`.
- Support repo-authored shared libraries under `.prism/code/libraries/`.
- Support the currently supported PRISM-native objects and operations through the compiler path.
- Preserve explicit structured control flow in compiled output rather than flattening everything.
- Enable future selective materialization for branches, loops, and large fan-outs.
- Keep deterministic inputs explicit and provenance-bearing.
- Provide strong source-mapped diagnostics and excellent ergonomics for both humans and agents.
- Eliminate client ids from the public programming model.
- Make the SDK, runtime prelude, docs, and compiler surface derive from one shared definition
  family.

## 4. Non-goals

This document does not require:

- inventing a separate declarative DSL for plan authoring
- making the service a general guest-code host
- interpreting durable plan structure directly from source at service runtime
- preserving the legacy `prism_query` or `prism_mutate` transport model
- flattening all loops or branches into fully expanded task graphs before persistence

## 5. Core principles

### 5.1 Code defines the graph, IR remains the truth

Authored JS/TS code defines the graph and control structure by compilation.

Compiled or lowered output, not authored source, is the durable operational truth:

- PRISM Execution IR for reusable plan artifacts
- explicit native authority transactions for direct transactional writes

The service must consume those compiled or lowered results and must never treat authored source as
the long-lived workflow truth.

### 5.2 One invocation is one transaction boundary

For inline `prism_code`:

- one invocation is one bounded execution
- if the program is read-only, it returns values
- if the program performs writes, all writes stage and commit once at the end
- if execution throws, nothing commits

### 5.3 Whole-program analysis, selective lowering

Write-capable code must undergo whole-program parsing, typing, and semantic analysis.

That does not mean every expression becomes durable IR.

Instead:

- pure read and pure compute parts may remain interpreted
- writes and the control structure that governs them must be lowered explicitly
- later reads in the same invocation execute against staged transactional state

### 5.4 No public mutation-era abstractions

The compiler architecture must not expose or depend on:

- public client ids
- legacy mutation action catalogs
- ad hoc schema-payload stitching as the authored model
- compatibility bridges from `prism_code` back into the old mutation tool surface

## 6. Source origins

The compiler must support two source origins through one shared pipeline.

### 6.1 Inline `prism_code`

Inline snippets sent through MCP, CLI, or future UI surfaces.

These are primarily used for:

- read-only queries
- ad hoc transactional writes
- live-plan extension
- direct task, claim, artifact, and review interaction

### 6.2 Repo-authored modules

Repo-authored source under:

```text
.prism/
  code/
    plans/
    actions/
    runners/
    validators/
    libraries/
```

These modules are used for:

- reusable plans
- shared helper logic
- future Action and validation modules
- future compile-time reusable composition

The source origin changes packaging and caching, not compiler semantics.

## 7. Execution modes

The compiler/runtime must classify code into one of four execution products.

### 7.1 Read-only evaluation

For pure reads:

- parse and typecheck
- bind the `prism` SDK
- enforce deterministic capability rules
- interpret directly
- return the computed value

No durable IR emission is required.

### 7.2 Transactional write evaluation

For inline code that mutates authoritative state:

- parse and typecheck the whole program
- classify effects
- interpret pure JS/TS evaluation
- JIT-lower PRISM writes and governing control structures
- stage all writes against a transactional state model
- commit once at the end

The durable result is a committed native authority transaction plus provenance, not necessarily a
persisted reusable artifact.

### 7.3 Reusable plan compilation

For repo-authored reusable plans:

- parse and typecheck
- analyze and lower the full structural orchestration logic
- emit PRISM Execution IR
- emit artifact provenance and identity

These artifacts may later be instantiated many times without rediscovering their structure from
source.

### 7.4 Compile-plus-instantiate

Some calls may:

- compile a reusable plan artifact
- instantiate it immediately into authoritative state

The compiler and runtime should treat these as a composition of the previous two modes rather than
a separate programming model.

## 8. Compiler products

The architecture should distinguish four representations.

### 8.1 Typed source program

The parsed and typed JS/TS program with resolved modules, symbols, and source locations.

This should be based on an existing JS/TS frontend rather than a custom parser.

### 8.2 PRISM Program IR

An internal structured semantic representation of authored PRISM code.

This is not yet the persisted execution artifact.

It should capture:

- effectful operations
- source-mapped lexical bindings
- control-flow regions
- callback and function boundaries
- loop semantics
- branch semantics
- exception regions
- staged data dependencies relevant to PRISM structure

This document refers to that internal representation as PRISM Program IR.

PRISM Program IR should be:

- transient by default
- cacheable when useful for incremental compilation or repeated evaluation
- introspectable in tests and compiler diagnostics
- never treated as the durable runtime truth
- normalized enough for stable diagnostics, stable tests, deterministic caching behavior, and
  predictable lowering

#### 8.2.1 PRISM Program IR node families

PRISM Program IR should be concrete enough that the compiler can reason about authored programs
without falling back to ad hoc callbacks or mutation lists.

At minimum it should contain explicit node families for:

- module and symbol boundaries
- lexical bindings and handle bindings
- pure compute expressions that are relevant to effectful control flow
- hosted deterministic input reads
- coordination reads
- authoritative write operations
- reusable artifact emission operations
- region entry and exit points
- source spans and provenance anchors

#### 8.2.2 PRISM Program IR control-region forms

PRISM Program IR should contain explicit structured region forms for:

- sequence
- parallel
- branch
- loop
- short-circuit
- try/catch/finally
- function and callback invocation
- fold or reduction
- competition or first-completion semantics for `Promise.race` and `Promise.any`

Each region should be able to name:

- inputs
- outputs
- guards or conditions
- effect sites
- exit modes such as normal completion, break, continue, throw, return, and short-circuit exit

#### 8.2.3 PRISM Program IR binding model

PRISM Program IR must preserve a first-class model of source bindings.

That includes:

- lexical variables
- parameter bindings
- closure captures
- handle-producing bindings
- staged durable entity bindings

This is required so diagnostics and later lowering can speak in source terms rather than internal
temporary ids.

### 8.3 PRISM Execution IR

The durable compiled orchestration artifact for reusable plans.

It must preserve structured control flow rather than flattening everything into a single expanded
graph.

PRISM Execution IR is not an expanded leaf-node list.

It must be able to persist explicit:

- sequence regions
- parallel regions
- branch regions
- loop regions
- exception regions
- guards
- materialization boundaries

PRISM Execution IR should also be canonicalized enough for:

- stable golden tests
- deterministic hashing
- artifact identity
- compile-cache reuse

#### 8.3.1 PRISM Execution IR entity families

PRISM Execution IR should persist explicit durable orchestration entities, not just abstract control
regions.

At minimum it should be able to represent:

- plans
- tasks
- dependencies and dependency kinds
- artifacts
- reviews
- validation-related nodes or references
- future Action nodes
- future dataflow and parameter-binding edges
- region-scoped inputs and outputs
- policy and scheduling metadata
- provenance references back to source and compiled artifact metadata

#### 8.3.2 PRISM Execution IR structural regions

PRISM Execution IR should preserve region structure directly rather than encoding it only as
flattened edges.

Structured regions should include at minimum:

- sequence region
- parallel region
- branch region
- loop region
- exception region
- reduction region
- competition region

Each durable region should be able to express:

- entry conditions
- exit conditions
- join semantics
- materialization policy
- child region or entity membership
- provenance to the source construct that produced it

#### 8.3.3 Selective materialization hooks

PRISM Execution IR should retain enough structure for future selective materialization without
semantic redesign.

That means loop, branch, and high-fan-out regions should be able to carry durable policy metadata
for:

- eager materialization
- lazy materialization
- bounded incremental expansion
- partial hydration for reads and scheduling

### 8.4 Native authority transactions

The durable commit form for direct transactional writes.

These are not reusable artifacts, but they must still be explicit and structured enough to preserve
the semantics of the authored program.

## 9. Compiler and runtime evaluator split

The architecture should distinguish two closely related but different subsystems.

### 9.1 Compiler frontend and lowering pipeline

This subsystem is responsible for:

- parsing
- module resolution
- typechecking
- symbol binding
- capability and determinism analysis
- effect analysis
- control-flow analysis
- PRISM Program IR construction
- lowering to PRISM Execution IR or native authority transactions
- source maps and compile diagnostics

This is the part that understands authored code as a language.

### 9.2 Runtime evaluator and transaction engine

This subsystem is responsible for:

- executing pure read and pure compute regions
- invoking hosted deterministic capabilities
- maintaining staged transactional state
- evaluating write-capable programs against that staged state
- invoking lowering and commit at the right boundaries
- returning source-level results, effect summaries, and commit metadata

This is the part that executes a bounded invocation.

### 9.3 Why this split matters

Without this split, the implementation is likely to collapse into one large compiler-runtime blob
that is:

- hard to reason about
- hard to test
- hard to cache
- hard to optimize
- hard to extend to reusable plan compilation

The compiler frontend should own language understanding.

The runtime evaluator should own bounded invocation execution.

## 10. High-level compiler pipeline

The compiler/runtime should be organized as explicit phases.

### 10.1 Parse and module resolution

Responsibilities:

- parse JS/TS
- resolve imports within the controlled `.prism/code/**` space and approved runtime modules
- build source maps
- reject disallowed module resolution patterns

### 10.2 Typecheck and symbol binding

Responsibilities:

- run TS typechecking
- resolve lexical bindings and function scopes
- identify exported and imported callable symbols
- reject incompatible SDK calls early where possible

### 10.3 Capability and determinism analysis

Responsibilities:

- identify use of hosted capabilities
- reject ambient nondeterminism
- classify unsafe language or platform features
- determine whether the program is read-only, write-capable, or artifact-emitting

### 10.4 Control-flow and effect analysis

Responsibilities:

- build structured control-flow understanding
- identify PRISM effects and where they occur
- analyze callback bodies passed through higher-order functions
- track short-circuiting and exception behavior
- compute staging requirements for transaction execution

### 10.5 PRISM Program IR construction

Responsibilities:

- translate effectful source structure into a structured internal IR
- preserve source-level handles and lexical provenance
- represent loops, branches, fan-outs, and exception scopes explicitly
- attach dataflow and dependency metadata needed for later lowering
- preserve enough symbolic structure that later roadmap phases can add Actions, validators, and
  dataflow without redesigning the frontend model

### 10.6 Lowering

Responsibilities:

- lower PRISM Program IR into either:
  - native authority transactions for inline writes, or
  - PRISM Execution IR for reusable plans
- perform validation passes
- preserve provenance
- preserve structured regions rather than flattening them away

### 10.7 Execution and commit

Responsibilities:

- interpret read-only portions
- maintain staged transactional state for write-capable programs
- commit once at the end if not in `dryRun`
- surface source-mapped diagnostics and effect summaries

## 11. Effect taxonomy

The compiler needs an explicit effect model.

Without one, “whole-program analysis, selective lowering” becomes too fuzzy to implement
consistently.

### 11.1 Pure compute

Pure compute includes expressions and control flow that:

- do not touch hosted capabilities
- do not read PRISM state
- do not write PRISM state

Pure compute is interpreted and never becomes a durable effect by itself.

### 11.2 Hosted deterministic input effects

These are explicit reads from PRISM-hosted deterministic capability surfaces such as:

- `prism.time.*`
- `prism.random.*`
- `prism.fs.*`
- `prism.config.*`
- `prism.env.*`

They are not authoritative writes, but they are still effects because they must be:

- capability-scoped
- provenance-bearing
- visible to compilation and diagnostics

### 11.3 Coordination read effects

These are explicit reads from PRISM state and read models.

They may remain interpreted, but they still matter for:

- classification
- auth
- staged execution semantics
- diagnostics

### 11.4 Authoritative write effects

These are operations that change durable PRISM state, including:

- plan and task mutations
- dependency wiring
- claim lifecycle changes
- artifact and review lifecycle changes
- declared work operations

These must always lower explicitly before commit.

### 11.5 Reusable artifact emission effects

These are operations that produce durable reusable compiled artifacts, especially reusable plans in
PRISM Execution IR.

They require full structural compilation rather than just transactional lowering.

### 11.6 External execution intent effects

PRISM will eventually need to distinguish:

- intent to schedule or invoke work
- actual observed execution results

This document treats that distinction as part of the effect taxonomy even where the exact future
surface is not fully implemented yet.

### 11.7 Effect boundaries in control flow

The compiler must understand not just where effects exist, but what control structure governs them.

That includes:

- branch guards
- loop boundaries
- exception boundaries
- callback boundaries
- short-circuit conditions
- function-call boundaries

This is why the compiler must analyze the whole program even when only some subregions emit durable
results.

## 12. Language support target

The compiler should aim to support almost all normal, deterministic JS/TS used in authored PRISM
code.

The guiding rule is:

- ordinary language features should work unless they make effect analysis, determinism, provenance,
  or structured lowering impossible

The compiler should not require users to learn a pseudo-JS subset just because PRISM is compiling
their code.

## 13. Language support classes

The language contract should be explicit about how features participate in PRISM semantics.

### 13.1 Fully supported

These features are allowed in effectful code and must lower correctly when they govern or emit
PRISM effects.

Examples include:

- ordinary bindings and expressions
- async and `await`
- branches
- loops
- `try` / `catch` / `finally`
- supported higher-order combinators
- functions, closures, and callbacks

### 13.2 Allowed in read-only or pure-compute regions

Some features may be acceptable when they remain in pure compute or read-only portions of the
program, even if they are not suitable to govern PRISM effects directly.

When such features appear in effectful regions, the compiler must either:

- prove they remain semantically harmless, or
- reject them with a precise diagnostic

### 13.3 Rejected in effectful regions

Features that obscure effect analysis, provenance, or determinism must be rejected when they
participate in or govern PRISM effects.

This classification does not weaken the ambition of the compiler.

It makes the language contract precise.

## 14. JS/TS syntax and semantics the compiler must support

### 14.1 Modules and imports

Must support:

- ES modules
- named imports and exports
- default imports and exports
- namespace imports
- re-exports
- relative imports within `.prism/code/**`
- imports from approved PRISM-provided SDK modules
- type-only imports in TS

Must preserve:

- module boundaries for source maps and diagnostics
- callable import analysis when imported functions contain PRISM effects

### 14.2 Basic expressions and bindings

Must support:

- `const`, `let`, and `var`
- literals, template literals, tagged templates if deterministic
- object and array literals
- property access and element access
- destructuring
- rest and spread
- default parameter values
- assignment and compound assignment
- nullish coalescing
- optional chaining
- ternary expressions
- logical `&&` and `||` with short-circuit semantics

### 14.3 Functions

Must support:

- function declarations
- function expressions
- arrow functions
- anonymous functions
- nested functions
- async functions
- closures over lexical variables
- returning handles and other PRISM-bound objects from functions
- helper functions split across modules

Compiler requirement:

- effectful code must not become opaque merely because it crosses a function boundary

The compiler must analyze and lower PRISM effects inside:

- locally declared functions
- imported helper functions
- anonymous callbacks passed into supported combinators

### 14.4 Classes

Must support ordinary deterministic JS/TS classes used as helper abstractions.

That includes:

- class declarations
- methods
- static methods
- private fields where the underlying frontend supports them
- object construction through `new`

The compiler does not need to lower classes as a special orchestration construct.

It does need to preserve correct semantics when PRISM effects occur inside class methods.

### 14.5 Async semantics

Must support:

- `async` / `await`
- `Promise.resolve`
- `Promise.reject`
- `Promise.all`
- `Promise.allSettled`
- `Promise.race`
- `Promise.any`

Semantics:

- `await` creates an explicit ordering boundary
- `Promise.all` creates an explicit parallel fan-out and join region
- `Promise.allSettled` creates a parallel fan-out with explicit outcome aggregation
- `Promise.race` and `Promise.any` must lower to explicit competition or first-success control
  semantics rather than pretending all branches always complete the same way

### 14.6 Conditionals

Must support:

- `if` / `else`
- `switch`
- conditional expressions
- short-circuiting boolean operators

The compiler must represent conditional structure explicitly in PRISM Program IR and PRISM
Execution IR rather than flattening all branches eagerly.

### 14.7 Loops

Must support:

- `for`
- `for...of`
- `for...in` where deterministic iteration semantics are acceptable
- `while`
- `do...while`
- `break`
- `continue`
- labeled loop control if the frontend can represent it cleanly

The compiler must support loop constructs structurally in IR.

It must not require all loops to be flattened into a fixed list of nodes.

This is essential for:

- large fan-outs
- long-running iteration
- future selective materialization

### 14.8 Higher-order collection combinators

Must support higher-order functions over arrays and known finite collections when their callbacks
contain reads or writes.

At minimum:

- `map`
- `flatMap`
- `filter`
- `reduce`
- `forEach`
- `some`
- `every`
- `find`
- `findIndex`

Compiler requirements:

- callback bodies must remain analyzable
- effectful callbacks must lower to explicit structured regions
- short-circuiting combinators such as `some`, `every`, and `find` must preserve short-circuit
  semantics
- `reduce` must preserve fold semantics rather than being treated as a crude loop synonym

`reduce` deserves special care.

Effectful `reduce` is allowed only when the accumulator and callback semantics remain analyzable and
lowerable as an explicit fold region.

If the reducer obscures accumulator identity, control structure, or write ordering beyond what the
compiler can model faithfully, it must be rejected with a source-mapped diagnostic rather than
silently degraded.

### 14.9 Exceptions

Must support:

- `throw`
- `try`
- `catch`
- `finally`

The compiler must represent exception regions explicitly.

`finally` must always be honored in lowering because it can contain reads, writes, and cleanup logic
that is semantically required even after earlier effects.

### 14.10 Async iteration and streams

Must support:

- `for await...of`
- async iterables

This is important for future streaming and selective materialization patterns.

### 14.11 Recursion

Support policy:

- pure recursive helpers may be supported normally
- effectful recursion that lowers to durable orchestration structure must be bounded or otherwise
  structurally analyzable

Explicitly unsupported for now:

- unbounded effectful recursion with no statically or semantically knowable termination condition

### 14.12 TypeScript-specific features

Must support:

- interfaces
- type aliases
- generics
- discriminated unions
- enums after normal TS lowering
- `as` assertions
- `satisfies`
- overloaded function signatures where TS can resolve them

These primarily matter for authoring ergonomics and type safety rather than special lowering rules.

## 15. Explicitly unsupported or restricted features

The compiler should be broad, but not unbounded.

The following should be explicitly rejected or tightly restricted.

### 15.1 Ambient nondeterminism

Rejected:

- raw `Date.now()`
- raw `new Date()` when used as a nondeterministic source
- raw `Math.random()`
- ambient UUID libraries that bypass PRISM provenance capture

Use hosted equivalents instead.

### 15.2 Uncontrolled filesystem and network access

Rejected on the authored-code surface:

- arbitrary `fs` access
- arbitrary shelling out
- uncontrolled subprocess execution
- uncontrolled network fetches

All such capabilities must go through explicit PRISM-hosted surfaces.

### 15.3 Dynamic code execution

Rejected:

- `eval`
- `Function` constructor
- dynamic runtime source generation used as executable code

### 15.4 Meta-object tricks that destroy analyzability

Rejected or heavily restricted:

- `with`
- arbitrary `Proxy`
- runtime mutation of object semantics through reflection in ways that hide PRISM effects
- monkey-patching the SDK

### 15.5 Arbitrary dynamic imports

Restricted:

- imports must resolve through approved module resolution rules
- arbitrary user-computed dynamic import targets should be rejected

### 15.6 Hidden global side effects

Rejected:

- mutation of ambient globals
- hidden shared mutable state across compiler invocations

### 15.7 Unbounded effectful recursion

Rejected until the compiler has a principled way to represent and materialize it.

## 16. Hosted deterministic capability surface

Dynamic inputs are useful and should be supported, but only through explicit hosted capabilities.

### 16.1 Time

Support:

- `prism.time.now()`
- future richer time helpers if needed

Must record:

- input kind
- value
- timestamp or origin metadata

### 16.2 Randomness

Support:

- `prism.random.uuid()`
- future explicit seeded or recorded randomness helpers

Must record:

- generated value
- source provenance

### 16.3 Filesystem

Support at minimum:

- `prism.fs.readText(path)`
- `prism.fs.readLines(path, { start, end })`
- `prism.fs.readAround(path, { line, before, after })`
- `prism.fs.exists(path)`
- `prism.fs.glob(pattern)`
- `prism.fs.stat(path)`
- `prism.fs.readJson(path)`
- `prism.fs.readToml(path)`
- `prism.fs.readYaml(path)`

Must record:

- path
- read mode
- range or window parameters when applicable
- file digest
- slice or payload digest

### 16.4 Config and environment

Support only through explicit allowlisted PRISM surfaces such as:

- `prism.config.get(...)`
- `prism.env.get(...)`

Ambient environment access should not be allowed directly.

### 16.5 Network

The design allows future hosted network inputs, but they should remain disabled until provenance,
caching, and determinism policy are strong enough.

## 17. PRISM-native semantics the compiler must cover

The compiler path must cover all currently supported PRISM-native operations.

That includes at minimum:

- plan creation, reopening, updating, and archiving
- task creation, reopening, updating, completion, handoff, accept-handoff, resume, and reclaim
- dependency creation and structural plan edits
- claim acquire, renew, and release
- artifact propose, review, and supersede
- declared work bootstrap
- mixed read-plus-write programs
- dry-run execution

As later roadmap items land, the same compiler must extend to support:

- Actions
- validations
- dataflow and parameter bindings
- reusable plan composition

This document should be updated when those semantics become concrete enough to define:

- new effect classes
- new durable entity families
- new structured region forms
- new lowering rules
- new fixture families

## 18. Structured control-flow semantics

This is the core of the compiler.

### 18.1 Sequence regions

Sequential authored control flow must lower to explicit sequence regions.

### 18.2 Parallel regions

Parallel authored control flow must lower to explicit parallel regions with join semantics.

This includes:

- `Promise.all`
- effectful `map` when the semantics are parallel
- future explicit concurrency helpers if introduced

### 18.3 Branch regions

Conditionals must lower to explicit branch regions with:

- guard conditions
- per-branch subregions
- join behavior after the branch

### 18.4 Loop regions

Loops must lower to explicit loop regions with:

- loop input state
- iteration body
- break and continue exits
- termination semantics
- room for future selective materialization

### 18.5 Exception regions

`try` / `catch` / `finally` must lower to explicit exception-handling regions.

### 18.6 Short-circuit regions

Short-circuit boolean and collection combinator semantics must be represented explicitly when they
govern PRISM effects.

### 18.7 Function-call regions

Function and callback calls that contain PRISM effects must remain visible in PRISM Program IR.

Inlining is not required everywhere, but semantic visibility is.

### 18.8 Region metadata requirements

Every structured region that survives into PRISM Program IR or PRISM Execution IR should be able to
carry:

- source span
- region kind
- input bindings
- output bindings
- governing guards or predicates
- effect classification
- exit modes
- provenance references
- future materialization policy

## 19. Why structured IR matters

PRISM must not flatten every authored program into a giant static DAG because that would:

- explode large fan-outs
- destroy long-loop representation
- make future selective materialization difficult or impossible
- erase branch and exception semantics
- make UI and query layers less informative
- make provenance worse

Structured IR is required for:

- scalability
- future scheduler optimization
- future partial graph hydration
- future lazy materialization of loops and branches

## 20. Artifact boundary rules

The architecture needs an explicit decision rule for when authored code produces:

- a direct transactional write
- a reusable compiled artifact
- both

### 20.1 Direct transactional execution

A program should be treated as direct transactional execution when its durable intent is:

- mutate existing authoritative state
- create or update live plans and tasks directly
- attach claims, artifacts, reviews, or work declarations
- return a source-level result from that one bounded invocation

The durable product is a committed authority transaction, not a reusable artifact.

### 20.2 Reusable artifact compilation

A program should be treated as reusable artifact compilation when its durable intent is:

- define reusable orchestration structure
- produce a plan artifact intended for later instantiation
- emit versioned, reviewable, cacheable compiled output

The durable product is PRISM Execution IR plus artifact metadata.

### 20.3 Compile-plus-instantiate

A program may explicitly request both:

- compile a reusable artifact
- instantiate it immediately

In that case:

- artifact compilation remains a first-class output
- instantiation remains a first-class authoritative write output

### 20.4 Mixed code does not imply mixed products accidentally

The compiler/runtime should not guess loosely from incidental structure.

The authored surface should make the durable intent explicit enough that the runtime can classify
the program deterministically into:

- read-only evaluation
- transactional execution
- reusable artifact compilation
- compile-plus-instantiate

## 21. Read/write execution semantics

### 21.1 Pure reads

Pure reads:

- are interpreted
- may use the same SDK and module system
- return source-level values
- do not emit durable structural artifacts

### 21.2 Write-capable programs

Write-capable programs:

- are still interpreted for pure compute portions
- are JIT-lowered for effects and governing control structures
- execute against staged transactional state
- may continue reading after writes
- return values shaped from staged state or commit results

The runtime must not assume that only statements before the first write matter.

It must analyze and execute the whole program because later branches, loops, catches, and finally
blocks may still affect the final durable result.

### 21.3 Reusable plan compilation

Reusable plan modules:

- should be structurally compiled into PRISM Execution IR
- should not depend on re-running the full authored source at each instantiation

## 22. Function and callback boundary rules

The compiler must preserve effect visibility across:

- local helpers
- imported helpers
- callbacks
- closures
- methods
- nested anonymous functions

Rules:

- effectful code is not allowed to become opaque simply because it crossed a function boundary
- closure-captured values must remain traceable for provenance and error reporting
- callbacks passed to supported combinators must remain semantically visible to the compiler

## 23. Source-level handles and durable entities

The authored model uses ordinary source bindings, not client ids.

That needs a precise statement.

### 23.1 Source bindings

Source-level bindings may refer to:

- pure local values
- hosted input results
- read-model views
- transient compiler handles
- durable graph entities
- staged transactional entities that will become durable on commit

### 23.2 Compiler responsibility

The compiler must preserve enough provenance to map durable structure back to source bindings and
source spans.

That means diagnostics should refer to:

- local variable names
- function names
- callback locations
- loop or branch sites

rather than internal graph ids or lowering handles.

### 23.3 Runtime responsibility

The runtime may still use transient internal handles while staging writes or assembling compiled
output.

Those handles must remain internal and must not leak into the public programming model.

## 24. SDK architecture

The SDK should be defined from one canonical surface registry.

That registry should drive:

- runtime host binding generation
- JS prelude generation
- TS type definitions
- docs generation
- example generation
- fixture generation helpers where useful

This is how the SDK becomes easy to build and extend:

- the runtime surface and SDK surface are the same surface

The SDK must expose:

- read helpers
- write helpers
- object handles
- hosted capabilities
- plan authoring helpers
- future Action and validator helpers

It must not expose:

- mutation-era payload assembly
- client ids
- legacy compatibility wrappers

### 24.1 Root SDK families

The concrete compiler-owned SDK should expose at least these root families:

- `prism.coordination`
- `prism.claim`
- `prism.artifact`
- `prism.review`
- `prism.work`
- `prism.time`
- `prism.random`
- `prism.fs`
- `prism.config`
- `prism.env`

Read-model and discovery helpers may continue to live on `prism` directly or under additional
read-oriented families, but the mutable orchestration surface should be organized around these
roots.

### 24.2 Handle and view taxonomy

The SDK should distinguish explicitly between:

- immutable read views
- mutable handles
- pure local values

#### 24.2.1 Immutable read views

Read-oriented APIs should return serializable immutable views when the caller wants to inspect
state.

Examples:

- plan summary views
- task detail views
- claim status views
- artifact detail views
- review detail views

Read views are:

- read effects only
- safe in read-only snippets
- never themselves mutation-capable

#### 24.2.2 Mutable handles

Mutation-capable APIs should return first-class handles.

At minimum the compiler model needs these handle families:

- `PlanHandle`
- `TaskHandle`
- `ClaimHandle`
- `ArtifactHandle`
- `ReviewHandle`

These handles may represent:

- an existing durable entity
- a staged transactional entity
- a staged compiled entity during reusable plan compilation

They must preserve stable source-level binding semantics even if the runtime uses transient
internal handles under the hood.

#### 24.2.3 Async rule

All SDK methods that touch hosted PRISM state or hosted capabilities should be async and awaitable.

That includes:

- reads against PRISM state
- opening mutation-capable handles
- creating durable entities
- mutating durable entities
- hosted deterministic input reads

This keeps the authored model uniform and allows the compiler to reason about ordering through
ordinary async control flow.

### 24.3 Read-only versus write-capable use of handles

Opening a mutable handle is not itself necessarily a write.

Examples:

- `await prism.coordination.plan(planId)` is a coordination read effect
- `await prism.coordination.task(taskId)` is a coordination read effect
- `await prism.artifact.open(artifactId)` is a coordination read effect

Writes happen when the program invokes mutation-capable methods such as:

- `plan.update(...)`
- `plan.archive()`
- `plan.addTask(...)`
- `task.update(...)`
- `task.dependsOn(...)`
- `task.complete(...)`
- `claim.release()`
- `artifact.supersede()`
- `artifact.review(...)`

This distinction is important because read-only snippets may still open handles for navigation and
inspection as long as they never invoke authoritative write methods.

### 24.4 Coordination surface

The concrete compiler-owned coordination surface should look like a normal async object API rather
than a mutation payload builder.

At minimum:

- `prism.coordination.createPlan(input): Promise<PlanHandle>`
- `prism.coordination.plan(planId): Promise<PlanHandle>`
- `prism.coordination.task(taskId): Promise<TaskHandle>`

Future extensions may add additional openers such as:

- `prism.coordination.review(reviewId)`
- `prism.coordination.node(kind, id)`

#### 24.4.1 `PlanHandle`

`PlanHandle` should expose at minimum:

- `id`
- `provisional` or equivalent staged-state marker when applicable
- `update(input): Promise<PlanHandle>`
- `archive(): Promise<PlanHandle>`
- `addTask(input): Promise<TaskHandle>`
- `task(input): Promise<TaskHandle>` as the ergonomic alias for `addTask`

`PlanHandle.addTask(...)` should support at minimum:

- `title`
- `status`
- `dependsOn`
- `assignee`
- `anchors`
- `acceptance`
- `artifactRequirements`
- `reviewRequirements`

Dependency references supplied through `dependsOn` should accept:

- existing task ids
- `TaskHandle` bindings
- arrays of either

The authored surface must not require client ids or mutation-payload stitching.

#### 24.4.2 `TaskHandle`

`TaskHandle` should expose at minimum:

- `id`
- `provisional` or equivalent staged-state marker when applicable
- `update(input): Promise<TaskHandle>`
- `dependsOn(taskOrTasks, options?): Promise<TaskHandle | void>`
- `complete(input?): Promise<TaskHandle>`
- `handoff(input): Promise<TaskHandle>`
- `acceptHandoff(input?): Promise<TaskHandle>`
- `resume(input?): Promise<TaskHandle>`
- `reclaim(input?): Promise<TaskHandle>`

`TaskHandle.update(...)` should support at minimum:

- `title`
- `summary`
- `status`
- `assignee`
- `priority`
- `dependsOn`
- `anchors`
- `acceptance`
- `validationRefs`
- `tags`
- `artifactRequirements`
- `reviewRequirements`

`TaskHandle.dependsOn(...)` must support both:

- explicit dependency wiring in source
- implicit control-flow dependency inference elsewhere in the compiler

These two models must coexist without conflict.

#### 24.4.3 Structural edits versus field updates

The SDK must make structural edits first-class.

That includes:

- creating plans
- adding tasks
- wiring dependencies
- future branch, loop, and reusable subgraph construction

The compiler must not reduce the public surface to field patching alone.

### 24.5 Claim surface

At minimum the claim surface should expose:

- `prism.claim.acquire(input): Promise<ClaimHandle>`
- `prism.claim.open(claimId): Promise<ClaimHandle>`

`ClaimHandle` should expose at minimum:

- `id`
- `renew(input?): Promise<ClaimHandle>`
- `release(): Promise<void>`

`prism.claim.acquire(...)` should support at minimum:

- `anchors`
- `capability`
- `mode`
- `ttlSeconds`
- `agent`
- `coordinationTaskId`

`coordinationTaskId` should accept either:

- a task id
- a `TaskHandle`

### 24.6 Artifact and review surface

The artifact and review model should expose first-class artifact and review handles rather than
forcing review operations to remain anonymous side effects.

At minimum:

- `prism.artifact.propose(input): Promise<ArtifactHandle>`
- `prism.artifact.open(artifactId): Promise<ArtifactHandle>`
- `prism.review.open(reviewId): Promise<ReviewHandle>`

`ArtifactHandle` should expose at minimum:

- `id`
- `supersede(input?): Promise<ArtifactHandle>`
- `review(input): Promise<ReviewHandle>`

`prism.artifact.propose(...)` should support at minimum:

- `taskId`
- `kind`
- `title`
- `summary`
- `uri` or equivalent external reference field when applicable
- `metadata`
- future provenance- and content-oriented fields

`taskId` should accept either:

- a task id
- a `TaskHandle`

#### 24.6.1 `ReviewHandle`

Even if the currently implemented runtime semantics are still thin, the compiler-owned SDK should
already treat reviews as first-class entities.

`ReviewHandle` should expose at minimum:

- `id`
- `update(input): Promise<ReviewHandle>`
- `complete(input): Promise<ReviewHandle>`

`ReviewHandle.complete(...)` should support at minimum:

- `decision`
- `summary`
- `evidenceRefs`
- future richer review payload fields as the roadmap advances

### 24.7 Declared work surface

At minimum:

- `prism.work.declare(input): Promise<WorkHandle | WorkView>`

The exact handle versus view split for declared work may evolve, but it must still participate in
the same compiler-owned SDK registry and effect model.

### 24.8 SDK effect classification by method family

The surface registry should explicitly classify each method as one of:

- pure local helper
- hosted deterministic input read
- coordination read
- authoritative write
- reusable artifact emission

This classification should drive:

- auth requirements
- read-only versus write-capable snippet classification
- compiler lowering rules
- docs generation
- error reporting

### 24.9 Same SDK in all compiler entry points

The same SDK contract must apply in:

- inline `prism_code`
- repo-authored reusable plan modules
- fixtures
- future Actions and validators once those roadmap items land

When later roadmap phases add new semantics, this section should be extended rather than forked.

## 25. Diagnostics

The compiler and runtime must produce strong source-mapped diagnostics.

### 25.1 Error quality

Errors should speak in source-level terms such as:

- unknown plan or task reference
- dependency cycle
- unsupported nondeterministic capability
- effectful recursion is unbounded
- callback used in `reduce` cannot be lowered because it captures unsupported state
- dynamic import target is not allowed
- branch condition depends on unsupported ambient side effect

### 25.2 Prohibited leakages

Diagnostics must not leak:

- client ids
- legacy mutation action names
- internal temp handles
- backend-specific storage details

### 25.3 Explanatory traces

The compiler should be able to explain when helpful:

- how it interpreted control flow
- why a loop or branch was represented structurally
- why a snippet was classified as read-only or write-capable
- which hosted inputs contributed provenance

## 26. Provenance and artifact identity

Every compiled or lowered durable result must preserve provenance.

### 26.1 Reusable artifact identity

Reusable plan artifacts should include at minimum:

- source revision
- compiler version
- PRISM Execution IR version
- controlled input digests
- artifact hash

### 26.2 Version compatibility

The compiler architecture must acknowledge version compatibility explicitly.

Reusable artifacts should carry enough metadata to answer:

- which compiler version produced this artifact
- which SDK surface version it targeted
- which PRISM Execution IR version it targets
- whether the current runtime can execute it directly
- whether recompilation is required because of incompatible compiler, SDK, or IR evolution

The runtime must never silently treat incompatible artifacts as if they were valid.

### 26.3 Source mapping

PRISM Program IR and PRISM Execution IR should carry source provenance granular enough to connect:

- regions
- nodes
- guards
- loop bodies
- handlers

back to authored source.

### 26.4 Transaction provenance

Direct transactional writes should record:

- source origin
- invocation identity
- write classification
- hosted inputs consumed
- commit metadata

## 27. Performance goals

The compiler architecture must be designed for low-latency reads and scalable durable writes.

### 27.1 Reads

Reads should:

- reuse parsing and type information where possible
- avoid durable artifact emission
- remain fast and interactive

### 27.2 Writes

Writes should:

- JIT-lower only the necessary structured semantics
- avoid flattening large loops or fan-outs
- commit once
- expose clear commit receipts

### 27.3 Reusable compilation

Reusable plan compilation should:

- support caching
- support incremental recompilation
- support module-level invalidation

## 28. Validation strategy

Validation must be fixture-first and compiler-first.

### 28.1 Fixture corpus

A comprehensive fixture corpus should cover:

- pure reads
- mixed read/write snippets
- all currently supported PRISM-native writes
- loops
- branches
- `Promise.all`
- `Promise.allSettled`
- `switch`
- `try/catch/finally`
- function-boundary effects
- `map`, `filter`, `reduce`, `find`, `some`, `every`
- supported hosted inputs
- negative cases for forbidden dynamic features

### 28.2 Golden outputs

The test suite should include:

- source fixture
- expected classification
- expected diagnostics or success
- expected PRISM Program IR shape where appropriate
- expected PRISM Execution IR shape for reusable plan compilation
- expected transaction shape or effect summary for transactional writes

### 28.3 Runtime integration tests

Tests must prove that:

- inline `prism_code` goes through this compiler/runtime path
- repo modules go through the same core
- no product path bypasses the compiler for writes

## 29. Explicit checklist for implementation

The implementation should not be considered complete until all of the following exist.

- a shared compiler frontend over JS/TS parsing, typing, and source maps
- program classification into read-only, transactional write, reusable compile, and
  compile-plus-instantiate
- deterministic capability enforcement
- PRISM Program IR
- structured lowering for sequence, parallel, branch, loop, and exception regions
- callback and function-boundary effect analysis
- support for `map`, `flatMap`, `filter`, `reduce`, `forEach`, `some`, `every`, and `find`
- source-mapped diagnostics
- one shared SDK surface registry
- runtime prelude generated from that registry
- fixture corpus with positive and negative cases
- removal of mutation-era product paths and terminology
- explicit artifact-version compatibility metadata and enforcement

## 30. Initial explicitly unsupported edge cases

The goal is to cover nearly all normal authored JS/TS, but the following may remain unsupported in
the first strict compiler delivery as long as they are rejected clearly and intentionally.

- arbitrary `Proxy`-based metaprogramming
- dynamic runtime code generation
- unbounded effectful recursion
- uncontrolled dynamic imports
- effectful dependence on prototype mutation of external objects
- ambient global mutation across compiler invocations

These exceptions must stay small and explicitly documented.

## 31. Decision

PRISM should build this compiler now.

Incremental builder shims are not enough.

The next compiler implementation work should be measured against this architecture, and the Phase
7b delivery spec should use this document as the normative design reference.
