# PRISM Dashboard Implementation Spec

Archived historical design. The active operator-console docs are
[PRISM_OPERATOR_CONSOLE_BACKEND_SPEC.md](../PRISM_OPERATOR_CONSOLE_BACKEND_SPEC.md) and
[PRISM_SSR_OPERATOR_CONSOLE.md](../PRISM_SSR_OPERATOR_CONSOLE.md).

This document remains useful as historical context for the earlier query-log dashboard direction,
but it is no longer the implementation contract for the current UI overhaul.

## Purpose

Build a first-class PRISM dashboard as a live single-page application where users can see what the
server is doing right now:

- active and recent queries
- active and recent mutations
- latency, trace phases, diagnostics, and failures
- daemon and bridge runtime state
- task, coordination, and curator activity
- validation and trust signals as the surface matures

The dashboard should feel like an operator console, not a static admin page.

## Product Goals

- Show current PRISM activity across multiple agents sharing one MCP daemon.
- Make query and mutation cost, failures, and behavior legible.
- Make runtime state, refresh behavior, and contention visible without shell commands.
- Keep the normal agent-facing MCP tool surface lean.
- Provide a stable public observability surface for UI consumers.
- Support light mode and dark mode, defaulting to system preference.
- Feel live and responsive from the start through SSE, not polling.

## Non-Goals

- Do not inflate normal MCP tool discovery with a large set of observability tools.
- Do not make the first version depend on periodic polling.
- Do not build a generic multi-page admin backend before the single-page dashboard proves the
  information architecture.
- Do not expose raw internal implementation details directly as the long-term UI contract when a
  narrower read model will do.

## Product Decision Summary

### 1. The dashboard is a first-class PRISM product surface

Observability is no longer only an internal-developer concern. PRISM already has enough runtime and
query instrumentation for this to become a supported user-facing capability.

### 2. Keep the normal MCP tool surface lean

The current query log and runtime introspection methods are gated behind the
`internal_developer` feature flag so ordinary MCP tool catalogs stay small. That remains the right
default for agent-facing tools.

We should not solve the dashboard by simply making all raw observability methods part of the normal
public MCP tool surface.

### 3. Add a public observability API for the dashboard

The dashboard should consume a supported HTTP read surface served by `prism-mcp` itself:

- JSON snapshot endpoints for initial load and recovery
- SSE for live updates

This keeps the UI contract stable and avoids coupling the browser directly to raw MCP tool calls and
internal log layouts.

### 4. Serve the dashboard from `prism-mcp`

The dashboard should be same-origin with the daemon:

- no separate devops service
- no CORS complexity for normal usage
- one local product entrypoint

The existing Axum server that already serves `/mcp` and `/healthz` should also serve dashboard
assets and dashboard HTTP endpoints.

### 5. SSE is required from day one

Polling is not acceptable because multiple agents can drive frequent updates through the same daemon.
The UI must feel live and snappy under shared activity.

## Existing PRISM Surface We Can Build On

PRISM already exposes most of the semantic inputs needed for a dashboard:

- query logs, slow queries, and per-query traces
- runtime status, runtime logs, and runtime timeline
- task journals and task replay
- changed files, changed symbols, recent patches, and semantic diffs
- coordination inbox, blockers, claims, conflicts, pending reviews, and task risk
- curator jobs and proposal state
- validation feedback recording and the broader validation scorecard direction described in
  `docs/VALIDATION.md`

The dashboard effort is therefore primarily about:

- packaging this into a stable UI-oriented contract
- adding missing live and active-operation surfaces
- making mutation activity first-class

## Gaps To Close Before The Dashboard Is Credible

### Active operations are missing

The current query log records completed queries. The dashboard also needs active in-flight work:

- query started
- query phase updates
- query finished
- mutation started
- mutation finished

### Mutations need a first-class live timeline

Mutations are explicit and auditable, but there is no equivalent of the query log as a dedicated
live mutation feed with duration, outcome, violations, and related task metadata.

### The browser needs stable read models

The raw internal query/runtime methods are useful, but the dashboard should read from summary and
detail views shaped for UI workflows.

### Trust and validation should become visible

The dashboard should grow into the machine-readable scorecard described in `docs/VALIDATION.md`,
but this can land after the core live operations surface.

## Backend Architecture

## 1. Add a dedicated dashboard module set to `prism-mcp`

