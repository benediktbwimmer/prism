# Coordination Mutation Persistence Rewrite

Status: active implementation contract
Audience: PRISM core, coordination, storage, git execution, and MCP maintainers
Scope: request-path coordination mutation latency under shared coordination refs

---

## 1. Problem

Shared-coordination-backed mutations are still too expensive for ordinary developer workflows.

Recent traces show that the system no longer stalls on request-path shared-ref hydration, but
ordinary coordination and git-execution mutations still spend multiple seconds inside the mutation
path itself.

The root cause is architectural, not incidental:

- a tiny authoritative coordination write still flows through the generic coordination mutation
  persistence pipeline
- that pipeline still couples the authoritative write to multiple derived or cache-like side
  effects
- request-path latency therefore reflects a mini republish pipeline rather than a minimal
  read-modify-CAS-publish operation

The rewrite goal is to restore the intended contract:

1. read current authoritative coordination revision
2. apply a small in-memory mutation
3. commit the authoritative delta
4. compare-and-swap publish the shared coordination ref
5. return success

Everything else should be explicitly justified as request-blocking, or moved off the hot path.

---

## 2. Current Baseline

### 2.1 Recent slow mutation traces

Recent repo-wide slow mutations showed:

- completion mutation around `8163ms`
  - `mutation.gitExecution.syncLoadedRevisionBefore`: about `199ms`
  - `mutation.gitExecution.preflight`: about `451ms`
  - `mutation.gitExecution.recordPublishIntentStep`: about `2944ms`
  - `mutation.gitExecution.pushBranch`: about `912ms`
  - `mutation.gitExecution.recordAuthoritativeStateStep`: about `2408ms`
  - final authoritative push: about `887ms`
- start mutation around `5929ms`
  - `mutation.gitExecution.syncLoadedRevisionBefore`: about `222ms`
  - `mutation.gitExecution.preflight`: about `440ms`
  - `mutation.gitExecution.applyRequestedMutationStep`: about `2568ms`
  - `mutation.gitExecution.recordTaskGitExecutionStep`: about `2677ms`

The earlier request-path shared-ref reload stall is already fixed. Recent traces show:

- `runtimeSync.reloadCoordination = 0ms`
- `runtimeSync.reloadInference = 0ms`
- `runtimeSync.reloadEpisodic = 0ms`

So the remaining problem is not shared-ref read hydration. It is mutation persistence and
follow-on request-path work.

### 2.2 Current hot path in code

The key request-path chain is:

- [`run_workspace_coordination_step()`](/Users/bene/code/prism-codex-d/crates/prism-mcp/src/host_mutations.rs)
- [`mutate_coordination_with_session_guarded()`](/Users/bene/code/prism-codex-d/crates/prism-core/src/session.rs)
- [`persist_coordination_mutation_state_for_root_with_session_observed()`](/Users/bene/code/prism-codex-d/crates/prism-core/src/coordination_persistence.rs)

Inside the current persistence path, a tiny authoritative mutation still synchronously does:

- commit authoritative coordination batch
- sync shared coordination ref
- sync tracked coordination snapshot
- save shared-coordination startup checkpoint
- build coordination read model
- build coordination queue read model
- save coordination read model
- save coordination queue read model
- maybe compact coordination events

That is the main latency bug.

---

## 3. Target Latency Budgets

The rewrite should optimize for warm ordinary developer flows first.

### 3.1 Warm unchanged read budget

For a request that does not need to observe a newer authoritative coordination head:

- target: `0ms` to `100ms`
- hard ceiling: `250ms`

This is already largely achieved after the shared-ref read-path reload fix.

### 3.2 Warm small mutation budget

For an ordinary small coordination mutation on a warm runtime, such as:

- task status update
- publish intent update
- publish failed update
- small task metadata patch
- claim/lease mutation

target:

- authoritative mutation without git push: `<= 250ms`
- warm common case aspirational target: `<= 100ms`

hard ceiling:

- `<= 500ms`

### 3.3 Warm git-execution mutation budget

For require-mode start and completion flows on a warm runtime:

- preflight plus authoritative task-state updates should usually remain under `500ms`
- end-to-end completion that includes an actual Git push may exceed that, but the non-push
  coordination overhead should stay below `250ms`

