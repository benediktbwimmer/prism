# Extend PRISM runtime and MCP log query views so agents can analyze daemon and MCP call state at worktree scope, repo-wide across all worktrees for the same repo on one machine, and across all available log files, while preserving current worktree-local behavior as the default.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:c9d3949ec3daa26644a23b0895122d97f26712d61e4ce2697893e8f69fbaed0d`
- Source logical timestamp: `unknown`
- Source snapshot: `3 nodes, 2 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn82nwqnv42m6fw33pgrabss`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `3`
- Edges: `2`

## Goal

Extend PRISM runtime and MCP log query views so agents can analyze daemon and MCP call state at worktree scope, repo-wide across all worktrees for the same repo on one machine, and across all available log files, while preserving current worktree-local behavior as the default.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn82nwqnv42m6fw33pgrabss.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn82papt611bh6q661hx7b6e`

## Nodes

### Add scope-aware runtime and MCP log backends

- Node id: `coord-task:01kn82papt611bh6q661hx7b6e`
- Kind: `edit`
- Status: `completed`
- Summary: Implementing scope-aware selection and aggregation for daemon and MCP call logs.
- Assignee: `codex-b-fresh`

#### Acceptance

- Runtime and MCP log readers can resolve worktree, repo, and all-log scopes without changing the default current-worktree behavior. [any]
- The implementation can distinguish per-worktree files and per-instance MCP records when aggregating results. [any]

### Expose scoped query options and add coverage

- Node id: `coord-task:01kn82pbhvpkb21qpmpwjb046k`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-b-fresh`

#### Acceptance

- Existing callers keep current behavior when they omit the new options. [any]
- The query surface exposes scope controls for runtime and MCP log views and tests cover worktree, repo, and all scopes. [any]

### Run repo validation for scoped log queries

- Node id: `coord-task:01kn82pcbg8whg9ecy6wnkhxex`
- Kind: `edit`
- Status: `completed`
- Assignee: `codex-b-fresh`

#### Acceptance

- Targeted tests for scoped log queries pass. [any]
- The full workspace cargo test suite passes, or any flakes are isolated and rerun successfully. [any]

## Edges

- `plan-edge:coord-task:01kn82pbhvpkb21qpmpwjb046k:depends-on:coord-task:01kn82papt611bh6q661hx7b6e`: `coord-task:01kn82pbhvpkb21qpmpwjb046k` depends on `coord-task:01kn82papt611bh6q661hx7b6e`
- `plan-edge:coord-task:01kn82pcbg8whg9ecy6wnkhxex:depends-on:coord-task:01kn82pbhvpkb21qpmpwjb046k`: `coord-task:01kn82pcbg8whg9ecy6wnkhxex` depends on `coord-task:01kn82pbhvpkb21qpmpwjb046k`

