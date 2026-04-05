# PRISM SSR Operator Console

Status: proposed target design
Audience: PRISM UI, MCP, projections, and operator-experience maintainers
Scope: a second, simpler PRISM UI built from server-rendered HTML, projected markdown, Mermaid graph slices, `vis-timeline` swimlanes, and optional htmx mutation flows

---

## 1. Summary

PRISM should gain a second human-facing UI that is simpler, more document-shaped, and more robust
than the React operator console.

This surface should:

- render primarily as server-side HTML
- reuse PRISM's existing markdown projection machinery wherever possible
- use Mermaid with ELK layout for plan graphs and bounded concept graph slices
- use `vis-timeline` for runtime swimlanes and lease timelines
- optionally use htmx so the surface can be write-capable without becoming a second SPA

The key positioning is:

- the React operator console remains the richer, more interactive control plane
- the SSR console becomes the simpler, lower-complexity, highly legible operational and
  documentation surface

This is not a fallback-only emergency UI. It should be a first-class product surface for operators
who prefer simple URLs, resilient pages, and document-style navigation.

---

## 2. Why This UI Should Exist

The current React console is useful, but it has real complexity:

- a client-side routing shell
- polling hooks and local state orchestration
- richer component and visualization logic
- more moving parts to debug when data shape, cache behavior, or interaction flow changes

PRISM already has strong ingredients for a simpler second surface:

- deterministic markdown projections
- stable server-side read models
- same-origin serving from the daemon
- clear mutation endpoints
- bounded graph and timeline data

An SSR UI can therefore provide:

- simpler implementation and maintenance
- excellent deep-linkability
- strong resilience to reloads, sleep/wake, and daemon restarts
- better fit for document-heavy PRISM surfaces
- a natural way to expose repo-native rendered knowledge without forcing every page into SPA state

---

## 3. Product Positioning

The two UIs should coexist intentionally.

### 3.1 React operator console

Best for:

- richer task editing
- complex cross-panel interaction
- highly dynamic operator workflows
- future advanced coordination controls

### 3.2 SSR operator console

Best for:

- highly legible read-mostly operational views
- lightweight human intervention through forms and buttons
- deep-linkable plan, concept, and runtime pages
- robust rendering with minimal client complexity
- document-centric navigation and review

The second UI should therefore not try to out-interact the React console. It should optimize for:

- clarity
- reliability
- inspectability
- low client complexity

---

## 4. Architectural Model

The SSR console should be built from four layers:

1. server-rendered HTML pages
2. markdown-derived page sections and summaries
3. lightweight client enhancement for charts
4. optional htmx for write-capable forms, actions, and polling fragments

The daemon remains the single serving surface.

### 4.1 Server-side rendering

PRISM should render the HTML on the daemon side using existing read models and projection logic.

The server owns:

- route handling
- data loading
- markdown-to-HTML rendering
- Mermaid source generation
- `vis-timeline` data serialization
- mutation form handling

The browser should receive ready-to-read HTML, not raw JSON that a thick client must assemble.

### 4.2 Markdown as a primary projection substrate

The existing markdown projection machinery should remain a source of truth for the document-shaped
parts of this UI.

That means:

- plan summaries can be projected from plan markdown or the same source inputs
- concept pages can reuse projected concept and relation content
- repo-state and structural docs can be rendered directly into HTML sections

The SSR console should not duplicate prose projection logic in a second frontend-specific layer
unless a page genuinely needs a different operator-focused projection.

### 4.3 Client enhancement, not client ownership

The client should enhance the page, not own the page.

Use client-side JavaScript only for:

- Mermaid rendering of already emitted graph definitions
- `vis-timeline` rendering from already emitted lane/item data
- htmx interactions and partial updates
- small local affordances such as drawer toggles or filter persistence

The client should not be responsible for reconstructing the page's truth from API calls.

---

## 5. Route Model

The SSR console should be a distinct route family instead of replacing the current React UI.

Recommended route prefix:

- `/console`

Recommended initial route set:

- `/console`
  - landing page with operator-oriented navigation and status
- `/console/plans`
  - active plan portfolio and summaries
- `/console/plans/{plan_id}`
  - single-plan detail page with plan graph and task list
- `/console/tasks/{task_id}`
  - task detail page
- `/console/concepts`
  - concept index/search
- `/console/concepts/{anchor}`
  - focused concept page with bounded graph slice
- `/console/fleet`
  - runtime swimlane timeline

Optional future routes:

- `/console/contracts/{anchor}`
- `/console/memory/{id}`
- `/console/repo`

This route family should be same-origin with the daemon and should reuse the same authentication and
principal binding as the React console.

---

## 6. Visualization Strategy

The visualization stack should be explicit and split by problem type.

### 6.1 Plan graphs: Mermaid with ELK

Plan graphs should use Mermaid flowcharts with ELK layout.

Why:

- plans are usually DAG-shaped and structured enough for Mermaid
- Mermaid fits naturally into server-rendered markdown/HTML pages
- ELK materially improves medium-sized layout quality
- the graph can remain legible without requiring a heavy graph application runtime

Expected quality:

- high for small plans
- good for medium plans
- acceptable but not ideal for very large/dense plans

For the SSR console, this quality level is sufficient.

### 6.2 Concept graphs: Mermaid with focused subgraph generation

Concept graphs should also use Mermaid with ELK, but never as a whole-repo graph.

Instead, concept pages should render bounded focus graphs:

- one focus concept
- configurable traversal depth
- optional direction filter:
  - outbound
  - inbound
  - both
- optional relation-type filters

This avoids the unreadable "hairball graph" failure mode.

The graph should be accompanied by textual structure:

- focus concept summary
- immediate neighbors grouped by relation type
- explicit links to expand the neighborhood

The graph is an aid to understanding, not the only navigation surface.

### 6.3 Fleet swimlanes: `vis-timeline`

The swimlane view should use `vis-timeline`, not Mermaid.

Why:

- the timeline is fundamentally a grouped time-range visualization
- `vis-timeline` maps directly to runtimes as groups and leases/claims as ranged items
- it is better suited than Mermaid to concurrent bars, varying durations, and operational scanning
- it remains lightweight enough to fit the SSR-plus-enhancement model

Rendering model:

- server emits runtime groups and lease/task bars as serialized data
- client initializes `vis-timeline` against a bounded HTML container
- clicks deep-link into the relevant task or open a detail fragment

Expected quality:

- good for the operator use case
- much better than Mermaid for concurrent swimlanes
- still simple enough to keep the second UI lightweight

---

## 7. Write Capability Model

The SSR console should be write-capable.

It should not remain permanently read-only.

### 7.1 htmx is the right mutation layer

htmx should be the primary interaction model for writes and partial refreshes.

Why:

- the page model is already document- and form-shaped
- PRISM mutations are pessimistic by nature
- same-origin POST actions map naturally to htmx forms and buttons
- fragment swapping is enough for most operator actions
- this avoids building a second client-state-heavy application

htmx should be used for:

- inline plan priority changes
- plan archive actions
- task metadata edits
- revoke lease actions
- reassign or reserve actions
- status overrides
- small filter/pagination fragment refreshes
- polling fragments where local freshness matters

### 7.2 Mutation path must stay identical to MCP mutation semantics

The SSR console must not invent a separate write path.

All human-triggered writes should go through the same mutation surface already established for the
React console:

- server route accepts the form or POST
- route resolves the request into the same `MutateAction` shape
- the server executes the identical mutation path used by MCP-triggered mutations
- audit logging, capability checks, Git execution policy, and PRISM publication remain identical

The only difference is presentation format:

- HTML fragment responses instead of JSON-first client orchestration

### 7.3 Pessimistic UX still applies

htmx should not make the UI optimistic.

The SSR UI should use the same product rule as the React console:

- show pending or syncing state immediately
- wait for the next authoritative refresh to confirm visible settled state
- never lie about completion before backend state actually converges

That means:

- actions may return a pending fragment or message
- the updated section is refreshed from authoritative read models
- conflicts and policy failures are shown honestly inline

---

## 8. Polling and Freshness

The SSR console should use simple polling, not SSE and not WebSockets.

There are two acceptable models:

- full-page refresh for the simplest pages
- htmx-triggered fragment polling for focused dynamic regions

Preferred model:

- page HTML renders fully on first request
- dynamic regions such as:
  - active task summaries
  - lease state
  - pending mutation banners
  - fleet slices
  can poll every 2 seconds using htmx fragment replacement

This matches the existing daemon design and avoids transport complexity.

---

## 9. Page Designs

### 9.1 Landing page

The landing page should provide:

- active operator identity
- workspace and branch context
- shared coordination health
- shortcuts into plans, fleet, and concepts
- recent high-signal state such as:
  - active plans
  - blocked plans
  - active runtimes
  - pending reviews or handoffs

This page should feel like a calm operator home, not a metrics dashboard.

