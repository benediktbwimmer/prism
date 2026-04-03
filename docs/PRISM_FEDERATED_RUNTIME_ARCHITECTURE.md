# PRISM Federated Runtime Architecture

Status: proposed target design
Audience: PRISM core, storage, auth, coordination, and MCP maintainers
Scope: replacing a centralized shared runtime database with a federated repo-native model

---

## 1. Summary

PRISM should support a federated runtime architecture that removes the need for a mandatory shared
Postgres runtime for most repositories.

The design uses three primary layers:

- shared Git refs for compact authoritative coordination and runtime discovery
- worktree-local SQLite for rich append-only runtime state and hot local serving
- optional blob-backed session exports for cold retrieval, replay, and historical continuity

An optional thin API service may sit on top of blob exports and runtime descriptors, but it should
not be the primary live authority.

The core principle is:

- Git refs carry the shared facts that must converge across runtimes
- local SQLite carries the rich facts that are too large, too chatty, or too local for Git refs
- direct runtime-to-runtime queries and blob exports provide opportunistic enrichment, not required
  correctness

This makes PRISM a distributed system, but in a repo-native way rather than a database-first way.

---

## 2. Problem

Several current and planned PRISM documents assume a "shared runtime authority" plane that may
eventually be implemented by Postgres or another centralized shared backend.

That model works, but it has real costs:

- an always-on shared service becomes part of the product's correctness envelope
- local-first usage grows a second operational dependency
- many repositories do not need database-grade live collaboration semantics
- centralizing all rich runtime state encourages overloading the shared backend with data that is
  expensive, noisy, or only locally valuable

At the same time, fully local-only PRISM is not enough for multi-agent and multi-worktree use:

- claims and leases must converge across agents
- task and plan continuity must be shared
- portfolio dispatch needs repo-wide visibility
- principals, runtime capabilities, and active sessions may need to be discoverable

The architecture should therefore avoid both extremes:

- not purely local and isolated
- not forced into a central database for everything

---

## 3. Design Goals

Required goals:

- eliminate mandatory dependence on a centralized shared Postgres runtime for normal repo use
- preserve a durable shared coordination truth across worktrees, branches, and machines
- keep rich append-only runtime state local by default
- support optional runtime-to-runtime state exchange when peers are reachable
- support durable cold retrieval of session/runtime state after a runtime exits
- preserve local-first and offline operation
- keep correctness independent of peer liveness
- keep authority boundaries explicit and inspectable

Required non-goals:

- no peer-to-peer topology should be required for correctness
- PRISM does not need to solve arbitrary public-internet service discovery without an explicit relay
- the design does not require every local SQLite file to be globally replicated
- a blob store does not become the live mutable source of truth
- Git refs do not become a transport for large journals or high-frequency presence chatter

---

## 4. Core Architecture

PRISM should evolve toward a four-layer federated model.

### 4.1 Shared coordination refs

The shared coordination ref design from
[PRISM_SHARED_COORDINATION_REFS.md](./PRISM_SHARED_COORDINATION_REFS.md)
remains the authoritative shared control plane.

It carries compact durable shared facts such as:

- plans
- task status
- claims
- leases
- publish state
- integration state
- runtime discovery descriptors

This is the minimum shared truth every runtime must agree on.

For v1, that shared truth should live under one canonical ref per logical repository:

```text
refs/prism/coordination/<logical-repo-id>/live
```

The shared coordination tree may contain multiple domains and indexes, but they should share one
live head so claims, leases, task state, publish state, and runtime descriptors converge as one
coherent snapshot.

### 4.2 Local runtime SQLite

Each worktree-local runtime keeps a rich SQLite store for:

- signed change sets
- detailed append-only journals
- high-resolution lineage and replay inputs
- local caches and serving accelerators
- local outcomes and memory materializations
- unpublished draft work
- runtime-local diagnostics

This is the primary home for rich runtime detail.

### 4.3 Optional peer transport

When runtimes are live and reachable, they may query each other directly for richer state than what
fits in the shared coordination ref.

Examples:

- replay slices
- detailed task history
- hot execution overlays
- local-only diagnostics
- not-yet-exported runtime bundles

Peer exchange is optional and opportunistic.

For public-internet reachability, PRISM should support a `relay_only` transport mode in which the
runtime opens an outbound long-lived connection to a dumb relay. The relay is transport only: it
does not interpret PRISM semantics, does not become an authority plane, and must be treated as
fully untrusted.

