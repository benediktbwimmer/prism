# PRISM Cross-Repo Future

Status: forward-looking design note  
Audience: PRISM core, coordination, runtime, storage, MCP, and query maintainers  
Scope: project-scoped published knowledge, cross-repo coordination, shared runtime truth, and the path from local SQLite to remote shared backends

---

## 1. Summary

PRISM should eventually support **cross-repo concepts, contracts, plans, and memories** without abandoning the core rule that published knowledge should be explicit, durable, reviewable, and portable.

The central design decision is:

- **repo-local published truth** continues to live in each code repo's `.prism/`
- **project-level published cross-repo truth** should live in a dedicated **project git repo**
- **shared runtime state** continues to live in the shared runtime backend (`SQLite` locally first, `Postgres` later)
- **worktree-local hot acceleration** remains local and rebuildable

This means PRISM grows by **adding a project scope**, not by collapsing all truth into a mutable database.

The shared runtime database is still important, but its role is different:

- live mutable coordination state
- unpublished working graph state
- draft cross-repo relations and concepts before promotion
- claims, leases, handoffs, reviews, and execution overlays
- runtime projections and query acceleration

The database should **not** become the only home of durable published system knowledge.

The long-term architecture is therefore:

1. **Repo scope** for published repo truth
2. **Project scope** for published cross-repo truth
3. **Shared runtime scope** for mutable coordination and working knowledge
4. **Worktree scope** for rebuildable hot acceleration

---

## 2. Why this matters

PRISM already solves the hardest part of agentic work inside one repo:

- orientation
- ownership lookup
- concept-driven top-down navigation
- durable plans
- coherence across time

Those same problems become even more painful across multiple repos:

- architectural boundaries become repo boundaries
- migrations become staged and interdependent
- ownership becomes fragmented
- the relevant concepts are often system-level rather than repo-local
- raw text search becomes even less useful as the primary navigation layer

The real opportunity is not just "PRISM, but on more code." It is:

**PRISM as system-level orientation and coordination for software that spans multiple repositories.**

That enables workflows such as:

- API migrations across service, SDK, and documentation repos
- schema and consumer rollouts across producer and downstream repos
- multi-repo deprecations and compatibility windows
- frontend/backend contract changes
- coordinated release-train plans with blockers, handoffs, and reviews
- cross-repo architectural initiatives grounded in durable concepts and contracts

---

## 3. Design goals

### 3.1 Goals

- Preserve the current strength of repo-local published truth.
- Add first-class cross-repo plans, concepts, contracts, and memories.
- Keep published knowledge explicit and reviewable.
- Keep mutable working state in the shared runtime backend.
- Make local SQLite and future remote Postgres fit the same scope model.
- Support one machine-level daemon managing many repos and worktrees.
- Support future multi-machine coordination without redefining the data model.
- Avoid blurring repo-local truth, project truth, runtime overlays, and rebuildable caches.

### 3.2 Non-goals

- Do not move repo-local published truth out of code repos.
- Do not make the shared runtime database the sole home of durable cross-repo truth.
- Do not duplicate all repo-local concepts and contracts into the project repo.
- Do not require one database per worktree.
- Do not require a remote backend in the first implementation.
- Do not make project scope mandatory for all repos.

---

## 4. Core design decisions

### 4.1 Add a project scope above repo scope

PRISM should gain a first-class **project scope** for workflows and knowledge that genuinely span multiple repos.

A project may include:

- zero or more repos
- system-level concepts
- cross-repo contracts
- cross-repo plans
- promoted cross-repo memories
- runtime coordination state spanning those repos

A repo may participate in zero or one project in the simple initial model.

### 4.2 Use a project git repo for published cross-repo knowledge

Published cross-repo knowledge should live in a dedicated **project git repo**.

This is the best long-term home because it preserves the same properties that make repo-local `.prism/` valuable today:

- reviewable
- diffable
- branchable
- cloneable
- portable
- explicitly promoted
- inspectable by both humans and agents

The project repo is the cross-repo equivalent of a code repo's local `.prism/`.

### 4.3 Keep the shared runtime backend for mutable cross-repo truth

The shared runtime backend should hold:

- cross-repo execution overlays
- claims, leases, handoffs, reviews, and blockers
- unpublished draft concepts and relations
- runtime-discovered cross-repo dependencies
- mutable plan execution state
- imported evidence and temporary working state
- denormalized projections and query accelerators

This remains true whether the backend is:

- local SQLite on one machine
- future Postgres shared across machines

### 4.4 Preserve worktree-scoped hot runtime engines

Even with project scope, the live serving model remains:

- one `WorkspaceRuntimeEngine` per worktree/workspace context
- one shared runtime backend per logical repo on the machine today
- optional project-scoped coordination state above repo scope
- one machine-level daemon as the normal operating shape

