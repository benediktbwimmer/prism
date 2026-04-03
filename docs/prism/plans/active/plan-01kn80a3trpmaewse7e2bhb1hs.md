# Implement the Protected State Runtime Sync design from docs/PROTECTED_STATE_RUNTIME_SYNC.md so repo-published .prism streams follow one uniform runtime import contract across bootstrap, live sync, self-write suppression, diagnostics, and recovery.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:97385919ec8d22f316949187f35063c256964f8e006332b6a89c81c1088c58a0`
- Source logical timestamp: `unknown`
- Source snapshot: `7 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn80a3trpmaewse7e2bhb1hs`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `7`
- Edges: `6`

## Goal

Implement the Protected State Runtime Sync design from docs/PROTECTED_STATE_RUNTIME_SYNC.md so repo-published .prism streams follow one uniform runtime import contract across bootstrap, live sync, self-write suppression, diagnostics, and recovery.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn80a3trpmaewse7e2bhb1hs.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kn80axazq1a4yan9avjtsv9e`
- `coord-task:01kn80b3hb260q2xqehsh73xs0`

## Nodes

### Codify the protected-state runtime contract in docs and invariants

- Node id: `coord-task:01kn80axazq1a4yan9avjtsv9e`
- Kind: `edit`
- Status: `completed`
- Summary: Add the runtime rule that repo-published .prism state is imported only by bootstrap or the dedicated protected-state sync path, never by ad hoc read-path hooks, and explicitly document why the source watcher continues to ignore .prism.

#### Acceptance

- Core docs state the single protected-state import rule and the intentional watcher split. [any]
- The plan captures TODO-PSYNC-1 and TODO-PSYNC-5 as explicit implementation expectations. [any]

#### Tags

- `docs`
- `protected-state`
- `runtime-contract`

### Replace memory-only read-path sync with one generalized protected-state sync surface

- Node id: `coord-task:01kn80b3hb260q2xqehsh73xs0`
- Kind: `edit`
- Status: `completed`
- Summary: Remove session read-path calls to sync_repo_memory_events_locked(...) and introduce a single protected-state sync entry point that bootstrap and later live sync can share without ad hoc domain exceptions.

#### Acceptance

- No read path performs memory-specific opportunistic repo import anymore. [any]
- A generalized protected-state sync surface exists for bootstrap and targeted stream sync work. [any]

#### Tags

- `memory`
- `phase-1`
- `sync-surface`

### Implement stream-oriented protected-state import handlers and runtime freshness tracking

- Node id: `coord-task:01kn80bbb2ty6xt505xtxsw4hw`
- Kind: `edit`
- Status: `completed`
- Summary: Add per-stream import handlers for memory, changes, concepts, concept relations, contracts, and published plan streams with verification, append-only decode, dedupe, targeted runtime patching, and domain freshness/materialization updates.

#### Acceptance

- Each protected stream class maps to one import handler with verification and dedupe. [any]
- Runtime freshness/materialization metadata is updated per affected domain or stream. [any]

#### Tags

- `freshness`
- `phase-1`
- `stream-handlers`

### Add a dedicated protected-state watcher while keeping the source watcher blind to .prism

- Node id: `coord-task:01kn80bmhav84s54wm0j8rt3am`
- Kind: `edit`
- Status: `completed`
- Summary: Introduce a protected-state watcher/importer for .prism/** that drives targeted protected-state sync without triggering source indexing, while preserving the normal watcher policy that intentionally ignores .prism.

#### Acceptance

- A dedicated watcher path exists for protected-state streams only. [any]
- The normal source watcher still ignores .prism and that behavior is intentional, not incidental. [any]

#### Tags

- `.prism`
- `phase-2`
- `watcher`

### Suppress self-writes and patch live runtime state directly on protected-state publishes

- Node id: `coord-task:01kn80bw4gasjmyaxhkd47x116`
- Kind: `edit`
- Status: `in_progress`
- Summary: Track per-stream self-write markers so PRISM does not react to its own repo-publish operations as upstream changes, and ensure mutation paths patch live runtime state directly instead of relying on persist-then-reload semantics.

#### Acceptance

- Protected-state publishes update suppression markers so self-authored writes do not churn back through import. [any]
- Mutation paths expose authored state directly in runtime without waiting for a later reload or watcher echo. [any]

#### Tags

- `mutation-paths`
- `phase-2`
- `self-write`

### Expose protected-state sync status, diagnostics, and fallback reconciliation

- Node id: `coord-task:01kn80c3hk4bk0kt0391289zh5`
- Kind: `edit`
- Status: `ready`
- Summary: Extend runtime status and internal developer surfaces to report per-stream sync state, watcher activity, suppressed self-writes, verified imports, and failed imports, then add a bounded reconciliation sweep that repairs missed watcher events without becoming the primary freshness mechanism.

#### Acceptance

- Runtime status and internal developer diagnostics report protected-state sync activity and freshness per stream or domain. [any]
- A bounded reconciliation sweep exists as a safety net and is not the primary freshness path. [any]

#### Tags

- `diagnostics`
- `phase-3`
- `reconciliation`

### Validate bootstrap, live-sync, self-write suppression, and missed-event recovery across all protected streams

- Node id: `coord-task:01kn80cbhehcndfs9fv21dbk4f`
- Kind: `validate`
- Status: `ready`
- Summary: Add targeted and full validation coverage for startup hydration, upstream merge simulation during a live session, uniform behavior across memory/changes/concepts/relations/contracts/plans, absence of memory-only lazy imports, correct self-write suppression, correct external watcher imports, and bounded reconciliation recovery.

#### Acceptance

- Tests cover bootstrap hydration, live upstream merge, and uniform import behavior across all protected stream classes. [validation_only]
- Tests prove read paths no longer special-case memory and that self-write suppression plus missed-event recovery behave correctly. [validation_only]

#### Tags

- `integration`
- `tests`
- `validation`

## Edges

- `plan-edge:coord-task:01kn80bbb2ty6xt505xtxsw4hw:depends-on:coord-task:01kn80b3hb260q2xqehsh73xs0`: `coord-task:01kn80bbb2ty6xt505xtxsw4hw` depends on `coord-task:01kn80b3hb260q2xqehsh73xs0`
- `plan-edge:coord-task:01kn80bmhav84s54wm0j8rt3am:depends-on:coord-task:01kn80bbb2ty6xt505xtxsw4hw`: `coord-task:01kn80bmhav84s54wm0j8rt3am` depends on `coord-task:01kn80bbb2ty6xt505xtxsw4hw`
- `plan-edge:coord-task:01kn80bw4gasjmyaxhkd47x116:depends-on:coord-task:01kn80bbb2ty6xt505xtxsw4hw`: `coord-task:01kn80bw4gasjmyaxhkd47x116` depends on `coord-task:01kn80bbb2ty6xt505xtxsw4hw`
- `plan-edge:coord-task:01kn80c3hk4bk0kt0391289zh5:depends-on:coord-task:01kn80bmhav84s54wm0j8rt3am`: `coord-task:01kn80c3hk4bk0kt0391289zh5` depends on `coord-task:01kn80bmhav84s54wm0j8rt3am`
- `plan-edge:coord-task:01kn80c3hk4bk0kt0391289zh5:depends-on:coord-task:01kn80bw4gasjmyaxhkd47x116`: `coord-task:01kn80c3hk4bk0kt0391289zh5` depends on `coord-task:01kn80bw4gasjmyaxhkd47x116`
- `plan-edge:coord-task:01kn80cbhehcndfs9fv21dbk4f:depends-on:coord-task:01kn80c3hk4bk0kt0391289zh5`: `coord-task:01kn80cbhehcndfs9fv21dbk4f` depends on `coord-task:01kn80c3hk4bk0kt0391289zh5`

