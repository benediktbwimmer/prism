# OPTIMIZATIONS

Status: proposed follow-on roadmap
Audience: PRISM core, projections, MCP, coordination, storage, and runtime maintainers
Scope: performance and scaling roadmap to follow the persistence split and multi-workspace/worktree scoping work

---

## 1. Summary

After the persistence split and multi-workspace/worktree scoping land, PRISM should begin a deliberate optimization phase.

The goal is **not** to prematurely optimize for hypothetical billion-line monorepos at the cost of product clarity. The goal is to build the next layer of scaling infrastructure in the correct order:

1. strengthen the substrate that already matters at current scale
2. design now for future large-repo and distributed use cases
3. implement heavier optimizations only when real dogfooding and deployment pressure justify them

This document defines that sequence.

The recommended order is:

* persistence split and scope model
* multi-workspace/worktree support
* incremental indexing and invalidation
* partial materialization and boundary semantics
* DB-backed coordination authority and service hardening
* lazy deep parsing
* graph-ranking and centrality-based retrieval
* database-pushed traversals for very large graphs

The most important rule is:

**optimize the architecture before optimizing the algorithms.**

---

## 2. Preconditions

This roadmap assumes the following are already in place:

* clean 3-way split:

  * `.prism` for repo-published truth
  * one explicit coordination authority backend for mutable coordination state
  * local materialization and process-local caches for fast ephemeral data
* explicit scope model:

  * `repo_id`
  * `worktree_id`
  * `branch_ref`
  * `session_id`
  * `instance_id`
* multi-workspace/worktree support in the runtime
* one MCP server can host multiple worktree contexts
* hydration/export rules remain clear and stable

If these preconditions are not yet true, they are higher priority than the optimizations below.

---

## 3. Core principles

### 3.1 Preserve local-first behavior

SQLite and local runtime behavior must remain a first-class experience.

### 3.2 Preserve repo-owned truth

Heavy optimizations must not collapse PRISM into “database as the only truth.”
Repo-quality concepts, memories, plans, and other published knowledge should remain exportable, hydratable, and branchable via `.prism`.

### 3.3 Scope before speed

Most scaling bugs are actually scope bugs:

* wrong worktree
* wrong branch
* mixed session state
* stale plan overlays
* wrong claim visibility

Correct scoping matters more than raw throughput early on.

### 3.4 Optimize for dogfooding first

The next optimization should be the one that solves the most painful observed bottleneck in real PRISM use, not the most elegant hypothetical problem.

### 3.5 Make expensive layers optional

Shallow, medium, and deep indexing should eventually become separate cost layers.

### 3.6 Push work down only when needed

Do not move complex graph logic into the database until in-process approaches clearly become the bottleneck.

---

## 4. Priority order

## Phase 1 — Foundational scaling substrate

These are the first optimizations to implement after the persistence split and multi-workspace scoping land.

### 4.1 Incremental indexing and invalidation

This should be the first major optimization.

#### Goal

Avoid re-indexing or re-projecting the whole repo for every meaningful change.

#### Why this comes first

Incremental invalidation improves:

* graph updates
* concept health refresh
* plan freshness
* worktree-local projections
* memory/concept/plan hydration repair
* active dogfooding latency

#### Requirements

* stable per-file and per-symbol fingerprints
* dependency-aware invalidation graph
* reindex only the affected delta path
* reproject only affected concepts/plans/overlays where possible
* explicit invalidation events or dirty-region markers

#### Design direction

Use a Merkle-like or lineage-aware change model:

* symbol hash changes
* file hash changes
* module/package region hash changes
* dependent projections refresh only where required

#### Acceptance criteria

* editing one file does not trigger broad repo reindex
* plan and concept health refreshes are localized where possible
* repeated incremental edits stay fast under dogfooding

---

### 4.2 Partial materialization and index-depth tiers

#### Goal

Support multiple levels of repo understanding cost.

#### Recommended tiers

* **Shallow**:

  * file existence
  * module/package boundaries
  * exports / top-level names
  * file hashes
* **Medium**:

  * symbol inventory
  * call/import/reference edges
  * concept/member attachment surfaces
* **Deep**:

  * fine-grained semantic indexing
  * detailed type/relationship extraction
  * expensive structural enrichment

