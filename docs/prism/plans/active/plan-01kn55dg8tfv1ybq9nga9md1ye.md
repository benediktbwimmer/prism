# Hit the refresh latency targets in docs/RUNTIME_REWRITE_ARCHITECTURE.md by replacing reconstructive workspace refresh with a true parallel-prepare / tiny-commit / parallel-settle runtime path, reducing hot-path persistence and lock contention, and validating the resulting daemon behavior against explicit p50/p95 budgets.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:443b898b82fca53790e580c4cf29d62c32f22ce4ea8bfd3733c58936811f702e`
- Source logical timestamp: `unknown`
- Source snapshot: `8 nodes, 10 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn55dg8tfv1ybq9nga9md1ye`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `8`
- Edges: `10`

## Goal

Hit the refresh latency targets in docs/RUNTIME_REWRITE_ARCHITECTURE.md by replacing reconstructive workspace refresh with a true parallel-prepare / tiny-commit / parallel-settle runtime path, reducing hot-path persistence and lock contention, and validating the resulting daemon behavior against explicit p50/p95 budgets.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn55dg8tfv1ybq9nga9md1ye.jsonl`

## Root Nodes

- `coord-task:01kn55dvkdfvxg8pdfk72p0h72`

## Nodes

### Instrument refresh latency against the architecture budgets

- Node id: `coord-task:01kn55dvkdfvxg8pdfk72p0h72`
- Kind: `investigate`
- Status: `in_progress`
- Summary: Live refresh instrumentation is now materially advanced. The daemon emits attributed refresh-pipeline and indexing timings from the live log, and the current pass used that evidence to remove two request-path bottlenecks: the async curator store-lock convoy and the incremental IndexPersistBatch clone. Live probes improved from 435ms add / 335ms delete to 139ms add / 229ms delete, with build_persist_batch_ms collapsing from 168ms and 91ms to 0ms. This node remains in progress because queue-wait, lock-hold, publish/settle, and checkpoint-lag coverage is still incomplete, and startup hydration remains a separate unclosed budget miss.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:116`
- Anchor: `file:256`
- Anchor: `file:343`
- Anchor: `file:84`

#### Acceptance

- The daemon exposes queue wait, lock hold, prepare, delta construction, commit, publish, and settle timing fields with enough detail to explain the current missing 'other' bucket. [any]
- A repeatable benchmark or log-extraction workflow reports p50 and p95 for tiny-edit publish latency, queue wait, direct dependent settle, and checkpoint lag. [any]

### Define the incremental delta-commit contract for refresh

- Node id: `coord-task:01kn55e2ery38vr30wxe3ca5r3`
- Kind: `decide`
- Status: `ready`
- Summary: Specify the exact boundary between parallel file-local preparation, tiny authoritative commit, and parallel settle so ordinary edits stop flowing through reconstructive WorkspaceIndexer refresh.
- Priority: `2`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:104`
- Anchor: `file:107`
- Anchor: `file:116`
- Anchor: `file:343`

#### Acceptance

- The design names the committed delta payload, file-facts substrate, domain freshness semantics, and what work is permitted inside the serialized commit. [any]
- The design explicitly removes reconstructive refresh from the normal edit path and identifies the residual recovery-only rebuild path. [any]

### Move file-local delta construction out of the serialized refresh path

- Node id: `coord-task:01kn55e93ftgm9bra7wfs4fnfs`
- Kind: `edit`
- Status: `ready`
- Summary: Build immutable candidate deltas in parallel from changed files and invalidation inputs, instead of mutating authoritative graph/history/outcome/projection state file-by-file inside the current refresh loop.
- Priority: `3`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:116`
- Anchor: `file:84`
- Anchor: `file:94`

#### Acceptance

- File-local parse and candidate delta construction run off the authoritative state and can execute in parallel for multi-file refreshes. [any]
- The current serial apply_parsed loop no longer owns parse-time history, co-change, outcome, and projection mutation work. [any]

### Reduce commit to authoritative apply, journal append, and generation publish

- Node id: `coord-task:01kn55egap84qbyxkarhq6y7vz`
- Kind: `edit`
- Status: `ready`
- Summary: Make the serialized commit kernel only apply prebuilt deltas to mutable runtime state, append the committed batch, and publish the next immutable generation with explicit domain freshness.
- Priority: `4`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:104`
- Anchor: `file:108`
- Anchor: `file:256`

#### Acceptance

- The commit path excludes broad parsing, rebuild, projection mutation, memory reanchor, and checkpoint work. [any]
- Generation publication exposes honest freshness for any domains left to settle after commit. [any]

### Move derived projections, memory reanchor, and coordination reloads fully behind settle

- Node id: `coord-task:01kn55eps6ms9b7hr6nexfnjhr`
- Kind: `edit`
- Status: `ready`
- Summary: Push non-authoritative downstream work behind coalesced settle lanes and surface pending or stale domain states rather than doing hidden work on the publish path.
- Priority: `5`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:108`
- Anchor: `file:256`
- Anchor: `file:84`

#### Acceptance

- Projection reload, memory reanchor, and coordination refresh run as explicit settle work with target-generation cancellation or coalescing. [any]
- Queries and runtime status expose pending or stale domains instead of implying fully current derived state after the fast publish path. [any]

### Separate hot commit persistence from asynchronous materialization and remove SQLite contention

