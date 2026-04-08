# PRISM Cross-Repo Future

Status: forward-looking design note  
Audience: PRISM core, coordination, runtime, storage, MCP, and query maintainers  
Scope: project-scoped published knowledge, cross-repo coordination, configured authority backends,
and local materialization and binding strategy

---

## 1. Summary

PRISM should eventually support **cross-repo concepts, contracts, plans, and memories** without abandoning the core rule that published knowledge should be explicit, durable, reviewable, portable, and still grounded in git-backed artifacts.

The central design decision is:

- **repo-local published truth** continues to live in each code repo's `.prism/`
- **project-level published cross-repo truth** should live in a dedicated **project git repo**
- **cross-repo coordination authority** should live behind one configured backend for the project
  coordination root
- **worktree-local hot acceleration** remains local and rebuildable

This means PRISM grows by **adding a project scope**, not by collapsing all truth into a mutable database or inventing a vague global mode.

This should read as a direct architecture extension of PRISM's current model, not as a parallel side system:

- published truth stays explicit and git-backed
- coordination authority stays explicit and backend-defined
- rebuildable acceleration stays disposable

Local SQLite and runtime-local state are still important, but their role is different:

- local materialized coordination snapshots
- local checkout and worktree bindings
- startup checkpoints
- draft cross-repo relations and concepts before promotion
- runtime projections and query acceleration

They must **not** become the only home of durable published system knowledge or authoritative coordination state.

The long-term architecture is therefore:

1. **Worktree scope** for local checkout reality and rebuildable acceleration
2. **Repo scope** for published repo truth
3. **Project scope** for published cross-repo truth
4. **Global import / external scope** for non-canonical imported evidence

With that scope model in place, local runtime state remains an accelerator and binding layer rather than becoming a fifth published scope.

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
- Keep coordination authority behind one explicit backend per coordination root.
- Keep local SQLite and runtime state in a non-authoritative materialization role.
- Support one machine-level daemon managing many repos and worktrees.
- Support future multi-machine coordination without redefining the data model.
- Avoid blurring repo-local truth, project truth, configured coordination authority, local
  bindings, and rebuildable caches.

### 3.2 Non-goals

- Do not move repo-local published truth out of code repos.
- Do not make local SQLite or any future backend the sole home of durable cross-repo truth.
- Do not duplicate all repo-local concepts and contracts into the project repo.
- Do not require one database per worktree.
- Do not require a remote backend in the first implementation.
- Do not make project scope mandatory for all repos.

---

## 4. Core design decisions

### 4.1 Add a project scope above repo scope

PRISM should gain a first-class **project scope** for workflows and knowledge that genuinely span multiple repos.

That keeps cross-repo work disciplined as a new scope layer rather than turning it into an undifferentiated global namespace.

A project may include:

- zero or more repos
- system-level concepts
- cross-repo contracts
- cross-repo plans
- promoted cross-repo memories
A repo may participate in zero or one project in the simple initial model.

### 4.2 Use a project git repo for published cross-repo knowledge

Published cross-repo knowledge should live in a dedicated **project git repo**.

This is the best long-term home because it preserves the same properties that make repo-local `.prism/` valuable today:

- reviewable
- diffable
- branchable
- cloneable
- forkable
- portable
- explicitly promoted
- human-legible
- inspectable by both humans and agents

It also lets published cross-repo truth inherit naturally through normal git workflows such as clone, fork, and branch, instead of depending on whichever backend happens to be live today.

The project repo is the cross-repo equivalent of a code repo's local `.prism/`.

### 4.3 Use one configured authority backend per project coordination root

For cross-repo coordination, the project must have one explicit coordination root and one active
authority backend.

That means:

- project-scoped plans, tasks, claims, artifacts, and reviews are committed through one authority
  backend for that root
- Git shared refs are the initial and default backend
- PostgreSQL may later be added as an alternative authority backend with the same coordination
  semantics
