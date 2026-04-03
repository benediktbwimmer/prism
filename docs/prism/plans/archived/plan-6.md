# Finish refresh-model simplification by quarantining persisted reload, consolidating refresh ownership, and demoting DB snapshots to optional durability.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:02c211df0205c36cc60630e42e3a85cf0225eaf276d37854eae17628e6cf75af`
- Source logical timestamp: `unknown`
- Source snapshot: `3 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:6`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `3`
- Edges: `0`

## Goal

Finish refresh-model simplification by quarantining persisted reload, consolidating refresh ownership, and demoting DB snapshots to optional durability.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:6.json`
- Legacy migration log path: `.prism/plans/streams/plan:6.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:13`
- `coord-task:14`
- `coord-task:15`

## Nodes

### Quarantine persisted reload behind an explicit admin or recovery path

- Node id: `coord-task:13`
- Kind: `edit`
- Status: `completed`

### Make refresh ownership singular across watcher, runtime, and status surfaces

- Node id: `coord-task:14`
- Kind: `edit`
- Status: `completed`

### Demote DB and snapshot hydration to optional startup or durability concerns

- Node id: `coord-task:15`
- Kind: `edit`
- Status: `completed`

## Edges

No published plan edges are currently recorded.

