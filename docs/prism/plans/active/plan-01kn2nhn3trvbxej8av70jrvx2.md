# first-class-plans-spec-alignment

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:6ff24dad2f7bfe56bcbd0a3cea66d05d93573e8130dfa3a9790d11e1b85093e3`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 4 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn2nhn3trvbxej8av70jrvx2`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `4`

## Goal

first-class-plans-spec-alignment

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn2nhn3trvbxej8av70jrvx2.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn2nhtq3a0h7aym135p3gxe4`

## Nodes

### Align plan status and lifecycle semantics with the first-class plans spec

- Node id: `coord-task:01kn2nhtq3a0h7aym135p3gxe4`
- Kind: `edit`
- Status: `completed`

### Implement first-class archive semantics in mutation, persistence, and hydration paths

- Node id: `coord-task:01kn2nhy59rebs6sz3xs7rzgc8`
- Kind: `edit`
- Status: `completed`

### Make completion and blocker surfaces explain spec-level gating in structured form

- Node id: `coord-task:01kn2nj34195h92ky0qzxz009y`
- Kind: `edit`
- Status: `completed`

### Align MCP schemas, vocab, and query surfaces with the full first-class plans contract

- Node id: `coord-task:01kn2nj66tmkgxqtv0f02fy1qc`
- Kind: `edit`
- Status: `completed`

### Validate end-to-end spec alignment and update the published plans docs to match the shipped behavior

- Node id: `coord-task:01kn2nj9rnhx2smh3c0w2van6s`
- Kind: `edit`
- Status: `completed`

## Edges

- `plan-edge:coord-task:01kn2nhy59rebs6sz3xs7rzgc8:depends-on:coord-task:01kn2nhtq3a0h7aym135p3gxe4`: `coord-task:01kn2nhy59rebs6sz3xs7rzgc8` depends on `coord-task:01kn2nhtq3a0h7aym135p3gxe4`
- `plan-edge:coord-task:01kn2nj34195h92ky0qzxz009y:depends-on:coord-task:01kn2nhy59rebs6sz3xs7rzgc8`: `coord-task:01kn2nj34195h92ky0qzxz009y` depends on `coord-task:01kn2nhy59rebs6sz3xs7rzgc8`
- `plan-edge:coord-task:01kn2nj66tmkgxqtv0f02fy1qc:depends-on:coord-task:01kn2nj34195h92ky0qzxz009y`: `coord-task:01kn2nj66tmkgxqtv0f02fy1qc` depends on `coord-task:01kn2nj34195h92ky0qzxz009y`
- `plan-edge:coord-task:01kn2nj9rnhx2smh3c0w2van6s:depends-on:coord-task:01kn2nj66tmkgxqtv0f02fy1qc`: `coord-task:01kn2nj9rnhx2smh3c0w2van6s` depends on `coord-task:01kn2nj66tmkgxqtv0f02fy1qc`

