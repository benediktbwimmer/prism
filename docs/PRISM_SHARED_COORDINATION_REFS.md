# PRISM Shared Coordination Refs

Status: implemented architecture with validated coverage
Audience: PRISM core, coordination, storage, git execution, and MCP maintainers
Scope: shared multi-agent coordination without an external database

Concurrent-write replay, semantic merge, and deterministic rejection rules are defined in
[PRISM_COORDINATION_CONFLICT_HANDLING.md](./PRISM_COORDINATION_CONFLICT_HANDLING.md).

---

## 1. Summary

PRISM uses a dedicated shared Git ref namespace as its authoritative cross-branch
coordination plane.

This replaces the need for a separate shared remote database for durable coordination state such as:

- active plans
- task status
- claims
- leases
- handoffs
- publish state
- portfolio scheduling inputs

The core idea is:

- branch-local `.prism/state/**` remains useful for branch-published intent and derived mirrors
- a dedicated shared coordination ref becomes the authoritative live multi-agent state
- the shared ref uses the same snapshot plus signed manifest model that tracked `.prism` now uses
- PRISM reads and writes that shared ref with fetch, verify, compare-and-swap push, and retry
  semantics

This gives PRISM one repo-native shared control plane, keeps history auditable in Git, and avoids
splitting truth across Git plus an external coordination service.

### 1.1 Implemented coverage and closure status

As of April 4, 2026, the repo has already implemented the baseline needed to stop treating the
shared coordination ref as a startup database:

- signed manifest verification, file hashing, compare-and-swap publish, and live shared-ref
  hydration live in `crates/prism-core/src/shared_coordination_ref.rs`
- local startup checkpoint loading and invalidation live in
  `crates/prism-core/src/coordination_startup_checkpoint.rs` and
  `crates/prism-core/src/protected_state/runtime_sync.rs`
- explicit sync-path checkpoint refresh lives in
  `crates/prism-core/src/coordination_persistence.rs` and
  `crates/prism-core/src/watch.rs`
- branch publication, shared-coordination publication, and verified target integration are tracked
  as distinct durable lifecycle states across coordination, query, and MCP surfaces
- `manual_pr`, `auto_pr`, `direct_integrate`, and `external` integration modes all record trusted
  integration evidence and verified landing state instead of collapsing target integration into
  plain task completion
- authoritative lease publication, bounded local assisted renewal, compaction continuity metadata,
  retry counters, and degraded verification diagnostics are implemented on the live shared ref
- daemon startup timing boundaries live in `crates/prism-core/src/indexer.rs`,
  `crates/prism-core/src/session_bootstrap.rs`, `crates/prism-mcp/src/lib.rs`, and
  `crates/prism-mcp/src/daemon_mode.rs`
- the startup-decoupling execution plan is already closed in the shared coordination history and
  can be rendered through an explicit docs export when needed
- the full validation matrix and release-daemon dogfooding evidence live in
  `docs/SHARED_COORDINATION_REFS_VALIDATION.md`

No items in the matrix below remain intentionally deferred. The matrix is now a closure record for
the work that brought this document from target design to implemented reality.

Pre-v1 follow-up: this document describes the implemented baseline. The compatibility contract and
1000-agent-scale topology that should replace the single live ref as the hot write path are defined
in `docs/PRISM_SHARED_COORDINATION_V1_ARCHITECTURE.md`.

### 1.2 Closed implementation matrix

This matrix is the authoritative closure record for the shared-ref work tracked by
`plan:01knav51cj8vgw0zp49qgktzps`.

