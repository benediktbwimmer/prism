# Add runtime descriptors and bounded peer reads

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:001591f96d2cfb248f770abade1bc8b6880bb41dde32060e1c8d6d4832daa7ec`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 4 overlays`

## Overview

- Plan id: `plan:01kna9h1n4tawteqc29qbp6v94`
- Status: `archived`
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

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kna9h1n4tawteqc29qbp6v94.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kna9hj9pk5nvg4max2248frc`

## Nodes

### Audit and simplify or disable the old shared-runtime versus local-runtime split in daemon startup and runtime wiring

- Node id: `coord-task:01kna9hj9pk5nvg4max2248frc`
- Kind: `edit`
- Status: `ready`

### Add shared coordination runtime descriptors for live runtime discovery and capability advertisement

- Node id: `coord-task:01kna9hjbctvt9e3j32d6cgmte`
- Kind: `edit`
- Status: `ready`

### Implement authenticated capability-scoped bounded peer runtime reads on local or trusted-network transport

- Node id: `coord-task:01kna9hjd5a38ezszy5fqr6h0d`
- Kind: `edit`
- Status: `ready`

### Validate local-machine peer observability, capability denial, and graceful fallback with no peer

- Node id: `coord-task:01kna9hjenyhw6ge6wb4eb8h7p`
- Kind: `edit`
- Status: `ready`

## Edges

- `plan-edge:coord-task:01kna9hjbctvt9e3j32d6cgmte:depends-on:coord-task:01kna9hj9pk5nvg4max2248frc`: `coord-task:01kna9hjbctvt9e3j32d6cgmte` depends on `coord-task:01kna9hj9pk5nvg4max2248frc`
- `plan-edge:coord-task:01kna9hjd5a38ezszy5fqr6h0d:depends-on:coord-task:01kna9hjbctvt9e3j32d6cgmte`: `coord-task:01kna9hjd5a38ezszy5fqr6h0d` depends on `coord-task:01kna9hjbctvt9e3j32d6cgmte`
- `plan-edge:coord-task:01kna9hjenyhw6ge6wb4eb8h7p:depends-on:coord-task:01kna9hjd5a38ezszy5fqr6h0d`: `coord-task:01kna9hjenyhw6ge6wb4eb8h7p` depends on `coord-task:01kna9hjd5a38ezszy5fqr6h0d`

## Execution Overlays

- Node: `coord-task:01kna9hj9pk5nvg4max2248frc`
- Node: `coord-task:01kna9hjbctvt9e3j32d6cgmte`
- Node: `coord-task:01kna9hjd5a38ezszy5fqr6h0d`
- Node: `coord-task:01kna9hjenyhw6ge6wb4eb8h7p`

