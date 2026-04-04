# Publish always-up-to-date human-readable markdown projections under docs/prism for all persisted .prism state surfaces, with a clear hierarchy for plans, memories, concepts, contracts, relations, and other current repo-state projections.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:59288def8aad039ec0c3c5ba2d4748a4182a5e63ed907ebb3c9d55a16e04fb24`
- Source logical timestamp: `unknown`
- Source snapshot: `3 nodes, 0 edges, 3 overlays`

## Overview

- Plan id: `plan:01kn89108wb4a78p35krmyn29t`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `3`
- Edges: `0`

## Goal

Publish always-up-to-date human-readable markdown projections under docs/prism for all persisted .prism state surfaces, with a clear hierarchy for plans, memories, concepts, contracts, relations, and other current repo-state projections.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn89108wb4a78p35krmyn29t.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn891asb951r6bdzws74dbtv`
- `coord-task:01kn891bw81xnx9tddb0vb99t7`
- `coord-task:01kn891d13gpz3snkn2k5qfww2`

## Nodes

### Define the docs/prism projection hierarchy and coverage map

- Node id: `coord-task:01kn891asb951r6bdzws74dbtv`
- Kind: `edit`
- Status: `in_progress`

#### Acceptance

- Implementation targets for projection generation and refresh are identified [any]
- Projection hierarchy is chosen and mapped to the persisted .prism state families that should publish into docs/prism [any]

### Implement generated markdown projections for persisted .prism state

- Node id: `coord-task:01kn891bw81xnx9tddb0vb99t7`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Projection generator emits human-readable markdown files under docs/prism for current persisted state families [any]
- Published docs update deterministically from repo state [any]

### Validate regenerated projections and refresh live PRISM surfaces

- Node id: `coord-task:01kn891d13gpz3snkn2k5qfww2`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Full cargo test passes or flakes are isolated per repo policy [any]
- Release binaries are rebuilt and the MCP daemon is restarted if PRISM behavior changed [any]
- Targeted validation passes for the projection implementation [any]

## Edges

No published plan edges are currently recorded.

## Execution Overlays

- Node: `coord-task:01kn891asb951r6bdzws74dbtv`
- Node: `coord-task:01kn891bw81xnx9tddb0vb99t7`
- Node: `coord-task:01kn891d13gpz3snkn2k5qfww2`