| Doc sections | Current status | Implemented result | Owning code paths | Implementation task |
| --- | --- | --- | --- | --- |
| `4.3`, `5.2`, `6.1`, `15 Phase 3`, `16` mirror-derivedness coverage | Completed | Branch-local `.prism/state/**` no longer mirrors full shared-coordination authority by default; remaining tracked exports are minimal, derived, and explicitly non-authoritative. | `crates/prism-core/src/shared_coordination_ref.rs`, `crates/prism-core/src/published_plans.rs`, `crates/prism-core/src/prism_doc/repo_state.rs`, `crates/prism-mcp/src/runtime_views.rs` | `coord-task:01knavc6b8qhznt2e1yrn7mmwh` |
| `10.1` to `10.4`, `10.6`, `10.9`, `11.3`, `11.4`, `14.3`, `14.4` | Completed | Branch publication, shared coordination publication, and verified target integration are distinct durable states in lifecycle storage, recovery logic, and query surfaces. | `crates/prism-core/src/session.rs`, `crates/prism-core/src/prism_doc/repo_state.rs`, `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-mcp/src/views.rs`, `crates/prism-ir/src/plans.rs` | `coord-task:01knavcv05nn8t2gc1z31gg0fy` |
| `10.5`, `10.7.2`, `10.7.3` | Completed | Squash and rebase landings record trusted integration evidence that binds review artifacts, target commits, and tasks without relying on ancestry alone. | `crates/prism-core/src/session.rs`, `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-mcp/src/views.rs`, `crates/prism-ir/src/plans.rs` | `coord-task:01knavdeaf5qs3zp6vycxgkg9t` |
| `10.8.1`, `10.7`, `14.3` | Completed | `manual_pr` requires review-backed evidence, observes external landing, and keeps partial publication recovery explicit and retryable. | `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-mcp/src/git_execution.rs`, `crates/prism-mcp/src/views.rs`, `crates/prism-core/src/session.rs` | `coord-task:01knave01nvcrq91vnt8np1mkg` |
| `10.8.2`, `10.7`, `14.3` | Completed | `auto_pr` creates or refreshes review artifacts, tracks merge enablement, and records verified target landing with durable evidence. | `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-mcp/src/git_execution.rs`, `crates/prism-mcp/src/views.rs`, `crates/prism-core/src/session.rs` | `coord-task:01knavehyd4a49fk7b4rnjxz67` |
| `10.8.3`, `10.7`, `14.4` | Completed | `direct_integrate` enforces preflight policy, emits trusted direct-landing metadata, and verifies the target update immediately. | `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-mcp/src/git_execution.rs`, `crates/prism-mcp/src/views.rs`, `crates/prism-core/src/session.rs` | `coord-task:01knavf33fkvpsc79jd5k9tkdj` |
| `9.3`, `10.9`, `11.2` to `11.4` | Completed | Scheduling, dependency gating, and read/query views distinguish completion, coordination publication, and target integration explicitly. | `crates/prism-core/src/session.rs`, `crates/prism-mcp/src/compact_tools/task_brief.rs`, `crates/prism-mcp/src/host_resources.rs`, `crates/prism-mcp/src/views.rs` | `coord-task:01knavfm5fk9gsvb56pbsb58hh` |
| `12.1`, `12.2`, `16` lease-renewal coverage | Completed | Durable lease acquisition, renewal, expiry, reclaim, and handoff facts are authoritative on the shared ref with low-frequency renewal writes. | `crates/prism-core/src/shared_coordination_ref.rs`, `crates/prism-core/src/session.rs`, `crates/prism-mcp/src/host_mutations.rs`, `crates/prism-ir/src/coordination.rs` | `coord-task:01knavg5dce3ytrw4pbsfrbz1g` |
| `12.3` | Completed | Assisted renewal is constrained to local watcher state, off by default, bounded, and explicitly non-authoritative. | `crates/prism-core/src/watch.rs`, `crates/prism-mcp/src/lease_advice.rs`, `crates/prism-mcp/src/host_resources.rs`, `crates/prism-mcp/src/runtime_views.rs` | `coord-task:01knavgtmdhbdm9hr190t7n7c7` |
| `13.2` to `13.4`, `14.1`, operator health surfaces in `17` | Completed | Compaction rewrites the live ref with continuity metadata, retry counters are durable, and operator diagnostics expose health, retention, and publish history explicitly. | `crates/prism-core/src/shared_coordination_ref.rs`, `crates/prism-core/src/session.rs`, `crates/prism-mcp/src/runtime_views.rs`, `crates/prism-mcp/src/runtime_state.rs` | `coord-task:01knavpwvj8r99x8yarrsam19g` |
| `14.2` to `14.4` | Completed | Verification failure, degraded-mode behavior, and split publication recovery block silent fallback and remain explicitly diagnosable and retryable. | `crates/prism-core/src/shared_coordination_ref.rs`, `crates/prism-core/src/protected_state/operators.rs`, `crates/prism-mcp/src/runtime_views.rs`, `crates/prism-mcp/src/host_resources.rs` | `coord-task:01knax3zr6tyxkcev2vzd2ptyg` |
| `16` | Completed | Targeted coverage for every lifecycle and integration mode, plus release-binary dogfooding against the live daemon, is recorded in the validation matrix. | `crates/prism-core/src/shared_coordination_ref.rs`, `crates/prism-core/src/tests.rs`, `crates/prism-mcp/src/tests.rs`, `crates/prism-mcp/src/tests/server_tool_calls.rs` | `coord-task:01knax49ge8qvv6yabya0k2xrc` |
| status and closure text across `1`, `15`, `17`, and the exportable repo projections | Completed | This document now describes implemented reality, and the repo projection renderer can close with no remaining shared-ref gap list. | `docs/PRISM_SHARED_COORDINATION_REFS.md`, `crates/prism-core/src/prism_doc.rs`, `crates/prism-core/src/prism_doc/repo_state.rs` | `coord-task:01knax5mvp5x40c6wnae7s3r9d` |

---

## 2. Problem

The current git execution policy work solves only part of the coordination problem.

When task completion pushes the current branch to `origin/<current-branch>`, that gives durable
publication for the branch, but it does not create one shared coordination truth across agents on
different branches.

