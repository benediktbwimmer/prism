# Implement the projections architecture from docs/PRISM_PROJECTIONS.md by making authority planes explicit, introducing first-class projection contracts and freshness semantics, separating published/serving/ad-hoc projections cleanly from authored truth, and exposing stable projection-oriented read surfaces across PRISM docs, MCP, CLI, and query/runtime layers.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:bd5f064b0b28d5f0fe8dca28c641868c68b1a7d847b3ede8143bc5ad756fdaec`
- Source logical timestamp: `unknown`
- Source snapshot: `8 nodes, 17 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn7kc7wd38p9kb8src073kpp`
- Status: `completed`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `8`
- Edges: `17`

## Goal

Implement the projections architecture from docs/PRISM_PROJECTIONS.md by making authority planes explicit, introducing first-class projection contracts and freshness semantics, separating published/serving/ad-hoc projections cleanly from authored truth, and exposing stable projection-oriented read surfaces across PRISM docs, MCP, CLI, and query/runtime layers.

## Source of Truth

- Index path: `.prism/plans/index.jsonl`
- Log path: `.prism/plans/streams/plan:01kn7kc7wd38p9kb8src073kpp.jsonl`

## Root Nodes

- `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`

## Nodes

### Lock projection contracts, authority planes, and freshness invariants

- Node id: `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`
- Kind: `edit`
- Status: `completed`
- Summary: Locked the projections contract in docs by making docs/PRISM_PROJECTIONS.md normative, adding explicit authority-plane and projection-class rules to docs/SPEC.md, and tightening docs/PERSISTENCE_STATE_CLASSIFICATION.md so projection rebuildability and freshness expectations are explicit before interface work begins.
- Priority: `1`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Authority-plane responsibilities are concrete enough to tell whether state belongs in published repo logs, shared runtime authority, or derived serving projections. [any]
- Projection classes and freshness semantics are explicit enough to guide schema and interface work without ambiguity about what is authoritative. [any]

#### Validation Refs

- `docs/PRISM_PROJECTIONS.md`
- `docs/SPEC.md`

### Introduce first-class projection metadata and read-model contracts

- Node id: `coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf`
- Kind: `edit`
- Status: `completed`
- Summary: Introduced first-class projection contract types in prism-projections, mirrored them into prism-js schema types, and surfaced them live through runtimeStatus() so projection scopes now declare class, authority planes, freshness, materialization, and per-read-model metadata without conflating derived read models with authored authority.
- Priority: `2`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Projection-facing types can distinguish source authority planes, freshness state, and projection class without leaking write-authority semantics into the read model. [any]
- The contract is reusable across query/runtime, MCP, CLI, and generated documentation surfaces. [any]

#### Validation Refs

- `crates/prism-ir`
- `crates/prism-query`

### Implement published projection generation and source stamping