- Node id: `coord-task:01kn55f0wgfw5rcrxzbqa0qpm4`
- Kind: `edit`
- Status: `ready`
- Summary: Keep the authoritative commit journal and minimal runtime state durable on the hot path, but push derived snapshots and checkpoint materialization behind low-contention workers that stop fighting SQLite locks.
- Priority: `6`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:316`
- Anchor: `file:76`
- Anchor: `file:84`

#### Acceptance

- The hot commit transaction excludes derived graph or projection materialization work not needed for correctness. [any]
- Nominal daemon usage no longer produces recurring checkpoint materializer 'database is locked' warnings. [any]

### Tighten coalescing, stale-work cancellation, and invalidation precision for repeated edits

- Node id: `coord-task:01kn55f7ffvqmnm2gkdptvdttd`
- Kind: `edit`
- Status: `ready`
- Summary: Reduce repeated work under edit storms by coalescing path requests to the latest revision, cancelling stale prepare or settle work aggressively, and shrinking dependency fanout for ordinary small edits.
- Priority: `7`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:107`
- Anchor: `file:256`
- Anchor: `file:84`

#### Acceptance

- Repeated edits to the same paths cancel or supersede stale prepare and settle work instead of replaying obsolete refreshes. [any]
- Small edits reduce dependency refresh scope and direct dependent work compared with the current reconstructive path. [any]

### Validate refresh latency against the document budgets and publish residual gaps

- Node id: `coord-task:01kn55feg438a0sr325zc2k313`
- Kind: `validate`
- Status: `ready`
- Summary: Use live daemon evidence and reproducible workloads to confirm whether the rewritten path hits the architecture budgets, and publish any remaining misses with attributed causes rather than hand-waving them away.
- Priority: `8`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:256`
- Anchor: `file:343`
- Anchor: `file:84`

#### Acceptance

- The repo has reproducible measurements for tiny-edit publish latency, queue wait, direct dependent settle latency, and checkpoint lag under nominal load. [any]
- If any budget is still missed, the remaining gap is attributed to named subsystems with follow-up work, not a generic 'needs tuning' conclusion. [any]

#### Validation Refs

- `bench:refresh-budget-validation`
- `log:live-daemon-refresh-timings`

## Edges

- `plan-edge:coord-task:01kn55e2ery38vr30wxe3ca5r3:depends-on:coord-task:01kn55dvkdfvxg8pdfk72p0h72`: `coord-task:01kn55e2ery38vr30wxe3ca5r3` depends on `coord-task:01kn55dvkdfvxg8pdfk72p0h72`
- `plan-edge:coord-task:01kn55e93ftgm9bra7wfs4fnfs:depends-on:coord-task:01kn55e2ery38vr30wxe3ca5r3`: `coord-task:01kn55e93ftgm9bra7wfs4fnfs` depends on `coord-task:01kn55e2ery38vr30wxe3ca5r3`
- `plan-edge:coord-task:01kn55egap84qbyxkarhq6y7vz:depends-on:coord-task:01kn55e93ftgm9bra7wfs4fnfs`: `coord-task:01kn55egap84qbyxkarhq6y7vz` depends on `coord-task:01kn55e93ftgm9bra7wfs4fnfs`
- `plan-edge:coord-task:01kn55eps6ms9b7hr6nexfnjhr:depends-on:coord-task:01kn55egap84qbyxkarhq6y7vz`: `coord-task:01kn55eps6ms9b7hr6nexfnjhr` depends on `coord-task:01kn55egap84qbyxkarhq6y7vz`
- `plan-edge:coord-task:01kn55f0wgfw5rcrxzbqa0qpm4:depends-on:coord-task:01kn55egap84qbyxkarhq6y7vz`: `coord-task:01kn55f0wgfw5rcrxzbqa0qpm4` depends on `coord-task:01kn55egap84qbyxkarhq6y7vz`
- `plan-edge:coord-task:01kn55f7ffvqmnm2gkdptvdttd:depends-on:coord-task:01kn55e93ftgm9bra7wfs4fnfs`: `coord-task:01kn55f7ffvqmnm2gkdptvdttd` depends on `coord-task:01kn55e93ftgm9bra7wfs4fnfs`
- `plan-edge:coord-task:01kn55feg438a0sr325zc2k313:depends-on:coord-task:01kn55dvkdfvxg8pdfk72p0h72`: `coord-task:01kn55feg438a0sr325zc2k313` depends on `coord-task:01kn55dvkdfvxg8pdfk72p0h72`
- `plan-edge:coord-task:01kn55feg438a0sr325zc2k313:depends-on:coord-task:01kn55eps6ms9b7hr6nexfnjhr`: `coord-task:01kn55feg438a0sr325zc2k313` depends on `coord-task:01kn55eps6ms9b7hr6nexfnjhr`
- `plan-edge:coord-task:01kn55feg438a0sr325zc2k313:depends-on:coord-task:01kn55f0wgfw5rcrxzbqa0qpm4`: `coord-task:01kn55feg438a0sr325zc2k313` depends on `coord-task:01kn55f0wgfw5rcrxzbqa0qpm4`
- `plan-edge:coord-task:01kn55feg438a0sr325zc2k313:depends-on:coord-task:01kn55f7ffvqmnm2gkdptvdttd`: `coord-task:01kn55feg438a0sr325zc2k313` depends on `coord-task:01kn55f7ffvqmnm2gkdptvdttd`