The project scope does **not** replace repo-scoped runtime state or worktree-scoped hot state.

### 4.5 Promotion remains explicit

Cross-repo truths should usually be born in runtime state and promoted later.

The expected lifecycle is:

1. observe or infer cross-repo structure in runtime state
2. curate it through human or agent review
3. promote stable truths into the project repo
4. derive runtime projections from the published project truth again

That preserves PRISM's current strength: **knowledge becomes durable by explicit promotion, not by accident.**

---

## 5. The four-scope model

PRISM should ultimately operate across four scopes.

### 5.1 Worktree scope

This is the scope of:

- one local checkout
- one local branch reality
- one `WorkspaceRuntimeEngine`
- rebuildable hot acceleration
- local MCP runtime files and logs

This scope is path-sensitive and disposable.

### 5.2 Repo scope

This is the scope of:

- one logical repository
- published repo truth in the repo's `.prism/`
- repo-scoped shared runtime truth on the local machine
- repo-scoped concepts, contracts, plans, and memories

This remains the foundational PRISM unit.

### 5.3 Project scope

This is the new scope of:

- cross-repo published system knowledge
- cross-repo plans and rollout coordination
- concepts that describe the system rather than one repo
- contracts whose producer and consumer live in different repos
- promoted memories about multi-repo changes or incidents

Project scope is above repo scope, but should not swallow it.

### 5.4 Global import / external scope

This is the scope of:

- imported external evidence
- GitHub/CI/benchmark/replay imports
- remote retrieval caches
- external identities or evidence that inform PRISM reasoning without becoming canonical published truth

This scope should remain clearly separate from both repo truth and project truth.

---

## 6. Published truth vs mutable runtime truth

The most important architectural rule is to keep these separate.

### 6.1 Published truth

Published truth is:

- explicit
- durable
- reviewable
- inheritable
- portable
- part of the long-lived knowledge model

Published truth should live in git-backed artifacts:

- repo-local published truth in code repos
- project-level published truth in the project repo

### 6.2 Mutable runtime truth

Mutable runtime truth is:

- operational
- provisional
- often unpublished
- coordination-heavy
- subject to contention and leases
- not necessarily ready to publish

Mutable runtime truth belongs in the shared runtime backend.

### 6.3 Derived acceleration

Derived acceleration is:

- rebuildable
- query-oriented
- non-authoritative
- safe to delete

This belongs in worktree-local or backend-generated materializations, not in the published artifact plane.

---

## 7. What lives where

### 7.1 In each code repo

Each code repo continues to own its published repo truth:

- repo-local concepts
- repo-local contracts
- repo-local plans
- repo-local memories

This remains in:

```text
<repo>/.prism/
  concepts/
  contracts/
  memory/
  plans/
```

### 7.2 In the project git repo

The project repo should own published cross-repo truth:

- project-scoped concepts
- cross-repo contracts
- cross-repo plans
- project-scoped promoted memories
- cross-repo relations curated as published truth
- optional membership and system manifest metadata

A first approximation of the project repo can mirror the same artifact layout PRISM already uses:

```text
<project-repo>/
  .prism/
    concepts/
    contracts/
    memory/
    plans/
```

The project repo may later gain additional project-specific manifests, but the base pattern should stay familiar.

### 7.3 In the shared runtime backend

The shared runtime backend should hold:

- unpublished cross-repo relations
- draft project concepts and contracts before promotion
- active plan execution overlays
- claims, leases, handoffs, blockers, reviews
- mutable coordination state
- project/repo/worktree membership records
- replayable mutation events and journals
- cross-repo projections and query indexes

### 7.4 In worktree-local acceleration

Worktree-local acceleration should hold:

- checkpoints
- materialized read models
- local query acceleration
- helper caches
- path-sensitive runtime artifacts

This layer must remain disposable.

---

## 8. Cross-repo artifact model

The future project repo should support the same major artifact types PRISM already uses, but at project scope.

### 8.1 Cross-repo concepts

Cross-repo concepts describe system-level structure or bounded contexts that span multiple repos.

Examples:

- the public API surface shared by service and SDK repos
- the deployment contract across infrastructure and application repos
- the migration choreography for producer and consumer repos
- the ownership model for a multi-repo subsystem

These are not copies of repo-local concepts. They are higher-level concepts that reference repo-local anchors where needed.

### 8.2 Cross-repo contracts

Cross-repo contracts describe obligations or compatibility rules that cross repository boundaries.

Examples:

- API producer/consumer contracts
- schema compatibility contracts
- rollout ordering invariants
- backwards-compatibility windows
- docs or migration requirements that span multiple repos

These may reference implementation anchors in several repos while remaining one project-published contract.

### 8.3 Cross-repo plans

Cross-repo plans are one of the highest-upside future directions.

They can represent:

- staged migrations across several repos
- dependency-ordered rollout plans
- cross-repo deprecations
- release-train workflows
- system initiatives that require multiple workstreams and ownership handoffs

These plans should be able to bind to:

- project-scoped concepts and contracts
- repo-scoped symbols and validations
- repo-scoped or project-scoped artifacts
- coordination state in the runtime backend

### 8.4 Cross-repo memories

Project-scoped memories should capture durable lessons that belong to the system rather than a single repo.

Examples:

- a migration lesson affecting several repos
- a contract violation incident spanning producer and consumer repos
- rollout pitfalls discovered during a system change
- repeated validation failures at a repo boundary

As with repo-local memory, promotion should be deliberate.

---

## 9. Cross-repo graph and reference model

Project scope requires PRISM to represent both:

- entities that are intrinsically project-scoped
- references to repo-scoped anchors across many repos

### 9.1 Repo-scoped anchors remain repo-scoped

Repo-local entities such as symbols, files, modules, or repo-local concepts should remain rooted in their home repo.

A project-level artifact should **reference** those anchors rather than absorb them into a giant merged namespace.

### 9.2 Project-scoped entities become first-class

Some system truths are not reducible to one repo.

Examples:

- a system concept spanning service, SDK, and docs repos
- a compatibility contract between two repos
- a multi-repo rollout wave
- a project-level artifact bundle

These should be modeled as project-scoped entities, not forced into one repo.

### 9.3 Cross-repo relations should exist in two forms

Cross-repo relations should be able to exist as:

1. **runtime working relations** in the shared backend
2. **published curated relations** in the project repo

That lets PRISM support the normal lifecycle:

- discover relation
- test and refine relation
- publish relation when stable enough to be part of durable system knowledge

### 9.4 Do not flatten everything into one global graph root

The graph should feel unified to the query surface, but not lose scope discipline internally.

The right model is:

- repo entities retain `repo_id`
- project entities retain `project_id`
- cross-repo relations explicitly connect endpoints across repos
- scope is encoded in the data model, not only implied by file location

---

## 10. Query semantics

The query experience should feel integrated, even though the underlying truth spans several scopes.

A future project-aware query should be able to join:

- project-published concepts and contracts
- repo-published concepts and contracts from member repos
- shared runtime overlays and coordination state
- worktree-local freshness and branch-sensitive hot state

Conceptually, the read stack becomes:

1. **project-published truth**
2. **repo-published truth**
3. **shared runtime overlays**
4. **worktree-local freshness and acceleration**

The ordering of those layers matters:

- published truth remains the stable base
- runtime state provides current working overlays
- worktree-local state provides immediacy and speed

This is the same pattern PRISM already uses locally, extended upward to project scope.

---

## 11. Coordination model

Cross-repo coordination should follow the same principles PRISM already uses inside one repo.

The shared runtime backend should support:

- project-scoped claims and leases
- cross-repo handoffs
- blockers spanning repos
- project-scoped review artifacts
- multi-repo execution overlays
- cross-repo task assignment and reclaim semantics

The project repo is not the place for high-churn live lease state.

The project repo is the place for:

- published plans
- published concepts
- published contracts
- promoted memories

The backend is the place for:

- active work ownership
- stale/reclaimable work
- draft execution state
- mutable coordination records

---

## 12. Relationship to the shared runtime backend

### 12.1 Local-first shape

The first useful shape is:

- one PRISM daemon per machine as the normal operating form
- one shared runtime SQLite store per repo on that machine today
- optional project-scoped coordination storage above repo scope
- one `WorkspaceRuntimeEngine` per worktree/workspace internally

### 12.2 Future remote shape

The future remote shape should preserve the same semantics while swapping the backend implementation.

That means:

- SQLite locally first
- Postgres later
- same scope model
- same identities
- same event and mutation model
- same distinction between published truth and runtime overlays

The remote backend should not redefine the architecture. It should only change where shared mutable truth is persisted and synchronized.

### 12.3 What the remote backend is for

The future remote backend is the right home for:

- cross-machine coordination
- shared project execution overlays
- live claims and leases
- working cross-repo graph state
- mutable coordination and provenance records

It is **not** the only home of published project truth.

That remains the project repo.

---

## 13. Project repo structure

A simple first version of the project repo should look familiar.

```text
<project-repo>/
  .prism/
    concepts/
      events.jsonl
      relations.jsonl
    contracts/
      events.jsonl
    memory/
      events.jsonl
    plans/
      index.jsonl
      active/
      archived/
```

Optional future additions may include:

- a project membership manifest
- project-level repo descriptors
- system ownership manifests
- import or benchmark metadata that is safe to publish

The important rule is to keep the project repo focused on **published system knowledge**, not on logs, caches, or runtime state.

---

## 14. Promotion flow

A healthy future lifecycle looks like this:

### 14.1 Discover

An agent or runtime notices a cross-repo pattern:

- a contract implemented in one repo and consumed in another
- a repeated migration dependency across repos
- a system-level concept spanning multiple bounded contexts
- a rollout dependency chain

This first lands in runtime state, not immediately in published truth.

### 14.2 Curate

The relation, concept, or plan is curated:

- confirmed by evidence
- linked to repo anchors
- named cleanly
- stripped of ephemeral noise
- checked for whether it belongs at repo or project scope

### 14.3 Promote

If it is stable enough to be part of durable system knowledge, it is promoted into the project repo.

### 14.4 Reproject

Once published, it becomes the base for:

- future queries
- future runtime overlays
- future coordination and plan execution

This keeps PRISM's best property intact: **knowledge compounds through explicit publication.**

---

## 15. Example: multi-repo API migration

Imagine a system with these repos:

- `service-api`
- `typescript-sdk`
- `docs-site`
- `integration-tests`

A project repo could publish:

- a concept for the `Public Search API`
- a contract defining compatibility and rollout rules
- a multi-repo plan for deprecating `search_v1` and adopting `search_v2`
- a memory recording that the last versioned rollout failed because docs and SDK examples lagged the service release

During active execution, the shared runtime backend would hold:

- who currently owns each step
- which repos are blocked
- active claims and leases
- draft rollout notes
- discovered affected validations
- runtime projections showing current progress and stale work

That is the exact split PRISM should aim for:

- published system truth in the project repo
- live mutable coordination in the backend
- repo-local implementation truth in each code repo

---

## 16. Migration strategy

PRISM should reach this future in phases.

### Phase 1 — scope model only

Add first-class support in the data model for:

- `project_id`
- `repo_id`
- `worktree_id`
- project-scoped entities and relations
- repo membership in a project

No mandatory project repo yet.

### Phase 2 — runtime-only cross-repo coordination

Support project-scoped runtime coordination in the shared backend:

- cross-repo plans in runtime form
- project-scoped claims, handoffs, and overlays
- discovered cross-repo relations in unpublished state

This proves the behavior before published project truth exists.

### Phase 3 — project repo as published cross-repo plane

Introduce the project git repo and promotion flows for:

- project concepts
- project contracts
- project plans
- project memories

### Phase 4 — integrated query and promotion workflows

Unify project queries across:

- project repo truth
- member repo truth
- runtime overlays
- worktree-local freshness

### Phase 5 — remote shared backend

Swap or add a Postgres backend for the shared runtime plane without changing the scope model.

---

## 17. Rules to protect

The following rules should remain strict.

### 17.1 Do not put all cross-repo truth only in the database

The database is the right home for mutable working state, not the only home of durable published system knowledge.

### 17.2 Do not copy every repo-local fact into the project repo

The project repo should hold system-level truths, not a giant duplication of member repos.

### 17.3 Keep promotion explicit

Published project knowledge should be reviewed and promoted intentionally.

### 17.4 Keep runtime overlays mutable and operational

Leases, claims, blockers, and execution overlays belong in runtime state, not in the project repo.

### 17.5 Keep scope explicit in the data model

Filesystem placement is helpful, but `project_id`, `repo_id`, and `worktree_id` must remain first-class semantic scope fields.

---

## 18. Open design questions

These questions do not block the direction, but they should be answered before implementation hardens.

- How should a repo declare or discover project membership?
- Should project membership be published in the project repo, local runtime state, or both?
- What is the canonical reference syntax from project-published artifacts to repo-published anchors?
- When should a relation stay runtime-only versus being promoted to project truth?
- Should project-scoped concepts and contracts use the exact same artifact schema as repo-scoped ones, or an extended variant?
- How much cross-repo projection should be eagerly materialized versus computed on demand?
- Should project scope initially support exactly one project per repo, or allow many-to-many later?

---

## 19. Recommended architectural stance

The recommended stance is:

- **repo repos** own local published truth
- the **project repo** owns published cross-repo truth
- the **shared runtime backend** owns mutable coordination and unpublished working graph state
- **worktree-local state** owns rebuildable hot acceleration

This gives PRISM the best of both worlds:

- durable, reviewable, portable knowledge in git
- strong live coordination and execution state in the backend
- a clean path from local SQLite to remote Postgres
- a real future for cross-repo plans, concepts, contracts, and memories

---

## 20. End-state vision

The long-term win is not merely "PRISM works across more files."

The win is:

**PRISM becomes a system-level orientation, memory, and coordination substrate for software that spans many repositories.**

In that world:

- each repo keeps its own grounded local truth
- the project repo captures durable system truth above repo boundaries
- the shared runtime backend manages live multi-actor execution and coordination
- agents can navigate the system by concepts first, repo symbols second, and raw text search third

That is the cross-repo future worth building.
