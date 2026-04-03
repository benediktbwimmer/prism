# Build a first-class Prism UI control plane around the existing dashboard, with an overview landing surface, a plans view, an architecture graph explorer, shared cross-links, and final concept curation so the repo-native architecture reflects the new human-facing product.

> Generated from repo-scoped PRISM plan state.
> Return to the plan index in `../index.md` or the repo entrypoint in `../../../PRISM.md`.

## Projection Metadata

- Projection class: `published`
- Authority planes: `published_repo`
- Projection version: `1`
- Source head: `sha256:4e617b3c94f0997537b09ebdacbe4f2af544aca1fd662d69e978fcfe3a2d1625`
- Source logical timestamp: `unknown`
- Source snapshot: `12 nodes, 14 edges, 0 overlays`

## Overview

- Plan id: `plan:01kmzvjdhb874nxd0v44d2hdmt`
- Status: `active`
- Kind: `task_execution`
- Scope: `repo`
- Revision: `0`
- Nodes: `12`
- Edges: `14`

## Goal

Build a first-class Prism UI control plane around the existing dashboard, with an overview landing surface, a plans view, an architecture graph explorer, shared cross-links, and final concept curation so the repo-native architecture reflects the new human-facing product.

## Git Execution Policy

- Start mode: `off`
- Completion mode: `off`
- Target branch: ``
- Require task branch: `false`
- Max commits behind target: `0`

## Source of Truth

- Snapshot manifest: `.prism/state/manifest.json`
- Snapshot plan shard: `.prism/state/plans/plan:01kmzvjdhb874nxd0v44d2hdmt.json`
- Legacy migration log path: none; tracked snapshot shards are the only current repo authority

## Root Nodes

- `coord-task:01kmzvjdhbybkjn70sasjwahwd`

## Nodes

### Milestone 7: Add cross-view linking, overlays, and shared architectural context

- Node id: `coord-task:01kmzvjdhb4depcxapk8a14naz`
- Kind: `edit`
- Status: `ready`
- Summary: Connect Overview, Dashboard, Plans, and Graph so each surface can highlight the same runtime work, affected concepts, and validation or risk context, with overlay semantics that stay coherent as the graph grows toward contract-aware architectural navigation.
- Priority: `7`

#### Bindings

- Concept: `concept://coordination_and_plan_runtime`
- Concept: `concept://dashboard_surface`
- Concept: `concept://prism_architecture`

#### Acceptance

- Clicking a plan can highlight touched concepts, and clicking a concept can reveal relevant active plans or live work [any]
- The first overlay set includes plan touchpoints plus at least one of health or risk or active-work state, with clear toggle semantics [any]
- Dashboard, Plans, and Graph feel like one product surface with shared context and deep links instead of unrelated tools [any]
- Shared cross-view state is compatible with later contract-aware overlays and deeper architectural evidence panes [any]

#### Tags

- `integration`
- `navigation`
- `overlays`
- `shared-context`

### Milestone 10: Dogfood, validate, and ship the Prism UI control plane safely

- Node id: `coord-task:01kmzvjdhb531p74z3efn6smg3`
- Kind: `validate`
- Status: `proposed`
- Summary: Validate the new shell and pages end-to-end, rebuild and restart prism-mcp, dogfood the control plane on real work, and close obvious trust gaps after the graph, freshness, and structured plan-control slices are in place.
- Priority: `10`

#### Bindings

- Concept: `concept://dashboard_surface`
- Concept: `concept://validation_and_dogfooding`

#### Acceptance

- Release build, MCP restart, status, and health checks confirm the live daemon is serving the new UI surface correctly [any]
- Manual dogfooding covers landing on Overview, drilling into Dashboard, inspecting Plans, and using the Graph explorer on a real repo flow [any]
- Any missing trust signals, stale states, or broken deep links found during dogfooding are fixed or explicitly tracked before rollout [any]

#### Validation Refs

