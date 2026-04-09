# Coordination-Only Lightweight Runtime Cutover

Status: implemented
Audience: coordination, runtime, MCP, CLI, service, and workflow maintainers
Scope: turn `coordination_only` into a truly lightweight coordination runtime with no FS indexing,
graph hydration, knowledge storage, cognition-era anchor resolution, or adjacent runtime baggage

---

## 1. Summary

`coordination_only` currently behaves more like "full runtime with cognition tools hidden" than a
truly lightweight coordination mode.

The implementation already disables graph hydration and knowledge hydration at bootstrap, but it
still constructs the normal `WorkspaceSession`, starts watch and service-shell infrastructure, uses
graph-backed file-anchor identity, routes coordination mutations through the shared workspace
refresh lock, and keeps session and mutation behavior coupled to outcome- and memory-era runtime
state.

As of this revision, coordination-only now has a dedicated no-indexer bootstrap branch, the MCP
session layer no longer allocates notes or inferred-edge stores in this mode, and the shared
session refresh / observed-change APIs are inert in coordination-only. Active work context now
stays in MCP session state instead of being mirrored into workspace-side observed-change tracking.
Coordination-only startup also no longer eagerly hydrates principal registry state or attaches
cold history / outcome backends to the published `Prism` handle. The shared `WorkspaceSession`
API still remains, but the coordination-only path no longer allocates the full refresh, watch,
curator, checkpoint-materialization, or observed-change backing state behind it. Coordination-only
shutdown also no longer persists workspace startup checkpoints, observed-change checkpoints, or
cached principal-registry state. MCP coordination-only sessions also no longer persist or restore
session-seed state from disk, and bootstrap now reuses the primary workspace store instead of
reopening a redundant cold runtime reader for this mode.

This spec defines the cutover to a stricter model:

- `coordination_only` is coordination-only
- no FS indexing is required
- file anchors are durable workspace-relative paths, not indexed file ids
- coordination reads and writes do not depend on graph refresh or workspace refresh admission
- non-coordination runtime subsystems are removed from this mode rather than preserved for
  compatibility

The bias for this work is deliberate:

- prefer breaking cognition-era and knowledge-era assumptions in `coordination_only`
- leave targeted TODO markers where a later full-mode reconciliation is needed
- do not preserve old runtime layering just because it already exists

## 2. Problem Statement

The current codebase still drags substantial non-coordination machinery into
`PrismRuntimeMode::CoordinationOnly`.

### 2.1 Bootstrap is still workspace-runtime shaped

Even when knowledge storage is disabled, the runtime still builds the full workspace session path:

- `crates/prism-core/src/session_bootstrap.rs`
- `crates/prism-core/src/indexer.rs`
- `crates/prism-core/src/indexer_support.rs`
- `crates/prism-core/src/session.rs`

Current behavior:

- constructs `WorkspaceIndexer` and `WorkspaceSession`
- discovers workspace layout
- opens runtime readers
- loads principal registry state
- allocates `WorkspaceRuntimeState`
- allocates `WorkspaceRefreshState`, `fs_snapshot`, and `refresh_lock`
- constructs checkpoint materialization infrastructure
- constructs curator infrastructure
- starts FS watch, protected-state watch, and coordination-authority watch

This is too heavy for a mode that should only read and mutate coordination state.

### 2.2 The runtime session still carries non-coordination storage and controls

`WorkspaceRuntimeState` and `WorkspaceSession` still expose full-runtime fields even when
knowledge storage is disabled:

- `crates/prism-core/src/workspace_runtime_state.rs`
- `crates/prism-core/src/session.rs`

Current baggage includes:

- `Graph`
- `HistoryStore`
- `OutcomeMemory`
- `ProjectionIndex`
- workspace refresh state
- FS snapshots
- watch handles
- curator handle
- checkpoint materializer
- observed-change tracking and repo-patch provenance scheduling

Most of those are not coordination semantics.

### 2.3 Coordination mutation admission is still coupled to workspace refresh

`mutateCoordination` still waits on the shared workspace `refresh_lock`:

- `crates/prism-mcp/src/host_mutations.rs`
- `crates/prism-core/src/session.rs`
- `crates/prism-core/src/admission.rs`

That lock is also used by:

- FS refresh
- protected-state watch updates
- coordination-authority watch application
- curator updates

This is the direct cause of the observed
`request admission busy for mutateCoordination: refresh_lock is currently held`
failure in `coordination_only`.

That coupling is wrong for this mode.

### 2.4 File anchors are still graph-backed identities

The current durable file-anchor representation is `AnchorRef::File(FileId)`:

- `crates/prism-ir/src/anchor.rs`

