# Rewrite coordination mutation persistence for sub-second latency

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:8cb2643f9dbe80cd8e697ff46cf28a5930cbe32ae1a3c220d986514a3179157b`
- Source logical timestamp: `unknown`
- Source snapshot: `12 nodes, 19 edges, 2 overlays`

## Overview

- Plan id: `plan:01knc64s1zzzxq7x324fms1cyc`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `12`
- Edges: `19`

## Goal

Make shared-coordination-backed PRISM mutations fast enough for normal developer use by shrinking the request-path work to a minimal authoritative write and moving snapshot/checkpoint/read-model/projection side effects off the hot path, while preserving correctness under CAS races, git-execution flows, and multi-worktree convergence.

## Git Execution Policy

- Start mode: `require`
- Completion mode: `require`
- Target branch: `main`
- Target ref: `origin/main`
- Require task branch: `true`
- Max commits behind target: `0`

## Branch Snapshot Export

- Shared coordination authority: shared coordination ref when present; branch-local `.prism/state/**` is not cross-branch authority
- Snapshot manifest: `.prism/state/manifest.json` (derived branch export)
- Snapshot plan shard: `.prism/state/plans/plan:01knc64s1zzzxq7x324fms1cyc.json` (derived branch export)
- Legacy migration log path: none; tracked snapshot plan shards are derived exports, not current shared coordination authority

## Root Nodes

- `coord-task:01knc6cfapwxtkfnvhcez2wszv`

## Nodes

### Freeze latency budgets and classify every synchronous coordination side effect

- Node id: `coord-task:01knc6cfapwxtkfnvhcez2wszv`
- Kind: `investigate`
- Status: `completed`
- Summary: Capture the current trace-backed latency budget, enumerate every synchronous phase in coordination mutations, and classify each one as authoritative, cache, projection, or deferred background work so the rewrite has a hard contract instead of ad hoc optimization.
- Priority: `100`

#### Acceptance

- Warm unchanged reads, warm small mutations, and first-load-after-head-change all have explicit target latency envelopes [any]
- Every current request-path phase is classified as authoritative, cache, projection, or background work with a stated correctness reason [any]
- The rewrite contract explicitly states which mutation phases must remain blocking to preserve CAS correctness and user-visible semantics [any]

#### Tags

- `authoritative-path`
- `coordination`
- `performance`

### Define the minimal authoritative coordination persist contract and revision model

- Node id: `coord-task:01knc6cswpwztpebeb7mmtmqtc`
- Kind: `decide`
- Status: `in_progress`
- Summary: Specify the exact request-path contract for a successful coordination mutation: which revision is checked, which authoritative state is written, which shared-ref CAS obligations are required before returning success, and which follow-on materializations are allowed to lag behind under explicit dirty revisions.
- Priority: `99`

#### Acceptance

- The authoritative commit contract distinguishes blocking shared truth from lagging caches and derived projections [any]
- Dirty revision markers or equivalent lag-tracking metadata are specified for tracked snapshot, startup checkpoint, read models, and repo projections [any]
- The contract covers start, complete, publish-failed, and CAS-conflict mutation outcomes without ambiguity [any]

#### Tags

- `coordination`
- `performance`
- `storage-design`

### Extract a minimal authoritative mutation commit path from the generic persistence pipeline

- Node id: `coord-task:01knc6d53pww48ryg7se29dp3y`
- Kind: `edit`
- Status: `ready`
- Summary: Split the current generic coordination mutation persistence path so a request-path mutation can commit the authoritative delta and shared coordination CAS update without synchronously paying for tracked snapshot publication, startup checkpoint writes, read-model saves, or compaction.
- Priority: `98`

#### Acceptance

- A dedicated authoritative mutation path exists and is used for hot-path coordination updates [any]
- The old generic persistence wrapper no longer forces tracked snapshot publication, startup checkpoint writes, read-model saves, and compaction on every authoritative mutation [any]
- Trace output distinguishes the minimal authoritative commit phase from any deferred follow-on work [any]

#### Tags

- `coordination`
- `hot-path`
- `performance`

### Move tracked snapshot publication off the request path behind revisioned background reconciliation

- Node id: `coord-task:01knc6dqrewn2hah2p54xya9xw`
- Kind: `edit`
- Status: `ready`
- Summary: Demote tracked `.prism/state` coordination publication from mandatory synchronous mutation work to a lagging derived surface keyed by authoritative coordination revision, with explicit dirty markers and background reconciliation so request latency stops depending on rewriting repo-published snapshot shards and manifests.
- Priority: `96`

#### Acceptance

- Hot-path coordination mutations can succeed without synchronously rewriting tracked coordination snapshot shards [any]
- Tracked snapshot lag is explicitly versioned and observable rather than silently hidden [any]
- Background reconciliation can catch tracked snapshot state up to the authoritative shared coordination revision after crashes or restarts [any]

#### Tags

- `background`
- `projections`
- `tracked-snapshot`

### Move startup checkpoint persistence off the request path and make it a revision-keyed cache

- Node id: `coord-task:01knc6dzagp1zbg7ne0df1gyqh`
- Kind: `edit`
- Status: `ready`
- Summary: Treat the shared-coordination startup checkpoint as a startup optimization cache rather than a blocking mutation obligation by keying it to authoritative revision and recomputing it asynchronously or lazily when stale.
- Priority: `95`

#### Acceptance

- Request-path mutations no longer block on saving the startup checkpoint [any]
- Checkpoint validity is determined by explicit authoritative revision or manifest/head binding rather than incidental timing [any]
- Startup still prefers a valid checkpoint when present and falls back safely when it is stale or absent [any]

#### Tags

- `background`
- `cache`
- `startup-checkpoint`

### Treat coordination read models and queue models as lagging caches instead of blocking writes

- Node id: `coord-task:01knc6e8tbctzsqh0ngt4963ak`
- Kind: `edit`
- Status: `ready`
- Summary: Stop rebuilding and saving coordination read models and queue models synchronously on every mutation. Key them by authoritative revision, refresh them lazily or in background workers, and surface staleness explicitly when a reader outruns the cache.
- Priority: `94`

#### Acceptance

- Hot-path mutations no longer save coordination read models or queue models synchronously [any]
- Readers can tell whether a served read model is current, stale-but-acceptable, or needs rebuild [any]
- Read-model rebuilds remain correct across restarts, compaction, and concurrent authoritative mutations [any]

#### Tags

- `background`
- `queue-model`
- `read-model`

### Make shared coordination ref publication incremental over touched objects and indexes

- Node id: `coord-task:01knc6egjk5ay9wm7amhm2p86c`
- Kind: `edit`
- Status: `ready`
- Summary: Replace full shared-coordination restaging on each publish with incremental object and index updates keyed by the touched plans, tasks, claims, artifacts, and reviews so the authoritative CAS publish itself becomes much cheaper under normal small mutations.
- Priority: `93`

#### Acceptance

- Small authoritative mutations no longer rewrite the full shared coordination staging tree [any]
- Manifest generation and object publication reuse prior state for untouched files wherever correctness allows [any]
- CAS retry and reconciliation still converge correctly when concurrent writers touch disjoint or overlapping coordination objects [any]

#### Tags

- `cas`
- `incremental`
- `shared-ref`

### Rebind git-execution start and completion flows onto the minimal authoritative patch path

- Node id: `coord-task:01knc6ffec2vfzbrw66hbmk559`
- Kind: `edit`
- Status: `ready`
- Summary: Move require-mode start and completion workflows onto the new fast authoritative patch path so preflight results, publish intent, publish failure, and final authoritative publication state do not each trigger full coordination persistence and derived-surface regeneration.
- Priority: `92`

#### Acceptance

- Recent start and completion traces show git-execution task-state steps using the fast authoritative patch path rather than the generic full persistence wrapper [any]
- Require-mode correctness remains intact for publish intent, branch push, publish failure, and final authoritative publication acknowledgement [any]
- The common start and complete mutation path reaches the new warm latency budget in dogfooding traces [any]

#### Tags

- `git-execution`
- `performance`
- `workflow`

### Expose deferred-surface lag, dirty revisions, and hot-path diagnostics explicitly

- Node id: `coord-task:01knc6fr2anfaz4k96dzy0168q`
- Kind: `validate`
- Status: `ready`
- Summary: Add first-class diagnostics for authoritative revision, lagging tracked snapshot revision, lagging checkpoint/read-model revisions, and hot-path phase timings so operators and agents can see when deferred materializations are healthy, behind, or broken instead of experiencing invisible latency or stale reads.
- Priority: `91`

#### Acceptance

- Runtime and query surfaces expose authoritative revision plus lagging revision state for each deferred surface [any]
- Trace output and diagnostics can distinguish authoritative write latency from deferred catch-up latency [any]
- Operators can tell when a mutation is fast but a derived surface is stale, rebuilding, or failed [any]

#### Tags

- `diagnostics`
- `observability`
- `performance`

### Validate CAS races, crash recovery, and multi-worktree convergence under the split pipeline

- Node id: `coord-task:01knc6fzv6fj09xzc6c5rpnkma`
- Kind: `validate`
- Status: `ready`
- Summary: Prove that the thinner authoritative mutation path still converges correctly under compare-and-swap conflicts, writer races, daemon restarts, background lag, and competing worktrees that observe different shared-ref heads at different times.
- Priority: `90`

#### Acceptance

- Concurrent disjoint and overlapping writers converge without losing authoritative task state [any]
- Crash and restart recovery rebuilds lagging caches and projections from authoritative shared state without manual repair [any]
- Multi-worktree dogfooding shows no false completion, stale publish acknowledgement, or shared-ref divergence under normal use [any]

#### Tags

- `cas`
- `multi-worktree`
- `recovery`

### Dogfood the rewrite against real mutation traces until warm operations are boring again

- Node id: `coord-task:01knc6gcwq6fbqvh9k3vrc3es7`
- Kind: `validate`
- Status: `ready`
- Summary: Use repo-wide MCP traces and real task workflows to prove that normal warm coordination mutations are back in the sub-second regime, that cold-path costs are explicit and bounded, and that the developer experience no longer depends on hidden projection churn.
- Priority: `89`

#### Acceptance

- Repo-wide slow-call traces show ordinary warm coordination mutations meeting the agreed latency budget under real work [any]
- Any remaining cold or catch-up costs are surfaced explicitly as deferred materialization or first-load work rather than hidden inside normal mutations [any]
- The final documented invariants explain why ordinary task start and completion are now close to a minimal read-modify-CAS-publish flow [any]

#### Tags

- `dogfood`
- `latency`
- `ux`

### Add a task-scoped fast-path patch API for git-execution and status micro-updates

- Node id: `coord-task:01knc6jcs2rn6sj8a60xwqkyxm`
- Kind: `edit`
- Status: `ready`
- Summary: Introduce a narrow task-patch write path for micro-updates such as preflight status, publish intent, publish failure, authoritative publish acknowledgement, and small task status transitions so git-execution flows stop routing each step through the full generic coordination persistence pipeline.
- Priority: `97`

#### Acceptance

- Task-scoped authoritative patches no longer require the generic full coordination mutation wrapper [any]
- Git-execution micro-updates can patch a single task plus its execution overlay without rebuilding unrelated coordination state [any]
- CAS conflict handling and retry semantics remain correct for concurrent patches to other tasks or plans [any]

#### Tags

- `coordination`
- `git-execution`
- `hot-path`

## Edges

- `plan-edge:coord-task:01knc6cswpwztpebeb7mmtmqtc:depends-on:coord-task:01knc6cfapwxtkfnvhcez2wszv`: `coord-task:01knc6cswpwztpebeb7mmtmqtc` depends on `coord-task:01knc6cfapwxtkfnvhcez2wszv`
- `plan-edge:coord-task:01knc6d53pww48ryg7se29dp3y:depends-on:coord-task:01knc6cswpwztpebeb7mmtmqtc`: `coord-task:01knc6d53pww48ryg7se29dp3y` depends on `coord-task:01knc6cswpwztpebeb7mmtmqtc`
- `plan-edge:coord-task:01knc6dqrewn2hah2p54xya9xw:depends-on:coord-task:01knc6d53pww48ryg7se29dp3y`: `coord-task:01knc6dqrewn2hah2p54xya9xw` depends on `coord-task:01knc6d53pww48ryg7se29dp3y`
- `plan-edge:coord-task:01knc6dzagp1zbg7ne0df1gyqh:depends-on:coord-task:01knc6d53pww48ryg7se29dp3y`: `coord-task:01knc6dzagp1zbg7ne0df1gyqh` depends on `coord-task:01knc6d53pww48ryg7se29dp3y`
- `plan-edge:coord-task:01knc6e8tbctzsqh0ngt4963ak:depends-on:coord-task:01knc6d53pww48ryg7se29dp3y`: `coord-task:01knc6e8tbctzsqh0ngt4963ak` depends on `coord-task:01knc6d53pww48ryg7se29dp3y`
- `plan-edge:coord-task:01knc6egjk5ay9wm7amhm2p86c:depends-on:coord-task:01knc6d53pww48ryg7se29dp3y`: `coord-task:01knc6egjk5ay9wm7amhm2p86c` depends on `coord-task:01knc6d53pww48ryg7se29dp3y`
- `plan-edge:coord-task:01knc6ffec2vfzbrw66hbmk559:depends-on:coord-task:01knc6dqrewn2hah2p54xya9xw`: `coord-task:01knc6ffec2vfzbrw66hbmk559` depends on `coord-task:01knc6dqrewn2hah2p54xya9xw`
- `plan-edge:coord-task:01knc6ffec2vfzbrw66hbmk559:depends-on:coord-task:01knc6dzagp1zbg7ne0df1gyqh`: `coord-task:01knc6ffec2vfzbrw66hbmk559` depends on `coord-task:01knc6dzagp1zbg7ne0df1gyqh`
- `plan-edge:coord-task:01knc6ffec2vfzbrw66hbmk559:depends-on:coord-task:01knc6e8tbctzsqh0ngt4963ak`: `coord-task:01knc6ffec2vfzbrw66hbmk559` depends on `coord-task:01knc6e8tbctzsqh0ngt4963ak`
- `plan-edge:coord-task:01knc6ffec2vfzbrw66hbmk559:depends-on:coord-task:01knc6egjk5ay9wm7amhm2p86c`: `coord-task:01knc6ffec2vfzbrw66hbmk559` depends on `coord-task:01knc6egjk5ay9wm7amhm2p86c`
- `plan-edge:coord-task:01knc6ffec2vfzbrw66hbmk559:depends-on:coord-task:01knc6jcs2rn6sj8a60xwqkyxm`: `coord-task:01knc6ffec2vfzbrw66hbmk559` depends on `coord-task:01knc6jcs2rn6sj8a60xwqkyxm`
- `plan-edge:coord-task:01knc6fr2anfaz4k96dzy0168q:depends-on:coord-task:01knc6dqrewn2hah2p54xya9xw`: `coord-task:01knc6fr2anfaz4k96dzy0168q` depends on `coord-task:01knc6dqrewn2hah2p54xya9xw`
- `plan-edge:coord-task:01knc6fr2anfaz4k96dzy0168q:depends-on:coord-task:01knc6dzagp1zbg7ne0df1gyqh`: `coord-task:01knc6fr2anfaz4k96dzy0168q` depends on `coord-task:01knc6dzagp1zbg7ne0df1gyqh`
- `plan-edge:coord-task:01knc6fr2anfaz4k96dzy0168q:depends-on:coord-task:01knc6e8tbctzsqh0ngt4963ak`: `coord-task:01knc6fr2anfaz4k96dzy0168q` depends on `coord-task:01knc6e8tbctzsqh0ngt4963ak`
- `plan-edge:coord-task:01knc6fr2anfaz4k96dzy0168q:depends-on:coord-task:01knc6egjk5ay9wm7amhm2p86c`: `coord-task:01knc6fr2anfaz4k96dzy0168q` depends on `coord-task:01knc6egjk5ay9wm7amhm2p86c`
- `plan-edge:coord-task:01knc6fzv6fj09xzc6c5rpnkma:depends-on:coord-task:01knc6ffec2vfzbrw66hbmk559`: `coord-task:01knc6fzv6fj09xzc6c5rpnkma` depends on `coord-task:01knc6ffec2vfzbrw66hbmk559`
- `plan-edge:coord-task:01knc6fzv6fj09xzc6c5rpnkma:depends-on:coord-task:01knc6fr2anfaz4k96dzy0168q`: `coord-task:01knc6fzv6fj09xzc6c5rpnkma` depends on `coord-task:01knc6fr2anfaz4k96dzy0168q`
- `plan-edge:coord-task:01knc6gcwq6fbqvh9k3vrc3es7:depends-on:coord-task:01knc6fzv6fj09xzc6c5rpnkma`: `coord-task:01knc6gcwq6fbqvh9k3vrc3es7` depends on `coord-task:01knc6fzv6fj09xzc6c5rpnkma`
- `plan-edge:coord-task:01knc6jcs2rn6sj8a60xwqkyxm:depends-on:coord-task:01knc6d53pww48ryg7se29dp3y`: `coord-task:01knc6jcs2rn6sj8a60xwqkyxm` depends on `coord-task:01knc6d53pww48ryg7se29dp3y`

## Execution Overlays

- Node: `coord-task:01knc6cfapwxtkfnvhcez2wszv`
  git execution status: `publish_pending`
  pending task status: `completed`
  source ref: `task/coordination-mutation-rewrite-plan`
  target ref: `origin/main`
  publish ref: `task/coordination-mutation-rewrite-plan`
- Node: `coord-task:01knc6cswpwztpebeb7mmtmqtc`
  git execution status: `in_progress`
  source ref: `task/coordination-mutation-rewrite-plan`
  target ref: `origin/main`
  publish ref: `task/coordination-mutation-rewrite-plan`

