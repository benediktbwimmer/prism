# PRISM Performance Milestone

## Purpose

This milestone packages the current PRISM performance audit into a prioritized execution plan.
The goal is not to land one isolated fix, but to reduce steady-state CPU, memory, and process
overhead across the `prism-mcp` serving path.

## Operating Rule

Every performance slice in this milestone must improve observability along with performance.

That means each meaningful optimization should leave behind at least one of:

- structured phase timing
- clearer error context
- counters or dimensions that explain cost
- lifecycle state that can be inspected from the CLI or daemon log

## Original Audit Baseline

Observed on March 26, 2026 in `/Users/bene/code/prism` before the milestone work landed:

- `prism-mcp` bridge/daemon topology was active through Codex launcher configuration.
- 21 `--mode bridge` processes were connected to a single `--mode daemon` process.
- The daemon showed roughly 4.2 GB RSS while `.prism/cache.db` was about 63 MB.
- `.prism/cache.db` contained about:
  - `nodes`: 3,856
  - `edges`: 8,255
  - `file_records`: 143
  - `projection_co_change`: 404,040
  - `projection_validation`: 11
- The main daemon had sustained historical CPU time and periodic high instantaneous CPU.

## Completed Work

As of later on March 26, 2026, several high-value slices are already landed.

Landed:

- Request handling now separates cheap freshness checks from broader persisted-state reloads.
- Workspace, episodic, inference, and coordination reloads are revision-aware.
- Co-change projections are capped to a bounded top-k in memory and in SQLite.
- SQLite is tuned for local daemon use and prunes legacy oversized co-change tables on open.
- `prism-cli mcp` now has `status`, `start`, `stop`, `restart`, `health`, and `logs`.
- MCP daemon/bootstrap paths now emit structured logs with phase timings and error chains.
- Startup and indexing paths now expose enough timing data to identify dominant phases from logs
  without external profiling.
- The JSON parser accepts JSONC-style config files such as `tsconfig.json`.
- Workspace traversal now honors Git ignore rules, so ignored trees such as `node_modules` are no
  longer fingerprinted or indexed.
- The daemon lifecycle is now self-managed in-process instead of depending on fragile shell
  detachment.

## Current Status

These changes materially shifted the live profile:

- The previous `node_modules/typescript/lib/*diagnosticMessages.generated.json` startup hotspot is
  gone.
- `pending_file_count` on this repo dropped from `204` to `150`.
- Startup parse/update loop time dropped from about `34s` to about `0.5s`.
- A healthy daemon now starts and stays alive under `prism-cli mcp start`.
- Healthy daemon RSS is now on the order of a few hundred MB rather than multi-GB.

## Latest Measured Baseline

Observed after the landed fixes on this repo:

- Healthy daemon startup completed in about `5s` to `7s` on recent runs when only a few files
  changed.
- A broader startup that still had to materialize and persist larger state took about `35s`.
- After Git ignore-aware traversal:
  - `pending_file_count`: `150`
  - `pending_bytes`: about `1.67 MB`
  - parse/update loop: about `0.5s`
- Healthy daemon RSS in steady state is now roughly `200 MB` to `300 MB` on this repo.

The current startup log shows the dominant remaining costs as:

- derived edge resolution: about `4.5s`
- SQLite persist/write amplification: between about `2.3s` and `29.7s`, depending on how much
  state has to be rewritten

## Runtime And MCP Log Baseline On April 2, 2026

Observed from live PRISM daemon and MCP log surfaces after the identity, protected-state, and
declared-work workstreams landed.

Primary sources:

- `prism.runtimeStatus()`
- `prism.runtimeTimeline({ limit: 20 })`
- `prism.mcpStats({ minDurationMs: 50 })`
- `prism.slowMcpCalls({ limit: 8, minDurationMs: 1000 })`
- `prism.slowQueries({ limit: 8, minDurationMs: 1000 })`
- `/Users/bene/.prism/repos/repo-8719dd7db144e96f/worktrees/worktree-8719dd7db144e96f/mcp/logs/prism-mcp-daemon.log`
- `/Users/bene/.prism/repos/repo-8719dd7db144e96f/worktrees/worktree-8719dd7db144e96f/mcp/logs/prism-mcp-call-log.jsonl`

