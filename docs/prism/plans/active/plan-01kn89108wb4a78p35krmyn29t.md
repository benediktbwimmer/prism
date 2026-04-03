# Publish always-up-to-date human-readable markdown projections under docs/prism for all persisted .prism state surfaces, with a clear hierarchy for plans, memories, concepts, contracts, relations, and other current repo-state projections.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:36c15bff2dfc743729bdace5d310020bcb18da83c2425bca8c98e88ff802951e`
- Source logical timestamp: `unknown`
- Source snapshot: `3 nodes, 0 edges, 0 overlays`

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

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn89108wb4a78p35krmyn29t.jsonl`

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

