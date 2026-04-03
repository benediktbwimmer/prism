# Rewrite the PRISM runtime around incremental invalidation, partial materialization, explicit boundary semantics, worktree-aware overlays, lazy deep parsing, and scalable ranking so the daemon stays radically efficient on small repos and scales cleanly to large monorepos, excluding the remote shared runtime backend and database-pushed traversals.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:3b47441d7493210256e870e436443d545891c8c61d2fd4403815327945c44671`
- Source logical timestamp: `unknown`
- Source snapshot: `10 nodes, 26 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn09gyzw4e8wzwvgvbbj9mkk`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `10`
- Edges: `26`

## Goal

Rewrite the PRISM runtime around incremental invalidation, partial materialization, explicit boundary semantics, worktree-aware overlays, lazy deep parsing, and scalable ranking so the daemon stays radically efficient on small repos and scales cleanly to large monorepos, excluding the remote shared runtime backend and database-pushed traversals.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn09gyzw4e8wzwvgvbbj9mkk.jsonl`

## Root Nodes

- `coord-task:01kn09je3c546kafd2z7bcgnyq`

## Nodes

### Define the runtime rewrite contracts and migration invariants

- Node id: `coord-task:01kn09je3c546kafd2z7bcgnyq`
- Kind: `decide`
- Status: `completed`

### Complete the incremental invalidation substrate with formal dependency-aware propagation and localized reprojection

- Node id: `coord-task:01kn09je49f0ncgdyh2jng7whm`
- Kind: `edit`
- Status: `completed`

### Replace broad runtime reloads with region-scoped incremental mutation and bounded hot-state hydration

- Node id: `coord-task:01kn09je51g42y7gg7cph5144f`
- Kind: `edit`
- Status: `completed`

### Introduce partial materialization and explicit index-depth tiers across the runtime

- Node id: `coord-task:01kn09je5pxad8grjdhf7hata8`
- Kind: `edit`
- Status: `completed`
- Summary: Explicit runtime depth tiers are now surfaced live through runtime status with conservative workspace coverage counts, making the existing partial-materialization boundary visible end-to-end.

### Add explicit boundary-node semantics for non-materialized, sparse, and out-of-scope regions

- Node id: `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- Kind: `edit`
- Status: `completed`
- Summary: Starting the boundary-semantics pass by surveying how the runtime currently represents known-but-not-materialized regions and where explicit boundary labels should live.

### Move history, outcomes, curator, and analytical evidence onto warm and cold lazy-read paths

- Node id: `coord-task:01kn09jea5pj0zgsd2m6n9c0f2`
- Kind: `edit`
- Status: `completed`
- Summary: Curator, lineage history, task replay, outcome recall, and analytical evidence now use warm/cold lazy-read paths; startup hydrates bounded hot state and defers full lineage/outcome logs to SQLite-backed backends.

### Make runtime projections and overlays explicitly repo-, worktree-, and session-scoped

- Node id: `coord-task:01kn09jeavcan93vyc9z9jvfkm`
- Kind: `edit`
- Status: `completed`
- Summary: Runtime status and runtime inspection now expose explicit repo, worktree, and session scope slices for projections and overlays, using live coordination overlays instead of flattened published state.

### Add lazy JIT deep parsing and selective structural enrichment on first touch

- Node id: `coord-task:01kn09jebspnr1xzpwtypb938p`
- Kind: `edit`
- Status: `completed`
- Summary: Persisted per-file parse depth, made large full reindexes shallow by default, and added on-demand deepening through semantic prism_open first touch with runtime logging.

### Add scalable architectural ranking signals for broad-query and impact precision

- Node id: `coord-task:01kn09jecg1haxe1vh7a7t13yd`
- Kind: `edit`
- Status: `in_progress`
- Summary: Next focus is ranking quality on broad queries and impact reads now that runtime materialization can stay shallow until deeper structure is actually needed.

### Measure and validate the rewrite against startup, incremental latency, memory, and retrieval quality

- Node id: `coord-task:01kn09jed6ybpsksq7d7sp1phw`
- Kind: `validate`
- Status: `proposed`

## Edges

