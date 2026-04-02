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
- every authoritative mutation must run under declared work context; the only bootstrap exception is
  the mutation that declares that work
- read-only queries and resources remain unauthenticated by default
- execution ownership is represented by leases, not by permanent session attachment
- leases are refreshed only by authenticated mutations
- durable provenance is recorded in the event log and projected into read models
- declared `work` is the provenance/intention unit, while `coord-task` remains the shared
  coordination object when one exists
- the current `prism_session` tool is removed as a coordination surface
- principals are machine-local by default, while coordination authority remains repo-scoped
- principal identity must carry an explicit issuing authority/namespace, so machine-local identity
  today can federate cleanly later
- repo-published `.prism` events must remain semantically self-contained on a cold clone; runtime
  ids may appear only as optional correlation handles

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

### 3.2.1 Distinguish principal identity from issuing authority

Machine-local principals need an explicit authority or namespace so they remain unambiguous once
multiple machines or runtimes contribute to shared storage.

The principal model should therefore distinguish:

- `principal_authority_id`
  - identifies the issuing authority or namespace for the principal id
- `principal_id`
  - stable principal identity within that authority

The externally visible representation may eventually combine them, for example:

- `principal:<authority>:<id>`

This keeps the design machine-local by default while making future multi-machine or Postgres-backed
federation a compatible extension rather than a redesign.

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

### 4.4 Credentials should carry minimal capabilities

PRISM should include a small capability layer on credentials from the start, even if it is coarse.

The goal is not a full ACL system. The goal is to avoid making every valid token equivalent to
global root access forever.

Examples of suitable coarse capabilities:

- `mutate_coordination`
- `mutate_repo_memory`
- `mint_child_principal`
- `admin_principals`
- `all`

This capability layer should be:

- attached to credentials, not principals
- checked at mutation boundaries
- intentionally small in v1

It exists to reduce blast radius, make child-principal minting safer, and leave room for later
shared-runtime authorization without redesigning all mutation paths.

### 4.5 Define the bootstrap trust root explicitly

The design must specify how the first authenticated principal is minted.

For the local-daemon deployment model, the default bootstrap should be:

- machine-local bootstrap only
- an explicitly trusted local CLI flow
- first-run init mints the initial human owner principal
- all later principal or credential minting happens through authenticated mutation

This avoids leaving first-credential issuance as an implicit or ad hoc ceremony.

---

## 5. Principal model

The principal system should have a small shared core plus kind-specific optional attributes.

### 5.1 Shared core

Every principal should have at least:

- `principal_authority_id`
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

### 5.3 CLI minting should be a first-class human path

The canonical backend operation remains a mutation, but humans should not be expected to assemble
raw mutation payloads by hand.

The initial rollout should therefore include a practical CLI minting/auth path for humans, and UI
support can follow later.

Representative commands might include:

- `prism auth init`
- `prism auth login`
- `prism principal mint --kind human --name <name>`
- `prism principal mint --kind agent --name <name> --parent <principal>`

The required human-readable `name` is distinct from an optional `role`. PRISM should require a
real name or label at principal creation time and should not rely on anonymous fallback labels for
durable principals.

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
- default lease timings should remain policy values, not schema axioms, so later task or plan
  classes can override them without redesign

### 6.3 Ambient activity must not refresh leases

Ordinary MCP activity such as queries, resource reads, or other ambient transport traffic must not
refresh live lease state.

The lease system must remain correct even if:

- the transport reconnects
- the MCP session changes
- a client reuses one MCP session for multiple agents
- one subagent stays active while another subagent sharing the same ambient context stalls
- there is a long idle period without authenticated mutation traffic

Because PRISM cannot assume that clients implement reliable background timers, the lease model
should not depend on agents free-running heartbeat intervals from local time perception alone.

Instead, task-scoped read surfaces may conditionally emit an explicit next-action instruction such
as "call `prism_mutate` with action `heartbeat_lease` before doing anything else" when the server
judges that a lease refresh is due soon or due now.

