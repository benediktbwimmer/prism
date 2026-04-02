# Refresh Runtime Redesign

This document is the execution artifact for `plan:2` / `coord-task:8`.

Its purpose is to define the hard architectural reset for PRISM refresh and runtime serving.

This is not a compatibility plan.

We do not need to preserve the current local database, snapshot formats, or request-path behavior.
The only durable state that must survive a storage reset is repo-committed truth already carried in
`.prism`, especially:

- repo memories
- repo concepts
- repo plans

## Problem Statement

The current refresh model has too many overlapping authorities and too many request-path branches.

In particular, normal reads and many mutations still synchronously enter persisted refresh logic:

- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `QueryHost::refresh_workspace_for_query`
- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `QueryHost::refresh_workspace_for_mutation`
- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `sync_persisted_workspace_state`
- [`crates/prism-core/src/session.rs`](/Users/bene/code/prism/crates/prism-core/src/session.rs)
  `WorkspaceSession::reload_persisted_prism_with_guard`
- [`crates/prism-mcp/src/server_surface.rs`](/Users/bene/code/prism/crates/prism-mcp/src/server_surface.rs)
  `execute_logged_mutation`

This is the direct cause of the observed stalls: request paths are still allowed to block on
snapshot/reload machinery that should never be on the hot path.

## Hard Decisions

### 1. Live runtime is the serving authority

The in-memory live runtime is the only authority for normal reads and normal mutations.

Queries, compact tools, resources, and mutation handlers read from live runtime state.
They do not rebuild that state from persisted storage before serving.

### 2. Request paths never do full persisted rebuilds

Normal request handling must never call a full persisted reload of the runtime graph, history,
outcomes, projections, or plan state.

If the live runtime is stale, request paths may:

- return current live state
- attach freshness/materialization metadata
- nudge the background runtime coordinator

They may not block on full reconstruction.

### 3. One owner for heavy refresh work

There is exactly one owner for heavy refresh work: the background runtime coordinator.

That coordinator owns:

- filesystem reconciliation
- incremental indexing
- dirty-region processing
- projection refresh
- optional snapshot writes
- optional background hydration or repair

Reads and mutations may signal it, but they do not perform its heavy work inline.

### 4. Mutations patch live state first

Normal mutations apply to live runtime state first.
Persistence happens after that as durability or export work, not as a prerequisite for serving the
mutated result.

This applies to:

- validation feedback
- memory writes
- concept writes
- concept relation writes
- coordination and plan mutations
- outcomes and inferred edges

### 5. Snapshots and cache are optional

Snapshots, sqlite reloads, cached projections, and similar artifacts are optional accelerators only.

They may help:

- cold start
- background checkpointing
- explicit repair
- debugging
- import/export

They are not required for steady-state serving.

### 6. Repo truth stays special

Repo-committed truth in `.prism` remains durable and special.

The runtime may be rebuilt from repo-published memories, concepts, plans, and source state.
The local database is disposable.

Repo-published `.prism` state must follow one runtime import rule:

- bootstrap may hydrate repo-published protected streams into runtime state
- a dedicated protected-state sync path may import later `.prism` changes into live runtime state
- normal read paths must not opportunistically import `.prism` streams on a per-domain basis

The normal source watcher must continue to ignore `.prism`.
That is intentional because repo-published protected streams need a different import path than
source-code edits; they should not fall back into source indexing by accident.

### 7. Freshness and materialization are explicit

Freshness must become observable runtime state rather than hidden blocking behavior.

The system should be able to say whether data is:

- current
- stale
- refresh pending
- shallow
- deep
- recovery mode

### 8. No backwards-compatibility constraints

We may delete current persistence and snapshot behavior aggressively.

If a schema, snapshot format, or local DB no longer matches the new model, we can drop it.

## Explicit Delete List

These are not “maybe later” cleanups. They are design-level deletions.

### Delete request-path persisted refresh

Remove normal request-path dependence on:

- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `QueryHost::refresh_workspace_for_query`
- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `QueryHost::refresh_workspace_for_mutation`
- [`crates/prism-mcp/src/workspace_runtime.rs`](/Users/bene/code/prism/crates/prism-mcp/src/workspace_runtime.rs)
  `sync_persisted_workspace_state`

These should be replaced by a lightweight runtime observation API that:

- returns freshness/materialization state
- optionally schedules background work
- never performs full reload inline

### Delete mutation-side pre-refresh as a policy default

Remove the current “persisted-only pre-refresh” behavior from:

- [`crates/prism-mcp/src/server_surface.rs`](/Users/bene/code/prism/crates/prism-mcp/src/server_surface.rs)
  `MutationRefreshPolicy::PersistedOnly`
- [`crates/prism-mcp/src/server_surface.rs`](/Users/bene/code/prism/crates/prism-mcp/src/server_surface.rs)
  `execute_logged_mutation`

The default mutation posture should become:

- patch live runtime
- persist asynchronously or as a follow-up durability step
- publish updated read models from live state

### Demote full persisted reload to admin-only behavior

Remove full-runtime rebuild from steady-state serving:

