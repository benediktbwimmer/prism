# PRISM Coordination Authority Store

Status: normative contract
Audience: PRISM coordination, service, runtime, MCP, query, storage, and future authority-backend maintainers
Scope: one backend-neutral authority interface for current and historical coordination state

---

## 1. Goal

PRISM should introduce one explicit **Coordination Authority Store** abstraction that sits between
all coordination-facing application code and the underlying authority substrate.

The abstraction exists to make three things true at the same time:

- the coordination kernel depends on one concrete transactional protocol instead of a spread of
  storage-specific helpers
- the first production-grade release path can use a DB-backed authority family without rewriting
  the rest of the coordination stack
- the current Git shared-ref implementation remains a first-class backend rather than a special-case
  side path

The abstraction is intentionally **high-level**. It does not try to unify Git refs and SQL rows at
an implementation level. It defines the coordination semantics that PRISM needs:

- current authoritative state
- coordination-facing read access semantics, while concrete lagging projections or materialization
  remain behind a separate seam when they exist
- transactional mutation
- runtime descriptor publication and discovery
- authoritative event-execution record storage and reads
- retained authoritative history
- authority diagnostics and provenance

This is the seam that lets the rest of PRISM stop caring whether the active authority backend is a
DB-backed store or Git shared refs.

One rule must remain explicit:

- each coordination root has exactly one active authority backend at a time

PRISM may later support export, import, mirroring, or migration between backends, but it must not
allow split-brain authority for the same coordination root.

Companion implementation spec:

- [coordination-authority-store-implementation-spec.md](./coordination-authority-store-implementation-spec.md)

This contract relies on:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)

---

## 2. Why this abstraction is needed now

The current codebase already has a clean architectural direction, but the implementation is still
spread across several layers:

- `shared_coordination_ref.rs` owns the Git shared-ref protocol directly
- `published_plans.rs` calls the shared-ref layer directly for authoritative loads
- `coordination_persistence.rs` mixes local journal persistence, shared-ref publication, startup
  checkpoint writes, and read-model writes in one place
- `session.rs` orchestrates reads, refresh, mutation persistence, and materialization scheduling
  directly against the store plus shared-ref helpers
- `watch.rs` knows about shared-ref polling and applies live updates directly
- MCP and CLI surfaces call runtime descriptor and diagnostics functions directly

That spread makes responsibilities unclear and makes it difficult to:

- finish the transactional coordination mutation rewrite cleanly
- harden atomicity and materialization ordering
- introduce a second authority backend later
- state clearly what the coordination kernel is allowed to assume

The Coordination Authority Store should become the single contractual answer to:

- how current coordination state is read
- how mutations become authoritative
- how history is queried
- how runtime descriptors are published and discovered
- how authoritative event-execution records are read and stored
- how authority metadata and diagnostics are surfaced

That includes identity attribution, capability-gated writes, provenance, and verification posture,
not just storage mechanics.

---

## 3. Non-goals

This abstraction should **not** try to solve unrelated concerns.

Non-goals:

- replacing local SQLite read models or startup checkpoints with a backend-independent cache layer
- abstracting shell Git, manifest files, shard refs, SQL transactions, or storage tables directly
- flattening backend-specific implementation details into one lowest-common-denominator API
- making current and historical reads pretend to have identical cost or richness on all backends
- solving cognition or knowledge-layer storage through the same interface

The abstraction is for **coordination authority**, not for all PRISM storage.

---

## 4. Architectural placement

The target layering should be:

1. **Coordination kernel**
   - plan/task/artifact/review semantics
   - actionability and dependency evaluation
   - mutation validation
   - review/reopen/reject/yield semantics

2. **Coordination Authority Store**
   - current authoritative reads
   - semantic strong and eventual reads
   - transactional mutation commit/reject
   - retained authoritative history
   - runtime descriptor publication/discovery
   - authoritative event-execution record storage/reads
   - authority diagnostics

3. **Authority backend implementation**
   - `DbCoordinationAuthorityStore`
     - `PostgresCoordinationDb`
     - `SqliteCoordinationDb`
   - `GitCoordinationAuthorityStore`

4. **Service-owned coordination materialization / runtime services**
   - optional service-owned coordination read models or projections
   - optional service-owned coordination checkpoints
   - UI and query acceleration
   - runtime-local activity telemetry

The key rule is:

- the coordination kernel and the product surfaces depend on the **Coordination Authority Store**
- only backend adapters know whether authority is implemented by the DB-backed family or by Git
  shared refs
