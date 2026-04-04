# Reduce the next highest-priority MCP slow-call hotspots after excluding prism_mutate mutate.coordination, starting with prism://plans, prism_concept, and shared compact-tool read paths, then validate the latency improvements with repo-wide call-log evidence.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:db153d7d717ec99932f8e4fb441bf47e179d5455e2d674e3a5aae61ef8aaeb6f`
- Source logical timestamp: `unknown`
- Source snapshot: `8 nodes, 6 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn88m9a9ckax8ft3pwm7ahgq`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `8`
- Edges: `6`

## Goal

Reduce the next highest-priority MCP slow-call hotspots after excluding prism_mutate mutate.coordination, starting with prism://plans, prism_concept, and shared compact-tool read paths, then validate the latency improvements with repo-wide call-log evidence.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01kn88m9a9ckax8ft3pwm7ahgq.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01kn88mvdemb54hwcw9wczv19j`
- `coord-task:01kn88n8aef9wcbvsbb6bjhfpq`
- `coord-task:01kn88n9bxjefsmjc8pqv1dhcq`
- `coord-task:01kn88naczk5jdjtgm5qzf23be`

## Nodes

### Trace the prism://plans hot path and identify why the warm resource handler still spends about 1.27s outside runtime sync.

- Node id: `coord-task:01kn88mvdemb54hwcw9wczv19j`
- Kind: `investigate`
- Status: `completed`
- Summary: Inspect the plans resource path, projection loading, pagination, and any derived-state work so the next edit has a precise latency budget and target.
- Priority: `1`

### Profile warm prism_concept latency and isolate the expensive concept resolution, decode, and render work.

- Node id: `coord-task:01kn88n8aef9wcbvsbb6bjhfpq`
- Kind: `investigate`
- Status: `completed`
- Summary: Warm prism_concept cost is now isolated: ProjectionIndex::resolve_concepts was scoring far more candidates than small lookups needed, and capping relation-rerank candidates improved live prism_concept(coordination) from 1767ms to 1504-1578ms on this worktree after rebuild/restart. Remaining cost appears to be per-call packet rendering and relation shaping in compact_tools/concept.rs.
- Priority: `1`

### Profile the shared compact-tool hot path behind prism_open, prism_workset, prism_locate, and prism_gather.

- Node id: `coord-task:01kn88n9bxjefsmjc8pqv1dhcq`
- Kind: `investigate`
- Status: `ready`
- Summary: Separate ranking, handle resolution, preview, file-read, and workset assembly costs so shared compact-tool optimizations land in the right module.
- Priority: `1`

### Design a lighter MCP log introspection path so slow prism_query diagnostics do not need many per-call prism.mcpTrace lookups.

- Node id: `coord-task:01kn88naczk5jdjtgm5qzf23be`
- Kind: `investigate`
- Status: `ready`
- Summary: Identify whether batching, richer aggregate views, or trace-shape changes can make heavy self-observability queries cheaper without changing general prism_query execution semantics.
- Priority: `2`

### Implement the prism://plans latency fixes with a warm-path target comfortably below the current 1.27s read.

- Node id: `coord-task:01kn88ntbg34qjspspev07mxj1`
- Kind: `edit`
- Status: `completed`
- Summary: First pass landed and validated: warm prism://plans now keeps a stable published generation, and immediate repeat reads collapse from about 638-658ms to 0ms in the durable MCP log. This node is complete; remaining work is in other hotspots, not the plans resource.
- Priority: `1`

### Implement the prism_concept latency fixes so common warm concept reads stop spending most time in the compact handler.

- Node id: `coord-task:01kn88nvmc3xnak8b7y6z7a297`
- Kind: `edit`
- Status: `ready`
- Summary: First pass landed: bounded concept relation reranking reduced live prism_concept(coordination) from 1767ms to 1504-1578ms. Next pick-up should focus on compact_tools/concept.rs packet rendering, especially repeated relation shaping and alternate packet view construction, because the remaining latency looks fully per-call.
- Priority: `1`

### Validate the latency work with targeted timing checks and a fresh repo-wide slow-call log pass.

- Node id: `coord-task:01kn88nwp3cqgppdhgn8zv10vw`
- Kind: `validate`
- Status: `blocked`
- Summary: Re-run targeted validations and compare the repo-wide MCP call log so the plan closes with measured before/after evidence instead of intuition.
- Priority: `1`

### Implement shared compact-tool latency fixes across prism_open, prism_workset, prism_locate, and prism_gather.

- Node id: `coord-task:01kn88nxq7nkrkmydemx7rpry1`
- Kind: `edit`
- Status: `blocked`
- Summary: Land the highest-value shared-path optimizations so the main bounded-context tools feel materially faster on warm runtimes.
- Priority: `1`

## Edges

- `plan-edge:coord-task:01kn88ntbg34qjspspev07mxj1:depends-on:coord-task:01kn88mvdemb54hwcw9wczv19j`: `coord-task:01kn88ntbg34qjspspev07mxj1` depends on `coord-task:01kn88mvdemb54hwcw9wczv19j`
- `plan-edge:coord-task:01kn88nvmc3xnak8b7y6z7a297:depends-on:coord-task:01kn88n8aef9wcbvsbb6bjhfpq`: `coord-task:01kn88nvmc3xnak8b7y6z7a297` depends on `coord-task:01kn88n8aef9wcbvsbb6bjhfpq`
- `plan-edge:coord-task:01kn88nwp3cqgppdhgn8zv10vw:depends-on:coord-task:01kn88ntbg34qjspspev07mxj1`: `coord-task:01kn88nwp3cqgppdhgn8zv10vw` depends on `coord-task:01kn88ntbg34qjspspev07mxj1`
- `plan-edge:coord-task:01kn88nwp3cqgppdhgn8zv10vw:depends-on:coord-task:01kn88nvmc3xnak8b7y6z7a297`: `coord-task:01kn88nwp3cqgppdhgn8zv10vw` depends on `coord-task:01kn88nvmc3xnak8b7y6z7a297`
- `plan-edge:coord-task:01kn88nwp3cqgppdhgn8zv10vw:depends-on:coord-task:01kn88nxq7nkrkmydemx7rpry1`: `coord-task:01kn88nwp3cqgppdhgn8zv10vw` depends on `coord-task:01kn88nxq7nkrkmydemx7rpry1`
- `plan-edge:coord-task:01kn88nxq7nkrkmydemx7rpry1:depends-on:coord-task:01kn88n9bxjefsmjc8pqv1dhcq`: `coord-task:01kn88nxq7nkrkmydemx7rpry1` depends on `coord-task:01kn88n9bxjefsmjc8pqv1dhcq`

