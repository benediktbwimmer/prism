# `prism_code` Compiler Implementation Roadmap

Status: in progress
Audience: compiler, prism-js, prism-mcp, prism-core, prism-cli, runtime, coordination, and UI maintainers
Scope: the full end-to-end implementation of the `prism_code` compiler and its integration into PRISM, from compiler foundations through runtime integration, fixture coverage, durable writes, and fully compiled reusable plans

---

## 1. Summary

This roadmap tracks the full implementation of the `prism_code` compiler as a first-class PRISM
subsystem.

At the end of this roadmap:

- `prism_code` is fully functional as PRISM's canonical programmable surface
- pure read-only `prism_code` executes through the intended interpreted runtime path
- transactional write-capable `prism_code` executes through the intended JIT-lowered compiler path
- reusable repo-authored plans compile into PRISM Execution IR
- compile-plus-instantiate works through the same compiler family
- the SDK is the same surface the compiler/runtime exposes
- the old mutation API is gone from the product path
- the fixture corpus covers the language and semantic promises PRISM makes

This roadmap is the trackable implementation plan for the architecture defined in:

- [../designs/2026-04-10-prism-code-compiler-architecture.md](../designs/2026-04-10-prism-code-compiler-architecture.md)
- [../designs/2026-04-10-prism-code-and-unified-js-sdk.md](../designs/2026-04-10-prism-code-and-unified-js-sdk.md)
- [../adrs/2026-04-10-prism-code-canonical-surface.md](../adrs/2026-04-10-prism-code-canonical-surface.md)

## 2. Process rules

These rules are part of this roadmap, not optional guidance.

## REVIEW RULE: NO PHASE MAY BE CLAIMED FINISHED UNTIL IT IS CAREFULLY AND HONESTLY REVIEWED

Every phase in this roadmap must be reviewed directly against
[../designs/2026-04-10-prism-code-compiler-architecture.md](../designs/2026-04-10-prism-code-compiler-architecture.md)
before it is allowed to count as complete.

This review must be careful and honest, not optimistic.

If the implementation is only directionally aligned, partially scaffolded, still builder-shaped, or
still dependent on mutation-era or host-op-era semantics where the design requires compiler-owned
semantics, the phase is not complete.

When a review finds that a phase is not 100% faithful to the compiler design, that phase must be
reopened and iterated until it is fully faithful before the roadmap may advance.

### 2.1 No separate implementation spec files

This compiler implementation should not spawn a new series of phase-specific spec files.

The controlling documents for this work are:

- the compiler architecture design
- this implementation roadmap

If a compiler phase needs clarification, update one of those two documents directly instead of
creating another implementation spec.

### 2.2 Explicit review gate between every phase

Every phase in this roadmap must be reviewed explicitly with the user before work begins on the next
phase.

That means:

- complete the current phase
- review it carefully and honestly against the compiler architecture design
- summarize what landed, what remains, and what changed in assumptions
- identify every place where the implementation is not yet 100% faithful to the design
- reopen the phase if any such gap exists
- review that phase together with the user
- iterate until the implementation is 100% faithful to the design
- only then advance the roadmap status to the next phase

There should be no silent rollover into the next compiler phase.

### 2.3 Hard-cut rule

This roadmap is a hard-cut implementation program.

The compiler work must not introduce:

- compatibility seams back to the old mutation API
- dual-stack write paths intended to survive between phases
- a second public SDK surface
- a second runtime path for write-capable `prism_code`

## 3. End-state definition

This roadmap is complete only when all of the following are true.

- `prism_code` supports all four execution products:
  - interpreted read-only evaluation
  - transactional write evaluation
  - reusable plan compilation
  - compile-plus-instantiate
- the implementation matches the compiler architecture design rather than the old builder bridge
- `prism_code` is the only canonical public programmable surface
- durable writes no longer flow through mutation-era internals
- reusable plans compile to PRISM Execution IR with structured control flow preserved
- the compiler/runtime and SDK are driven from one surface registry
- the fixture corpus provides strong coverage for the supported JS/TS language features and PRISM
  semantics

## 4. Status

Current phase checklist:

- [x] Phase 0: freeze roadmap, architecture, and review process
- [x] Phase 1: build the compiler substrate and surface registry foundations
- [ ] Phase 2: implement PRISM Program IR and effect classification
- [ ] Phase 3: implement structured transactional lowering for current write semantics
- [ ] Phase 4: cut runtime `prism_code` execution onto the compiler path
- [ ] Phase 5: hard-remove mutation-era product paths and finalize the SDK surface
- [ ] Phase 6: build the fixture corpus and language-feature coverage harness
- [ ] Phase 7: compile reusable repo-authored plans into PRISM Execution IR
- [ ] Phase 8: implement compile-plus-instantiate and finish end-to-end integration
- [ ] Phase 9: close validation, performance, and ergonomics gaps

Current active phase:

- Phase 3: structured transactional lowering for current write semantics remains in progress

Current phase note:

- Phase 0 is complete.
- Phases 1-3 are reopened.
- The current implementation contains useful compiler substrate work, but it was claimed complete
  too early relative to the compiler architecture design.
- Phase 1 is now back at the explicit review gate.
- The canonical `prism-js` surface registry now owns both the read/query and compiler/write SDK
  contract rather than splitting those definitions across separate `query_surface` and
  `compiler_surface` inventories.
- The Phase 1 fidelity rework moved the compiler-owned `work`, `claim`, `artifact`,
  `coordination`, `plan`, and `task` runtime surface into registry-driven runtime generation
  inside `prism-js`, replacing the previously hand-authored compiler-specific runtime methods and
  handle methods.
- The latest Phase 1 slice folds compiler metadata directly into the canonical API registry so
  runtime option-key generation, API declarations, compiler method lookup, and compiler runtime
  registry generation all derive from the same method inventory.
- The first Phase 1 slice establishes a compiler-owned
  `prism-js` surface registry and uses that registry to drive the runtime prelude and
  typechecked method surface for current compiler-owned SDK entry points.
- Phase 1 also establishes a dedicated `prism_code_compiler` module family in
  `prism-mcp`, a typed compiler-input envelope that classifies all four execution products, and a
  shared source-loading foundation for inline snippets and repo-authored modules under
  `.prism/code/**`.
- The final Phase 1 slice moves TypeScript program preparation, typechecking, and transpilation
  under the compiler module boundary so runtime entry points no longer hand-wire those frontend
  steps directly.
- Phase 2 is now back at the explicit review gate.
- `PRISM Program IR` in `crates/prism-mcp/src/prism_code_compiler/program_ir.rs` now carries
  explicit control-region metadata for sequence, parallel, branch, loop, short-circuit,
  try/catch/finally, function boundary, callback boundary, reduction, and competition forms.
- The semantic model now preserves region inputs, outputs, guard spans, exit modes, binding
  captures, flow operations, and source binding provenance instead of only coarse region/effect
  tags.
- Phase 2 analysis in `crates/prism-mcp/src/prism_code_compiler/analysis.rs` now classifies the
  full effect taxonomy required by the design: pure compute, hosted deterministic input,
  coordination read, authoritative write, reusable artifact emission, and external execution
  intent.
- The analyzer now distinguishes plain arrow/function boundaries from callback boundaries,
  propagates child-region semantics into governing control regions, and exercises those semantics
  through dedicated compiler tests.
- The live `prism_code` runtime path still runs semantic analysis before transpilation and records
  a dedicated `typescript.*.semanticAnalysis` phase, so the richer Program IR is exercised on the
  real runtime path rather than only through isolated unit tests.
- The reopened Phase 2 fidelity pass closes the gaps that previously kept the phase open.
- Imported helper functions are now analyzed across `.prism/code/**` module boundaries, including
  exported const arrow helpers and re-export chains, and callsites surface callee effects through
  explicit invocation summaries.
- Program IR now carries a first-class invocation model with explicit invocation kinds, target
  regions, visible effect kinds, possible exit modes, module/export metadata, and class-method
  metadata rather than only coarse effect-site tagging.
- The analyzer now preserves `for await...of`, labeled `break` and `continue`, richer competition
  semantics for `Promise.race` and `Promise.any`, and reduction metadata that preserves
  accumulator/element binding identity, initial-value presence, and iteration-order semantics.