### 4.4 Optional blob-backed archive/export

When a runtime exits, rotates, or compacts local state, it may export signed bundles to object
storage.

Those bundles are for:

- cold replay
- historical recovery
- delayed inspection
- remote access when the original runtime is gone

They do not replace the shared coordination ref.

---

## 5. Authority Planes

This architecture revises the old "shared backend database" assumption into a more precise set of
authority planes.

### 5.1 Repo-published authority

Tracked `.prism/state/**` and its signed manifest remain repo-published authority for repo-scoped
knowledge and branch-published intent.

### 5.2 Shared coordination authority

Shared coordination refs become the authoritative cross-branch coordination plane.

This replaces the "all shared mutable truth lives in Postgres" assumption for many workloads.

### 5.3 Local runtime authority

Each live runtime remains authoritative for its own rich local journal and execution-local detail
while it is alive.

This includes facts that do not need immediate global convergence.

### 5.4 Exported archive authority

Signed exported runtime bundles are authoritative only for the bounded historical state they
describe.

They are cold artifacts, not the live mutable control plane.

### 5.5 Derived serving state

Any query-side index, bundle, or cache remains derived.

This keeps the authority model clean even in a federated topology.

---

## 6. Runtime Descriptor Model

To support peer discovery without making peers mandatory, the shared coordination ref should carry a
small runtime descriptor per live runtime.

Illustrative descriptor fields:

- `runtime_id`
- `repo_id`
- `worktree_id`
- `principal_id`
- `instance_started_at`
- `last_seen_at`
- `branch_ref`
- `checked_out_commit`
- `capabilities`
- `discovery_mode`
  - `none`
  - `lan_direct`
  - `relay_only`
- optional `peer_endpoint`
  - direct IP or hostname plus port for trusted local-network reachability
- optional `relay_endpoint`
  - relay route such as `wss://relay.example/route/<runtime_id>`
- optional `peer_transport_identity`
- optional `blob_snapshot_head`
- optional `export_policy`

The descriptor must stay compact.

It exists to answer:

- which runtimes are alive for this repo?
- what can they serve?
- how recent are they?
- where can PRISM query them, if allowed?
- what exported state exists if they are gone?

### 6.1 Discovery privacy modes

Peer disclosure must be explicit and policy-controlled.

Recommended modes:

- `none`
  - do not disclose a network endpoint
- `lan_direct`
  - disclose a peer endpoint expected to be reachable on the trusted local network
- `relay_only`
  - disclose only a relay handle and route identity; the runtime is reachable through an outbound
    relay connection, not by exposing a direct inbound socket
- `full`
  - disclose both direct local-network endpoint and supported relay capabilities

This avoids turning peer presence into an accidental privacy leak.

---

## 7. Local SQLite as the Rich Runtime Substrate

Local SQLite should remain the default rich runtime store.

### 7.1 What belongs in local SQLite

Good local-SQLite candidates:

- signed change sets
- full append-only mutation journals
- detailed memory and outcome event logs
- replay-oriented history slices
- expensive local projections
- high-resolution diagnostics
- worktree-local caches

These are too large or too chatty for shared refs.

### 7.2 What should not be forced into local SQLite only

Bad local-only candidates:

- active claims
- current lease truth
- publish state that other agents depend on
- target integration state
- runtime discovery descriptors

Those need shared convergence and belong on shared refs.

### 7.3 Local-first remains the default

A runtime must continue to work correctly when:

- no peer is reachable
- no blob store is configured
- no thin API service exists

In that case, PRISM still has:

- local SQLite
- local worktree state
- repo-published `.prism`
- shared coordination refs when Git remote is reachable

That is enough for correct operation.

---

## 8. Peer-to-Peer Query Model

Peer querying should be an enrichment path, not a correctness dependency.

### 8.1 What peers may serve

Peers may expose authenticated read APIs for:

- bounded replay slices
- bounded task history
- hot execution overlays
- local unexported journals
- local serving projections
- runtime diagnostics

### 8.2 What peers should not be required to serve

Peers should not be mandatory for:

- claims
- leases
- task truth
- plan truth
- publish truth

Those must already be recoverable from shared refs plus repo-published state.

### 8.3 Motivating workflows

The federated peer layer matters because it lets agents collaborate before state is reduced to Git.

