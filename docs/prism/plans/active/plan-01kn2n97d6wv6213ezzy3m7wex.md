# spec-drift-improvements

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:4972ba5288b80a33dc889f49c09aab1497295fe24097d660cec71102b4d03c57`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 4 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn2n97d6wv6213ezzy3m7wex`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `4`

## Goal

spec-drift-improvements

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn2n97d6wv6213ezzy3m7wex.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn2n9bp2xgmxbq9xvj4we2b8`

## Nodes

### Inventory PRISM spec docs and define the spec-drift validation matrix

- Node id: `coord-task:01kn2n9bp2xgmxbq9xvj4we2b8`
- Kind: `edit`
- Status: `in_progress`

### Exercise spec drift on every spec doc and capture failures, gaps, and noisy outputs

- Node id: `coord-task:01kn2n9fbf4gcvq9hbx1dzzz07`
- Kind: `edit`
- Status: `ready`

### Improve spec links, target resolution, and drift explanations until spec drift is broadly useful

- Node id: `coord-task:01kn2n9jhrtr361x3t0pej23d9`
- Kind: `edit`
- Status: `ready`

### Validate spec drift end-to-end across the spec corpus and harden regressions

- Node id: `coord-task:01kn2na0msksb21hqg2321ta0k`
- Kind: `edit`
- Status: `ready`

### Update prism://instructions and supporting guidance so agents naturally reach for spec-drift analysis

- Node id: `coord-task:01kn2na4qh0vhn6h9fbdce9s9v`
- Kind: `edit`
- Status: `ready`

## Edges

- `plan-edge:coord-task:01kn2n9fbf4gcvq9hbx1dzzz07:depends-on:coord-task:01kn2n9bp2xgmxbq9xvj4we2b8`: `coord-task:01kn2n9fbf4gcvq9hbx1dzzz07` depends on `coord-task:01kn2n9bp2xgmxbq9xvj4we2b8`
- `plan-edge:coord-task:01kn2n9jhrtr361x3t0pej23d9:depends-on:coord-task:01kn2n9fbf4gcvq9hbx1dzzz07`: `coord-task:01kn2n9jhrtr361x3t0pej23d9` depends on `coord-task:01kn2n9fbf4gcvq9hbx1dzzz07`
- `plan-edge:coord-task:01kn2na0msksb21hqg2321ta0k:depends-on:coord-task:01kn2n9jhrtr361x3t0pej23d9`: `coord-task:01kn2na0msksb21hqg2321ta0k` depends on `coord-task:01kn2n9jhrtr361x3t0pej23d9`
- `plan-edge:coord-task:01kn2na4qh0vhn6h9fbdce9s9v:depends-on:coord-task:01kn2na0msksb21hqg2321ta0k`: `coord-task:01kn2na4qh0vhn6h9fbdce9s9v` depends on `coord-task:01kn2na0msksb21hqg2321ta0k`