That leaves several failure modes:

- Agent A claims or completes work on branch `task/a`, but Agent B on branch `task/b` does not see
  the coordination update until some later merge.
- Portfolio dispatch cannot reliably choose the next best actionable task across all active agents,
  because active coordination truth is still branch-scoped.
- Claims, leases, stale-work detection, and handoff state are not truly shared if they depend on
  branch-local publication.
- PRISM still needs some other shared substrate if it wants repo-wide execution continuity.

The repository already has a shared, replicated, durable transport: the Git remote.

The missing piece is to use that transport directly as the shared coordination plane instead of
treating branch-local `.prism` publication as the only durable surface.

---

## 3. Design Goals

Required goals:

- eliminate the need for a separate shared remote DB for durable coordination state
- make claims, leases, handoffs, task status, and dispatch inputs visible across branches
- preserve Git-native auditability, replication, and failure recovery
- reuse the existing snapshot plus signed manifest architecture rather than inventing a second
  storage model
- keep current HEAD state compact and bounded
- support optimistic concurrent multi-agent updates without central locking
- keep strict ownership boundaries for git execution policy
- preserve a cold-start path where a fresh agent can reconstruct meaningful shared coordination
  state from Git alone

Required non-goals:

- Git does not become a high-frequency presence transport
- PRISM does not try to emulate a low-latency transactional database
- the shared ref does not preserve every tiny operational mutation forever in live HEAD
- the shared ref does not replace normal branch history for user code publication

---

## 4. Core Decision

### 4.1 Shared coordination should be branch-independent

PRISM should store authoritative multi-agent coordination state in a dedicated shared ref namespace,
not in the current branch worktree.

Canonical authoritative ref family:

```text
refs/prism/coordination/<logical-repo-id>/live
refs/prism/coordination/<logical-repo-id>/tasks/<shard>
refs/prism/coordination/<logical-repo-id>/claims/<shard>
refs/prism/coordination/<logical-repo-id>/runtimes/<runtime-id>
```

For the initial implementation, the naming should be fixed rather than left open:

- one shared logical summary head per repository
- sharded task refs
- sharded claim refs
- per-runtime runtime refs
- writable through normal Git fetch and push
- not tied to `main` or any feature branch

PRISM should therefore treat:

```text
refs/prism/coordination/<logical-repo-id>/live
```

as the canonical live coordination summary ref for a logical repository.

The summary ref is the transaction root for authoritative reads. It must name the exact shard and
runtime ref heads that belong to the published snapshot.

Task, claim, and runtime refs remain physically sharded so routine hot writes stay narrow, but
those refs are not authoritative to readers on their own. A reader must only trust the shard heads
that the current summary manifest names.

### 4.2 The shared ref should be snapshot-oriented

The shared coordination ref should use the same fundamental model that
[`docs/PRISM_REPO_SNAPSHOT_REWRITE.md`](PRISM_REPO_SNAPSHOT_REWRITE.md)
defines for tracked `.prism`:

- stable snapshot shards
- one current signed manifest
- Git commit history as the durable historical substrate
- no live append-only blob growth in HEAD

This means the shared ref should publish current authoritative coordination state, not a forever
growing raw event log.

### 4.3 Branch-local `.prism` becomes a secondary plane for shared coordination

Once a shared coordination ref exists, branch-local `.prism/state/**` should no longer be treated
as the primary cross-agent authority for live coordination continuity.

Branch-local `.prism` still has value:

- branch-published intent
- reviewable mirrors
- cold-clone branch context
- optional export of relevant shared state into normal branch history

But for shared multi-agent execution, the authoritative source should be the shared ref.

### 4.4 Git remote becomes the shared coordination backbone

This design intentionally uses the Git remote as the shared control plane.

The remote provides:

- durability
- replication
- conflict detection
- compare-and-swap style ref updates
- audit-friendly history

For PRISM's coordination workload, those properties are more important than database-style query
latency.

---

## 5. State Planes

This design sharpens PRISM into four explicit state planes.

### 5.1 Shared coordination ref plane

The shared ref stores durable live coordination facts that must be visible across branches:

- active plans that matter repo-wide
- task lifecycle state
- task publish state
- claims
- leases
- reclaimable ownership state
- handoffs
- durable coordination artifacts
- portfolio ranking inputs and dispatch-relevant metadata

This is the authoritative plane for cross-branch coordination continuity.

### 5.2 Branch-local published plane

Branch-local `.prism/state/**` remains useful for:

- branch-specific published intent
- branch-specific plans
- derived or mirrored views of shared coordination where useful
- normal code-review visibility in the branch itself

This plane remains Git-branch-aware by design.

It should no longer be the only durable carrier of live shared execution continuity.

### 5.3 Runtime/shared journal plane

High-resolution operational history should remain in runtime/shared journals, not in live snapshot
HEAD:

- detailed mutation logs
- rich diagnostics
- watcher churn
- optional deep replay material
- temporary compaction sources

This matches the existing snapshot rewrite direction.

### 5.4 Process-local plane

Process-local state continues to hold:

- caches
- hot materializations
- request-path conveniences
- derived summaries
- local retry scratch state

This plane remains disposable.

---

## 6. Shared Ref Layout

The shared coordination ref should publish a compact tree that resembles the tracked snapshot layout
already used in `.prism/state`.

An illustrative commit tree for the shared ref:

```text
coordination/
  manifest.json
  plans/<plan-id>.json
  coordination/tasks/<task-id>.json
  coordination/artifacts/<artifact-id>.json
  coordination/claims/<claim-id>.json
  coordination/leases/<lease-id>.json
  indexes/plans.json
  indexes/tasks.json
```

This tree does not need to be checked out into the normal worktree.

The important properties are:

- stable per-object or narrow shard paths
- deterministic overwrite-in-place publication
- one manifest that digests the exact snapshot set
- narrow conflict surfaces

The exact shard names may evolve, but the live ref should stay snapshot-oriented and modular.

### 6.1 Mirror policy for branch-local `.prism/state/**`

Branch-local `.prism/state/**` should not mirror the full shared coordination snapshot by default.

The default rule should be:

- shared coordination ref is the only authoritative cross-branch live coordination plane
- branch-local `.prism/state/**` keeps branch-published intent and branch-scoped semantic state
- branch-local mirrors of shared coordination are allowed only when they are explicitly derived,
  bounded, and useful for branch review or task-scoped evidence

In practice that means:

- no automatic full mirror of claims, leases, handoffs, or portfolio inputs back into branch-local
  `.prism/state/**`
- task-local git execution evidence or review-facing derived summaries may be mirrored when useful
- any such mirror must be clearly derived from the shared coordination ref and must never become a
  second hidden authority

---

## 7. Manifest and Trust Model

The shared ref should reuse the tracked snapshot trust model rather than introducing a second one.

Each publish boundary should update one signed manifest that:

- records the exact file digests for the shared coordination snapshot
- records publisher identity
- records work context when available
- chains to the previous manifest digest

This follows the same model used in
[`tracked_snapshot.rs`](../crates/prism-core/src/tracked_snapshot.rs).

PRISM should also support signed commits on the shared ref, but the manifest remains valuable even
if commit-signature policy varies by environment.

Best combined integrity story:

- manifest signature attests snapshot content and continuity
- commit signature attests the ref update transport boundary

The manifest is content truth.
The commit is transport truth.

Both are useful.

---

## 8. Write Protocol

Shared coordination writes should use explicit optimistic concurrency.

### 8.1 Authoritative mutation flow

For a mutation that changes shared coordination state:

1. fetch the current shared coordination summary ref and any shard/runtime ref families needed to
   materialize the current authoritative snapshot
2. verify the current summary manifest and the shard/runtime commits it names
3. hydrate the current authoritative state from the summary ref plus those exact named shard heads
4. apply the mutation in memory
5. materialize deterministic trees for:
   - each changed task shard ref
   - each changed claim shard ref
   - each changed runtime ref
   - the summary `live` ref
6. create local commits for every changed ref without first mutating the published local ref heads
7. write the summary manifest so `summarySources` names the exact new shard/runtime commit heads
8. push all changed refs together with `git push --atomic` and expected-old-head semantics for
   every destination ref
9. only after that atomic push succeeds, update local derived state such as:
   - coordination read models
   - queue read models
   - startup checkpoints
   - any local compatibility/event mirrors

If the atomic push succeeds, the mutation is globally published.
If it fails, no ref becomes authoritative and the local cache must remain unchanged.

### 8.2 Compare-and-swap semantics

The publish must behave like compare-and-swap across the whole changed ref set:

- if every remote ref head is still the one PRISM fetched, the atomic push succeeds
- if any one destination ref changed first, the atomic push is rejected and none of the refs move

On rejection, PRISM should:

1. fetch the new head
2. re-verify the shared snapshot
3. rehydrate fresh state
4. re-evaluate whether the mutation still applies
5. retry or fail explicitly

This gives PRISM optimistic concurrency without a central database lock manager and without
exposing partial remote state.

### 8.3 Idempotence

Shared coordination mutations should be idempotent where possible.

Examples:

- a claim mutation that retries after a race should detect whether the task is now already claimed
  by another actor
- a lease renewal should confirm that the lease is still owned by the renewing principal
- a completion acknowledgment should confirm whether publication was already recorded

This is especially important once multiple agents are writing the same summary and shard ref
family.

### 8.4 Transport implementation boundary

The shared-ref write path should be implemented behind a narrow Git transport abstraction, but the
initial implementation should use shell Git as the only backend.

That gives PRISM:

- the same push, fetch, compare-and-swap, and credential behavior operators already use today
- easy observability through raw Git stderr/stdout and existing repo policy
- less ambiguity while the shared-ref semantics are still being proven

