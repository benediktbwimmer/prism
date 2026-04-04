# audit-02: identify and remove repo-scale persistence and refresh work hidden behind narrow update paths, then land targeted fixes in priority order

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:1ef8f2114c11fc04f0347a634bb5b1630863da80e865ecfc26171f6911cf529a`
- Source logical timestamp: `unknown`
- Source snapshot: `5 nodes, 0 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn0yaqr4ktj8ey63r1r3n79q`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `5`
- Edges: `0`

## Goal

audit-02: identify and remove repo-scale persistence and refresh work hidden behind narrow update paths, then land targeted fixes in priority order

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn0yaqr4ktj8ey63r1r3n79q.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn0yb9zz75x9sngz7eppvvt8`
- `coord-task:01kn0yba1epg1zwxbfpwevnyva`
- `coord-task:01kn0yba2qp9xsp8wrvxx0qwad`
- `coord-task:01kn0yba4t8v1r8tvgxe2aphna`
- `coord-task:01kn0ybpgxrm7ddbhaxkq78kx5`

## Nodes

### Instrument and rank hidden repo-scale persistence work

- Node id: `coord-task:01kn0yb9zz75x9sngz7eppvvt8`
- Kind: `edit`
- Status: `completed`

#### Bindings

- Anchor: `file:263`
- Anchor: `file:74`

#### Acceptance

- The dominant repo-scale costs on narrow refresh and mutation paths are identified with concrete code anchors and runtime evidence. [any]

### Remove full-log snapshot reloads from auxiliary persistence

- Node id: `coord-task:01kn0yba1epg1zwxbfpwevnyva`
- Kind: `edit`
- Status: `completed`

#### Bindings

- Anchor: `file:262`
- Anchor: `file:263`
- Anchor: `file:264`

#### Acceptance

- Outcome and episodic persistence stop reconstructing whole snapshots inside hot write transactions. [any]

### Make projection maintenance proportional to changed scope

- Node id: `coord-task:01kn0yba2qp9xsp8wrvxx0qwad`
- Kind: `edit`
- Status: `completed`

#### Bindings

- Anchor: `file:263`
- Anchor: `file:265`
- Anchor: `file:74`

#### Acceptance

- Co-change and validation projection persistence no longer does repo-wide maintenance for tiny deltas without justification. [any]

### Validate persistence hot-path improvements with runtime evidence

- Node id: `coord-task:01kn0yba4t8v1r8tvgxe2aphna`
- Kind: `edit`
- Status: `completed`

#### Bindings

- Anchor: `file:167`
- Anchor: `file:206`
- Anchor: `file:74`

#### Acceptance

- Runtime logs and focused regressions show materially lower narrow-scope persist and refresh costs after the fixes. [any]

### Eliminate global graph rewrites from targeted index persistence

- Node id: `coord-task:01kn0ybpgxrm7ddbhaxkq78kx5`
- Kind: `edit`
- Status: `completed`

#### Bindings

- Anchor: `file:259`
- Anchor: `file:263`
- Anchor: `file:74`

#### Acceptance

- Targeted index persistence no longer rewrites repo-wide derived-edge or finalize state unless the change genuinely requires it. [any]

## Edges

No published plan edges are currently recorded.

