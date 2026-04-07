# Runtime Rewrite Architecture

Archived historical runtime architecture. The current authority and runtime split now live in:

- [PRISM_COORDINATION_TARGET_ARCHITECTURE.md](../PRISM_COORDINATION_TARGET_ARCHITECTURE.md)
- [PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md](../PRISM_FEDERATED_RUNTIME_ARCHITECTURE.md)
- [PRISM_SHARED_RUNTIME_SQLITE_REMOVAL_CONTRACT.md](../PRISM_SHARED_RUNTIME_SQLITE_REMOVAL_CONTRACT.md)

This document is the execution artifact for:

- `plan:01kn2edq9k2cr10t7p9pwscpjz`
- `coord-task:01kn2edqappsmarg40z5ta9x02`

It supersedes narrower refresh-only thinking and defines the hard target architecture for the
incremental semantic database rewrite.

This is the contract the remaining plan nodes should implement against. It is intentionally more
strict than the current codebase.

## Goal

PRISM should become a worktree-scoped incremental semantic runtime in which:

- one MCP daemon can manage many repos and many worktrees on one machine
- one `WorkspaceRuntimeEngine` owns authoritative mutable semantic state per bound worktree context
- heavy preparation and settle work run in parallel
- the authoritative commit is tiny, serialized, and explicit
- every committed change produces a monotonic delta batch suitable for replay and downstream consumers
- published generations are immutable read views with explicit per-domain freshness
- snapshots exist only for read acceleration and checkpoint recovery
- the three-plane persistence split remains intact

## Architectural Destination

The rewrite must not optimize around the current reconstructive substrate. It must replace it.

The destination is:

- incremental core:
  - long-lived mutable semantic state inside a worktree-scoped runtime engine
- published read views:
  - structurally shared immutable generations for queries and tools
- checkpointed recovery:
  - checkpoint plus committed-delta replay, not snapshot-as-authority

PRISM should stop behaving like "an index that gets rebuilt and swapped in" and start behaving like
"a long-lived semantic runtime that publishes generations."

## Three-Plane Persistence Split

The rewrite must preserve the existing three-plane split from
[PERSISTENCE_STATE_CLASSIFICATION.md](PERSISTENCE_STATE_CLASSIFICATION.md).

The three planes are:

- repo-published truth:
  - `.prism` event logs, published concepts, published memories, published plans
- shared runtime state:
  - shared mutable coordination and runtime continuity state in one backend database
- in-process runtime memory:
  - hot mutable semantic state, ephemeral caches, and published immutable read generations

The rewrite must not collapse these planes together.

In particular:

- repo truth stays repo-owned and clone-portable
- shared runtime state complements `.prism`; it does not replace it
- live request serving remains memory-authoritative while the daemon is alive

## Deployment Topology

The default deployment model is:

- one MCP daemon per machine
- one machine-local shared runtime store per machine
- one authoritative `WorkspaceRuntimeEngine` per active worktree context inside that daemon

This means:

- the daemon is machine-scoped
- the engine is worktree-scoped
- shared runtime persistence is machine-scoped by default

This is the default operating shape, not a hidden correctness assumption.

The architecture must also remain correct if multiple MCP daemons on one machine share the same
runtime store for the same repo and worktree set.

That requires:

- explicit `repo_id`, `worktree_id`, `session_id`, and `instance_id`
- revision-aware writes
- lease or heartbeat semantics where continuity ownership matters
- no singleton assumptions baked into correctness

SQLite remains the first local shared-runtime backend.

The shared-runtime backend boundary must remain backend-neutral so that Postgres can later replace
SQLite without another architecture rewrite.

That future backend swap should extend the same semantics across machines:

- same three-plane split
- same worktree-scoped engine authority
- same committed-delta replay model
- same domain freshness contract

## Existing Substrate

Some useful groundwork already exists and should be reused rather than thrown away:

- file and directory fingerprints in the workspace tree
- dirty-path tracking in the live workspace session
- file-scoped incremental refresh planning
- scoped parsing and graph updates
- basic dependency expansion for edge resolution
- incremental projection updates from lineage and outcome deltas
- memory-authoritative request serving on large parts of the runtime path
- checkpoint and materialization workers for rebuildable persisted state

The rewrite should reuse these pieces where they fit the new model, but it must not preserve the
reconstructive refresh core.

## Hard Contracts

### 1. One authoritative engine per worktree context

Each bound worktree context gets exactly one authoritative `WorkspaceRuntimeEngine`.

That engine owns:

- authoritative mutable semantic state
- authoritative commit ordering
- generation publication
- dirty-domain tracking
- queue admission policy

### 2. The actor is the commit kernel, not the compute sink

