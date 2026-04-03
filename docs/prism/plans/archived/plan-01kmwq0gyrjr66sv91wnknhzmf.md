# Eliminate every remaining audited PRISM performance bottleneck by removing full-state refresh and hydration work from hot paths, suffixizing remaining persistence and reload flows, tightening locking boundaries, and validating that no audited hotspot remains on mutate, request, refresh, reload, startup, or background paths.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:c49f79a6103a25cbbbcac353ca279da40dc8b9088e31a950f1db4c06ec8e9746`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 8 edges, 0 overlays`

## Overview

- Plan id: `plan:01kmwq0gyrjr66sv91wnknhzmf`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `8`

## Goal

Eliminate every remaining audited PRISM performance bottleneck by removing full-state refresh and hydration work from hot paths, suffixizing remaining persistence and reload flows, tightening locking boundaries, and validating that no audited hotspot remains on mutate, request, refresh, reload, startup, or background paths.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kmwq0gyrjr66sv91wnknhzmf.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kmwq21ymh3cqdzbggynehfxv`

## Nodes

### Milestone 0: Establish performance baselines, budgets, and regression guardrails

- Node id: `coord-task:01kmwq21ymh3cqdzbggynehfxv`
- Kind: `investigate`
- Status: `completed`
- Summary: Established milestone baselines and guardrails: attached per-milestone validation recipes and before/after measurement targets, exposed lockHoldMs plus loadedBytes/replayVolume/fullRebuildCount in refresh telemetry, and validated the refresh-path baseline with targeted prism-mcp tests.
- Priority: `0`

#### Acceptance

- Benchmarks and traces cover no-op refresh, scoped refresh, stale auxiliary read refresh, coordination reload, coordination mutate, startup, and background indexing throughput [any]
- Every later milestone has explicit before/after metrics and a validation recipe attached before implementation starts [any]
- Instrumentation exposes lock hold time, replay volume, loaded bytes, and full-rebuild counts for the audited paths [any]

#### Tags

- `baseline`
- `benchmark`
- `perf`

### Milestone 1: Replace full refresh reconstruction with a persistent runtime and narrower lock boundaries

- Node id: `coord-task:01kmwq21zg4hy0etha7x0nj40a`
- Kind: `edit`
- Status: `in_progress`
- Summary: FS refresh now rebuilds the transient indexer from the live Prism runtime instead of reconstructing graph/history/outcomes/coordination/projections from persisted snapshots; the replacement Prism preserves coordination context and session-local projection state, with refresh-focused regression coverage in prism-core.
- Priority: `1`

#### Acceptance

- Ordinary filesystem refreshes no longer construct a fresh WorkspaceIndexer or rehydrate graph, history, outcomes, coordination, and projections from scratch [any]
- Read-side refresh does not hold one shared per-root sync lock across long-running reload and rebuild work [any]
- No-op and small-delta compact.refreshWorkspace runs stay on an incremental runtime path with measurable latency reduction [any]

#### Validation Refs

- `perf:m1-refresh-runtime`

#### Tags

- `locking`
- `perf`
- `refresh`
- `runtime`

### Milestone 2: Make repo memory ingestion and episodic materialization strictly suffix-based

- Node id: `coord-task:01kmwq220anv077nkqm8pqpe89`
- Kind: `edit`
- Status: `ready`
- Summary: Measure episodic and repo-memory reload work before and after suffix-based ingestion, using loadedBytes and replayVolume to confirm append-only updates stop scaling with total memory history.
- Priority: `1`

#### Acceptance

- snapshot_revisions and load_episodic_snapshot stop reparsing the full repo memory JSONL and stop replaying the full sqlite memory_event_log on unchanged or append-only paths [any]
- Episodic save and reanchor paths use materialized state plus high-water marks instead of full event-log reconstruction [any]
- Refresh and reload ingest only appended memory events while preserving current semantics across local and shared runtime stores [any]

#### Validation Refs

- `perf:m2-memory-suffix-ingest`

#### Tags

- `hydration`
- `memory`
- `perf`
- `persistence`

### Milestone 3: Convert coordination reload and persistence to incremental read/write models

- Node id: `coord-task:01kmwq2215db6wsf87n689k12p`
- Kind: `edit`
- Status: `ready`
- Summary: Measure coordination reload and mutation persistence before and after the incremental coordination path, using refresh traces plus loadedBytes, replayVolume, and fullRebuildCount to detect broad rehydration.
- Priority: `1`

#### Acceptance

- Coordination read refresh no longer rebuilds snapshot, published plan projection, and live graphs from full event and log replay on each revision bump [any]
- Coordination mutate persistence appends events without global existing-event scans and without reloading the full published plan projection [any]
- Published plan synchronization only touches changed plans, changed logs, and necessary index entries while preserving hydration correctness [any]

#### Validation Refs

- `perf:m3-coordination-incremental`

#### Tags

- `coordination`
- `perf`
- `published-plans`
- `reload`

### Milestone 4: Batch and incrementalize projection concept maintenance

- Node id: `coord-task:01kmwq2220x7gt4s80n933sh93`
- Kind: `edit`
- Status: `ready`
- Summary: Measure projection and concept maintenance before and after batching/incrementalization so loadedBytes, replayVolume, and fullRebuildCount drop on lineage and outcome-driven refreshes.
- Priority: `1`

#### Acceptance

- Indexing and outcome application no longer trigger full rebuild_concepts work on every lineage batch or every single outcome event [any]
- Startup and reload projection overlay paths avoid duplicate whole-concept rebuild passes when combining local, shared, and repo knowledge [any]
- Projection maintenance recomputes only the concept state affected by changed lineage, outcomes, or curated concept inputs [any]

#### Validation Refs

- `perf:m4-projection-incremental`

#### Tags

- `concepts`
- `perf`
- `projections`
- `startup`

### Milestone 5: Burn down residual bottlenecks and prove the audit is closed

- Node id: `coord-task:01kmwq222tvk94q3crqke24zw1`
- Kind: `validate`
- Status: `ready`
- Summary: Use the final startup, refresh, mutate, and background measurements plus the refresh telemetry counters to verify the original audit is closed and no hot path still forces broad rebuild work.
- Priority: `2`

#### Acceptance

- Before/after benchmarks show that each audited hotspot has been removed or demoted to an explicitly documented cold or fallback path [any]
- Regression coverage locks in incremental refresh, reload, mutate, startup, and background throughput invariants introduced by the earlier milestones [any]
- A final audit confirms no remaining bottleneck from the original list is still present on a hot, request, mutate, reload, startup, or background path [any]

#### Validation Refs

- `perf:m5-closeout-audit`

#### Tags

- `closeout`
- `perf`
- `validation`

## Edges

- `plan-edge:coord-task:01kmwq21zg4hy0etha7x0nj40a:depends-on:coord-task:01kmwq21ymh3cqdzbggynehfxv`: `coord-task:01kmwq21zg4hy0etha7x0nj40a` depends on `coord-task:01kmwq21ymh3cqdzbggynehfxv`
- `plan-edge:coord-task:01kmwq220anv077nkqm8pqpe89:depends-on:coord-task:01kmwq21ymh3cqdzbggynehfxv`: `coord-task:01kmwq220anv077nkqm8pqpe89` depends on `coord-task:01kmwq21ymh3cqdzbggynehfxv`
- `plan-edge:coord-task:01kmwq2215db6wsf87n689k12p:depends-on:coord-task:01kmwq21zg4hy0etha7x0nj40a`: `coord-task:01kmwq2215db6wsf87n689k12p` depends on `coord-task:01kmwq21zg4hy0etha7x0nj40a`
- `plan-edge:coord-task:01kmwq2220x7gt4s80n933sh93:depends-on:coord-task:01kmwq21zg4hy0etha7x0nj40a`: `coord-task:01kmwq2220x7gt4s80n933sh93` depends on `coord-task:01kmwq21zg4hy0etha7x0nj40a`
- `plan-edge:coord-task:01kmwq222tvk94q3crqke24zw1:depends-on:coord-task:01kmwq21zg4hy0etha7x0nj40a`: `coord-task:01kmwq222tvk94q3crqke24zw1` depends on `coord-task:01kmwq21zg4hy0etha7x0nj40a`
- `plan-edge:coord-task:01kmwq222tvk94q3crqke24zw1:depends-on:coord-task:01kmwq220anv077nkqm8pqpe89`: `coord-task:01kmwq222tvk94q3crqke24zw1` depends on `coord-task:01kmwq220anv077nkqm8pqpe89`
- `plan-edge:coord-task:01kmwq222tvk94q3crqke24zw1:depends-on:coord-task:01kmwq2215db6wsf87n689k12p`: `coord-task:01kmwq222tvk94q3crqke24zw1` depends on `coord-task:01kmwq2215db6wsf87n689k12p`
- `plan-edge:coord-task:01kmwq222tvk94q3crqke24zw1:depends-on:coord-task:01kmwq2220x7gt4s80n933sh93`: `coord-task:01kmwq222tvk94q3crqke24zw1` depends on `coord-task:01kmwq2220x7gt4s80n933sh93`

