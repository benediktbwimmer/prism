# PRISM Shared Coordination V1 Architecture

Status: target architecture for pre-v1 implementation
Audience: PRISM core, coordination, storage, git execution, runtime, and MCP maintainers
Scope: schema compatibility, 1000-agent shared coordination topology, state pruning, and semantic merging

---

## 1. Summary

The current shared coordination ref implementation is a strong baseline, but it is not yet the
final v1 architecture for a repo that must tolerate hundreds or thousands of concurrent agents.

Four design decisions must be locked in before v1:

- every cross-runtime authoritative shared-coordination payload must carry an explicit
  `schema_version` compatibility contract
- the single repo-wide live shared coordination ref must stop being the hot write path for leases,
  heartbeats, task state, and claims
- the hot coordination workspace must be explicitly pruned and archived instead of growing without
  bound
- PRISM must own schema-aware conflict resolution for `refs/prism/**` instead of relying on generic
  Git text merges

For v1, PRISM should treat the repo-wide live summary as a derived read model and move hot
authoritative writes onto partitioned refs where only semantically conflicting work contends, keep
the live coordination state bounded, and merge conflicting coordination writes semantically inside
the daemon.

If this document is not implemented, PRISM may still work well for small multi-agent deployments,
but it should not be considered architecturally ready for a 1000-agent reality.

---

## 2. Why This Exists

Four gaps are still painful to retrofit after a public release:

1. Shared coordination schema compatibility

Once multiple PRISM installations read and write the same shared Git coordination state, the JSON
format becomes a compatibility contract. Readers must be able to distinguish:

- known older payloads
- known current payloads
- newer payloads that require an upgrade

2. Shared ref contention under scale

The current single live shared coordination ref still serializes unrelated writes through one ref
head. That is acceptable for a small number of agents with low-frequency authoritative updates. It
is not the right long-term write topology for hundreds or thousands of concurrent runtimes.

3. State pruning and archival

Git does not automatically prune old logical rows. If completed plans, failed tasks, closed claims,
and stale runtime metadata remain in the hot live coordination workspace forever, fetch, push,
parsing, and in-memory materialization all grow with dead history instead of active coordination
state.

4. Deep coordination conflict resolution

Compare-and-swap retries are necessary but not sufficient. When two runtimes update the same
authoritative object concurrently, PRISM needs a deterministic semantic merge policy for
coordination payloads. The daemon cannot rely on interactive Git conflict resolution for machine
actors.

This document defines the target architecture that future implementation work should follow.

---

## 3. Goals

Required goals:

- make every authoritative shared-coordination payload self-describing and versioned
- allow mixed-version PRISM installations to fail safely instead of silently misreading newer data
- eliminate repo-wide hot-path contention for unrelated heartbeats, leases, task mutations, and
  claim mutations
- keep Git as the only durable shared substrate
- preserve auditability, offline fetchability, and repo-native recovery semantics
- keep startup and MCP reads efficient by serving a compact derived summary instead of forcing
  fan-out reads on every normal request
- define a topology that remains valid for 1000-agent scale even if the initial rollout is phased
- keep live coordination state bounded so active reads and writes scale with current work, not
  repository age
- make daemon-driven semantic merge policy explicit so conflicts do not deadlock agents

Non-goals:

- introducing an external coordination database before v1
- making every shared mutation globally transactional across unrelated coordination domains
- solving cross-repo coordination in this design pass
- replacing local rich runtime state or local journals with Git refs

---

## 4. Compatibility Contract

### 4.1 Authoritative payloads must be explicitly versioned

Every authoritative shared-coordination payload must include:

- `schema_version`
- `kind`

Recommended shape:

```json
{
  "schema_version": 1,
  "kind": "runtime_descriptor",
  "...": "payload fields"
}
```

This applies to:

- the repo-wide summary manifest
- runtime descriptor payloads
- authoritative task-shard payloads
- authoritative claim-shard payloads
- any future artifact, review, or event shard payload that becomes authoritative across runtimes

The current summary manifest already has a `version` field in code. Before v1, PRISM should
standardize on `schema_version` for the public compatibility contract. Loaders may temporarily
accept legacy `version` during the migration to the final v1 envelope, but v1-published payloads
should use `schema_version`.

### 4.2 Reader and writer rules

Readers must:

- accept `schema_version < current` when the loader still knows how to interpret that version
- accept `schema_version == current`
- reject `schema_version > current` with an explicit upgrade-required diagnostic
- ignore unknown additive fields inside a supported schema version

Writers must:

- emit only the current `schema_version`
- avoid destructive field reuse across schema versions
- bump `schema_version` when a change is not safely readable by the previous version

The minimum error contract for an unsupported newer version should communicate:

- observed `schema_version`
- highest supported `schema_version`
- payload `kind`
- an explicit “upgrade PRISM” instruction

### 4.3 Runtime descriptors need the same contract

Runtime descriptors are consumed across runtimes and across releases. They therefore need the same
envelope and compatibility rules as plan or task state.

This is especially important for federated runtime queries:

- Machine A may discover a runtime descriptor written by Machine B
- Machine A must know whether it can interpret that descriptor safely
- peer query routing must reject unsupported newer descriptors explicitly instead of treating them
  as malformed generic JSON

### 4.4 Compatibility discipline

Before v1, PRISM should adopt these rules:

- additive fields are preferred over renames or semantic reuse
- removing or renaming a field requires a schema version bump unless the old field remains readable
- enum meaning changes require a schema version bump
- shard layout changes that affect path derivation, sharding, or interpretation require a schema
  version bump

---

## 5. Authority and Ref Topology for 1000-Agent Scale

### 5.1 Core rule

Only semantically conflicting work should contend.

Unrelated heartbeats, unrelated task updates, and unrelated claim updates must not serialize
through one repo-global ref head.

### 5.2 Target authoritative planes

PRISM should use distinct authoritative ref families:

- `refs/prism/coordination/<repo>/summary/live`
  - derived compact read model
  - optimized for startup, MCP reads, diagnostics, and operator inspection
  - not the hot mutation target for every authoritative write
- `refs/prism/coordination/<repo>/runtimes/<runtime-id>`
  - authoritative per-runtime liveness and discovery state
  - heartbeats update only this ref
- `refs/prism/coordination/<repo>/tasks/<shard>`
  - authoritative task state for one fixed hash shard
  - task claims, status transitions, handoffs, and publish lifecycle facts write only here
- `refs/prism/coordination/<repo>/claims/<shard>`
  - authoritative work-claim state for one fixed hash shard
  - anchor contention stays local to the shard that owns the claim

Reserved for later scale-up if write rates require it:

- `refs/prism/coordination/<repo>/artifacts/<shard>`
- `refs/prism/coordination/<repo>/reviews/<shard>`
- `refs/prism/coordination/<repo>/events/<shard>`

The summary ref remains important, but it becomes a derived aggregate of authoritative shard and
runtime refs instead of the only live mutable head.

### 5.3 Summary ref role after the topology change

The repo-wide summary ref should exist for:

- cold startup
- compact shared truth for common reads
- operator inspection
- replication and audit
- recovery from local cache loss

It should not be required for:

- every heartbeat
- every lease renewal
- every task status mutation
- every claim update

The summary may lag shard truth slightly as long as:

- the lag is bounded and observable
- authoritative fallback reads can resolve ambiguity when needed
- MCP/operator surfaces expose summary freshness explicitly

---

## 6. Lease and Heartbeat Model

### 6.1 Heartbeats move to per-runtime refs

The highest-write coordination fact is runtime liveness. That write stream must not hit the global
summary head.

For v1 scale readiness:

- heartbeats update only `refs/prism/coordination/<repo>/runtimes/<runtime-id>`
- the summary runtime index is rebuilt from runtime refs on a slower or event-driven cadence
- task or claim state does not get rewritten just because a runtime emitted a fresh heartbeat

### 6.2 Lease validity becomes a join, not a global rewrite

Authoritative task and claim records should carry the durable lease boundary facts:

- holder principal
- holder runtime id
- holder runtime instance id
- lease epoch
- acquisition, handoff, reclaim, and expiry-boundary timestamps

Per-runtime refs should carry the high-frequency liveness facts:

- current runtime instance id
- `last_seen_at`
- peer discovery metadata
- capabilities

Lease validity is then determined by joining:

- the authoritative task or claim record
- the current runtime descriptor for the referenced runtime id and instance id
- the lease policy window

This prevents “heartbeat refresh” from rewriting every task or claim touched by that runtime.

### 6.3 Consequence of runtime restart

Every lease-bearing record must bind to a specific runtime instance, not only to a stable runtime
id.

If a runtime restarts and publishes a new instance identity:

- old task or claim assignments bound to the old instance do not become silently valid again
- reclaim logic can reason from a precise stale holder identity

---

## 7. Sharding Strategy

### 7.1 Fixed shard count

Task and claim refs should use a fixed, deterministic shard map in v1.

