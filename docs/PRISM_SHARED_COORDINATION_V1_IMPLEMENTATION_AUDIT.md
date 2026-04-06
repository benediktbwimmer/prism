# PRISM Shared Coordination V1 Implementation Audit

Status: implementation baseline for `plan:01kng427007ptj9mpg3s0gp4vc`
Audience: PRISM core, MCP, coordination, storage, and runtime maintainers
Scope: freeze the current authoritative shared-coordination file/ref map before the v1 hardening rollout

---

## 1. Purpose

[`PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md`](./PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md)
defines the target architecture for schema-versioned, shard-friendly, 1000-agent-ready shared
coordination. This audit records the current implementation shape in code so the rollout can be
done incrementally without losing track of what is authoritative today.

This document is the frozen baseline for the first implementation task in
`plan:01kng427007ptj9mpg3s0gp4vc`.

---

## 2. Current Authoritative Topology

### 2.1 One live shared ref is still the authoritative cross-runtime plane

Today the authoritative shared-coordination surface is one Git ref per logical repo:

- `refs/prism/coordination/<repo>/live`

That ref is derived in
[`crates/prism-core/src/shared_coordination_ref.rs`](../crates/prism-core/src/shared_coordination_ref.rs)
by `shared_coordination_ref_name(...)`.

The publish path in the same file still stages one snapshot tree, writes one signed manifest, and
compare-and-swap pushes one live head through `sync_shared_coordination_ref_state(...)` and
`sync_shared_coordination_ref_state_inner(...)`.

### 2.2 The live ref contains both authoritative objects and compact indexes

Current staged tree layout:

- `coordination/manifest.json`
- `plans/*.json`
- `coordination/tasks/*.json`
- `coordination/artifacts/*.json`
- `coordination/claims/*.json`
- `coordination/reviews/*.json`
- `coordination/runtimes/*.json`
- `indexes/plans.json`
- `indexes/tasks.json`
- `indexes/artifacts.json`
- `indexes/claims.json`
- `indexes/reviews.json`
- `indexes/runtimes.json`

These paths are built and written in
[`crates/prism-core/src/shared_coordination_ref.rs`](../crates/prism-core/src/shared_coordination_ref.rs)
through the stage helpers, object sync functions, and `rebuild_*_index(...)` helpers.

### 2.3 Startup hydration still assumes one shared authority shape

The local startup checkpoint authority is still derived from the live ref name, head commit, and
manifest digest through
[`crates/prism-core/src/coordination_startup_checkpoint.rs`](../crates/prism-core/src/coordination_startup_checkpoint.rs)
and
[`crates/prism-core/src/shared_coordination_ref.rs`](../crates/prism-core/src/shared_coordination_ref.rs).

That means startup checkpoint validity currently assumes:

- one authoritative shared coordination ref family
- one manifest digest for the whole shared snapshot
- one head commit boundary for the whole shared snapshot

### 2.4 The shared summary and the authoritative write plane are still the same surface

The current live ref is doing all of these jobs at once:

- authoritative task, claim, artifact, review, and runtime-descriptor storage
- startup hydration source
- runtime diagnostics source
- runtime discovery source
- compact operator summary source
- compaction boundary source

This is the main architectural mismatch with the v1 doc, which wants the summary to become a
derived read model rather than the hot write target for everything.

---

## 3. Frozen Authoritative File And Ref Map

### 3.1 Repo-wide summary/live ref

- Ref: `refs/prism/coordination/<repo>/live`
- Current payload root: `coordination/`
- Summary manifest path: `coordination/manifest.json`
- Writer: `sync_shared_coordination_ref_state(...)`
- Readers:
  - `load_shared_coordination_ref_state_*`
  - `shared_coordination_ref_diagnostics(...)`
  - startup checkpoint authority
  - watch live-sync hydration
  - MCP runtime status
  - peer runtime routing

Current contract notes:

- manifest is signed
- manifest now publishes `schema_version` and `kind`
- manifest loaders still accept legacy `version` during migration
- manifest covers the whole snapshot tree rather than a ref family

### 3.2 Plans

- Current authoritative location: `plans/*.json` under the live ref
- Record type: `SharedCoordinationPlanRecord`
- Writer: `sync_plan_objects(...)`
- Readers:
  - `load_shared_coordination_ref_state_from_contents(...)`
  - startup hydration merge path
  - watch live-sync hydration

Current contract notes:

- plan graph and execution overlays are packed into the same plan record
- plan records now publish `schema_version` and `kind`
- plans are not yet sharded into independent ref families

### 3.3 Tasks

- Current authoritative location: `coordination/tasks/*.json` under the live ref
- Record type: `CoordinationTask`
- Writer: `sync_task_objects(...)`
- Readers:
  - `load_shared_coordination_ref_state_from_contents(...)`
  - hydrated coordination snapshot builders
  - watch live-sync hydration

Current contract notes:

- all task writes still contend on the single live ref head
- task payloads are individually file-scoped and version-enveloped, but not ref-sharded

### 3.4 Claims