- service-owned coordination read models and checkpoints remain downstream of the authority
  interface
- the concrete local persistence seam for eventual reads lives in
  [coordination-materialized-store.md](./coordination-materialized-store.md)

---

## 5. Core principles

### 5.1 One contract for both current state and retained history

The rest of PRISM should not read current state through one interface and coordination history
through direct `.git` inspection.

Both are part of the authority plane and should live behind the same abstraction.

### 5.2 Strong and eventual remain first-class semantics

The abstraction must preserve the distinction already present in the coordination-only runtime:

- **Eventual** means read from a lagging but allowed derived view of authoritative state when such a
  view exists
- **Strong** means refresh against the authority substrate before answering

These are coordination semantics, not Git-specific implementation details.

The interface owns the semantics of strong versus eventual reads.
For DB-backed authority, both semantics may map to the same current-authority path until a real
lagging projection exists.

### 5.3 Transactionality is a semantic contract, not a storage accident

The abstraction should define what it means for a coordination mutation to:

- commit atomically
- be rejected deterministically
- be retried safely
- return authoritative commit metadata

The Git backend achieves this with summary manifests, shard heads, and `git push --atomic`.
A PostgreSQL backend may achieve it with SQL transactions.
The coordination kernel should care only about the semantic result.

### 5.4 History is authoritative but retention-aware

Backends may differ in how they retain and query history.
The abstraction must therefore surface:

- what history is available
- which history class is authoritative-retained
- whether a requested range is compacted or archived
- what provenance metadata can be guaranteed

### 5.5 Runtime descriptors are authority data

Runtime discovery and descriptor publication are part of coordination authority, not separate side
channels.
The abstraction must support publishing and reading runtime descriptors explicitly.

Local descriptor caches or indexes may still exist, but they are derived views over authority data,
not separate channels.

### 5.6 Event-execution records are authority data in a dedicated namespace

Event-execution records are authoritative facts once persisted, but they are not hot coordination
summary state.

The abstraction must therefore support storing and reading them explicitly while keeping them in a
dedicated authority namespace rather than embedding them into the plan/task summary snapshot.

### 5.7 DB-backed authority is authority-first by default

For DB-backed authority:

- the default coordination read path is the authority backend itself
- a separate coordination materialized store is not required by default
- optional extra materialization is a follow-on optimization, primarily for Postgres when justified

---

## 6. Proposed interface shape

The interface should be explicit and coordination-shaped.
The following sketch is intentionally Rust-like, but it is a behavioral contract rather than final
code.

```rust
pub trait CoordinationAuthorityStore {
    fn capabilities(&self) -> CoordinationAuthorityCapabilities;

    fn read_current(
        &self,
        request: CoordinationReadRequest,
    ) -> Result<CoordinationReadEnvelope<CoordinationCurrentState>>;

    fn apply_transaction(
        &self,
        request: CoordinationTransactionRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn publish_runtime_descriptor(
        &self,
        request: RuntimeDescriptorPublishRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn clear_runtime_descriptor(
        &self,
        request: RuntimeDescriptorClearRequest,
    ) -> Result<CoordinationTransactionResult>;

    fn list_runtime_descriptors(
        &self,
        request: RuntimeDescriptorQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<RuntimeDescriptor>>>;

    fn read_event_execution_records(
        &self,
        request: EventExecutionRecordAuthorityQuery,
    ) -> Result<CoordinationReadEnvelope<Vec<EventExecutionRecord>>>;

    fn upsert_event_execution_record(
        &self,
        record: EventExecutionRecord,
    ) -> Result<EventExecutionRecordWriteResult>;

    fn read_history(
        &self,
        request: CoordinationHistoryRequest,
    ) -> Result<CoordinationHistoryEnvelope>;

    fn diagnostics(&self, request: CoordinationDiagnosticsRequest)
        -> Result<CoordinationAuthorityDiagnostics>;
}
```

The main point is not the exact method count. The point is to make **all authoritative coordination
access** route through one contract.

---

## 7. Core data types

### 7.1 Current-state reads

```rust
pub struct CoordinationReadRequest {
    pub consistency: CoordinationReadConsistency,
    pub view: CoordinationStateView,
}

pub enum CoordinationStateView {
    Snapshot,
    SnapshotV2,
    PlanState,
    RuntimeDescriptors,
    Summary,
}

pub struct CoordinationReadEnvelope<T> {
    pub consistency: CoordinationReadConsistency,
    pub freshness: CoordinationReadFreshness,
    pub authority: CoordinationAuthorityStamp,
    pub value: Option<T>,
    pub refresh_error: Option<String>,
}
```