MCP file-anchor input still resolves paths into indexed file ids:

- `crates/prism-mcp/src/query_types.rs`

That resolution can trigger:

- `mark_fs_dirty_paths`
- `refresh_fs_with_paths`
- graph file-id lookup

This means a "file anchor" in `coordination_only` is still secretly an indexed graph anchor.

That is the wrong identity model for this mode.

### 2.5 Coordination anchor expansion still assumes graph, lineage, and co-change state

Coordination read and claim surfaces still expand anchors through graph and history-era data:

- `crates/prism-query/src/coordination.rs`
- `crates/prism-query/src/lib.rs`

Current behavior includes:

- expanding node anchors
- resolving lineages
- adding neighboring graph nodes
- deriving file anchors from graph nodes
- following co-change neighbors

That is cognition- and knowledge-era enrichment, not coordination-only behavior.

### 2.6 MCP host and session state still allocate non-coordination services

`QueryHost` and the MCP session layer still construct:

- `SessionMemory`
- `InferenceStore`
- JS worker pool
- workspace service shell
- runtime gateway
- event engine
- diagnostics refresh infrastructure

Relevant files:

- `crates/prism-mcp/src/lib.rs`
- `crates/prism-mcp/src/service_shell.rs`
- `crates/prism-mcp/src/session_state.rs`

This is not justified for a coordination-only runtime.

### 2.7 Coordination-only still exposes non-coordination resources

The reduced resource surface still includes `prism://protected-state`:

- `crates/prism-mcp/src/features.rs`
- `crates/prism-mcp/src/instructions.rs`
- `crates/prism-mcp/src/host_resources.rs`

Protected-state diagnostics are not coordination semantics and should not be part of the public
coordination-only surface.

### 2.8 `declare_work` still writes outcome-era data

`prism_mutate` action `declare_work` remains visible in coordination-only, but the implementation
still appends `OutcomeEvent` records with `OutcomeKind::PlanCreated`:

- `crates/prism-mcp/src/host_mutations.rs`

That is session/task-journal behavior built on outcome memory, not coordination state.

If `declare_work` remains available in `coordination_only`, it must become a lightweight session
context operation rather than an outcome mutation.

## 3. Scope

This spec includes:

- defining a true lightweight runtime model for `coordination_only`
- removing graph-, history-, outcome-, projection-, watch-, curator-, and refresh-based
  dependencies from coordination-only reads and writes
- redefining file anchors in coordination-only as durable relative paths
- narrowing MCP coordination-only resources, query helpers, and session machinery to the minimum
  needed coordination surface
- accepting short-term breakage in full-mode compatibility paths when that is the cheapest route
  to a clean coordination-only implementation

This spec does not include:

- a new full-mode architecture
- backfilling cognition-mode compatibility during the same slice
- redesigning the coordination domain model itself
- changing coordination authority semantics

## 4. Related Roadmap And Contracts

This spec depends on:

- `docs/contracts/coordination-mutation-protocol.md`
- `docs/contracts/coordination-query-engine.md`
- `docs/contracts/coordination-artifact-review-model.md`
- `docs/contracts/anchor-resolution.md`
- `docs/contracts/runtime-identity-and-descriptors.md`
- `docs/contracts/service-runtime-gateway.md`
- `docs/roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md`

This spec follows the platform-freeze work in:

- `docs/specs/2026-04-09-coordination-platform-freeze-phase-7.md`

## 5. Target Model

### 5.1 Mode definition

`coordination_only` means:

- coordination authority reads
- coordination mutation protocol writes
- coordination read-model derivation
- runtime descriptor participation when needed for coordination semantics
- lightweight session work context for MCP interaction

It does not mean:

- graph indexing without cognition tools
- FS watch without search tools
- knowledge storage without concept tools
- outcome or memory systems hidden behind coordination entrypoints

### 5.2 No-FS-indexing rule

`coordination_only` must not require:

- parsing workspace source files
- hydrating a graph
- hydrating history
- hydrating outcome memory
- hydrating projections
- maintaining a workspace tree snapshot
- maintaining file materialization coverage
- performing scoped FS refresh for coordination requests

If a coordination-only request needs a file anchor, it should use the path directly.

### 5.3 Coordination-native file-anchor rule

In `coordination_only`, file anchors must be durable workspace-relative paths.

Required behavior:

- accept relative `path`
- reject absolute `path`
- reject path traversal outside the workspace root
- normalize path separators and `.` segments before persistence
- compare and dedupe file anchors by normalized relative path

Forbidden behavior in this mode:

- converting file anchors into `FileId`
- refreshing the workspace to discover a file id
- requiring the file to be indexed before the anchor can exist
- expanding file anchors into nodes, lineages, or co-change neighborhoods

