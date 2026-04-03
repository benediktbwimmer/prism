# Eliminate the current slow compact MCP calls by instrumenting refresh-wrapper latency precisely, adding a true current-state fast path for compact reads, coalescing concurrent refresh work across sessions, and validating the resulting latency reduction against live PRISM MCP traces.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:249c8fa3d0965aa0f6fa16adebcc381c9bd6d878c934f54e690844154b353e55`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn7wmt2vfdy77mxc3apcp1qw`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `6`

## Goal

Eliminate the current slow compact MCP calls by instrumenting refresh-wrapper latency precisely, adding a true current-state fast path for compact reads, coalescing concurrent refresh work across sessions, and validating the resulting latency reduction against live PRISM MCP traces.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn7wmt2vfdy77mxc3apcp1qw.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn7wn9a19ytcvcza32byvhcs`

## Nodes

### Instrument compact refresh-wrapper latency and queueing boundaries

- Node id: `coord-task:01kn7wn9a19ytcvcza32byvhcs`
- Kind: `edit`
- Status: `completed`
- Summary: Added explicit accounted-vs-unattributed runtime refresh timing to compact and prism_query traces, including a runtimeSync.unattributed phase and envelope args that expose wrapper overhead in live mcpTrace output.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Current compact read traces expose where wall time is spent inside refresh-wrapper work rather than only showing one coarse compact.refreshWorkspace bucket. [any]

### Add a true current-state fast path for compact read tools

- Node id: `coord-task:01kn7wnjqg9s6j9ttbvmh1z3g3`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- prism_open, prism_workset, and related compact reads skip refresh-wrapper bookkeeping when FS and runtime revisions are already current. [any]

### Coalesce concurrent compact-read refresh work across sessions

- Node id: `coord-task:01kn7wnt1me2w8bsq5pkz31tvt`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Multiple sessions hitting compact tools during the same stale window share one refresh operation instead of serializing behind repeated wrapper work. [any]

### Reduce bridge pressure and expose contention diagnostics for slow compact reads

- Node id: `coord-task:01kn7wp5gccdrhaq1y969tpbtv`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- The daemon surfaces enough bridge and contention state to explain when many idle or duplicate sessions contribute to compact-read latency. [any]

### Validate compact-read latency improvements against live MCP traces and full repo checks

- Node id: `coord-task:01kn7wpf732s15kpc7fnwj50ns`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Live prism.mcpLog and prism.mcpTrace evidence show compact reads no longer spend most wall time in refresh-wrapper bookkeeping under current-state usage. [any]
- Targeted tests, full workspace tests, release rebuild, daemon restart, status, and health all pass. [any]

## Edges

- `plan-edge:coord-task:01kn7wnjqg9s6j9ttbvmh1z3g3:depends-on:coord-task:01kn7wn9a19ytcvcza32byvhcs`: `coord-task:01kn7wnjqg9s6j9ttbvmh1z3g3` depends on `coord-task:01kn7wn9a19ytcvcza32byvhcs`
- `plan-edge:coord-task:01kn7wnt1me2w8bsq5pkz31tvt:depends-on:coord-task:01kn7wn9a19ytcvcza32byvhcs`: `coord-task:01kn7wnt1me2w8bsq5pkz31tvt` depends on `coord-task:01kn7wn9a19ytcvcza32byvhcs`
- `plan-edge:coord-task:01kn7wp5gccdrhaq1y969tpbtv:depends-on:coord-task:01kn7wn9a19ytcvcza32byvhcs`: `coord-task:01kn7wp5gccdrhaq1y969tpbtv` depends on `coord-task:01kn7wn9a19ytcvcza32byvhcs`
- `plan-edge:coord-task:01kn7wpf732s15kpc7fnwj50ns:depends-on:coord-task:01kn7wnjqg9s6j9ttbvmh1z3g3`: `coord-task:01kn7wpf732s15kpc7fnwj50ns` depends on `coord-task:01kn7wnjqg9s6j9ttbvmh1z3g3`
- `plan-edge:coord-task:01kn7wpf732s15kpc7fnwj50ns:depends-on:coord-task:01kn7wnt1me2w8bsq5pkz31tvt`: `coord-task:01kn7wpf732s15kpc7fnwj50ns` depends on `coord-task:01kn7wnt1me2w8bsq5pkz31tvt`
- `plan-edge:coord-task:01kn7wpf732s15kpc7fnwj50ns:depends-on:coord-task:01kn7wp5gccdrhaq1y969tpbtv`: `coord-task:01kn7wpf732s15kpc7fnwj50ns` depends on `coord-task:01kn7wp5gccdrhaq1y969tpbtv`

