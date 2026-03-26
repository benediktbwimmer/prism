# PRISM Modularization Plan

## Goal

Break up the current large single-file crates before they become the default place to add unrelated behavior.

This plan is intentionally about internal structure, not feature expansion. Public APIs should stay stable while the implementation moves behind clearer module boundaries.

## Current Pressure Points

The initial trigger for this plan was crate-root growth large enough to slow review and increase change coupling:

* `crates/prism-mcp/src/lib.rs`: about 6.4k LOC
* `crates/prism-store/src/lib.rs`: about 2.6k LOC
* `crates/prism-core/src/lib.rs`: about 2.4k LOC

The main issue is not only line count. Each file currently mixes state, IO, serialization, orchestration, helper logic, and tests in one place.

Current status as of 2026-03-26:

* `crates/prism-store/src/lib.rs`: 12 LOC
* `crates/prism-core/src/lib.rs`: 47 LOC
* `crates/prism-mcp/src/lib.rs`: 263 LOC

The crate-root monolith problem is now largely addressed. The remaining work is second-pass cleanup: tighten ownership around still-large extracted modules and keep future changes out of crate roots.

## Sequencing

Recommended order of attack:

1. `prism-store` - completed
2. `prism-core` - first pass completed, second pass partially completed
3. `prism-mcp` - completed

Why this order:

* `prism-store` has the cleanest seams and the lowest behavioral risk, so it is the best place to establish the modularization pattern
* `prism-core` is the next orchestration bottleneck and will benefit from having store boundaries clearer first
* `prism-mcp` is the largest file, but it also has the widest surface area; it is safer to split once the lower layers are already easier to navigate

If multiple people work in parallel, the safe ownership split is:

* one stream owns `prism-store`
* one stream owns `prism-core`
* `prism-mcp` starts after the shared naming and module conventions are settled

That sequencing held up well in practice. `prism-store` established the pattern, `prism-core` followed with low-risk helper and support splits, and `prism-mcp` was then decomposed without destabilizing the lower layers.

## Global Rules

Every decomposition step should follow the same constraints:

* preserve crate public APIs unless a follow-up cleanup is explicitly planned
* move code first, then rename or redesign in a separate pass
* keep tests passing after each extraction step
* avoid mixing modularization with semantic behavior changes
* prefer extracting pure helpers before extracting stateful orchestration
* add `mod` boundaries that reflect ownership, not arbitrary file size cuts
* keep module names domain-shaped and stable so future contributors know where to add code

## Target Layouts

### `prism-store`

Current clusters:

* graph model and `Graph` methods
* `Store` trait plus `MemoryStore`
* SQLite schema setup and migrations
* graph load and save logic
* snapshot persistence
* projection snapshot and delta persistence
* SQL encoding and decoding helpers

Target structure:

```text
crates/prism-store/src/
  lib.rs
  graph.rs
  store.rs
  memory_store.rs
  tests.rs
  sqlite/
    mod.rs
    schema.rs
    graph_io.rs
    snapshots.rs
    projections.rs
    codecs.rs
```

Module ownership:

* `graph.rs`: `Graph`, `GraphSnapshot`, `FileRecord`, `FileState`, `FileUpdate`, graph mutation and lookup helpers
* `store.rs`: `Store`, `IndexPersistBatch`, `AuxiliaryPersistBatch`
* `memory_store.rs`: `MemoryStore` and its `Store` implementation
* `sqlite/mod.rs`: `SqliteStore` public entry point and high-level transaction wiring
* `sqlite/schema.rs`: schema initialization and schema-version handling
* `sqlite/graph_io.rs`: load/save/delete file state, root-node persistence, derived-edge persistence
* `sqlite/snapshots.rs`: generic snapshot row helpers and history/outcome/episodic/inference/coordination/curator persistence
* `sqlite/projections.rs`: projection snapshot row load/save and delta application
* `sqlite/codecs.rs`: enum encode/decode helpers, fingerprint decode helpers, SQL conversion helpers

Recommended extraction order:

