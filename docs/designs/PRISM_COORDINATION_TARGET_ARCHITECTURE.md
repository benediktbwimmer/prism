# PRISM Coordination Target Architecture

Status: proposed target architecture
Audience: PRISM coordination, runtime, MCP, bridge, SQLite, and shared-ref maintainers
Scope: the long-term authority, runtime, read, write, and scaling model for PRISM coordination

The definitive concurrent-write decision tree for CAS retry, replay, semantic merge, and rejection
lives in [PRISM_COORDINATION_CONFLICT_HANDLING.md](../PRISM_COORDINATION_CONFLICT_HANDLING.md).

The normative coordination seams and contracts now live under
[contracts/README.md](../contracts/README.md).

Warm completion gating and runtime-local validation posture now have companion designs in
[2026-04-09-warm-state-validation-feedback.md](./2026-04-09-warm-state-validation-feedback.md)
and
[2026-04-09-repo-init-and-validation-ejection.md](./2026-04-09-repo-init-and-validation-ejection.md).

This document should describe architecture shape, motivation, and rollout direction.
Exact seam semantics should defer to the contract docs rather than being redefined here.

---

## 1. Summary

PRISM coordination should be built around one hard rule:

- each coordination root has exactly one active authoritative coordination backend

Everything else exists to make that authority usable at speed and at scale:

- service-owned coordination materialization
- service-owned checkpoints and read acceleration
- the PRISM Service as the required coordination host
- runtime-local activity timelines and flight recorder data

The service is required for interactive coordination participation, but it must remain a host
around the authority plane rather than becoming authority itself.

Service-owned materialization, checkpoints, and runtime-local observability layers remain
optimizations. Removing those optimization layers must make the system slower or less observable,
not less correct.

This document freezes the target architecture implied by that rule.

---

## 2. Architectural Thesis

PRISM should treat coordination as a self-sufficient kernel with one required service host around
it.

The model is:

1. one configured authority backend holds current authoritative coordination state
2. that backend exposes retained authoritative history according to its retention contract
3. the coordination kernel is sufficient for coordination correctness
4. the PRISM Service is the required coordination host for reads, writes, materialization, and
   runtime participation without becoming authority
5. the local runtime provides liveness sensing, runtime-local operational state, and observability
6. cognition and graph-backed understanding are optional enrichers, never a requirement for
   coordination correctness

This lets PRISM remain:

- correct without the service becoming the authority plane
- portable across worktrees and machines
- restart-safe
- compatible with local-service and hosted-service deployments
- scalable through optimization layers that do not become hidden authority

---

## 3. Goals

Required goals:

- keep authoritative coordination truth behind one explicit authority backend
- make the coordination kernel self-sufficient for correctness
- make all local runtime state disposable
- allow high-quality operation without requiring cognition
- preserve tamper evidence and explicit identity attribution for every authoritative state change
- support fast service-local reads through derived materializations
- support scale through poll, read, and write coalescing in the required PRISM Service
- avoid introducing consistency traps through hidden cache shortcuts

Required non-goals:

- no mandatory Git authority backend for the first robust release
- no more than one active authoritative store for the same coordination root
- no unverified shared-coordination reads
- no separate explicit append-only coordination event log embedded in git payloads
- no dependence on cognition for lease, claim, plan, task, or artifact correctness

---

## 4. Core Rules

### 4.1 Authority rule

The configured coordination authority backend is the sole source of authoritative coordination truth
for that coordination root.

That includes:

- plans
- tasks
- claims
- leases
- artifacts
- reviews
- runtime descriptors that must be visible across runtimes
- compact shared activity summaries that are meant to be authoritative

SQLite, startup checkpoints, and local runtime memory are never authoritative.

The current implemented repo-native backend is Git shared refs, but the first production release
path should be DB-backed:

- SQLite for single-instance or local-service deployments
- Postgres for hosted or multi-instance deployments
- Git shared refs as a serious later or advanced backend