High-value examples:

- pre-commit conflict avoidance
  - before starting a broad refactor, a runtime can ask peers for bounded draft-scope signals such
    as touched paths, active edit focus, or claimed local draft areas
  - this lets agents detect likely conflicts and block or narrow work before either side commits
- speculative branching with live consensus
  - when two implementation directions are plausible, two runtimes can take bounded speculative
    passes in parallel, exchange uncommitted diffs and local validation results over peer transport,
    and commit only the winning direction
  - this keeps Git history clean by confining exploratory loser branches to ephemeral peer exchange
- live API contract negotiation
  - a producer runtime can expose bounded draft interface state such as type signatures or local
    schema fragments while a consumer runtime attempts integration against that draft
  - peers can negotiate missing fields, contract changes, and integration fixes before either side
    publishes a commit, so the first durable commit is already closer to a validated contract
- unsticking and thrash detection
  - if another agent has held a lease for a long time without publishing, a peer query can reveal
    bounded diagnostics such as repeated failed checks, current validation loops, or stuck replay
    state
  - that makes intervention, assistance, or handoff decisions possible without blind waiting
- distributed test partitioning
  - one runtime can distribute bounded validation slices across other idle runtimes, gather the
    partial results over peer transport, and aggregate them into one shared validation conclusion
  - this turns local multi-machine availability into a lightweight parallel validation fabric
- expensive ephemeral index sharing
  - one runtime may already hold a costly local semantic index, replay slice, or deep diagnostic
    materialization in SQLite
  - another runtime should be able to reuse that bounded artifact through peer exchange instead of
    rebuilding it from scratch
- experience transfer without durable memory pollution
  - a runtime that just finished a subtle debugging session can expose bounded recent traces,
    hypotheses, and failed attempts to another peer working on a related surface
  - this lets peers benefit from raw local experience without prematurely promoting every useful
    but still-contextual lesson into durable repo memory
- live debugging and "rubber ducking"
  - a stuck agent should be able to send a bounded debug packet containing local scratch context,
    selected uncommitted file state, terminal output, and current diagnostics
  - another runtime can inspect that packet and respond without the first runtime having to publish
    noisy intermediate state into Git
- human-agent live pairing
  - a human using PRISM CLI or a future UI should be able to act as a bounded peer, inspect a
    runtime's current working packet, and send high-priority corrective feedback or pairing input
  - this keeps human intervention inside the same capability-scoped coordination fabric instead of
    forcing process restarts or prompt resets
- seamless shift-change handoffs
  - when an agent rotates out, hits a token limit, or moves from one machine to another, the next
    runtime can inherit a bounded handoff packet containing open work context, temporary notes, and
    uncommitted progress summaries
  - this preserves continuity without pretending that unfinished operational context belongs in the
    permanent repository history

These scenarios are the reason the peer layer exists. It is not just a transport convenience. It
is the mechanism that lets PRISM share rich operational context without turning that context into
repo-published authority.

### 8.4 Query preference order

For a rich historical or local-diagnostic query, PRISM should prefer:

1. local hot/runtime state
2. peer live runtime if available and trusted
3. exported blob bundle if available
4. bounded cold local reconstruction

For authoritative shared coordination queries, PRISM should prefer:

1. shared coordination refs
2. repo-published `.prism` if relevant
3. local/peer enrichment only as additional context

### 8.5 Authentication and trust

Peer queries must be authenticated and policy-controlled.

The peer runtime should be able to verify:

- the calling principal
- the caller's capability to read the requested data class
- the freshness and origin of the request

Direct peer transport should never bypass PRISM's principal and capability model.

For public-internet or relay-mediated transport, peer traffic should be end-to-end encrypted and
mutually authenticated using key material bound to PRISM principal identity. The relay must only see
opaque encrypted payloads and routing metadata.

Authorization should be explicit and repo-scoped, not inferred from generic participation.
Illustrative peer-read capabilities:

- `can_discover_runtime`
- `can_read_peer_diagnostics`
- `can_read_peer_journals`
- `can_read_peer_replay`
- `can_request_bundle_export`

Shared refs remain the trust anchor for discovering which principals are active for a repo, but
peer access should be granted by explicit capability policy rather than by "this principal has
participated in the repo before."

---

## 9. Blob Export and Retrieval Model