- compare-and-swap, replay, and semantic merge remain backend-defined implementations of one
  coordination conflict contract
- local SQLite and runtime state materialize committed authority results for fast reads and restart
  acceleration
- machine-local checkout bindings remain local and non-authoritative

Cross-repo coordination should therefore extend the existing coordination semantics rather than
introducing a second mutable authority plane.

### 4.4 Preserve worktree-scoped hot runtime engines

Even with project scope, the live serving model remains:

- one `WorkspaceRuntimeEngine` per worktree/workspace context
- optional project-scoped coordination state materialized above repo scope
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
- repo-scoped local materialization and runtime bindings on the local machine
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

## 6. Published truth vs coordination authority vs local acceleration

The most important architectural rule is to keep these separate.

In compact form:

- published truth is durable and git-backed
- coordination authority is durable and git-backed
- local runtime state is operational and provisional
- derived acceleration is disposable

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

### 6.2 Coordination authority

Authoritative coordination truth is:

- durable
- explicit
- git-backed
- subject to compare-and-swap, replay, and semantic merge
- the correctness plane for claims, leases, tasks, artifacts, and reviews

For cross-repo coordination, this belongs in the project repo shared coordination refs.

### 6.3 Local runtime state and materialization

Local runtime state is:

- operational
- provisional
- path-sensitive
- machine-local
- non-authoritative
- allowed to lag and recover

This belongs in worktree-local runtime state and SQLite materialization.

### 6.4 Derived acceleration

Derived acceleration is:

- rebuildable
- query-oriented
- non-authoritative
- safe to delete

This belongs in worktree-local materializations, not in the published artifact plane.

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

### 7.3 In local runtime state and SQLite

Local runtime state and SQLite should hold:

- materialized project coordination snapshots loaded from shared refs
- unpublished cross-repo relations
- draft project concepts and contracts before promotion
- local projections and query indexes
- machine-local project and repo binding state
- restart checkpoints and read-optimized indexes

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
- authoritative coordination state in the project repo shared refs, with local materialized views

The important implementation rule is that a cross-repo plan node should not pretend to be one
global repo-less task. It should be a project-scoped execution object that can:

- reference project-scoped concepts, contracts, and acceptance criteria
- bind to zero or more repo-local anchors
- fan out into repo-local work items, validations, or review artifacts when execution begins
- aggregate progress from those repo-local bindings back into one project-scoped execution view

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

1. **runtime working relations** in local unpublished state
2. **published curated relations** in the project repo

That lets PRISM support the normal lifecycle:

- discover relation
- test and refine relation
- publish relation when stable enough to be part of durable system knowledge

### 9.4 Canonical cross-repo reference model

The hardest implementation problem is not storage. It is reference stability across decoupled repos
and branches.

Project-published artifacts therefore need a first-class reference object for repo-local anchors.

A good initial shape is:

```text
CrossRepoAnchorRef {
  project_id
  repo_id
  anchor_kind
  logical_anchor
  lineage_id?
  path_hint?
  branch_hint?
  commit_hint?
}
```

Where:

- `repo_id` identifies the member repo that owns the anchor
- `anchor_kind` distinguishes symbol, file, concept, contract, validation, or plan targets
- `logical_anchor` is the semantic identity to resolve first
- `lineage_id` is used when PRISM already has a durable lineage handle
- `path_hint` helps localize resolution and explain drift
- `branch_hint` or `commit_hint` are optional diagnostics, not the primary identity

The key rule is:

- prefer semantic identity first
- use path and revision as hints
- never make a project artifact depend only on raw line numbers in another repo

`repo_id` is therefore a critical primitive, not an incidental label. It should identify the logical member repo rather than one local checkout.

That means the identity policy should aim to keep one `repo_id` stable across:

- multiple local clones of the same repo
- multiple worktrees of the same clone
- checkout path changes on one machine
- partial local discovery where only some member repos are currently bound
- normalized remote URL changes or aliases when the underlying repo is still the same published member

