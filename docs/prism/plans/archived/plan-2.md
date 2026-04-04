# Redesign PRISM refresh/runtime so request paths never block on full persisted rebuilds, the live runtime is authoritative for serving reads and applying mutations, snapshots/cache become optional startup or recovery aids only, and repo-committed memories, concepts, and plans remain the only durable truth that must survive storage resets.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:a2d231158cd5b0abaeaad53278e762e3cbae32f232ed1e4f1a830665ba92e337`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 4 edges, 0 overlays`

## Overview

- Plan id: `plan:2`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `4`

## Goal

Redesign PRISM refresh/runtime so request paths never block on full persisted rebuilds, the live runtime is authoritative for serving reads and applying mutations, snapshots/cache become optional startup or recovery aids only, and repo-committed memories, concepts, and plans remain the only durable truth that must survive storage resets.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:2.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:8`

## Nodes

### Make mutations patch live runtime first and isolate admin-only rebuild flows

- Node id: `coord-task:10`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Normal mutations update live runtime directly and persist without pre-refresh [any]
- Snapshot reload or repair becomes explicit admin behavior only [any]
- Validation feedback, memory, concept, plan, and coordination writes avoid request-path rebuilds [any]

### Demote snapshots and cache to optional startup or recovery aids

- Node id: `coord-task:11`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Cold hydrate and explicit recovery paths are separated from steady-state serving [any]
- Live serving works with snapshots disabled [any]
- Storage reset or schema break does not endanger repo-committed memories, concepts, or plans [any]

### Instrument freshness and materialization state and validate latency behavior

- Node id: `coord-task:12`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Compact and query latency reflects handler cost instead of hidden synchronous rebuild cost [any]
- Logs can distinguish stale, deferred, shallow, deep, and recovery states [any]
- Refresh phases expose lock wait, revision check, delta application, and reload timing explicitly [any]

### Define refresh-runtime invariants and explicit delete list

- Node id: `coord-task:8`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Deprecated mechanisms and compatibility paths are explicitly listed for removal [any]
- Request paths are forbidden from full persisted rebuilds [any]
- Serving authority is defined as live runtime state, not snapshot reloads [any]

### Cut synchronous request-path refresh from read surfaces

- Node id: `coord-task:9`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Background refresh is the only owner of heavy refresh work [any]
- Compact, query, and resource reads do not call full persisted sync on the request path [any]
- Read freshness is observed and surfaced without blocking reload [any]

## Edges

- `plan-edge:coord-task:10:depends-on:coord-task:9`: `coord-task:10` depends on `coord-task:9`
- `plan-edge:coord-task:11:depends-on:coord-task:10`: `coord-task:11` depends on `coord-task:10`
- `plan-edge:coord-task:12:depends-on:coord-task:11`: `coord-task:12` depends on `coord-task:11`
- `plan-edge:coord-task:9:depends-on:coord-task:8`: `coord-task:9` depends on `coord-task:8`