- `ui:control-plane-dogfood`
- `ui:release-build-restart`

#### Tags

- `dogfood`
- `release`
- `trust`
- `validation`

### Milestone 5: Define the concept-graph projection, zoom model, and overlay contract

- Node id: `coord-task:01kmzvjdhb96zxv7pnt1t06fj8`
- Kind: `decide`
- Status: `completed`
- Summary: Decide the first-cut architecture explorer contract around subsystem-level semantic zoom, typed relations, and bounded overlays so the graph page is inspectable instead of noisy.
- Priority: `5`

#### Bindings

- Concept: `concept://dashboard_surface`
- Concept: `concept://prism_architecture`

#### Acceptance

- The graph starts at subsystem or concept level instead of trying to render the whole repo graph at once [any]
- The first release contract covers semantic zoom, neighborhood expansion, evidence inspection, and typed relation rendering [any]
- Overlay priority is explicit: concept relations first, then plan touchpoints, health or risk, and active work; freeform graph editing stays out of scope [any]

#### Tags

- `concepts`
- `design`
- `graph`

### Milestone 6: Implement a contract-ready graph explorer baseline on typed read models

- Node id: `coord-task:01kmzvjdhbgcw86tj5tjs7cd1b`
- Kind: `edit`
- Status: `completed`
- Summary: Ship the Graph page on top of explicit UI-facing graph read models and query adapters, with concept-first neighborhood loading, typed node and edge semantics, deep-linkable focus state, evidence inspection, and a data contract that is ready to incorporate contracts later without redesign.
- Priority: `6`

#### Bindings

- Concept: `concept://dashboard_surface`
- Concept: `concept://prism_architecture`

#### Acceptance

- The Graph page is backed by explicit UI-facing graph read models or query adapters rather than raw MCP payload dumping [any]
- Users can start from a clean architectural graph, expand concept neighborhoods, inspect evidence, and follow links down toward code or supporting context [any]
- The first release graph contract uses typed node and edge semantics plus deep-linkable focus state instead of an untyped whole-graph dump [any]
- The graph data model is contract-ready even if the first UI only renders concept-centered neighborhoods [any]

#### Validation Refs

- `ui:graph-explorer-baseline`

#### Tags

- `contracts`
- `explorer`
- `frontend`
- `graph`
- `read-models`

### Milestone 2: Introduce a routed frontend shell and shared client data layer

- Node id: `coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`
- Kind: `edit`
- Status: `completed`
- Summary: Completed the routed frontend shell and shared client data layer: the monolithic dashboard app is split into an app frame, route model, reusable dashboard bootstrap/SSE hook, dedicated dashboard page, overview page, and placeholder plans/graph pages without discarding the current dashboard surface.
- Priority: `2`

#### Bindings

- Concept: `concept://dashboard_surface`

#### Acceptance

- The frontend has an app shell and navigation that can host Overview, Dashboard, Plans, and Graph as coherent sibling surfaces [any]
- Shared fetching, bootstrap, and SSE connection state live in reusable client infrastructure instead of being duplicated per page [any]
- The current dashboard UI survives as a dedicated page inside the new shell rather than being discarded as throwaway code [any]

#### Tags

- `frontend`
- `navigation`
- `shell`

### Milestone 4: Build a plans view on native coordination and plan-runtime data

- Node id: `coord-task:01kmzvjdhbsgjm53m3ws4r2ax8`
- Kind: `edit`
- Status: `completed`
- Summary: Replacing the plans placeholder with a native plan-runtime page backed by explicit UI read models for plan lists, selected plan graph summaries, blockers, handoffs, and next-node recommendations.
- Priority: `4`

#### Bindings

- Concept: `concept://coordination_and_plan_runtime`
- Concept: `concept://query_coordination_and_plan_views`

#### Acceptance

