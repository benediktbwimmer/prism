# Dogfood require-vs-auto git execution

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:e15aa0a29e04e73f385cff84409c7b23b575643ca7722da4908e4815c115ba5c`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 1 overlays`

## Overview

- Plan id: `plan:01kn8rqy29rqhqc21kf87arqe1`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Exercise live require-mode completion behavior on a temporary task.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn8rqy29rqhqc21kf87arqe1.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn8rr5zr0md0jzevcfssaqvc`

## Nodes

### Live require completion dogfood

- Node id: `coord-task:01kn8rr5zr0md0jzevcfssaqvc`
- Kind: `edit`
- Status: `in_progress`

## Edges

No published plan edges are currently recorded.

## Execution Overlays

- Node: `coord-task:01kn8rr5zr0md0jzevcfssaqvc`

