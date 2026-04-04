# Implement shared coordination refs and bind git execution to them

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7ccccf2637af0258ef021720308c4535c692879d4f204b95d9a3f535d7108b15`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 4 overlays`

## Overview

- Plan id: `plan:01kna9h1khpyyzjgeed0y1ggk5`
- Status: `archived`
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

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kna9h1khpyyzjgeed0y1ggk5.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kna9hj3ems9xhkzpd11nmdrw`

## Nodes

### Design the shared coordination ref storage, manifest chaining, and CAS update protocol

- Node id: `coord-task:01kna9hj3ems9xhkzpd11nmdrw`
- Kind: `edit`
- Status: `ready`

### Move shared coordination truth onto the new ref plane for claims, leases, task status, publish state, and integration state

- Node id: `coord-task:01kna9hj512cfn4r042z848gbq`
- Kind: `edit`
- Status: `ready`

### Rebind strict require git execution to shared coordination publication and integration state

- Node id: `coord-task:01kna9hj6m6r0sws8wn1ep3rry`
- Kind: `edit`
- Status: `ready`

### Validate shared cross-branch coordination and dogfood the new git-execution path on top of it

- Node id: `coord-task:01kna9hj84dbm3c7w0z1ykenss`
- Kind: `edit`
- Status: `ready`

## Edges

- `plan-edge:coord-task:01kna9hj512cfn4r042z848gbq:depends-on:coord-task:01kna9hj3ems9xhkzpd11nmdrw`: `coord-task:01kna9hj512cfn4r042z848gbq` depends on `coord-task:01kna9hj3ems9xhkzpd11nmdrw`
- `plan-edge:coord-task:01kna9hj6m6r0sws8wn1ep3rry:depends-on:coord-task:01kna9hj512cfn4r042z848gbq`: `coord-task:01kna9hj6m6r0sws8wn1ep3rry` depends on `coord-task:01kna9hj512cfn4r042z848gbq`
- `plan-edge:coord-task:01kna9hj84dbm3c7w0z1ykenss:depends-on:coord-task:01kna9hj6m6r0sws8wn1ep3rry`: `coord-task:01kna9hj84dbm3c7w0z1ykenss` depends on `coord-task:01kna9hj6m6r0sws8wn1ep3rry`

## Execution Overlays

- Node: `coord-task:01kna9hj3ems9xhkzpd11nmdrw`
- Node: `coord-task:01kna9hj512cfn4r042z848gbq`
- Node: `coord-task:01kna9hj6m6r0sws8wn1ep3rry`
- Node: `coord-task:01kna9hj84dbm3c7w0z1ykenss`

