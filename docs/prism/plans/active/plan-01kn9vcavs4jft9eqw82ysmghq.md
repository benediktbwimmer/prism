# Eliminate absolute path leakage and make path identity repo-relative across PRISM

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:77d148b5aeb0827ec3e4dc517816ff64f3313c43d8811d6059ee162ba5e7fd63`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 11 edges, 3 overlays`

## Overview

- Plan id: `plan:01kn9vcavs4jft9eqw82ysmghq`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `11`

## Goal

Define one correct path identity model for PRISM so tracked snapshots, shared/runtime state, APIs, docs, and projections never persist absolute filesystem paths; use file ids plus repo-relative paths as canonical identity and anchors as the primary semantic reference.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`
- Max fetch age seconds: `120`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn9vcavs4jft9eqw82ysmghq.json`
- Legacy migration log path: unavailable in the current projection

## Root Nodes

- `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`

## Nodes

### Lock the canonical path identity contract and no-absolute-path boundary

- Node id: `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`
- Kind: `edit`
- Status: `completed`
- Summary: Define where absolute paths may exist, where only repo-relative paths are allowed, and where anchors must replace raw paths entirely across tracked state, shared runtime, and local runtime.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Specs and contracts explicitly state that tracked `.prism` and shared/logical runtime data must not persist absolute filesystem paths [any]
- The canonical identity model distinguishes file ids, repo-relative paths, anchors, and local derived absolute paths without ambiguity [any]

### Canonicalize graph and workspace file identity around file ids plus repo-relative paths

- Node id: `coord-task:01kn9vd88apgdq35f4hr9tmsss`
- Kind: `edit`
- Status: `completed`
- Summary: Make file ids the primary internal key, make repo-relative paths the canonical stored path identity, and ensure absolute paths are derived only in local worktree runtime code that actually touches the filesystem.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Graph and workspace identity layers no longer require absolute paths as canonical persisted file identity [any]
- Absolute paths are treated as local execution conveniences derived from worktree root plus repo-relative path [any]

### Normalize tracked snapshot, protected publication, and shared/runtime serialization to repo-relative paths

- Node id: `coord-task:01kn9vdfjsrbzwz5w5mff8yqck`
- Kind: `edit`
- Status: `completed`
- Summary: Remove absolute path leakage from tracked `.prism/state/**`, protected publication flows, shared journals, and repo-published change snapshots, using anchors as the primary reference and repo-relative paths only as the fallback file-level reference.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Tracked snapshot shards and manifests contain no absolute filesystem paths [any]
- Shared/runtime records intended to survive across worktrees or clones do not persist absolute filesystem paths [any]
- Change snapshots and outcome metadata use anchors plus repo-relative file references instead of absolute file paths [any]

### Audit MCP/query/docs/projection surfaces for path leakage and anchor misuse

- Node id: `coord-task:01kn9vdqfnnkkywq1bb0jk5fq8`
- Kind: `edit`
- Status: `ready`
- Summary: Update public schemas, PRISM query surfaces, docs, generated projections, and read models so portable repo-relative paths and semantic anchors are exposed consistently, and machine-local absolute paths never leak into durable or shared surfaces.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Public docs and schemas no longer present absolute paths as durable PRISM identity [any]
- Public read surfaces prefer anchors and repo-relative paths for portable file references [any]

### Add migration, auditing, and repair paths for legacy absolute-path data

- Node id: `coord-task:01kn9vdznkm34knqjxgdqdqcxe`
- Kind: `edit`
- Status: `ready`
- Summary: Detect and repair existing absolute-path leakage in tracked snapshot state, shared/runtime records, and caches so current repos can converge onto the corrected path model without carrying machine-specific artifacts forward.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Existing tracked or shared data with absolute paths can be detected and repaired deterministically [any]
- Migration does not require preserving machine-specific absolute path identity in repo-published state [any]

### Validate no absolute paths survive in published or shared PRISM state

- Node id: `coord-task:01kn9ve8ants7rmt18m4rpf25b`
- Kind: `edit`
- Status: `ready`
- Summary: Prove through regression coverage and live inspection that tracked `.prism`, publish manifests, repo-published changes, and portable/shared runtime exports contain no absolute filesystem paths while local runtime still functions correctly from derived worktree paths.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Repo-published `.prism/state/**` contains no absolute filesystem paths [any]
- Portable/shared PRISM state contains no absolute filesystem paths across worktrees [any]
- Validation coverage and live checks enforce the invariant going forward [any]

