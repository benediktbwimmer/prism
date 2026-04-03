# plans-task-graph-node-rewrite: make coordination tasks canonical for task-execution plans, reduce task-backed plan nodes to pure projections, and preserve standalone native plan nodes for graph-native planning.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:3cf27543ed0e79658470fbcb61b06ef36e2f041d3a7db05c37137b246a5f8846`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 7 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn11fra3ddvjqmtw4qjrqax2`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `7`

## Goal

plans-task-graph-node-rewrite: make coordination tasks canonical for task-execution plans, reduce task-backed plan nodes to pure projections, and preserve standalone native plan nodes for graph-native planning.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn11fra3ddvjqmtw4qjrqax2.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn11fra3ddvjqmtw4qjrqax2.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn11fy74f41qhc9qb5grrceq`

## Nodes

### Define canonical ownership boundaries for task-execution plans versus graph-native standalone plan nodes

- Node id: `coord-task:01kn11fy74f41qhc9qb5grrceq`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- The rewrite has an explicit ownership contract: task-backed node state comes from coordination tasks, while standalone native plan nodes remain first-class only for non-task graph structure. [any]

### Refactor task-backed plan graph projection so task state is the only source of truth

- Node id: `coord-task:01kn11g3qvfcx3dtbgqpxpjjkr`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Task-backed plan nodes are computed from coordination tasks and no longer carry independently mutable duplicated state. [any]

### Route MCP coordination mutations and task-shaped query surfaces through task ownership for task-backed ids

- Node id: `coord-task:01kn11gdk11zw3kmv05j66gdxb`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- A task-backed id cannot silently mutate a separate node-state layer; shared task fields operate on the coordination task model directly. [any]

### Preserve standalone native plan nodes and graph-only authored structure for non-task plans

- Node id: `coord-task:01kn11gkbcr6dyf31n9d03qb37`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Standalone native plan nodes remain supported for authored graph structure that is not represented as claimable coordination tasks. [any]

### Align plan completion, blockers, task brief, and validation surfaces with the unified ownership model

- Node id: `coord-task:01kn11h4jf0zhf89bsjfrcty7n`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Plan-close checks, blockers, and task-brief views no longer disagree because of duplicated task-backed node state. [any]

### Validate the rewrite end-to-end and document migration rules for task-backed versus standalone plan nodes

- Node id: `coord-task:01kn11hc4p50xjgr374gqqe624`
- Kind: `edit`
- Status: `ready`

#### Acceptance

- Tests and live PRISM usage show no task-versus-node divergence for task-execution plans, and the remaining standalone-node contract is explicit. [any]

## Edges

- `plan-edge:coord-task:01kn11g3qvfcx3dtbgqpxpjjkr:depends-on:coord-task:01kn11fy74f41qhc9qb5grrceq`: `coord-task:01kn11g3qvfcx3dtbgqpxpjjkr` depends on `coord-task:01kn11fy74f41qhc9qb5grrceq`
- `plan-edge:coord-task:01kn11gdk11zw3kmv05j66gdxb:depends-on:coord-task:01kn11fy74f41qhc9qb5grrceq`: `coord-task:01kn11gdk11zw3kmv05j66gdxb` depends on `coord-task:01kn11fy74f41qhc9qb5grrceq`
- `plan-edge:coord-task:01kn11gkbcr6dyf31n9d03qb37:depends-on:coord-task:01kn11fy74f41qhc9qb5grrceq`: `coord-task:01kn11gkbcr6dyf31n9d03qb37` depends on `coord-task:01kn11fy74f41qhc9qb5grrceq`
- `plan-edge:coord-task:01kn11h4jf0zhf89bsjfrcty7n:depends-on:coord-task:01kn11g3qvfcx3dtbgqpxpjjkr`: `coord-task:01kn11h4jf0zhf89bsjfrcty7n` depends on `coord-task:01kn11g3qvfcx3dtbgqpxpjjkr`
- `plan-edge:coord-task:01kn11h4jf0zhf89bsjfrcty7n:depends-on:coord-task:01kn11gdk11zw3kmv05j66gdxb`: `coord-task:01kn11h4jf0zhf89bsjfrcty7n` depends on `coord-task:01kn11gdk11zw3kmv05j66gdxb`
- `plan-edge:coord-task:01kn11h4jf0zhf89bsjfrcty7n:depends-on:coord-task:01kn11gkbcr6dyf31n9d03qb37`: `coord-task:01kn11h4jf0zhf89bsjfrcty7n` depends on `coord-task:01kn11gkbcr6dyf31n9d03qb37`
- `plan-edge:coord-task:01kn11hc4p50xjgr374gqqe624:depends-on:coord-task:01kn11h4jf0zhf89bsjfrcty7n`: `coord-task:01kn11hc4p50xjgr374gqqe624` depends on `coord-task:01kn11h4jf0zhf89bsjfrcty7n`

