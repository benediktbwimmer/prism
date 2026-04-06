# PRISM Layered Runtime Split Implementation

Status: proposed execution doc
Audience: PRISM core, query, projections, memory, and MCP maintainers
Scope: make PRISM runtime layers composable and independently shippable without splitting the product surface

---

## 1. Goal

Refactor PRISM so one runtime can compose three explicit layers:

- coordination
- knowledge storage
- cognition

The immediate product goals are:

- a coordination-only runtime can start, mutate, persist, and reload without graph indexing
- knowledge and memory storage can work without cognition
- graph-backed enrichment degrades deliberately when cognition is absent
- the CLI, daemon, and MCP server still ship as one product with explicit runtime modes

This is a runtime boundary rewrite, not a packaging split.

---

## 2. Why This Rewrite Is Necessary

The current runtime is still organized as if graph indexing is the default substrate for almost
everything:

- `crates/prism-core/src/session_bootstrap.rs` always boots through `WorkspaceIndexer`
- `crates/prism-core/src/lib.rs` exposes `WorkspaceSessionOptions`, but those options only toggle
  coordination and hydration details, not runtime layers
- `crates/prism-mcp/src/features.rs` gates tools and query groups, but does not express what the
  underlying runtime can or cannot provide
- `crates/prism-query/src/contracts.rs` and related query code resolve anchors through a live graph
- `crates/prism-projections` mixes durable knowledge packets with graph-backed enrichment
- `crates/prism-memory` stores anchors as durable references, but the rest of the stack often treats
  those anchors as if graph-backed resolution is always available

That coupling creates three practical failures:

1. coordination startup is heavier than necessary because graph hydration is still on the boot path
2. memory and durable knowledge storage inherit graph assumptions they do not require
3. MCP and query surfaces degrade accidentally instead of through typed mode-aware behavior

---

## 3. Layer Model

PRISM should adopt three explicit runtime layers.

### 3.1 Coordination layer

Owns shared workflow state and its local runtime support:

- plans
- tasks
- claims
- artifacts and reviews
- shared coordination refs
- coordination checkpoints and runtime synchronization

This layer must be able to run without cognition.

### 3.2 Knowledge storage layer

Owns durable storage of repo and runtime knowledge:

- memory entries and memory events
- outcome events and replay state
- curated concept and contract packets as stored artifacts
- repo-published knowledge and durable query-supporting packets

This layer must be able to persist and reload without requiring live graph resolution.

### 3.3 Cognition layer

Owns graph-backed understanding and enrichment:

- workspace graph indexing
- lineage assignment and rebinding
- projections derived from graph and history
- concept and contract target resolution against live graph state
- impact, validation, and discovery flows that require graph semantics

This layer depends on the graph/indexing pipeline and is optional in the target runtime model.

---

## 4. Supported Runtime Modes

We should support a small explicit set of modes first instead of claiming arbitrary combinations.

### 4.1 Full mode

Enabled layers:

- coordination
- knowledge storage
- cognition

This is the current product-equivalent mode and remains the default.

### 4.2 Coordination-only mode

Enabled layers:

- coordination

Disabled layers:

- knowledge storage
- cognition

This mode is for lightweight shared planning and workflow operation without graph indexing.

### 4.3 Knowledge-storage mode

Enabled layers:

- coordination
- knowledge storage

Disabled layers:

- cognition

This mode keeps durable memory and knowledge flows alive while returning degraded responses for
graph-backed enrichment.

### 4.4 Unsupported combinations

The first implementation should not claim support for:

- cognition without knowledge storage
- cognition without coordination
- knowledge storage without coordination

Those combinations can be revisited later, but they would complicate the first split without
helping the immediate shipping goal.

---

## 5. Target Runtime Contract

We need one explicit runtime contract shared by `prism-core` and `prism-mcp`.

### 5.1 Core type shape

Introduce a small layer/capability model in core:

- `PrismRuntimeLayer`
- `PrismLayerSet`
- `PrismRuntimeMode`
- `PrismRuntimeCapabilities`