- Class helper semantics now survive across local declarations, class expressions, imported
  classes, re-exported classes, namespace-imported classes, instance construction through `new`,
  and both static and instance method invocation.
- Phase 2 semantic analysis now gives `Promise.resolve` and `Promise.reject` explicit intrinsic
  async outcome semantics instead of treating them as undifferentiated ordinary calls.
- Private class methods and private fields are now preserved in semantic analysis and invocation
  resolution where the underlying frontend exposes them, including `this.#method()` and
  `this.#field` access inside class helpers.
- The semantic layer now explicitly rejects forbidden ambient constructs promised by the design,
  including raw `Date.now()`, raw `Math.random()`, `eval`, `Function`, and dynamic import calls.
- The final reopened safety pass closes the remaining restricted-feature gaps from the compiler
  design: raw `new Date()`, `with`, ambient UUID generators, uncontrolled runtime filesystem /
  network / process modules, uncontrolled network fetches, `Proxy`, reflection-driven object
  semantics mutation, monkey-patching the `prism` SDK, mutation of ambient globals, and
  unbounded effectful recursion are now rejected with explicit semantic diagnostics.
- Multi-module Program IR merging now preserves export metadata, invocation metadata,
  instance-class bindings, and class-method metadata instead of dropping those structures during
  the merge step.
- The current targeted validation bar for this Phase 2 boundary is:
  - `cargo test -p prism-mcp prism_code_compiler -- --nocapture`
  - `cargo test -p prism-mcp mcp_server_executes_native_prism_code_coordination_builders -- --nocapture`
  - `cargo test -p prism-mcp mcp_server_executes_mixed_prism_code_writes_in_one_invocation -- --nocapture`
- Phase 2 now appears faithful to the compiler design, but it remains at the explicit review gate
  until the user confirms that assessment.
- Phase 3 is active again after the Phase 2 fidelity pass.
- Phase 3 replaces the old `prism_code_builder` runtime path with a compiler-owned
  write-runtime and lowering layer under
  `crates/prism-mcp/src/prism_code_compiler/write_runtime.rs`.
- Current write-capable `prism_code` now stages structured coordination and direct-write
  operations in one lowering plan, flushes coordination batches through compiler-owned lowering,
  and resolves handles from the lowered results instead of accumulating eager mutation payloads in
  the old staged builder.
- The current write runtime now preserves region lineage and effect order in a dedicated
  `StructuredTransactionPlan`, and same-invocation coordination and mixed direct-write flows are
  again validated on the live MCP path.
- `dryRun` now exercises that same lowering path and skips only the final write execution step.
- Coordination rejections from the transactional write path are now surfaced as `prism_code`
  invocation errors instead of silently leaving provisional handle previews unresolved.
- The Phase 3 durability gap at the coordination transaction boundary is now closed:
  `intentMetadata` is accepted by `prism-query` transactions, carried through the protocol-state
  contract, and stamped onto the newly emitted coordination events as durable
  `transactionIntent` metadata.
- Phase 3 is now at the explicit review gate. The current implementation appears faithful to the
  compiler design for structured transactional lowering of today's write semantics, but it must be
  reviewed honestly against the compiler architecture before Phase 4 begins.
- Targeted MCP regressions now cover dry-run, the existing native coordination builder flow,
  mixed coordination/direct writes in one invocation, and claim/review follow-up flows on the new
  lowering path.
- The next work must first make Phases 1-3 fully faithful to the compiler architecture design
  before Phase 4 may begin.

## 5. Ordering thesis

The compiler should be implemented in this order:

1. freeze the implementation roadmap and the architecture contract
2. build one shared compiler substrate and surface registry
3. define the internal semantic representation and effect model
4. make current write-capable `prism_code` lower through structured compiler paths
5. hard-cut runtime execution onto that compiler path
6. remove mutation-era product paths and finalize the SDK contract
7. prove the language contract with a strong fixture corpus
8. add reusable plan compilation on the same compiler core
9. finish compile-plus-instantiate and cross-surface integration
10. close validation and performance gaps without changing the architectural contract

