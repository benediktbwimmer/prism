# Runtime Rewrite Architecture

This document is the execution artifact for:

- `plan:01kn09gyzw4e8wzwvgvbbj9mkk`
- `coord-task:01kn09je3c546kafd2z7bcgnyq`

Its purpose is to define the hard contracts and migration invariants for the PRISM runtime rewrite.

This is the architectural target for the remaining rewrite-plan nodes. It is broader than
`docs/REFRESH_RUNTIME_REDESIGN.md`, which focused on removing request-path persisted reloads from
steady-state serving.

## Goals

The rewrite must make PRISM:

- efficient by default on small repos
- scalable to large monorepos without broad eager hydration
- explicit about freshness, scope, and materialization depth
- incrementally maintainable under repeated edits and watch-driven refreshes

## Existing substrate

Some of the rewrite foundation already exists and should be treated as the starting point rather
than reimplemented from scratch.

Implemented today:

- file and directory fingerprints in the workspace tree
- dirty-path tracking in the live workspace session
- file-scoped incremental refresh planning
- scoped parsing and graph updates
- basic dependency expansion for edge resolution
- incremental projection updates from lineage and outcome deltas

Not fully implemented yet:

- a first-class dependency-aware invalidation graph
- localized concept and plan-health reprojection
- explicit hot/warm/cold runtime-state boundaries
- partial materialization and depth-aware serving
- explicit boundary semantics for non-materialized regions
- repo/worktree/session-scoped runtime overlays as a first-class model

## Hard invariants

### 1. Live runtime is the serving authority

Normal reads and normal mutations serve from live runtime state.

Optional persisted state may accelerate startup or recovery, but it is not the normal serving
authority.

### 2. Runtime state must be tiered

PRISM may not treat all historical, analytical, and serving data as equally hot.

The runtime must be split into:

- hot state: always-ready serving state
- warm state: cheap lazy-read state for recent or task-local context
- cold state: durable evidence and historical records that do not belong in the daemon hot path

### 3. Incremental updates are the default

A meaningful edit should update only the affected region, plus the minimal dependent surfaces
required for correctness.

Broad rebuilds are recovery behavior, not normal behavior.

### 4. Materialization depth is explicit

The runtime must know what depth a repo region currently has and expose that state to queries,
status, and logs.

### 5. Scope must stay explicit

Repo, worktree, branch, and session state must not silently bleed together.

The rewrite must preserve the distinction between:

- repo-published truth
- worktree-local reality
- session-local intent and overlays

### 6. Missing is not the same as unmaterialized

Queries and reasoning must be able to distinguish:

- absent
- not yet indexed
- intentionally shallow
- known but not materialized
- out of worktree scope

### 7. Ranking is downstream of the runtime substrate

Architectural ranking may improve broad-query precision, but it must sit on top of explicit scope,
materialization, and invalidation semantics.

## Runtime state model

### Hot state

Hot state is what the daemon hydrates eagerly and keeps ready for normal serving.

It should contain:

- current graph for materialized regions
- bounded serving projections
- active coordination and plan execution overlays
- current worktree refresh state
- current materialization and freshness metadata

Hot state must stay bounded and cheap to rebuild incrementally.

### Warm state

Warm state is lazily available and may be cached opportunistically.

It should contain:

- recent outcomes and task-local outcome slices
- recent lineage/history windows
- current-task or current-region curator state
- medium-depth region details not required everywhere

Warm state may be loaded on demand, retained briefly, or evicted.

### Cold state

Cold state is durable or analytical evidence that should not be eagerly hydrated into the process.

It should contain:

- full lineage event history
- full outcome event log
- analytical evidence that derives serving projections
- old curator records and historical snapshots

Cold state must be queryable and rebuildable, but not treated as daemon-hot by default.

## Invalidation model

The invalidation substrate should extend the existing dirty-path refresh machinery into a formal
dependency-aware model.

### Required inputs

- file fingerprints
- symbol fingerprints
- module/package region fingerprints
- explicit dirty-region markers
- dependency edges required for invalidation propagation

### Required behavior

- filesystem changes produce dirty regions
- dirty regions expand through dependency-aware propagation
- only impacted files and dependent regions are reparsed
- only impacted projections and overlays are recomputed
- unaffected regions keep their current hot state

