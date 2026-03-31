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
- principals are machine-local by default, while coordination authority remains repo-scoped

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

### 5.1.1 Principal scope

Principals should be machine-local by default, not repo-local.

That means:

- the same human can keep one identity across multiple repos on the same machine
- the same agent can keep one identity across multiple worktrees or repos on the same machine
- leases, claims, plans, and coordination authority remain repo-scoped
- provenance events should snapshot enough actor information that history remains legible even when
  the original local principal registry is unavailable on another machine

This intentionally separates identity portability from repo-scoped authority.

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

Lease refresh should be narrow, not broad:

- one mutation should refresh only the specific task or claim lease it touches
- a mutation on Task A must not silently keep an abandoned lease on Task B alive

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

## 7. Credential handling and storage

### 7.1 Server-side handling

PRISM should store only hashed credential material on the server side.

The principal registry should persist:

- `principal_id`
- token hash or equivalent verifier
- lifecycle state
- parent linkage when present
- timestamps such as creation, rotation, revocation, and last successful use

### 7.2 Client-side handling

Credential storage must be practical for the actor type using it.

Expected default patterns:

- human local development
  - a local gitignored credential file such as `.prism/credentials.local`
- CI or automation
  - environment variable injection such as `PRISM_PRINCIPAL_TOKEN`
- spawned agents
  - minted credential returned directly to the parent and passed into the child's initial context
    without requiring durable disk storage

The exact storage format is still an implementation detail, but practical human and CI flows are part
of the initial design, not a later afterthought.

---

## 8. Provenance and audit trail

### 8.1 Canonical provenance belongs in the event log

PRISM should not thread principal ids into every domain entity as canonical state.

The authoritative provenance should live in append-only mutation events.

This matches PRISM's existing event-oriented structure and keeps identity history audit-friendly.

### 8.2 Extend event actor metadata from coarse kind to durable identity

Current event metadata already records an `actor`, but today it is only a coarse category such as
`Agent`, `User`, or `System`.

PRISM should evolve this so authoritative mutation events record:

- principal kind
- principal id
- optional event-time snapshot fields where needed for audit stability

### 8.3 Use projections for fast lookups

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

## 9. Practical behavior in the common host-agnostic case

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

## 10. System and runtime principals

System-authored mutations should use the same principal model for provenance, but not the same
external credential path as human or agent principals.

The recommended design is:

- runtime/system principals live in a privileged internal namespace
- system-authored actions do not require bearer-token authentication through the external credential
  registry
- provenance should still clearly identify the acting subsystem, for example `daemon`,
  `settle_worker`, or `checkpoint_writer`

This keeps internal system behavior:

- non-impersonatable by external clients
- cheap for high-frequency runtime operations
- clearly distinguishable from human or agent activity in audit trails

---

## 11. Non-goals

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

## 12. Resolved implementation decisions

The following points were previously open questions and are now resolved for the target design.

### 12.1 Credential lifecycle

- One principal may have multiple active credentials at once.
- Credential rotation should be zero-downtime:
  - mint a new credential through an authenticated mutation
  - revoke the old credential once the replacement is live
- Credentials are revoked explicitly through a revocation mutation.
- Revoking a credential does not immediately kill active leases, because leases belong to the
  principal, not to one specific credential.
- A revoked credential simply loses the ability to authenticate future mutations, so any affected
  lease will naturally become stale and later expire if no other valid credential refreshes it.

### 12.2 Parent-child principal semantics

- Subagents are not forced to be separate principals in every case.
- A parent may choose:
  - to let a helper operate under the same principal when it is effectively the same logical actor
  - or to mint a distinct child principal when the helper is a meaningfully separate agent
- When a parent mints a brand new child principal, parent linkage is required.
- Parent-child relationships are immutable once minted.
- Provenance must preserve both:
  - direct authorship by the child principal
  - the lineage chain through parent principals

### 12.3 Lease policy

- Default lease timings:
  - `stale` after 30 minutes with no authenticated mutation touching that specific lease
  - `expired` after 2 hours
- Leases attach to both tasks and claims:
  - task lease = execution ownership of a workflow node
  - claim lease = active lock/ownership over a claimed coordination object
