# PRISM Event Engine Architecture

Status: proposed V2 architecture
Audience: PRISM core, coordination, runtime, bridge, MCP, and orchestration maintainers
Scope: recurring execution, hook evaluation, decentralized event deduplication, and event
execution on top of shared coordination authority

---

## 1. Summary

PRISM needs a temporal and event-driven execution layer above the coordination graph.

That layer should provide:

- recurring execution without unbounded graph growth
- hook-based orchestration with a real developer SDK
- decentralized exactly-once execution ownership without requiring an external lock service

The event engine should not become a second authority.

Instead:

- shared refs remain the source of truth
- the PRISM Service is the preferred execution plane
- authoritative event execution ownership is still acquired through shared-ref compare-and-swap

---

## 2. Design Goals

Required goals:

- support push-based orchestration on top of the coordination graph
- keep graph growth bounded for recurring work
- make hook logic ergonomic and type-safe
- preserve decentralized correctness even if multiple PRISM Services are running
- avoid polluting hot coordination state with transient execution bookkeeping
- ensure event execution decisions are based on verified coordination state
- make event execution ownership durable and attributable

Required non-goals:

- no dependence on a central database for event correctness
- no assumption that one PRISM Service instance is the global singleton
- no exactly-once claim for arbitrary external side effects unless the sink itself is idempotent
- no unbounded execution telemetry embedded into shared refs

---

## 3. Continue As New

### 3.1 Problem

Recurring or scheduled work cannot keep appending new tasks into one permanent live plan without
causing boundedness and retention problems.

### 3.2 Solution

Recurrence should be modeled at the plan boundary.

The target model is:

- a plan template defines recurring structure
- each execution cycle is a separate plan instance
- when the instance completes, the event engine performs a coordination transaction that:
  - archives the completed plan instance
  - creates the next plan instance from the template

### 3.3 Consequences

This preserves:

- append-only mathematical purity of individual plan instances
- bounded hot read models
- archival continuity across cycles
- exact auditability of prior executions

The event engine should think in terms of:

- plan template
- plan instance
- recurrence policy
- archive lineage

not “reopen old tasks in place.”

---

## 4. Hook SDK

### 4.1 Repository-facing entrypoint

PRISM should support a repository-local TypeScript hook surface such as:

- `.prism/hooks.ts`

This gives developers:

- versioned orchestration logic in repo
- type safety
- autocomplete
- reuse of repo-local logic

### 4.2 Execution model

Hooks should run against the verified coordination view exposed by the PRISM Service.

The contract should not be:

- “run against whatever happens to be in SQLite”

It should be:

- “run against the current verified coordination materialization that the service is serving”

SQLite may back that view, but it is not the contract surface.

### 4.3 Hook categories

The SDK should support at least:

- state-transition hooks
- periodic or cron hooks
- explicit orchestration actions

Examples:

- task became actionable
- claim expired
- recurring plan tick fired
- runtime became stale

---

## 5. Event Identity

### 5.1 Trigger identity must be transition-based

Event identity must be derived from authoritative transitions, not from level predicates alone.

Bad:

- “task 123 is actionable”

Good:

- “task 123 entered actionable at authoritative coordination revision X”

### 5.2 Event identity inputs

The deterministic event id should be based on:

- trigger kind
- target identity
- authoritative revision or transition marker
- hook identity
- hook version or content digest when appropriate

This prevents ambiguous re-execution when the same predicate remains true across many reads.

---

## 6. Event Execution Ownership

### 6.1 Multiple services are expected

PRISM must assume that multiple PRISM Services may be running simultaneously:

- one per developer machine
- one per CI machine
- multiple replicas of an org-wide service

Therefore, PRISM cannot rely on service singleton assumptions for correctness.

One of those services may hold the repo-scoped leader lease, but event correctness must continue to
hold even while leadership changes or when no reachable leader exists.

### 6.2 Preferred execution plane vs correctness plane

The PRISM Service is the preferred execution plane because it is efficient.

But shared-ref CAS remains the correctness plane.

That means:

- the service evaluates hooks and decides candidates
- the right to execute is acquired through authoritative shared-ref CAS

### 6.3 Event execution records, not one-bit tombstones

PRISM should not model event deduplication as a permanent one-bit “processed tombstone” only.

Instead it should use authoritative event execution records with lifecycle states such as:

- `claimed`
- `running`
- `succeeded`
- `failed`
- `expired`
- `abandoned`

This supports:

- winner selection
- crash visibility
- retry rules
- operator inspection
- bounded cleanup and compaction

### 6.4 Separate event execution ref family

Event execution records should not live inside the hot coordination summary/live tree.

They are authoritative shared facts, but they are not coordination graph state.

They should live in a separate shared-ref family such as:

- `refs/prism/events/<repo-id>/<bucket>/<event-id>`

or an equivalent dedicated authoritative event namespace.

This avoids:

- polluting the hot coordination state
- increasing write contention on coordination summary refs
- coupling retention of coordination state to transient execution locks