- [`crates/prism-core/src/session.rs`](/Users/bene/code/prism/crates/prism-core/src/session.rs)
  `WorkspaceSession::try_reload_persisted_prism`
- [`crates/prism-core/src/session.rs`](/Users/bene/code/prism/crates/prism-core/src/session.rs)
  `WorkspaceSession::reload_persisted_prism_with_guard`

Keep only an explicit recovery or admin path for:

- startup hydrate when configured
- repair after corruption
- developer debugging

### Stop using persisted revision checks as the general read gate

The current `snapshot_revisions()` and related loaded-revision checks are too entangled with the
steady-state request path.

They should no longer decide whether a query is allowed to proceed.

Instead, runtime state should track:

- source graph dirty state
- memory dirty state
- concept/projection dirty state
- coordination dirty state
- current materialization depth

### Delete “persist then reload to see your write”

Any write path that relies on “persisted store first, then reload the in-memory runtime” should be
considered invalid under the new design.

## Replacement Architecture

### A. Runtime planes

The redesign should use three planes, aligned with
[`docs/PERSISTENCE_STATE_CLASSIFICATION.md`](/Users/bene/code/prism/docs/PERSISTENCE_STATE_CLASSIFICATION.md):

- Repo truth plane: committed `.prism` events and source tree reality
- Runtime state plane: live mutable in-memory serving state
- Optional durability/cache plane: local DB, snapshots, checkpoints, read caches

The runtime state plane serves requests.
The durability/cache plane helps recovery or startup.

### B. Runtime coordinator

Introduce one runtime coordinator that owns refresh and materialization.

Suggested responsibilities:

- track dirty domains
- schedule background work
- apply incremental source deltas
- apply projection/materialization updates
- publish freshness state
- perform optional checkpoints
- expose admin recovery hooks

Suggested domains:

- source graph
- history/lineage
- outcomes and memory
- concepts and concept relations
- coordination and plan runtime
- inferred edges and projections

### C. Read path contract

Read-path contract:

1. read live runtime state
2. attach freshness/materialization metadata if relevant
3. optionally enqueue background refresh
4. return

Read paths must not:

- acquire full reload authority
- rebuild PRISM from sqlite
- block on snapshot replay
- scan full persisted history just to answer a compact lookup

### D. Mutation path contract

Mutation-path contract:

1. validate mutation input
2. apply mutation to live runtime state
3. emit domain events or persistence writes
4. mark affected domains dirty or refreshed
5. update derived read models from live state or schedule their background update
6. return the result from live state

This makes mutation latency reflect mutation work, not hidden refresh work.

### E. Startup and recovery

Startup and recovery become explicit phases, not background assumptions hidden in every tool call.

At startup, PRISM may:

- hydrate from repo truth only
- hydrate from repo truth plus optional local durability state
- rebuild optional caches after startup

Recovery may:

- discard incompatible local DB state
- replay repo truth
- rebuild optional derived stores

This is acceptable because the local DB is no longer the required durable truth.

### F. Freshness state model

Introduce an explicit freshness/materialization model for runtime status and query traces.

Suggested shape:

- `current`
- `stale`
- `refresh_queued`
- `refresh_running`
- `recovery_required`
- `materialization_shallow`
- `materialization_deep`

The exact enum names can change, but the semantics must be explicit and surfaced in logs.

## Initial Implementation Sequence

This document maps directly to `plan:2`.

### Step 1: Define and enforce the delete boundary

First execution task:

- finish this document
- treat the delete list above as the non-negotiable boundary

### Step 2: Replace request-path refresh with runtime observation

Replace current query/mutation refresh entrypoints with a lightweight API such as:

- `observe_runtime_state_for_read()`
- `observe_runtime_state_for_mutation()`

Those functions should:

- inspect live freshness state
- optionally schedule coordinator work
- return metadata only

They should not trigger full persisted sync.

### Step 3: Move mutation implementations to live-state-first updates

Hosted mutations should update live runtime state directly and persist second.

This likely requires new narrowly scoped modules rather than growing existing mixed files.

Suggested ownership split:

- runtime coordinator module
- freshness state module
- read-path observation module
- mutation apply module
- durability/checkpoint module
- admin recovery module

### Step 4: Demote or remove snapshot reload plumbing from steady state

After read and mutation cutover, remove full persisted reload logic from normal serving.

Any remaining snapshot/reload path should be:

- startup-only
- admin-only
- clearly named as recovery/checkpoint logic

### Step 5: Rebuild telemetry around the new model

Telemetry should report:

- read-path observe cost
- mutation apply cost
- coordinator queue delay
- coordinator work cost by domain
- optional checkpoint cost
- admin recovery cost

## Non-Goals For This Step

This redesign step does not require:

- remote backend implementation
- large-repo ranking changes
- DB-pushed graph traversals
- broad lazy deep parsing

Those belong later, after the serving model is correct.

## Acceptance Criteria For `coord-task:8`

This step is complete when:

- the serving authority is explicitly defined as live runtime state
- full persisted rebuild is explicitly banned from normal request paths
- the delete list names the current functions and policies to remove or demote
- the replacement architecture is concrete enough to drive the next implementation step