The important distinction is:

- layer set: what subsystems are enabled
- capabilities: what the runtime can actually answer at the surface

Capabilities should be derived from the enabled layers, not toggled independently all over the
codebase.

### 5.2 Session options

`WorkspaceSessionOptions` should stop being a grab bag of partial booleans. It should express:

- selected runtime mode or layer set
- shared runtime backend selection
- optional hydration policy details that are subordinate to the mode

The session/bootstrap layer should decide from that contract whether it needs:

- graph hydration
- projection hydration
- memory hydration
- coordination hydration

### 5.3 MCP surface contract

`PrismMcpFeatures` should remain a surface-shaping layer, but it must consume runtime capabilities
instead of acting as a proxy for runtime architecture.

The rule should be:

- mode decides what the runtime can do
- MCP features decide what parts of that capability are exposed

If a capability is absent, MCP must return one of:

- a deliberate surface reduction
- a typed degraded response

It must not fall through to accidental lookup failures.

---

## 6. Current Coupling Inventory

This is the implementation inventory we should work from.

### 6.1 Bootstrap is graph-first

Relevant files:

- `crates/prism-core/src/lib.rs`
- `crates/prism-core/src/session_bootstrap.rs`
- `crates/prism-core/src/indexer.rs`
- `crates/prism-core/src/session.rs`

Current problem:

- bootstrapping always constructs `WorkspaceIndexer`
- prior snapshot hydration still flows through indexer-centric state
- there is no non-graph coordination bootstrap path

Required change:

- split session bootstrap into layer-aware paths
- make indexer initialization conditional on cognition

### 6.2 Persisted anchors still leak graph assumptions

Relevant files:

- `crates/prism-ir/src/anchor.rs`
- `crates/prism-ir/src/plans.rs`
- `crates/prism-query/src/plan_bindings.rs`
- `crates/prism-query/src/contracts.rs`

Current problem:

- durable anchors are persisted independently already, but query and binding logic often assumes
  they can be resolved against a live graph

Required change:

- distinguish persisted anchor semantics from graph resolution semantics
- make graph-backed expansion optional

### 6.3 Knowledge packets are not cleanly split from cognition

Relevant files:

- `crates/prism-memory/src/types.rs`
- `crates/prism-projections/src/lib.rs`
- `crates/prism-projections/src/concepts.rs`
- `crates/prism-query/src/contracts.rs`

Current problem:

- durable knowledge packets and graph-enriched resolution paths are mixed closely enough that
  absence of cognition is still a correctness hazard

Required change:

- separate stored packet availability from graph-enriched resolution
- define degraded behavior when packet enrichment cannot run

### 6.4 MCP gating is feature-centric, not layer-centric

Relevant files:

- `crates/prism-mcp/src/features.rs`
- `crates/prism-mcp/src/server_surface.rs`
- `crates/prism-mcp/src/query_views.rs`

Current problem:

- tooling flags like `workflow`, `claims`, and `artifacts` do not answer whether the runtime has
  graph/indexing support, durable knowledge support, or only coordination state

Required change:

- thread runtime capabilities into surface registration and query dispatch
- gate by layer-backed capabilities, not just user-facing feature flags

### 6.5 Persistence and startup are not mode-aware

Relevant files:

- `crates/prism-core/src/coordination_startup_checkpoint.rs`
- `crates/prism-core/src/protected_state/runtime_sync.rs`
- `crates/prism-core/src/workspace_runtime_state.rs`

Current problem:

- persisted state and startup hydration still assume a largely uniform runtime

Required change:

- tag persisted state with layer-awareness where needed
- choose hydration and checkpoint paths based on the active mode

---

## 7. Implementation Phases

The work should land in this order.

### Priority target: make coordination-only mode usable first

Before broad mode coverage, the rewrite should optimize for one immediate outcome:

