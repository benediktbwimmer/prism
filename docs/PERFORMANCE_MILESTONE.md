# PRISM Performance Milestone

## Purpose

This milestone packages the current PRISM performance audit into a prioritized execution plan.
The goal is not to land one isolated fix, but to reduce steady-state CPU, memory, and process
overhead across the `prism-mcp` serving path.

## Audit Baseline

Observed on March 26, 2026 in `/Users/bene/code/prism`:

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

## Main Findings

### 1. Request handling forces full workspace refresh

`QueryHost::refresh_workspace()` in
[crates/prism-mcp/src/lib.rs](/Users/bene/code/prism/crates/prism-mcp/src/lib.rs#L449)
unconditionally calls `workspace.refresh_fs()`.

That refresh path reaches
[crates/prism-core/src/watch.rs](/Users/bene/code/prism/crates/prism-core/src/watch.rs#L104),
which constructs a fresh `WorkspaceIndexer` and runs `index_with_trigger()`. In practice this
means normal reads and mutations can trigger full repo refresh work.

This affects:

- session reads in [crates/prism-mcp/src/host_resources.rs](/Users/bene/code/prism/crates/prism-mcp/src/host_resources.rs#L30)
- TypeScript query execution in [crates/prism-mcp/src/query_runtime.rs](/Users/bene/code/prism/crates/prism-mcp/src/query_runtime.rs#L65)
- task and mutation flows in [crates/prism-mcp/src/host_mutations.rs](/Users/bene/code/prism/crates/prism-mcp/src/host_mutations.rs#L127)

### 2. Process topology multiplies the cost

The Codex launcher in
[scripts/prism-mcp-codex-launcher.sh](/Users/bene/code/prism/scripts/prism-mcp-codex-launcher.sh#L41)
starts a daemon and then `exec`s a bridge process in
[scripts/prism-mcp-codex-launcher.sh](/Users/bene/code/prism/scripts/prism-mcp-codex-launcher.sh#L67).

Bridge mode in
[crates/prism-mcp/src/daemon_mode.rs](/Users/bene/code/prism/crates/prism-mcp/src/daemon_mode.rs#L72)
connects to the daemon and then serves stdio indefinitely via
[crates/prism-mcp/src/proxy_server.rs](/Users/bene/code/prism/crates/prism-mcp/src/proxy_server.rs#L52).

This is not itself the core CPU bug, but it amplifies memory and connection pressure and makes
daemon inefficiency much more visible.

### 3. Projection storage and memory look oversized

`ProjectionIndex` stores all co-change neighbors in memory as
`HashMap<LineageId, Vec<CoChangeRecord>>` in
[crates/prism-projections/src/projections.rs](/Users/bene/code/prism/crates/prism-projections/src/projections.rs#L13).

The derive path materializes the full co-change graph in
[crates/prism-projections/src/projections.rs](/Users/bene/code/prism/crates/prism-projections/src/projections.rs#L31),
while common query sites usually request only the top 8 neighbors, for example:

- [crates/prism-query/src/impact.rs](/Users/bene/code/prism/crates/prism-query/src/impact.rs#L215)
- [crates/prism-mcp/src/semantic_contexts.rs](/Users/bene/code/prism/crates/prism-mcp/src/semantic_contexts.rs#L157)

The incremental update path also does linear search plus full resort on each co-change delta in
[crates/prism-projections/src/projections.rs](/Users/bene/code/prism/crates/prism-projections/src/projections.rs#L343).

### 4. Persistence is functional but not tuned

`SqliteStore::open()` in
[crates/prism-store/src/sqlite/mod.rs](/Users/bene/code/prism/crates/prism-store/src/sqlite/mod.rs#L20)
opens the database and initializes schema, but there is no obvious WAL or connection tuning.

Refresh persistence also rewrites derived edges wholesale in
[crates/prism-store/src/sqlite/graph_io.rs](/Users/bene/code/prism/crates/prism-store/src/sqlite/graph_io.rs#L347),
which magnifies the cost of full refreshes.

### 5. Workspace sessions always bring background machinery

`build_workspace_session()` in
[crates/prism-core/src/indexer_support.rs](/Users/bene/code/prism/crates/prism-core/src/indexer_support.rs#L44)
always creates a watcher and curator handle for a session.

That may be correct for some modes, but it should be treated as a cost center and made explicit
when optimizing long-lived MCP serving.

## Milestone Scope

This milestone covers the performance of long-lived `prism-mcp` serving for local agent usage.

In scope:

- steady-state daemon CPU
- repeated query latency on unchanged workspaces
- refresh cost after small edits
- memory footprint of the long-lived daemon
- bridge and connection overhead
- persistence overhead in the refresh path

Out of scope for this milestone:

- unrelated parser correctness changes
- large feature work on new MCP APIs
- UI-level Codex client lifecycle policy outside what PRISM can control locally

## Prioritized Work

### P0. Split freshness checks from full refresh

Problem:

- Request handling currently treats "serve a query" and "rebuild workspace state" as nearly the
  same operation.

Required outcome:

- Repeated reads on an unchanged repo do not reindex the workspace.
- Session reads, `prism_query`, and common mutations hit a cheap fast path when nothing changed.

Candidate work:

- Add explicit workspace revision or dirtiness tracking.
- Make `refresh_workspace()` skip `refresh_fs()` when no change has been observed.
- Reload episodic and inference state only when persisted revisions changed.

### P1. Make refresh incremental instead of reconstructive

Problem:

- `refresh_prism_snapshot()` constructs a fresh `WorkspaceIndexer` and re-runs indexing work.

Required outcome:

- A one-file edit only reparses and updates affected file state and derived projections.
- Refresh cost scales with changed files, not total workspace size.

Candidate work:

- Reuse live in-memory graph/session state instead of reconstructing from scratch.
- Update changed file records in place.
- Re-resolve only affected derived edges where possible.

### P1. Reduce projection footprint and update cost

Problem:

- Co-change data is far larger than typical query demand.

Required outcome:

- Long-lived daemon memory drops substantially.
- Co-change maintenance cost is bounded.

Candidate work:

- Cap stored neighbors per lineage by score.
- Consider sparse or demand-driven co-change lookup for long-tail neighbors.
- Replace linear scan plus full sort in `increment_co_change_neighbor()` with a structure that
  supports bounded top-k maintenance efficiently.

### P2. Rationalize bridge and daemon lifecycle

Problem:

- Bridge count grows with connected clients, and long-lived stdio bridges hold persistent daemon
  connections.

Required outcome:

- One daemon per workspace.
- Bridge count is bounded by actual active clients and stale bridges do not accumulate.

Candidate work:

- Add clearer lifetime and health semantics for bridge processes.
- Add optional idle shutdown or reaping behavior where compatible with clients.
- Document expected topology per client type so process count is explainable.

### P2. Tune SQLite and write amplification

Problem:

- The persistence layer does broad rewrites and lacks obvious runtime tuning.

Required outcome:

- Lower write latency and lower lock contention during refresh and mutation bursts.

Candidate work:

- Enable WAL and connection pragmas appropriate for local single-user serving.
- Avoid delete-and-reinsert passes for unchanged derived edge sets.
- Measure snapshot and projection write size before and after refresh work lands.

### P3. Cache TypeScript query preparation

Problem:

- `prism_query` transpiles TypeScript on each execution.

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
- Long-lived daemon RSS is reduced by at least 60% from the current baseline.
- Bridge count and daemon connection count remain stable under normal Codex usage instead of
  monotonically increasing.

## Recommended Execution Order

1. Land `P0` first.
2. Measure again.
3. Land `P1` refresh changes.
4. Measure again.
5. Land projection footprint work.
6. Tune topology and persistence once the core refresh path is no longer pathological.
7. Add TypeScript query caching after the larger structural issues are fixed.

## First Implementation Slice

The first concrete implementation slice for this milestone should be:

1. Add cheap workspace dirtiness tracking.
2. Gate `refresh_workspace()` behind that tracking.
3. Add instrumentation around refresh entry, refresh duration, and whether the fast path or full
   path ran.
4. Re-measure daemon CPU, RSS, and query latency before taking on the larger incremental refresh
   refactor.