That instruction is advisory workflow guidance, not an implicit lease refresh. The read remains
unauthenticated and must not itself change live lease state.

### 6.3.1 Optional assisted watcher renewal may exist as an explicit trust policy

PRISM may additionally support an opt-in assisted lease-renewal policy for local deployments that
choose to trust worktree-local filesystem activity as a liveness proxy.

That policy must be treated as an explicit trust downgrade relative to strict mode:

- it is off by default
- it is enabled only through explicit local configuration such as an environment variable or
  per-workspace config
- it does not make filesystem activity into proof of agent identity
- it only authorizes PRISM to treat trusted local worktree activity as sufficient reason to send an
  authenticated `heartbeat_lease` mutation automatically

The assisted policy should be bounded and narrow:

- it applies only when exactly one principal is bound to the worktree
- it applies only when exactly one renewable lease is active for that worktree
- it disables immediately on ambiguity, contention, reclaim, handoff, or owner change
- it may extend a lease only for a bounded window since the last explicit authenticated mutation by
  that principal

This preserves the main lease invariant: raw ambient reads and transport activity still do not
refresh leases, and watcher assistance remains a local opt-in convenience rather than an
authoritative identity signal.

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

### 8.3 Record execution context separately from actor identity

Authoritative actor identity and non-authoritative runtime context should both be present in event
metadata, but they must remain distinct.

The actor portion should answer:

- who authenticated and authored this mutation

The execution-context portion should answer:

- which daemon or runtime instance handled it
- which worktree or repo context it came from
- which request or correlation ids were involved
- optionally which credential id or key id was used

Representative execution-context fields may include:

- `instance_id`
- `daemon_id`
- `worktree_id`
- request or session correlation ids
- `credential_id`

This gives PRISM richer debugging and audit surfaces without re-elevating transport/session state
into an authority boundary.

### 8.3.1 Record declared work context separately from runtime correlation

Authoritative mutation events must also snapshot declared work context separately from actor
identity and separately from runtime correlation ids.

The declared-work portion should answer:

- why this mutation happened
- whether the work was ad hoc, coordination-bound, or delegated
- which coordination task or plan it was serving when present

Representative declared-work snapshot fields may include:

- `work_id`
- work kind
- work title
- `parent_work_id`
- `coordination_task_id`
- `plan_id`
- plan title snapshot

This snapshot is semantic, not merely operational. Repo-published events must remain interpretable
from `.prism` alone on a cold clone with an empty runtime database.

That means:

- runtime-only ids may appear in repo-published events as optional correlation handles
- runtime-only ids must not be required resolution targets for understanding the event
- if a mutation references ad hoc or runtime-only work, the event must still carry enough inline
  work context to explain itself without shared runtime state

### 8.4 Use projections for fast lookups

Fast lookup surfaces should be derived from the event log, not treated as the canonical source of
authorship truth.

Useful projections include:

- latest mutating principal for a task or plan
- all principals who touched a plan
- all mutations authored by a principal
- child principals minted by a parent principal

Optional denormalized read-model fields such as `created_by` or `last_updated_by` are acceptable as
projections, but not as the primary provenance store.

### 8.5 Self-containment boundary by published surface

Repo-published `.prism` state must follow one simple rule:

- semantic context must resolve from `.prism` alone
- runtime-only ids may appear only as optional diagnostics or protected-stream signing metadata

Applied to current published surfaces:

- `.prism/plans/...`
  - may publish plan headers, nodes, edges, authored bindings, and repo-semantic handoff state
  - must not publish runtime session ids, worktree ids, branch refs, active lease holders, or
    other shared-runtime-only ownership fields
- `.prism/memory/events.jsonl`
  - may publish repo-scoped memory entries and their authored event snapshots
  - any work/task reference must be understandable without runtime lookup once declared-work
    cutover lands
- `.prism/concepts/...`, `.prism/contracts/...`, and concept relations
  - may publish repo-scoped packets, provenance snapshots, and publication metadata
  - runtime correlation ids remain non-authoritative and optional
