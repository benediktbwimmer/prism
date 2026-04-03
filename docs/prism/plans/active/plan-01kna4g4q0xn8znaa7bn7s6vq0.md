# Finish removing legacy tracked .prism append logs

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:b72b1fe406da62be8e26139afa2ff706a78a2b9563d08d77c646d62c6f847b7f`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 1 overlays`

## Overview

- Plan id: `plan:01kna4g4q0xn8znaa7bn7s6vq0`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Make tracked .prism/state the only repo-published authority by removing the remaining tracked legacy append-log surfaces for memory, concepts, contracts, and plans; keep append-only history in shared runtime only; and preserve cold-clone restore, manifest trust semantics, and startup performance.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kna4g4q0xn8znaa7bn7s6vq0.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kna4g9fyxzpyykde9s9z1m1d`

## Nodes

### Lock the final no-legacy-append-log authority contract

- Node id: `coord-task:01kna4g9fyxzpyykde9s9z1m1d`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

## Edges

No published plan edges are currently recorded.

## Execution Overlays

- Node: `coord-task:01kna4g9fyxzpyykde9s9z1m1d`
  git execution status: `publish_pending`
  pending task status: `completed`
  source ref: `task/remove-legacy-prism-logs`
  target ref: `origin/main`
  publish ref: `task/remove-legacy-prism-logs`