#### Why this matters

PRISM should not assume every repo deserves maximum-cost indexing at all times.

#### Acceptance criteria

* the runtime can explicitly tell what depth a region currently has
* low-cost repo-wide operations do not require deep semantic indexing everywhere
* deep indexing can be applied selectively later without redesign

---

### 4.3 Explicit boundary nodes / non-materialized regions

#### Goal

Represent “known but not locally materialized” repo regions explicitly.

This matters for:

* sparse worktrees
* partial checkouts
* remote source regions
* deferred deep indexing

#### Design

Introduce explicit stub or boundary nodes rather than pretending missing regions do not exist.

A boundary node should at least carry:

* stable identity
* location / path or package marker
* source provenance
* known exports or boundaries if available
* materialization state

#### Why this matters

Agents need to know the difference between:

* nonexistent
* not yet indexed
* not currently materialized
* remotely present but outside current worktree

#### Acceptance criteria

* queries can distinguish absent from out-of-scope
* plan and impact reasoning do not silently ignore boundary regions
* sparse worktrees remain semantically legible

---

## Phase 2 — Optional coordination-backend scaling

These should follow once the foundational substrate is stable.

### 4.4 Optional service-backed coordination authority backend (e.g. Postgres)

#### Goal

Allow multiple PRISM instances or sessions to share authoritative coordination state safely through
an alternative backend implementation.

#### High-value uses

* multiple agents on one repo
* cross-machine coordination roots with service-backed authority
* CI/runtime/worker integration
* enterprise deployments
* shared maintenance queues
* cross-instance handoffs and claims

#### What belongs here

* plans and tasks
* claims and leases
* handoffs and reviews
* runtime descriptors
* retained coordination history according to the backend's contract
* maintenance queues that are part of authoritative coordination state

#### What does not move here

* repo-published truth that belongs in `.prism`
* purely local process caches
* worktree-local journals and hot materializations that do not need cross-runtime convergence

#### Required semantics

The persistence abstraction should support more than simple CRUD:

* append retained history
* compare-and-swap or revisioned mutation
* lease acquire/renew/release
* active claim scan
* stale session scan
* strong and eventual authority reads
* idempotent replay behavior

#### Acceptance criteria

* multiple PRISM instances can safely coordinate on one repo/worktree set
* claim conflicts and handoffs remain consistent
* the alternative backend can replace Git shared refs as the active authority backend for one
  coordination root without changing product semantics

---

### 4.5 Worktree-aware runtime projections

#### Goal

Keep shared repo knowledge coherent while allowing divergent worktree realities.

#### Requirements

PRISM should support:

* repo-wide published concepts and plans
* worktree-local draft overlays
* branch-specific active intent
* stale/fresh judgments tied to the actual checkout state
* claim and patch context scoped to the correct worktree

#### Acceptance criteria

* one repo with multiple worktrees behaves correctly without cross-contaminating state
* sessions can attach to the intended worktree explicitly
* worktree-local drafts do not leak into published repo truth accidentally

---

## Phase 3 — Large-repo algorithmic optimization

These should be designed for early, but implemented only when dogfooding and deployment scale justify them.

### 4.6 Lazy JIT deep parsing

#### Goal

Avoid eagerly paying the full semantic indexing cost for huge repos.

#### Strategy

Perform:

* shallow indexing repo-wide
* deeper indexing only when:

  * an agent enters a region
  * `prism_open` / `prism_locate` / deep concept decode requires it
  * a file is actively modified
  * a plan or concept health task specifically targets that region

#### Caution

Do not make the runtime unpredictably slow on first touch.

Lazy parsing is only valuable if:

* the latency is acceptable
* the cache/materialization story is good
* the runtime can explain what is shallow vs deep

#### Acceptance criteria

* deep indexing can be triggered on demand
* the system does not require full deep parsing for broad repo-wide navigation
* first-touch latency is bounded and observable

---

### 4.7 Graph centrality / architectural ranking

#### Goal

Improve retrieval and concept resolution in huge repos where lexical matching alone becomes noisy.

#### Candidate uses

* broad term resolution
* concept ranking
* architectural surface selection
* semantic blast-radius prioritization
* “top 3 most central things to inspect” behavior

#### Signals to combine

