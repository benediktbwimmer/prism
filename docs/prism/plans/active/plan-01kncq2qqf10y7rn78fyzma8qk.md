# Implement agent task affinity execution

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:50de3a95f645ace62751e71c219f42034947e7281c9e466f2fcb4f52abf90fa7`
- Source logical timestamp: `unknown`
- Source snapshot: `1 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kncq2qqf10y7rn78fyzma8qk`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `1`
- Edges: `0`

## Goal

Implement decentralized task-affinity execution with local fitness scoring, soft continuation reservations, explainable selection, and shared coordination/query surfaces for warm execution chains.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present
- Local hot cache: shared-runtime SQLite startup checkpoint and hydrated in-memory runtime
- Branch-local tracked `.prism/state/plans/**` export: disabled; plans no longer mirror into tracked repo snapshot state
- Manual markdown export path: `docs/prism/plans/**` only when `sync_prism_doc` or `repair-snapshot-artifacts` is invoked explicitly

## Root Nodes

- `coord-task:01kncq3sm4dazb6xfqsfz8vgex`

## Nodes

### Validate starvation guards, reservation decay, and warm-chain behavior end to end

- Node id: `coord-task:01kncq3sm4dazb6xfqsfz8vgex`
- Kind: `validate`
- Status: `ready`
- Summary: Dogfood the new execution model across multiple worktrees and prove that short useful continuity improves pull selection without causing hoarding or starvation.

#### Acceptance

- Tests and dogfooding cover TTL expiry, stale reservation takeover, bounded reservation depth, and urgent-work preemption. [any]
- Observed pull-selection behavior is explainable through the exposed coordination and query surfaces. [any]

## Edges

No published plan edges are currently recorded.