### 9.2 Plans index

The plans index should support:

- active-first ordering
- priority sorting
- status filters
- text search
- assigned-runtime filtering

Each plan row or card should include:

- title
- status
- priority
- progress
- active lease pressure
- brief summary
- link to the plan detail page

### 9.3 Plan detail page

The plan detail page should include:

- plan header and summary
- Mermaid plan graph
- task table or grouped task list
- key blockers
- active claims and runtime ownership
- lightweight operator actions

The Mermaid graph should be secondary to a readable textual task list, not the only source of
truth.

### 9.4 Task detail page

The task page should include:

- title, goal, and status
- current lease/owner
- blockers and dependents
- claim history
- outcomes and validation evidence
- recent commits
- editable metadata fields
- operator actions such as revoke, reassign, archive, or status override

This page should be fully usable without any client-side application shell.

### 9.5 Concept pages

Concept pages should include:

- concept summary
- promoted structural facts
- neighboring concepts grouped by relation type
- bounded Mermaid graph slice
- controls for:
  - depth
  - direction
  - relation-type filter

These controls should be htmx-driven so the page can expand or refocus without becoming a full SPA.

### 9.6 Fleet page

The fleet page should include:

- grouped runtime list
- `vis-timeline` swimlanes
- current active claims and durations
- clearly highlighted long-running or stale leases
- runtime detail links

The fleet page should optimize for spotting:

- stuck agents
- idle runtimes
- suspiciously long lease durations
- uneven work distribution

---

## 10. Backend Reuse and New Work

The SSR console should maximize reuse of existing backend work.

### 10.1 Existing reusable pieces

Already useful:

- versioned operator-console API routes
- server-side plan, task, and fleet read models
- existing markdown projection pipeline
- existing mutation routing through MCP-equivalent code paths
- same-origin UI asset serving

### 10.2 New backend work

Additional work likely needed:

- markdown-to-HTML rendering helpers suitable for daemon-served pages
- Mermaid source builders for plan and concept slices
- concept-slice query/read model endpoints
- SSR route handlers and HTML template/rendering helpers
- fragment routes for htmx-driven partial refresh
- `vis-timeline` data adapters
- a small shared asset bundle for:
  - Mermaid
  - `vis-timeline`
  - htmx
  - local console CSS

The implementation should stay modular. Do not collapse HTML rendering, graph generation, route
handling, and mutation handling into one large UI file.

---

## 11. Rendering Model

The SSR console should not rely on ad hoc string concatenation scattered through route handlers.

Preferred rendering structure:

- dedicated SSR route modules
- dedicated page/view model modules
- dedicated markdown rendering module
- dedicated Mermaid generation module
- dedicated timeline serialization module
- dedicated fragment rendering module for htmx partials

The HTML templating technology can stay intentionally simple:

- Rust-side template rendering
- or carefully composed HTML builders

The important constraint is modularity and stable page contracts, not a specific template engine.

---

## 12. Scope Boundaries

### 12.1 In scope for the first SSR UI

- a distinct `/console` route family
- server-rendered HTML pages
- Mermaid plan graphs with ELK
- Mermaid concept graph slices with bounded depth
- `vis-timeline` fleet swimlanes
- htmx-powered write actions for the most important operator mutations
- htmx polling for dynamic regions

### 12.2 Out of scope for the first SSR UI

- freeform graph editing
- replacing the React console
- whole-repo concept graph rendering
- complex client-side state management
- WebSockets or SSE
- a second mutation semantics layer distinct from MCP-equivalent mutation execution

---

## 13. Rollout Strategy

Recommended rollout order:

1. land the doc and route model
2. add a minimal SSR shell and landing page under `/console`
3. render plans index and plan detail pages using existing read models
4. add Mermaid plan graphs
5. add concept pages with bounded slice controls
6. add `vis-timeline` fleet page
7. add htmx-powered operator mutations for plan/task actions
8. polish shared styling and navigation

This should be developed as a second UI surface, not by destabilizing the existing React console
first.

---

## 14. Final Recommendation

PRISM should build this second UI.

The chosen stack should be:

- SSR HTML as the page substrate
- projected markdown as a major content source
- Mermaid with ELK for plan graphs
- Mermaid with bounded focus slices for concept graphs
- `vis-timeline` for swimlanes
- htmx for write-capable interactions and fragment polling

This stack is simpler than a second SPA, more expressive than plain static docs, and well aligned
with PRISM's repo-native and document-oriented strengths.
