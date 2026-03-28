# PRISM Roadmap

## Current Focus

PRISM has reached the point where the next cycle should prioritize hardening over broadening.

The goal of this roadmap is to turn the current alpha feedback into concrete engineering work:

* tighten correctness where the thesis depends on trust
* enforce workflow rules where coordination is already exposed
* split growing subsystems before they become drag
* align the written docs with the system that now exists
* add integration coverage around the places where subsystems meet

## Priority Order

The recommended order for the next phase is:

1. modular decomposition of large crates
2. documentation alignment and product-surface cleanup
3. lineage correctness and evidence semantics
4. coordination enforcement and policy hardening
5. end-to-end reliability coverage
6. Rust target identity improvements

## 1. Modular Decomposition Of Large Crates

Several crates have reached the size where file-level decomposition will improve change safety and review speed.

The detailed execution plan for this work lives in [MODULARIZATION_PLAN.md](MODULARIZATION_PLAN.md).

Status as of 2026-03-26:

* `prism-store` has been decomposed and no longer centers its implementation in `src/lib.rs`
* `prism-core` has been decomposed into focused modules, with `lib.rs` now acting as a small facade
* `prism-mcp` has been substantially decomposed; the former crate-root monolith is now split across runtime, resource, mutation, view, schema, and test modules
* `prism-query`, `prism-coordination`, `prism-js`, `prism-memory`, `prism-lang-rust`, `prism-curator`, `prism-cli`, and `prism-projections` have also been split so their roots now act as facades instead of implementation sinks
* the first-pass modularization work is largely complete across the core workspace
* the largest remaining work in this area is second-pass cleanup of internal boundaries and selective follow-up splitting where ownership is still too broad

Work:

* finish the second-pass cleanup for the newly split crates, especially where extracted modules are still larger than they should be
* keep `prism-mcp` split along runtime, resources, query host, mutation handlers, session state, schema, and view boundaries instead of letting behavior drift back into the root
* keep `prism-core` split along workspace loading, indexing, watch refresh, patch outcomes, curator support, and session boundaries
* keep `prism-store` split along graph model, store trait, in-memory backend, SQLite schema, graph IO, snapshots, projections, and codecs boundaries
* keep the newer facades in `prism-query`, `prism-coordination`, `prism-js`, `prism-memory`, `prism-lang-rust`, `prism-curator`, `prism-cli`, and `prism-projections` from collapsing back into broad crate-root files
* preserve public APIs while moving internals so refactoring does not create churn for downstream crates
* treat this as a maintainability pass, not a feature pass

Done when:

* large crates no longer concentrate most behavior in one crate-root source file
* subsystem ownership is obvious from module boundaries
* new changes can land with smaller review surfaces
* remaining larger modules represent real ownership boundaries rather than historical crate-root sprawl

## 2. Documentation Alignment And Product-Surface Cleanup

PRISM's docs should describe the system that exists today, not the smaller system that existed a few iterations ago.

Work:

* keep the documented workspace layout in sync with the actual crate set, including `prism-projections`, `prism-coordination`, and `prism-curator`
* keep the memory docs aligned with the current implementation: `SessionMemory` composes episodic, structural, and semantic recall, while `OutcomeMemory` remains a separate event-history store
* document the current automatic patch-outcome flow so it is clear how `ObservedChangeSet` and `PatchApplied` fit together
* reconcile manual CLI patch-recording commands with the newer automatic patch-outcome pipeline
* keep roadmap and spec updates coupled so implementation drift is visible quickly

Done when:

* a new reader can match the docs to the current crate graph and feature set without guessing
* the difference between implemented behavior and future hooks is explicit
* patch outcomes have one clear user-facing story across CLI, core, and docs

## 3. Lineage Correctness And Evidence Semantics

Lineage is central to PRISM's credibility, so evidence labels and ambiguity behavior need to be exact.

Work:

* fix evidence labeling so unmatched `Born` and `Died` events do not claim `FingerprintMatch`
* extend the resolver to represent uncertainty more precisely, including ambiguous, split, and merge-style outcomes where needed
* strengthen matching with contextual signals such as same-container continuity and Git rename or move hints
* surface richer lineage evidence through query and inspection paths so uncertain lineage is visible rather than hidden
* add focused tests around rename, move, split, merge, and uncertain resolution cases

Done when:

* evidence labels mean exactly what they say
* ambiguous lineage is preserved as uncertainty instead of flattened into a false match
* the resolver can explain why a lineage decision was made

## 4. Coordination Enforcement And Policy Hardening

Coordination is already a useful shared inspection model. The next step is to make workflow rules harder to bypass at mutation time.

Work:

* define and enforce a stricter task state machine for task creation, status transitions, review, handoff, and closure
* validate base revision and stale-revision checks consistently at mutation boundaries, not only in read models and blockers
* strengthen claim conflict detection beyond exact anchor overlap, especially around lineage-level overlap and nearby graph neighborhoods
* make policy violations explicit and auditable in coordination events and mutation responses
* add integration tests that cover claims, task updates, artifact review, blockers, and concurrent sessions together

Done when:

* invalid task transitions are rejected consistently
* stale coordination mutations fail at the write path
* contention behavior is predictable under concurrent work

## 5. End-To-End Reliability Coverage

The most important tests now are not isolated crate tests. They are tests for the full system loop.

Work:

* add end-to-end coverage for parse -> `ObservedChangeSet` -> lineage -> memory re-anchoring -> patch outcomes -> projections -> persistence
* add restart and reload tests to prove that persisted history, memory, projections, and coordination state survive process boundaries
* add watcher and incremental reindex tests that exercise automatic outcome recording and curator enqueueing together
* add combined coordination + lineage + persistence scenarios to catch cross-subsystem regressions early

Done when:

* the major Prism loop is exercised by integration tests instead of only unit tests
* persistence and reload behavior are proven for the coupled subsystems
* regressions at crate boundaries are caught before release

## 6. Rust Target Identity Improvements

Package-level identity is good enough for simple workspaces, but Prism should model Rust targets more precisely before larger repositories force the issue.

Work:

* move from package-name-centric identity toward explicit package and target modeling
* represent custom library names, multiple binaries, and other multi-target layouts cleanly
* clarify how `crate_name`, package identity, and target identity interact in the IR and query surface
* add fixture workspaces that stress custom target naming and multi-target packages

Done when:

* Prism can distinguish package identity from compiled target identity
* multi-target Rust workspaces produce stable, unsurprising IDs
* the docs explain the identity model clearly

## Out Of Scope For This Cycle

These areas still matter, but they should not pull focus ahead of the hardening work above:

* broad new language support
* major new agent-facing surface area
* speculative semantic memory expansion before current memory behavior is better documented and tested
* polish work that does not improve correctness, enforcement, or maintainability

## Exit Criteria For The Hardening Cycle

This cycle is successful when the following are all true:

* the docs match the current workspace and feature set
* lineage evidence is more exact and uncertainty is represented honestly
* coordination mutations enforce policy and revision expectations consistently
* the main cross-crate Prism loop is covered by integration tests
* the largest crates stay decomposed into clearer module boundaries
* Rust target identity is defined well enough for non-trivial workspaces
