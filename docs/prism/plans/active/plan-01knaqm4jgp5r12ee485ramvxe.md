# Implement PRISM snapshot merge repair and auto git support

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:170b1b3125a2b70fe06c0eb3c7154990dce9c6e844e761ee7d9d444aaa680115`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01knaqm4jgp5r12ee485ramvxe`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `6`

## Goal

Make snapshot-era PRISM state survivable in arbitrary repos by auto-installing repo git support on first attach, extending merge-driver coverage to tracked snapshot-era derived artifacts, and adding explicit repair/regeneration workflows for manifests and other generated PRISM files that cannot be safely hand-merged.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01knaqm4jgp5r12ee485ramvxe.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01knaqmkjbz69aka2dzh8yhyke`

## Nodes

### Audit current git-support and snapshot artifact conflict classes

- Node id: `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- Kind: `edit`
- Status: `in_progress`

#### Acceptance

- The implementation boundary is explicit: which files get normal merge, which get derived merge-driver handling, which require regenerate-and-sign repair, and where auto-install should hook into attach/startup flows. [any]

### Auto-install PRISM repo git support on first attach

- Node id: `coord-task:01knaqn4pzpx278wss25zqwqma`
- Kind: `edit`
- Status: `proposed`

#### Acceptance

- A repo that first attaches to PRISM gets managed git support installed automatically without requiring a separate manual command, and the hook is safe for normal arbitrary repos. [any]

### Extend merge-driver coverage for snapshot-era derived PRISM artifacts

- Node id: `coord-task:01knaqn5gdm1syjgwvjw2a0s72`
- Kind: `edit`
- Status: `proposed`

#### Acceptance

- Snapshot-era generated PRISM docs and other merge-safe derived artifacts are covered by managed git attributes and merge-driver behavior, while authoritative signed manifests remain excluded from naive text merges. [any]

### Add explicit snapshot repair and regeneration commands for merge and rebase conflicts

- Node id: `coord-task:01knaqn6kmcv0347syd3zxjvk2`
- Kind: `edit`
- Status: `proposed`

#### Acceptance

- PRISM exposes a first-class repair path that can rebuild snapshot-era manifests and derived docs/indexes from the merged snapshot state instead of requiring manual conflict surgery. [any]

### Validate and dogfood snapshot merge recovery in a real rebase scenario

- Node id: `coord-task:01knaqnk440qpjcssdta8anmv7`
- Kind: `edit`
- Status: `proposed`

#### Acceptance

- A real merge or rebase conflict in generated PRISM snapshot state can be resolved end-to-end through the supported tooling with no repo-internal code spelunking. [any]

## Edges

- `plan-edge:coord-task:01knaqn4pzpx278wss25zqwqma:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn4pzpx278wss25zqwqma` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqn5gdm1syjgwvjw2a0s72:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn5gdm1syjgwvjw2a0s72` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqn6kmcv0347syd3zxjvk2:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn6kmcv0347syd3zxjvk2` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn4pzpx278wss25zqwqma`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn4pzpx278wss25zqwqma`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn5gdm1syjgwvjw2a0s72`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn5gdm1syjgwvjw2a0s72`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn6kmcv0347syd3zxjvk2`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn6kmcv0347syd3zxjvk2`