Only one backend may be authoritative for a coordination root at a time.

### 4.2 History rule

PRISM should not build a second explicit append log inside git.

Instead:

- the authority backend stores current authoritative state
- that backend exposes the effective retained event trail for its current retention window
- compaction or pruning may later reduce retained history depth

For the current Git backend, this keeps shared-ref state compact and avoids bloating git with both
current state and a duplicate embedded event journal.

### 4.3 Verification rule

PRISM should never serve newly fetched shared-coordination data unless that data has been verified.

Verification means:

- signed manifest validation
- manifest file digest validation
- identity attribution through the signing authority metadata

There should be no lenient path that treats unverified shared data as usable authority.

### 4.4 Disposable runtime-state rule

The worktree-local SQLite database is disposable.

Deleting it or restarting the daemon must not weaken coordination correctness.

What may be lost:

- local read acceleration
- startup speed
- lease activity detail
- local flight-recorder detail
- detailed runtime diagnostics

What must not be lost:

- authoritative coordination truth
- ability to rehydrate current coordination state
- ability to perform correct future coordination mutations

### 4.5 Optimization rule

Service-owned caches, checkpoints, and materialized read models must remain optimization layers.

The service itself is required for interactive coordination participation, but it must still fail
cleanly as a host around the authority backend rather than becoming authority itself.

---

## 5. System Layers

### 5.1 Coordination kernel

The coordination kernel owns correctness.

It includes:

- coordination state model
- policy evaluation
- mutation semantics
- lease/claim/task/artifact/review state transitions
- authoritative shared-ref encoding and decoding
- current-state materialization rules

The coordination kernel should not require:

- cognition
- graph indexing
- symbol resolution
- concept or contract enrichment
- semantic filesystem understanding
- local SQLite

### 5.2 Authority backend adapters

Authority backend adapters are the bridge from kernel semantics to the active authority substrate.

The initial shipping family should be DB-backed:

- SQLite-backed authority for single-instance deployments
- Postgres-backed authority for hosted or multi-instance deployments

The currently implemented Git adapter remains important as a repo-native backend and future
advanced option.

Backend adapters own:

- current-state IO
- transaction execution
- retained-history reconstruction
- descriptor publication and discovery
- verification and provenance checks appropriate to that backend

### 5.3 PRISM Service

PRISM should use one service shape:

- the PRISM Service

This service is operational, not authoritative.

It should own:

- authority access and refresh orchestration
- service-owned coordination materialization and checkpoints
- verified snapshot caching and fanout
- strong-read and eventual-read brokering
- write coalescing and mutation brokering
- event-engine execution
- local runtime notifications and cache delivery
- service-hosted UI serving and browser-session auth handling

These responsibilities share the same prerequisites:

- fresh verified authority state
- current service-owned coordination materialization
- mutation protocol machinery
- conflict handling and retry

Splitting them into separate long-lived services would duplicate logic and introduce more internal
state boundaries without improving correctness.

The service should remain lean and role-oriented, but it is not stateless: service-owned
coordination materialization is part of its normal operation. If it disappears, interactive
coordination participation should fail clearly rather than falling back to runtime-owned
coordination behavior.

### 5.4 Service descriptors and hosted runtimes

Federated transport should be published through one service descriptor, not one public endpoint per
runtime.

The top-level published object should be:

- a PRISM Service descriptor

That descriptor should advertise:

- service identity
- reachable endpoint or endpoints
- supported transport modes for follower and runtime connections
- trust metadata
- capabilities
- hosted runtime identities
- leadership role when applicable

Hosted runtimes should be modeled as logical children behind the service endpoint, not as
independent top-level public endpoints.

This allows:

- multiple runtimes on one machine behind one reachable service endpoint
- local-network and fixed-IP deployments to publish one stable address
- leader and follower service fanout without forcing every runtime to be directly reachable
- follower services to connect outward to one elected leader over a long-lived stream such as
  WebSocket without requiring inbound reachability on every machine
