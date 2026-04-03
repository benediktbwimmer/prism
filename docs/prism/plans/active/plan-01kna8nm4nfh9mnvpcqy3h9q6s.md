# Implement shared coordination refs and bind git execution to them

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:21d874989497163afb7033b6aafd8b8e2286bfeeb008e79508a2b360594f6e33`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 4 overlays`

## Overview

- Plan id: `plan:01kna8nm4nfh9mnvpcqy3h9q6s`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `3`

## Goal

Build the shared coordination ref authority plane, move core coordination truth onto it, and rebind git execution policy to that shared cross-branch control plane.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`
- Max fetch age seconds: `300`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kna8nm4nfh9mnvpcqy3h9q6s.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kna8srw6ccvntg5qb23zprqr`

## Nodes

### Design the shared coordination ref storage, manifest chaining, and CAS update protocol

- Node id: `coord-task:01kna8srw6ccvntg5qb23zprqr`
- Kind: `edit`
- Status: `completed`

### Move shared coordination truth onto the new ref plane for claims, leases, task status, publish state, and integration state

- Node id: `coord-task:01kna8sy5barah56gz9x3fs2fx`
- Kind: `edit`
- Status: `completed`

### Rebind strict require git execution to shared coordination publication and integration state

- Node id: `coord-task:01kna8t3rvk3n07vafmpsdzvvn`
- Kind: `edit`
- Status: `completed`

### Validate shared cross-branch coordination and dogfood the new git-execution path on top of it

- Node id: `coord-task:01kna8ta5hywfef8t87dn6bret`
- Kind: `edit`
- Status: `completed`

## Edges

- `plan-edge:coord-task:01kna8sy5barah56gz9x3fs2fx:depends-on:coord-task:01kna8srw6ccvntg5qb23zprqr`: `coord-task:01kna8sy5barah56gz9x3fs2fx` depends on `coord-task:01kna8srw6ccvntg5qb23zprqr`
- `plan-edge:coord-task:01kna8t3rvk3n07vafmpsdzvvn:depends-on:coord-task:01kna8sy5barah56gz9x3fs2fx`: `coord-task:01kna8t3rvk3n07vafmpsdzvvn` depends on `coord-task:01kna8sy5barah56gz9x3fs2fx`
- `plan-edge:coord-task:01kna8ta5hywfef8t87dn6bret:depends-on:coord-task:01kna8t3rvk3n07vafmpsdzvvn`: `coord-task:01kna8ta5hywfef8t87dn6bret` depends on `coord-task:01kna8t3rvk3n07vafmpsdzvvn`

## Execution Overlays

- Node: `coord-task:01kna8srw6ccvntg5qb23zprqr`
  git execution status: `published`
  source ref: `task/shared-coordination-refs`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs`
- Node: `coord-task:01kna8sy5barah56gz9x3fs2fx`
  git execution status: `published`
  source ref: `task/shared-coordination-refs`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs`
- Node: `coord-task:01kna8t3rvk3n07vafmpsdzvvn`
  git execution status: `published`
  source ref: `task/shared-coordination-refs`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs`
- Node: `coord-task:01kna8ta5hywfef8t87dn6bret`
  git execution status: `published`
  source ref: `task/shared-coordination-refs`
  target ref: `origin/main`
  publish ref: `task/shared-coordination-refs`