- The Plans page can list plans, inspect one plan graph, and show node status, blockers, next actions, and recent execution evidence [any]
- Human intervention stays structured in the first release: notes, blockers, acceptance, and status affordances come before any freeform graph editing [any]
- The page is backed by explicit UI read models or query adapters for plan summary, plan graph, execution overlay, and next-node recommendations [any]

#### Validation Refs

- `ui:plans-runtime-surface`

#### Tags

- `coordination`
- `human-control`
- `plans`

### Milestone 11: Refresh Prism concepts, relations, contracts, and generated docs for the new UI reality

- Node id: `coord-task:01kmzvjdhbtbmjrjr2txxn2y0j`
- Kind: `edit`
- Status: `proposed`
- Summary: Refresh the repo-native knowledge layer so Prism’s own architecture, contracts, and generated docs describe the product as a multi-surface human control plane rather than just a standalone dashboard.
- Priority: `11`

#### Bindings

- Concept: `concept://coordination_and_plan_runtime`
- Concept: `concept://dashboard_surface`
- Concept: `concept://prism_architecture`

#### Acceptance

- Existing dashboard concepts are updated or split so Overview, Plans, Graph, shared UI shell, and new control-plane ownership boundaries are represented accurately [any]
- New or updated concept relations and contracts capture how the UI surface connects architecture, intent, execution, evidence, and route or transport ownership [any]
- Repo-scoped concept, contract, memory, and generated doc updates land only after the implementation is real enough that future agents can rely on them [any]
- Generated artifacts such as `PRISM.md` and `docs/prism/*` reflect the new UI reality, including the contract catalog where relevant [any]

#### Validation Refs

- `concepts:prism-ui-reality`

#### Tags

- `closeout`
- `concepts`
- `contracts`
- `curation`
- `docs`

### Milestone 1: Generalize dashboard serving into a top-level Prism app shell

- Node id: `coord-task:01kmzvjdhbwhnhz1fgkrebe94p`
- Kind: `edit`
- Status: `completed`
- Summary: Completed the top-level Prism UI shell routing: prism-mcp now serves the shared SPA at `/`, `/dashboard`, `/plans`, and `/graph` through dedicated `ui_router` and `ui_assets` modules, while `/dashboard/api/*` and `/dashboard/events` remain stable dashboard-owned transport. Release build, daemon restart, and live HTTP checks passed.
- Priority: `1`

#### Bindings

- Concept: `concept://dashboard_routing_and_assets`
- Concept: `concept://dashboard_surface`

#### Acceptance

- HTTP routes serve a top-level Prism entrypoint plus stable subroutes for dashboard, plans, and graph without breaking existing `/dashboard` deep links [any]
- Existing dashboard bootstrap, summary, operations, and SSE behavior remain available behind the new shell instead of being replaced ad hoc [any]
- New shell-serving code stays modular, with dedicated router and asset ownership rather than growing facade files into mixed-purpose logic [any]

#### Tags

- `backend`
- `routing`
- `shell`

### Milestone 3: Build the overview page as the human entrypoint

- Node id: `coord-task:01kmzvjdhbwtrnjz0q7w13337m`
- Kind: `edit`
- Status: `completed`
- Summary: Completed the overview entrypoint slice: the landing page now uses a dedicated `/api/overview` read model with active-plan pressure, recent outcomes, pending handoffs, and concept-linked deep links into dashboard, plans, and graph focus states. Release build, daemon restart, and live HTTP checks passed.
- Priority: `3`

#### Bindings

- Concept: `concept://coordination_and_plan_runtime`
- Concept: `concept://dashboard_surface`

#### Acceptance

- Overview shows system health, active work, blocked plan signals, recent outcomes, and hot concepts strongly enough to orient a human on first load [any]
- Overview cards and summaries deep-link into dashboard operations, plan detail, and graph focus states instead of being dead summary widgets [any]
- Overview is independently shippable as the new landing page even if later surfaces are still maturing [any]

#### Tags

- `entrypoint`
- `overview`
- `product`