- follower services to keep their local git refs current so local historical queries keep working

### 5.5 Local runtime services

Local runtime services are operational and non-authoritative.

They include:

- bridge liveness sensing
- file-write sensing
- `prism run` command sensing
- lease renewal scheduling
- local diagnostics and observability

### 5.6 Cognition enrichers

Cognition enrichers are optional.

They include:

- graph indexing
- lineage and rebinding
- semantic impact
- concept and contract enrichment
- graph-backed query expansion

Coordination correctness must not depend on them.

---

## 6. Shared-Ref Model

### 6.1 Current-state orientation

Shared refs should store current authoritative coordination state, not a forever-growing embedded
event log.

This keeps blob reads bounded and works well with the fact that git reads whole blobs, not selected
JSON fields.

### 6.2 Ref families

The coordination namespace may contain multiple ref families such as:

- one live summary ref
- task shard refs
- claim shard refs
- runtime refs

Hot mutable state should be sharded across refs instead of forcing every mutation through one hot
summary head.

### 6.3 Compactness requirement

Because shared-ref reads load whole blobs, shared-ref payloads must stay compact and intentionally
bounded.

Large or high-frequency telemetry belongs in local runtime state or optional exported bundles, not
in the shared refs.

### 6.4 Identity and tamper evidence

Every authoritative shared-ref publish must be:

- attributable to an identity
- signed
- manifest-verified
- digest-verified

This makes state changes tamper evident and explicitly attributable.

### 6.5 Effective repo configuration in shared refs

PRISM should keep the effective repo configuration in shared refs.

That effective config should be:

- signed automatically by PRISM during publication
- attributable to an unlocked human credential
- the authoritative source of repo-scoped service and coordination policy

Branch-local config files and repo-authored PRISM code may still exist as optional templates or proposals, but they must not
become runtime authority automatically.

---

## 7. Read Model

### 7.1 Eventual and strong reads

PRISM should expose two consistency modes for coordination reads.

#### Eventual reads

Eventual reads return the latest verified local materialization.

They do not perform a fresh remote check first.

They are appropriate for:

- dashboards
- plan and task browsing
- operator views
- polling UI
- non-critical status inspection

#### Strong reads

Strong reads must:

1. determine the relevant shared-ref set
2. route through the PRISM Service when available
3. perform a fresh remote head check for that ref set
4. fetch changed refs if needed
5. verify manifests and digests
6. rematerialize the local verified state
7. answer from that verified materialization

Strong reads are appropriate for:

- pre-mutation arbitration
- correctness-sensitive lease or claim checks
- reads that must reflect current shared truth before the caller acts

### 7.2 No fake freshness shortcut

PRISM should not introduce a synthetic single freshness token that becomes a consistency trap.

If a shortcut can be bypassed by direct shared-ref mutation, then strong reads would need to ignore
it, which means the shortcut does not belong on the correctness path.

Instead:

- poll the real relevant ref heads
- keep strong reads honest
- optimize by reducing remote round trips, not by weakening consistency semantics

### 7.3 Strong-read coalescing

The PRISM Service should coalesce strong reads that target the same relevant ref set.

Because remote git roundtrips are already on the order of hundreds of milliseconds, the service may
use a short bounded batching window such as:

- `100-200ms`

This window should be configurable.

The semantics remain strong:

- the service still performs a fresh remote head check
- the service still fetches and verifies changed refs
- the service still answers from verified refreshed state

### 7.4 Verified state classes

PRISM should think about shared-coordination read state in three classes:

- `VerifiedCurrent`
- `VerifiedStale`
- `Unavailable`

`VerifiedStale` means:

- the runtime still has a previously verified local materialization
- a current refresh has not succeeded yet

It must not mean:

- newly fetched but unverified data

The same rule applies to effective repo configuration:

- current config must come from verified shared refs
- previously verified config may be retained as stale-but-trusted fallback
- unverified config must never be applied

---

## 8. Keeping Local Materializations in Sync

### 8.1 Polling is fundamental