The push itself is not treated as a coordination performance regression if the rest of the
pipeline is already thin.

### 3.4 Post-head-change reload budget

For the first request after a real shared-ref head change:

- target: low hundreds of milliseconds
- hard ceiling: about `1s`

Cold or catch-up work must be explicit, not hidden inside normal warm requests.

---

## 4. Request-Path Classification

Every synchronous phase in a coordination mutation should be classified as one of:

- `authoritative`
- `cache`
- `projection`
- `background`

### 4.1 Authoritative request-path work

These phases must remain blocking for correctness:

- read current authoritative revision and applicable baseline state
- validate preconditions against that revision
- apply mutation in memory
- append or patch authoritative coordination state
- compare-and-swap publish shared coordination ref state
- return success or explicit conflict/failure

For git-execution flows, request-path authoritative work also includes:

- git preflight required to determine whether the mutation may proceed
- branch push required for the workflow to succeed
- authoritative task-state acknowledgement of publication outcome

### 4.2 Cache work

These surfaces are derived from authoritative coordination state and may lag behind under explicit
revision tracking:

- coordination read model
- coordination queue read model
- startup checkpoint

They should not block ordinary mutations.

### 4.3 Projection work

These surfaces are useful and durable, but should not define the success of an authoritative
mutation:

- tracked `.prism/state/**` coordination snapshot shards
- tracked `.prism/state/manifest.json`
- repo-published plan and coordination mirrors
- PRISM docs and repo projection artifacts

They must become explicitly lagging projections keyed to authoritative revision.

### 4.4 Background work

These operations should happen off the request path:

- rebuilding read models
- writing queue models
- saving startup checkpoints
- syncing tracked snapshot mirrors
- PRISM doc regeneration
- event compaction
- deep reconciliation of lagging derived surfaces

---

## 5. The New Contract

### 5.1 Minimal authoritative mutation

A successful small coordination mutation should mean:

1. the mutation read the latest acceptable authoritative revision
2. the mutation produced a new authoritative coordination state
3. that new state was published to the shared coordination ref under CAS semantics
4. the runtime recorded which derived surfaces are now behind

It should not mean:

- tracked snapshot is already republished
- startup checkpoint is already rewritten
- read models are already rebuilt
- queue models are already saved
- PRISM docs are already regenerated

### 5.2 Lagging-surface revision tracking

At minimum, the system should explicitly track:

- authoritative shared coordination revision
- tracked snapshot revision
- startup checkpoint revision
- read model revision
- queue read model revision
- repo projection revision

Each surface may be:

- current
- stale but acceptable
- rebuilding
- failed

### 5.3 Concrete revision model

The rewrite should stop relying on implicit freshness and instead persist an explicit coordination
materialization ledger.

The exact representation may vary, but the model should be equivalent to:

```text
authoritative_revision: u64
tracked_snapshot_revision: u64 | null
startup_checkpoint_revision: u64 | null
read_model_revision: u64 | null
queue_read_model_revision: u64 | null
repo_projection_revision: u64 | null

tracked_snapshot_state: current | stale | rebuilding | failed
startup_checkpoint_state: current | stale | rebuilding | failed
read_model_state: current | stale | rebuilding | failed
queue_read_model_state: current | stale | rebuilding | failed
repo_projection_state: current | stale | rebuilding | failed
```

Derived surfaces are current only when their revision equals the authoritative revision they are
supposed to mirror.

They are stale when:

- the authoritative revision advanced beyond their recorded revision
- they are still safe to serve within the relevant read contract

They are rebuilding when:

- work has been scheduled or started to close the gap

They are failed when:

- the last rebuild attempt failed and the authoritative revision still exceeds the surface
  revision

This model must be inspectable from diagnostics and traces.

### 5.4 Mutation outcome contract

The mutation contract should be explicit for each request-path outcome.

#### Successful ordinary coordination mutation

On success, the system guarantees:

1. the latest acceptable authoritative revision was read
2. the mutation was applied to authoritative coordination state
3. the shared coordination ref CAS update succeeded
4. lagging derived surfaces were marked dirty against the new authoritative revision

It does not guarantee:

- tracked snapshot is already updated
- startup checkpoint is already updated
- read models are already rebuilt
- queue models are already rebuilt
- repo projections or PRISM docs are already regenerated

#### CAS-conflict outcome

