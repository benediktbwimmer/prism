# Rewrite PRISM co-change persistence and refresh materialization boundaries so authoritative runtime state, bounded serving projections, and cold analytical evidence are separated; eliminate pathological co-change growth from the daemon hot path; and migrate existing cache state without regressing correctness, startup time, or daemon health.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:6810fd767a208babd99f70d11b3fc41655460891b8ba9707e76d670c77362b8c`
- Source logical timestamp: `unknown`
- Source snapshot: `7 nodes, 10 edges, 0 overlays`

## Overview

- Plan id: `plan:01kn02m1dek35hmmpsxmaw5yd7`
- Status: `archived`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `7`
- Edges: `10`

## Goal

Rewrite PRISM co-change persistence and refresh materialization boundaries so authoritative runtime state, bounded serving projections, and cold analytical evidence are separated; eliminate pathological co-change growth from the daemon hot path; and migrate existing cache state without regressing correctness, startup time, or daemon health.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kn02m1dek35hmmpsxmaw5yd7.json`
- Legacy migration log path: `.prism/plans/streams/plan:01kn02m1dek35hmmpsxmaw5yd7.jsonl` (compatibility only, not current tracked authority)

## Root Nodes

- `coord-task:01kn02mwy8twwr8mcjrc6w60qn`

## Nodes

### Capture migration invariants and baseline the current failure mode

- Node id: `coord-task:01kn02mwy8twwr8mcjrc6w60qn`
- Kind: `investigate`
- Status: `completed`
- Summary: Baseline and migration invariants are captured in docs/CO_CHANGE_RUNTIME_REWRITE.md, including current cache and daemon metrics, the pathological 3.49M co-change delta refresh, and the required boundary split between authoritative state, bounded serving projections, and cold analytical evidence.

#### Bindings

- Anchor: `file:104`
- Anchor: `file:231`
- Anchor: `file:260`
- Anchor: `file:275`
- Anchor: `file:281`
- Anchor: `file:74`

#### Acceptance

- Documents the current pathological path from refresh to co-change persistence to daemon health degradation [any]
- Defines the target separation between authoritative runtime state, bounded serving projections, and cold analytical evidence [any]
- Captures baseline measurements for cache growth, startup load, refresh latency, and daemon RSS [any]

#### Tags

- `architecture`
- `baseline`
- `co-change`
- `runtime`

### Add immediate guardrails against pathological co-change batches

- Node id: `coord-task:01kn02nzcts148twpb0t9ssxp6`
- Kind: `edit`
- Status: `completed`
- Summary: The co-change rewrite removed the hot-path source of pathological pairwise writes, so large refreshes no longer persist unbounded symbol-pair history and daemon responsiveness no longer depends on short-term guardrail skips.

#### Bindings

- Anchor: `file:231`
- Anchor: `file:263`
- Anchor: `file:74`

#### Acceptance

- Large or bulk refreshes no longer emit pathological co-change delta volumes on the hot path [any]
- Containment behavior is explicit and observable in logs or diagnostics [any]
- The daemon remains responsive while the broader migration is still incomplete [any]

#### Tags

- `co-change`
- `containment`
- `runtime`

### Remove cold co-change history from hot runtime hydration

- Node id: `coord-task:01kn02nzdnkxkz5dc3fbmgjfkb`
- Kind: `edit`
- Status: `completed`
- Summary: Startup hydration now loads authoritative history and bounded serving projections only; legacy unbounded co-change history is no longer hydrated into the live daemon, which materially reduced restart memory and startup latency on the previously oversized cache DB.

#### Bindings

- Anchor: `file:104`
- Anchor: `file:260`
- Anchor: `file:74`
- Anchor: `file:86`

#### Acceptance

- Daemon startup no longer hydrates unbounded history co-change rows into in-memory runtime state [any]
- Hot runtime memory is bounded by authoritative state plus serving projections rather than cold analytical history [any]
- Startup and restart latency improve measurably on an existing large cache DB [any]

#### Tags

- `history`
- `hydration`
- `memory`
- `startup`

### Introduce a cold evidence model and bounded projection materialization pipeline

- Node id: `coord-task:01kn02nzedf9pa1689y558kzk2`
- Kind: `edit`
- Status: `completed`
- Summary: Unbounded persisted pairwise co-change history was removed from authoritative state; the store now keeps plain lineage history and derives bounded serving co-change from colder evidence instead of hydrating or persisting an unbounded historical pair table.

#### Bindings

- Anchor: `file:231`
- Anchor: `file:260`
- Anchor: `file:265`
- Anchor: `file:275`

#### Acceptance

- Authoritative state and analytical co-change evidence are persisted in separate layers with clear ownership [any]
- Serving-side co-change projection remains bounded and can be refreshed without full historical hydration [any]
- The new model preserves enough evidence to derive co-change behavior without quadratic hot-path writes [any]

#### Tags

- `analytics`
- `materialization`
- `persistence`
- `projection`

### Migrate and compact legacy cache state safely

- Node id: `coord-task:01kn02nzf7ftmveaznb5hh2r4j`
- Kind: `edit`
- Status: `completed`
- Summary: Store open now retires legacy history_co_change state, records the migration, and vacuums when reclaim is large enough; on the live cache this shrank the DB from roughly 2.25 GB to roughly 278 MB while preserving startup correctness.

#### Bindings

- Anchor: `file:260`
- Anchor: `file:263`
- Anchor: `file:266`

#### Acceptance

- Existing oversized cache DBs can migrate to the new shape without losing authoritative runtime correctness [any]
- Legacy co-change data is compacted, retired, or rebuilt according to the new architecture [any]
- Post-migration database size and freelist growth are materially reduced [any]

#### Tags

- `compaction`
- `migration`
- `sqlite`

### Harden refresh, locking, and freshness behavior around long-running persistence work

- Node id: `coord-task:01kn02nzgqcvhrahmb2wt92gfw`
- Kind: `edit`
- Status: `completed`
- Summary: Runtime refresh and freshness now treat bounded lag as a deferred state instead of a daemon failure: workspace-runtime snapshot loaders use pure-read session helpers, transient SQLite lock contention in the background refresh loop is downgraded to deferred refresh bookkeeping, and runtimeStatus reports health ok while freshness surfaces deferred lag explicitly.

#### Bindings

- Anchor: `file:206`
- Anchor: `file:263`
- Anchor: `file:281`
- Anchor: `file:86`

#### Acceptance

- Background refresh no longer repeatedly fails with database-locked errors during expected long-running maintenance or projection work [any]
- Daemon health and freshness reporting distinguish bounded lag from hard failure [any]
- Request paths remain orientation-giving even while derived state is catching up [any]

#### Tags

- `freshness`
- `health`
- `locking`
- `runtime`

### Validate the migration against daemon health, performance, and correctness

- Node id: `coord-task:01kn02nzhkyych6dhta2d3he9x`
- Kind: `validate`
- Status: `completed`
- Summary: The migration has now been validated end to end: required prism-js and prism-mcp checks passed on the rewritten co-change architecture, the release prism-cli/prism-mcp build succeeded, the restarted daemon is healthy, and native plan completion bookkeeping now accepts task-correlated validation outcomes for native node ids without requiring anchors or a backing coordination task.

#### Bindings

- Anchor: `file:198`
- Anchor: `file:269`
- Anchor: `file:284`
- Anchor: `file:90`

#### Acceptance

- Representative tests cover startup, restart, refresh, and migration behavior under large historical state [any]
- Measured daemon RSS, cache DB growth, and refresh latency improve against the recorded baseline [any]
- No correctness regressions appear in lineage, projection, or query behavior after migration [any]

#### Tags

- `health`
- `performance`
- `validation`

## Edges

- `plan-edge:coord-task:01kn02nzcts148twpb0t9ssxp6:depends-on:coord-task:01kn02mwy8twwr8mcjrc6w60qn`: `coord-task:01kn02nzcts148twpb0t9ssxp6` depends on `coord-task:01kn02mwy8twwr8mcjrc6w60qn`
- `plan-edge:coord-task:01kn02nzdnkxkz5dc3fbmgjfkb:depends-on:coord-task:01kn02mwy8twwr8mcjrc6w60qn`: `coord-task:01kn02nzdnkxkz5dc3fbmgjfkb` depends on `coord-task:01kn02mwy8twwr8mcjrc6w60qn`
- `plan-edge:coord-task:01kn02nzedf9pa1689y558kzk2:depends-on:coord-task:01kn02mwy8twwr8mcjrc6w60qn`: `coord-task:01kn02nzedf9pa1689y558kzk2` depends on `coord-task:01kn02mwy8twwr8mcjrc6w60qn`
- `plan-edge:coord-task:01kn02nzf7ftmveaznb5hh2r4j:depends-on:coord-task:01kn02nzdnkxkz5dc3fbmgjfkb`: `coord-task:01kn02nzf7ftmveaznb5hh2r4j` depends on `coord-task:01kn02nzdnkxkz5dc3fbmgjfkb`
- `plan-edge:coord-task:01kn02nzf7ftmveaznb5hh2r4j:depends-on:coord-task:01kn02nzedf9pa1689y558kzk2`: `coord-task:01kn02nzf7ftmveaznb5hh2r4j` depends on `coord-task:01kn02nzedf9pa1689y558kzk2`
- `plan-edge:coord-task:01kn02nzgqcvhrahmb2wt92gfw:depends-on:coord-task:01kn02nzdnkxkz5dc3fbmgjfkb`: `coord-task:01kn02nzgqcvhrahmb2wt92gfw` depends on `coord-task:01kn02nzdnkxkz5dc3fbmgjfkb`
- `plan-edge:coord-task:01kn02nzgqcvhrahmb2wt92gfw:depends-on:coord-task:01kn02nzedf9pa1689y558kzk2`: `coord-task:01kn02nzgqcvhrahmb2wt92gfw` depends on `coord-task:01kn02nzedf9pa1689y558kzk2`
- `plan-edge:coord-task:01kn02nzhkyych6dhta2d3he9x:depends-on:coord-task:01kn02nzcts148twpb0t9ssxp6`: `coord-task:01kn02nzhkyych6dhta2d3he9x` depends on `coord-task:01kn02nzcts148twpb0t9ssxp6`
- `plan-edge:coord-task:01kn02nzhkyych6dhta2d3he9x:depends-on:coord-task:01kn02nzf7ftmveaznb5hh2r4j`: `coord-task:01kn02nzhkyych6dhta2d3he9x` depends on `coord-task:01kn02nzf7ftmveaznb5hh2r4j`
- `plan-edge:coord-task:01kn02nzhkyych6dhta2d3he9x:depends-on:coord-task:01kn02nzgqcvhrahmb2wt92gfw`: `coord-task:01kn02nzhkyych6dhta2d3he9x` depends on `coord-task:01kn02nzgqcvhrahmb2wt92gfw`