The important rule is that `CoordinationAuthorityStamp` is **opaque to callers** but stable enough
for the runtime to reason about currentness, replay, and provenance.

### 7.2 Authority stamp

```rust
pub struct CoordinationAuthorityStamp {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub logical_repo_id: String,
    pub snapshot_id: String,
    pub transaction_id: Option<String>,
    pub committed_at: Option<u64>,
    pub provenance: CoordinationAuthorityProvenance,
}
```

Git may populate this from summary head commit + manifest digest.
SQLite or PostgreSQL may populate it from transaction id / commit sequence.
The rest of PRISM should treat it as the current authoritative identity token for a snapshot.

### 7.3 Transactions

```rust
pub struct CoordinationTransactionRequest {
    pub base: CoordinationTransactionBase,
    pub intents: Vec<CoordinationMutationIntent>,
    pub options: CoordinationTransactionOptions,
}

pub enum CoordinationTransactionBase {
    LatestStrong,
    ExpectedAuthorityStamp(CoordinationAuthorityStamp),
}

pub struct CoordinationTransactionResult {
    pub status: CoordinationTransactionStatus,
    pub committed: bool,
    pub authority: Option<CoordinationAuthorityStamp>,
    pub snapshot: Option<CoordinationCurrentState>,
    pub conflict: Option<CoordinationConflictInfo>,
    pub diagnostics: Vec<CoordinationTransactionDiagnostic>,
}
```

The abstraction must make one thing explicit:

- a transaction result is either **committed**, **rejected**, or **conflicted/retriable**
- local read models are not allowed to advance until a committed authority result exists
- explicit backend migration is outside normal transaction semantics and must happen through
  export/import cutover flows

### 7.4 Retained history

```rust
pub struct CoordinationHistoryRequest {
    pub query: CoordinationHistoryQuery,
    pub retention: CoordinationHistoryRetentionPolicy,
}

pub enum CoordinationHistoryQuery {
    SnapshotAt(CoordinationSnapshotSelector),
    ObjectTimeline(CoordinationObjectRef),
    TransactionTimeline(CoordinationTransactionHistoryQuery),
    RuntimeDescriptorTimeline { runtime_id: String },
}

pub struct CoordinationHistoryEnvelope {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub retention_status: CoordinationRetentionStatus,
    pub results: CoordinationHistoryResultSet,
}
```

The current Git backend should satisfy this by reconstructing retained history from local `.git`
state and shared-ref manifests. A PostgreSQL backend may satisfy it with indexed history tables or a
transaction log projection. The caller should not need to know which.

History richness is capability-driven:

- current-state semantics are mandatory
- retained-history depth and query richness may vary by backend

### 7.5 Diagnostics

The diagnostics surface should also become backend-neutral:

```rust
pub struct CoordinationAuthorityDiagnostics {
    pub backend_kind: CoordinationAuthorityBackendKind,
    pub availability: CoordinationAuthorityAvailability,
    pub latest_authority: Option<CoordinationAuthorityStamp>,
    pub verification_status: CoordinationVerificationStatus,
    pub retention_status: CoordinationRetentionStatus,
    pub runtime_descriptor_count: usize,
    pub backend_details: CoordinationAuthorityBackendDetails,
}
```

The Git backend may expose compaction status, manifest digests, shard lag, and history depth.
The SQLite backend may expose local DB integrity, migration, and retention metadata.
The PostgreSQL backend may expose replication status, transaction lag, and retention windows.
The top-level contract stays the same.

One rule is important for the public type family:

- `CoordinationAuthorityBackendKind` must include the actual supported backend family
  - `GitSharedRefs`
  - `Sqlite`
  - `Postgres`
- `CoordinationAuthorityBackendDetails` must be backend-family shaped
- backend-specific detail is allowed, but it must remain nested under the backend-neutral
  diagnostics surface rather than forcing Git-only detail types upward as the steady-state model

---

## 8. Required semantic guarantees

### 8.1 Eventual reads

Eventual reads must:

- return only an allowed lagging or derived view produced from authoritative state when such a view
  exists
- never include speculative local mutation state
- carry a freshness classification and authority stamp when available

### 8.2 Strong reads

Strong reads must:

- refresh against the authority substrate before answering
- fail closed or return a stale-but-verified fallback according to current runtime policy
- never treat newly fetched but unverified data as authoritative

### 8.3 Transaction commits

A committed transaction must guarantee:

- one coherent post-commit authoritative snapshot
- one post-commit authority stamp
- no visible partial success to other readers
- no local read-model advancement before authority commit is known

### 8.4 Conflict behavior

The abstraction must support deterministic conflict behavior:

- reject because the expected base authority stamp no longer matches
- optionally include conflict metadata so the caller can retry or re-evaluate

The abstraction should not expose backend-specific conflict internals like shard-head mismatch or
SQL serialization error codes directly to the coordination kernel.

### 8.5 Runtime descriptor publication

Runtime descriptor updates are ordinary coordination authority mutations and must obey the same
transaction rules as task/claim/artifact/review mutations.

### 8.6 History visibility

History queries must be honest about retention:

- available now
- compacted but summarized
- archived elsewhere
- unavailable beyond retention policy

---

## 9. Git shared-refs backend mapping

The Git backend should become an implementation of the abstraction, not a surface the rest of the
app calls directly.

### 9.1 Current state

The Git backend implements current-state reads by:

- eventual: reading the local materialized startup checkpoint / read model
- strong: polling/fetching the summary ref, verifying manifests, pinning the named shard/runtime
  heads, re-materializing local state, then answering

### 9.2 Transactions

The Git backend implements transaction commit by:

- loading the current authoritative summary snapshot
- applying mutation intents in memory
- producing changed shard/runtime commits and a new summary manifest
- publishing the changed ref set with `git push --atomic` and expected-old-head semantics
- returning a committed authority stamp only after remote atomic publish succeeds

### 9.3 History

The Git backend implements retained history by:

- traversing local `.git` shared-ref history
- reconstructing historical snapshots or transaction views from summary manifests and retained
  shard/runtime commits
- surfacing compaction/archive boundaries explicitly

### 9.4 Diagnostics

The current `shared_coordination_ref_diagnostics(...)` surface becomes a Git-backend implementation
of `CoordinationAuthorityStore::diagnostics(...)`.

---

## 10. DB-backed backend mapping

The first release-oriented authority family should be DB-backed.

That family should still satisfy the same semantic contract as any other true authority backend.

### 10.1 Backend family shape

The intended layering is:

- `CoordinationAuthorityStore`
  - `DbCoordinationAuthorityStore`
    - `SqliteCoordinationDb`
    - `PostgresCoordinationDb`

The point is to share one DB-backed implementation model where SQLite and Postgres are close
variants, rather than making product code care about two different SQL backends.

### 10.2 Current state

- eventual: read from the same current-authority path as strong by default
- strong: read directly from the active DB-backed authority implementation inside the required
  authority semantics
- optional later: eventual may use an explicitly configured lagging projection or materialized view

### 10.3 Transactions

- apply the mutation inside one DB transaction
- commit atomically
- return the resulting authority stamp and post-commit state

### 10.4 History

- serve object and transaction history from durable tables or history projections
- map retention to explicit history-window metadata

### 10.5 Runtime descriptors

- store runtime descriptors in ordinary authority tables
- expose them through the same query and diagnostics contract

### 10.6 Deployment defaults

- SQLite is the single-instance or local-service DB-backed authority option
- Postgres is the multi-instance or hosted production DB-backed authority option
- once the DB-backed authority family is functioning, SQLite is the default local
  `CoordinationAuthorityStore` backend selection
- extra coordination materialization is disabled by default for the DB-backed path
- optional separate coordination materialization remains a Postgres-oriented optimization, not the
  baseline path

The point is not to force DB-backed authority to imitate Git. The point is to let both backend
families satisfy the same coordination semantics while allowing DB-backed authority to skip
redundant coordination materialization by default.

---

## 11. Migration and portability implications

This abstraction enables a future where PRISM can:

- ship a DB-backed authority family first
- use SQLite for single-instance service deployments
- use Postgres for hosted or multi-instance deployments
- keep Git shared refs as a serious later or advanced repo-native backend
- migrate authority state between backends by explicit snapshot import/export and retained-history
  windows

The cutover rule must stay strict:

- one coordination root
- one active authority backend
- explicit export/import migration when changing that backend

The migration unit should be:

- a current authoritative snapshot
- authority metadata and provenance
- runtime descriptor state
- a bounded retained-history window or archive bundle

not raw backend internals.

---

## 12. End-state rule

The end-state rule for PRISM coordination should be:

- **all authoritative coordination access, present and historic, flows through the Coordination
  Authority Store**

Everything else in the runtime may remain optimized, cached, materialized, or disposable.
But no product surface should need to know whether authority currently comes from Git shared refs
or a DB-backed authority service.