This order is deliberate.

It avoids:

- building fixture coverage against the wrong compiler core
- building reusable-plan compilation on a runtime path that still depends on mutation-era lowering
- pretending the SDK is stable before the compiler path actually owns it

## 6. Phase dependencies

These dependencies are strict.

- Phase 0 before Phase 1
- Phase 1 before Phase 2
- Phase 2 before Phase 3
- Phase 3 before Phase 4
- Phase 4 before Phase 5
- Phase 5 before Phase 6
- Phase 6 before Phase 7
- Phase 7 before Phase 8
- Phase 8 before Phase 9

User review is required between every dependency edge.
No dependency edge may be crossed until the preceding phase is both reviewed and found 100%
faithful to the compiler design.

## 7. Phases

### Phase 0: Freeze roadmap, architecture, and review process

Lock the implementation program around:

- the compiler architecture design
- this roadmap
- the hard-cut rule
- the explicit review gate rule

Exit criteria:

- the compiler architecture design is detailed enough to guide implementation
- this roadmap is accepted as the execution plan
- the broader execution rollout points at this roadmap for compiler delivery tracking

### Phase 1: Build the compiler substrate and surface registry foundations

Implement the shared foundations that every compiler mode will depend on.

This phase should establish:

- one compiler crate or module family boundary with clear ownership
- one surface registry that defines the compiler-owned SDK
- one generated or centralized runtime prelude path driven from that registry
- one compiler entry model shared by:
  - inline `prism_code`
  - fixtures
  - repo-authored modules
- one shared source-loading and module-resolution model for `.prism/code/**`
- one typed compiler-input envelope that can classify:
  - read-only execution
  - transactional write execution
  - reusable artifact compilation
  - compile-plus-instantiate

Exit criteria:

- the surface registry exists and owns the SDK contract
- the runtime prelude is generated from or directly driven by that registry
- compiler entry points no longer require ad hoc hand-wiring per capability family

### Phase 2: Implement PRISM Program IR and effect classification

Implement the semantic core.

This phase should establish:

- PRISM Program IR as a concrete internal representation
- effect classification for:
  - pure compute
  - hosted deterministic inputs
  - coordination reads
  - authoritative writes
  - reusable artifact emission
  - external execution intent
- source binding tracking
- control-region tracking for:
  - sequence
  - parallel
  - branch
  - loop
  - short-circuit
  - try/catch/finally
  - function and callback boundaries
  - reduction and competition semantics

Exit criteria:

- PRISM Program IR exists as code, not just doc prose
- effect classification is explicit in the implementation
- source spans and source bindings survive into Program IR

### Phase 3: Implement structured transactional lowering for current write semantics

Replace the staged builder and host-op write model with real compiler-owned lowering.

This phase should:

- lower current supported write semantics through PRISM Program IR
- preserve structured control semantics instead of flattening everything to ad hoc op lists
- lower the current mutable PRISM object set:
  - plans
  - tasks
  - dependencies
  - claims
  - artifacts
  - reviews
  - declared work
- support mixed read-plus-write snippets in one invocation
- support `dryRun` on the same lowering path

Exit criteria:

- current write-capable `prism_code` no longer depends on the old staged builder model
- writes are compiler-owned and source-structured
- `dryRun` uses the same lowering path and skips only final commit

### Phase 4: Cut runtime `prism_code` execution onto the compiler path

Move the live runtime onto the new compiler.

This phase should:

- route write-capable inline `prism_code` through the compiler runtime path
- keep pure reads on the intended interpreted path
- ensure write-capable programs analyze the whole snippet and stage effects correctly
- ensure post-write reads observe staged transactional state
- ensure runtime diagnostics are source-mapped and compiler-owned

Exit criteria:

- `prism_code` runtime execution uses the compiler path for writes
- the runtime no longer depends on mutation-era write bridges
- read-only and write-capable classification is enforced by the compiler runtime

### Phase 5: Hard-remove mutation-era product paths and finalize the SDK surface

Remove residual product-path traces of the old mutation model and lock the compiler-owned SDK.

This phase should:

