# Close all remaining shared coordination refs gaps

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:bb68a7c19044beb176f72426a770c585d0a041df8e072feeaea1d3d0f69c6eb0`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 1 overlays`

## Overview

- Plan id: `plan:01knap36apc42jgrq3w7k300yz`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Finish every material remaining gap from PRISM_SHARED_COORDINATION_REFS.md by completing target integration policy and lifecycle modeling, durable integration evidence and verification, shared-ref publication ordering for strict require, remote live-sync and reconciliation hardening, branch-local mirror demotion, and retention/compaction so cross-branch coordination becomes the unambiguous primary authority for daily development.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01knap36apc42jgrq3w7k300yz.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01knaqpp6mxkybk88n72frrq4p`

## Nodes

### Implement durable target integration lifecycle and verification

- Node id: `coord-task:01knaqpp6mxkybk88n72frrq4p`
- Kind: `edit`
- Status: `in_progress`
- Summary: Wire shared-coordination task execution beyond branch publication by persisting target integration lifecycle, landing evidence, and verification rules for merge, rebase, and squash flows.

#### Acceptance

- Completion and follow-up flows persist target integration policy context and do not collapse branch publication into verified target integration. [any]
- Shared coordination task git-execution state can represent integration_pending, integration_in_progress, integrated_to_target, and integration_failed with durable evidence fields. [any]
- Targeted tests cover normal reachability-based integration and evidence-gated rebase or squash verification. [any]

## Edges

No published plan edges are currently recorded.

## Execution Overlays

- Node: `coord-task:01knaqpp6mxkybk88n72frrq4p`