Forks should default to distinct `repo_id` values unless the project publishes an explicit relationship saying otherwise.

### 9.5 Resolution contract for cross-repo anchors

Cross-repo anchor resolution should be explicit and degradable.

The runtime should be able to resolve a `CrossRepoAnchorRef` into states such as:

- `exact`
- `remapped`
- `stale`
- `missing`
- `ambiguous`

That matters because project and member repos will drift independently in normal development.

The UX contract should be:

- project-published truth remains readable even when some bindings drift
- unresolved bindings are reported clearly instead of silently disappearing
- degraded bindings explain why they degraded, not only that they degraded
- degraded bindings point at the next action needed, such as rebinding, accepting a remap, or checking out the missing repo
- plan readiness, validation, and claim logic can treat stale bindings as blockers when policy
  requires it
- rebinding is an explicit runtime or promotion-time action, not an invisible mutation of
  published history

The runtime should be able to explain whether drift came from:

- a repo that is not currently bound on this machine
- descriptor mismatch or repo identity ambiguity
- anchor movement or rename with a credible remap candidate
- branch or revision divergence
- multiple plausible candidates in the target repo

PRISM should also distinguish **artifact semantic validity** from **current binding health**.

A project concept, contract, or plan can remain semantically valid even while one or more concrete bindings are stale. Binding degradation should be treated as an explicit maintenance condition, not as automatic invalidation of the published truth.

### 9.6 Do not flatten everything into one global graph root

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
- local materialized coordination and overlays
- worktree-local freshness and branch-sensitive hot state

Conceptually, the read stack becomes:

1. **project-published truth**
2. **repo-published truth**
3. **local materialized coordination overlays**
4. **worktree-local freshness and acceleration**

The ordering of those layers matters:

- published truth remains the stable base
- local runtime state provides current working overlays
- worktree-local state provides immediacy and speed

This is the same pattern PRISM already uses locally, extended upward to project scope.

---

## 11. Coordination model

Cross-repo coordination should follow the same principles PRISM already uses inside one repo.

The project coordination backend plus local materialization should support:

- project-scoped execution leases
- cross-repo handoffs
- blockers spanning repos
- project-scoped review artifacts
- multi-repo execution overlays
- cross-repo task assignment and reclaim semantics

### 11.1 Separate project execution ownership from repo anchor ownership

PRISM should not collapse project coordination authority and repo-local anchor ownership into one
scope.

The clean split is:

- project scope owns execution leases for project plan nodes and cross-repo work packets
- repo scope owns concrete claims on repo-local files, symbols, validations, and artifacts

That means a project-scoped task may coordinate work across many repos, but each repo-local edit
claim still belongs to the repo that owns the anchor.

This preserves scope discipline and avoids inventing a muddy global lock namespace.

### 11.2 Project nodes can aggregate repo-local claims

A project-scoped execution node should be able to point at:

- zero or more repo-local claims
- zero or more repo-local validation runs
- zero or more repo-local review artifacts

The project layer then answers questions such as:

- which repo-local claims are active under this cross-repo task
- which downstream repos are blocked
- whether the project node can advance
- whether stale repo-local bindings should trigger reclaim or replan behavior

The project repo is the place for:

- published plans
- published concepts
- published contracts
- promoted memories
- authoritative cross-repo coordination state

Local runtime state is the place for:

- active machine-local materialization
- stale or partial checkout bindings
- draft execution projections
- read-optimized indexes and checkpoints

---

## 12. Relationship to local materialization

### 12.1 Local-first shape

The first useful shape is:

- one PRISM daemon per machine as the normal operating form
- worktree-local SQLite stores for hot runtime state
- shared coordination refs plus runtime descriptors for continuity
- optional project-scoped coordination materialization above repo scope
- one `WorkspaceRuntimeEngine` per worktree/workspace internally