- Current authoritative location: `coordination/claims/*.json` under the live ref
- Record type: `WorkClaim`
- Writer: `sync_claim_objects(...)`
- Readers:
  - `load_shared_coordination_ref_state_from_contents(...)`
  - hydrated coordination snapshot builders
  - watch live-sync hydration

Current contract notes:

- claim payloads are individually file-scoped and version-enveloped, but still serialize through the single live ref head
- claim liveness and lease truth are not split from runtime liveness yet

### 3.5 Runtime descriptors and liveness

- Current authoritative location: `coordination/runtimes/*.json` under the live ref
- Record type: `RuntimeDescriptor`
- Writers:
  - startup/runtime descriptor publish in MCP server bootstrap
  - `sync_live_runtime_descriptor(...)`
  - full shared-ref publish path
- Readers:
  - `load_shared_coordination_ref_state_from_contents(...)`
  - `shared_coordination_ref_diagnostics(...)`
  - MCP runtime status
  - peer runtime routing and discovery

Current contract notes:

- runtime liveness still republishes the single repo-wide live ref
- runtime descriptor payloads now publish `schema_version` and `kind`
- runtime descriptors are not yet on per-runtime refs
- peer runtime routing still resolves runtimes from the live shared summary surface

### 3.6 Artifacts and reviews

- Current authoritative locations:
  - `coordination/artifacts/*.json`
  - `coordination/reviews/*.json`
- Writers:
  - `sync_artifact_objects(...)`
  - `sync_review_objects(...)`
- Readers:
  - `load_shared_coordination_ref_state_from_contents(...)`
  - hydrated coordination snapshot builders

Current contract notes:

- artifacts and reviews already have separate object files, but not separate authoritative ref
  families
- authoritative artifact and review payloads now publish `schema_version` and `kind`
- v1 architecture reserves separate shard families only if scale demands it later

### 3.7 Indexes

- Current derived locations:
  - `indexes/plans.json`
  - `indexes/tasks.json`
  - `indexes/artifacts.json`
  - `indexes/claims.json`
  - `indexes/reviews.json`
  - `indexes/runtimes.json`
- Writers: `rebuild_*_index(...)`
- Current use: compact browse/read support inside the live snapshot

Current contract notes:

- indexes are derived from the authoritative object files during every publish
- they are stored in the same live ref tree as the authoritative objects
- they are not yet a distinct derived summary plane

### 3.8 Startup checkpoint surface

- Local authority record: `CoordinationStartupCheckpointAuthority`
- Authority derivation:
  - shared ref name
  - current head commit
  - current manifest digest
- Writers:
  - `save_shared_coordination_startup_checkpoint(...)`
  - shared-ref watch live-sync
- Readers:
  - `load_materialized_coordination_snapshot(...)`
  - `load_materialized_coordination_plan_state(...)`

Current contract notes:

- authority shape assumes one summary/live ref
- it does not yet model runtime/task/claim ref families independently

### 3.9 MCP and runtime surfaces

Current shared-coordination readers in runtime/MCP code:

- [`crates/prism-core/src/watch.rs`](../crates/prism-core/src/watch.rs)
  - polls one live shared ref through `poll_shared_coordination_ref_live_sync(...)`
  - hydrates one shared coordination snapshot into runtime state
- [`crates/prism-mcp/src/runtime_views.rs`](../crates/prism-mcp/src/runtime_views.rs)
  - surfaces one `shared_coordination_ref` diagnostics object
- [`crates/prism-mcp/src/peer_runtime_router.rs`](../crates/prism-mcp/src/peer_runtime_router.rs)
  - resolves peer runtime descriptors from `shared_coordination_ref_diagnostics(...)`
  - currently assumes runtime descriptors are discoverable through the repo-wide live ref summary

Current contract notes:

- runtime/MCP code still treats shared coordination as one ref-level authority surface
- there is no shard-family freshness reporting yet

---

## 4. Current Persistence Split

### 4.1 Shared ref publication

[`crates/prism-core/src/coordination_persistence.rs`](../crates/prism-core/src/coordination_persistence.rs)
still treats shared-ref publication as the authoritative cross-runtime publish step through
`sync_authoritative_shared_coordination_ref_observed(...)`.

### 4.2 Local read models and checkpoints

The same persistence path also writes local derived state:

- coordination read model
- coordination queue read model
- startup checkpoint
- local tracked/published coordination snapshot state where enabled

Important baseline decision:

- local SQLite and startup checkpoints are derived/local acceleration layers
- the shared coordination ref remains the only cross-runtime authoritative shared fact plane today

### 4.3 Shared runtime store is not shared coordination authority

[`crates/prism-core/src/shared_runtime_store.rs`](../crates/prism-core/src/shared_runtime_store.rs)
is a local/shared runtime query backend and storage facade. It is not the authoritative Git-backed
coordination ref plane and should not be treated as such in the v1 rollout.

---

## 5. Gaps Against The V1 Architecture

### 5.1 Schema envelope gap

Current state:

- shared manifest now publishes `schema_version` and `kind`
- plan, task, claim, runtime descriptor, artifact, and review payloads now publish `schema_version`
  and `kind`