### Live daemon shape

- `bridgeCount`: `32`
- `connectedBridgeCount`: `32`
- `daemonCount`: `1`
- worktree cache DB size: about `326.8 MB`
- daemon log size: about `15.5 MB`
- MCP call log size: about `40.3 MB`
- current daemon startup: about `2086 ms`
- current workspace build time: about `1851 ms`
- last refresh duration: about `782 ms`
- last refresh replay volume: `65293`
- materialized files: `372`
- materialized nodes: `17950`
- materialized edges: `27969`

The current daemon is healthy. Recent calls since the last restart are mostly fast, and the
slowest recent calls were inspection queries rather than ordinary repo work.

### Startup and recovery fixed-cost baseline

The current startup timeline still contains meaningful fixed costs before the daemon becomes ready:

- cache store open: about `411 ms`
  - `schema_ms`: `27 ms`
  - `prune_ms`: `382 ms`
- graph snapshot load: about `98 ms`
- indexer preparation: about `850 ms`
  - `load_graph_ms`: `135 ms`
  - `load_coordination_ms`: `191 ms`
  - `derive_or_restore_projection_ms`: `304 ms`
- full daemon ready time: about `2086 ms`

This means startup is no longer pathological, but it still pays for prune, coordination load,
projection restore, and recovery replay work every time.

### Incremental refresh baseline

Recent incremental refreshes looked healthy enough for ordinary edits, but still expose a few
imbalances:

- one-file targeted refresh example:
  - `parse_apply_ms`: `26 ms`
  - `persist_ms`: `3 ms`
  - indexer total: `45 ms`
  - refresh pipeline total: `382 ms`
- another one-file targeted refresh example:
  - indexer total: `27 ms`
  - refresh pipeline total: `552 ms`

The refresh path is no longer broadly reconstructive for every small edit, but the surrounding
build-indexer and pipeline overhead still dominates pure indexing time.

### Current hotspot classes

#### 1. `prism_task_brief` still has the worst historical tail latency

Historical slow-call and slow-query surfaces show:

- average `prism_task_brief` duration around `14.9 s`
- worst observed slow MCP call at about `914,697 ms`
- second worst observed slow MCP call at about `794,864 ms`

The worst traces are dominated by:

- `runtimeSync.deferred`
- `compact.refreshWorkspace`
- `compact.handler`

This is the clearest compact-surface latency hotspot.

#### 2. coordination mutations can still stall behind `refresh_lock`

Historical slow-call surfaces also show failed `prism_mutate` coordination requests that waited on
refresh admission for far too long:

- about `189,726 ms`
- about `69,726 ms`

The corresponding trace phases are centered on:

- `mutation.coordination.waitRefreshLock`
- `mutation.coordination.refreshWorkspace`

That is an admission-policy problem, not just a raw compute problem.

#### 3. co-change work still explodes on some ordinary edits

The current daemon logs still show indexed edits with large co-change fanout even when the edit set
is tiny:

- one recent one-file refresh produced `co_change_delta_count: 11990`
- another recent one-file refresh produced `co_change_delta_count: 72`

Recent log scans also showed repeated warnings of:

- `sampling symbol-level co-change deltas for oversized change set`

This means the remaining co-change work is still too sensitive to certain large or high-fanout
files.

#### 4. bridge transport logs are still noisy around reconnects and restarts

Recent daemon logs contain repeated warnings like:

- `sse stream error: body error: error decoding response body`
- `sse client event stream terminated with error: Err(TokioJoinError(...Cancelled...))`

These events appear around restart and reconnect churn. They look more like expected bridge
lifecycle noise than actual daemon correctness failures, but they currently pollute the warning
surface.

#### 5. bridge lifecycle visibility is still weak

