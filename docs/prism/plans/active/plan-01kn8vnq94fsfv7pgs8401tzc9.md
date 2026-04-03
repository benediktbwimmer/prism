# Measure coordination mutation latency after hot-path cuts

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:154cdcceba260ad9de156f1baeed0e18d659d35c130058496d928663ac585a33`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 0 overlays`

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

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn8vnq94fsfv7pgs8401tzc9.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn8vnq94fsfv7pgs8401tzc9.jsonl` (compatibility only, not current tracked authority)

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