### 12.2 Future remote shape

The future remote shape should preserve the same semantics while swapping only acceleration or replication details.

That means:

- SQLite locally first
- same scope model
- same identities
- same event and mutation model
- same distinction between configured coordination authority and local overlays

Any future remote layer should not redefine the architecture. It should only change how acceleration, replication, or discovery is provided.

### 12.3 What any future remote layer is for

A future remote layer is a candidate place for:

- cross-machine replication or fetch acceleration
- project membership discovery
- remote cache distribution
- shared query acceleration

It is **not** the authority backend for published project truth or authoritative coordination
state.

That remains the configured project coordination backend, with the project repo plus Git shared refs
as the initial/default shape.

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
  workspace.prism.toml
```

Optional future additions may include:

- project-level repo descriptors
- system ownership manifests
- import or benchmark metadata that is safe to publish

The important rule is to keep the project repo focused on **published system knowledge**, not on logs, caches, or runtime state.

### 13.1 Published membership manifest plus local checkout bindings

Project membership should be modeled in two places, with different responsibilities.

Published in the project repo:

- `workspace.prism.toml` is the canonical declaration of project membership
- it lists member repos and stable descriptors for them
- it defines the published project identity

Stored in local runtime state:

- machine-local bindings from those repo descriptors to actual checkout paths
- local discovery metadata such as `last_seen`, branch, health, and resolution failures
- temporary or incomplete bindings while a machine only has some repos checked out

The project repo should answer "what belongs to this project?"

The local runtime should answer "where are those repos on this machine right now?"

The published manifest and runtime bindings should work together to preserve repo identity discipline:

- the manifest publishes the stable logical member set
- runtime bindings attach zero or more local checkouts or worktrees to one published `repo_id`
- worktrees never get their own repo identity
- partial checkout is normal, not an error condition

### 13.2 Suggested manifest shape

The exact TOML schema can evolve, but the first version should stay small and explicit.

```toml
[project]
id = "project:search-platform"
name = "Search Platform"

[[repos]]
repo_id = "repo:service-api"
remote = "git@github.com:org/service-api.git"
hosting_id = "github:org/service-api"
role = "producer"

