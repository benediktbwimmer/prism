# Coordination Authority Abstraction And Storage Refactor

Status: superseded
Audience: coordination, storage, runtime, query, MCP, CLI, and service maintainers
Scope: replace the snapshot-shaped `CoordinationAuthorityStore` contract with a command, projection, and history oriented authority seam; migrate all product call sites; and define the target SQLite and Postgres authority storage model

Superseded by:

- [2026-04-09-sql-only-coordination-authority-cutover.md](./2026-04-09-sql-only-coordination-authority-cutover.md)

---

## 1. Summary

PRISM's current authority seam is backend-neutral only in name.

The existing `CoordinationAuthorityStore` and internal DB authority traits still expose
snapshot-shaped semantics:

- writes send a full `CoordinationSnapshot` plus `CoordinationSnapshotV2`
- reads return a full `CoordinationCurrentState`

That contract leaks the current SQLite implementation strategy upward and will make future Postgres
work, performance work, and later roadmap phases more expensive if it remains the default
interface.

This refactor introduces the real authority contract PRISM should build on:

- authoritative append-style coordination writes
- shaped current-state authority reads for hot paths
- retention-aware history reads
- typed current-state projection tables for operational reads
- optional checkpoints and export snapshots as recovery tools rather than the primary write model

The immediate implementation target is not "ship Postgres now."

It is:

- replace the bad contract now
- migrate all current call sites now
- keep SQLite working behind the new seam
- make later Postgres implementation a backend task rather than a semantic rewrite

## 2. Status

Coarse checklist:

- [ ] define the replacement authority traits and result types
- [ ] remove full-snapshot fields from authority transaction requests
- [ ] define the target event-log plus typed-projection storage model
- [ ] implement a SQLite backend behind the new contract
- [ ] migrate `prism-core` authority call sites
- [ ] migrate `prism-query`, `prism-mcp`, and `prism-cli` authority call sites
- [ ] confine snapshots and checkpoints to explicit recovery or export paths
- [ ] remove product-path dependencies on whole-state authority reads

Progress note (2026-04-09):

- the current authority seam already centralizes backend selection
- the current seam is still snapshot-shaped and therefore too weak to protect later phases from
  storage-model coupling
- this spec is the next blocking step before more execution and graph work builds on that contract

## 3. Problem statement

The current contract has three structural problems.

### 3.1 The write contract is too heavy

`CoordinationTransactionRequest` requires the caller to send:

- `snapshot`
- `canonical_snapshot_v2`
- `appended_events`

That means the caller has already hydrated and derived the whole coordination world before it asks
the authority layer to persist a mutation.

This is the opposite of the intended abstraction boundary.

### 3.2 The read contract is too broad

`read_current(...)` returns `CoordinationCurrentState`, which contains the full current
coordination snapshot and canonical snapshot.

That makes whole-state hydration the default product read model even when a caller only needs:

- one task
- one plan summary
- pending reviews
- ready tasks
- active runtime descriptors

### 3.3 The backend abstraction leaks the storage strategy

The current `CoordinationAuthorityStore` and internal `CoordinationAuthorityDb` traits abstract
"which backend" but not "what persistence contract."

As a result:

- snapshot-oriented backends fit naturally
- event-log plus projection backends fit awkwardly
- future Postgres work would be pushed toward emulating snapshots rather than using relational
  strengths

## 4. Goals

This refactor must make the following true.

### 4.1 Product code no longer depends on snapshot-shaped authority semantics

Product code should not need to:

- build whole coordination snapshots to commit one mutation
- load the whole coordination state blob to answer hot current-state queries

### 4.2 The authority seam is storage-strategy-neutral

The replacement contract must work cleanly for:

- SQLite
- Postgres
- Git shared refs during compatibility periods

without forcing all backends to look like snapshot stores.

### 4.3 Hot reads are shaped and projection-backed

The authority layer should make current-state operational queries cheap by design.

That means the steady-state target is:

- authoritative event log for write history
- typed projection tables for current state
- indexed current-state reads for hot product paths

### 4.4 Historical reads remain available without driving the whole architecture

Historical queries are important for auditability, provenance, and debugging, but they are not the
dominant workload.

The authority design should optimize current state first while keeping retained history usable.

## 5. Non-goals

This refactor does not attempt to:

- implement the shared execution substrate
- implement the full Postgres backend
- redesign coordination-domain semantics such as artifact, review, or task rules
- replace every product query with one universal authority query API in a single slice
- remove `CoordinationSnapshot` from replay, export, or checkpoint code immediately

## 6. Related contracts and roadmap

Primary contracts:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)

Related domain contract:

- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)

Roadmaps:

- [../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md](../roadmaps/2026-04-09-execution-substrate-and-compiled-plan-rollout.md)
- [../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md](../roadmaps/2026-04-09-db-authority-family-and-abstraction-hardening.md)

Existing implementation context:

- [2026-04-09-sqlite-coordination-authority-phase-3.md](./2026-04-09-sqlite-coordination-authority-phase-3.md)
- [2026-04-09-db-coordination-authority-seam-phase-2.md](./2026-04-09-db-coordination-authority-seam-phase-2.md)

## 7. Design

### 7.1 Core principles

The replacement authority design should follow these rules:

- full snapshots are not the normal authority write contract
- full current-state blobs are not the normal authority read contract
- the authority backend owns authoritative append and projection update semantics
- current-state reads should come from typed projections
- history reads should come from the authoritative event log and related history helpers
- checkpoints are optional recovery accelerators, not the primary logical write model

### 7.2 Replace one snapshot-shaped trait with one facade over narrower responsibilities

`CoordinationAuthorityStore` should remain the public authority facade, but its operations should
be reorganized around responsibilities rather than snapshots.

The facade should cover:

- command append and optimistic concurrency
- shaped current-state queries
- runtime descriptor reads and writes
- event execution record reads and writes
- retained history reads
- diagnostics

Internally, the implementation may split this into narrower traits or modules such as:

- `CoordinationAuthorityCommands`
- `CoordinationAuthorityQueries`
- `CoordinationAuthorityHistory`
- `CoordinationRuntimeDescriptorStore`
- `CoordinationEventExecutionAuthorityStore`

The important rule is that callers should not see snapshot-shaped operations as the default path.

### 7.3 New write contract

The main authority write request should no longer contain:

- a full `CoordinationSnapshot`
- a full `CoordinationSnapshotV2`

Instead it should contain:

- an optimistic base such as current revision or authority stamp
- session and provenance metadata
- one or more authoritative coordination events or event-like command records
- optional changed-object hints if needed for projection refresh bookkeeping

Minimum target shape:

- `base`
- `session_id`
- `events`
- `derived_state_mode` or replacement projection-update policy

The backend must own:

- appending the authoritative records
- applying them transactionally
- updating the typed current-state projections
- returning commit metadata and changed-entity metadata

The write result should return:

- commit status
- new authority stamp or revision
- changed plans, tasks, artifacts, and reviews when available
- diagnostics or conflict details

It should not return the full current snapshot by default.

### 7.4 New current-state read contract

The authority read surface should expose shaped current-state operations for the main product
paths.

At minimum the replacement surface should support:

- plan summary and plan detail reads
- task detail reads
- artifact detail reads
- review detail reads
- ready-task reads
- pending-review reads
- blocker and completion-evidence reads
- runtime descriptor reads
- event execution record reads

Whole-plan or whole-workspace coordination graph reads may still exist, but they must be explicit
special-purpose operations rather than the default current-state API that every caller uses.

### 7.5 New history contract

History should remain part of the authority surface, but as an explicit retained-history API.

It should support:

- event timeline by root and revision range
- object-scoped history by plan, task, artifact, or review id
- retention and compaction metadata
- provenance and commit metadata

History reads may remain slower than hot current-state reads.

That is acceptable.

### 7.6 Target storage model

The target storage design for both SQLite and Postgres is:

- one authoritative coordination event log
- typed current-state projection tables
- optional checkpoints for recovery or bootstrap

#### 7.6.1 Event log

The authority backend should maintain one coordination event log with:

- typed envelope columns
- event-specific payload storage

The steady-state relational shape should include at least:

- `repo_id`
- `revision` or monotonic sequence
- `event_id`
- `ts`
- `event_kind`
- `actor`
- `session_id` when present
- `plan_id`, `task_id`, `artifact_id`, `review_id` when applicable
- event payload column

For Postgres, the event payload should be `jsonb`.

For SQLite, the payload may remain JSON text or another structured encoding, but the logical model
should match the same event envelope.

#### 7.6.2 Projection tables

Current-state tables should be typed and indexed.

At minimum the target authority projection family should include:

- `coordination_plans`
- `coordination_tasks`
- `coordination_task_artifact_requirements`
- `coordination_task_review_requirements`
- `coordination_artifacts`
- `coordination_artifact_reviews`
- `coordination_runtime_descriptors`
- `coordination_event_execution_records`

These tables should carry typed columns for hot filters and joins.

The design should not rely on querying into the event payload for ordinary operational reads.

#### 7.6.3 Index posture

The projection tables should be indexed for the real hot paths, including:

- by repo plus current revision
- by plan id
- by task id
- by task status and assignee or worktree
- by artifact requirement or review requirement linkage
- by pending review status
- by runtime id
- by event execution id and status

### 7.7 Snapshot and checkpoint policy after the refactor

`CoordinationSnapshot` and `CoordinationSnapshotV2` should remain valid for:

- replay
- export or import
- recovery
- checkpoints
- selected testing helpers

They should not remain the ordinary product-facing authority contract.

Checkpoint writes should be optional and explicit.

They may be used to:

- accelerate restart
- provide a compact export artifact
- bound replay cost

They must not be required for correctness of ordinary mutation commit or read paths.

### 7.8 Call-site migration rules

This refactor includes all product call sites that currently depend on snapshot-shaped authority
operations.