- `plan-edge:coord-task:01kn09je49f0ncgdyh2jng7whm:depends-on:coord-task:01kn09je3c546kafd2z7bcgnyq`: `coord-task:01kn09je49f0ncgdyh2jng7whm` depends on `coord-task:01kn09je3c546kafd2z7bcgnyq`
- `plan-edge:coord-task:01kn09je51g42y7gg7cph5144f:depends-on:coord-task:01kn09je3c546kafd2z7bcgnyq`: `coord-task:01kn09je51g42y7gg7cph5144f` depends on `coord-task:01kn09je3c546kafd2z7bcgnyq`
- `plan-edge:coord-task:01kn09je51g42y7gg7cph5144f:depends-on:coord-task:01kn09je49f0ncgdyh2jng7whm`: `coord-task:01kn09je51g42y7gg7cph5144f` depends on `coord-task:01kn09je49f0ncgdyh2jng7whm`
- `plan-edge:coord-task:01kn09je5pxad8grjdhf7hata8:depends-on:coord-task:01kn09je3c546kafd2z7bcgnyq`: `coord-task:01kn09je5pxad8grjdhf7hata8` depends on `coord-task:01kn09je3c546kafd2z7bcgnyq`
- `plan-edge:coord-task:01kn09je8z6tgxpsxh2f115wb2:depends-on:coord-task:01kn09je3c546kafd2z7bcgnyq`: `coord-task:01kn09je8z6tgxpsxh2f115wb2` depends on `coord-task:01kn09je3c546kafd2z7bcgnyq`
- `plan-edge:coord-task:01kn09je8z6tgxpsxh2f115wb2:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09je8z6tgxpsxh2f115wb2` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jea5pj0zgsd2m6n9c0f2:depends-on:coord-task:01kn09je51g42y7gg7cph5144f`: `coord-task:01kn09jea5pj0zgsd2m6n9c0f2` depends on `coord-task:01kn09je51g42y7gg7cph5144f`
- `plan-edge:coord-task:01kn09jea5pj0zgsd2m6n9c0f2:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09jea5pj0zgsd2m6n9c0f2` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jea5pj0zgsd2m6n9c0f2:depends-on:coord-task:01kn09je8z6tgxpsxh2f115wb2`: `coord-task:01kn09jea5pj0zgsd2m6n9c0f2` depends on `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- `plan-edge:coord-task:01kn09jeavcan93vyc9z9jvfkm:depends-on:coord-task:01kn09je3c546kafd2z7bcgnyq`: `coord-task:01kn09jeavcan93vyc9z9jvfkm` depends on `coord-task:01kn09je3c546kafd2z7bcgnyq`
- `plan-edge:coord-task:01kn09jeavcan93vyc9z9jvfkm:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09jeavcan93vyc9z9jvfkm` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jeavcan93vyc9z9jvfkm:depends-on:coord-task:01kn09je8z6tgxpsxh2f115wb2`: `coord-task:01kn09jeavcan93vyc9z9jvfkm` depends on `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- `plan-edge:coord-task:01kn09jebspnr1xzpwtypb938p:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09jebspnr1xzpwtypb938p` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jebspnr1xzpwtypb938p:depends-on:coord-task:01kn09je8z6tgxpsxh2f115wb2`: `coord-task:01kn09jebspnr1xzpwtypb938p` depends on `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- `plan-edge:coord-task:01kn09jebspnr1xzpwtypb938p:depends-on:coord-task:01kn09jeavcan93vyc9z9jvfkm`: `coord-task:01kn09jebspnr1xzpwtypb938p` depends on `coord-task:01kn09jeavcan93vyc9z9jvfkm`
- `plan-edge:coord-task:01kn09jecg1haxe1vh7a7t13yd:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09jecg1haxe1vh7a7t13yd` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jecg1haxe1vh7a7t13yd:depends-on:coord-task:01kn09je8z6tgxpsxh2f115wb2`: `coord-task:01kn09jecg1haxe1vh7a7t13yd` depends on `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- `plan-edge:coord-task:01kn09jecg1haxe1vh7a7t13yd:depends-on:coord-task:01kn09jebspnr1xzpwtypb938p`: `coord-task:01kn09jecg1haxe1vh7a7t13yd` depends on `coord-task:01kn09jebspnr1xzpwtypb938p`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09je49f0ncgdyh2jng7whm`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09je49f0ncgdyh2jng7whm`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09je51g42y7gg7cph5144f`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09je51g42y7gg7cph5144f`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09je5pxad8grjdhf7hata8`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09je5pxad8grjdhf7hata8`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09je8z6tgxpsxh2f115wb2`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09je8z6tgxpsxh2f115wb2`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09jea5pj0zgsd2m6n9c0f2`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09jea5pj0zgsd2m6n9c0f2`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09jeavcan93vyc9z9jvfkm`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09jeavcan93vyc9z9jvfkm`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09jebspnr1xzpwtypb938p`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09jebspnr1xzpwtypb938p`
- `plan-edge:coord-task:01kn09jed6ybpsksq7d7sp1phw:depends-on:coord-task:01kn09jecg1haxe1vh7a7t13yd`: `coord-task:01kn09jed6ybpsksq7d7sp1phw` depends on `coord-task:01kn09jecg1haxe1vh7a7t13yd`