- coordination-only mode must become the simplest viable PRISM configuration
- every mandatory dependency on cognition must be removed from that mode
- MCP and `prism://instructions` must strip down aggressively so only coordination-relevant public
  APIs remain
- coordination-only should fail closed for cognition-backed behavior instead of exposing noisy or
  misleading partial surfaces

Execution order for that target:

1. Finish non-cognition runtime paths in `prism-core`.
   Remove mandatory graph/indexer work from startup, refresh, fs watch, protected-state reload,
   and fallback recovery when `runtime_mode = coordination_only`.
   Estimated effort: 1.5-2.5 days

2. Lock down the coordination-only runtime contract in `prism-core`.
   Make the mode authoritative so coordination-only sessions build only the coordination-owned
   runtime state and minimal persistence/checkpoint flows.
   Estimated effort: 0.5 day after step 1

3. Strip the MCP surface hard for coordination-only mode.
   Tool registration, resources, query views, and UI read models should expose only
   coordination-relevant capabilities in this mode.
   Estimated effort: 1.5-2 days

4. Rewrite `prism://instructions` for coordination-only mode.
   This surface should become a minimal operator guide for plans, tasks, claims, artifacts,
   reviews, and coordination persistence only.
   Estimated effort: 0.5 day

5. Remove residual query-layer cognition dependencies from coordination-only public APIs.
   Any coordination-facing reads that still depend on graph-backed helpers should be replaced or
   removed from the public surface.
   Estimated effort: 1-1.5 days

6. Make persistence and checkpoints explicitly coordination-only safe.
   Persist and reload only what this mode owns, and avoid loading knowledge/cognition artifacts by
   default.
   Estimated effort: 1 day

7. Add the minimum validation matrix for coordination-only mode.
   Cover startup, mutate, reload, watcher refresh, MCP surface gating, and
   `prism://instructions`.
   Estimated effort: 1 day

### Phase 0: Freeze the layer contract

Deliverable:

- a written coupling inventory and target runtime contract

Files:

- this document
- follow-up notes in `docs/` only if the contract changes materially

Exit criteria:

- maintainers agree on the three-layer model
- the initial supported modes are fixed
- no implementation starts from contradictory assumptions

### Phase 1: Introduce the layer set and capability model

Files:

- `crates/prism-core/src/lib.rs`
- `crates/prism-core/src/session_bootstrap.rs`
- `crates/prism-mcp/src/features.rs`

Changes:

- add the canonical layer set and runtime capability types
- move `WorkspaceSessionOptions` to mode-driven semantics
- make MCP derive runtime capabilities from the selected mode

Exit criteria:

- runtime configuration can express full, coordination-only, and knowledge-storage modes
- parsing and defaults are stable

### Phase 2: Decouple durable anchors from graph-required resolution

Files:

- `crates/prism-ir/src/anchor.rs`
- `crates/prism-ir/src/plans.rs`
- `crates/prism-query/src/plan_bindings.rs`
- `crates/prism-query/src/contracts.rs`

Changes:

- define which anchor operations require only stored durable state
- define which anchor operations require cognition
- add typed “unresolved because cognition is disabled” behavior where needed

Exit criteria:

- coordination and memory anchors persist and reload without a graph
- query code no longer conflates “anchor exists” with “anchor resolves to live nodes”

### Phase 3: Add coordination-only bootstrap and session paths

Files:

- `crates/prism-core/src/session_bootstrap.rs`
- `crates/prism-core/src/indexer.rs`
- `crates/prism-core/src/session.rs`

Changes:

- split bootstrap into graph and non-graph paths
- let coordination-only sessions hydrate shared coordination and local runtime state without
  building the graph
- ensure mutation and persistence flows still work in that mode

Exit criteria:

- a coordination-only workspace can start, mutate, persist, and reload without graph indexing

### Phase 4: Split knowledge storage from graph-aware knowledge resolution

Files:

- `crates/prism-memory/src/types.rs`
- `crates/prism-projections/src/lib.rs`
- `crates/prism-projections/src/concepts.rs`
- `crates/prism-query/src/contracts.rs`