If shared refs are authoritative and remote git may change independently, then polling is the base
sync-discovery mechanism.

There is no general push-based git-host primitive that notifies PRISM whenever a ref namespace
changes.

### 8.2 Poll heads, not blobs

The poller should check ref heads, not read shared-ref blobs on every cycle.

The cheap steady-state loop is:

1. ask the remote for relevant ref heads
2. compare with locally observed heads
3. if unchanged, stop
4. if changed, fetch and rematerialize

### 8.3 Poll the whole coordination namespace in one remote call

The runtime should prefer one namespace-wide remote head query such as:

- `refs/prism/coordination/<repo-id>/*`

and then partition the returned ref heads locally.

This is better than making multiple sequential remote calls for:

- `live`
- `tasks/*`
- `claims/*`
- future families

The main cost of polling is remote roundtrip and repeated command overhead, not per-ref blob
inspection.

### 8.4 Machine-local coalescing

On one machine, multiple runtimes for the same logical repo should not all poll independently.

PRISM should support one per-repo machine-local PRISM Service instance that:

- polls the coordination namespace once
- fetches and verifies once
- fans out change notifications or verified payloads to local runtimes

Each worktree runtime can still keep its own disposable SQLite materialization.

### 8.5 Optional org-level PRISM Service deployment

At larger scale, PRISM may use an org-level PRISM Service that:

- polls the coordination namespace
- fetches changed refs
- verifies manifests and digests
- fans out verified updates

The service is an optimization, not authority.

This optimization must remain useful across all network topologies:

- with zero publicly reachable services, PRISM still works through direct git polling and direct
  verified shared-ref operations
- with a few reachable services, private followers can attach to a reachable upstream
- as more services become publicly reachable, the network can become more efficient without changing
  correctness

If it disappears, runtimes must fall back to direct git polling.

---

## 9. Local SQLite Materialization

### 9.1 Purpose

SQLite exists to make coordination practical, not to hold authority.

It should be used for:

- fast current-state read models
- indexed query support
- startup acceleration
- local lease activity state
- flight-recorder detail
- diagnostics

### 9.2 Recovery

After SQLite loss, PRISM should recover by:

1. polling or fetching shared refs
2. verifying shared-ref contents
3. rebuilding current coordination state
4. rebuilding derived read models

This may lose:

- detailed local activity timelines
- exact local heartbeat buckets
- detailed command-run history

but should not lose correctness.

### 9.3 Coarse fallback after SQLite loss

For operational features such as lease activity, PRISM may reconstruct coarse current facts from:

- current git diff
- current worktree state
- current shared coordination state

This is a safe fallback for coarse changed-files and changed-lines context.

PRISM must not fabricate lost detailed command or timeline history.

---

## 10. Git Facts and Repo Facts

Git facts do not need to be stored authoritatively in SQLite.

Facts such as:

- current branch
- HEAD commit
- commit ancestry
- current diff
- worktree dirty state

should be read from git directly when needed.

SQLite may cache or index such facts for convenience, but the authoritative source is git itself.

---

## 11. Lease Heartbeats and Activity

### 11.1 Heartbeats do not require cognition

Lease continuity should depend on:

- bridge liveness
- file-write activity
- explicit `prism run` command activity
- runtime policy

It should not depend on:

- graph indexing
- cognition
- PRISM tool-call reminders from the model

### 11.2 Heartbeats do not fundamentally require SQLite

SQLite improves fidelity and observability, but the heartbeat model can still function without it.

Without SQLite:

- the runtime still has live bridge activity
- the runtime can still use fresh file activity
- the runtime can fall back to coarse git diff facts for changed files or lines

SQLite mainly improves:

- continuity across restart
- detailed activity history
- exact bucketization
- inspection and diagnostics

### 11.3 Shared summaries stay compact

Authoritative heartbeat-related shared-ref updates should remain compact.

Detailed flight-recorder timelines belong in local runtime state or optional federated runtime
query, not in shared refs.

