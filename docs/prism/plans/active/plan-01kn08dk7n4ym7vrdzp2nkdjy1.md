# Improve PRISM’s agent-facing workflow by hardening workflow contracts, making compact discovery and editing ownership-aware and large-file-aware, and making durable knowledge publication easier to promote safely, while closing the dogfooding friction surfaced during live use.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:6c413a0081f9ac283606cd3466a84860ed0014b9dec5266a49f3ac5d0adca634`
- Source logical timestamp: `unknown`
- Source snapshot: `4 nodes, 3 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn08dk7n4ym7vrdzp2nkdjy1`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `4`
- Edges: `3`

## Goal

Improve PRISM’s agent-facing workflow by hardening workflow contracts, making compact discovery and editing ownership-aware and large-file-aware, and making durable knowledge publication easier to promote safely, while closing the dogfooding friction surfaced during live use.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn08dk7n4ym7vrdzp2nkdjy1.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn08e5zpqsed4xsmjy2g0zq5`
- `coord-task:01kn08e60qbk9fe00zn861x5j4`
- `coord-task:01kn08e61maayjpm61gyp35s70`

## Nodes

### Milestone 1: Trusted workflow contracts

- Node id: `coord-task:01kn08e5zpqsed4xsmjy2g0zq5`
- Kind: `edit`
- Status: `completed`
- Summary: Unify native plan-node, coordination-task, session-state, and schema-surface behavior so agents do not need to care which workflow shape is active. Fold in the live friction where repo guidance implied a `prism_session` inspect hop that the server does not actually support.

#### Acceptance

- The same validation and completion flow succeeds for coordination tasks and native plan nodes. [any]
- Schema examples, tool schemas, and live behavior agree for prism_session and high-value prism_mutate paths. [any]
- Session and workflow diagnostics clearly explain which workflow unit is active and what action is supported next. [any]

#### Tags

- `contracts`
- `dogfood`
- `milestone`
- `workflow`

### Milestone 2: Ownership-aware compact editing

- Node id: `coord-task:01kn08e60qbk9fe00zn861x5j4`
- Kind: `edit`
- Status: `completed`
- Summary: Improve locate, workset, and open follow-through so PRISM ranks owning modules first, preserves semantic-to-exact edit momentum in large files, and emits explicit monolith-pressure diagnostics instead of quietly degrading. Fold in the live friction where concept-driven worksets resolved directionally but then failed with `no reusable members`.

#### Acceptance

- Known routing, shell, and ownership-oriented queries rank the owning module ahead of nearby consumers. [any]
- prism_open returns edit-shaped structural context for large files without immediately forcing shell fallback. [any]
- prism_workset and related concept flows either produce reusable follow-through or fail with explicit next actions instead of empty-member dead ends. [any]
- Large-file and mixed-purpose targets emit useful pressure diagnostics and decomposition-aware next steps. [any]

#### Tags

- `compact-tools`
- `dogfood`
- `edit-handoff`
- `milestone`
- `ranking`

### Milestone 3: Durable knowledge publication

- Node id: `coord-task:01kn08e61maayjpm61gyp35s70`
- Kind: `edit`
- Status: `completed`
- Summary: Make the last mile from strong session findings to repo-scoped memory and concepts low-friction, safe, and reviewable so durable lessons stop getting rediscovered across sessions.

#### Acceptance

- Strong session memories surface clear repo-promotion candidates with duplicate detection and sensible defaults. [any]
- Repo-scoped memory and concept publication support review, supersede, and retire flows that feel safe to use routinely. [any]
- Real tasks promote the strongest architectural or workflow lessons without flooding repo knowledge with one-off notes. [any]

#### Tags

- `concepts`
- `memory`
- `milestone`
- `publication`

### Gate: Predictable operational smoothness

- Node id: `coord-task:01kn08efjpe3avh5zc4xm918sq`
- Kind: `validate`
- Status: `completed`
- Summary: Treat operational smoothness as a release gate across the milestone set. Each implementation track must improve dogfooding reliability, freshness, and diagnostics so agents can stay inside PRISM for orient, workset, open, mutate, and validate loops without repeated fallback caused by abstraction leaks.

#### Acceptance

- Each milestone ships with at least one dogfooding scenario and one freshness or diagnostic check. [any]
- PRISM-first workflows stay inside PRISM for orient, workset, open, mutate, and validate loops without repeated shell fallback caused by ambiguity or unsupported paths. [any]
- Operational diagnostics are specific enough to explain failures like unsupported session actions or empty-member worksets. [any]

#### Tags

- `dogfood`
- `gate`
- `operations`
- `validation`

## Edges

- `plan-edge:coord-task:01kn08efjpe3avh5zc4xm918sq:depends-on:coord-task:01kn08e5zpqsed4xsmjy2g0zq5`: `coord-task:01kn08efjpe3avh5zc4xm918sq` depends on `coord-task:01kn08e5zpqsed4xsmjy2g0zq5`
- `plan-edge:coord-task:01kn08efjpe3avh5zc4xm918sq:depends-on:coord-task:01kn08e60qbk9fe00zn861x5j4`: `coord-task:01kn08efjpe3avh5zc4xm918sq` depends on `coord-task:01kn08e60qbk9fe00zn861x5j4`
- `plan-edge:coord-task:01kn08efjpe3avh5zc4xm918sq:depends-on:coord-task:01kn08e61maayjpm61gyp35s70`: `coord-task:01kn08efjpe3avh5zc4xm918sq` depends on `coord-task:01kn08e61maayjpm61gyp35s70`

