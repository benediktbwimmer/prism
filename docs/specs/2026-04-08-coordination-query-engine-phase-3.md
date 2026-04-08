# Coordination Query Engine Phase 3

Status: in progress
Owner: coordination-query
Created: 2026-04-08
Updated: 2026-04-08
Roadmap: [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)
Related contracts:
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)

---

## 1. Summary

Phase 3 implements the **Coordination Query Engine** as a real code seam, not just an informal
collection of `Prism` helpers.

The goal of this phase is:

- centralize coordination reasoning in one engine boundary
- keep `Prism` as a facade over that engine
- stop MCP, CLI, UI, and runtime surfaces from re-deriving coordination semantics ad hoc
- make Phase 4 mutation work and Phase 5 broad cutover target a stable read-side abstraction

The core rule for this phase is:

- product-facing coordination semantics must route through the query engine, not through scattered
  combinations of runtime helpers, `Prism` methods, and surface-local logic

## 2. Scope

This phase is in scope for:

- one canonical coordination-query module boundary in `crates/prism-query`
- query-engine request and result families for task, plan, evidence, review, and queue reads
- extraction of coordination evaluation logic from current `Prism` helper clusters into that engine
- explicit result envelopes that preserve consistency and freshness metadata
- migration of product-facing coordination read call sites onto the engine-facing API
- spec and roadmap updates that reflect the engine as implemented reality

This phase explicitly includes the currently scattered coordination-read families in:

- `crates/prism-query/src/coordination.rs`
- `crates/prism-query/src/plan_insights.rs`
- `crates/prism-query/src/plan_discovery.rs`
- `crates/prism-mcp/src/compact_tools/task_brief.rs`
- `crates/prism-mcp/src/host_resources.rs`
- `crates/prism-mcp/src/query_runtime.rs`
- `crates/prism-mcp/src/ui_read_models.rs`

This phase may continue to rely on lower-level derivation helpers from:

- `crates/prism-coordination`

but those helpers must sit *below* the query-engine seam rather than being consumed directly from
product surfaces.

## 3. Non-goals

This phase does not:

- redesign coordination semantics again
- collapse authority, materialization, and query responsibilities into one layer
- implement the transactional mutation protocol
- solve native spec parsing or spec semantics
- redesign MCP or UI presentation formats from scratch
- require all historical `Prism` convenience methods to disappear immediately

This phase also does not treat branch-local specs as authoritative coordination inputs.

## 4. Current problem statement

Today, coordination reasoning is split across multiple layers:

- `prism-coordination` owns low-level blockers and derivation helpers
- `prism-query` owns `Prism` helper methods for task, plan, and queue reads
- MCP compact tools and UI read models still perform extra shaping and policy interpretation
- product surfaces often know too much about which underlying helper to compose

That leads to drift risks:

- blocker logic can be extended in one surface but not another
- plan rollup logic can diverge between discovery, UI, and compact tools
- evidence and review posture can remain under-modeled in some surfaces
- Phase 4 and Phase 5 would otherwise be forced to cut over against a fuzzy read boundary

## 5. Target module shape

The canonical query engine for Phase 3 should live in `crates/prism-query`.

The intended shape is:

- `Prism` remains the public facade
- a dedicated coordination-query module family owns evaluation and result shaping
- `Prism` delegates to that engine instead of embedding the reasoning directly in many unrelated
  extension modules

The preferred internal shape is:

- one engine entry module such as `coordination_query_engine.rs` or a dedicated
  `coordination_query_engine/` subtree
- dedicated submodules for:
  - task evaluation
  - plan evaluation
  - evidence and review evaluation
  - queue and portfolio reads
  - coordination-plus-spec join adapters later when those become real

`lib.rs` must remain a facade. Substantive query logic belongs in dedicated modules.

## 6. Required semantic families

Phase 3 must implement the query families required by the contract, at least through the `Prism`
facade:

### 6.1 Object reads

- plan
- task
- artifact
- review

### 6.2 Task evaluation reads

- task status
- task blockers
- task actionability
- task evidence status
- task review scope
- task review targets
- task review status

### 6.3 Plan evaluation reads

- plan summary
- plan actionable tasks
- plan pending reviews
- plan rollup

### 6.4 Queue and portfolio reads

- actionable tasks
- pending reviews
- stale work
- reclaimable work

The exact Rust API names may evolve, but the engine must clearly own these semantic families.

## 7. Freshness and input model

The query engine must evaluate authoritative coordination state while preserving the existing
authority/materialization split.

The Phase 3 implementation should make this layering explicit:

- strong coordination reads evaluate against fresh authority-backed state
- eventual coordination reads may evaluate against previously verified locally available
  materialized authority-backed state
- the query engine owns the semantic interpretation of the result
- the concrete persistent local storage details remain the concern of the materialized-store layer

This phase does not need to redesign the consistency envelope, but every new engine-facing result
shape must preserve enough metadata for callers to distinguish:

- strong vs eventual
- verified current vs verified stale vs unavailable
- authority stamp or equivalent coordination version

## 8. Adoption rule

This phase introduces a hard rule for new code:

- no new product-facing coordination read path may bypass the query engine seam

During migration:

- temporary adapters may exist below the query engine seam
- `Prism` may forward older helper names into the new engine
- surface-specific formatting may remain where it already lives

But:

- MCP handlers
- CLI read commands
- SSR console views
- compact tools
- runtime status and related coordination views

must stop embedding coordination evaluation logic directly whenever the engine can own it.

Any newly discovered coordination-read surface encountered during Phase 3 must either:

- be migrated in this phase
- or be listed explicitly in this spec as deferred, with a reason

## 9. Implementation slices

### Slice 1: Introduce the engine seam

Implement:

- the canonical coordination-query module boundary in `crates/prism-query`
- shared request and result types for task, plan, and queue reads
- `Prism` forwarding methods into the engine

Exit criteria:

- there is one obvious module boundary that owns coordination evaluation
- new coordination-read logic can be added there without touching product surfaces first

### Slice 2: Move task and plan evaluation into the engine

Migrate:

- task status and blocker evaluation wrappers
- actionable task selection
- plan summary and rollup logic
- queue-style reads such as ready/actionable task families

Primary existing sources:

- `crates/prism-query/src/coordination.rs`
- `crates/prism-query/src/plan_insights.rs`
- `crates/prism-query/src/plan_discovery.rs`

Exit criteria:

- core task/plan reasoning is centralized in engine modules rather than spread across ad hoc
  `Prism` extension files

### Slice 3: Add evidence and review families

Implement:

- task evidence status
- review scope and target resolution
- pending review and review-status reads

This slice must follow the artifact/review contract closely rather than inventing one-off
surface-specific projections.

Exit criteria:

- review and evidence posture no longer depends on surface-local interpretation

### Slice 4: Cut over product-facing readers

Migrate product read surfaces to consume the engine-facing API:

- `prism-mcp` compact task brief
- `prism-mcp` host resources
- `prism-mcp` query runtime coordination families
- `prism-mcp` UI read models
- relevant CLI coordination reads, if any remain outside `Prism`

Exit criteria:

- product-facing coordination read logic stops composing raw runtime helpers directly

### Slice 5: Remove obvious duplicated reasoning

Clean up:

- duplicated blocker shaping where the engine already provides a canonical result
- duplicated plan-summary calculations in surface code
- compatibility shims that are no longer necessary after cutover

Exit criteria:

- the repo has one dominant place for coordination semantics

## 10. Validation

Minimum validation for this phase:

- targeted `prism-query` tests covering:
  - blockers
  - task status
  - actionable task selection
  - plan summary / rollup
  - review/evidence status families once added
- direct downstream validation in:
  - `prism-mcp`
  - `prism-cli`
  when public query-facing types or facade methods change
- `git diff --check`

Important regression checks for this phase:

- compact task brief still surfaces the same blocker and guidance posture
- plan list / plan summary views stay consistent across MCP and UI
- peer/runtime-facing reads do not silently degrade freshness labeling

## 11. Completion criteria

Phase 3 is complete only when:

- the coordination query engine exists as a real module seam in code
- task, plan, blocker, evidence, and review reasoning are owned by that seam
- `Prism` acts as a facade over the engine instead of being the only place where coordination
  semantics happen
- MCP, CLI, and UI coordination reads no longer implement their own workflow semantics when the
  engine already covers them
- the Phase 3 spec and roadmap are updated to `completed`

## 12. Implementation checklist

- [ ] Introduce the coordination-query engine module and result families in `prism-query`
- [ ] Route `Prism` task/plan coordination reads through the engine
- [ ] Centralize actionable-task, blocker, and plan-rollup reasoning
- [ ] Add explicit evidence/review query families
- [ ] Cut over MCP/UI/CLI coordination readers to the engine-facing API
- [ ] Remove obvious duplicated coordination-read logic
- [ ] Validate `prism-query`, `prism-mcp`, and `prism-cli`
- [ ] Mark Phase 3 complete in the roadmap