Blob export is the answer to "what happens when the runtime that owned the interesting local
journal is gone?"

### 9.1 Exported bundle shape

An exported bundle should be:

- signed
- content-addressed
- immutable
- self-describing
- bounded in scope

Illustrative contents:

- runtime descriptor snapshot
- principal and work context
- bounded journal slices
- bounded replay slices
- compaction metadata
- checksums for included SQLite-derived exports

PRISM should export derived interchange bundles, not raw SQLite files by default.

Raw database upload is possible as an escape hatch, but it should not be the primary contract.

### 9.2 Blob pointer publication

If a runtime exports a bundle, the shared coordination ref may publish a compact pointer such as:

- `blob_store_kind`
- `object_uri`
- `content_digest`
- `exported_at`
- `bundle_kind`
- `retention_until`

The pointer is shared truth.
The blob is cold payload.

### 9.3 When exports happen

Exports may happen:

- on graceful runtime shutdown
- on explicit archive or compact command
- on lease relinquish for a task with valuable local context
- periodically for long-running sessions

The export policy should be configurable per repo or per runtime.

### 9.4 Thin API service

An optional API service may serve:

- runtime descriptor discovery
- blob metadata lookup
- authenticated download of exported bundles
- dumb relay access to live peers

This service is useful, but it should remain thin.

It should not become the sole live state authority.

---

## 10. Identity and Secrets

This model interacts with identity differently than the old centralized-runtime assumption.

### 10.1 Shared identity metadata

Principal and credential metadata may still need a shared view across machines.

In a federated model, that shared identity metadata can live in one of two ways:

- on shared coordination refs if the metadata set stays compact enough
- in signed exported identity bundles referenced by shared refs

The key rule remains:

- shared metadata is fine
- secret credential material remains local

This stays consistent with
[SHARED_IDENTITY_FUTURE_STATE.md](./SHARED_IDENTITY_FUTURE_STATE.md),
but changes the storage substrate from "necessarily Postgres" to "federated shared state."

### 10.2 Secrets stay local

PRISM should not publish:

- bearer tokens
- private keys
- local active-profile selection

Those remain machine-local.

### 10.3 Peer disclosure is not peer authority

Publishing a peer endpoint in shared refs does not automatically make that peer trustworthy.

Trust still comes from:

- principal identity
- capability checks
- signed runtime descriptors or certificates
- local policy

Peer trust over the public internet should therefore be understood as:

- relay for reachability
- end-to-end encryption for confidentiality
- principal-bound authentication for identity
- explicit repo-scoped capabilities for authorization

---

## 11. Leases, Presence, and Heartbeats

This architecture should keep the same principle already established for shared coordination refs:

- durable lease truth is shared
- heartbeat noise is not

### 11.1 Presence descriptor vs lease authority

A runtime descriptor can say:

- I am alive
- I was seen recently
- I can answer peer queries

But it must not replace lease truth.

Lease truth still belongs in shared coordination state.

### 11.2 Heartbeats should remain low-frequency

Runtimes may refresh their discovery descriptor or lease timestamps occasionally, but not at
database-heartbeat frequency.

Examples of acceptable refresh triggers:

- runtime startup
- explicit renewal near expiry
- meaningful authenticated mutation
- periodic bounded keepalive for descriptors

The system should not depend on sub-second or even every-few-seconds updates.

---

## 12. Failure Model

Federated systems only work if the failure semantics are honest.

### 12.1 Peer unavailable

If a peer is down or unreachable:

- shared coordination remains correct
- rich peer-served data may be temporarily unavailable
- PRISM falls back to local or exported-state retrieval

This is expected behavior, not an exceptional corruption case.

### 12.2 Blob store unavailable

If blob storage is unavailable:

- live operation still works
- export fails explicitly
- rich cold replay continuity may be reduced

Blob export must never be required for live correctness.

### 12.3 Shared ref unavailable

If Git remote or shared coordination ref access is unavailable:

- local work may continue in a degraded/offline mode
- shared claims and leases cannot be authoritatively refreshed
- PRISM must surface the degraded coordination state honestly

This is the true correctness dependency.

### 12.4 Divergent local journals

Different runtimes will naturally hold different rich local state.

That is acceptable as long as:

- shared coordination facts converge
- exported bundles are signed and attributable
- queries say whether they are reading local, peer, or exported historical state

---

## 13. Query and Projection Semantics