Changes:

- define storage-only availability for memory and durable knowledge packets
- make graph-backed enrichment and target expansion conditional on cognition
- preserve useful packet reads even when target-node expansion is unavailable

Exit criteria:

- memory and durable knowledge storage work without cognition
- graph-backed enrichments return predictable degraded results

### Phase 5: Gate MCP and query surfaces by active layers

Files:

- `crates/prism-mcp/src/features.rs`
- `crates/prism-mcp/src/server_surface.rs`
- `crates/prism-mcp/src/query_views.rs`

Changes:

- wire runtime capabilities into tool registration, resource exposure, and query views
- define degraded result contracts for cognition-backed operations
- remove accidental runtime panics or implicit “not found” behavior caused by missing layers

Exit criteria:

- every disabled layer yields either deliberate surface reduction or typed degraded responses

### Phase 6: Make persistence and startup checkpoints layer-aware

Files:

- `crates/prism-core/src/coordination_startup_checkpoint.rs`
- `crates/prism-core/src/protected_state/runtime_sync.rs`
- `crates/prism-core/src/workspace_runtime_state.rs`

Changes:

- encode enough mode metadata to choose safe hydration paths
- prevent mode changes from corrupting or over-hydrating persisted state
- make startup choose the right checkpoint restore path for each supported mode

Exit criteria:

- startup and persistence remain compatible across supported mode transitions

### Phase 7: Add a validation matrix for supported combinations

Files:

- `crates/prism-core/src/tests.rs`
- `crates/prism-mcp/src/tests_support.rs`
- targeted tests in touched crates

Changes:

- add fixtures and helpers for each supported runtime mode
- make integration coverage explicit for coordination-only, knowledge-storage, and full modes

Exit criteria:

- supported combinations have dedicated regression coverage

### Phase 8: Document packaging and rollout boundaries

Files:

- `docs/PACKAGING_AND_DISTRIBUTION_PLAN.md`
- this document
- any release playbook updates required by the final design

Changes:

- document which layers are bundled together in the shipped product
- define how degraded modes are surfaced to operators and users
- document rollout order and compatibility constraints

Exit criteria:

- release policy is explicit and matches the actual runtime model

---

## 8. Behavior Contracts For Degraded Modes

The split will fail if we leave degraded behavior implicit. These rules should hold everywhere.

### 8.1 Coordination-only mode

Allowed:

- plan and task queries
- claims and artifact coordination
- shared ref persistence
- coordination checkpointing

Not allowed:

- graph lookup
- concept enrichment from graph members
- impact and validation views that require graph traversal
- node-target contract expansion

Surface rule:

- hide tools that are meaningless without cognition
- for mixed surfaces, return typed degraded results that state cognition is disabled

### 8.2 Knowledge-storage mode

Allowed:

- all coordination-only operations
- memory store and recall
- outcome storage and replay
- reads of stored concept and contract packets

Degraded:

- packet target resolution against live graph nodes
- lineage-aware impact and validation expansion
- discovery flows that require graph ranking

Surface rule:

- return stored packets when they exist
- mark graph-enriched fields unavailable instead of fabricating empty results

### 8.3 Full mode

Allowed:

- existing product behavior

Rule:

- full mode must remain the regression baseline throughout the split

---

## 9. Validation Matrix

The minimum validation matrix for this rewrite is:

- `test:layer-config-parsing`
- `test:coordination-anchor-without-graph`
- `test:coordination-only-session-startup`
- `test:knowledge-storage-without-cognition`
- `test:mcp-surface-layer-gating`
- `test:layered-startup-checkpoint-compat`
- `test:coordination-only-integration`
- `test:knowledge-storage-mode`
- `test:full-mode-regression`

### 9.1 Coordination-only matrix currently covered on this branch

Core runtime coverage:

- startup and reload without graph indexing:
  `hydrate_coordination_only_session_skips_graph_indexing_and_reloads_coordination_state`
