# PRISM Operator Console Backend Spec

Status: proposed target design
Audience: PRISM MCP, coordination, auth, and frontend maintainers
Scope: backend contract for the first operator console UI

---

## 1. Summary

PRISM should expose a versioned HTTP backend for a bundled React operator console served directly
from the local daemon.

The V1 backend is intentionally simple:

- a React SPA is bundled into the Rust binaries and served same-origin by `prism-mcp`
- the UI reads from polling-friendly REST endpoints under `/api/v1/**`
- the UI mutates state through one `POST /api/v1/mutate` endpoint
- UI-triggered mutations must execute through the exact same host mutation path as MCP-triggered
  mutations, so audit behavior, policy enforcement, Git execution, and shared-coordination
  publication stay identical
- SSE, WebSockets, and the legacy dashboard HTTP surface are removed from the target design

This backend is the operator control plane for plans, tasks, leases, runtimes, and human
intervention. It is not a second coordination system.

---

## 2. Product Scope

V1 supports two primary pages plus a task detail panel:

- strategic plans portfolio plus dependency graph
- fleet timeline and runtime utilization
- task detail side panel for read context and human interventions

Out of scope for V1:

- concepts and semantic memory browsing
- shell flight recorder
- browser-push transport such as SSE or WebSockets
- remote multi-user auth flows

---

## 3. Backend Principles

### 3.1 Same-origin bundled product surface

The daemon should serve:

- the embedded SPA shell and static assets
- the operator-console REST API

There is no separate frontend server or UI-only backend process.

### 3.2 Polling over streaming

The UI should poll the REST API every two seconds.

The backend contract should optimize for:

- stable snapshot reads
- idempotent reloads
- cheap recovery after laptop sleep or daemon restarts

The backend should not expose SSE or WebSocket contracts for this UI generation.

### 3.3 HTTP reads separate from MCP transport

Browser reads must not go through JSON-RPC MCP.

The UI REST layer should reuse the same internal read-model code that MCP-backed read surfaces use,
but it should not emit MCP call-log noise for ordinary browser polling.

### 3.4 Human writes are normal PRISM mutations

A human operator is just another principal in the coordination fabric.

When the UI requests a mutation, the daemon must:

- resolve the active local operator principal
- construct the same mutation context an MCP-triggered mutation would use
- execute the same host mutation path
- emit the same mutation audit records and downstream coordination publications

The only intended difference is the transport entrypoint, not the mutation semantics.

### 3.5 Pessimistic UI semantics

The backend remains authoritative.

The UI may show a local pending state while a mutation is in flight, but the backend contract is
snapshot-based:

- mutation request accepted
- next polling cycle observes authoritative state
- UI resolves from authoritative state only

The backend does not need optimistic mutation deltas or browser-push confirmation channels.

---

## 4. Authority Boundaries

The operator console backend must preserve PRISM's existing authority planes.

### 4.1 Shared coordination refs

Shared coordination refs remain authoritative for:

- plans
- tasks
- claims and leases
- handoffs and reservations
- runtime descriptors
- shared coordination indexes

### 4.2 Repo-published `.prism/state`

Tracked `.prism/state/**` remains authoritative for repo-published semantic state such as:

- concepts
- contracts
- repo memories
- published manifests and indexes

The operator console backend should not invent parallel repo truth.

### 4.3 Local runtime state

Local runtime state remains authoritative for:

- hot diagnostics
- local execution overlays
- draft work
- detailed local journals
- runtime-serving accelerators

### 4.4 UI read-model role

Every `/api/v1/**` response is a derived serving view over those authority planes.

The backend should expose narrow, stable UI-oriented read models rather than raw internal storage
layouts.

---

## 5. API Surface

The backend contract for V1 is the following versioned HTTP surface.

### 5.1 Read endpoints

Required endpoints:

- `GET /api/v1/session`
- `GET /api/v1/plans`
- `GET /api/v1/plans/:plan_id/graph`
- `GET /api/v1/tasks/:task_id`
- `GET /api/v1/fleet`

Optional additive endpoints may follow later, but V1 should center on those five.

### 5.2 Mutation endpoint

Required write endpoint:

- `POST /api/v1/mutate`

There should not be one HTTP endpoint per mutation action. The HTTP API mirrors the MCP mutation
model intentionally.

### 5.3 Versioning

The UI backend must be explicitly versioned under `/api/v1`.

The previous unversioned `/api/overview`, `/api/plans`, and `/api/graph` routes are legacy and
must be retired once the new surface is in place.

---

## 6. Read Models

### 6.1 `GET /api/v1/session`

Purpose:

- bootstrap UI identity and global shell context

Must include:

- workspace root
- daemon health summary
- active local operator principal id
- active local operator display label when available
- current runtime id
- current worktree id
- current branch ref
- checked-out commit
- polling recommendation in milliseconds
- feature flags relevant to the operator console

This payload is the UI shell bootstrap and header source.

### 6.2 `GET /api/v1/plans`

Purpose:

- strategic portfolio list for the left panel

Must include, for each plan:

- plan id
- title
- goal or summary
- status
- priority or scheduling score inputs required for sorting
- completion percentage
- total node count
- completed node count
- active node count
- blocked node count
- leased runtime or principal summaries when present
- recency markers needed for secondary sorting

Supports query parameters:

- `status`
- `search`
- `sort`
- `runtime_id`
- `principal_id`

Default behavior:

- only active plans
- sorted by execution priority

### 6.3 `GET /api/v1/plans/:plan_id/graph`

Purpose:

- dependency graph for the selected plan

Must include:

- selected plan metadata
- graph nodes
- graph edges
- node status
- node priority
- current claim/lease holder summary
- runtime and principal labels for active nodes

The response must already be shaped for ReactFlow-style rendering. The browser should not rebuild
raw coordination graphs on its own.

### 6.4 `GET /api/v1/tasks/:task_id`

Purpose:

- task detail panel

Must include read-only sections:

- task metadata
- claim history
- outcomes
- recent commits linked to the task
- blockers and dependency state
- current assignee or lease holder

Must include editable fields and action affordances metadata:

- title
- description or goal
- priority
- validation requirements
- status override options
- revoke-lease availability
- reassign availability
- archive or remove availability

The backend should return the current authoritative editable values and enough metadata for the UI
to render the mutation affordances without separate schema fetches.

### 6.5 `GET /api/v1/fleet`

Purpose:

- runtime and agent utilization timeline

Must include:

- runtime lanes
- operator or agent labels
- active and recent task lease bars
- start time
- end time or active-to-now marker
- task id and title
- principal id
- runtime id
- stuckness signals such as unusually old active leases

This read model should be shaped for direct Gantt-style rendering rather than raw event replay.

---

## 7. Mutation Contract

### 7.1 Endpoint shape

`POST /api/v1/mutate` accepts a UI mutation envelope that translates directly into a standard
PRISM mutation action.

The UI payload must carry:

- mutation action family
- action payload
- optional task or plan context
- optional optimistic client request id for UI reconciliation

The backend may wrap that payload with server-derived operator identity and transport metadata, but
it must not fork into a separate human-only mutation implementation.

### 7.2 Exact execution path requirement

The HTTP mutation handler must route into the same host mutation path used by MCP-triggered
mutations.

That means the UI path must reuse:

- the same auth and principal resolution rules
- the same `MutateAction` translation layer
- the same violation and policy checks
- the same Git execution gates
- the same coordination persistence and shared-ref publication path
- the same audit logging

The transport adapter should be thin. Business logic must not be reimplemented in the UI router.

### 7.3 Mutation families required in V1

The backend must support at least these operator actions through `POST /api/v1/mutate`:

- task metadata update
- task priority update
- task validation requirement update
- task status override
- claim revoke or release
- continuation reservation or reassignment
- plan archive

Each of those should map to existing coordination mutation families rather than bespoke UI-only
commands.

### 7.4 Response semantics

The mutation endpoint should return:

- success or failure
- accepted mutation id or resulting coordination event ids when available
- authoritative task or plan ids affected
- transport-safe error details

The response should not attempt to return a full fresh UI snapshot. The next poll remains the
authoritative state refresh.

---

## 8. Identity Binding

The operator console is served locally by the daemon, so V1 uses local profile binding.

The daemon should resolve the active local PRISM profile and expose the resulting operator identity
through `GET /api/v1/session`.

The mutation endpoint should sign or authorize writes with that same active local principal.

V1 does not introduce a browser login screen or token dance.

The UI must always show which principal the operator is acting as.

---

## 9. UI Asset Delivery

The frontend build must be bundled into the Rust binaries.

Required properties:

- no dependency on `www/dashboard/dist` existing at runtime
- assets are available from the compiled daemon binary
- same-origin serving for the SPA shell and static assets

Recommended shape:

- generated embedded asset module or `include_dir` style asset packaging
- one module owning embedded asset lookup and shell fallback
- unbuilt development fallback may remain for local dev, but production release binaries must serve
  embedded assets

The backend contract assumes that bundled delivery is the default release behavior.

---

## 10. Legacy Surface Removal

The following surfaces are legacy and must be retired as part of the rollout:

- `/dashboard/api/**`
- `/dashboard/events`
- SSE-oriented frontend state management
- unversioned operator-console API routes once `/api/v1/**` is live
- the old dashboard product framing where observability is the primary UI surface

This replacement is intentional:

- V1 is an operator console for plans, tasks, and runtimes
- not a live query-log dashboard

---

## 11. Module Boundaries

`main.rs` and `lib.rs` remain facades only.

The backend implementation should move toward a dedicated operator-console module set under
`crates/prism-mcp/src/ui/` or equivalent focused files.

Recommended ownership split:

- router and route registration
- embedded assets and shell serving
- session bootstrap read model
- plans read model
- graph read model
- task detail read model
- fleet timeline read model
- HTTP mutation adapter
- shared UI DTOs

Mutation execution logic should remain owned by the existing host mutation subsystem, not by the UI
module tree.

---

## 12. Validation Requirements

The backend implementation is complete only when all of the following are true:

- the daemon serves bundled UI assets from release binaries
- the UI bootstrap and read models are available under `/api/v1/**`
- the backend supports polling-only operation with no SSE dependence
- `POST /api/v1/mutate` reaches the exact MCP mutation execution path
- human-triggered mutations appear in the same audit and coordination flows as MCP-triggered
  mutations
- the active local principal is visible in the session payload
- legacy dashboard and SSE surfaces are removed or clearly retired

---

## 13. Implementation Order

The intended order is:

1. lock this backend contract
2. bundle UI assets into the daemon binary
3. add the `/api/v1` router foundation
4. implement the strategic plans and graph read models
5. implement the task detail read model
6. implement the fleet read model
7. wire `POST /api/v1/mutate` through the existing host mutation path
8. expose local principal identity in the session bootstrap
9. remove legacy dashboard and SSE backend surfaces
10. validate end to end with the bundled UI

This order keeps the operator-console product surface coherent while preserving the mutation and
authority guarantees that already exist in PRISM.