---

## 12. Write Path and Scaling

### 12.1 Compare-and-swap stays at the authority boundary

Authoritative writes to shared refs must remain compare-and-swap publishes.

That preserves:

- explicit concurrency control
- deterministic retry behavior
- git-backed authority

### 12.2 Coalescing is still possible

CAS does not prevent write coalescing.

PRISM may coalesce multiple logical mutations before the final CAS publish, for example in a short
window such as:

- `100-200ms`

This window should be configurable.

The correct flow is:

1. gather pending mutation intents
2. read current authoritative heads
3. materialize next state
4. publish with CAS
5. retry by replaying intents if the head advanced

### 12.3 Coalesce intents, not blind state overwrites

Write brokers should batch mutation intents such as:

- acquire claim
- renew lease
- update task status
- attach review

They should not accept blind full-state replacement as the scaling primitive.

Intent replay is what makes CAS retry safe and composable.

### 12.4 Acknowledge only after publish

An optimization service must not acknowledge a mutation before the authoritative shared-ref write
has succeeded.

Otherwise the service becomes hidden authority.

### 12.5 Shard hot writes

High-frequency coordination writes should be distributed across ref families.

The summary/live ref should not be the hot write path for every operation.

### 12.6 PRISM Service write coalescing

At scale, the PRISM Service should accept mutation intents, batch them briefly, and
perform authoritative CAS publication.

It should:

- accept mutation intents
- coalesce writes within the configured batching window
- perform the authoritative CAS publish
- retry on conflict by replaying intents

Hot-path write volume should still remain bounded by policy where possible.

For example:

- lease renewals should be low-frequency by design
- write coalescing should operate on the resulting sparse mutation stream

That means the larger scaling problem is usually follower refresh and fanout, not raw write count.

If absent, runtimes must still be able to publish directly.

---

## 13. PRISM Service Model

### 13.1 One service shape for local and distributed scale

PRISM should use the same PRISM Service shape for:

- one machine with many local runtimes
- many machines across an organization

### 13.2 Repo-scoped leader selection

PRISM should support an elected leader per repository for optimization purposes.

That leader must be selected through the configured coordination authority backend, not through
hidden in-memory coordination. For the current Git-backed deployment shape, that means shared-ref
authority rather than an external side channel.

The recommended mechanism is:

- a repo-scoped PRISM Service leadership lease in shared refs
- explicit holder identity and published service descriptor
- a short TTL with renewable ownership
- visible expiry so another service can safely take over

Leader selection must remain an optimization, not a correctness dependency.

If no leader exists, or if followers cannot reach it, PRISM must still work through direct git
polling and direct verified shared-ref reads and writes.

PRISM should also allow multiple leader candidates per repository.

But the default active model should still be:

- one preferred write/event leader
- zero or more standby candidates
- ordered failover when the preferred leader is unavailable

Multiple simultaneously active leaders should be reserved for a future explicitly partitioned mode,
not the default topology.

### 13.3 Leader and follower transport

The elected leader should expose a reachable endpoint through the existing bring-your-own-transport
model.

Follower services should normally connect outward to the leader rather than requiring the leader to
discover arbitrary follower endpoints.

The preferred connection shape is:

- long-lived follower-initiated streams such as WebSocket
- ref-update and leadership-change notifications from leader to followers over that stream
- local runtime fanout handled by each follower service on its own machine

This keeps the global fanout problem manageable:

- leader service fans out to connected follower services
- follower services fan out to their local runtimes
- local runtimes do not need to be globally reachable

In practice, PRISM should distinguish:

- publicly reachable PRISM Services, which may act as leaders or relays
- private/local PRISM Services, which act as edge followers

Edge followers connect outward but do not need to expose themselves as public relays.

### 13.4 Follower-side git fetch, verification, and parsing

Followers should not blindly trust parsed JSON materialized by the leader.

The default leader-to-follower update path should be:

1. the leader notices authoritative shared-ref head changes
2. the leader sends a fanout notification containing the changed refs and new heads
3. the follower fetches the updated coordination namespace from the leader's git endpoint
4. the follower updates its own local `.git` state
5. the follower verifies manifests and signatures locally
6. the follower reparses and rematerializes its local SQLite read model locally

This preserves two important properties:

- follower correctness does not depend on trusting the leader's parse
- local git history remains current enough for historical and audit-style local queries

The leader may still compute and cache verified snapshots for its own use, but follower trust should
be rooted in local git fetch plus local verification.

### 13.5 Leader as git source for the coordination namespace

The preferred scaling model is for followers to fetch shared coordination refs from the elected
leader instead of fetching them directly from the upstream git host.

That means the leader should maintain a fetchable bare or mirrored copy of the shared coordination
namespace, even when it has no checked-out worktree and no local runtimes attached.

This gives PRISM the right shape:

- the upstream git host remains the ultimate authority boundary
- the leader acts as an internal fetch source and fanout accelerator
- follower local repos stay current without creating a thundering herd against the upstream host

This is the preferred default over having followers fetch directly from the upstream git host.

### 13.6 Optional packfile or bundle optimization

As an optimization, the leader may also push a compact git delta to followers over the long-lived
stream.

Possible forms include:

- a small packfile
- a git bundle representing the delta between old and new coordination heads

Followers may ingest that delta into local git and then run the same local verification and parsing
flow.

This is an optimization, not the primary architecture.

The default design should remain:

- WebSocket (or equivalent) for notifications
- git fetch from the leader for local repo freshness
- local verification and local materialization on the follower

### 13.7 Leader preference and follower routing

Followers should not choose an upstream purely by whatever service self-assigns the highest
preference.

Instead, follower routing should combine:

- self-published service descriptors
- effective repo policy from shared refs
- local reachability checks

The intended flow is:

1. follower loads verified effective repo config
2. follower loads visible service descriptors
3. follower orders candidates according to repo policy
4. follower connects to the highest-priority reachable candidate
5. follower falls back to the next candidate if the preferred one is unavailable
6. follower ultimately falls back to direct git/shared-ref mode if no candidate is reachable

Followers should switch upstreams when verified policy changes or when their current upstream
becomes unavailable.

### 13.8 Mostly stateless service

The PRISM Service should be stateless or near-stateless.

It may keep:

- in-memory caches
- ephemeral verified snapshots
- in-flight strong-read batches
- in-flight write batches
- short-lived fanout state
- a rebuildable bare or mirrored coordination-ref cache for follower fetches

It should not hold correctness-critical durable state.

If it restarts, it should simply:

1. poll git again
2. fetch and verify shared refs
3. resume fanout

### 13.9 Fallback

If the PRISM Service is unavailable, runtimes must fall back to direct polling and
direct verified shared-ref reads and writes.

---

## 14. Event Engine

PRISM should add an event engine above the coordination graph.

That event engine should:

- support recurring execution through continue-as-new at the plan boundary
- support a repository-local TypeScript hook SDK
- run against verified coordination state
- acquire execution ownership through shared-ref CAS

The preferred execution plane is the PRISM Service.

However, correctness must not depend on a singleton service assumption. Multiple PRISM Services may
exist:

- one per developer machine
- one per CI machine
- multiple org-level replicas

Therefore event execution ownership must still be protected by authoritative shared-ref CAS.

Event execution records should:

- live in a separate authoritative event execution ref family
- use lifecycle states such as `claimed`, `running`, `succeeded`, `failed`, `expired`, or
  `abandoned`
- support retry and crash visibility

PRISM can provide:

- exactly-once execution ownership

PRISM cannot universally guarantee:

- exactly-once external side effects

unless the external sink itself supports idempotency or durable acknowledgement semantics.

The detailed event-engine design is captured in:

- `docs/PRISM_EVENT_ENGINE_ARCHITECTURE.md`

---

## 15. Verification and Trust Model

### 15.1 When verification happens