- Node id: `coord-task:01kn7kd59d7m289c02x11e3z0p`
- Kind: `edit`
- Status: `completed`
- Summary: Added deterministic projection metadata stamping to the prism_doc generator and regenerated PRISM.md plus docs/prism/* so published projections now declare version, authority plane, source head, logical source timestamp, and source snapshot counts.
- Priority: `3`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Generated artifacts carry enough source metadata to debug freshness and provenance of the published projection output. [any]
- Published docs are generated as projections over published PRISM knowledge rather than treated as hand-authored authority. [any]

#### Validation Refs

- `PRISM.md`
- `docs/prism/concepts.md`
- `docs/prism/contracts.md`

### Build serving projection read models with explicit freshness

- Node id: `coord-task:01kn7kd5vsbjjt95a5m6axdcpk`
- Kind: `edit`
- Status: `completed`
- Summary: Built first-class serving projection scope/read-model contracts, moved serving projection derivation out of runtime_views, and validated explicit freshness/materialization reporting through targeted, package, workspace, and live daemon checks.
- Priority: `4`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Serving projections can declare which authority planes they read from and whether the result is current, stale, deferred, or partially hydrated. [any]
- The serving path remains derived and refreshable rather than becoming a hidden second authority. [any]

#### Validation Refs

- `crates/prism-core`
- `crates/prism-mcp`
- `crates/prism-query`

### Add ad hoc projection requests for time-travel and diff views

- Node id: `coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5`
- Kind: `edit`
- Status: `completed`
- Summary: Added first-class ad hoc plan projection requests in prism-query for point-in-time and diff views by replaying coordination history into raw historical plan graphs and execution overlays, plus documented the structural replay semantics and latency honesty contract in PRISM_PROJECTIONS.md.
- Priority: `5`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Historical read paths state replay and latency expectations explicitly rather than implying all time-travel views are cheap. [any]
- There is a clear request model for time-scoped and diff-scoped projection reads over plans or coordination state. [any]

#### Validation Refs

- `crates/prism-query`
- `docs/PRISM_PROJECTIONS.md`

### Expose projection-oriented MCP and CLI surfaces

- Node id: `coord-task:01kn7kd70a3ba70rv75vj27nbk`
- Kind: `edit`
- Status: `completed`
- Summary: Exposed ad hoc projection reads as first-class MCP/query and CLI surfaces via prism.planProjectionAt(...), prism.planProjectionDiff(...), and the new `prism project <plan-id> --at/--diff` command, with prism-js type/docs updates, live daemon validation, and workflow capability registration.
- Priority: `6`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- MCP and CLI surfaces use projection terminology consistently and expose freshness or source-plane information where it matters. [any]
- The resulting interfaces make projection requests first-class without confusing them for write-authority mutations. [any]

#### Validation Refs

- `crates/prism-cli`
- `crates/prism-mcp`

### Integrate projection persistence, invalidation, and refresh boundaries

- Node id: `coord-task:01kn7kd7jzw8ss2yj734zffnv1`
- Kind: `edit`
- Status: `completed`
- Summary: Added explicit persisted projection materialization metadata in prism-store, switched startup and recovery hydration to use those boundaries instead of coarse derived-row heuristics, and validated that incomplete projection coverage no longer masquerades as a full snapshot while bounded hot-outcome reload behavior remains intact.
- Priority: `7`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Persisted projection accelerators can be invalidated and rebuilt without losing authoritative correctness. [any]
- Projection refresh boundaries are explicit enough to avoid stale serving state silently acting as truth. [any]

#### Validation Refs

- `crates/prism-core`
- `crates/prism-mcp`
- `crates/prism-store`

### Validate deterministic rebuilds, cold-clone behavior, and latency/freshness guarantees

- Node id: `coord-task:01kn7kd857b3amezy4qd3v253j`
- Kind: `edit`
- Status: `completed`
- Summary: Validated the projections rollout with targeted prism-store and prism-core regressions, green prism-store/prism-core/prism-mcp package suites, a full workspace suite whose only failure was the known parallel query-refresh flake that passed immediately in isolation, and fresh release-binary restart/status/health checks against the live MCP daemon.
- Priority: `8`
- Assignee: `codex-runtime-storage-boundary-redesign-2026-04-01`

#### Acceptance

- Validation covers targeted tests, full workspace validation, rebuilt release binaries, daemon restart, and live surface checks for projection freshness semantics. [any]
- Validation proves deterministic rebuilds from authoritative state, including fresh-clone or rebuilt-runtime scenarios. [any]

#### Validation Refs

- `./target/release/prism-cli mcp health`
- `./target/release/prism-cli mcp restart --internal-developer`
- `./target/release/prism-cli mcp status`
- `cargo build --release -p prism-cli -p prism-mcp`
- `cargo test --workspace --quiet`

## Edges

- `plan-edge:coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf:depends-on:coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`: `coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf` depends on `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`
- `plan-edge:coord-task:01kn7kd59d7m289c02x11e3z0p:depends-on:coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`: `coord-task:01kn7kd59d7m289c02x11e3z0p` depends on `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`
- `plan-edge:coord-task:01kn7kd59d7m289c02x11e3z0p:depends-on:coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf`: `coord-task:01kn7kd59d7m289c02x11e3z0p` depends on `coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf`
- `plan-edge:coord-task:01kn7kd5vsbjjt95a5m6axdcpk:depends-on:coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`: `coord-task:01kn7kd5vsbjjt95a5m6axdcpk` depends on `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`
- `plan-edge:coord-task:01kn7kd5vsbjjt95a5m6axdcpk:depends-on:coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf`: `coord-task:01kn7kd5vsbjjt95a5m6axdcpk` depends on `coord-task:01kn7kd4qdqgr8t6s4yt4rn1sf`
- `plan-edge:coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5:depends-on:coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`: `coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5` depends on `coord-task:01kn7kcgg7q9qszsfsb3j2vs7s`
- `plan-edge:coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5:depends-on:coord-task:01kn7kd5vsbjjt95a5m6axdcpk`: `coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5` depends on `coord-task:01kn7kd5vsbjjt95a5m6axdcpk`
- `plan-edge:coord-task:01kn7kd70a3ba70rv75vj27nbk:depends-on:coord-task:01kn7kd59d7m289c02x11e3z0p`: `coord-task:01kn7kd70a3ba70rv75vj27nbk` depends on `coord-task:01kn7kd59d7m289c02x11e3z0p`
- `plan-edge:coord-task:01kn7kd70a3ba70rv75vj27nbk:depends-on:coord-task:01kn7kd5vsbjjt95a5m6axdcpk`: `coord-task:01kn7kd70a3ba70rv75vj27nbk` depends on `coord-task:01kn7kd5vsbjjt95a5m6axdcpk`
- `plan-edge:coord-task:01kn7kd70a3ba70rv75vj27nbk:depends-on:coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5`: `coord-task:01kn7kd70a3ba70rv75vj27nbk` depends on `coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5`
- `plan-edge:coord-task:01kn7kd7jzw8ss2yj734zffnv1:depends-on:coord-task:01kn7kd5vsbjjt95a5m6axdcpk`: `coord-task:01kn7kd7jzw8ss2yj734zffnv1` depends on `coord-task:01kn7kd5vsbjjt95a5m6axdcpk`
- `plan-edge:coord-task:01kn7kd7jzw8ss2yj734zffnv1:depends-on:coord-task:01kn7kd70a3ba70rv75vj27nbk`: `coord-task:01kn7kd7jzw8ss2yj734zffnv1` depends on `coord-task:01kn7kd70a3ba70rv75vj27nbk`
- `plan-edge:coord-task:01kn7kd857b3amezy4qd3v253j:depends-on:coord-task:01kn7kd59d7m289c02x11e3z0p`: `coord-task:01kn7kd857b3amezy4qd3v253j` depends on `coord-task:01kn7kd59d7m289c02x11e3z0p`
- `plan-edge:coord-task:01kn7kd857b3amezy4qd3v253j:depends-on:coord-task:01kn7kd5vsbjjt95a5m6axdcpk`: `coord-task:01kn7kd857b3amezy4qd3v253j` depends on `coord-task:01kn7kd5vsbjjt95a5m6axdcpk`
- `plan-edge:coord-task:01kn7kd857b3amezy4qd3v253j:depends-on:coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5`: `coord-task:01kn7kd857b3amezy4qd3v253j` depends on `coord-task:01kn7kd6e5d4bcd4vy3jqzhtc5`
- `plan-edge:coord-task:01kn7kd857b3amezy4qd3v253j:depends-on:coord-task:01kn7kd70a3ba70rv75vj27nbk`: `coord-task:01kn7kd857b3amezy4qd3v253j` depends on `coord-task:01kn7kd70a3ba70rv75vj27nbk`
- `plan-edge:coord-task:01kn7kd857b3amezy4qd3v253j:depends-on:coord-task:01kn7kd7jzw8ss2yj734zffnv1`: `coord-task:01kn7kd857b3amezy4qd3v253j` depends on `coord-task:01kn7kd7jzw8ss2yj734zffnv1`