The durable coordination representation must therefore stop depending on `AnchorRef::File(FileId)`
for coordination-only behavior.

### 5.4 Coordination-only anchor semantics

Coordination-only anchor semantics must be deliberately smaller than full-mode semantics.

Allowed:

- direct path anchors
- direct coordination object ids
- explicit task, plan, artifact, and review references
- optional explicit node or lineage anchors only if they are already durable coordination facts

Disallowed:

- graph-neighbor expansion
- lineage discovery from local code state
- co-change expansion
- file-id resolution
- path-to-symbol inference

If callers want cognition-style anchor enrichment, they must use a cognition-enabled runtime mode.

### 5.5 Mutation admission rule

Coordination mutations in `coordination_only` must not contend on the workspace refresh mutex.

Required behavior:

- coordination mutations use coordination-specific admission only
- workspace refresh admission and coordination mutation admission are separated
- file-anchor validation for coordination-only is lexical and path-based, not refresh-based

The only acceptable coordination-only write contention is contention on coordination authority or
coordination materialization boundaries themselves.

### 5.6 Read-path rule

Coordination-only reads should come from one of:

- authority-backed current state
- coordination startup checkpoint
- coordination materialized store
- deterministic read-model derivation from the coordination snapshot

They must not depend on:

- live graph state
- hot/cold history merges
- outcome memory
- projection knowledge
- protected-state diagnostics

### 5.7 Lightweight MCP session rule

The coordination-only MCP session layer should keep only:

- current work
- current task
- current agent
- query limits
- lightweight event-id sequencing if coordination mutation provenance still needs it

It should not require:

- `SessionMemory`
- `InferenceStore`
- task-journal replay from outcomes
- memory publication
- inferred-edge storage

If `declare_work` remains in this mode, it must:

- update session seed only
- avoid writing `OutcomeEvent`
- avoid writing `MemoryEvent`

### 5.8 Service-shell rule

The coordination-only host should not bind heavy service-shell subsystems unless they are
coordination-critical.

Default removals for this mode:

- runtime gateway
- workspace event engine
- protected-state resource diagnostics
- curator plumbing

Runtime descriptor publication may remain if it is still the chosen coordination discovery
mechanism.

### 5.9 Ruthless deletion rule

For this cutover, preserving a shared abstraction with full mode is not a goal by itself.

When a shared component makes coordination-only heavier than necessary:

- branch the implementation for `coordination_only`, or
- delete the shared dependency from the coordination-only path, or
- leave a TODO and knowingly narrow behavior in full mode until it is rebuilt cleanly

Do not keep heavyweight dependencies alive in coordination-only just to avoid touching
cognition-era code.

## 6. Design

### 6.1 Introduce a coordination-native durable path anchor

Add a durable anchor representation that can carry normalized workspace-relative paths without
graph identity.

Preferred direction:

- introduce a new path-based anchor variant in `prism-ir`
- migrate coordination comparison, dedupe, conflict, and view code to understand it directly

The implementation may leave `File(FileId)` in place temporarily for full mode, but coordination
surfaces must stop depending on it.

### 6.2 Add a lightweight coordination-only workspace runtime

Introduce a dedicated bootstrap/session path for `PrismRuntimeMode::CoordinationOnly`.

That path should avoid constructing:

- `WorkspaceIndexer` state that exists only for FS indexing
- watch handles
- refresh state and FS snapshots
- curator
- checkpoint materializer responsibilities outside coordination state
- observed-change tracking
- repo-patch provenance scheduling

The resulting runtime object may still expose a `WorkspaceSession`-like API, but it should be
implemented by a smaller backing structure instead of the full workspace runtime.

### 6.3 Split coordination mutation admission from refresh admission

Replace the current `refresh_lock` coupling for coordination writes with:

- a coordination write admission path
- authority-store or materialized-store concurrency where needed

This split is required even if the rest of the runtime remains shared for one intermediate slice.

### 6.4 Route coordination-only reads through coordination-native brokers

The reduced runtime should read coordination state through:

- authority store provider
- coordination materialized store
- deterministic read-model derivation

It should not build or publish a `Prism` instance whose main value is graph, history, or outcome
state.

If retaining `Prism` temporarily is cheaper for intermediate slices, the coordination-only branch
must populate only coordination-native state and must not revive graph-era loading to satisfy it.

### 6.5 Reduce the public coordination-only resource surface

The default resource set for coordination-only should be limited to:

- `prism://capabilities`
- `prism://session`
- `prism://vocab`
- `prism://plans`
- `prism://tool-schemas`
- instruction resources

Remove from coordination-only:

- `prism://protected-state`
- cognition-oriented example and shape surfaces
- any resource that exists only to explain graph, search, concept, or memory state

### 6.6 Demote non-coordination mutate behavior

In coordination-only:

- keep `coordination`, `claim`, `artifact`, and `heartbeat_lease` only when enabled by the feature
  set
- keep `declare_work` only if it becomes session-only and coordination-neutral
- do not route `declare_work` through outcome memory
- keep curator, memory, outcome, and inferred-edge actions unavailable

## 7. Implementation Slices

### Slice 1: Coordination-native path anchors

- add a durable relative-path anchor representation
- update MCP anchor conversion for coordination-only to produce path anchors only
- update coordination sorting, dedupe, overlap, and view rendering to use normalized relative paths
- remove path-to-file-id resolution from coordination-only

Exit criteria:

- coordination-only file anchors never trigger FS refresh or file-id lookup

### Slice 2: Lightweight coordination-only runtime bootstrap

- add a dedicated bootstrap/session path for `PrismRuntimeMode::CoordinationOnly`
- skip layout/index/bootstrap work that exists only for graph or knowledge runtime state
- do not allocate watch state, FS snapshot state, or refresh admission state for this mode
- do not construct curator or observed-change tracking in this mode

Exit criteria:

- starting a coordination-only runtime does not build or own FS indexing machinery

### Slice 3: Coordination-only mutation and read-path decoupling

- split coordination mutation admission from workspace refresh admission
- route coordination reads directly through authority/materialized-store brokers
- remove graph/history/outcome/projection dependencies from coordination-only read paths

Exit criteria:

- coordination-only coordination mutations cannot fail because a workspace refresh mutex is held

### Slice 4: Lightweight MCP host and session state

- add a lightweight session-state variant or stripped coordination-only session state
- remove `SessionMemory` and `InferenceStore` requirements from coordination-only
- make `declare_work` session-seed-only or remove it from this mode
- stop binding non-essential service-shell subsystems in coordination-only

Exit criteria:

- MCP coordination-only startup no longer allocates memory/inference/task-journal machinery just to
  serve coordination reads and writes

### Slice 5: Surface cleanup and compatibility debt marking

- remove `prism://protected-state` from coordination-only resources
- tighten instructions and capability docs to match the new reality
- add TODO markers where full-mode or shared abstractions need a later clean rebuild

Exit criteria:

- the coordination-only product surface no longer advertises non-coordination runtime concepts

## 8. Validation

Minimum validation for this work:

- targeted `prism-core` tests for coordination-only bootstrap and mutation admission behavior
- targeted `prism-mcp` tests for coordination-only tool/resource visibility and file-anchor behavior
- targeted `prism-query` or `prism-coordination` tests when anchor comparison semantics change
- `git diff --check`

Required regression coverage:

- coordination-only bootstrap does not hydrate graph, history, outcomes, or projections
- coordination-only bootstrap does not start FS/protected-state watch loops or curator plumbing
- coordination-only file anchors accept normalized relative paths and reject absolute paths
- coordination-only file-anchor mutations do not call workspace refresh
- coordination-only coordination mutations do not fail on `refresh_lock`
- coordination-only resources exclude `prism://protected-state`
- `declare_work`, if retained, does not append `OutcomeEvent` or `MemoryEvent`

## 9. Rollout And Migration

This cutover is intentionally allowed to break shared full-mode assumptions.

Migration rules:

1. Make coordination-only behavior correct first.
2. Add targeted TODO markers where full-mode reconciliation is needed later.
3. Do not preserve heavyweight shared runtime objects just to keep temporary compatibility.

If an intermediate slice leaves dead or unreachable cognition-era code behind, that is acceptable
until the follow-up cleanup lands.

## 10. Open Questions

1. Should `AnchorRef::File(FileId)` remain as a full-mode-only concept, or should all durable file
   anchors migrate to path identity?
2. Should `declare_work` survive in coordination-only as a session-only operation, or should
   coordination-only require explicit plan/task references instead?
3. Is runtime descriptor publication still required at coordination-only startup, or can it move to
   an explicit coordination-service responsibility?

## 11. Implementation Checklist

- [x] Introduce coordination-native durable path anchors
- [x] Remove file-id resolution from coordination-only anchor handling
- [x] Add a lightweight coordination-only bootstrap/session path
- [x] Remove FS refresh admission from coordination-only coordination mutations
- [x] Disable live watchers, curator, and checkpoint materialization in coordination-only
- [x] Stop building the workspace service shell in coordination-only
- [x] Slim MCP session state for coordination-only
- [x] Remove protected-state resource exposure from coordination-only
- [x] Keep `declare_work` session-local in coordination-only
- [x] Update docs and tests to match the landed slice
