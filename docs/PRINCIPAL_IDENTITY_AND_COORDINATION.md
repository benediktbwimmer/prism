# PRISM Principal Identity and Coordination

Status: pre-implementation design note  
Audience: PRISM core, coordination, MCP, runtime, and storage maintainers  
Scope: identity, authentication, provenance, and lease semantics for shared multi-actor coordination

---

## 1. Summary

PRISM should introduce a first-class **principal identity** system for all state-changing
coordination and runtime mutations.

The key design constraint is that PRISM cannot rely on ambient MCP session behavior for durable
identity. Different clients may:

- reuse one MCP session across multiple threads or subagents
- reconnect and receive a different logical MCP session
- keep a connection idle for long periods while still working on a task
- hide the real agent/runtime topology from the server

That means transport session state is useful only for observability, request correlation, or other
non-authoritative convenience behavior. It is not a reliable authority boundary.

The canonical design is:

- every mutation authenticates an acting principal explicitly
- read-only queries and resources remain unauthenticated by default
- execution ownership is represented by leases, not by permanent session attachment
- leases are refreshed only by authenticated mutations
- durable provenance is recorded in the event log and projected into read models
- the current `prism_session` tool is removed as a coordination surface

---

## 2. Why this is needed

PRISM already models coordination state, claims, handoffs, and explicit session-scoped work, but
that is not enough for robust multi-agent operation when the MCP client topology is outside PRISM's
control.

Without a stronger identity model, PRISM cannot safely answer:

- who created or changed this task
- which agent is the intended logical owner of this work
- whether a resumed mutation is the same actor as before
- whether a stale in-progress task should be resumed, reclaimed, or handed off
- how multiple concurrent agents sharing one MCP daemon should authenticate themselves

The goal is to make these answers independent of client-specific MCP session behavior.

---

## 3. Core design decisions

### 3.1 Use principals, not just agents

The system should model all actors through one shared abstraction: `Principal`.

Principal kinds should include at least:

- `human`
- `agent`
- `system`
- `ci`
- `external`

This keeps audit, coordination, and provenance uniform across humans, agents, and runtime-authored
actions.

### 3.2 Separate durable identity from live execution state

PRISM should model three distinct concepts:

- `PrincipalId`
  - durable logical actor identity
  - used for attribution, assignment, audit, and handoff semantics
- credential
  - authentication secret proving control of a principal
  - not the same thing as the principal id
- lease
  - ephemeral execution ownership for an active task or claim
  - renewable and expirable

This separation is necessary because a logical actor may reconnect, restart, or spawn subagents
without preserving a single live transport session.

### 3.3 Do not trust ambient MCP session state as authority

Ambient MCP session state may still exist for ergonomics and observability, but it must not be the
source of truth for authenticated mutation identity.

Specifically:

- PRISM must not assume one MCP session maps to exactly one agent
- PRISM must not assume one thread or subagent gets its own MCP session
- PRISM must not bind durable ownership to the transport session alone

### 3.4 Remove the current `prism_session` tool as a coordination mechanism

The current explicit `prism_session` setup model should be removed from the coordination design.

In the target model:

- PRISM should not require an explicit session-binding call before coordination can work
- the acting principal comes from the mutation credential
- execution ownership comes from leases
- any surviving per-client convenience state should live in narrower non-authoritative surfaces, not
  in a general session abstraction

### 3.5 Require credentials on all mutations

Every `prism_mutate` call that changes authoritative state should carry an acting principal
credential at the mutation envelope level.

This includes at least:

- coordination mutations
- claim acquire, renew, and release
- task resume, reclaim, reopen, assign, and handoff mutations
- memory/outcome/feedback mutations
- any future mutation that writes authoritative repo state

Read-only queries and resources should remain credential-free unless a future feature explicitly
requires authenticated reads.

### 3.6 Keep the credential cost bounded

The credential should be passed once at the outer mutation envelope, not repeated in nested payloads.

This keeps prompt/token overhead modest while still making mutation authorship explicit and robust.

---

## 4. Authentication model

### 4.1 Mint principal credentials through mutation, not resources

Credential minting is state-changing and non-cacheable, so it must be a mutation surface, not a
resource.

A future mutation should mint one or more credentials, for example:

- one credential for the current actor
- one or more child credentials for subagents