- protected repo-stream envelopes
  - may include runtime signing identity and verification metadata
  - are not semantic dependencies for interpreting the payload itself

This boundary keeps cold clones intelligible even when no shared runtime database is present.

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

### 9.1 Keep room for authenticated reads and introspection later

Read-only queries remain unauthenticated by default in the initial model, but the protocol shape
should remain compatible with optional authenticated reads later.

That leaves room for:

- personalized views
- richer audit access control
- remote or shared deployment modes

Separately, PRISM should still offer non-authoritative introspection surfaces for humans and agents,
for example:

- which principal a credential authenticates as
- which daemon or runtime instance is being used
- which worktree or repo context is active
- which active leases are currently held by that principal

These are debugging and ergonomics features, not authority mechanisms.

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

This also reinforces the three-plane persistence split:

- repo truth plane
  - published plans, concepts, memory, and other repo-owned durable state
- shared runtime plane
  - principal registry, credential verifiers, lease state, and coordination continuity
- hot in-process plane
  - active runtime admission, live work execution, and transient request handling

Identity authority belongs in the shared runtime plane, not in the hot in-process engine.

---

## 11. Non-goals

This design note does not attempt to specify:

- the exact UI/UX for humans authenticating into PRISM
- the final on-disk schema for principal profile storage
- the final dashboard or MCP query surfaces for identity exploration
- a complete ACL or authorization model beyond mutation authorship and provenance

The initial capability layer on credentials is intentionally minimal. Richer authorization policy
can be layered later once the principal and credential substrate is stable.

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
- Credentials carry a small capability set rather than implying unrestricted authority.
- Credential rotation should be zero-downtime:
  - mint a new credential through an authenticated mutation
  - revoke the old credential once the replacement is live
- Credentials are revoked explicitly through a revocation mutation.
- Revoking a credential does not immediately kill active leases, because leases belong to the
  principal, not to one specific credential.
- A revoked credential simply loses the ability to authenticate future mutations, so any affected
  lease will naturally become stale and later expire if no other valid credential refreshes it.
- Capabilities are checked at mutation boundaries and should be coarse, not fully general, in v1.

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

- Lease renewal policy should support two modes:
  - `strict`
    - default mode
    - only explicit authenticated mutations renew leases
  - `assisted`
    - optional local trust policy
    - allows watcher-triggered authenticated `heartbeat_lease` renewal under the bounded guardrails
      below
- Default lease timings:
  - `stale` after 30 minutes with no authenticated mutation touching that specific lease
  - `expired` after 2 hours
- Leases attach to both tasks and claims:
  - task lease = execution ownership of a workflow node
  - claim lease = active lock/ownership over a claimed coordination object
- Lease refresh is narrow:
  - one mutation refreshes only the specific task or claim lease it touches
  - activity on Task A must not keep a stale lease on Task B alive
- A lightweight authenticated `heartbeat_lease` mutation should exist for long silent work.
- PRISM should not require agents to infer heartbeat timing from wall-clock intuition alone.
- When a task-scoped read surface sees that a lease refresh is due, it may return an explicit
  immediate next action instructing the agent to call `prism_mutate` with action `heartbeat_lease`
  before continuing other work.
- Those result-payload instructions must be conditional and server-authored; they are hints that
  trigger an authenticated mutation, not a read-path lease refresh.
- Assisted mode is an explicit local trust policy, not an identity proof:
  - it is opt-in and off by default
  - it is allowed only for one-principal, one-lease worktrees
  - it is bounded by time since the last explicit authenticated mutation from that principal
  - it should record renewal provenance such as `explicit` versus `watcher_auto`
- `prism://instructions` should explicitly teach agents that these heartbeat prompts may appear and
  that, when present, they should satisfy them before continuing normal task work.

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
  - `principal_authority_id`
  - `principal_id`
  - principal kind
- Mutation events should snapshot:
  - principal kind
  - human-readable name