The engine actor owns:

- admission
- coalescing decisions
- authoritative state transitions
- generation publication
- commit ordering

It must not become the place where all parsing and recomputation execute.

Heavy work should run off-actor against immutable inputs and return deterministic deltas.

### 3. Parallelize everything except the commit

The target model is:

- parallel prepare
- serialized commit
- parallel settle

Preparation may include:

- file reads
- parsing
- file-local fact extraction
- invalidation planning
- delta construction

Settle work may include:

- cross-file edge resolution
- dependent recomputation
- projections
- memory reanchor
- checkpoint and materialization work

### 4. Incremental updates are the default

Broad rebuilds are recovery behavior, not normal behavior.

A small edit should:

- update only the changed file's local facts
- invalidate only the dependent regions that actually need recomputation
- publish a new generation quickly
- leave heavier downstream work to the settle path

### 5. The committed delta journal is first-class

Every authoritative commit must emit a monotonic committed delta batch.

That committed delta journal is part of the core design, not only a recovery detail.

It is the backbone for:

- replay
- checkpoints
- deterministic testing
- stale-work cancellation
- downstream side-effect consumers
- metrics and debugging
- future shared-runtime backends

### 6. Published generations are explicit read views

Published generations are immutable read surfaces, not mutable working state.

They should be:

- structurally shared where possible
- bounded in retention count
- cheap to publish
- explicit about freshness and materialization state

### 7. Freshness is domain-scoped

A generation must not only say "current generation is N."

It must say which domains are current at that generation.

Representative domains:

- file-local facts
- cross-file edges
- projections
- memory reanchor
- checkpoint state
- coordination overlays

Queries, tools, resources, and logs must be able to see these domain-scoped states.

### 8. Fast path may be conservative, never silently misleading

The fast path is allowed to publish incomplete downstream state.

It is not allowed to publish misleading certainty.

If some domains are unsettled, the runtime must expose that explicitly.

Acceptable:

- file-local facts are current
- dependent edges are pending
- projection is stale by one generation

Not acceptable:

- silently presenting stale derived state as fully current

### 9. Snapshots are demoted

Snapshots remain useful for:

- immutable read views
- query acceleration
- checkpoint recovery
- debugging and export

Snapshots must not remain:

- the authoritative mutation substrate
- the ordinary refresh substrate
- the thing that gets rebuilt and swapped in on small edits

### 10. Scope must stay explicit

The rewrite must preserve explicit boundaries between:

- repo scope
- worktree scope
- branch reality
- session scope
- process or instance scope

Running one daemon per machine must not blur these contexts together.

### 11. Multi-instance cleanliness is required even in local-first mode

One daemon per machine is the default deployment shape.

It is not the correctness model.

The runtime and shared backend must stay valid under:

- crash and restart handoff
- daemon upgrades
- accidental double-launches
- tests that simulate local contention
- future Postgres-backed multi-instance or cross-machine serving

If a design shortcut only works because exactly one daemon happens to be alive, it is not an
acceptable contract for this rewrite.

## Runtime Objects

The runtime should standardize on these core objects.

### WorkspaceRuntimeEngine

Owns authoritative mutable state for one worktree context.

### Working generation

The current mutable semantic state inside the engine before and during incoming commits.

### Published generation

The immutable read generation currently exposed to queries and tools.

### Settle frontier

The newest generation up to which background settle work has completed for each domain.

### Checkpoint generation

The newest generation durably checkpointed for recovery acceleration.

### Committed delta batch

A monotonic journal entry representing one authoritative committed state transition.

### Dirty domains

The domains that are known to need prepare or settle work.

## Query And Freshness Contract

Every published generation should carry:

- generation id
- parent generation or predecessor marker
- commit timestamp or sequence
- domain freshness state
- materialization state
- worktree context identity

Minimum freshness states should preserve these semantics:

- current
- pending
- stale
- recovery
- shallow
- medium
- deep
- known_unmaterialized
- out_of_scope

The exact enum names may differ, but the semantics must survive.

## Queue Model

The engine queue model must support explicit priority classes from the beginning.

Minimum conceptual classes:

- interactive edit / immediate mutation
- follow-up mutation
- fast semantic preparation
- settle work
- checkpoint and materialization work

The queue model must also support:

- coalescing repeated writes to the same file
- cancellation of stale prepare work
- cancellation of stale settle work
- bounded waiting for interactive mutations

## Shared Runtime Backend Contract

The shared-runtime backend interface must be semantic, not SQLite-shaped.

Important capabilities:

- append committed event or delta batch
- read ordered event or delta streams
- perform revision-aware or compare-and-swap mutation
- acquire, renew, and release lease
- list scoped worktree or repo runtime state
- record heartbeat and scan stale instances or sessions
- poll or subscribe for relevant updates

