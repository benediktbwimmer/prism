# Simplify the post-redesign refresh model by removing misleading persisted-refresh naming, consolidating refresh ownership, and further demoting local snapshot/DB authority from steady-state serving.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:95741259142f006f110f26f80b0a58acc51dd594ff79281a2b875bd8a455cf8c`
- Source logical timestamp: `unknown`
- Source snapshot: `0 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:3`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `0`
- Edges: `0`

## Goal

Simplify the post-redesign refresh model by removing misleading persisted-refresh naming, consolidating refresh ownership, and further demoting local snapshot/DB authority from steady-state serving.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:3.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Nodes

No published plan nodes are currently recorded.

## Edges

No published plan edges are currently recorded.

