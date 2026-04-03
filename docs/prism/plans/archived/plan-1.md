# Migrate PRISM persistence toward authoritative runtime storage for memories, concepts, plans, and coordination state while preserving a three-plane model: repo-published truth in `.prism`, shared mutable runtime state in local or remote backends, and process-local ephemeral caches. Make local embedded and shared remote runtime backends first-class deployment modes, support one Prism endpoint multiplexing multiple repo/worktree contexts through explicit repo, worktree, branch, session, and instance identity, and keep snapshots plus plan compaction artifacts derived-only while native plan graphs, bindings, hydration, rebinding, compatibility projection, and multi-context coordination become explicit persistence concerns.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:9ec929b64d46b387bafcce727b560b39bb841bbec4fbaa65ebeaf017023364bf`
- Source logical timestamp: `unknown`
- Source snapshot: `7 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:1`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `7`
- Edges: `6`

## Goal

Migrate PRISM persistence toward authoritative runtime storage for memories, concepts, plans, and coordination state while preserving a three-plane model: repo-published truth in `.prism`, shared mutable runtime state in local or remote backends, and process-local ephemeral caches. Make local embedded and shared remote runtime backends first-class deployment modes, support one Prism endpoint multiplexing multiple repo/worktree contexts through explicit repo, worktree, branch, session, and instance identity, and keep snapshots plus plan compaction artifacts derived-only while native plan graphs, bindings, hydration, rebinding, compatibility projection, and multi-context coordination become explicit persistence concerns.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:1.jsonl`

## Root Nodes

- `coord-task:1`

## Nodes

### Classify authoritative state vs derived artifacts across persistence, plans, and projections

- Node id: `coord-task:1`
- Kind: `edit`
- Status: `completed`
- Summary: Define what is authoritative runtime truth versus derived or export-only state, including native plan graphs, authored node and edge state, execution overlays, compatibility projections, summaries, and snapshot artifacts.

#### Acceptance

- Persistence classification explicitly marks native plan graph data, authored node fields, authored edges, execution overlays, and stable published refs as authoritative state where appropriate. [any]
- The classification explicitly marks compatibility task projections, plan summaries and recommendations, snapshots, compaction artifacts, and other caches or exports as derived state. [any]
- The remaining persistence-migration tasks can reference this classification to avoid reintroducing dual truth between native plans and compatibility projections. [any]

### Make native plan and coordination writes authoritative via incremental DB-native persistence

- Node id: `coord-task:2`
- Kind: `edit`
- Status: `completed`
- Summary: Replace snapshot-shaped write authority with incremental native writes so plan, node, edge, and continuity mutations persist through authoritative revisioned event-backed storage, carry explicit repo/worktree/branch/session/instance identity where scope depends on it, and remain suitable for both embedded and shared remote backends.

#### Acceptance

- Native plan, node, edge, and continuity mutations persist through an authoritative DB-native write path rather than through snapshot-shaped rewrites. [any]
- The authoritative runtime write path supports revision-aware or compare-and-swap semantics and idempotent event handling suitable for multi-instance coordination. [any]
- Authoritative writes record or preserve explicit repo, worktree, branch or checkout, session, and instance identity where runtime scoping depends on them. [any]
- Compatibility task or coordination projections remain import or export compatibility views and are not treated as the authoritative write truth. [any]

### Move outcomes, projections, and plan read models onto incremental maintenance

- Node id: `coord-task:3`
- Kind: `edit`
- Status: `completed`
- Summary: Maintain outcomes, compatibility projections, plan summaries, recommendation frontiers, and shared runtime read models incrementally from authoritative events so one endpoint can serve many worktree contexts without relying on whole-snapshot rebuilds.

#### Acceptance

- Outcome history, compatibility projections, and plan-native read models such as summaries or next recommendations are maintained incrementally from authoritative events. [any]
- Shared runtime read models cover active claims, handoffs, maintenance queues, live plan materializations, and worktree-bound coordination state without requiring whole-snapshot rebuilds after each mutation or reload. [any]
- Read models support one endpoint serving many worktree contexts without leaking branch or worktree state across bound sessions. [any]
- Projection maintenance clearly distinguishes authoritative event streams from derived shared-runtime read models, exported artifacts, and process-local caches. [any]

### Refactor WorkspaceSession hydration around incremental loads, binding hydration, and repo-motion recovery

- Node id: `coord-task:4`
- Kind: `edit`
- Status: `completed`
- Summary: Hydrate native plan graphs and continuity state incrementally, then rebind plan bindings and attach runtime overlays with explicit repo/worktree/branch identity so one Prism endpoint can host many worktree contexts cleanly.

#### Acceptance

- Workspace and session hydration rebuild native plan graphs, continuity state, and execution overlays incrementally rather than requiring whole-Prism snapshot reconstruction. [any]
- Plan binding hydration follows authored anchor, then lineage, then concept-centered recovery, then explicit unresolved handling during reload or repo motion. [any]
- Runtime hydration preserves cross-instance session continuity and keeps repo, branch, and worktree identity explicit so mutable runtime state does not bleed across unrelated workspaces. [any]
- Sessions can bind explicitly to a worktree context so one endpoint can host many worktrees without requiring one daemon per worktree. [any]

### Demote snapshots to compatibility caches, bootstrap accelerators, export artifacts, and plan compaction outputs

- Node id: `coord-task:5`
- Kind: `edit`
- Status: `completed`
- Summary: Keep replay authoritative while allowing deterministic per-plan snapshots and compaction artifacts derived from canonical event history so snapshots remain secondary acceleration and export mechanisms rather than runtime truth.

#### Acceptance

- Deterministic per-plan snapshots or compaction artifacts are derived from canonical event history rather than acting as a separate source of truth. [any]
- Snapshots serve compatibility caching, bootstrap acceleration, and export use cases without regaining authority over native runtime plan state. [any]
- Snapshot and compaction flows preserve the native plan information needed for reload and inspection while remaining explicitly secondary to replay. [any]

### Validate reload latency, concurrency behavior, migration safety, and native plan persistence correctness

- Node id: `coord-task:6`
- Kind: `edit`
- Status: `ready`
- Summary: Validate that the migrated persistence model preserves correctness for native plan graphs, binding hydration, compatibility projections, shared-runtime coordination, and single-endpoint multi-worktree isolation while keeping latency and migration risks acceptable.

#### Acceptance

- Validation covers reload latency and concurrency behavior for native plan graphs, continuity state, leases, and incremental hydration paths across more than one runtime instance and more than one worktree context. [any]
- Migration checks prove native plan persistence, compatibility projections, binding hydration, stale-session cleanup, and worktree context isolation remain correct across reloads, upgrades, partial failures, and reconnects. [any]
- The final validation pass exercises optimistic concurrency, lease or heartbeat renewal, idempotent event replay, single-endpoint multi-worktree binding, and repo or branch isolation without assuming snapshot authority or SQLite-specific orchestration. [any]

### Introduce backend-neutral persistence and session abstractions for native plans and coordination state

- Node id: `coord-task:7`
- Kind: `edit`
- Status: `completed`
- Summary: Create backend-neutral persistence and session interfaces that cover native plan event storage, snapshot loading, coordination continuity, and binding hydration so SQLite becomes one implementation rather than the architectural center.

#### Acceptance

- Shared persistence and session contracts explicitly distinguish repo-published truth in `.prism`, shared mutable runtime state in the backend, and process-local ephemeral caches. [any]
- Backend-neutral interfaces cover native plan event storage, coordination continuity state, runtime overlays, and optional snapshot or compaction loaders without SQLite-specific assumptions. [any]
- A future Postgres backend can implement the same shared-runtime contracts for plans, memories, concepts, claims, handoffs, and coordination state without another architectural rewrite. [any]

## Edges

- `plan-edge:coord-task:2:depends-on:coord-task:7`: `coord-task:2` depends on `coord-task:7`
- `plan-edge:coord-task:3:depends-on:coord-task:2`: `coord-task:3` depends on `coord-task:2`
- `plan-edge:coord-task:4:depends-on:coord-task:3`: `coord-task:4` depends on `coord-task:3`
- `plan-edge:coord-task:5:depends-on:coord-task:4`: `coord-task:5` depends on `coord-task:4`
- `plan-edge:coord-task:6:depends-on:coord-task:5`: `coord-task:6` depends on `coord-task:5`
- `plan-edge:coord-task:7:depends-on:coord-task:1`: `coord-task:7` depends on `coord-task:1`