#### 7.8.1 `prism-core`

The following modules must stop depending on full-snapshot authority semantics on hot paths:

- `coordination_persistence.rs`
- `session.rs`
- `published_plans.rs`
- `watch.rs`
- `coordination_reads.rs`
- `coordination_startup_checkpoint.rs`
- `checkpoint_materializer.rs`
- `workspace_runtime_state.rs`
- `coordination_authority_store/*`

Required direction:

- write paths append authoritative events and refresh projections through the backend
- strong and eventual reads use shaped authority queries or explicit projection reads
- snapshot export and checkpoint helpers become explicit secondary flows

#### 7.8.2 `prism-query`

`prism-query` should stop assuming that authority-backed strong reads naturally provide a full
coordination snapshot blob.

Required direction:

- query helpers should request the current entity or shaped summary they actually need
- broad snapshot reads should be isolated to explicit export or replay surfaces only

#### 7.8.3 `prism-mcp`

`prism-mcp` should consume shaped coordination reads instead of relying on whole-state hydration in
host or broker helpers.

Required direction:

- plan, task, artifact, review, and queue surfaces should flow from typed authority reads or
  already-shaped query helpers
- authority diagnostics and runtime status should remain available without snapshot coupling

#### 7.8.4 `prism-cli`

CLI runtime-status, diagnostics, and authority-facing commands should move to the new shaped
authority operations and explicit history or export operations where needed.

### 7.9 Git backend posture during migration

This spec does not require Git shared refs to disappear immediately.

However:

- Git should become an adapter behind the new authority contract
- product code must not keep snapshot semantics alive solely because the Git backend prefers them

If necessary, Git may temporarily implement the new command and query contract through a narrower
compatibility adapter internally.

## 8. Implementation slices

### Slice A: Authority trait and type redesign

Replace the current authority request and response types so the primary contract is no longer
snapshot-shaped.

This includes:

- removing `snapshot` and `canonical_snapshot_v2` from the primary transaction request
- defining shaped current-state read requests and responses
- defining explicit history requests and responses
- preserving authority stamps, optimistic concurrency, and diagnostics

### Slice B: SQLite authority storage redesign

Rework the SQLite authority backend so it can satisfy the new contract through:

- authoritative event-log persistence
- typed current-state projection tables
- explicit optional checkpoints

This slice should not wait for Postgres.

SQLite must become the first working implementation of the target model.

### Slice C: Core write-path migration

Migrate:

- `coordination_persistence.rs`
- `session.rs`
- adjacent mutation helpers

so they append authoritative changes through the new contract instead of handing authority a
caller-built world snapshot.

### Slice D: Core read-path migration

Migrate:

- session strong and eventual reads
- read broker and publication helpers
- runtime status and authority diagnostics surfaces

so hot current-state reads use shaped authority operations or typed projections.

### Slice E: Query, MCP, and CLI migration

Migrate:

- `prism-query`
- `prism-mcp`
- `prism-cli`

to the new authority contract and remove their product-path dependencies on whole-state authority
reads.

### Slice F: Snapshot confinement and cleanup

After the migration:

- snapshots remain only on explicit replay, export, or checkpoint paths
- old snapshot-shaped authority helpers are deleted
- compatibility shims are removed from product paths

## 9. Validation

Tier 1:

- `cargo test -p prism-core coordination_authority_store`
- `cargo test -p prism-core session`
- `cargo test -p prism-query`
- `cargo test -p prism-mcp`
- `cargo test -p prism-cli`

Tier 2:

- run direct downstream crates when `prism-core` authority types or read contracts change

Tier 3:

- `cargo test`

This full workspace run is required in this refactor because:

- the SQLite authority schema and persistence contract change
- the authority read and write path is a cross-cutting runtime concern

Targeted tests should cover at least:

- authority append with optimistic concurrency conflicts
- typed projection updates for task, artifact, and review state
- shaped current-state reads without whole-snapshot hydration
- retained history reads after multiple commits
- runtime descriptor and event execution record persistence
- restart and checkpoint recovery without making checkpoints the primary write model
- migration of MCP, query, and CLI call sites away from whole-state authority reads

## 10. Rollout and migration

The intended rollout is:

1. land the replacement authority traits and request or response types
2. add the SQLite implementation of the new contract
3. migrate core write paths
4. migrate core read paths
5. migrate query, MCP, and CLI surfaces
6. remove snapshot-shaped product-path helpers

Migration rules:

- do not add new product code against the old snapshot-shaped authority contract once this spec is active
- do not implement Postgres against the current snapshot-shaped contract
- do not preserve whole-snapshot reads as the easy default path for new surfaces

## 11. Open questions

Questions to settle during implementation:

- whether the top-level public authority facade should remain one trait or split into a small trait
  family plus one composed facade
- whether Git shared refs should keep a compatibility adapter long term or become explicitly
  second-class after Postgres lands
- how much of the old snapshot-based checkpoint export format should remain as a stable interchange
  artifact versus an internal recovery tool