### Milestone 0: Lock the Prism UI information architecture, route map, and reuse boundaries

- Node id: `coord-task:01kmzvjdhbybkjn70sasjwahwd`
- Kind: `investigate`
- Status: `completed`
- Summary: Milestone 0 decisions locked: keep `/dashboard` as a preserved destination and make `/` the new Prism overview shell; add sibling routes `/plans` and `/graph`; keep existing `/dashboard/api/*` and `/dashboard/events` as the dashboard-owned transport; add a new top-level UI boundary in prism-mcp for shell and non-dashboard page routing rather than growing dashboard_router; split the frontend into an app shell plus page modules (Overview, Dashboard, Plans, Graph) and shared data modules for dashboard, plans, and graph reads. Freeform graph editing stays out of the first release.
- Priority: `0`

#### Bindings

- Concept: `concept://coordination_and_plan_runtime`
- Concept: `concept://dashboard_surface`

#### Acceptance

- The route plan explicitly covers `/`, `/dashboard`, `/plans`, and `/graph` with shared navigation and preserved direct access to the existing dashboard surface [any]
- Ownership boundaries are explicit across prism-mcp routes and read models, the React app shell, and any new plan or graph page data contracts [any]
- The rollout rule is explicit: keep the current dashboard usable throughout and keep freeform graph editing out of the first release [any]

#### Tags

- `ia`
- `routing`
- `ui-shell`

### Milestone 8: Add freshness and live-update trust signals across control-plane surfaces

- Node id: `coord-task:01kn00sx8j4fjmtbte3bsyf8pv`
- Kind: `edit`
- Status: `proposed`
- Summary: Make Overview, Plans, and Graph trustworthy operational surfaces by adding explicit freshness semantics, stale indicators, manual refresh, and a shared path toward polling or SSE where live updates matter.
- Priority: `8`

#### Acceptance

- Overview, Plans, and Graph expose last-updated or stale state clearly enough that humans can judge freshness before acting [any]
- Users can manually refresh every major control-plane surface without navigating away or reloading the whole app [any]
- Shared client infrastructure defines consistent freshness semantics across Dashboard, Overview, Plans, and Graph instead of ad hoc per-page behavior [any]
- The implementation leaves a clear path for polling or SSE on non-dashboard surfaces where live updates materially improve trust [any]

#### Validation Refs

- `ui:control-plane-freshness`

#### Tags

- `freshness`
- `live-update`
- `trust`
- `ui`

### Milestone 9: Add structured human intervention flows for plans

- Node id: `coord-task:01kn00t3b6svvvcm759dwvt8fw`
- Kind: `edit`
- Status: `proposed`
- Summary: Extend the read-only Plans surface into a true human control plane with structured mutation flows for notes, blockers, acceptance, status changes, and handoff actions, while keeping freeform graph editing out of scope.
- Priority: `9`

#### Acceptance

- Humans can perform structured plan mutations for notes, blockers, acceptance criteria, and status changes without falling back to raw plan-file edits [any]
- Handoff and review-oriented plan actions are available through explicit UI flows backed by stable MCP mutation adapters [any]
- Plan mutation affordances preserve structure and guardrails; freeform graph editing remains out of scope for the first release [any]
- The Plans page distinguishes clearly between read-only runtime state and human-authored control actions [any]

#### Validation Refs

- `ui:plan-human-controls`

#### Tags

- `human-control`
- `mutations`
- `plans`
- `ui`

## Edges