### 4.2 Use a public identity and a separate secret

PRISM should not make the principal id itself the secret.

Instead, credentials should conceptually consist of:

- `principal_id`
- `principal_token`

The server should treat:

- `principal_id` as public identity
- `principal_token` as the bearer secret

The token should be unguessable and stored only in hashed form on the server side.

### 4.3 Parent-child agent minting should be supported

Subagents should be representable as distinct principals when desired.

The model should support:

- parent principals minting child credentials
- provenance showing parent-child relationships
- later queries such as "which agents were spawned from this agent during this task"

---

## 5. Principal model

The principal system should have a small shared core plus kind-specific optional attributes.

### 5.1 Shared core

Every principal should have at least:

- `principal_id`
- `kind`
- `name` or equivalent human-readable label
- lifecycle status
- created/updated timestamps

The human-readable name should be required at creation time. PRISM should not rely on anonymous or
purely generated fallback labels for durable principals.

### 5.2 Kind-specific optional attributes

Different principal kinds should be able to store optional attributes without polluting the common
core.

Examples:

- agent
  - model
  - harness
  - parent principal
  - runtime label
- human
  - email
  - display name
  - handle
- system
  - subsystem name
  - daemon/runtime instance label
- ci
  - pipeline name
  - provider
- external
  - source kind
  - imported handle or email

These should be modeled as optional kind-specific profile attributes, not as mandatory fields on
every principal.

The distinction should be:

- `principal_id`
  - stable opaque identity
- `name`
  - required human-readable label
- `role`
  - optional semantic purpose such as `reviewer`, `planner`, or `validator`
- kind-specific attributes
  - optional profile metadata

---

## 6. Lease model

### 6.1 Leases are for execution ownership

Leases should represent active execution ownership and contention, not durable authorship.

They are the right mechanism for:

- active task execution
- active claims
- stale-work detection
- reclaim/resume semantics

### 6.2 Leases must expire

Tasks and claims must not remain permanently stuck because a process died, a thread disappeared, or a
subagent was abandoned.

That means leases must:

- refresh only through authenticated mutations by the acting principal
- become stale after inactivity
- eventually expire

### 6.3 Ambient activity must not refresh leases

Ordinary MCP activity such as queries, resource reads, or other ambient transport traffic must not
refresh live lease state.

The lease system must remain correct even if:

- the transport reconnects
- the MCP session changes
- a client reuses one MCP session for multiple agents
- one subagent stays active while another subagent sharing the same ambient context stalls
- there is a long idle period without authenticated mutation traffic

### 6.4 Reclaiming stale work must be explicit

When a lease becomes stale or expired, PRISM should not silently pretend continuity.

Instead, it should surface the task or claim as reclaimable or resumable, with an explicit mutation
path for takeover or continuation.

---

## 7. Provenance and audit trail

### 7.1 Canonical provenance belongs in the event log

PRISM should not thread principal ids into every domain entity as canonical state.

The authoritative provenance should live in append-only mutation events.

This matches PRISM's existing event-oriented structure and keeps identity history audit-friendly.

### 7.2 Extend event actor metadata from coarse kind to durable identity

Current event metadata already records an `actor`, but today it is only a coarse category such as
`Agent`, `User`, or `System`.

PRISM should evolve this so authoritative mutation events record:

- principal kind
- principal id
- optional event-time snapshot fields where needed for audit stability

### 7.3 Use projections for fast lookups

Fast lookup surfaces should be derived from the event log, not treated as the canonical source of
authorship truth.

Useful projections include:

- latest mutating principal for a task or plan
- all principals who touched a plan
- all mutations authored by a principal
- child principals minted by a parent principal

Optional denormalized read-model fields such as `created_by` or `last_updated_by` are acceptable as
projections, but not as the primary provenance store.

---

## 8. Practical behavior in the common host-agnostic case

This model is designed to work even when PRISM has no special cooperation from the MCP host beyond
the ability to call mutations.

For a common setup:

- one PRISM MCP daemon on a developer machine
- one Codex MCP config entry
- multiple concurrent Codex threads
- possible subagents
- possible shared MCP sessions beneath the surface
- no explicit `prism_session` setup call

PRISM should still remain correct because:

- authoritative identity comes from mutation credentials, not ambient MCP sessions
- active work ownership comes from leases, not permanent session attachment
- lease refresh happens only through authenticated mutations
- stale work can be reclaimed explicitly
- provenance is recorded for every mutation regardless of client transport behavior

This is the minimum robust design that does not require trusted client isolation.

---

## 9. Non-goals

This design note does not attempt to specify:

- the exact UI/UX for humans authenticating into PRISM
- the final on-disk schema for principal profile storage
- the final dashboard or MCP query surfaces for identity exploration
- a complete ACL or authorization model beyond mutation authorship and provenance

Those can be layered on top once the principal, credential, lease, and provenance foundations are in
place.

Even so, the initial rollout should include a practical human-facing mint path through the CLI.
The canonical operation should remain a mutation, but humans should not be expected to assemble raw
mutation payloads by hand.

---

## 10. Open questions that should be resolved before implementation

The following points should be answered explicitly before implementation begins.

### 10.1 Principal scope

- Are principals repo-local, workspace-local, machine-local, or globally portable across repos?
- If a principal exists in multiple repos, is that the same `principal_id` or separate repo-bound
  identities linked by a higher-level external identity?

### 10.2 Credential lifecycle

- Can one principal have multiple active credentials at once?
- How are credentials rotated?
- How are credentials revoked?
- What happens to active leases when the backing credential is revoked?

### 10.3 Parent-child principal semantics

- Are subagents always separate principals, or can a parent choose to delegate using the same
  principal?
- Should PRISM require explicit parent linkage for child-minted credentials?
- Should parent-child relationships be immutable once minted?

### 10.4 Lease policy

- What are the exact stale and expired timings?
- Are leases attached to tasks, claims, or both?
- Does one mutation refresh all active leases for that principal, or only the specific touched task
  or claim?
- How should long periods of local work without authenticated mutation activity be handled
  conservatively?

### 10.5 Resume versus reclaim policy

- When a stale lease exists, when is resumption allowed versus requiring explicit reclaim?
- Should reclaim preserve the original assignee and merely transfer execution ownership, or should it
  also update authored assignment by default?
- What event taxonomy should distinguish resume, reclaim, handoff, and reopen?

### 10.6 Event schema shape

- Should `EventActor` become a richer structured enum carrying `principal_id`, or should
  `EventMeta` gain separate principal fields while leaving actor kind coarse?
- Which principal attributes, if any, should be snapshotted into the event for audit stability?

### 10.7 Principal profile mutability

- Which principal attributes are mutable profile state versus immutable creation-time identity?
- Is the required human-readable `name` mutable, or should renames be modeled as profile aliases
  while preserving the original creation-time name?
- If an agent's model or harness changes, should historical events continue to show the old values
  via event snapshots, current profile lookups, or both?

### 10.8 Runtime and system principals

- Should runtime/system actors live in the same credential registry as user/agent principals, or in
  a privileged internal namespace?
- How should internal system-authored mutations authenticate without exposing user-visible bearer
  credentials?

### 10.9 Human principal UX

- How do human users obtain and manage credentials in a practical local-dev workflow?
- Is the first implementation agent-first, with human principals added immediately after, or should
  both land together?

### 10.10 Imported external identities

- How should observational identities such as git authors be related to authenticated principals?
- Do we support explicit linking between imported external identities and local principals, and if
  so, under what trust model?

### 10.11 Query and projection surfaces

- Which identity/provenance views must be first-class in MCP and query surfaces from day one?
- Which projections are required eagerly for performance, and which can remain derived on demand?

### 10.12 Backward compatibility and migration

- How are existing coordination events and entities migrated when they only distinguish coarse actor
  kinds?
- What compatibility behavior should old mutation callers receive before they provide credentials?
- How is the current `prism_session` tool removed or decomposed without leaving confusing partial
  semantics behind?

---

## 11. Recommended implementation order

When implementation starts, the safest sequence is:

1. Define the principal core types, credential registry, and event metadata changes.
2. Add mutation-envelope authentication for authoritative writes.
3. Record canonical provenance in mutation events.
4. Introduce explicit lease lifecycle and stale/reclaim semantics.
5. Add projections and query surfaces for authorship and identity history.
6. Add CLI principal mint/show/lineage flows and child-principal minting for multi-agent orchestration.

This order minimizes the risk of building ad hoc ownership semantics before the identity substrate is
stable.