The live daemon reports `32` connected bridges, with several processes older than `20` hours.
That may be correct for an active Codex session topology, but the current status surface does not
make it obvious which bridges are actively serving requests versus merely still connected.

### Immediate optimization hypotheses

The next targeted optimization passes should focus on:

1. slimming `prism_task_brief` so it can return a cheap useful answer before broader refresh or
   enrichment work
2. removing long `refresh_lock` stalls from coordination mutation admission, preferably by using a
   lighter snapshot path or failing fast with retry guidance
3. bounding or approximating oversized co-change work so small edit batches do not inherit large
   fanout costs from hot files
4. trimming startup or recovery fixed costs in store open, prune, coordination load, projection
   restore, and replay
5. reducing transport warning noise and exposing clearer active-versus-idle bridge lifecycle state

These hypotheses are now tracked explicitly in
`plan:01kn7cxa9tvnaa1svrgppcth4a`.

## Main Findings

### 1. Request handling used to force full workspace refresh

`QueryHost::refresh_workspace()` in
[crates/prism-mcp/src/lib.rs](/Users/bene/code/prism/crates/prism-mcp/src/lib.rs)
used to unconditionally reach `workspace.refresh_fs()`, and that refresh path reached
[crates/prism-core/src/watch.rs](/Users/bene/code/prism/crates/prism-core/src/watch.rs), which
constructed a fresh `WorkspaceIndexer` and ran `index_with_trigger()`.

Status:

- Partially addressed.
- Cheap-path and revision-aware reload work is landed.
- Real refreshes are still more reconstructive than they should be.

### 2. Process topology multiplies cost and confusion

The Codex launcher and bridge model still amplify memory and connection pressure, and they make
daemon inefficiency more visible when the daemon is slow or stale.

Status:

- Lifecycle control is materially better.
- Daemon startup is stable and inspectable.
- Bridge topology and stale-client policy are still open design work, but they are no longer the
  primary blocker.

### 3. Projection storage and maintenance were oversized

`ProjectionIndex` previously stored the full long-tail co-change graph in memory and on disk, even
though typical query sites usually ask for only a small top-k neighborhood.

Status:

- The largest unbounded long-tail problem is addressed by top-k capping.
- Projection delta volume is still large enough to make persistence expensive.

### 4. Persistence is still a major bottleneck

Connection tuning is now landed, but the persistence path still performs broad rewrites and large
delta application work.

Status:

- Connection pragmas and basic tuning are landed.
- Persistence cost is now one of the clearest remaining startup hotspots.

### 5. Workspace sessions still always bring background machinery

`build_workspace_session()` still creates watcher and curator machinery for sessions by default.

Status:

- Still open.
- This should be revisited after the current persistence and edge-resolution hotspots are reduced.

## Milestone Scope

This milestone covers the performance of long-lived `prism-mcp` serving for local agent usage.

In scope:

- steady-state daemon CPU
- repeated query latency on unchanged workspaces
- refresh cost after small edits
- memory footprint of the long-lived daemon
- bridge and connection overhead
- persistence overhead in the refresh path
- observability and instrumentation needed to make performance work explainable

Out of scope for this milestone:

- unrelated parser correctness changes beyond what is needed to keep the daemon alive
- large feature work on new MCP APIs
- UI-level Codex client lifecycle policy outside what PRISM can control locally

## Prioritized Work

### P0. Reduce edge-resolution and persist cost on startup and refresh

Problem:

- After ignored files were removed from the hot path, the dominant startup costs on this repo moved
  to derived edge resolution and SQLite persistence.

Required outcome:

- Startup and refresh spend substantially less time in `resolve_graph_edges()` and
  `commit_index_persist_batch()`.
- Near-no-op index passes avoid broad write work.

Candidate work:

- Stop re-resolving broad derived state when the input file set did not materially change.
- Measure and then reduce the largest contributors inside edge resolution.
- Reduce co-change delta write amplification and broad derived-edge rewrite behavior.
- Add more detailed persistence-batch instrumentation so large write bursts are explainable.