- `plan-edge:coord-task:01kmzvjdhb4depcxapk8a14naz:depends-on:coord-task:01kmzvjdhbgcw86tj5tjs7cd1b`: `coord-task:01kmzvjdhb4depcxapk8a14naz` depends on `coord-task:01kmzvjdhbgcw86tj5tjs7cd1b`
- `plan-edge:coord-task:01kmzvjdhb4depcxapk8a14naz:depends-on:coord-task:01kmzvjdhbsgjm53m3ws4r2ax8`: `coord-task:01kmzvjdhb4depcxapk8a14naz` depends on `coord-task:01kmzvjdhbsgjm53m3ws4r2ax8`
- `plan-edge:coord-task:01kmzvjdhb4depcxapk8a14naz:depends-on:coord-task:01kmzvjdhbwtrnjz0q7w13337m`: `coord-task:01kmzvjdhb4depcxapk8a14naz` depends on `coord-task:01kmzvjdhbwtrnjz0q7w13337m`
- `plan-edge:coord-task:01kmzvjdhb531p74z3efn6smg3:depends-on:coord-task:01kn00t3b6svvvcm759dwvt8fw`: `coord-task:01kmzvjdhb531p74z3efn6smg3` depends on `coord-task:01kn00t3b6svvvcm759dwvt8fw`
- `plan-edge:coord-task:01kmzvjdhb96zxv7pnt1t06fj8:depends-on:coord-task:01kmzvjdhbybkjn70sasjwahwd`: `coord-task:01kmzvjdhb96zxv7pnt1t06fj8` depends on `coord-task:01kmzvjdhbybkjn70sasjwahwd`
- `plan-edge:coord-task:01kmzvjdhbgcw86tj5tjs7cd1b:depends-on:coord-task:01kmzvjdhb96zxv7pnt1t06fj8`: `coord-task:01kmzvjdhbgcw86tj5tjs7cd1b` depends on `coord-task:01kmzvjdhb96zxv7pnt1t06fj8`
- `plan-edge:coord-task:01kmzvjdhbgcw86tj5tjs7cd1b:depends-on:coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`: `coord-task:01kmzvjdhbgcw86tj5tjs7cd1b` depends on `coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`
- `plan-edge:coord-task:01kmzvjdhbpqxs2nzjxwbvznp9:depends-on:coord-task:01kmzvjdhbwhnhz1fgkrebe94p`: `coord-task:01kmzvjdhbpqxs2nzjxwbvznp9` depends on `coord-task:01kmzvjdhbwhnhz1fgkrebe94p`
- `plan-edge:coord-task:01kmzvjdhbsgjm53m3ws4r2ax8:depends-on:coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`: `coord-task:01kmzvjdhbsgjm53m3ws4r2ax8` depends on `coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`
- `plan-edge:coord-task:01kmzvjdhbtbmjrjr2txxn2y0j:depends-on:coord-task:01kmzvjdhb531p74z3efn6smg3`: `coord-task:01kmzvjdhbtbmjrjr2txxn2y0j` depends on `coord-task:01kmzvjdhb531p74z3efn6smg3`
- `plan-edge:coord-task:01kmzvjdhbwhnhz1fgkrebe94p:depends-on:coord-task:01kmzvjdhbybkjn70sasjwahwd`: `coord-task:01kmzvjdhbwhnhz1fgkrebe94p` depends on `coord-task:01kmzvjdhbybkjn70sasjwahwd`
- `plan-edge:coord-task:01kmzvjdhbwtrnjz0q7w13337m:depends-on:coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`: `coord-task:01kmzvjdhbwtrnjz0q7w13337m` depends on `coord-task:01kmzvjdhbpqxs2nzjxwbvznp9`
- `plan-edge:coord-task:01kn00sx8j4fjmtbte3bsyf8pv:depends-on:coord-task:01kmzvjdhb4depcxapk8a14naz`: `coord-task:01kn00sx8j4fjmtbte3bsyf8pv` depends on `coord-task:01kmzvjdhb4depcxapk8a14naz`
- `plan-edge:coord-task:01kn00t3b6svvvcm759dwvt8fw:depends-on:coord-task:01kn00sx8j4fjmtbte3bsyf8pv`: `coord-task:01kn00t3b6svvvcm759dwvt8fw` depends on `coord-task:01kn00sx8j4fjmtbte3bsyf8pv`