libgit2 or another plumbing backend may be added later behind the same abstraction once the shared
coordination protocol is stable, but v1 should not depend on dual backends.

---

## 9. Read and Hydration Protocol

PRISM should treat the shared coordination summary ref as the first durable source for cross-branch
live coordination.

### 9.1 Startup

On startup PRISM should:

1. load one local materialized coordination snapshot or checkpoint that was previously produced from
   a verified shared-ref import
2. build in-memory shared coordination runtime state once from that local materialized artifact
3. then hydrate branch-local `.prism/state/**` for branch-scoped published intent
4. layer runtime-only overlays and caches on top

The critical-path rule is:

- the shared summary ref remains the authority and replication source
- the local materialized coordination snapshot is the startup artifact
- startup must not treat the shared ref as a live database by fetch-verifying and reloading hundreds
  of shared snapshot files on every hot restart

#### 9.1.1 Local materialized startup checkpoint

The startup artifact should be one local materialized coordination checkpoint bundle, not a
best-effort scatter of unrelated local rows.

That bundle should contain:

- the current shared-coordination `CoordinationSnapshot`
- authored `PlanGraph` state
- execution overlays
- authority metadata for the imported shared ref
  - ref name
  - source head commit when known
  - canonical manifest digest when known
  - schema version
  - materialized timestamp

For the first implementation, PRISM should store that bundle in the local checkpoint store that the
daemon already controls through the checkpoint materializer, rather than inventing another startup
filesystem cache.

That gives startup one bounded local read:

1. load the local checkpoint bundle
2. build in-memory coordination runtime state once
3. reuse that loaded state across session bootstrap, workspace host, workspace runtime, and query
   serving layers

If the local checkpoint bundle is missing, startup may recover from other local runtime durability
state, but it should still avoid inline shared-ref fetch and verification on the critical path.

This order matters:

- shared coordination tells PRISM what the repo is actively doing together
- branch-local `.prism` tells PRISM what this branch publishes or mirrors

### 9.2 Live sync

The protected-state runtime sync work described in
[`docs/PROTECTED_STATE_RUNTIME_SYNC.md`](PROTECTED_STATE_RUNTIME_SYNC.md)
should be extended conceptually to shared coordination ref updates.

The runtime needs:

- a dedicated watcher or poller for the shared ref head
- targeted import of the shard/runtime heads named by the current summary manifest
- self-write suppression so PRISM does not churn on its own just-published coordination updates
- bounded reconciliation as a safety net

That explicit sync path should also own:

- fetch and manifest verification for the shared summary ref
- fetch and verification for the exact shard/runtime heads named by the summary manifest
- rebuild or refresh of the local materialized coordination snapshot or checkpoint
- invalidation keyed by shared-ref identity such as the live head commit, canonical manifest digest,
  or both

The sync path should compare the summary-head authority key against the last imported local key and
rebuild the local startup checkpoint only when that authority input changed or the local checkpoint
schema version changed.

The local SQLite cache is a read model. It must not become authoritative just because a local
mutation was attempted. Direct write paths may refresh that cache after a successful authoritative
publish, but the cache must never advance ahead of the shared summary and shard refs.

Longer term, the shared ref may also publish one compact canonical checkpoint artifact so sync no
longer has to rehydrate from hundreds of tiny files as its primary ingestion format. But even in
that future shape, daemon startup should still consume the local materialized checkpoint, not the
shared ref directly.

### 9.3 Query semantics

Query views that answer questions like:

- what is in progress
- what is reclaimable
- what is blocked
- what should I do next

should read shared coordination state first when the question is cross-branch or cross-agent in
nature.

---

## 10. Interaction with Branch Publication

This design separates two different facts that are currently too easy to conflate.

### 10.1 Published to branch

A task's code or PRISM-managed branch projection may have been published to the task branch.

That is branch-local code publication.

### 10.2 Published to shared coordination

The repo-wide coordination plane may now know that:

- the task is claimed
- the task is in progress
- the task is publish-pending
- the task is publish-failed
- the task was published successfully to its branch

That is shared coordination publication.

### 10.3 Integrated to target

Later, the branch may be merged or otherwise integrated into the task's target ref such as
`origin/main`.

That is a third fact, distinct from branch publication and distinct from shared coordination update.

PRISM should model these separately.

Recommended durable distinctions:

- `published_to_branch`
- `coordination_published`
- `integrated_to_target`

This makes the scheduler and downstream agents much less likely to act on false assumptions.

### 10.4 Target integration policy modes

PRISM should treat target integration as an explicit policy-controlled stage, not as an incidental
side effect of task completion.

Recommended per-plan policy modes:

- `manual_pr`
  - PRISM requires a PR-style review artifact and waits for human-approved landing on the target
    ref
  - PRISM may help create or update the artifact, but it must not land the change itself
