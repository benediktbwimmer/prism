# Implement shared coordination refs and bind git execution to them

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:ac9851f4f45b2e33d3d0b070241f8212929c94fa0415538af5e4b33260201662`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 0 overlays`

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
- Status: `ready`

### Move shared coordination truth onto the new ref plane for claims, leases, task status, publish state, and integration state

- Node id: `coord-task:01kna8sy5barah56gz9x3fs2fx`
- Kind: `edit`
- Status: `ready`

### Rebind strict require git execution to shared coordination publication and integration state

- Node id: `coord-task:01kna8t3rvk3n07vafmpsdzvvn`
- Kind: `edit`
- Status: `ready`

### Validate shared cross-branch coordination and dogfood the new git-execution path on top of it

- Node id: `coord-task:01kna8ta5hywfef8t87dn6bret`
- Kind: `edit`
- Status: `ready`

## Edges

- `plan-edge:coord-task:01kna8sy5barah56gz9x3fs2fx:depends-on:coord-task:01kna8srw6ccvntg5qb23zprqr`: `coord-task:01kna8sy5barah56gz9x3fs2fx` depends on `coord-task:01kna8srw6ccvntg5qb23zprqr`
- `plan-edge:coord-task:01kna8t3rvk3n07vafmpsdzvvn:depends-on:coord-task:01kna8sy5barah56gz9x3fs2fx`: `coord-task:01kna8t3rvk3n07vafmpsdzvvn` depends on `coord-task:01kna8sy5barah56gz9x3fs2fx`
- `plan-edge:coord-task:01kna8ta5hywfef8t87dn6bret:depends-on:coord-task:01kna8t3rvk3n07vafmpsdzvvn`: `coord-task:01kna8ta5hywfef8t87dn6bret` depends on `coord-task:01kna8t3rvk3n07vafmpsdzvvn`

