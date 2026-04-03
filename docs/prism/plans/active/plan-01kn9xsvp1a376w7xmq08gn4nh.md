# Remove tracked changes from .prism state and rely on signed commit history plus manifest metadata

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:cb037ad2e8e407c125bed5532d927d6ab0b3ecea241aef95c252f8beb1ab2b84`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 11 edges, 4 overlays`

## Overview

- Plan id: `plan:01kn9xsvp1a376w7xmq08gn4nh`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `11`

## Goal

Make shared runtime the sole owner of append-only change history, remove tracked `.prism/state/changes/**`, keep tracked `.prism/state` as stable semantic state only, and rely on signed Git commit history plus PRISM manifest metadata for durable coarse-grained publish history.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Require task branch: `true`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn9xsvp1a376w7xmq08gn4nh.json`
- Legacy migration log path: unavailable in the current projection

## Root Nodes

- `coord-task:01kn9xt9h76645shp4jcmt0bsv`

## Nodes

### Lock the tracked changes removal contract and commit-history trust boundary

- Node id: `coord-task:01kn9xt9h76645shp4jcmt0bsv`
- Kind: `edit`
- Status: `completed`
- Summary: Define that tracked `.prism/state` keeps only current semantic state, shared runtime becomes the sole append-log owner for changes, signed Git commit history becomes the coarse-grained durable change history, and the PRISM manifest carries structured publish metadata including a short `publishSummary`.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Specs and contracts explicitly state that tracked `.prism/state/changes/**` is not part of the durable repo-owned authority model [any]
- The trust boundary between signed Git commits, PRISM manifest metadata, and shared-runtime journals is locked without ambiguity [any]

### Implement manifest publishSummary and structured publish metadata without tracked changes history

- Node id: `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`
- Kind: `edit`
- Status: `completed`
- Summary: Extend the tracked snapshot manifest so durable publish context lives there instead of in tracked change history, including a concise `publishSummary` and any required structured publish metadata for principal, work, and continuity.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Tracked manifests carry concise semantic publish summaries and structured publish metadata without expanding into operational change history [any]
- Manifest chaining and publish continuity remain valid after removing tracked changes [any]

### Stop publishing tracked .prism state changes shards and indexes

- Node id: `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`
- Kind: `edit`
- Status: `completed`
- Summary: Remove `.prism/state/changes/**` and any tracked indexes or manifest inventory entries that treat change history as repo-owned snapshot authority, while preserving current semantic shards for memories, concepts, contracts, plans, and durable coordination.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Tracked publishes no longer write change history shards under `.prism/state/changes/**` [any]
- Tracked snapshot indexes and manifests no longer model change history as durable repo-owned state [any]

### Remove tracked changes read-path assumptions and redirect operational reports to shared runtime

- Node id: `coord-task:01kn9xv2a54t54pbzbbremqrc7`
- Kind: `edit`
- Status: `completed`
- Summary: Update tracked snapshot readers, projections, docs, MCP/query surfaces, and generated reports so current semantic docs read tracked snapshots while operational change reports read shared runtime or shared-runtime-derived projections.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Semantic docs and tracked state readers no longer depend on tracked change history [any]
- Operational change reports and diagnostics read shared runtime rather than tracked snapshot change shards [any]

### Add migration and repair for repos that already carry tracked changes state

- Node id: `coord-task:01kn9xv8x94h8gwja48wahgsnb`
- Kind: `edit`
- Status: `in_progress`
- Summary: Provide deterministic migration for repos that already contain `.prism/state/changes/**`, including manifest/index repair, removal or archival of obsolete tracked change shards, and explicit continuity across the pre-removal and post-removal snapshot formats.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Existing tracked repos can converge onto the new no-tracked-changes model without losing current semantic state [any]
- Migration explicitly preserves trust and publish continuity across the tracked-changes removal boundary [any]

### Validate clone size, startup behavior, signed publish semantics, and runtime-only change history ownership

- Node id: `coord-task:01kn9xvmrn86qb17qz7w97na8n`
- Kind: `edit`
- Status: `ready`
- Summary: Prove that tracked `changes` is gone from repo-owned state, cold clones still load current semantic state correctly, hot and cold startup no longer pay for tracked change history, signed Git plus manifest metadata cover the publish boundary, and fine-grained change history remains available from shared runtime.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Tracked repo state contains no durable `changes` subtree while current semantic state remains cold-clone self-contained [any]
- Startup and clone-size validation confirm that tracked change history is no longer part of the repo-owned cost model [any]
- Fine-grained change history remains available from shared runtime rather than tracked snapshot state [any]

## Edges

- `plan-edge:coord-task:01kn9xvmrn86qb17qz7w97na8n:depends-on:coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`: `coord-task:01kn9xvmrn86qb17qz7w97na8n` depends on `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`
- `plan-edge:coord-task:01kn9xvmrn86qb17qz7w97na8n:depends-on:coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`: `coord-task:01kn9xvmrn86qb17qz7w97na8n` depends on `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`
- `plan-edge:coord-task:01kn9xvmrn86qb17qz7w97na8n:depends-on:coord-task:01kn9xv2a54t54pbzbbremqrc7`: `coord-task:01kn9xvmrn86qb17qz7w97na8n` depends on `coord-task:01kn9xv2a54t54pbzbbremqrc7`
- `plan-edge:coord-task:01kn9xvmrn86qb17qz7w97na8n:depends-on:coord-task:01kn9xv8x94h8gwja48wahgsnb`: `coord-task:01kn9xvmrn86qb17qz7w97na8n` depends on `coord-task:01kn9xv8x94h8gwja48wahgsnb`
- `plan-edge:coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j:depends-on:coord-task:01kn9xt9h76645shp4jcmt0bsv`: `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j` depends on `coord-task:01kn9xt9h76645shp4jcmt0bsv`
- `plan-edge:coord-task:01kn9xtsf57cqfxa0a7hd2wsx8:depends-on:coord-task:01kn9xt9h76645shp4jcmt0bsv`: `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8` depends on `coord-task:01kn9xt9h76645shp4jcmt0bsv`
- `plan-edge:coord-task:01kn9xtsf57cqfxa0a7hd2wsx8:depends-on:coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`: `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8` depends on `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`
- `plan-edge:coord-task:01kn9xv2a54t54pbzbbremqrc7:depends-on:coord-task:01kn9xt9h76645shp4jcmt0bsv`: `coord-task:01kn9xv2a54t54pbzbbremqrc7` depends on `coord-task:01kn9xt9h76645shp4jcmt0bsv`
- `plan-edge:coord-task:01kn9xv2a54t54pbzbbremqrc7:depends-on:coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`: `coord-task:01kn9xv2a54t54pbzbbremqrc7` depends on `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`
- `plan-edge:coord-task:01kn9xv8x94h8gwja48wahgsnb:depends-on:coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`: `coord-task:01kn9xv8x94h8gwja48wahgsnb` depends on `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`
- `plan-edge:coord-task:01kn9xv8x94h8gwja48wahgsnb:depends-on:coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`: `coord-task:01kn9xv8x94h8gwja48wahgsnb` depends on `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`

## Execution Overlays

- Node: `coord-task:01kn9xt9h76645shp4jcmt0bsv`
  git execution status: `published`
  source ref: `task/prism-changes-state-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-changes-state-rewrite`
- Node: `coord-task:01kn9xtjh9ac4zwwfx2mbn9k0j`
  git execution status: `published`
  source ref: `task/prism-changes-state-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-changes-state-rewrite`
- Node: `coord-task:01kn9xtsf57cqfxa0a7hd2wsx8`
  git execution status: `published`
  source ref: `task/prism-changes-state-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-changes-state-rewrite`
- Node: `coord-task:01kn9xv2a54t54pbzbbremqrc7`
  git execution status: `published`
  source ref: `task/prism-changes-state-rewrite`
  target ref: `origin/main`
  publish ref: `task/prism-changes-state-rewrite`