- `auto_pr`
  - PRISM is allowed to create or update the PR artifact and enable merge automatically when policy
    permits
  - final target integration still depends on verification that the target ref actually contains the
    landed work
- `direct_integrate`
  - PRISM is allowed to integrate directly into the target ref when branch policy, validation, and
    trust rules permit it
  - this is the agent-native fast path for repos that want autonomous landing
- `external`
  - PRISM never initiates integration
  - it only observes and records that some external process landed the work on the target

This policy should be separate from start/completion git execution policy.

`require` governs who chooses source-code commit scope.
Target integration policy governs how published branch work reaches the target ref.

### 10.5 Task-level refs and evidence

To support integration cleanly, each task should carry durable git execution evidence including:

- `source_ref`
- `source_commit`
- `publish_ref`
- `publish_commit`
- `target_ref`
- `target_commit_at_publish`
- optional `review_artifact_ref`
  - for example a PR number, merge request id, or equivalent review object
- optional `integration_commit`
  - the commit on the target ref that PRISM accepted as the landing boundary
- `integration_mode`

The important distinction is:

- refs capture intent and routing
- commits capture concrete evidence

PRISM should store both.

### 10.6 Integration lifecycle

The task model should make target integration explicit.

Recommended durable lifecycle:

- `published_to_branch`
  - branch publication succeeded
- `integration_pending`
  - PRISM knows which published branch work is waiting to land on which target
- `integration_in_progress`
  - PRISM or an external system is currently attempting to land the work
- `integrated_to_target`
  - PRISM has verified that the target ref contains the task's landed work
- `integration_failed`
  - an integration attempt was made and failed explicitly

This lifecycle is separate from branch publication and separate from shared coordination publish.

### 10.7 Verification rules for target integration

PRISM should verify integration from Git state and durable evidence, not from optimistic status
assumptions.

The verification rules should be:

#### 10.7.1 Normal merge or fast-forward

If `publish_commit` is reachable from `target_ref`, the task is integrated.

This is the simplest and strongest case.

#### 10.7.2 Rebase integration

If the work was rebased before landing, the original `publish_commit` may no longer be reachable
from `target_ref`.

In that case PRISM should accept integration only if there is durable evidence binding the target
landing back to the task, for example:

- a review artifact that records the landed target commit
- an explicit integration mutation carrying the landed target commit
- a trusted merge metadata record written by PRISM at landing time

Reachability alone is not sufficient in a rebased flow because the original branch tip may have been
rewritten.

#### 10.7.3 Squash integration

Squash merge should be treated the same way as rebase from a verification perspective.

The target branch will usually contain a new commit that is not reachable from `publish_commit`.

So PRISM should require durable landing evidence for squash integration.

Recommended acceptance rule:

- squash integration is valid only when PRISM can bind the landed target commit back to the task
  through a review artifact or an explicit trusted integration record

This is the conservative choice.
It avoids PRISM pretending that a target commit is "obviously the squash" without evidence.

### 10.8 Integration flows by mode

#### 10.8.1 `manual_pr`

Flow:

1. task reaches `published_to_branch`
2. PRISM records `integration_pending`
3. PRISM requires a review artifact reference
4. a human approves and lands the change
5. PRISM observes that landing and records `integrated_to_target`

PRISM may assist with detection, but it must not perform the landing itself.

#### 10.8.2 `auto_pr`

Flow:

1. task reaches `published_to_branch`
2. PRISM creates or updates the review artifact
3. PRISM may enable auto-merge when policy permits
4. PRISM waits for the target to move
5. PRISM verifies the landing and records `integrated_to_target`

This mode is still review-artifact-centered even if merge enablement is automated.

#### 10.8.3 `direct_integrate`

Flow:

1. task reaches `published_to_branch`
2. PRISM verifies integration preconditions
  - target freshness
  - validation gates
  - branch policy
3. PRISM performs the merge, fast-forward, or other allowed integration action
4. PRISM verifies the target landing immediately
5. PRISM records `integrated_to_target`

This mode should be allowed only where the repo explicitly trusts agent-native landing.

#### 10.8.4 `external`

Flow:

1. task reaches `published_to_branch`
2. PRISM records `integration_pending`
3. some external system or human lands the work
4. PRISM later verifies that landing and records `integrated_to_target`

This mode is useful for repos that have other release or merge orchestration systems.

### 10.9 Relationship to completion

Task completion and target integration should remain distinct.

The recommended interpretation is:

- `completed`
  - the task's branch-scoped deliverable has been published and coordination state is consistent
- `integrated_to_target`
  - the deliverable has landed on the configured target ref

That means downstream scheduling can use whichever threshold is appropriate:

- some dependent tasks may proceed after `completed`
- others, especially target-sensitive release or follow-up work, may require
  `integrated_to_target`

This distinction is important and should not be collapsed.

---

## 11. Interaction with Git Execution Policy