[[repos]]
repo_id = "repo:typescript-sdk"
remote = "git@github.com:org/typescript-sdk.git"
hosting_id = "github:org/typescript-sdk"
role = "consumer"
```

The manifest should not try to mirror runtime truth such as local checkout paths, active leases, or
freshness state.

It should, however, provide enough stable descriptor material for runtime binding to survive common clone and remote variations without changing the published `repo_id`.

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
- checked for whether each anchor binding is exact, remapped, stale, missing, or ambiguous

### 14.3 Bind and validate

Before promotion, the runtime should validate the cross-repo bindings explicitly.

That means:

- resolve each referenced repo descriptor to a member repo
- resolve each `CrossRepoAnchorRef`
- record resolution state for each binding
- determine the artifact-level binding-validation posture for the promotion candidate
- reject or downgrade promotion when policy requires exact bindings and the runtime only has stale
  or ambiguous ones

### 14.4 Promote

If it is stable enough to be part of durable system knowledge, it is promoted into the project repo.

Promotion should record not only the artifact contents, but also the binding-validation posture under which it was published.

Examples of that posture include:

- published with all bindings exact
- published with degraded bindings explicitly accepted
- published with unresolved bindings intentionally allowed by policy

That metadata matters later when someone asks whether a project artifact is trustworthy but needs binding maintenance, or whether it was knowingly published in a degraded state from the start.

### 14.5 Reproject

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

During active execution, local runtime state would hold:

- who currently owns each step
- which repos are blocked
- active claims and leases
- draft rollout notes
- discovered affected validations
- runtime projections showing current progress and stale work

That is the exact split PRISM should aim for:

- published system truth in the project repo
- authoritative coordination state in the project repo shared refs
- local materialized coordination and checkout bindings on each machine
- repo-local implementation truth in each code repo

---

## 16. Coordination-only v1 release

Cross-repo coordination-only support is a plausible PRISM v1 feature if it is scoped tightly.

The goal of this slice is:

- support cross-repo plans, tasks, claims, artifacts, and reviews
- keep authority in one configured backend for the project coordination root
- keep local SQLite and runtime state purely as materialization and binding layers
- defer project-scoped concepts, contracts, and memories to a later release

### 16.1 v1 release stance

For v1:

- one umbrella or project repo is the coordination root
- the initial authoritative backend is Git shared refs
- every cross-repo coordination record is explicitly scope-qualified
- artifacts are repo-qualified evidence pointers
- repo-local claims remain repo-qualified rather than becoming one giant global lock space
- local runtime state knows both:
  - the coordination root
  - the local execution repo or worktree currently being worked in

This is intentionally narrower than the full project-knowledge future. It is coordination first.

### 16.2 Concrete implementation steps for coordination-only v1

The minimum viable implementation should do these things explicitly:

1. Add `project_id` and stable logical `repo_id` support to the coordination model where scope matters.
2. Introduce a project or umbrella repo as the explicit coordination root for cross-repo sessions.
3. Publish a small project membership manifest in the project repo:
   - project id
   - member repos
   - stable repo descriptors
   - stable `repo_id` values
4. Add machine-local bindings from published `repo_id` values to checkout and worktree paths.
5. Extend artifacts so code and validation evidence can carry repo-qualified pointers such as:
   - `repo_id`
   - commit sha
   - commit range
   - ref
   - optional path or module hints
6. Make project-scoped tasks able to point to repo-qualified work ownership and repo-local follow-up work.
7. Keep concrete edit claims repo-qualified, even when the owning project task is cross-repo.
8. Materialize committed project coordination state into local SQLite for fast reads and restart,
   without making SQLite authoritative.
9. Make MCP and query surfaces expose both:
   - the coordination root
   - the local execution repo binding
10. Reuse the existing shared-coordination conflict contract for project-scoped coordination writes.

### 16.3 What v1 deliberately does not need

Coordination-only v1 does not need:

- project-scoped concepts
- project-scoped contracts
- project-scoped memories
- full cross-repo cognition
- a remote shared database
- globally scoped edit claims

### 16.4 Why this is low-risk

This slice is relatively low-risk because the current architecture already aligns with it:

- coordination authority already lives in shared refs
- local SQLite is already non-authoritative
- the graph is already small and execution-focused
- claims, artifacts, reviews, and blockers are already explicit coordination records

The main risk is not transport or storage. It is identity sloppiness.

So the v1 rule should be:

- do not ship cross-repo coordination without stable `project_id`, stable `repo_id`, a published membership manifest, and machine-local repo bindings

---

## 17. Longer-term migration strategy

PRISM should reach this future in phases.

### Phase 1 — scope model only

Add first-class support in the data model for:

- `project_id`
- `repo_id`
- `worktree_id`
- project-scoped entities and relations
- repo membership in a project

No mandatory project repo yet.

Concrete outcomes for this phase:

- add `project_id` to runtime context, persistence context, and event metadata where scope matters
- define project-scoped ids and entity kinds
- add a local representation for project membership and checkout bindings
- define the initial `repo_id` stability policy before cross-repo truth is published
- keep all cross-repo behavior runtime-only until the identity model is stable

### Phase 2 — coordination-only cross-repo release

Support project-scoped cross-repo coordination through the initial Git shared-refs backend plus
local materialization:

- project-scoped plans, tasks, claims, artifacts, and reviews
- project-scoped execution leases, handoffs, and overlays materialized locally from shared refs
- repo-local claims attached under project execution nodes
- cross-repo anchor resolution with explicit resolution states
- project membership manifest plus machine-local checkout bindings

This proves the behavior before project-scoped knowledge is added.

### Phase 3 — project repo as published cross-repo plane

Introduce the project git repo and promotion flows for:

- project concepts
- project contracts
- project plans
- project memories
- `workspace.prism.toml` as the published membership manifest
- explicit binding-validation posture on promoted cross-repo artifacts

### Phase 4 — integrated query and promotion workflows

Unify project queries across:

- project repo truth
- member repo truth
- runtime overlays
- worktree-local freshness

At this point, PRISM should be able to:

- explain why a cross-repo binding is stale or ambiguous
- distinguish semantic truth validity from current binding health
- show project execution state and linked repo-local claims together
- promote runtime-discovered relations into published project artifacts
- degrade gracefully when only part of the project is checked out locally

### Phase 5 — optional remote acceleration

Add remote acceleration or replication only if needed, without changing the authority model.

---

## 18. Rules to protect

The following rules should remain strict.

### 18.1 Do not put all cross-repo truth only in local SQLite or any backend

Local SQLite and any future backend are the right home for acceleration and materialization, not the only home of durable published system knowledge or authoritative coordination state.

### 18.2 Do not copy every repo-local fact into the project repo

The project repo should hold system-level truths, not a giant duplication of member repos.

### 18.3 Keep promotion explicit

Published project knowledge should be reviewed and promoted intentionally.

### 18.4 Keep local overlays mutable and operational

Leases, claims, blockers, and execution overlays may be materialized locally for speed, but the authoritative coordination facts for the project belong in the project repo shared refs.

### 18.5 Keep scope explicit in the data model

Filesystem placement is helpful, but `project_id`, `repo_id`, and `worktree_id` must remain first-class semantic scope fields.

---

## 19. Open design questions

These questions do not block the direction, but they should be answered before implementation hardens.

- When should a relation stay runtime-only versus being promoted to project truth?
- Should project-scoped concepts and contracts use the exact same artifact schema as repo-scoped ones, or an extended variant?
- How much cross-repo projection should be eagerly materialized versus computed on demand?
- Should project scope initially support exactly one project per repo, or allow many-to-many later?
- Which published descriptors are sufficient for `repo_id` stability across clone, worktree, remote, and fork realities?
- Which binding-validation posture values should be first-class in published artifacts?

Questions with a recommended default answer now:

- Project membership should be published in the project repo and resolved into machine-local checkout
  bindings in runtime state.
- The canonical cross-repo reference syntax should be a repo-qualified semantic anchor reference,
  not raw file-line pointers.
- Project scope should own execution leases, while repo scope should continue to own concrete anchor
  claims.
- `repo_id` should identify a logical member repo rather than a checkout, and forks should default
  to distinct identities unless the project publishes an explicit relationship.
- Published cross-repo artifacts should record their binding-validation posture explicitly, and
  binding health should not silently redefine semantic truth validity.

---

## 20. Recommended architectural stance

The recommended stance is:

- **code repos** own local published truth
- the **project repo** owns published cross-repo truth
- the **configured project coordination backend** owns authoritative cross-repo coordination state
- **worktree-local state and SQLite** own rebuildable hot acceleration, materialization, and checkout bindings

This gives PRISM the best of both worlds:

- durable, reviewable, portable knowledge in git
- one clear authority backend for coordination
- fast local reads without authority confusion
- a real future for cross-repo plans, concepts, contracts, and memories

---

## 21. End-state vision

The long-term win is not merely "PRISM works across more files."

The win is:

**PRISM becomes a system-level orientation, memory, and coordination substrate for software that spans many repositories.**

In that world:

- each repo keeps its own grounded local truth
- the project repo captures durable system truth above repo boundaries
- the project repo shared refs carry authoritative cross-repo coordination truth
- local runtimes materialize that truth and bind it to machine-local checkouts
- agents can navigate the system by concepts first, repo symbols second, and raw text search third

That is the cross-repo future worth building.