## Edges

- `plan-edge:coord-task:01kn9vdqfnnkkywq1bb0jk5fq8:depends-on:coord-task:01kn9vd1qxxaht9ap1ygbmd23g`: `coord-task:01kn9vdqfnnkkywq1bb0jk5fq8` depends on `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`
- `plan-edge:coord-task:01kn9vdqfnnkkywq1bb0jk5fq8:depends-on:coord-task:01kn9vd88apgdq35f4hr9tmsss`: `coord-task:01kn9vdqfnnkkywq1bb0jk5fq8` depends on `coord-task:01kn9vd88apgdq35f4hr9tmsss`
- `plan-edge:coord-task:01kn9vdqfnnkkywq1bb0jk5fq8:depends-on:coord-task:01kn9vdfjsrbzwz5w5mff8yqck`: `coord-task:01kn9vdqfnnkkywq1bb0jk5fq8` depends on `coord-task:01kn9vdfjsrbzwz5w5mff8yqck`
- `plan-edge:coord-task:01kn9vdznkm34knqjxgdqdqcxe:depends-on:coord-task:01kn9vd88apgdq35f4hr9tmsss`: `coord-task:01kn9vdznkm34knqjxgdqdqcxe` depends on `coord-task:01kn9vd88apgdq35f4hr9tmsss`
- `plan-edge:coord-task:01kn9vdznkm34knqjxgdqdqcxe:depends-on:coord-task:01kn9vdfjsrbzwz5w5mff8yqck`: `coord-task:01kn9vdznkm34knqjxgdqdqcxe` depends on `coord-task:01kn9vdfjsrbzwz5w5mff8yqck`
- `plan-edge:coord-task:01kn9ve8ants7rmt18m4rpf25b:depends-on:coord-task:01kn9vdfjsrbzwz5w5mff8yqck`: `coord-task:01kn9ve8ants7rmt18m4rpf25b` depends on `coord-task:01kn9vdfjsrbzwz5w5mff8yqck`
- `plan-edge:coord-task:01kn9ve8ants7rmt18m4rpf25b:depends-on:coord-task:01kn9vdqfnnkkywq1bb0jk5fq8`: `coord-task:01kn9ve8ants7rmt18m4rpf25b` depends on `coord-task:01kn9vdqfnnkkywq1bb0jk5fq8`
- `plan-edge:coord-task:01kn9ve8ants7rmt18m4rpf25b:depends-on:coord-task:01kn9vdznkm34knqjxgdqdqcxe`: `coord-task:01kn9ve8ants7rmt18m4rpf25b` depends on `coord-task:01kn9vdznkm34knqjxgdqdqcxe`
- `plan-edge:coord-task:01kn9vd88apgdq35f4hr9tmsss:depends-on:coord-task:01kn9vd1qxxaht9ap1ygbmd23g`: `coord-task:01kn9vd88apgdq35f4hr9tmsss` depends on `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`
- `plan-edge:coord-task:01kn9vdfjsrbzwz5w5mff8yqck:depends-on:coord-task:01kn9vd1qxxaht9ap1ygbmd23g`: `coord-task:01kn9vdfjsrbzwz5w5mff8yqck` depends on `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`
- `plan-edge:coord-task:01kn9vdfjsrbzwz5w5mff8yqck:depends-on:coord-task:01kn9vd88apgdq35f4hr9tmsss`: `coord-task:01kn9vdfjsrbzwz5w5mff8yqck` depends on `coord-task:01kn9vd88apgdq35f4hr9tmsss`

## Execution Overlays

- Node: `coord-task:01kn9vd1qxxaht9ap1ygbmd23g`
  git execution status: `published`
  source ref: `task/path-identity-repo-relative`
  target ref: `origin/main`
  publish ref: `task/path-identity-repo-relative`
- Node: `coord-task:01kn9vd88apgdq35f4hr9tmsss`
  git execution status: `published`
  source ref: `task/path-identity-repo-relative`
  target ref: `origin/main`
  publish ref: `task/path-identity-repo-relative`
- Node: `coord-task:01kn9vdfjsrbzwz5w5mff8yqck`
  git execution status: `publish_pending`
  pending task status: `completed`
  source ref: `task/path-identity-repo-relative`
  target ref: `origin/main`
  publish ref: `task/path-identity-repo-relative`