1. `store.rs`
2. `memory_store.rs`
3. `sqlite/codecs.rs`
4. `sqlite/projections.rs`
5. `sqlite/snapshots.rs`
6. `sqlite/graph_io.rs`
7. `sqlite/schema.rs`
8. `graph.rs`
9. shrink `lib.rs` to re-exports plus crate-level glue

Status:

* completed
* `lib.rs` is now a facade
* tests have moved into `tests.rs`

Rationale:

* the trait and in-memory backend are mostly mechanical moves
* SQLite helper extraction is high-signal and low-risk
* `Graph` should move after the persistence helpers are separated, otherwise the file split will still leave too many cross-cutting edits in one step

### `prism-core`

Current clusters:

* workspace session API
* FS watcher thread and refresh loop
* curator queue state and curator-trigger logic
* workspace indexer construction and main indexing pipeline
* auto patch-outcome recording
* layout discovery and package normalization
* file move detection and reanchor inference
* unresolved edge resolution

Target structure:

```text
crates/prism-core/src/
  lib.rs
  session.rs
  watch.rs
  curator.rs
  curator_support.rs
  indexer.rs
  indexer_support.rs
  layout.rs
  patch_outcomes.rs
  reanchor.rs
  resolution.rs
  util.rs
  tests.rs
```

Module ownership:

* `session.rs`: `WorkspaceSession`, persistence helpers, coordination mutation wrapper
* `watch.rs`: `WatchHandle`, watcher thread bootstrap, watch-event filtering, refresh entry point
* `curator.rs`: curator handles, queue state, and enqueue logic
* `curator_support.rs`: curator context building, trigger selection, focus extraction, sequence derivation
* `indexer.rs`: `WorkspaceIndexer` and indexing entry points
* `indexer_support.rs`: session assembly, scan collection, and resolution-pass support helpers
* `layout.rs`: `WorkspaceLayout`, `PackageInfo`, manifest discovery, identifier normalization, root node sync
* `patch_outcomes.rs`: auto patch outcome creation, anchor dedupe, patch summaries
* `reanchor.rs`: move detection, reanchor inference, candidate scoring
* `resolution.rs`: unresolved calls/imports/impls resolution
* `util.rs`: timestamp, hashing, walk filters, small generic helpers that remain shared

Recommended extraction order:

1. `util.rs`
2. `layout.rs`
3. `resolution.rs`
4. `reanchor.rs`
5. `patch_outcomes.rs`
6. `curator.rs`
7. `watch.rs`
8. `session.rs`
9. `indexer.rs`
10. shrink `lib.rs` to public entry points and re-exports

Status:

* first pass completed
* `lib.rs` is now a facade
* root tests have moved into `tests.rs`
* `curator` and `indexer` each now have dedicated support modules
* likely next resume point: split `session.rs` if future growth justifies it

Rationale:

* the pure helper modules are easiest to move without semantic risk
* `curator`, `watch`, and `session` are stateful and should move only after shared helper boundaries settle
* `indexer` should be the last large move because the other modules are dependencies of that orchestration path

### `prism-mcp`

Current clusters:

* CLI and server bootstrap
* session-local task and limit state
* tool argument and schema types
* MCP resource URI parsing and pagination
* query host state and workspace refresh access
* resource payload builders
* JS worker runtime and TypeScript transpilation
* query execution methods
* Prism-to-view conversion helpers
* query-side input, dispatch, and conversion helpers

Target structure:

```text
crates/prism-mcp/src/
  lib.rs
  main.rs
  common.rs
  server_surface.rs
  session_state.rs
  tool_args.rs
  resource_schemas.rs
  resources.rs
  host_resources.rs
  host_mutations.rs
  query_types.rs
  query_helpers.rs
  query_runtime.rs
  js_runtime.rs
  views.rs
  tests.rs
```

Module ownership:

* `main.rs`: binary entry point
* `common.rs`: shared utility and result helpers
* `server_surface.rs`: `PrismMcpServer`, router setup, MCP handler implementation, tool registration
* `session_state.rs`: `SessionState`, `SessionTaskState`, session limit and current-task management
* `tool_args.rs`: tool argument structs, mutation payloads, and result types
* `resource_schemas.rs`: resource payload structs and view schema types
* `resources.rs`: resource URI parsing, URI builders, pagination helpers, resource-link helpers
* `host_resources.rs`: read-only `QueryHost` resource payload assembly
* `host_mutations.rs`: explicit write and mutation handlers
* `query_types.rs`: query-side input, output, and dispatch support types
* `query_helpers.rs`: query-side lookup and view assembly helpers
* `query_runtime.rs`: `QueryExecution` and query-style execution methods
* `js_runtime.rs`: `JsWorker`, worker messages, runtime bootstrap, TypeScript transpilation
* `views.rs`: Prism-to-view mapping helpers
* `tests.rs`: crate integration-style tests previously embedded in `lib.rs`

Extraction order used in practice:

1. `session_state.rs`
2. `resources.rs`
3. `js_runtime.rs`
4. `views.rs`
5. `host_resources.rs`
6. `host_mutations.rs`
7. `query_helpers.rs`
8. `query_types.rs`
9. `query_runtime.rs`
10. `server_surface.rs`
11. `tool_args.rs`
12. `resource_schemas.rs`
13. `common.rs`
14. `tests.rs`
15. shrink `lib.rs` to module wiring and public exports

Status:

* completed
* `lib.rs` is now a facade instead of the implementation center
* the largest remaining files are concentrated where they represent real ownership boundaries rather than crate-root sprawl

Rationale:

* the data-only and helper-only sections can move with minimal risk
* the JS runtime should be isolated before moving query host logic because it is a self-contained execution subsystem
* the server surface should move late because it touches most other MCP modules and is the highest fan-out point

## Execution Waves

Use the same wave structure in each crate.

### Wave 1: Scaffolding

For one crate at a time:

* create the target module files
* move only types, constants, and pure helpers
* keep `lib.rs` as the public facade
* add `pub(crate)` visibility only where needed

Exit condition:

* the crate builds with the same public exports
* there is no behavior change

Status:

* completed for `prism-store`, `prism-core`, and `prism-mcp`

### Wave 2: Stateful Subsystems

After the helper modules settle:

* move stateful structs and their impls into the owning modules
* move transactional or threaded code after helper dependencies are already extracted
* keep cross-module dependencies one-directional where possible

Exit condition:

* each stateful subsystem has one obvious home
* the remaining `lib.rs` no longer contains business logic

Status:

* completed for `prism-store`
* substantially completed for `prism-core`
* completed for `prism-mcp`

### Wave 3: Tests And Cleanup

Once the module moves are complete:

* move tests next to their modules where that improves locality
* remove stale helper duplication created during extraction
* standardize internal naming and imports
* document any intentionally public re-exports in `lib.rs`

Exit condition:

* tests are no longer anchored to one giant file
* the crate root reads like an API facade rather than an implementation dump

Status:

* completed for `prism-store`, `prism-core`, and `prism-mcp`
* remaining cleanup is now selective rather than structural

## Concrete First Pass

The most efficient first pass is:

1. split `prism-store` into `store.rs`, `memory_store.rs`, and `sqlite/{schema,graph_io,snapshots,projections,codecs}.rs`
2. split `prism-core` into `layout.rs`, `resolution.rs`, `reanchor.rs`, and `patch_outcomes.rs`
3. split `prism-mcp` into session-state, schema, resource, view, and query-helper modules before moving the server surface and runtime

That first pass delivers most of the maintainability benefit without forcing deep state rewrites.

Status:

* completed, although `prism-mcp` landed with a more granular shape than the original sketch

## Current Resume Points

If modularization work resumes later, the highest-value next steps are:

1. split oversized test modules where locality matters, especially in `prism-mcp`
2. keep `prism-core::session` under review so it does not become the next implementation sink
3. tighten imports, visibility, and ownership comments in the newly created modules
4. only do further splitting when it improves ownership, not merely line counts

## Success Criteria

This decomposition effort is successful when:

* no large crate centers its implementation in a single `lib.rs`
* each major subsystem has a stable module home
* new work has an obvious destination without growing crate-root files
* the public API remains stable through the refactor
* the refactor can land in small reviewable steps rather than one rewrite
