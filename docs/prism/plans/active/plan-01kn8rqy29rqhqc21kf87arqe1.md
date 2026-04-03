# Dogfood require-vs-auto git execution

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:396a3c879559c8f55b673c1a5cd18086c2e9c51788a2c122e5d8a4b4f345240f`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 0 overlays`

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

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn8rqy29rqhqc21kf87arqe1.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn8rqy29rqhqc21kf87arqe1.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn8rr5zr0md0jzevcfssaqvc`

## Nodes

### Live require completion dogfood

- Node id: `coord-task:01kn8rr5zr0md0jzevcfssaqvc`
- Kind: `edit`
- Status: `in_progress`

## Edges

No published plan edges are currently recorded.

