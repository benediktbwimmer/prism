# prism-query-01: add a runtime-gated query-view registry, harden existing PRISM query surfaces, and ship a small measured set of new agent-facing prism_query views with usage logging and pruning discipline.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:7df0da548facb5d3be8325e2be425b4a2a33578a6c025599e36de74ae5149258`
- Source logical timestamp: `unknown`
- Source snapshot: `6 nodes, 5 edges, 0 overlays`

## Overview

- Plan id: `plan:01kmwxra5d93wb6nmy6a79v8eh`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `6`
- Edges: `5`

## Goal

prism-query-01: add a runtime-gated query-view registry, harden existing PRISM query surfaces, and ship a small measured set of new agent-facing prism_query views with usage logging and pruning discipline.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kmwxra5d93wb6nmy6a79v8eh.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kmwxt05gtntnfmn60ppbwpaf`

## Nodes

### Milestone 0: Query-view registry and runtime feature gating

- Node id: `coord-task:01kmwxt05gtntnfmn60ppbwpaf`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented the query-view registry, capability advertising, runtime dispatch, and per-view query/MCP logging aggregation required for milestone 0.
- Priority: `1`

#### Acceptance

- Views can be enabled or omitted independently at runtime without adding new top-level MCP tools [any]
- prism://capabilities exposes the enabled/disabled query-view surface with enough metadata for an agent to adapt [any]
- Query/MCP logs can aggregate invocation count, unique task usage, latency, and result size by view name [any]

#### Validation Refs

- `pq01:m0-registry-gating`

#### Tags

- `feature-gating`
- `logging`
- `prism-query`
- `registry`

### Milestone 1: Harden existing query surfaces and docs

- Node id: `coord-task:01kmwxt8f457f2f0kwzfbfq50h`
- Kind: `edit`
- Status: `completed`
- Summary: Milestone 1 completed. prism_query now maps feature-disabled and bad-argument user errors to consistent invalid-params MCP responses, session task/agent/limits restore after daemon restart through a persisted session seed, and prism_workset/API baseline hardening is in place with release-build and live-daemon verification.
- Priority: `1`

#### Acceptance

- Current prism_query and compact surfaces produce consistent disabled-feature and bad-argument errors [any]
- Daemon restart preserves or explicitly restores the active task/session context needed for follow-on work [any]
- prism_workset and API reference output are strong enough to serve as the comparison baseline for new views [any]

#### Validation Refs

- `pq01:m1-surface-hardening`

#### Tags

- `continuity`
- `docs`
- `hardening`
- `workset`

### Milestone 2: Add repoPlaybook() and validationPlan(...)

- Node id: `coord-task:01kmwxth0nk7w9jrf4zhs8w348`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented repoPlaybook() and validationPlan(...) as feature-gated dynamic query views with typed API/docs coverage, per-view logging continuity, focused validation tests, and a refreshed live daemon.
- Priority: `2`

#### Acceptance

- repoPlaybook() returns actionable build, test, lint, format, and gotcha guidance with sources or provenance [any]
- validationPlan(...) returns fast and broader checks with explicit why fields and does not require a new MCP tool [any]
- Both views are independently gateable, logged by view name, and documented in the API reference [any]

#### Validation Refs

- `pq01:m2-playbook-validation-plan`

#### Tags

- `experimental`
- `new-view`
- `playbook`
- `validation-plan`

### Milestone 3: Add impact(...) and afterEdit(...)

- Node id: `coord-task:01kmwxtrc6vbrsmv2xx2phnv8k`
- Kind: `edit`
- Status: `completed`
- Summary: Added `impact(...)` and `afterEdit(...)`, wired them into the dynamic query-view registry, extended the typed/docs surface, added focused feature-flag and query-history coverage, and refreshed the live daemon.
- Priority: `2`

#### Acceptance

- impact(...) returns downstream files or targets, risk hints, and recommended checks with explicit why fields [any]
- afterEdit(...) turns recent edits or task changes into the next high-value reads, tests, and docs to inspect [any]
- Both views stay compact, deterministic, independently gateable, and measurable through registry logging [any]

#### Validation Refs

- `pq01:m3-impact-after-edit`

#### Tags

- `after-edit`
- `explainable`
- `impact`
- `new-view`

### Milestone 4: Add command memory from explicit observed evidence

- Node id: `coord-task:01kmwxv28vwzpra08sz1qznxfm`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented `commandMemory(...)` as a feature-gated dynamic query view that merges repo playbook signals with explicit observed command evidence, added typed/docs coverage, extended outcome evidence with explicit command argv, added focused query-history validation, and refreshed the live daemon.
- Priority: `2`

#### Acceptance

- The command memory design does not rely on hidden shell interception and works from repo signals plus explicit observed evidence [any]
- Recommendations include confidence, provenance, and avoid or caveat signals where appropriate [any]
- The resulting surface remains gated, logged, and compact enough to be evaluated against actual agent use [any]

#### Validation Refs

- `pq01:m4-command-memory`

#### Tags

- `command-memory`
- `evidence`
- `new-view`
- `repo-workflow`

### Milestone 5: Add adoption review, prune dead views, and promote survivors

- Node id: `coord-task:01kmwxv9nm654h46g4p3gbxcy4`
- Kind: `validate`
- Status: `ready`
- Summary: Close the loop on registry-backed query views. Add or reuse analysis surfaces over query and MCP logs so maintainers can answer which views are exposed, which are used, how often, by how many tasks, with what latency, and whether fallback behavior suggests the view actually helped. Use those measurements to promote a small set of useful views, keep marginal ones experimental, and remove dead or misleading surfaces instead of accumulating permanent API clutter.
- Priority: `3`

#### Acceptance

- Maintainers can inspect per-view exposure, invocation counts, unique task usage, and latency from durable logs [any]
- The plan yields an explicit promote, keep-experimental, or remove decision for each newly added view [any]
- Unused or low-value views do not remain permanently exposed by default [any]

#### Validation Refs

- `pq01:m5-adoption-review`

#### Tags

- `adoption`
- `measurement`
- `promotion`
- `pruning`

## Edges

- `plan-edge:coord-task:01kmwxt8f457f2f0kwzfbfq50h:depends-on:coord-task:01kmwxt05gtntnfmn60ppbwpaf`: `coord-task:01kmwxt8f457f2f0kwzfbfq50h` depends on `coord-task:01kmwxt05gtntnfmn60ppbwpaf`
- `plan-edge:coord-task:01kmwxth0nk7w9jrf4zhs8w348:depends-on:coord-task:01kmwxt8f457f2f0kwzfbfq50h`: `coord-task:01kmwxth0nk7w9jrf4zhs8w348` depends on `coord-task:01kmwxt8f457f2f0kwzfbfq50h`
- `plan-edge:coord-task:01kmwxtrc6vbrsmv2xx2phnv8k:depends-on:coord-task:01kmwxth0nk7w9jrf4zhs8w348`: `coord-task:01kmwxtrc6vbrsmv2xx2phnv8k` depends on `coord-task:01kmwxth0nk7w9jrf4zhs8w348`
- `plan-edge:coord-task:01kmwxv28vwzpra08sz1qznxfm:depends-on:coord-task:01kmwxtrc6vbrsmv2xx2phnv8k`: `coord-task:01kmwxv28vwzpra08sz1qznxfm` depends on `coord-task:01kmwxtrc6vbrsmv2xx2phnv8k`
- `plan-edge:coord-task:01kmwxv9nm654h46g4p3gbxcy4:depends-on:coord-task:01kmwxv28vwzpra08sz1qznxfm`: `coord-task:01kmwxv9nm654h46g4p3gbxcy4` depends on `coord-task:01kmwxv28vwzpra08sz1qznxfm`