Verification should happen when PRISM loads shared-ref contents, not when it merely polls heads.

Head polling answers:

- did something change?

Verification answers:

- is the changed content authentic?
- is it signed by a trusted identity?
- do the file digests match the manifest?

### 15.2 Strong-read trust rule

Strong reads must use verified shared-ref state only.

If verification fails:

- the read should fail closed
- or fall back to a previously verified stale materialization where appropriate

PRISM must not silently import unverified authority.

### 15.3 Manual mutation stance

Direct manual mutation of shared refs outside PRISM should be treated as unsupported unless it
still preserves the verification and publication contract.

If such mutation breaks verification, PRISM should surface diagnostics and fail closed.

This is preferable to introducing a synthetic freshness shortcut that hides drift.

### 15.4 Repo-scoped authority policy

The effective repo configuration should be the source of truth for who is allowed to do what in a
repository.

That includes policy such as:

- which principals are repo admins
- which principals may publish or update effective repo config
- which principals may publish service policy
- which principals may become leader candidates
- which principals may publish public service descriptors

These permissions should be expressed through the same PRISM identity and trust model already used
everywhere else.

Ordinary services may self-publish their own descriptors, but they must not be able to unilaterally
change repo-wide routing policy or leader preference unless the effective repo config grants that
authority.

### 15.5 Config publication flow

Effective repo config should be published through explicit PRISM commands, not by implicitly reading
branch-local files as authority.

The intended flow is:

- `prism config init`
  - generate a local editable draft config
- `prism config publish`
  - validate the draft
  - sign it with the current unlocked human credential
  - publish it into shared refs as the new effective repo config
- `prism config show`
  - display the current effective repo config from shared refs
- `prism config pull`
  - materialize the current effective repo config into a local draft file for inspection or editing

Repo files may still contain human-friendly templates, examples, or repo-authored PRISM code under
`.prism/code/**`, but the authoritative runtime config should live in shared refs only.

---

## 16. Consequences for API Design

### 15.1 Coordination queries

Coordination query APIs should expose freshness metadata clearly.

They should be able to say:

- this answer is eventual or strong
- this answer is based on verified current or verified stale materialization
- the current authoritative refresh failed verification or is unavailable

### 15.2 Local runtime queries

Local runtime query surfaces may expose:

- lease activity timelines
- command-run detail
- renewal diagnostics
- local file activity detail

Those queries are useful, but they are not authority queries.

### 15.3 Shared history queries

History queries should reconstruct retained authoritative history from local git state, not from an
explicit second event log in SQLite.

---

## 17. Anti-Patterns to Avoid

PRISM should avoid:

- treating SQLite as hidden authority
- splitting sync, strong-read coalescing, write coalescing, and event execution into separate
  correctness-significant services
- publishing one public endpoint per runtime instead of one service descriptor with hosted runtimes
- serving newly fetched but unverified shared-ref contents
- adding a synthetic freshness token that strong reads cannot trust
- forcing all hot writes through one live summary ref
- embedding a second explicit append-only event log inside shared-ref payloads
- making cognition a prerequisite for coordination correctness
- requiring a central database for correctness

---

## 18. Recommended Implementation Direction

From this target architecture, the most important implementation direction is:

1. keep removing correctness dependencies from local runtime state
2. remove lenient shared-coordination import behavior
3. collapse polling into one namespace-wide remote head check
4. introduce the PRISM Service as the single required coordination host
5. coalesce polling and strong reads per logical repo
6. shard hot write paths cleanly
7. add configurable write coalescing over CAS publishes
8. implement the event engine on top of the PRISM Service and shared-ref CAS execution
   records

These changes improve scale without changing authority.

---

## 19. Final Rule

The final architecture should be understood in one sentence:

- one configured authority backend is truth; the PRISM Service is the required host around that
  truth; service-owned materialization and local runtime state remain disposable; DB-backed
  authority is the current release path and Git shared refs remain an implemented backend

That is the target PRISM should implement against.