### P1. Make refresh incremental instead of reconstructive

Problem:

- `refresh_prism_snapshot()` still constructs a fresh `WorkspaceIndexer` and re-runs indexing work.

Required outcome:

- A one-file edit only reparses and updates affected file state and derived projections.
- Refresh cost scales with changed files, not total workspace size.

Candidate work:

- Reuse live in-memory graph/session state instead of reconstructing from scratch.
- Update changed file records in place.
- Re-resolve only affected derived edges where possible.

### P1. Reduce projection update cost further

Problem:

- Co-change storage is now bounded, but update volume is still high enough to dominate persistence
  on some runs.

Required outcome:

- Co-change maintenance cost drops substantially during indexing and refresh.
- Persistence cost becomes proportional to meaningful changes rather than broad lineage churn.

Candidate work:

- Replace linear scan plus full sort in co-change maintenance with a structure that supports
  bounded top-k maintenance efficiently.
- Re-examine whether every currently emitted co-change delta is worth persisting.

### P2. Rationalize bridge and daemon lifecycle

Problem:

- Bridge count still grows with connected clients, and long-lived bridges hold persistent daemon
  connections.

Required outcome:

- One daemon per workspace.
- Bridge count is bounded by actual active clients and stale bridges do not accumulate.

Candidate work:

- Add clearer lifetime and health semantics for bridge processes.
- Add optional idle shutdown or reaping behavior where compatible with clients.
- Document expected topology per client type so process count is explainable.

### P2. Revisit session background machinery

Problem:

- Workspace sessions always bring watcher and curator machinery even when serving mode does not
  need both.

Required outcome:

- Long-lived serving sessions only pay for the background subsystems they actually need.

Candidate work:

- Make watcher and curator setup mode-aware.
- Measure the steady-state memory and CPU effect of each background subsystem separately.

### P3. Cache TypeScript query preparation

Problem:

- `prism_query` still transpiles TypeScript on each execution.

Required outcome:

- Repeated identical queries avoid repeated transpilation work.

Candidate work:

- Cache transpiled source keyed by query text and language.
- Keep the current JS runtime warm and pair it with a bounded compile cache.

## Success Metrics

The milestone is complete when the following are all true on this repo or a representative repo of
similar size:

- Idle `prism-mcp` daemon CPU remains below 2% for at least 5 minutes with no active queries.
- Repeated identical `prism_query` calls on an unchanged workspace do not trigger indexing work.
- A single-file change refresh is at least 5x cheaper than a cold full index.
- Long-lived daemon RSS is reduced by at least 60% from the original baseline.
- Bridge count and daemon connection count remain stable under normal Codex usage instead of
  monotonically increasing.
- Daemon startup logs make the top 3 startup cost centers obvious without external profiling.

## Recommended Execution Order

1. Finish the current persistence and edge-resolution hotspot work.
2. Measure again using the structured startup timeline now in the daemon log.
3. Land incremental refresh changes so changed-file cost scales with actual scope.
4. Reduce co-change update amplification further if it still dominates persistence.
5. Revisit bridge topology and background-session machinery after the core path is efficient.
6. Add TypeScript query caching after the larger structural issues are fixed.

## Milestone Validation Recipes

Milestone 0 closes only when every later milestone has an explicit before/after measurement target
and a concrete validation recipe attached up front.

### `perf:m1-refresh-runtime`

Scope:
- Replace reconstructive read-path refresh work with a persistent runtime path and narrower locking.

Before:
- Small no-op and small-delta reads can still pay non-trivial request-path refresh cost.
- `refreshPath` can still degrade into a broader reload path on common read traffic.
- `lockHoldMs`, `loadedBytes`, `replayVolume`, and `fullRebuildCount` remain non-zero on cases
  that should stay incremental.

After:
- No-op and stale-auxiliary read refreshes report `refreshPath` of `none` or `deferred`.
- `fsRefreshMs` and `fullRebuildCount` stay at `0` on those read paths.
- `lockHoldMs`, `loadedBytes`, and `replayVolume` drop materially versus the before case.

