# Sharp SQL Read-Model And Command Ports

Status: proposed
Audience: coordination, storage, runtime, query, MCP, CLI, UI, and service maintainers
Scope: replace the remaining generic SQL authority read/store abstraction with exact caller-shaped read-model ports plus a final backend-neutral SQL command seam

---

## 1. Summary

PRISM has already removed most snapshot-era coupling from the coordination authority layer, but the
remaining hot query seam is still too store-shaped.

It still encourages a model where callers:

- open a generic authority read surface
- request broad payloads such as canonical snapshots
- shape the final response in the caller instead of asking the authority/query layer for an exact view

That is not the right end state for a competitive orchestration engine.

The final target is:

- one sharp SQL command seam for authoritative writes
- many sharp read-model ports for hot query paths
- no generic hot authority reads returning broad blobs
- no SQLite- or Postgres-specific details in caller-facing contracts
- snapshot/export/recovery access only on explicit non-hot admin seams

SQLite remains the local-first implementation.

Postgres is the production target.

Both must implement the same caller-shaped ports.

## 2. Goals

### 2.1 Replace generic hot reads with caller-shaped read-model ports

Hot reads must return exactly what the caller needs.

Examples of acceptable query shapes:

- `list_ready_tasks_for_executor(...) -> Vec<ReadyTaskView>`
- `list_pending_reviews(...) -> Vec<PendingReviewView>`
- `read_plan_header(...) -> Option<PlanHeaderView>`
- `read_plan_detail(...) -> Option<PlanDetailView>`
- `list_active_runtime_leases(...) -> Vec<RuntimeLeaseView>`
- `read_event_execution_status(...) -> Option<EventExecutionStatusView>`
- `read_authority_health(...) -> AuthorityHealthView`

Examples of unacceptable hot query shapes:

- `read_current_state(...)`
- `read_canonical_snapshot_v2(...)`
- `read_projection_blob(...)`
- generic “get me the state and I will filter it locally” APIs

### 2.2 Keep the command seam narrow and backend-neutral

The command side should expose authoritative write operations without leaking database details.

It should focus on:

- appending authoritative coordination events
- optimistic concurrency
- commit metadata
- command-specific write helpers only where they are logically distinct and truly needed

It should not expose:

- SQLite transaction objects
- Postgres session handles
- SQL query fragments
- broad state replacement on the hot path

### 2.3 Make SQLite and Postgres implementation details invisible to callers

The public contract should describe:

- domain ids
- filter parameters
- sort and pagination parameters
- exact returned DTOs
- concurrency expectations for writes

It should not describe:

- SQL tables
- joins
- JSONB payload layout
- checkpoint files
- snapshot storage strategy

### 2.4 Preserve admin/export/recovery functionality without polluting hot paths

Broad snapshot/export/recovery operations may still exist, but only on explicit secondary seams.

Those seams are not part of the normal product hot path.

## 3. Non-goals

This spec does not:

- implement the full Postgres backend
- design every physical SQL table in full detail
- change coordination domain semantics
- define the shared execution substrate itself
- require eventual consistency for core read models

## 4. Design principles

### 4.1 Query ports model use cases, not storage

The abstraction boundary should be organized around caller intent.

Bad:

- one generic projection store
- one generic coordination read service
- one generic state blob that every caller interprets differently

Good:

- task queue reads
- review queue reads
- plan summary/detail reads
- runtime lease reads
- event execution reads
- authority diagnostics reads

### 4.2 DTOs are exact and minimal

Each read-model port returns exact DTOs for one access pattern.

DTOs should:

- include only the fields required by the caller
- include explicit ids, statuses, timestamps, counts, and display-ready summaries where needed
- be independently evolvable by use case

DTOs should not:

- embed entire coordination snapshots
- contain unrelated nested state for convenience
- force callers to locally derive the view they actually wanted

### 4.3 Core read models should be transactionally updated

The authoritative event append and the core current-state projection updates should usually happen in
the same SQL transaction.

That gives PRISM:

- strong current-state reads for hot paths
- small writes
- queryable relational projections
- predictable local-first SQLite behavior
- a clean path to indexed Postgres projections

### 4.4 Snapshot/export/recovery is a secondary surface

Broad snapshot reads and state replacement are allowed only for:

- recovery
- export/import
- debugging
- migration

They should not be the primary path for UI, MCP, runtime, or orchestration decisions.

## 5. Target contract shape

### 5.1 Command port

The hot write side should converge on one narrow command surface, for example:

- `append_coordination_events(request) -> CoordinationCommitReceipt`

The request should include:

- base concurrency expectation
- session or actor correlation where required
- appended authoritative events

The result should include:

- committed revision
- applied event count
- conflict or rejection information
- backend-neutral diagnostics when needed

The hot command result should not return:

- full current state
- canonical snapshots
- backend-native persistence artifacts

### 5.2 Read-model ports

Split hot reads into dedicated ports.

The exact final set can evolve, but the architecture should look like this:

- `TaskQueueReadPort`
- `ReviewQueueReadPort`
- `PlanReadPort`
- `RuntimeLeaseReadPort`
- `EventExecutionReadPort`
- `AuthorityDiagnosticsReadPort`

Optional additional ports if real callers justify them:

- `ArtifactReadPort`
- `WorktreeCoordinationReadPort`
- `SpecCoordinationReadPort`

### 5.3 Example sharp query APIs

Illustrative examples:

#### Task queue reads

- `list_ready_tasks_for_executor(request) -> Vec<ReadyTaskView>`
- `list_in_progress_tasks_for_worktree(request) -> Vec<ActiveTaskView>`
- `list_blocked_tasks(request) -> Vec<BlockedTaskView>`

#### Review queue reads

- `list_pending_reviews(request) -> Vec<PendingReviewView>`
- `list_reviews_for_artifact(request) -> Vec<ArtifactReviewView>`

#### Plan reads

- `read_plan_header(plan_id) -> Option<PlanHeaderView>`
- `read_plan_detail(plan_id) -> Option<PlanDetailView>`
- `list_plan_children(plan_id) -> Vec<PlanChildView>`

#### Runtime reads

- `list_runtime_descriptors(request) -> Vec<RuntimeDescriptorView>`
- `list_active_runtime_leases(request) -> Vec<RuntimeLeaseView>`

#### Event execution reads

- `read_event_execution(event_execution_id) -> Option<EventExecutionView>`
- `list_recent_event_execution_failures(request) -> Vec<EventExecutionFailureView>`

#### Diagnostics

- `read_authority_health() -> AuthorityHealthView`
- `read_authority_stamp() -> Option<AuthorityStampView>`

These names are illustrative.

The important point is the shape:

- caller-specific
- exact DTOs
- no broad blob payloads

### 5.4 Admin/export/recovery ports

Keep broad secondary operations isolated, for example:

- `SnapshotExportPort`
- `SnapshotRecoveryPort`

Those ports may support:

- snapshot export
- snapshot import
- state replacement for recovery/migration
- broad debug reads

But they must be clearly separate from hot orchestration reads and writes.

## 6. Physical storage guidance

This spec intentionally keeps the public contract backend-neutral, but the expected SQL-friendly
implementation model is:

- authoritative event log
- typed relational current-state projections
- targeted indexes for hot reads
- optional snapshot/checkpoint artifacts for recovery

A representative shape is:

- one coordination event log with typed envelope columns and structured payload
- current-state projection tables for plans, tasks, artifacts, reviews, requirements, runtime
  descriptors, and event execution records
- read-model queries that map directly onto those projection tables

Callers must not depend on that physical layout directly.

## 7. Migration plan

### 7.1 Stop extending the generic projection seam

No new hot caller should be added to:

- generic projection reads
- canonical snapshot reads
- broad current-state assembly helpers

### 7.2 Introduce read-model DTOs and ports by caller family

Add read-model ports for the current hot callers first:

- MCP read broker
- trust/diagnostics surfaces
- runtime gateway
- event engine
- task/review/plan-oriented query surfaces

### 7.3 Migrate call sites off broad reads

Each caller should move from:

- “open authority projection store, read broad blob, derive locally”

to:

- “call the one read-model port that returns the exact DTO this caller needs”

### 7.4 Demote broad reads to admin-only use

Once hot callers are off generic reads:

- keep snapshot/export/recovery only on explicit admin seams
- forbid them on normal product flows

### 7.5 Keep the Postgres stub honest

Even before full Postgres implementation lands:

- the stub should compile against the sharp read-model ports
- new read DTOs should be designed with Postgres-backed indexed execution in mind

## 8. Validation

When implementing this spec:

- run `cargo test -p prism-core -p prism-mcp -p prism-cli`
- run additional targeted tests for any newly split read-model modules
- run `cargo test` because this changes a shared authority/query contract

## 9. Exit criteria

- no hot product path depends on broad authority reads like canonical snapshots or generic current-state blobs
- the hot authority write surface is a narrow backend-neutral SQL command seam
- hot reads are expressed through caller-shaped read-model ports with exact DTOs
- SQLite and Postgres can implement the same public query/command contracts
- admin/export/recovery operations remain available only on explicit secondary seams
- the resulting contract is suitable for a high-performance local-first SQLite implementation and a high-performance production Postgres implementation