- indexed session bootstrap still skips cognition work:
  `indexed_coordination_only_session_still_skips_graph_indexing`
- refresh updates the workspace snapshot without graph indexing:
  `coordination_only_refresh_fs_updates_snapshot_without_graph_indexing`
- refresh strips live projection knowledge:
  `coordination_only_refresh_fs_drops_live_projection_knowledge`
- refresh strips live outcomes when knowledge storage is disabled:
  `coordination_only_refresh_fs_drops_live_outcomes_without_knowledge_storage`
- protected-state reload ignores knowledge streams:
  `coordination_only_protected_state_watch_ignores_knowledge_streams`
- reopen from full mode hides knowledge layers:
  `coordination_only_mode_reopens_full_workspace_without_exposing_knowledge_layers`
- reopen from coordination-only back to full rehydrates graph state:
  `full_mode_reopens_coordination_only_workspace_and_rehydrates_graph_state`
- shared-runtime SQLite reload preserves coordination-only behavior:
  `coordination_only_shared_runtime_sqlite_reloads_coordination_without_graph_state`
  `coordination_only_shared_runtime_sqlite_hides_knowledge_when_reopened_from_full_mode`
  `full_mode_shared_runtime_sqlite_reopens_coordination_only_workspace_and_rehydrates_graph_state`

Coordination/query contract coverage:

- durable plan bindings survive without knowledge storage:
  `coordination_only_reload_preserves_durable_plan_bindings_without_knowledge_storage`
  `coordination_only_reopen_from_full_preserves_native_plan_bindings`
  `coordination_only_shared_runtime_sqlite_preserves_native_plan_bindings_from_full_mode`

MCP surface coverage:

- coordination-only query surface keeps coordination reads and rejects cognition reads:
  `coordination_only_prism_query_keeps_coordination_reads_and_rejects_cognition_reads`
- `prism_mutate` advertises only coordination-safe actions:
  `coordination_only_prism_query_filters_mutate_schema_and_validation`
- reduced plan and artifact reads degrade deliberately:
  `coordination_only_prism_query_plan_graph_keeps_durable_bindings`
  `coordination_only_prism_query_artifact_risk_uses_degraded_artifact_view`
- daemon and bridge surfaces stay reduced:
  `coordination_only_server_strips_cognition_tools_and_resources`
  `coordination_only_bootstrap_tool_cache_matches_reduced_surface`
- instructions stay mode-correct:
  `coordination_only_instructions_strip_cognition_guidance`
  `coordination_only_instruction_render_reflects_feature_toggles`
  `coordination_only_task_brief_avoids_stripped_tool_guidance`

Validation tiers:

- always run targeted tests for every touched crate
- run direct downstream dependents when public runtime types change
- run the full workspace suite before merging because this work changes startup, runtime behavior,
  and persistence boundaries

---

## 10. Concrete First Edits

The first implementation PR should stay narrow and only establish the contract.

Recommended first patch:

1. add runtime layer and capability types in `prism-core`
2. reshape `WorkspaceSessionOptions` around the new mode contract
3. adapt `prism-mcp` feature derivation to consume runtime capabilities
4. add parsing and unit tests for the supported modes
5. do not add the coordination-only bootstrap path yet unless the contract work is already clean

This keeps the first step reviewable and prevents the bootstrap rewrite from happening before the
mode contract is stable.

---

## 11. Non-Goals

This implementation plan does not require:

- splitting PRISM into separate products
- supporting every theoretical layer combination immediately
- removing graph-backed cognition from the default runtime
- redesigning the packaging front door away from `prism`
- rewriting unrelated projection systems during the initial split

---

## 12. Decision Record

The working decisions in this document are:

- PRISM keeps one product surface
- runtime composition is expressed through explicit modes, not hidden booleans
- coordination must not require cognition
- knowledge storage must not require cognition
- graph-backed enrichment is optional capability, not a baseline assumption
- degraded behavior must be typed and deliberate

Any implementation that violates one of those decisions should update this document first instead
of drifting in code.
