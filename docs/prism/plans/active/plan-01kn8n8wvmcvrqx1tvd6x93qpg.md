# Finish portfolio dispatch with cross-plan dependencies and a ranked actionable inbox

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7b12e379b7fa731e769e8f6272482b81388fe24b9ec660b347319fbe678ff370`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn8n8wvmcvrqx1tvd6x93qpg`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `5`

## Goal

Complete the portfolio dispatch feature by modeling cross-plan dependencies explicitly, ranking actionable work across plans with explainable scoring, and exposing a practical inbox/next-work surface for agents.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn8n8wvmcvrqx1tvd6x93qpg.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`

## Nodes

### Define the cross-plan dependency model and dispatch inputs

- Node id: `coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`
- Kind: `investigate`
- Status: `ready`
- Priority: `80`

#### Acceptance

- The portfolio scheduler inputs and inter-plan relation semantics are concrete and bounded. [any]

### Implement cross-plan relations and blocking semantics

- Node id: `coord-task:01kn8na0axebh607nk06be0hkg`
- Kind: `edit`
- Status: `ready`
- Priority: `78`

#### Acceptance

- Plans can express the needed hard and soft relations for portfolio scheduling. [any]

### Implement a ranked portfolio inbox and next-work query

- Node id: `coord-task:01kn8na1d7v4knswb3jmp21p7m`
- Kind: `edit`
- Status: `ready`
- Priority: `82`

#### Acceptance

- PRISM can return a ranked set of actionable work items across active plans instead of a single-plan-local next step. [any]

### Expose score breakdowns and explanations in read surfaces

- Node id: `coord-task:01kn8na2fnmy9efdqc2g57t9nn`
- Kind: `edit`
- Status: `ready`
- Priority: `76`

#### Acceptance

- Agents can inspect why one work item outranks another from normal PRISM outputs. [any]

### Dogfood the portfolio inbox against concurrent active plans

- Node id: `coord-task:01kn8na3jdcx78a3t4n9htmhx5`
- Kind: `validate`
- Status: `ready`
- Priority: `74`

#### Acceptance

- The ranking behaves sanely across real active plans and produces actionable next picks. [any]

## Edges

- `plan-edge:coord-task:01kn8na0axebh607nk06be0hkg:depends-on:coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`: `coord-task:01kn8na0axebh607nk06be0hkg` depends on `coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`
- `plan-edge:coord-task:01kn8na1d7v4knswb3jmp21p7m:depends-on:coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`: `coord-task:01kn8na1d7v4knswb3jmp21p7m` depends on `coord-task:01kn8n9z7yxmpw7fnhzz8fchjj`
- `plan-edge:coord-task:01kn8na2fnmy9efdqc2g57t9nn:depends-on:coord-task:01kn8na0axebh607nk06be0hkg`: `coord-task:01kn8na2fnmy9efdqc2g57t9nn` depends on `coord-task:01kn8na0axebh607nk06be0hkg`
- `plan-edge:coord-task:01kn8na2fnmy9efdqc2g57t9nn:depends-on:coord-task:01kn8na1d7v4knswb3jmp21p7m`: `coord-task:01kn8na2fnmy9efdqc2g57t9nn` depends on `coord-task:01kn8na1d7v4knswb3jmp21p7m`
- `plan-edge:coord-task:01kn8na3jdcx78a3t4n9htmhx5:depends-on:coord-task:01kn8na2fnmy9efdqc2g57t9nn`: `coord-task:01kn8na3jdcx78a3t4n9htmhx5` depends on `coord-task:01kn8na2fnmy9efdqc2g57t9nn`

