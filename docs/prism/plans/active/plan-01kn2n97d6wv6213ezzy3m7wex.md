# spec-drift-improvements

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:a086e2b4667aaa64e825fe4a3d8af26857f926f6178b9b72767cf8fc6b8f1c41`
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

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn2n97d6wv6213ezzy3m7wex.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn2n97d6wv6213ezzy3m7wex.jsonl` (compatibility only, not current tracked authority)

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

