# Measure coordination mutation latency after hot-path cuts

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:1ec6517b9bb9c8a60d659d922aaf72a6cb4994a6ecb795b23247bf490050b307`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 1 overlays`

## Overview

- Plan id: `plan:01kn8vnq94fsfv7pgs8401tzc9`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Capture a live coordination mutation trace after removing auth writes, UI hot-path work, and synchronous plan projection regeneration.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn8vnq94fsfv7pgs8401tzc9.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn8vptjxtbw2tnffjg5gsf9e`

## Nodes

### Warm follow-up coordination mutation

- Node id: `coord-task:01kn8vptjxtbw2tnffjg5gsf9e`
- Kind: `edit`
- Status: `ready`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Produces a second warm trace [any]

## Edges

No published plan edges are currently recorded.

## Execution Overlays

- Node: `coord-task:01kn8vptjxtbw2tnffjg5gsf9e`