This architecture changes how query surfaces should describe authority.

### 13.1 Query classes

Useful query classes in a federated model:

- `shared_authoritative`
  - shared coordination refs and repo-published authority
- `local_authoritative`
  - local runtime authority only
- `peer_enriched`
  - shared/local answers enriched from a live peer
- `archive_enriched`
  - answers enriched from blob-exported historical bundles

Query surfaces should expose which class they used.

### 13.2 Projection rule stays unchanged

Projections remain derived no matter where the source came from:

- shared refs
- local SQLite
- peer runtime
- blob export

This is important. The federated model changes where data comes from, not the rule that projections
must not silently become authority.

---

## 14. Retention and Garbage Collection

This model fits well with the retention policy already described in
[PRISM_HOME_RETENTION_AND_GC.md](./PRISM_HOME_RETENTION_AND_GC.md).

### 14.1 Local SQLite retention

Local rich runtime state should retain the longest when it is:

- expensive to rebuild
- still referenced by active exports
- the only copy of valuable hot local context

But it can still be compacted or exported over time.

### 14.2 Export retention

Blob exports should have explicit retention metadata:

- export generation
- retention deadline
- archive class
- content digest

This lets operators expire old exported bundles safely.

### 14.3 Shared ref growth

The shared coordination ref must stay compact and aggressively compactable.

That is non-negotiable.

The federated model only works if:

- shared refs stay small
- local SQLite absorbs rich churn
- blob exports absorb cold historical mass

---

## 15. Comparison with a Centralized Shared Postgres Runtime

### 15.1 What Postgres still does well

A centralized shared runtime is still attractive for:

- very low-latency shared reads
- centralized operator control
- unified cross-machine query infrastructure
- simpler service discovery

For large hosted deployments, it may still be the right answer.

### 15.2 Why federated should be the default target

For repo-native PRISM, the federated design is a better default because:

- it preserves local-first operation
- it reduces deployment burden
- it keeps the repository and its runtimes self-contained
- it lets shared truth be Git-native and auditable
- it avoids shipping every rich local journal into a central mutable service

### 15.3 Optional hosted layer

A hosted service may still exist on top of the federated model.

But it should act as:

- relay
- cache
- archive index
- bundle fetch service

not as the only live truth.

---

## 16. Migration Plan

### Phase 1: shared coordination refs

Implement the shared coordination ref model first.

Without that, the rest of the federated design has no reliable shared control plane.

That first phase should also lock two implementation constraints:

- shell Git is the initial fetch/push/CAS transport behind a narrow backend abstraction
- branch-local `.prism/state/**` mirrors of shared coordination remain optional and derived, not a
  second authority plane

### Phase 2: runtime descriptors

Add compact runtime discovery descriptors to shared coordination state.

### Phase 3: peer read protocol

Add authenticated peer read APIs for bounded rich-state exchange.

### Phase 4: export bundles

Add signed exported runtime bundles and shared-ref pointers to them.

### Phase 5: optional relay/API service

Add a thin service for:

- bundle metadata
- authenticated retrieval
- dumb peer relaying over outbound runtime connections

### Phase 6: central shared DB becomes optional

At this point Postgres or another centralized shared backend becomes a deployment option, not a
required architecture assumption.

---

## 17. Testing Requirements

Implementation should add coverage for:

- multi-worktree shared coordination with no shared DB
- local runtime rich state surviving independently per worktree
- peer discovery through shared refs
- authenticated peer read success and denial
- relay-mediated peer exchange with end-to-end encryption
- explicit capability enforcement for peer diagnostics and journal reads
- graceful degradation when peers disappear
- signed export bundle generation and retrieval
- bundle pointer publication through shared refs
- query surfaces reporting whether they used local, peer, or archive enrichment
- retention and compaction behavior for local SQLite plus export bundles

---

## 18. Recommendation

PRISM should adopt a federated runtime architecture as the default multi-agent model.

The recommended stack is:

- shared coordination refs for compact shared truth
- local SQLite for rich runtime authority
- optional peer exchange for live enrichment
- optional blob export for cold continuity
- optional thin API service for retrieval and relaying

That architecture keeps PRISM:

- repo-native
- local-first
- auditable
- distributed
- operationally lighter than a mandatory shared database

Postgres can still exist as an optional deployment backend later, but it should not be required to
make PRISM feel like one coherent distributed system.