The local SQLite implementation may realize these capabilities differently than a future Postgres
implementation, but the runtime contract should be written in terms of these semantics rather than
in terms of concrete SQLite APIs.

## Commit Pipeline

The healthy pipeline is:

1. intake commands for one worktree context
2. coalesce and prioritize them
3. run file-local and invalidation preparation in parallel
4. build deterministic candidate deltas
5. enter a tiny serialized commit
6. apply authoritative deltas to mutable state
7. append a committed delta batch
8. publish a new immutable generation
9. schedule parallel settle and materialization work

The commit section should do the minimum necessary to make the runtime current.

It must not block on:

- broad recomputation
- checkpoint writes
- memory reanchor
- projection materialization
- non-authoritative follow-up work

## Hot, Warm, And Cold Runtime State

The runtime must stay tiered.

### Hot

Always-ready serving state:

- current file-local facts for active regions
- current published generation
- bounded serving projections
- active coordination overlays
- current worktree freshness state

### Warm

Cheap lazy-read state:

- recent outcomes
- recent lineage windows
- task-local slices
- medium-depth active-region details

### Cold

Durable evidence not kept on the hot path:

- full lineage history
- full outcome event history
- analytical evidence
- old snapshots and exports

## Invalidation And Granularity

Per-file semantic facts are the correct starting unit for this rewrite.

That does not mean the architecture should hard-code "file" as the smallest unit forever.

The design must leave room for future refinement such as:

- region-level facts
- symbol-local invalidation
- large-file subdivision

The immediate rewrite should still implement:

- explicit per-file fact ownership
- reverse dependency indexes
- precise dependent invalidation

## Performance Budgets

These are architecture-level target budgets for the PRISM dogfood workflow.

### Tiny edit fast path

- enqueue-to-published-generation p95: `<= 50ms`

### Interactive mutation admission

- queue wait before commit start p95 under nominal interactive load: `<= 25ms`

### Direct dependent settle

- changed file to direct dependent settle p95 for routine small edits: `<= 200ms`

### Checkpoint and materialization lag

- visible but off hot path
- normal interactive lag target p95: `<= 5s`

### Generation retention

- published generations must remain structurally shared and explicitly bounded
- retained published generation window target: `<= 3`

### Memory budgeting

Budgets must remain explicit for:

- hot semantic facts
- published generations
- queued prepare work
- queued settle work
- checkpoint materialization backlog

## Recovery Contract

Recovery must be:

- checkpoint plus replay
- driven by committed delta batches
- bounded in replay cost
- compatible with the three-plane persistence split

The local shared-runtime store is the first backend that must satisfy this contract.

That same contract should later support Postgres without changing the runtime semantics.

## Explicit Non-Goals

This migration does not include:

- multi-writer shared mutable semantic state
- database-pushed graph traversal as the primary runtime substrate
- snapshot-centric serving
- per-worktree daemon sprawl as the required deployment model
- remote backend implementation work in this phase

However, the migration must leave behind:

- a backend-neutral shared-runtime store seam
- a replayable committed-delta model
- clear scope identity

Those are prerequisites for future shared-runtime backends.

## Migration Sequence

The rewrite should proceed in this order:

1. codify runtime generations, domain freshness, queue classes, three-plane boundaries, and budgets
2. preserve the machine-scoped host plus shared-runtime-store model explicitly
3. introduce the worktree-scoped `WorkspaceRuntimeEngine`
4. separate mutable engine state from published immutable generations
5. remove reconstructive refresh and live-state indexer rebuilds from ordinary edits
6. establish per-file semantic facts and committed delta batches as the ordinary substrate
7. add precise invalidation and reverse dependency indexes
8. parallelize file-local preparation and batch coalescing
9. implement the tiny fast-path commit and domain-scoped freshness publication
10. add parallel settle workers and stale-work cancellation
11. move non-authoritative side effects fully behind the commit
12. replace fail-fast lock UX with engine-queued admission
13. complete checkpoint-plus-replay recovery on top of the new runtime core
14. validate budgets, freshness visibility, multi-worktree shared-runtime behavior, and backsliding prevention

## Acceptance Criteria For This Node

This node is complete when:

- the machine-scoped host plus worktree-scoped engine model is explicit
- the three-plane persistence split is explicit
- runtime generations, committed delta batches, and domain freshness are explicit
- the actor's role as commit kernel is explicit
- priority queues, coalescing, and cancellation are explicit
- hard interactivity and memory budgets are explicit
- later plan nodes can implement against this document without re-deciding the architecture ad hoc
