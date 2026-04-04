# persistence-migration: migrate PRISM to a memory-authoritative runtime with synchronous authoritative journals, asynchronous checkpoint/materialization persistence, bounded hot state, and explicit cold-query semantics; finish by reviewing with the user whether a separate persistence worker deserves a follow-up plan.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:186660f61d03a61e903bf69fe9b9d061df06369c3f98e6c7f24bb42e83c7b91a`
- Source logical timestamp: `unknown`
- Source snapshot: `10 nodes, 15 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn13pqvcx3xmnkrs493ff3ra`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `10`
- Edges: `15`

## Goal

persistence-migration: migrate PRISM to a memory-authoritative runtime with synchronous authoritative journals, asynchronous checkpoint/materialization persistence, bounded hot state, and explicit cold-query semantics; finish by reviewing with the user whether a separate persistence worker deserves a follow-up plan.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn13pqvcx3xmnkrs493ff3ra.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn13pz030s4q1c3qrbbg203f`

## Nodes

### Specify authoritative-vs-derived persistence boundaries and crash semantics for every persisted domain

- Node id: `coord-task:01kn13pz030s4q1c3qrbbg203f`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Each persisted domain has an explicit durability class, crash guarantee, source-of-truth owner, rebuild path, and allowed read surfaces. [any]
- The migration rules align with docs/PERSISTENCE_STATE_CLASSIFICATION.md and do not leave any snapshot or compatibility view as implicit semantic authority. [any]

### Design backend-neutral persistence interfaces that separate synchronous journal appends from asynchronous checkpoints and materialized views

- Node id: `coord-task:01kn13q61f8j93je9vws9kdqjy`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- The interface shape remains compatible with local SQLite first and future shared-runtime backends. [any]
- The store/runtime boundary has explicit interfaces for authoritative journal writes, checkpoint reads/writes, and cold-query backends instead of one snapshot-shaped Store authority. [any]

### Refactor WorkspaceSession, watch, and runtime plumbing so live in-memory state is authoritative and no core request path depends on SqliteStore as the semantic source of truth

- Node id: `coord-task:01kn13qdfw63hwrr79cqm7rqs3`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Concrete SqliteStore coupling is reduced enough that a memory-authoritative runtime can evolve without rewriting every request-path consumer. [any]
- Core runtime/session/watch code no longer treats SqliteStore as the canonical semantic state holder for live operations. [any]

### Implement synchronous authoritative journals for coordination continuity and other crash-sensitive workflow state

- Node id: `coord-task:01kn13qj69y75rtwrbw6ewqzw5`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Plan-close, blocker, and coordination mutation semantics no longer depend on async snapshot persistence for correctness. [any]
- Plans, tasks, claims, artifacts, reviews, handoffs, and other policy-relevant workflow continuity survive crashes through a minimal synchronous durable write path. [any]

### Implement synchronous authoritative journals for lineage/history deltas and authored memory and outcome events

- Node id: `coord-task:01kn13qsapc9fjk72wvth681yf`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Hydrated snapshots remain accelerators only; the event/journal layer is the semantic recovery authority for these domains. [any]
- Rename/lineage continuity, authored memory facts, and authored outcome facts survive crashes through append-oriented durable writes. [any]

### Convert graph snapshots, derived edges, projections, workspace tree state, and compatibility read models into asynchronous coalesced checkpoints and materializations

- Node id: `coord-task:01kn13qykg063n2zc3h5rt507q`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Checkpoint and materialization work can coalesce multiple runtime updates into fewer storage writes without changing semantic truth. [any]
- Single-file and small-batch runtime writes no longer synchronously pay for rebuildable graph/projection/checkpoint persistence. [any]

### Preserve bounded hot memory and codify explicit hot-only, hot-plus-cold, and cold-backed query paths

- Node id: `coord-task:01kn13r7qf0yf4kqqng4a3nt4w`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Each query surface explicitly declares whether it reads authoritative hot memory, merges hot memory with cold persisted state, or serves a cold-backed historical view. [any]
- The migration does not accidentally hydrate unbounded historical or projection state into RAM. [any]

### Implement checkpoint-plus-journal recovery, revision-lag observability, and replay bounds for restart behavior

- Node id: `coord-task:01kn13rgty80e4y885n1vzb170`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Runtime recovery correctness comes from authoritative journal replay plus bounded checkpoints rather than snapshot authority. [any]
- The system exposes replay time, checkpoint age, backlog, and hot-versus-persisted revision lag so recovery and lag are observable. [any]

### Validate crash safety, restart behavior, latency reduction, memory bounds, and multi-instance shared-runtime correctness for the migration

- Node id: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- Tests and live dogfooding cover crash recovery, cold-query correctness, checkpoint lag, reduced hot-path persistence cost, and bounded memory growth. [any]
- The migration is documented clearly enough that follow-on persistence changes can preserve the same authority and durability rules. [any]

### Review the migration outcome with the user and decide together whether a separate persistence worker deserves a follow-up plan

- Node id: `coord-task:01kn13ryrrk04aacsvg1ac96c2`
- Kind: `edit`
- Status: `completed`

#### Acceptance

- If needed, the follow-up worker plan is scoped from post-migration evidence rather than guessed upfront. [any]
- The user and agent explicitly evaluate whether remaining contention, isolation needs, or backend evolution justify a separate persistence-worker follow-up plan. [any]

## Edges

- `plan-edge:coord-task:01kn13q61f8j93je9vws9kdqjy:depends-on:coord-task:01kn13pz030s4q1c3qrbbg203f`: `coord-task:01kn13q61f8j93je9vws9kdqjy` depends on `coord-task:01kn13pz030s4q1c3qrbbg203f`
- `plan-edge:coord-task:01kn13qdfw63hwrr79cqm7rqs3:depends-on:coord-task:01kn13q61f8j93je9vws9kdqjy`: `coord-task:01kn13qdfw63hwrr79cqm7rqs3` depends on `coord-task:01kn13q61f8j93je9vws9kdqjy`
- `plan-edge:coord-task:01kn13qj69y75rtwrbw6ewqzw5:depends-on:coord-task:01kn13qdfw63hwrr79cqm7rqs3`: `coord-task:01kn13qj69y75rtwrbw6ewqzw5` depends on `coord-task:01kn13qdfw63hwrr79cqm7rqs3`
- `plan-edge:coord-task:01kn13qsapc9fjk72wvth681yf:depends-on:coord-task:01kn13qdfw63hwrr79cqm7rqs3`: `coord-task:01kn13qsapc9fjk72wvth681yf` depends on `coord-task:01kn13qdfw63hwrr79cqm7rqs3`
- `plan-edge:coord-task:01kn13qykg063n2zc3h5rt507q:depends-on:coord-task:01kn13qj69y75rtwrbw6ewqzw5`: `coord-task:01kn13qykg063n2zc3h5rt507q` depends on `coord-task:01kn13qj69y75rtwrbw6ewqzw5`
- `plan-edge:coord-task:01kn13qykg063n2zc3h5rt507q:depends-on:coord-task:01kn13qsapc9fjk72wvth681yf`: `coord-task:01kn13qykg063n2zc3h5rt507q` depends on `coord-task:01kn13qsapc9fjk72wvth681yf`
- `plan-edge:coord-task:01kn13r7qf0yf4kqqng4a3nt4w:depends-on:coord-task:01kn13qdfw63hwrr79cqm7rqs3`: `coord-task:01kn13r7qf0yf4kqqng4a3nt4w` depends on `coord-task:01kn13qdfw63hwrr79cqm7rqs3`
- `plan-edge:coord-task:01kn13rgty80e4y885n1vzb170:depends-on:coord-task:01kn13qykg063n2zc3h5rt507q`: `coord-task:01kn13rgty80e4y885n1vzb170` depends on `coord-task:01kn13qykg063n2zc3h5rt507q`
- `plan-edge:coord-task:01kn13rgty80e4y885n1vzb170:depends-on:coord-task:01kn13r7qf0yf4kqqng4a3nt4w`: `coord-task:01kn13rgty80e4y885n1vzb170` depends on `coord-task:01kn13r7qf0yf4kqqng4a3nt4w`
- `plan-edge:coord-task:01kn13rrg3yzbrcgrjh06ppv3h:depends-on:coord-task:01kn13qj69y75rtwrbw6ewqzw5`: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` depends on `coord-task:01kn13qj69y75rtwrbw6ewqzw5`
- `plan-edge:coord-task:01kn13rrg3yzbrcgrjh06ppv3h:depends-on:coord-task:01kn13qsapc9fjk72wvth681yf`: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` depends on `coord-task:01kn13qsapc9fjk72wvth681yf`
- `plan-edge:coord-task:01kn13rrg3yzbrcgrjh06ppv3h:depends-on:coord-task:01kn13qykg063n2zc3h5rt507q`: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` depends on `coord-task:01kn13qykg063n2zc3h5rt507q`
- `plan-edge:coord-task:01kn13rrg3yzbrcgrjh06ppv3h:depends-on:coord-task:01kn13r7qf0yf4kqqng4a3nt4w`: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` depends on `coord-task:01kn13r7qf0yf4kqqng4a3nt4w`
- `plan-edge:coord-task:01kn13rrg3yzbrcgrjh06ppv3h:depends-on:coord-task:01kn13rgty80e4y885n1vzb170`: `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` depends on `coord-task:01kn13rgty80e4y885n1vzb170`
- `plan-edge:coord-task:01kn13ryrrk04aacsvg1ac96c2:depends-on:coord-task:01kn13rrg3yzbrcgrjh06ppv3h`: `coord-task:01kn13ryrrk04aacsvg1ac96c2` depends on `coord-task:01kn13rrg3yzbrcgrjh06ppv3h`