- Mutation events should also snapshot declared work context:
  - `work_id`
  - work kind
  - work title
  - optional `parent_work_id`
  - optional `coordination_task_id`
  - optional plan snapshot fields when the work serves a published plan
- Mutation events should also record non-authoritative execution context separately from actor
  identity, including instance/worktree/correlation information where available.
- Runtime correlation ids remain diagnostics only; repo-published events must not depend on
  runtime-only objects for semantic interpretation.
- Additional actor snapshot fields may be added later where audit stability materially benefits, but
  `kind` and `name` are the initial minimum.

### 12.6 Principal profile mutability

- Immutable principal state:
  - `principal_authority_id`
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
- The bootstrap trust root is an explicitly trusted local CLI flow that mints the initial human
  owner principal for a machine-local daemon.
- After bootstrap, further principal or credential issuance is authenticated mutation only.

### 12.8 Imported external identities

- Observational identities such as git authors remain distinct from authenticated principals.
- The relationship is a soft link, not an auth claim.
- Principals may attach linked emails or handles for grouping and UX purposes.
- A bare external identity must never be treated as proof of authenticated PRISM authorship.

### 12.9 Query and projection surfaces

- Day-one first-class views should expose at least:
  - active lease holder by `principal_id` and `name`
  - assigned principal by `principal_id` and `name`
  - current lease renewal policy when it is not `strict`
- These must appear in the core task and plan read surfaces such as task briefs and plan summaries.
- Eager projections should include:
  - active lease holder
  - assigned principal
- Full authorship trails may remain event-derived on demand initially.

### 12.10 Backward compatibility and migration

- The cutover is intentionally hard.
- `prism_session` is removed rather than supported through a long compatibility window.
- All mutations require credentials from day one of the cutover.
- All authoritative mutations must also have declared work from day one of the cutover.
- The only mutation allowed to bootstrap without existing work context is `declare_work`.
- Existing coarse actor history should be migrated through synthetic fallback principals, for example:
  - `legacy_agent_fallback`
  - `legacy_human_fallback`
- Existing implicit session-task behavior should be removed rather than preserved as a durable
  provenance pattern.
- Missing-credential mutations should fail loudly and descriptively.
- Missing-work mutations should fail loudly and descriptively, with instructions to declare work
  before retrying.
- Instructions and surrounding tooling should clearly state how agents obtain and pass mutation
  credentials.
- The same instructions should also tell agents:
  - to adopt identity, inspect as needed, declare work or bind to an existing coordination task,
    and only then mutate
  - that task-scoped reads may occasionally return a server-authored instruction to send
    `heartbeat_lease` immediately, and that this should take precedence over other next steps
- Repo-published history may retain runtime correlation handles for diagnostics, but no
  repo-published semantic reference may require the shared runtime database to resolve.

### 12.11 Remaining future policy space

The core design questions are now settled. Future work may still refine:

- additional actor and execution-context snapshot fields beyond the initial minimum
- optional `heartbeat_lease` ergonomics beyond the initial `strict` and bounded `assisted` modes
- stronger human credential storage such as OS keychain integration
- richer query and UI surfaces for lineage and provenance exploration

---

## 13. Recommended implementation order

When implementation starts, the safest sequence is:

1. Define the principal core types, credential registry, and event metadata changes.
2. Define principal authority/namespace handling and bootstrap trust-root semantics.
3. Add mutation-envelope authentication and coarse credential capabilities for authoritative writes.
4. Record canonical provenance plus non-authoritative execution context in mutation events.
5. Introduce explicit lease lifecycle and stale/reclaim semantics.
6. Add strict heartbeat guidance first, then bounded assisted watcher renewal as an explicit opt-in
   trust policy.
7. Add projections and query surfaces for authorship, identity history, principal introspection,
   and active lease policy state.
8. Add CLI principal mint/show/lineage flows and child-principal minting for multi-agent orchestration.

This order minimizes the risk of building ad hoc ownership semantics before the identity substrate is
stable.