Recommended v1 strategy:

- 256 shards
- shard key = first two hex characters of `blake3(stable-id)`

This gives:

- a bounded namespace
- stable routing
- human-inspectable shard identifiers
- enough distribution to avoid one repo-global write head

If PRISM ever needs a different shard count, that layout change should be treated as a schema
versioned compatibility event.

### 7.2 Why fixed shards instead of per-object refs

Per-object refs would avoid some contention, but they create an unbounded ref explosion and make
inspection, fetch behavior, and maintenance more expensive.

Fixed shard refs provide:

- bounded ref count
- localized contention
- predictable maintenance and compaction
- room for batch writes inside a shard when one workflow touches nearby objects

---

## 8. Read Path and Materialization

### 8.1 Default read path

Normal startup and MCP read flows should prefer:

1. the summary live ref
2. derived summary indexes
3. targeted authoritative shard or runtime reads only when the summary is stale, missing, or
   ambiguous

This preserves fast common-case reads while removing the summary ref from the hot write path.

### 8.2 Materializer role

PRISM should have an explicit materialization pipeline that:

- ingests authoritative runtime and shard refs
- republishes a compact summary snapshot
- emits summary freshness metadata
- updates operator-facing indexes

Materialization may be event-driven, periodic, or opportunistic, but it must be observable and
bounded. A slow or failed materializer should degrade read freshness, not silently corrupt
authority.

### 8.3 Freshness visibility

The summary should expose enough metadata to answer:

- when was the summary last rebuilt?
- from which shard or runtime heads was it built?
- how stale is it relative to authoritative refs?
- is the summary degraded or incomplete?

---

## 9. State Pruning and Archive Policy

### 9.1 Hot state must stay bounded

The live coordination workspace is hot shared memory, not an infinite append-only database.

The summary ref, task shards, claim shards, and runtime refs should retain only the active working
set plus a bounded recent history window needed for correctness, replay, and operator diagnosis.

### 9.2 What belongs in hot coordination state

Hot state should include:

- active and recently completed plans still needed for dispatch or handoff reasoning
- active and recently completed tasks still needed for dependency resolution, retry policy, or
  verification follow-through
- live claims and recently released claims still needed for reclaim diagnostics
- live runtime descriptors and short-lived recent runtime continuity metadata
- bounded recent coordination history needed for repair or conflict replay

### 9.3 What should be archived out of the hot plane

PRISM should prune or archive:

- completed plans beyond the hot retention window
- terminal tasks beyond the hot retention window
- released or terminal claims beyond the hot retention window
- stale runtime descriptors from dead runtime instances beyond the runtime retention window
- superseded fine-grained history once a compact continuity boundary or archive snapshot exists

Archived state remains durable and auditable, but it should move to a colder coordination storage
plane.

### 9.4 Archive targets

PRISM should support at least these archive targets:

- `refs/prism/coordination/<repo>/archive/<partition>`
  - Git-native cold archive for coordination snapshots and shard rollups
- optional blob-backed archive bundles when the operator enables exported cold storage

The hot plane should be pruned only after PRISM records an explicit continuity boundary that points
to the archived state.

### 9.5 Archive triggers

Archive and pruning policy should be explicit and configurable. At minimum, PRISM should support:

- archive on terminal state plus retention window
- archive on explicit operator compact/archive command
- archive when shard size or summary size crosses configured budgets
- archive when compaction builds a new continuity baseline

### 9.6 Archive metadata and continuity

Every prune/archive action should record:

- archive boundary timestamp
- archive target handle or ref
- source shard or summary head
- retained continuity metadata needed for trust verification
- enough summary metadata for operators to know what was pruned

### 9.7 Read behavior across hot and archive state

Default MCP and startup flows should stay hot-path only.

Archive reads should be explicit or demand-driven:

- normal dispatch and task execution use only hot state
- operator and audit flows may read archive state
- targeted lineage or historical queries may request archive enrichment

This prevents old coordination history from taxing the active control plane.

---

## 10. Schema-Aware Conflict Resolution

### 10.1 Generic Git text merges are not sufficient

PRISM controls the coordination schema and the daemon. It therefore must own conflict resolution for
`refs/prism/**`.

The daemon must not rely on an interactive merge tool or raw Git conflict markers for coordination
payloads.

### 10.2 Merge policy lives at the schema layer

Conflicts should be resolved by payload kind and field semantics, not by line-based text merge.

The merge system should:

- decode the base, local, and remote payloads
- perform a schema-aware three-way merge
- produce either a deterministic merged payload or an explicit semantic-conflict rejection
- preserve provenance about the merge decision when needed for operator diagnostics

### 10.3 Default merge policy by field class

The default policy should be:

- set-like arrays such as dependency ids or claim history entries: union merge with stable ordering
- append-only event/history arrays: stable append merge with de-duplication by stable id
- maps keyed by stable id: key-wise recursive merge
- booleans and scalar metadata that are pure observations: last-writer-wins unless the schema says
  otherwise
- enums or terminal lifecycle states: schema-specific resolution rule, never implicit text merge

Examples:

- dependency arrays should union, not overwrite
- claim history should append or union by event id
- task `status` should follow an explicit lifecycle precedence rule instead of generic text merge

### 10.4 Explicit semantic-conflict cases

Some conflicts should not auto-merge. PRISM should reject and surface a deterministic semantic
conflict when:

- two writers assert incompatible holders for the same lease epoch
- two writers assign incompatible terminal outcomes that the schema does not define a precedence for
- a mutation depends on a stale base assumption that cannot be replayed safely
- a merge would violate lifecycle invariants for tasks, claims, reviews, or artifacts

### 10.5 Recommended status and severity policy

For coordination lifecycles, the schema should define precedence explicitly.

Recommended direction:

- lease-holder identity conflicts require semantic rejection unless one side is explicitly stale
- terminality beats non-terminality only when the lifecycle rules say that transition is valid
- higher-severity failure states may dominate lower-severity transient states when the schema
  defines that ordering

The important point is not the exact first policy, but that the policy is explicit, deterministic,
and encoded in the daemon rather than left to Git text conflict behavior.

### 10.6 Where this logic lives

Semantic merge logic should live in dedicated coordination merge modules, not in generic `lib.rs`,
`main.rs`, or shell scripts.

Suggested ownership:

- per-kind merge policies in coordination/storage modules close to the authoritative schema
- daemon orchestration in the shared-ref publish and retry path
- operator diagnostics exposed through runtime/MCP views

---

## 11. Diagnostics and Operational Signals

For this architecture to be safe under scale, PRISM should expose:

- CAS retry rate by ref family and shard
- summary materialization lag
- highest-contention task and claim shards
- runtime heartbeat publish rate
- suppressed or coalesced heartbeat counts
- authoritative-fallback read counts
- incompatible schema-version rejection counts

Operator surfaces should make it obvious whether the bottleneck is:

- one hot task
- one hot claim anchor
- one overloaded shard
- summary lag
- mixed-version deployment

Add these pruning and merge signals too:

- hot-state object counts by plane and shard
- archive/prune throughput and lag
- archive boundary publication status
- semantic merge success rate
- semantic merge rejection counts by payload kind
- fallback-to-retry counts after merge failure

## 12. Migration Plan

### Phase A: compatibility floor

- add `schema_version` envelopes and explicit compatibility gates
- version runtime descriptors
- surface upgrade-required errors cleanly

### Phase B: runtime liveness split

- introduce authoritative per-runtime refs
- move heartbeat writes off the summary live ref
- derive the summary runtime index from runtime refs

### Phase C: shard task and claim authority

- introduce fixed task and claim shard refs
- move authoritative task and claim writes off the summary live ref
- rebuild the summary from runtime and shard refs

### Phase D: pruning and archive plane

- introduce explicit hot-state retention policy
- archive terminal coordination state to archive refs or archive bundles
- publish continuity metadata for prune/archive boundaries
- keep normal startup and MCP flows hot-path only

### Phase E: semantic merge and hardening

- implement schema-aware merge for coordination payloads
- reject unresolved semantic conflicts deterministically
- add scale diagnostics and load validation
- shard artifact or review state if production write rates justify it
- finalize compaction and repair tooling for the new topology

### V1 ship gate

PRISM should not call the shared coordination design v1-ready for 1000-agent scale until:

- Phase A is complete
- Phase B is complete
- Phase C is complete
- Phase D is complete
- Phase E is complete
- load validation demonstrates that heartbeat traffic and unrelated task updates no longer contend
  through one repo-global head

---

## 13. Relationship to Existing Docs

This document supersedes the old assumption that the single repo-wide shared coordination ref is
both:

- the only authoritative cross-branch coordination plane
- the primary hot write target for all shared coordination facts

`docs/PRISM_SHARED_COORDINATION_REFS.md` remains the implemented baseline and closure record for
the current system.

This document defines the pre-v1 compatibility and scale work that should now be implemented on top
of that baseline.
