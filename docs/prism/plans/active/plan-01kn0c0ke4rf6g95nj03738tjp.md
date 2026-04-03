# Trace and eliminate rare MCP slow-call outliers by adding universal request-path observability, attributing shared bottlenecks, and fixing the blocking behaviors those traces expose.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:76e06becc8c56b9baec684ca8f8d3a3eaf01df781b08fed3c80b7aeb5ca360e9`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn0c0ke4rf6g95nj03738tjp`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `5`

## Goal

Trace and eliminate rare MCP slow-call outliers by adding universal request-path observability, attributing shared bottlenecks, and fixing the blocking behaviors those traces expose.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn0c0ke4rf6g95nj03738tjp.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn0c0vv128bn1qyntk6nbenq`

## Nodes

### Universal request envelope

- Node id: `coord-task:01kn0c0vv128bn1qyntk6nbenq`
- Kind: `investigate`
- Status: `completed`
- Summary: Universal MCP request envelopes now record receive and route phases on real transport requests, merge into tool and resource traces, and fall back to generic request records for unowned paths like initialize and ping.

### Shared bottleneck coverage

- Node id: `coord-task:01kn0c0zcaprzd5d0v654wrt1v`
- Kind: `investigate`
- Status: `completed`
- Summary: Starting shared bottleneck coverage by attributing shared refresh, lock, worker, and persistence phases across request types that already have the universal outer envelope.

### Lock and queue observability

- Node id: `coord-task:01kn0c12nn24r0d6sdphez2gtj`
- Kind: `investigate`
- Status: `completed`
- Summary: Split waiting from work across all major locks and queueing boundaries so contention, queue delay, and actual execution time are distinguishable in the durable MCP traces.

### Automatic slow-call capture

- Node id: `coord-task:01kn0c15wy4qv1yz37kzqkfcv5`
- Kind: `investigate`
- Status: `completed`
- Summary: Emit structured runtime-state snapshots or equivalent metadata automatically when requests cross a slow-call threshold so rare outliers can be diagnosed from the durable log alone.

### Coverage closure

- Node id: `coord-task:01kn0c1941bxe5hyxtm90hvee9`
- Kind: `investigate`
- Status: `completed`
- Summary: Durable log audit now attributes fresh slow calls, captures dropped requests, and closes the publishTaskUpdate trace gap.

### Behavior fixes

- Node id: `coord-task:01kn0c1df70t70650rs2y9b261`
- Kind: `edit`
- Status: `completed`
- Summary: MCP request paths now use non-blocking admission for refresh_lock and workspace_runtime_sync_lock: reads defer, persisted-only refreshes fail fast, and user-facing mutations stop waiting indefinitely behind shared runtime work.

## Edges

- `plan-edge:coord-task:01kn0c0zcaprzd5d0v654wrt1v:depends-on:coord-task:01kn0c0vv128bn1qyntk6nbenq`: `coord-task:01kn0c0zcaprzd5d0v654wrt1v` depends on `coord-task:01kn0c0vv128bn1qyntk6nbenq`
- `plan-edge:coord-task:01kn0c12nn24r0d6sdphez2gtj:depends-on:coord-task:01kn0c0zcaprzd5d0v654wrt1v`: `coord-task:01kn0c12nn24r0d6sdphez2gtj` depends on `coord-task:01kn0c0zcaprzd5d0v654wrt1v`
- `plan-edge:coord-task:01kn0c15wy4qv1yz37kzqkfcv5:depends-on:coord-task:01kn0c12nn24r0d6sdphez2gtj`: `coord-task:01kn0c15wy4qv1yz37kzqkfcv5` depends on `coord-task:01kn0c12nn24r0d6sdphez2gtj`
- `plan-edge:coord-task:01kn0c1941bxe5hyxtm90hvee9:depends-on:coord-task:01kn0c15wy4qv1yz37kzqkfcv5`: `coord-task:01kn0c1941bxe5hyxtm90hvee9` depends on `coord-task:01kn0c15wy4qv1yz37kzqkfcv5`
- `plan-edge:coord-task:01kn0c1df70t70650rs2y9b261:depends-on:coord-task:01kn0c1941bxe5hyxtm90hvee9`: `coord-task:01kn0c1df70t70650rs2y9b261` depends on `coord-task:01kn0c1941bxe5hyxtm90hvee9`