Do not rely on centrality alone.

Use a weighted combination of:

* concept quality
* relation count
* inbound dependency weight
* successful historical use
* current task proximity
* worktree proximity
* recent outcomes and plan relevance
* centrality score

#### Why not first

Centrality is powerful, but not foundational.
It should improve a good retrieval substrate, not substitute for one.

#### Acceptance criteria

* broad architectural queries improve top-1/top-3 precision in large repos
* agents receive fewer noisy candidate lists
* ranking remains explainable enough to trust

---

## Phase 4 — Database-pushed graph execution

These are late-stage optimizations for truly large runtime graphs.

### 4.8 Push heavy traversals into the database

#### Goal

Avoid loading huge graph slices into Rust memory for expensive traversals.

#### Candidate operations

* hierarchical path expansion
* concept relation walks
* semantic blast radius
* critical-path derivation over very large plan sets
* dependency closure and impact summaries

#### Near-term approach

Start with:

* relational edge tables
* recursive CTEs where useful
* materialized summary tables for common queries

#### Later options

Only if needed, evaluate:

* graph extensions
* hybrid graph-query execution
* precomputed ranked projections

#### Caution

Do not introduce graph-specific DB complexity too early.
A simpler relational model will likely go further than expected.

#### Acceptance criteria

* heavy graph queries stop being memory-bound in the MCP server
* query latency stays acceptable under large graph sizes
* the DB remains an implementation detail, not the conceptual center of PRISM

---

## 5. What to design now, but not fully build yet

The following should be explicitly designed for, but not necessarily fully implemented immediately:

* sparse worktree semantics
* remote source / boundary node identity
* index depth tiers
* query planning that can choose shallow vs deep paths
* graph ranking hooks
* DB traversal abstraction points

This means:

* choose interfaces and data models that do not block these later
* do not spend months fully implementing them until real pressure exists

---

## 6. What to postpone aggressively

Do not implement these just because they are elegant:

* full graph extensions in the database
* deep repository-wide semantic parsing for all repos
* sophisticated centrality math before ranking pain is real
* background auto-materialization of everything
* aggressive distributed coordination features before local/worktree correctness is solid

These are good later moves, not immediate roadmap requirements.

---

## 7. Recommended execution order

After the persistence split and multi-workspace scoping land, follow this order:

1. incremental indexing and invalidation
2. partial materialization / depth tiers
3. explicit boundary-node semantics
4. DB-backed coordination authority and service hardening
5. worktree-aware projections and runtime overlays
6. lazy deep parsing
7. graph centrality / large-repo ranking
8. database-pushed graph traversals

If a severe bottleneck appears earlier in dogfooding, it is acceptable to reorder a later item upward, but only with evidence.

---

## 8. Measurement before and after each optimization

Every optimization should be justified by measurement and evaluated after landing.

Track at least:

* cold-start time
* warm-start time
* incremental update latency
* concept-resolution latency
* `prism_locate` / `prism_open` latency
* plan-next / plan-expand latency
* memory use
* number of nodes/edges/materialized concepts/plans
* worktree attach latency
* DB round-trips for shared-runtime operations
* agent-facing token impact where relevant

The point is not raw speed alone.
The point is preserving the feeling that PRISM remains **fast, grounded, and orientation-giving** even as scale increases.

---

## 9. Strategic interpretation

This optimization roadmap is not just about handling bigger repos.

It is about making sure PRISM scales as:

* a local-first tool
* a multi-worktree runtime
* a multi-agent coordination substrate
* a shared enterprise system
* an architectural reasoning engine over large codebases

The main architectural sequence is:

* first, separate truth from runtime from cache
* then, separate repo scope from worktree scope from session scope
* then, make updates incremental
* then, make materialization selective
* only then optimize traversal and ranking at very large scale

That is the order most likely to preserve PRISM’s clarity while letting it grow.

---

## 10. Final principle

Do not build a billion-line optimization stack all at once.

Build the next layer that:

* already improves current dogfooding
* fits PRISM’s local-first philosophy
* keeps repo-published truth special
* makes later large-scale optimizations natural instead of desperate

The right roadmap is not:

**"implement every clever scaling trick now"**

It is:

**"build the next substrate that makes the next scale regime feel simple."**