### Important boundary

The current file-scoped incremental refresh path is real and valuable, but it is not yet the full
target model. The missing piece is formal dependency propagation and localized reprojection beyond
file-level parsing.

## Materialization-depth model

The runtime should support three default depth tiers.

### Shallow

Used for broad repo navigation and low-cost repo-wide awareness.

Should include:

- file presence
- package/module boundaries
- file fingerprints
- basic exports or top-level names where available

### Medium

Used for normal semantic navigation in active regions.

Should include:

- symbol inventory
- call/import/reference edges
- concept/member attachment surfaces
- lineage bindings needed for normal reasoning

### Deep

Used only where task pressure justifies higher cost.

Should include:

- expensive structural enrichment
- richer semantic extraction
- heavyweight region-specific analysis

Queries, status surfaces, and logs must be able to state which tier a region currently has.

## Boundary semantics

PRISM must represent non-materialized regions explicitly instead of silently dropping them.

Boundary records or nodes must be able to express:

- stable identity
- source path or package marker
- provenance
- known exports or attachment hints when available
- materialization state
- scope state

Minimum materialization/scope states:

- `absent`
- `shallow`
- `medium`
- `deep`
- `known_unmaterialized`
- `out_of_scope`

The exact enum names can change, but these semantics must survive.

## Scope model

The runtime rewrite must treat scope as a core correctness boundary.

### Repo scope

Repo-published truth and repo-scoped concepts, memories, contracts, and plans remain durable and
exportable.

### Worktree scope

Graph state, freshness, dirtiness, and draft overlays tied to a checkout must be scoped to the
actual worktree.

### Session scope

Active intent, temporary claims, task-local overlays, and ephemeral caches belong at session scope.

The rewrite must make it hard to accidentally answer a question from the wrong worktree or with the
wrong overlay set.

## Serving contract

### Read path

Read paths should:

1. read from hot runtime state
2. load warm state only if the query actually needs it
3. attach freshness/materialization metadata where relevant
4. enqueue background work when the runtime is stale or too shallow
5. return

Read paths should not:

- force broad persisted reloads
- hydrate cold evidence wholesale
- rebuild full projections to answer narrow queries

### Mutation path

Mutation paths should:

1. validate input
2. patch hot runtime state
3. update or enqueue affected warm/derived surfaces
4. persist durable events
5. return from live state

Mutation correctness must not depend on “persist then reload everything to see the write.”

## Parsing and enrichment contract

Lazy deep parsing belongs after hot/warm/cold boundaries and materialization tiers are explicit.

When implemented, it must obey these rules:

- shallow repo-wide awareness remains cheap
- deep parsing is targeted to active regions or explicitly requested work
- first-touch deep parsing is observable in status and telemetry
- deep parsing cannot silently become the default cost of ordinary navigation

## Ranking contract

Architectural ranking belongs after the runtime substrate is explicit.

Ranking may use:

- dependency weight
- concept quality
- historical successful use
- task proximity
- worktree proximity
- bounded centrality-like signals

Ranking may not become a substitute for correct scope, freshness, or materialization semantics.

## Explicit non-goals for this migration

This rewrite plan does not include:

- remote shared runtime backend work
- database-pushed graph traversals

Those should remain excluded unless later evidence justifies reopening them.

## Migration sequence

The rewrite should proceed in this order:

1. define contracts and invariants
2. complete the dependency-aware invalidation substrate on top of existing incremental refresh
3. replace broad reload behavior with region-scoped hot-state mutation
4. introduce explicit materialization tiers
5. add boundary semantics for non-materialized regions
6. move history, outcomes, curator, and analytical evidence onto warm/cold lazy-read paths
7. make runtime overlays explicitly repo-, worktree-, and session-scoped
8. add lazy deep parsing on top of the tiered runtime
9. improve broad-query ranking on top of the explicit substrate
10. validate the system against startup, latency, memory, and retrieval quality

## Acceptance criteria for this node

This node is complete when:

- the runtime-state tiers are explicitly defined
- the invalidation target model is explicit about what already exists and what remains missing
- materialization-depth and boundary semantics are explicit
- repo/worktree/session scope rules are explicit
- later implementation nodes can use this document as their architectural contract