The shared coordination ref strengthens the `require` execution model.

### 11.1 Strict ownership boundary stays the same

`require` should still mean:

- agent or human chooses user-code commit scope
- PRISM verifies repo and workflow invariants
- PRISM may finalize only PRISM-managed files
- PRISM must not guess source-code commit scope

### 11.2 Shared coordination solves the cross-branch visibility gap

Once the shared ref exists, PRISM can publish task execution state globally without waiting for
branch-local `.prism` state to land on another branch.

That means:

- claims become visible across branches immediately after shared coordination publish
- lease expiry and reclaimability become shared facts
- task completion and publish failure become shared facts
- portfolio dispatch can rank work across branches using one durable substrate

### 11.3 Completion ordering

For strict `require`, the recommended ordering is:

1. agent or human intentionally commits source-code changes
2. PRISM verifies completion preconditions
3. PRISM finalizes any PRISM-managed projection commit needed on the task branch
4. PRISM pushes the task branch
5. PRISM publishes the coordination outcome to the shared coordination ref

Only after step 5 should the task become globally completed.

If step 4 succeeds but step 5 fails, the branch is published but repo-wide coordination remains
stale until PRISM retries the shared coordination publish.

That is acceptable as long as the task does not falsely appear globally completed before the shared
coordination update succeeds.

### 11.4 Final strict completion semantics

The final strict model therefore becomes:

- code publication is branch-scoped
- coordination publication is shared-ref-scoped
- task completion is authoritative only after shared coordination publication succeeds

This keeps the system honest.

---

## 12. Leases and Heartbeats

Leases fit this model well.
Heartbeats fit only if modeled carefully.

### 12.1 Durable lease state belongs on the shared ref

The shared ref should carry the durable lease facts that other agents need:

- lease acquired
- lease renewed when meaningfully extended
- lease released
- lease expired or declared stale
- reclaim accepted
- handoff accepted

These are coordination facts, not local UI noise.

### 12.2 High-frequency heartbeat noise should not be durable

PRISM should not publish a new shared-ref commit every few seconds just to say "still alive."

That would create too much churn and too much Git history noise.

Instead, the shared ref should store durable lease timestamps such as:

- `lease_acquired_at`
- `lease_renewed_at`
- `lease_expires_at`
- optionally `last_authoritative_heartbeat_at`

The agent renews only when needed, for example:

- near expiry
- after a meaningful mutation
- when PRISM explicitly instructs it to call `heartbeat_lease`

This matches the lease rules already described in
[`docs/PRISM_AUTH_AND_IDENTITY_MODEL.md`](./PRISM_AUTH_AND_IDENTITY_MODEL.md).

### 12.3 Assisted renewal remains optional and local

An opt-in assisted heartbeat or watcher-based lease renewal policy may still exist for local
convenience, but it should remain:

- off by default
- explicitly trusted
- bounded
- non-authoritative as an identity proof

The shared ref should only carry the resulting authoritative lease extension, not every local
liveness signal that influenced it.

---

## 13. Growth, Retention, and Compaction

State growth is the main risk in a Git-backed coordination plane.

The design should control that risk explicitly.

### 13.1 HEAD should hold current state, not an ever-growing log

The live shared ref should contain:

- the current snapshot shards
- one current signed manifest
- small indexes

It should not contain a growing append-only operational log in HEAD.

This is exactly why the snapshot rewrite matters.

### 13.2 Retention layers

PRISM should use three retention layers:

1. hot live state
   - current active plans
   - active tasks
   - active claims
   - active leases
   - current portfolio inputs

2. warm recent history
   - a bounded recent commit window if needed for operator inspection
   - optional small recent journal shards

3. cold archive
   - old detailed history exported or compacted away from the live ref

### 13.3 Compaction policy

The shared ref should be explicitly compactable.

Compaction may:

- rewrite the live ref to a new compact snapshot baseline
- keep only the latest manifest chain head and a bounded recent tail
- export or archive older fine-grained detail elsewhere if desired

Unreachable older commits can then age out through normal Git garbage collection policies.

### 13.4 Signed continuity after compaction

Compaction must preserve trust continuity.

That can be done by:

- recording the previous manifest digest in the new compacted manifest when continuity is preserved
- or recording an explicit archive boundary / compaction checkpoint if the chain is intentionally
  rolled forward to a fresh baseline

The trust story must stay explicit.

The implemented live shared coordination ref now uses:

- regular publication records the successful publish timestamp and CAS retry count in the signed
  manifest
- live-ref compaction rewrites the ref to a single-head baseline only after minting a fresh signed
  manifest with `continuity_preserved` metadata and the previous manifest digest
- separate archive/export flows may use an explicit `archive_boundary` compaction record when they
  intentionally sever the live manifest chain

---

## 14. Failure and Recovery Model

### 14.1 Shared ref push races

If two agents update the same coordination state concurrently:

- one shared-ref push wins
- one loses compare-and-swap
- the loser refetches, re-evaluates, and retries or fails explicitly

This is normal and expected.

### 14.2 Verification failures

If the shared coordination summary manifest or any shard/runtime commit that it names fail
verification:

- PRISM must refuse authoritative hydration
- runtime should surface a clear degraded status
- repair or restore flows should operate on the shared ref family, not silently fall back to stale
  local shard heads

### 14.3 Partial completion publication

The most important failure case is:

- branch push succeeded
- shared coordination publish failed

In that case:

- the task branch is already published
- shared coordination is not yet updated
- the task must not appear globally completed

PRISM should record or retain a recoverable publish-pending state locally and retry the shared
coordination publish until it succeeds or is explicitly failed.

### 14.4 Shared coordination publish succeeded, branch publish failed

This ordering should usually be avoided for strict task completion.

PRISM should prefer branch publication first, then shared coordination publication, because shared
coordination should not advertise success before the branch publication has actually landed.

---

## 15. Migration Outcome

### Phase 1: shared-ref read path

- the shared coordination ref layout is defined and verified through a signed manifest
- runtime can fetch, verify, and hydrate the shared ref
- branch-local `.prism` remained in place during the initial read-path migration

### Phase 2: shared-ref write path

- claims, leases, task status, and publish metadata are written to the shared ref
- compare-and-swap push and retry handling is implemented
- shared-ref diagnostics are surfaced through normal PRISM runtime views

### Phase 3: branch-local `.prism` demotion

- branch-local `.prism` is no longer treated as primary live cross-branch authority
- remaining tracked exports are derived branch publication artifacts rather than hidden authority

### Phase 4: strict integration with git execution policy

- task execution and completion state publish through the shared ref
- branch publication, shared coordination publication, and verified target integration are distinct
- strict completion requires shared coordination publication to succeed before global completion

### Phase 5: retention and compaction

- live-ref compaction keeps the recent history window bounded
- archive/export flows can mark explicit archive boundaries when they intentionally sever continuity
- operator diagnostics expose history size, publish status, retry counts, and compaction health

---

## 16. Testing Coverage

The implementation now includes explicit coverage for:

- cold hydration from the shared coordination ref alone
- compare-and-swap push races between two writers
- multi-ref atomic push so task/claim/runtime shard refs and the summary ref never publish
  partially
- claim visibility across branches without merging the task branch
- lease renewal and expiry using low-frequency authoritative updates
- self-write suppression during local publication
- task completion where branch push succeeds and shared coordination publish succeeds
- task completion where branch push succeeds and shared coordination publish fails
- task completion where branch push fails before shared coordination publish
- compaction preserving manifest trust continuity
- branch-local `.prism` mirrors staying derived rather than becoming a second hidden authority
- `manual_pr`, `auto_pr`, `direct_integrate`, and `external` integration flows with trusted landing
  evidence and post-land verification
- degraded verification behavior, publish recovery, and operator repair surfaces

Concrete test and dogfooding evidence is recorded in `docs/SHARED_COORDINATION_REFS_VALIDATION.md`.

---

## 17. Resolved Decisions

The storage and CAS design decisions are now implemented for v1:

- use one live shared coordination summary head per logical repository plus sharded task, claim,
  and runtime refs
- treat the summary manifest as the transaction root for authoritative reads by pinning the exact
  shard/runtime heads that belong to the snapshot
- publish changed refs by creating local commits first, then using one `git push --atomic` with
  compare-and-swap expectations for the whole changed ref set
- update local read models and startup checkpoints only after authoritative publish succeeds
- keep branch-local `.prism/state/**` mirrors minimal, derived, and explicitly non-authoritative
- implement the shared-ref transport behind an abstraction, but ship shell Git first as the only
  backend
- expose operator diagnostics through normal PRISM read surfaces, with explicit shared-ref health,
  current head, last verified manifest, last successful publish, CAS retry counts, and compaction
  status
- keep the live ref bounded to the recent window (`SHARED_COORDINATION_HISTORY_MAX_COMMITS`) and
  make any older-history archive boundary explicit in signed metadata rather than implicit in Git
  parentage alone

---

## 18. Conclusion

PRISM now uses shared coordination refs as the durable cross-branch control plane.

This is the cleanest way to:

- remove the need for a shared remote DB
- make plans, claims, leases, and dispatch state truly shared across branches
- preserve Git-native auditability and cold-start recovery
- stay compatible with the snapshot plus signed manifest direction
- keep the strict `require` git execution model honest

The main operational constraint remains bounded-history growth, which is why compaction continuity,
archive-boundary metadata, and operator diagnostics are part of the implemented design rather than a
later follow-up.

That is manageable if PRISM treats the shared coordination ref family as:

- one signed summary manifest as the transaction root
- narrow shards
- bounded recent detail
- aggressive compaction for older history

That is a good fit for PRISM's workload.