- Lease refresh is narrow:
  - one mutation refreshes only the specific task or claim lease it touches
  - activity on Task A must not keep a stale lease on Task B alive
- A lightweight authenticated `heartbeat_lease` mutation is a valid future extension for long silent
  work, but the first implementation must not depend on agents using it correctly or on agents having
  a reliable sense of time.

### 12.4 Resume versus reclaim policy

- `resume` is for the same principal returning to its own stale lease.
- `reclaim` is for a different principal intentionally taking over stale work.
- Reclaim transfers the lease.
- If the reclaimer differs from the current assignee, reclaim should also append an assignment change
  so authored ownership converges with execution ownership by default.
- Event taxonomy:
  - `TaskResumed`
  - `TaskReclaimed`
  - `TaskHandedOff`
  - `TaskReopened`

### 12.5 Event schema shape

- `EventActor` should be enriched directly rather than leaving identity outside the actor shape.
- The authoritative mutation actor should record at least:
  - `principal_id`
  - principal kind
- Mutation events should snapshot:
  - principal kind
  - human-readable name
- Additional actor snapshot fields may be added later where audit stability materially benefits, but
  `kind` and `name` are the initial minimum.

### 12.6 Principal profile mutability

- Immutable principal state:
  - `principal_id`
  - `kind`
  - creation timestamp
  - `parent_principal_id` when present
- Mutable profile state:
  - `name`
  - `role`
  - model, harness, handles, and other profile metadata
- The human-readable `name` is mutable.
- Historical audit stability comes from event-time snapshots, not from keeping an alias history in
  the principal profile for v1.
- If an agent changes model or harness, the live profile reflects the new state while historical
  events keep the old snapshot.

### 12.7 Human principal UX

- Human principal support lands together with agent principal support.
- Humans must be able to obtain credentials on day one because mutation auth is mandatory from the
  first cutover.
- The primary human flow should be CLI-driven, for example:
  - `prism auth init`
  - `prism auth login`
- Human credentials should be stored in a practical local mechanism such as:
  - `~/.prism/credentials.toml`
  - or a later OS-keychain-backed equivalent

### 12.8 Imported external identities

- Observational identities such as git authors remain distinct from authenticated principals.
- The relationship is a soft link, not an auth claim.
- Principals may attach linked emails or handles for grouping and UX purposes.
- A bare external identity must never be treated as proof of authenticated PRISM authorship.

### 12.9 Query and projection surfaces

- Day-one first-class views should expose at least:
  - active lease holder by `principal_id` and `name`
  - assigned principal by `principal_id` and `name`
- These must appear in the core task and plan read surfaces such as task briefs and plan summaries.
- Eager projections should include:
  - active lease holder
  - assigned principal
- Full authorship trails may remain event-derived on demand initially.

### 12.10 Backward compatibility and migration

- The cutover is intentionally hard.
- `prism_session` is removed rather than supported through a long compatibility window.
- All mutations require credentials from day one of the cutover.
- Existing coarse actor history should be migrated through synthetic fallback principals, for example:
  - `legacy_agent_fallback`
  - `legacy_human_fallback`
- Missing-credential mutations should fail loudly and descriptively.
- Instructions and surrounding tooling should clearly state how agents obtain and pass mutation
  credentials.

### 12.11 Remaining future policy space

The core design questions are now settled. Future work may still refine:

- additional actor snapshot fields beyond `kind` and `name`
- optional `heartbeat_lease` ergonomics
- stronger human credential storage such as OS keychain integration
- richer query and UI surfaces for lineage and provenance exploration

---

## 13. Recommended implementation order

When implementation starts, the safest sequence is:

1. Define the principal core types, credential registry, and event metadata changes.
2. Add mutation-envelope authentication for authoritative writes.
3. Record canonical provenance in mutation events.
4. Introduce explicit lease lifecycle and stale/reclaim semantics.
5. Add projections and query surfaces for authorship and identity history.
6. Add CLI principal mint/show/lineage flows and child-principal minting for multi-agent orchestration.

This order minimizes the risk of building ad hoc ownership semantics before the identity substrate is
stable.