- remove remaining `prism_mutate` product-path code
- remove remaining mutation-era tool schemas, validation paths, transport routes, and guidance
- finalize the concrete SDK families and handle surfaces
- ensure docs, runtime, and fixture surfaces all use the same SDK contract

Exit criteria:

- no mutation-era product path remains
- the SDK contract is compiler-owned and singular
- runtime docs and generated docs teach the same API the compiler implements

### Phase 6: Build the fixture corpus and language-feature coverage harness

Turn the architecture promises into executable proof.

This phase should establish a fixture corpus for:

- pure reads
- mixed read/write snippets
- all currently supported PRISM-native writes
- branches
- loops
- `Promise.all`
- `Promise.allSettled`
- `Promise.race`
- `Promise.any`
- `switch`
- `try/catch/finally`
- functions, closures, and callbacks
- `map`, `flatMap`, `filter`, `reduce`, `forEach`, `some`, `every`, `find`, and `findIndex`
- hosted deterministic inputs
- negative cases for disallowed dynamic features

The harness should validate at minimum:

- classification
- diagnostics
- Program IR shape where relevant
- transactional lowering shape where relevant
- runtime execution results where relevant

Exit criteria:

- the fixture corpus exists and is broad
- it is easy to add new fixtures for later roadmap semantics
- the coverage harness proves the current language and semantic contract

### Phase 7: Compile reusable repo-authored plans into PRISM Execution IR

Implement the reusable-plan side of the compiler on the same core.

This phase should:

- compile `.prism/code/plans/**/*.ts` into PRISM Execution IR
- preserve structured regions in Execution IR
- emit provenance and artifact identity metadata
- emit compatibility metadata for compiler version, SDK version, and IR version
- keep the service on compiled artifacts rather than authored source

Exit criteria:

- reusable plan compilation exists end to end
- PRISM Execution IR is structurally compiled, not flattened
- compiled artifacts are hashable, cacheable, and versioned

### Phase 8: Implement compile-plus-instantiate and finish end-to-end integration

Finish the integrated execution model.

This phase should:

- support compile-plus-instantiate through the same compiler family
- integrate reusable plan compilation with authoritative instantiation
- ensure inline `prism_code`, repo modules, fixtures, and runtime all share the same core surface
- preserve source-level handles and diagnostics through instantiation

Exit criteria:

- all four execution products work end to end
- compile-plus-instantiate is not a side path
- the runtime and authority layers consume compiled or lowered results, not authored source

### Phase 9: Close validation, performance, and ergonomics gaps

After functional completeness, harden the system.

This phase should close:

- latency regressions on read-only execution
- avoidable overhead in transactional lowering
- fixture blind spots
- diagnostics rough edges
- SDK/documentation mismatches
- cache or normalization gaps affecting hashing and artifact identity

Exit criteria:

- the implementation is functionally complete and operationally credible
- performance and ergonomics are good enough to proceed with later roadmap phases building on it

## 8. Validation rules

Validation for this roadmap should follow the repository validation policy and the phase-specific
needs of compiler work.

Minimum expected bars:

- targeted crate tests after each code phase
- downstream tests when the public SDK or runtime path changes
- fixture harness runs as soon as Phase 6 lands
- full workspace tests when the compiler cutover changes shared runtime, transport, or schema-level
  behavior broadly enough to warrant it

## 9. Anti-patterns

The following are explicit failure modes for this roadmap.

- continuing to grow the old staged builder as though it were the real compiler
- letting runtime writes bypass the compiler path
- treating a fixture corpus as optional polish
- introducing a second hand-maintained SDK contract
- allowing mutation-era terminology to survive on the product path
- flattening loops, branches, or fan-out aggressively just to make the first lowering pass easier
- silently progressing into the next phase without the required user review

## 10. Relationship to the broader rollout

This roadmap is the detailed delivery plan for the compiler-related portion of:

- [2026-04-09-execution-substrate-and-compiled-plan-rollout.md](./2026-04-09-execution-substrate-and-compiled-plan-rollout.md)

That broader roadmap should continue to track ecosystem-level sequencing.

This roadmap should track the compiler implementation itself.
