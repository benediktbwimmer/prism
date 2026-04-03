# incremental-semantic-db-runtime-engine-rewrite: transform PRISM into a true incremental semantic database with a WorkspaceRuntimeEngine actor, heavy parallel preparation and settle pipelines, coalesced edit batches, a minimal serialized commit, and snapshots limited to checkpoint recovery and query acceleration.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:3a71082b6a916c1193cecc395016d30ca4089f32b08d8b9a31dfa7010424e568`
- Source logical timestamp: `unknown`
- Source snapshot: `17 nodes, 20 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn2edq9k2cr10t7p9pwscpjz`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `17`
- Edges: `20`

## Goal

incremental-semantic-db-runtime-engine-rewrite: transform PRISM into a true incremental semantic database with a WorkspaceRuntimeEngine actor, heavy parallel preparation and settle pipelines, coalesced edit batches, a minimal serialized commit, and snapshots limited to checkpoint recovery and query acceleration.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn2edq9k2cr10t7p9pwscpjz.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn2edq9k2cr10t7p9pwscpjz.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn2edqappsmarg40z5ta9x02`
- `coord-task:01kn2qbpkx5z0n3jv4d1e4r1wm`
- `coord-task:01kn2qbpn743ksd9zb4j24njtm`
- `coord-task:01kn2qbppm2bz9w5e37rfe8cm8`

## Nodes

### Codify runtime generations, consistency envelopes, and hard budgets

- Node id: `coord-task:01kn2edqappsmarg40z5ta9x02`
- Kind: `decide`
- Status: `completed`
- Summary: Define the canonical runtime state machine up front for a bound worktree context inside a machine-scoped MCP host: working generation, published generation, settle frontier, checkpoint generation, dirty domains, domain-scoped freshness tiers, query consistency contracts, memory budgets, and latency SLOs. Make the monotonic committed delta journal a first-class part of the architecture alongside checkpoint-plus-replay recovery, preserve the three-plane persistence split explicitly, and lock in the rule that the fast path may be conservative but must never be silently misleading. The architecture contract should explicitly encode the parallel-prepare / serialized-commit / parallel-settle model as a core semantic boundary, not an implementation detail.
- Priority: `1`

#### Acceptance

- Architecture contract names the runtime generations, domain-scoped freshness tiers, committed delta journal, three-plane persistence boundaries, correctness envelope, memory budgets, and no-compromise latency targets. [any]

#### Tags

- `architecture`
- `budgets`
- `delta-journal`
- `freshness`
- `generations`
- `three-plane`

### Build WorkspaceRuntimeEngine and the coalescing-aware command model

- Node id: `coord-task:01kn2edqbj2w6gfhv8xnw00abb`
- Kind: `edit`
- Status: `completed`
- Summary: Introduce a single-writer WorkspaceRuntimeEngine per bound worktree context inside one machine-scoped MCP host that owns authoritative mutable semantic state and accepts coalescing-aware commands. The actor is the commit kernel: it owns admission, authoritative state transitions, commit ordering, generation publication, and queue-class policy, while heavy parse and analysis work stays off-actor against immutable inputs and returns deterministic deltas for commit. One daemon per machine is the default deployment shape, but the host and backend interfaces must remain multi-instance-clean so multiple daemons can safely share one machine-local runtime store without blurring per-worktree authority.
- Priority: `1`

#### Acceptance

- A worktree-scoped workspace engine exists under the default machine-scoped host shape with queue classes, coalescing, cancellation, and serialized authoritative commits, while preserving correctness if multiple local MCP daemons share the same backend runtime store. [any]

#### Tags

- `actor`
- `admission`
- `coalescing`
- `commit-kernel`
- `default-topology`
- `engine`
- `machine-host`
- `multi-instance`
- `worktree-context`

### Separate mutable engine state from published immutable generations

- Node id: `coord-task:01kn2edqccqazhjhrj7dfx4a2t`
- Kind: `edit`
- Status: `completed`
- Summary: Make published Prism generations an immutable read surface instead of the mutable working substrate. The engine should retain long-lived mutable state internally, publish structurally shared immutable generations for readers, retain only a bounded generation window, and attach explicit per-domain freshness and materialization state to every published generation so readers can distinguish file-local currentness, unsettled cross-file facts, pending reanchor work, and older but valid checkpoints.
- Priority: `1`

#### Acceptance

- Queries and diagnostics read published generations or bounded materialized derivatives, working mutations no longer rebuild from published Prism state, and retained generation count and memory shape are explicitly bounded. [any]

#### Tags

- `domain-freshness`
- `read-view`
- `runtime`
- `snapshots`
- `structural-sharing`

### Remove reconstructive refresh and ordinary live-state indexer rebuilds

- Node id: `coord-task:01kn2edqd9zwb32v5zrwrrff2z`
- Kind: `edit`
- Status: `completed`
- Summary: Remove the ordinary edit-time path that rebuilds a fresh working indexer from published Prism state. Routine refreshes must mutate long-lived engine state incrementally, so reconstructive refresh remains only as an exceptional fallback and not as the steady-state substrate.
- Priority: `1`

#### Acceptance

- Routine refreshes no longer call the reconstructive live-state indexer path, and ordinary edit handling mutates long-lived engine state incrementally. [any]

#### Tags

- `indexer`
- `rebuild-elimination`
- `refresh`

### Implement persistent per-file semantic facts and delta batches

- Node id: `coord-task:01kn2edqe64parv20k1n5k4npw`
- Kind: `edit`
- Status: `completed`
- Summary: Introduce explicit per-file semantic facts as the core mutable data model: source hash, parsed/local declarations, imports, unresolved references, local graph fragments, and lineage links. Ordinary edits should enter the engine as merged delta batches over those facts instead of snapshot-shaped refresh plans.
- Priority: `1`

#### Acceptance

- The engine persists file-local semantic facts explicitly and accepts merged delta batches instead of reconstructive refresh plans for ordinary edits. [any]

#### Tags

- `deltas`
- `file-facts`
- `semantic-db`

### Add precise invalidation and reverse dependency indexes

- Node id: `coord-task:01kn2edqf5rm8zt5aczfqrqp7f`
- Kind: `edit`
- Status: `completed`
- Summary: Build precise invalidation and reverse dependency indexes over files, symbols, packages, and downstream consumers. A changed file or delta batch should produce a narrow affected-set frontier, not broad workspace-wide resolution walks or heuristic rescans.
- Priority: `1`

#### Acceptance

- The engine can derive precise affected sets from a changed file or delta batch without broad workspace-wide invalidation heuristics. [any]

#### Tags

- `dependencies`
- `invalidation`
- `precision`

### Parallelize file-local preparation for bursty batched edits

- Node id: `coord-task:01kn2edqg7qjtwt4xmtgkwyxyz`
- Kind: `edit`
- Status: `completed`
- Summary: Make coalescing a native part of the command model and parallelize file-local preparation aggressively. Bursty editor or agent writes must collapse obsolete intermediate versions, parallelize file-local read/parse/extract work, and merge only the newest surviving deltas into the engine commit batch.
- Priority: `1`

#### Acceptance

- Many-file edit bursts are coalesced and prepared in parallel, and stale intermediate file versions are cancelled instead of fully processed. [any]

#### Tags

- `batching`
- `coalescing`
- `parallelism`
- `preparation`

### Implement the fast-path commit and layered freshness semantics

- Node id: `coord-task:01kn2edqh1zkhb8br4za6tx8ha`
- Kind: `edit`
- Status: `completed`
- Summary: Define and implement the fast-path correctness envelope: small edits publish a new generation after a minimal serialized commit, emit a monotonic committed delta batch suitable for replay and downstream consumers, and surface unsettled downstream domains explicitly through layered freshness metadata. The fast path may be conservative, but it must never be silently misleading about unsettled cross-file facts or stale derived domains.
- Priority: `1`

#### Acceptance

- Small edits publish a new generation after a short serialized commit, emit a monotonic committed delta batch for downstream consumers, and surface all unsettled semantic or diagnostics domains explicitly through freshness/materialization metadata. [any]

#### Tags

- `commit`
- `correctness`
- `delta-journal`
- `fast-path`
- `freshness`

### Add parallel settle workers, prioritization, and stale-work cancellation

- Node id: `coord-task:01kn2edqj2yfbe50m4hgy6cspx`
- Kind: `edit`
- Status: `completed`
- Summary: Run downstream recomputation in parallel settle workers outside the commit path. Prioritize the changed file, direct dependents, and likely query targets first; support stale-work cancellation when newer generations supersede older settle tasks; and expose queue depth and settle-lag telemetry as first-class runtime metrics.
- Priority: `1`

#### Acceptance

- Downstream recomputation runs in parallel outside the commit path, direct dependents are prioritized, and stale settle work is cancelled when superseded by newer generations. [any]

#### Tags

- `cancellation`
- `parallelism`
- `prioritization`
- `settle`

### Move non-authoritative side effects off commit and demote snapshots

- Node id: `coord-task:01kn2edqjy6xsjev29fs0jrs1x`
- Kind: `edit`
- Status: `completed`
- Summary: Move non-authoritative side effects fully behind the fast commit and demote snapshots to their final roles only: immutable read views, query acceleration, and checkpoint recovery. Memory reanchor, projections, curator follow-up, checkpoints, and other secondary work must no longer hold up the authoritative runtime commit.
- Priority: `1`

#### Acceptance

- Routine commits no longer wait on memory reanchor or snapshot-shaped persistence, and snapshots are no longer the authoritative mutation substrate anywhere in the ordinary edit path. [any]

#### Tags

- `checkpoints`
- `materialization`
- `side-effects`
- `snapshots`

### Replace lock-based fail-fast admission with engine-queued mutation admission

- Node id: `coord-task:01kn2edqkt2rd8zxerns99v957`
- Kind: `edit`
- Status: `completed`
- Summary: Replace the current layered fail-fast lock UX with engine-owned queued or bounded-wait admission. The authoritative engine should serialize commits, but normal user-facing mutations should no longer surface raw refresh_lock or sync_lock contention as the primary interaction model.
- Priority: `1`

#### Acceptance

- Ordinary mutation overlap is handled by engine admission and bounded waiting, and the old fail-fast refresh_lock busy semantics are no longer the normal user-facing experience. [any]

#### Tags

- `admission`
- `locks`
- `queueing`
- `ux`

### Finalize checkpoint-plus-replay recovery on the incremental core

- Node id: `coord-task:01kn2edqmq4f43nnryqptyhfjy`
- Kind: `edit`
- Status: `completed`
- Summary: Rebuild startup and crash recovery around checkpoint generations plus incremental replay on top of the new semantic database, while preserving the machine-local shared runtime store as the durability layer for shared mutable state and keeping the store boundary backend-neutral. Recovery must preserve the incremental authority model, keep replay costs bounded, support one machine-scoped host serving multiple worktree engines, and avoid reintroducing snapshot-as-authority semantics through the back door.
- Priority: `1`

#### Acceptance

- Startup and crash recovery use checkpoint-plus-replay over the incremental core while preserving a backend-neutral shared-runtime store contract that remains correct under multiple local MCP daemons sharing one runtime store and can later take Postgres without changing runtime semantics. [any]

#### Tags

- `backend-seam`
- `checkpoints`
- `durability`
- `recovery`
- `replay`
- `shared-runtime`

### Validate the performance-beast end state and remove backsliding surfaces

- Node id: `coord-task:01kn2edqnp3sj59dpc6qzv6ba0`
- Kind: `validate`
- Status: `completed`
- Summary: Validate the full performance-beast end state against hard budgets: tiny-edit fast-path latency, many-file burst throughput, queue wait time, settle lag, restart recovery time, and bounded memory. Validate generation drift behavior on handles, tool outputs, and published read surfaces; validate that unsettled domains are surfaced rather than silently hidden; and validate the machine-scoped host model with multiple worktree-scoped engines sharing one local runtime store for the same repo. Retire or clearly demote old reconstructive snapshot surfaces so the old architecture cannot drift back in unnoticed.
- Priority: `1`

#### Acceptance

- The new runtime meets the agreed interactivity, diagnostics-latency, and memory budgets, surfaces unsettled domains explicitly, and keeps runtime status and dashboard surfaces on bounded materialized read models instead of request-time reconstruction. [any]

#### Tags

- `cleanup`
- `dogfooding`
- `freshness-contracts`
- `performance`
- `shared-runtime`
- `validation`

### Preserve the three-plane persistence split and the machine-scoped shared runtime host

- Node id: `coord-task:01kn2g2ajr16mv2s6g5xtm47dv`
- Kind: `decide`
- Status: `completed`
- Summary: Preserve PRISM's three-plane persistence split as a hard architectural boundary: repo-published truth stays in `.prism`, shared mutable runtime state lives in one machine-local shared runtime database, and each active workspace or worktree gets its own authoritative WorkspaceRuntimeEngine inside one machine-scoped MCP host. The rewrite must keep the shared-runtime store backend-neutral so SQLite remains the local implementation now while Postgres can later replace it to extend the same coordination model across machines without another semantic rewrite.
- Priority: `1`

#### Acceptance

- The rewrite preserves repo truth, a single machine-local shared runtime store, and worktree-scoped engines under one machine host, with a backend-neutral shared-runtime store seam that can later take Postgres without another architecture rewrite. [any]

#### Tags

- `backend-seam`
- `machine-host`
- `shared-runtime`
- `three-plane`
- `worktree-context`

### Build an incremental diagnostics and runtime read-model plane

- Node id: `coord-task:01kn2qbpkx5z0n3jv4d1e4r1wm`
- Kind: `edit`
- Status: `completed`
- Summary: Completed the incremental diagnostics and runtime read-model plane by ensuring runtime status and dashboard reads stay on cached/materialized state from first host startup onward. WorkspaceRuntimeBinding now seeds cached runtime status synchronously after persisted-state hydration, request paths remain read-only, and the node validates cleanly with the new startup-seeding regression plus a green cargo test --workspace and healthy rebuilt release daemon.

#### Acceptance

- The host owns bounded materialized read models for runtime status, dashboard summary, and recent operation state that are fed incrementally from engine commits, process lifecycle events, and operation events instead of being rebuilt on request. [any]

#### Tags

- `dashboard`
- `diagnostics`
- `materialized-view`
- `read-models`
- `runtime-status`

### Implement a cheap machine-scoped process and instance ledger for diagnostics

- Node id: `coord-task:01kn2qbpn743ksd9zb4j24njtm`
- Kind: `edit`
- Status: `completed`
- Summary: Implemented a cheap machine-scoped diagnostics process ledger by pruning dead runtime-state process records on write, filtering runtime-status process views to live PIDs only, and aligning the MCP runtime status surface with live-process semantics instead of stale runtime-state entries. Also updated the stale compact locate task-scope test expectation and revalidated with cargo test --workspace plus release rebuild/restart.

#### Acceptance

- Runtime process and daemon/bridge status comes from an instance-aware process ledger with bounded liveness checks, not broad request-time ps scans, while staying correct for one or many local MCP daemons sharing the same runtime store. [any]

#### Tags

- `instance-ledger`
- `liveness`
- `multi-instance`
- `processes`
- `shared-runtime`

### Route runtimeStatus, dashboard summary, and recent operation views to materialized read models

- Node id: `coord-task:01kn2qbppm2bz9w5e37rfe8cm8`
- Kind: `edit`
- Status: `completed`
- Summary: runtimeStatus, dashboard summary, and recent operation views now read bounded diagnostics snapshots instead of falling through to request-time ps or lsof scans, workspace walks, coordination graph reconstruction, or log reparsing. The diagnostics plane now serves the common request path while exposing freshness and lag through cached read models.

#### Acceptance

- Common diagnostics surfaces no longer perform request-time workspace walks, plan graph reconstruction, query-log file reparsing, or broad process scans; they read bounded materialized views that expose explicit domain-scoped freshness and lag state. [any]

#### Tags

- `dashboard`
- `diagnostics`
- `request-path`
- `runtime-status`
- `slow-path-removal`

## Edges

- `plan-edge:coord-task:01kn2edqbj2w6gfhv8xnw00abb:depends-on:coord-task:01kn2edqappsmarg40z5ta9x02`: `coord-task:01kn2edqbj2w6gfhv8xnw00abb` depends on `coord-task:01kn2edqappsmarg40z5ta9x02`
- `plan-edge:coord-task:01kn2edqbj2w6gfhv8xnw00abb:depends-on:coord-task:01kn2g2ajr16mv2s6g5xtm47dv`: `coord-task:01kn2edqbj2w6gfhv8xnw00abb` depends on `coord-task:01kn2g2ajr16mv2s6g5xtm47dv`
- `plan-edge:coord-task:01kn2edqccqazhjhrj7dfx4a2t:depends-on:coord-task:01kn2edqbj2w6gfhv8xnw00abb`: `coord-task:01kn2edqccqazhjhrj7dfx4a2t` depends on `coord-task:01kn2edqbj2w6gfhv8xnw00abb`
- `plan-edge:coord-task:01kn2edqd9zwb32v5zrwrrff2z:depends-on:coord-task:01kn2edqccqazhjhrj7dfx4a2t`: `coord-task:01kn2edqd9zwb32v5zrwrrff2z` depends on `coord-task:01kn2edqccqazhjhrj7dfx4a2t`
- `plan-edge:coord-task:01kn2edqe64parv20k1n5k4npw:depends-on:coord-task:01kn2edqd9zwb32v5zrwrrff2z`: `coord-task:01kn2edqe64parv20k1n5k4npw` depends on `coord-task:01kn2edqd9zwb32v5zrwrrff2z`
- `plan-edge:coord-task:01kn2edqf5rm8zt5aczfqrqp7f:depends-on:coord-task:01kn2edqe64parv20k1n5k4npw`: `coord-task:01kn2edqf5rm8zt5aczfqrqp7f` depends on `coord-task:01kn2edqe64parv20k1n5k4npw`
- `plan-edge:coord-task:01kn2edqg7qjtwt4xmtgkwyxyz:depends-on:coord-task:01kn2edqe64parv20k1n5k4npw`: `coord-task:01kn2edqg7qjtwt4xmtgkwyxyz` depends on `coord-task:01kn2edqe64parv20k1n5k4npw`
- `plan-edge:coord-task:01kn2edqh1zkhb8br4za6tx8ha:depends-on:coord-task:01kn2edqf5rm8zt5aczfqrqp7f`: `coord-task:01kn2edqh1zkhb8br4za6tx8ha` depends on `coord-task:01kn2edqf5rm8zt5aczfqrqp7f`
- `plan-edge:coord-task:01kn2edqh1zkhb8br4za6tx8ha:depends-on:coord-task:01kn2edqg7qjtwt4xmtgkwyxyz`: `coord-task:01kn2edqh1zkhb8br4za6tx8ha` depends on `coord-task:01kn2edqg7qjtwt4xmtgkwyxyz`
- `plan-edge:coord-task:01kn2edqj2yfbe50m4hgy6cspx:depends-on:coord-task:01kn2edqh1zkhb8br4za6tx8ha`: `coord-task:01kn2edqj2yfbe50m4hgy6cspx` depends on `coord-task:01kn2edqh1zkhb8br4za6tx8ha`
- `plan-edge:coord-task:01kn2edqjy6xsjev29fs0jrs1x:depends-on:coord-task:01kn2edqh1zkhb8br4za6tx8ha`: `coord-task:01kn2edqjy6xsjev29fs0jrs1x` depends on `coord-task:01kn2edqh1zkhb8br4za6tx8ha`
- `plan-edge:coord-task:01kn2edqjy6xsjev29fs0jrs1x:depends-on:coord-task:01kn2edqj2yfbe50m4hgy6cspx`: `coord-task:01kn2edqjy6xsjev29fs0jrs1x` depends on `coord-task:01kn2edqj2yfbe50m4hgy6cspx`
- `plan-edge:coord-task:01kn2edqkt2rd8zxerns99v957:depends-on:coord-task:01kn2edqbj2w6gfhv8xnw00abb`: `coord-task:01kn2edqkt2rd8zxerns99v957` depends on `coord-task:01kn2edqbj2w6gfhv8xnw00abb`
- `plan-edge:coord-task:01kn2edqkt2rd8zxerns99v957:depends-on:coord-task:01kn2edqh1zkhb8br4za6tx8ha`: `coord-task:01kn2edqkt2rd8zxerns99v957` depends on `coord-task:01kn2edqh1zkhb8br4za6tx8ha`
- `plan-edge:coord-task:01kn2edqmq4f43nnryqptyhfjy:depends-on:coord-task:01kn2edqjy6xsjev29fs0jrs1x`: `coord-task:01kn2edqmq4f43nnryqptyhfjy` depends on `coord-task:01kn2edqjy6xsjev29fs0jrs1x`
- `plan-edge:coord-task:01kn2edqnp3sj59dpc6qzv6ba0:depends-on:coord-task:01kn2edqj2yfbe50m4hgy6cspx`: `coord-task:01kn2edqnp3sj59dpc6qzv6ba0` depends on `coord-task:01kn2edqj2yfbe50m4hgy6cspx`
- `plan-edge:coord-task:01kn2edqnp3sj59dpc6qzv6ba0:depends-on:coord-task:01kn2edqjy6xsjev29fs0jrs1x`: `coord-task:01kn2edqnp3sj59dpc6qzv6ba0` depends on `coord-task:01kn2edqjy6xsjev29fs0jrs1x`
- `plan-edge:coord-task:01kn2edqnp3sj59dpc6qzv6ba0:depends-on:coord-task:01kn2edqkt2rd8zxerns99v957`: `coord-task:01kn2edqnp3sj59dpc6qzv6ba0` depends on `coord-task:01kn2edqkt2rd8zxerns99v957`
- `plan-edge:coord-task:01kn2edqnp3sj59dpc6qzv6ba0:depends-on:coord-task:01kn2edqmq4f43nnryqptyhfjy`: `coord-task:01kn2edqnp3sj59dpc6qzv6ba0` depends on `coord-task:01kn2edqmq4f43nnryqptyhfjy`
- `plan-edge:coord-task:01kn2g2ajr16mv2s6g5xtm47dv:depends-on:coord-task:01kn2edqappsmarg40z5ta9x02`: `coord-task:01kn2g2ajr16mv2s6g5xtm47dv` depends on `coord-task:01kn2edqappsmarg40z5ta9x02`

