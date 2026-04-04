# Implement PRISM snapshot merge repair and auto git support

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:d79b1a515e48cbce055b59b8af657f74518fe0a8aad2635be4a5ed1764d52abf`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 6 edges, 5 overlays`

## Overview

- Plan id: `plan:01knaqm4jgp5r12ee485ramvxe`
- Status: `completed`
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

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01knaqm4jgp5r12ee485ramvxe.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01knaqmkjbz69aka2dzh8yhyke`

## Nodes

### Audit current git-support and snapshot artifact conflict classes

- Node id: `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- The implementation boundary is explicit: which files get normal merge, which get derived merge-driver handling, which require regenerate-and-sign repair, and where auto-install should hook into attach/startup flows. [any]

### Auto-install PRISM repo git support on first attach

- Node id: `coord-task:01knaqn4pzpx278wss25zqwqma`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- A repo that first attaches to PRISM gets managed git support installed automatically without requiring a separate manual command, and the hook is safe for normal arbitrary repos. [any]

### Extend merge-driver coverage for snapshot-era derived PRISM artifacts

- Node id: `coord-task:01knaqn5gdm1syjgwvjw2a0s72`
- Kind: `edit`
- Status: `completed`
- Summary: Implementing the snapshot-derived merge-driver coverage and managed git-attributes expansion for tracked snapshot outputs.

#### Acceptance

- Snapshot-era generated PRISM docs and other merge-safe derived artifacts are covered by managed git attributes and merge-driver behavior, while authoritative signed manifests remain excluded from naive text merges. [any]

### Add explicit snapshot repair and regeneration commands for merge and rebase conflicts

- Node id: `coord-task:01knaqn6kmcv0347syd3zxjvk2`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- PRISM exposes a first-class repair path that can rebuild snapshot-era manifests and derived docs/indexes from the merged snapshot state instead of requiring manual conflict surgery. [any]

### Validate and dogfood snapshot merge recovery in a real rebase scenario

- Node id: `coord-task:01knaqnk440qpjcssdta8anmv7`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- A real merge or rebase conflict in generated PRISM snapshot state can be resolved end-to-end through the supported tooling with no repo-internal code spelunking. [any]

## Edges

- `plan-edge:coord-task:01knaqn4pzpx278wss25zqwqma:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn4pzpx278wss25zqwqma` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqn5gdm1syjgwvjw2a0s72:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn5gdm1syjgwvjw2a0s72` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqn6kmcv0347syd3zxjvk2:depends-on:coord-task:01knaqmkjbz69aka2dzh8yhyke`: `coord-task:01knaqn6kmcv0347syd3zxjvk2` depends on `coord-task:01knaqmkjbz69aka2dzh8yhyke`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn4pzpx278wss25zqwqma`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn4pzpx278wss25zqwqma`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn5gdm1syjgwvjw2a0s72`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn5gdm1syjgwvjw2a0s72`
- `plan-edge:coord-task:01knaqnk440qpjcssdta8anmv7:depends-on:coord-task:01knaqn6kmcv0347syd3zxjvk2`: `coord-task:01knaqnk440qpjcssdta8anmv7` depends on `coord-task:01knaqn6kmcv0347syd3zxjvk2`

## Execution Overlays

- Node: `coord-task:01knaqmkjbz69aka2dzh8yhyke`
  git execution status: `coordination_published`
  source ref: `task/prism-merge-repair-support`
  target ref: `origin/main`
  publish ref: `task/prism-merge-repair-support`
- Node: `coord-task:01knaqn4pzpx278wss25zqwqma`
  git execution status: `coordination_published`
  source ref: `task/prism-merge-repair-support`
  target ref: `origin/main`
  publish ref: `task/prism-merge-repair-support`
- Node: `coord-task:01knaqn5gdm1syjgwvjw2a0s72`
  git execution status: `coordination_published`
  source ref: `task/prism-merge-repair-support`
  target ref: `origin/main`
  publish ref: `task/prism-merge-repair-support`
- Node: `coord-task:01knaqn6kmcv0347syd3zxjvk2`
  git execution status: `coordination_published`
  source ref: `task/prism-merge-repair-support`
  target ref: `origin/main`
  publish ref: `task/prism-merge-repair-support`
- Node: `coord-task:01knaqnk440qpjcssdta8anmv7`
  git execution status: `coordination_published`
  source ref: `task/prism-merge-repair-support`
  target ref: `origin/main`
  publish ref: `task/prism-merge-repair-support`