Validation:
- `cargo test -p prism-mcp queries_skip_request_path_persisted_reload_when_runtime_is_current`
- `cargo test -p prism-mcp queries_defer_request_path_refresh_when_runtime_sync_is_busy`
- `cargo test -p prism-mcp unchanged_query_skips_workspace_refresh`

### `perf:m2-memory-suffix-ingest`

Scope:
- Make repo memory ingestion and episodic materialization suffix-based instead of replaying full
  state on unchanged or append-only paths.

Before:
- Episodic reload paths can repopulate state from a full snapshot or full event history even when
  only a suffix changed.
- `loadedBytes` and `replayVolume` scale with total memory state rather than the appended delta.

After:
- Append-only memory updates keep reload work proportional to the new suffix.
- `loadedBytes` and `replayVolume` for episodic refreshes track the appended material rather than
  the full persisted history.

Validation:
- `cargo test -p prism-core repo_memory_events_round_trip_through_committed_jsonl_and_reload`
- `cargo test -p prism-mcp refresh_workspace_reloads_updated_persisted_notes`

### `perf:m3-coordination-incremental`

Scope:
- Convert coordination reload and persistence to incremental read/write models.

Before:
- Coordination refreshes can still hydrate a broad coordination state payload after revision bumps.
- Coordination mutation paths can still force unnecessary persisted refresh work.

After:
- Coordination mutations append and hydrate only the necessary delta.
- Coordination reloads keep `loadedBytes`, `replayVolume`, and `fullRebuildCount` bounded to the
  changed coordination state.

Validation:
- `cargo test -p prism-mcp coordination_mutation_trace_records_persistence_subphases`
- `cargo test -p prism-mcp validation_feedback_tool_mutation_skips_request_path_refresh`
- `cargo test -p prism-mcp first_mutation_after_workspace_refresh_skips_persisted_reload`

### `perf:m4-projection-incremental`

Scope:
- Batch and incrementalize projection and concept maintenance so reload and indexing do not rebuild
  unaffected projection state.

Before:
- Projection upkeep can still trigger broad rebuild work on lineage or outcome churn.
- `loadedBytes` and `replayVolume` remain tied to whole-projection rebuilds rather than affected
  subsets.

After:
- Projection updates touch only the affected concept or projection slices.
- Incremental runs show lower `loadedBytes`, `replayVolume`, and `fullRebuildCount` than the
  original full-rebuild path.

Validation:
- `cargo test -p prism-core coordination_persistence_compacts_large_event_suffixes_into_optional_baseline`
- Re-run the startup and small-change refresh measurements from this document and compare
  projection-heavy cases before and after the change.

### `perf:m5-closeout-audit`

Scope:
- Prove the original audited hotspots are removed or explicitly quarantined to cold paths.

Before:
- Remaining audited hotspots still appear in runtime timelines, query traces, or daemon startup
  logs.

After:
- The audited hot paths no longer show broad rebuild work during ordinary request, mutate, reload,
  startup, or background flows.
- `loadedBytes`, `replayVolume`, and `fullRebuildCount` make residual whole-state work obvious if
  it regresses.

Validation:
- `cargo test -p prism-mcp compact_tool_query_trace_records_refresh_and_handler_phases`
- `cargo test -p prism-mcp mutation_trace_records_internal_phases_for_persisted_only_mutations`
- `cargo test -p prism-mcp refresh_workspace_reloads_updated_persisted_notes`
- Re-run the milestone startup, refresh, and steady-state daemon measurements from this document.

## Next Slice

The next concrete implementation slice for this milestone should be:

1. Instrument the persistence batch in more detail so the largest write contributors are explicit.
2. Reduce `commit_index_persist_batch()` write amplification for unchanged derived state.
3. Reduce or localize derived edge resolution work when only a small file subset changed.
4. Re-measure startup and small-change refresh cost after those two hotspots move.