- readers still accept legacy raw payloads and legacy manifest `version` during migration

Remaining mismatch:

- no schema-envelope mismatch remains for the current authoritative payload set

### 5.2 Hot write topology gap

Current state:

- heartbeats, runtime descriptor refreshes, task updates, claim updates, and other shared writes
  still serialize through `refs/prism/coordination/<repo>/live`

Target mismatch:

- v1 requires per-runtime refs and sharded task/claim authority

### 5.3 Summary-vs-authority gap

Current state:

- summary, diagnostics, startup hydration, runtime discovery, and authoritative object truth all
  live together on the same ref

Target mismatch:

- v1 requires the summary to become a derived aggregate with bounded, observable freshness

### 5.4 Lease and liveness gap

Current state:

- lease truth and runtime liveness are still effectively coupled through the single live shared ref

Target mismatch:

- v1 requires lease validity to become a join between durable task/claim lease facts and per-runtime
  liveness refs

### 5.5 Reader-shape gap

Current state:

- startup checkpoint authority
- watch live-sync
- MCP runtime status
- peer runtime routing

all assume one shared coordination authority shape.

Target mismatch:

- v1 requires those readers to tolerate shard-family authority and explicit freshness/compatibility
  diagnostics

### 5.6 Merge and repair gap

Current state:

- compare-and-swap retries exist
- conflict handling is still centered on one live ref head and same-object retry rules

Target mismatch:

- v1 requires schema-aware semantic merge and repair flows across `refs/prism/**`

---

## 6. Frozen Code Ownership Map For The Rollout

### 6.1 Core authoritative ref model

- [`crates/prism-core/src/shared_coordination_ref.rs`](../crates/prism-core/src/shared_coordination_ref.rs)
  owns:
  - ref naming
  - manifest envelope
  - staged object layout
  - publish patch generation
  - verification
  - diagnostics
  - compaction
  - authoritative shared snapshot load

This file is the primary owner for tasks:

- explicit `schema_version` envelopes
- ref-family topology split
- shard path derivation
- summary derivation
- semantic merge hooks
- archive boundary metadata

### 6.2 Coordination persistence and startup hydration

- [`crates/prism-core/src/coordination_persistence.rs`](../crates/prism-core/src/coordination_persistence.rs)
  owns request-path authoritative publication orchestration and local derived persistence
- [`crates/prism-core/src/coordination_startup_checkpoint.rs`](../crates/prism-core/src/coordination_startup_checkpoint.rs)
  owns startup checkpoint authority matching

These files are the primary owners for tasks:

- summary-vs-authority split
- startup authority family expansion
- local fallback behavior while summary freshness is bounded

### 6.3 Runtime/watch integration

- [`crates/prism-core/src/watch.rs`](../crates/prism-core/src/watch.rs)
  owns live shared-coordination poll and hydrate behavior
- [`crates/prism-core/src/shared_runtime_store.rs`](../crates/prism-core/src/shared_runtime_store.rs)
  remains a secondary local/shared runtime backend and should stay out of authoritative topology

These files are the primary owners for tasks:

- per-runtime liveness plane adoption
- summary freshness fallback
- runtime-surface lag and diagnostics

### 6.4 MCP/runtime/operator surfaces

- [`crates/prism-mcp/src/runtime_views.rs`](../crates/prism-mcp/src/runtime_views.rs)
  owns runtime diagnostics projection for shared coordination
- [`crates/prism-mcp/src/peer_runtime_router.rs`](../crates/prism-mcp/src/peer_runtime_router.rs)
  owns runtime descriptor resolution and peer query routing
- [`crates/prism-mcp/src/server_surface.rs`](../crates/prism-mcp/src/server_surface.rs)
  owns mutation-facing compatibility and work-subject checks that will need summary/shard aware
  diagnostics

These files are the primary owners for tasks:

- shard-family freshness reporting
- compatibility diagnostics
- runtime descriptor discovery cutover
- operator guidance for repair/takeover/staleness

---

## 7. Immediate Rollout Implications

The next tasks in the plan should treat the following as fixed starting facts:

- the current authoritative shared plane is one signed live ref
- runtime descriptors are currently published and discovered through that same live ref
- startup hydration authority is currently one live ref plus manifest digest
- task and claim object files already exist, but they are not yet on independent ref families
- the current indexes are derived files inside the same live ref, not a separate summary plane

This baseline means the rollout can proceed in the following order without ambiguity:

1. add explicit versioned envelopes and compatibility gates
2. split runtime liveness off the hot live summary path
3. split task and claim authority into shard families
4. redefine summary as a derived bounded-freshness aggregate
5. update diagnostics, startup hydration, and peer routing to understand the new authority shape

---

## 8. Out Of Scope For This Audit

This document intentionally does not:

- redesign the target architecture beyond what
  [`PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md`](./PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md)
  already decided
- prescribe the exact shard count or archive partition count
- implement any behavioral change
- redefine local runtime/query storage as shared authority

It is a frozen baseline for the current implementation only.