Keep `lib.rs` and `main.rs` as facades only. Add focused modules for the dashboard surface instead
of growing the existing crate root.

Recommended module split:

- `dashboard_router.rs`
- `dashboard_assets.rs`
- `dashboard_events.rs`
- `dashboard_snapshots.rs`
- `dashboard_operations.rs`
- `dashboard_runtime.rs`
- `dashboard_coordination.rs`
- `dashboard_validation.rs`
- `dashboard_types.rs`

If this grows further, move these into `src/dashboard/` with a small facade module.

## 2. Introduce an event bus plus replay buffer

Add an in-process dashboard event bus that can broadcast updates from query execution, mutation
execution, runtime refresh, curator activity, and coordination changes.

Requirements:

- bounded in-memory replay buffer
- monotonic event ids
- support for `Last-Event-ID`
- fan-out to multiple SSE subscribers
- separation between event production and HTTP transport

## 3. Track active operations explicitly

Introduce an `ActiveOperationRegistry` that stores live state for in-flight queries and mutations.

Each operation should at minimum capture:

- `id`
- `kind` as `query` or `mutation`
- `name` or operation label
- `startedAt`
- `taskId`
- `sessionId`
- `agentId` if available
- current status
- current phase
- touched files or anchors when known

For completed operations, persist a compact record into bounded stores:

- `QueryLogStore` already exists and should remain the source for completed queries
- add a parallel `MutationLogStore`

## 4. Add a first-class mutation log

Mutation records should be shaped similarly to query records, but with mutation-specific fields:

- action family such as `session`, `outcome`, `memory`, `validation_feedback`, `coordination`,
  `claim`, `artifact`, `curator`
- duration
- success or failure
- policy violations
- resulting ids such as task, event, memory, claim, or artifact ids
- related anchors
- related task id

This should become a first-class dashboard feed and detail surface.

## 5. Add dashboard-oriented snapshot endpoints

The browser should not need to reconstruct a landing view from many low-level calls.

Recommended endpoints:

- `GET /dashboard/api/summary`
- `GET /dashboard/api/operations`
- `GET /dashboard/api/operations/:id`
- `GET /dashboard/api/runtime`
- `GET /dashboard/api/tasks/current`
- `GET /dashboard/api/coordination`
- `GET /dashboard/api/changes`
- `GET /dashboard/api/curator`
- `GET /dashboard/api/validation`

These are read models, not new mutation APIs.

## 6. Add an SSE stream

Primary live endpoint:

- `GET /dashboard/events`

Event types should include at least:

- `query.started`
- `query.phase`
- `query.finished`
- `mutation.started`
- `mutation.finished`
- `runtime.refreshed`
- `runtime.processes.changed`
- `task.updated`
- `coordination.updated`
- `curator.updated`
- `validation.recorded`

Events should be deltas, not full snapshots. The client should bootstrap with JSON snapshots and
then stay current by applying SSE updates to a local store.

## 7. Do not depend on the normal MCP tool catalog for the UI

The dashboard API is public and supported, but it is a distinct surface from normal agent-facing
MCP tool discovery.

Recommended policy:

- keep raw query/runtime observability methods gated for now in the MCP query surface
- build the dashboard against the new HTTP snapshot and SSE contracts
- only expose a subset through public `prism_query` later if there is a strong product reason

This avoids bloating ordinary agent sessions while still making observability first-class.

## API Shape

## Summary view

`GET /dashboard/api/summary`

Should include:

- workspace root
- daemon health
- daemon count
- bridge count
- daemon RSS
- cache size
- log size
- current task
- active query count
- active mutation count
- last workspace refresh
- recent error count

## Operations view

`GET /dashboard/api/operations`

Should support:

- kind filter
- status filter
- task filter
- minimum duration filter
- limit
- cursor or before-id pagination if needed

Should combine active and recent records into one UI-oriented shape.

## Operation detail view

`GET /dashboard/api/operations/:id`

For queries:

- query text or summary
- result summary
- diagnostics
- phase waterfall
- touched files
- touched anchors
- duration
- task and session metadata

For mutations:

- action payload summary
- result summary
- policy violations
- resulting ids
- touched anchors
- duration
- task and session metadata

## Runtime view

`GET /dashboard/api/runtime`

Should include:

- health
- daemon and bridge process list
- RSS and elapsed data
- cache and log file sizes
- recent runtime timeline
- recent warnings and errors

## Coordination and change views

The dashboard should make shared activity legible:

- current task journal
- blockers
- pending reviews
- active claims and conflicts
- changed files and recent semantic patches
- recent curator jobs and proposal counts

## Validation view

Not required for the first visual shell, but this should be the landing place for:

- validation feedback counts and trends
- query runtime trust metrics
- lineage and projection scorecards
- machine-readable validation outputs from the broader validation work

## Frontend Architecture

## 1. Add a dedicated frontend app

Create a dedicated app under `www/dashboard`.

Recommended stack:

- React
- TypeScript
- Vite

Do not fold the dashboard into the existing marketing app under `www/landing-page`.

## 2. Use a live local store fed by snapshots plus SSE

Recommended frontend data flow:

- fetch bootstrap snapshots on load
- open SSE stream
- apply deltas to a central client store
- reconnect with `Last-Event-ID` after disconnects

The dashboard should remain useful under short disconnects and daemon restarts.

## 3. Use D3 selectively

React should own app structure and state.

Use D3 only where it materially improves visualization quality, such as:

- query latency sparklines
- latency histograms
- phase trace waterfall
- compact runtime activity timelines

Avoid turning simple charts into a D3-first app.

## 4. Suggested UI layout

Single-page dashboard:

- top status strip
- active operations rail
- recent operations table
- detail drawer or right-side inspection pane
- runtime panel
- coordination and curator panel
- validation and trust panel

This should feel like one expressive console rather than a collection of disconnected pages.

## Theming

The dashboard must support:

- light mode
- dark mode
- system default mode

Requirements:

- `prefers-color-scheme` is the default
- explicit user override with `light`, `dark`, and `system`
- persisted preference in local storage
- CSS variables or design tokens from the start
- chart palettes tuned separately for light and dark themes

Theme support should not be deferred because charts, contrast, and live status colors will be
harder to retrofit later.

## UX Principles

- The dashboard should answer “what is PRISM doing right now?” in under a second.
- Active work should always be visually distinct from historical work.
- Errors, policy violations, and truncation should be visible without opening a trace.
- Deep detail should be one click away through a detail pane, not hidden behind route churn.
- Shared multi-agent activity should be obvious.
- The UI should privilege explanation over decoration.

## Delivery Plan

## Phase 1: Backend live infrastructure

- add dashboard module boundaries in `prism-mcp`
- add event bus and replay buffer
- add active operation registry
- add mutation log store
- emit query and mutation lifecycle events
- expose JSON snapshot endpoints
- expose SSE endpoint

Done when:

- the backend can stream live query and mutation updates
- a reconnecting client can recover via `Last-Event-ID`
- mutation activity is as inspectable as query activity

## Phase 2: Frontend shell

- scaffold `www/dashboard`
- implement theme system with system default
- build bootstrap plus SSE client store
- build top status strip
- build recent and active operations views
- build operation detail drawer

Done when:

- the SPA shows live operation activity with no polling
- light, dark, and system theme modes all work

## Phase 3: Runtime and shared-work panels

- runtime process panel
- refresh timeline
- current task journal
- coordination summary
- changed files and patch activity
- curator activity panel

Done when:

- the dashboard is useful for real multi-agent shared-server debugging

## Phase 4: Trust and validation surfaces

- validation feedback stream
- scorecard summary views
- trust trends and quality panels

Done when:

- the dashboard begins to reflect PRISM as a measured world model, not only a runtime console

## Implementation Notes

- Prefer same-origin static asset serving from `prism-mcp`.
- Keep dashboard read models additive and stable even if internal stores change.
- Preserve modular boundaries; do not let dashboard logic collapse back into `lib.rs`.
- Reuse existing query/runtime/coordination domain views where they are already correct, but add
  dedicated dashboard adapters instead of overloading browser code with raw internal shapes.
- The dashboard should remain useful even when `prism_query` raw observability methods stay gated
  for non-internal agent sessions.

## Open Questions

- Whether dashboard access should eventually have its own capability gate distinct from
  `internal_developer`
- Whether completed operation history should remain in-memory only or gain workspace persistence
- Whether validation scorecards should be computed on demand or materialized into projections
- Whether the dashboard should eventually expose lightweight operator actions such as filtering,
  trace export, or curator proposal review