On CAS conflict, the system guarantees:

- no authoritative success was falsely reported
- the caller gets a retryable conflict or a reconciled retry path
- dirty derived-surface revisions are not advanced past the last confirmed authoritative revision

#### Crash-after-authoritative-write outcome

If a process crashes after the authoritative write but before derived surfaces catch up:

- the authoritative revision remains the source of truth
- background or recovery rebuild must be able to reconstruct all lagging surfaces from that state
- startup must never interpret missing derived surfaces as authoritative loss

#### Derived-surface rebuild failure

If tracked snapshot, checkpoint, read model, or projection rebuild fails:

- the authoritative revision remains valid
- the failing surface remains marked stale or failed
- readers either serve an explicitly stale surface under contract or rebuild lazily

### 5.5 Git-execution correctness rules

The rewrite must preserve these semantics:

- require-mode preflight remains blocking
- publish intent must be durable before the branch publish is attempted
- branch push remains blocking for successful completion
- final authoritative state must distinguish:
  - publish pending
- publish failed
- published
- background projections may lag, but authoritative task state may not lie

### 5.6 Git-execution lifecycle under the thinner path

Require-mode flows should look like this:

#### Start

1. load authoritative revision
2. run preflight
3. patch task state and git-execution state through the authoritative fast path
4. mark derived surfaces dirty
5. return

#### Completion success

1. load authoritative revision
2. run preflight
3. durably record publish intent through the authoritative fast path
4. push branch / protected follow-up commit as required
5. durably record final authoritative publication outcome through the authoritative fast path
6. mark derived surfaces dirty
7. return

#### Completion failure after publish intent

1. load authoritative revision
2. run preflight
3. durably record publish intent
4. branch publish fails
5. durably record publish failure
6. return failure without false completion

The important rule is that each authoritative task-state transition stays tiny and honest, while
all rebuild and mirror work is free to lag behind it.

---

## 6. Implementation Sequence

The rewrite should happen in this order:

1. define and freeze the minimal authoritative mutation contract
2. extract a dedicated authoritative persistence path from the generic mutation pipeline
3. add a task-scoped fast-path patch API for git-execution micro-updates
4. move tracked snapshot publication off the request path
5. move startup checkpoint writes off the request path
6. move read-model and queue-model writes off the request path
7. make shared coordination ref publication incremental over touched objects and indexes
8. rebind git-execution start and completion flows onto the thin path
9. expose lag and hot-path diagnostics
10. validate CAS races, crash recovery, multi-worktree convergence, and real-world latency

---

## 7. Immediate Code Targets

The first code changes should focus on:

- [`host_mutations.rs`](/Users/bene/code/prism-codex-d/crates/prism-mcp/src/host_mutations.rs)
  - stop routing git-execution micro-updates through the full generic coordination mutation path
- [`session.rs`](/Users/bene/code/prism-codex-d/crates/prism-core/src/session.rs)
  - split authoritative mutation commit from derived-surface materialization
- [`coordination_persistence.rs`](/Users/bene/code/prism-codex-d/crates/prism-core/src/coordination_persistence.rs)
  - separate authoritative commit from tracked snapshot sync, checkpoint save, read-model writes,
    and compaction
- [`shared_coordination_ref.rs`](/Users/bene/code/prism-codex-d/crates/prism-core/src/shared_coordination_ref.rs)
  - reduce publish cost from full snapshot restage toward incremental touched-object updates
- [`tracked_snapshot.rs`](/Users/bene/code/prism-codex-d/crates/prism-core/src/tracked_snapshot.rs)
  - reframe coordination snapshot publication as a lagging derived surface rather than a blocking
    mutation obligation
- [`coordination_startup_checkpoint.rs`](/Users/bene/code/prism-codex-d/crates/prism-core/src/coordination_startup_checkpoint.rs)
  - make checkpoint persistence explicitly cache-like and revision-keyed

---

## 8. Success Condition

This rewrite is successful when normal coordination mutations feel boring again.

That means:

- warm ordinary task mutations are sub-second by default
- request-path latency mostly reflects actual authoritative work
- cold or catch-up work is surfaced explicitly as deferred materialization
- git-execution completion cost is dominated by real git work, not coordination persistence churn
- multi-worktree correctness remains intact under CAS races and recovery