### 6.5 Ownership algorithm

The ownership flow should be:

1. verified coordination state indicates that an event trigger is eligible
2. the service computes the deterministic event id
3. the service attempts to publish a `claimed` execution record via CAS
4. exactly one publisher wins
5. the winner advances the record to `running`
6. the winner performs the side effect
7. the winner publishes `succeeded` or `failed`

Losers:

- refresh from shared refs
- observe that the event execution record already exists
- abort local execution cleanly

### 6.6 Exactly-once wording

PRISM can provide:

- exactly-once execution ownership

PRISM cannot universally guarantee:

- exactly-once external side effects

unless the downstream sink also supports idempotency or durable acknowledgement semantics.

This distinction should be explicit in the design and user-facing claims.

---

## 7. Event Engine and the PRISM Service

### 7.1 One service, not separate sync and broker services

PRISM should use one stateless service shape:

- the PRISM Service

This service should own:

- remote polling
- fetch and verification
- verified snapshot fanout
- strong-read coalescing
- write coalescing and CAS publication
- event engine execution

When a leader is present, it should be the preferred execution plane for these responsibilities.

### 7.2 Leader selection and follower transport

PRISM should use a repo-scoped leader lease in shared refs to elect the preferred PRISM Service
leader.

That leader should publish its service descriptor and reachable endpoint through the federated
bring-your-own-transport model.

Follower services should normally connect outward to the elected leader over a long-lived stream,
preferably WebSocket or an equivalent bidirectional transport.

That stream should carry:

- verified snapshot update fanout
- event-trigger notifications
- write outcome notifications
- leadership change notifications

This keeps the event engine decentralized but efficient:

- one leader is the preferred execution plane
- followers can stay hot and informed
- shared-ref CAS execution records remain the correctness fence

### 7.3 Why one service is better

These capabilities all depend on the same shared prerequisites:

- fresh verified shared-ref state
- a current materialized view
- CAS publication
- conflict retry
- runtime fanout

Splitting them into separate long-lived services would duplicate logic and introduce more internal
state boundaries without improving correctness.

### 7.4 Statelessness

The PRISM Service should remain stateless or near-stateless.

It may keep:

- in-memory verified snapshots
- in-flight read coalescing state
- in-flight write batches
- short-lived event execution scheduling state

It must not become hidden authority.

If it dies:

- shared refs remain authoritative
- clients may fall back to direct shared-ref behavior
- a restarted service can reconstruct state from shared refs

---

## 8. Polling and Trigger Evaluation

### 8.1 Polling remains the discovery mechanism

The service discovers remote changes by polling shared-ref heads.

It should prefer one namespace-wide remote head query rather than many separate family calls.

### 8.2 Hook evaluation inputs

Hooks should run only after:

- a verified refresh of relevant shared-ref state
- or against the latest locally retained verified materialization

PRISM must not evaluate hooks against newly fetched but unverified data.

### 8.3 Poll-triggered and time-triggered hooks

The service should support:

- state-triggered execution after verified coordination refresh
- time-triggered execution on a scheduler

These should share the same execution ownership and CAS lock path.

---

## 9. Read and Write Coalescing

### 9.1 Strong-read coalescing

Strong reads should be coalesced by the PRISM Service.

Because remote git checks already cost on the order of hundreds of milliseconds, the service may
use a short bounded batching window such as:

- `100-200ms`

to group strong reads targeting the same relevant ref set.

This batching window must be configurable.

### 9.2 Write coalescing

Writes should also be coalesced by the PRISM Service with a short configurable batching
window such as:

- `100-200ms`

The service should batch mutation intents, not blind state overwrites, then perform one CAS publish
per affected ref or ref family.

### 9.3 Shared implementation shape

Strong-read coalescing and write coalescing belong in the same service because both benefit from:

- shared verified current state
- shared remote head checks
- shared fetches
- shared conflict handling

---

## 10. Event Record Retention and Compaction

Event execution records are authoritative and should be retained long enough to support:

- deduplication
- failure visibility
- audit

But they should still be compactable.

PRISM should support compaction policies that:

- keep current and recent execution records hot
- retain enough history for audit and replay analysis
- prune older execution records without affecting current correctness

As with coordination state, git history remains the retained trail, and live current-state blobs
should stay compact.

---

## 11. Recommended Acceptance Criteria

The event engine architecture should be considered sound when all of the following hold:

- recurring plans use continue-as-new at the plan boundary
- hooks run against verified coordination state
- multiple PRISM Services can evaluate the same trigger without duplicate execution
- exactly one service can acquire execution ownership for a given event id
- a crashed winner leaves visible execution state that supports retry policy
- event execution records do not pollute the hot coordination summary state
- the PRISM Service remains an optimization, not authority

---

## 12. Final Rule

The event engine should be understood like this:

- the PRISM Service is the preferred execution plane, but authoritative event
  execution ownership is still acquired through shared-ref CAS on a separate event execution
  namespace
