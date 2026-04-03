# Eliminate nondeterministic SQLite locking failures from the full parallel workspace test suite

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:b4062f4dd77ad911d79cd2e32609b6cdf61c77e3b1aa43b3b0456bf9c31878b0`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn5xz844mc22fb9g7g5czxw7`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Eliminate nondeterministic SQLite locking failures from the full parallel workspace test suite

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn5xz844mc22fb9g7g5czxw7.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn5xz844mc22fb9g7g5czxw7.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn5xzdb5p8pyyx0kxjpx0epx`

## Nodes

### Stabilize full-workspace parallel tests against SQLite locking flakes

- Node id: `coord-task:01kn5xzdb5p8pyyx0kxjpx0epx`
- Kind: `edit`
- Status: `completed`
- Summary: Centralized SQLite write retries and immediate transactions across the store, moved malformed unresolved-row scrubbing off the graph read path, and validated with repeated full workspace runs.
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Bindings

- Anchor: `file:242`
- Anchor: `file:243`
- Anchor: `file:316`
- Anchor: `file:406`

#### Acceptance

- Full workspace test suite runs deterministically in parallel without transient SQLite lock failures [any]
- Lock handling is centralized in shared runtime/store paths rather than ad hoc test-local retries [any]

## Edges

No published plan edges are currently recorded.

