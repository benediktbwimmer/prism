# Add runtime descriptors and bounded peer reads

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7e013764d8a20f9cb9d3280751d4c5c238399632df04cb550fa937733174ef35`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 4 overlays`

## Overview

- Plan id: `plan:01kna8sczaf3vhy8q6erq4fdvr`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `3`

## Goal

Add shared-ref runtime descriptors and authenticated peer runtime reads, while simplifying or disabling the old shared-runtime versus local-runtime split wherever it is no longer needed.

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
- Snapshot plan shard: `.prism/state/plans/plan:01kna8sczaf3vhy8q6erq4fdvr.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kna8tgf7s523e5edfv0gc0v9`

## Nodes

### Audit and simplify or disable the old shared-runtime versus local-runtime split in daemon startup and runtime wiring

- Node id: `coord-task:01kna8tgf7s523e5edfv0gc0v9`
- Kind: `edit`
- Status: `completed`

### Add shared coordination runtime descriptors for live runtime discovery and capability advertisement

- Node id: `coord-task:01kna8tv5khkd6ypdg9q6dfs87`
- Kind: `edit`
- Status: `completed`

### Implement authenticated capability-scoped bounded peer runtime reads on local or trusted-network transport

- Node id: `coord-task:01kna8v02c0p53pdfckcyq0vaw`
- Kind: `edit`
- Status: `completed`

### Validate local-machine peer observability, capability denial, and graceful fallback with no peer

- Node id: `coord-task:01kna8v52e247h2xymfv8ksdb0`
- Kind: `edit`
- Status: `completed`

## Edges

- `plan-edge:coord-task:01kna8tv5khkd6ypdg9q6dfs87:depends-on:coord-task:01kna8tgf7s523e5edfv0gc0v9`: `coord-task:01kna8tv5khkd6ypdg9q6dfs87` depends on `coord-task:01kna8tgf7s523e5edfv0gc0v9`
- `plan-edge:coord-task:01kna8v02c0p53pdfckcyq0vaw:depends-on:coord-task:01kna8tv5khkd6ypdg9q6dfs87`: `coord-task:01kna8v02c0p53pdfckcyq0vaw` depends on `coord-task:01kna8tv5khkd6ypdg9q6dfs87`
- `plan-edge:coord-task:01kna8v52e247h2xymfv8ksdb0:depends-on:coord-task:01kna8v02c0p53pdfckcyq0vaw`: `coord-task:01kna8v52e247h2xymfv8ksdb0` depends on `coord-task:01kna8v02c0p53pdfckcyq0vaw`

## Execution Overlays

- Node: `coord-task:01kna8tgf7s523e5edfv0gc0v9`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation`
- Node: `coord-task:01kna8tv5khkd6ypdg9q6dfs87`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation`
- Node: `coord-task:01kna8v02c0p53pdfckcyq0vaw`
  git execution status: `publish_pending`
  pending task status: `completed`
  source ref: `task/federated-runtime-implementation`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation`
- Node: `coord-task:01kna8v52e247h2xymfv8ksdb0`
  git execution status: `coordination_published`
  source ref: `task/federated-runtime-implementation`
  target ref: `origin/main`
  publish ref: `task/federated-runtime-implementation`

