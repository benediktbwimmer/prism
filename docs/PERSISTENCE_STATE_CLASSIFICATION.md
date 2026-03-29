# Persistence State Classification

This document is the classification artifact for `plan:1` / `coord-task:1`.

Its job is to distinguish authoritative persisted state from derived, cache, export, and compatibility artifacts so the remaining persistence migration does not recreate dual truth between native plans and legacy snapshot-shaped flows.

This is a target classification for the migration, not an assertion that every current API boundary already matches the target shape.

## Three State Planes

PRISM should converge on three distinct state planes:

- Repo tree / `.prism`: published truth the repo owns and carries across clones
- Shared backend database: shared mutable runtime state for live multi-session and multi-agent coordination
- In-process memory/cache: fast local ephemeral state and convenience materializations

The most important design rule is that a shared database complements `.prism`; it does not replace it.

## Rules

- Authoritative persisted state is the durable truth the runtime should reconstruct from.
- Derived state may be dropped and rebuilt from authoritative state without semantic loss.
- Compatibility projections may remain useful, but they are not allowed to become the sole write truth.
- Runtime-only hydration details such as fresh handles, resolved overlays, or process-local caches must not be published as authored repo truth.
- Snapshots and compaction outputs are allowed as accelerators, bootstrap aids, or exports, but not as a second semantic authority.
- A shared backend should be treated as the mutable runtime state plane for collaboration, not as a replacement for repo-published knowledge.

## Classification

| Domain | Authoritative persisted state | Derived / compatibility / export state | Notes |
| --- | --- | --- | --- |
| Structural graph and file state | Durable graph/file state and raw observed changes owned by `prism-store` | Replaced derived edges, convenience materializations, one-shot bootstrap views | Structure remains the repo/runtime authority. Derived edges stay additive. |
| Lineage and temporal identity | Durable lineage history, tombstones, and identity assignment state owned by `prism-history` | Co-change counts, lineage summaries, replay-oriented snapshots | Temporal identity is authoritative; co-change and similar aggregates are rebuildable. |
| Outcomes and workspace memory | Outcome events, append-only memory events, and published repo memory events | Episodic snapshots, recall indexes, fuzzy recall helpers, hydrated memory views | Event history is authoritative; snapshot forms are acceleration or hydration aids. |
| Curated repo knowledge | Published concept events, concept-relation events, and repo memory events under `.prism/` | Hydrated concept packets, decode lenses, search bundles, curator convenience views | Published repo knowledge travels by event log, not by hydrated packet shape. |
| Native plans and authored plan bindings | `.prism/plans/index.jsonl` plus per-plan event logs; authored plan metadata; authored node fields; authored edges; stable published refs in bindings | Hydrated plan graphs, runtime binding overlays, compatibility task projections, `planSummary`, `planNext`, compact task guidance, plan resource views | For plans, authored intent is authoritative. Hydrated handles and runtime rebinding results are runtime-only. |
| Shared workflow continuity | Durable claim, artifact, review, handoff, and policy-relevant continuity state | Blocker summaries, inbox/task context views, conflict summaries, risk hints | Continuity that changes completion or contention semantics is authoritative; summaries are not. |
| Projections and read models | None by default; these are derived from authoritative state | Projection snapshots, co-change neighbors, validation deltas, query-oriented summaries, recommendation frontiers, compatibility read models | If a projection is rebuildable from authoritative events/state, it is not a write authority. |
| Snapshots, compaction, and exports | None by default; these remain derived | `GraphSnapshot`, `HistorySnapshot`, `OutcomeMemorySnapshot`, `ProjectionSnapshot`, `CoordinationSnapshot`, episodic/inference/curator snapshots, deterministic per-plan compaction outputs, export artifacts | Snapshots may accelerate reload or export state, but replay/event-backed state remains canonical. |

## Shared Runtime Backend

When PRISM uses a shared backend such as Postgres, the backend should primarily own shared mutable runtime state such as:

- active claims and leases
- handoffs and live workflow continuity
- live plan materializations and execution overlays
- review and validation overlays
- maintenance queues and shared runtime telemetry
- session continuity across restarts or relocations
- ephemeral draft coordination state before publication

That backend should not become the only home for repo-quality concepts, memories, or published plans. Those still belong in `.prism`.

SQLite remains a good fit for:

- local-first and offline operation
- single-user or single-machine work
- simple dogfooding and OSS adoption

A shared backend becomes valuable for:

- multi-agent collaboration across machines
- multiple Prism MCP runtimes sharing one live coordination state
- CI or automation-attached runtimes contributing live outcomes
- longer-lived organizational deployments where local-only runtime state is too fragmented

## Distributed Runtime Capabilities

The long-term backend abstraction should be shaped around coordination capabilities and semantics, not just generic CRUD.

Important examples:

- append event
- read event stream
- revision-aware or compare-and-swap mutation
- acquire, renew, and release lease
- record outcome
- materialize or refresh projection
- scan stale sessions
- query active claims and handoffs
- poll or subscribe for changes

This also means the persistence plan must account for:

- optimistic concurrency
- idempotent event handling
- lease and heartbeat renewal
- stale session cleanup
- repo, branch, and worktree identity
- latency tolerance and reconnect behavior
- partial-failure handling

## Boundary Guidance

Good candidates for the shared remote backend:

- session state
- active claims
- handoffs
- live plan materializations
- review and validation overlays
- maintenance queues
- ephemeral draft plans or concepts before publication
- live coordination telemetry

State that should remain repo-published in `.prism`:

- repo-quality concepts
- repo-quality memories
- repo-quality plans and intent
- durable learned knowledge the repo should carry with it

State that should remain process-local cache:

- transient lookup caches
- hot derived projections
- local memoization
- UI or session convenience state

## Current Transitional Caveats

The current store boundary is still snapshot-shaped in several places. In particular, [`crates/prism-store/src/store.rs`](/Users/bene/code/prism/crates/prism-store/src/store.rs) still exposes load/save methods for `HistorySnapshot`, `OutcomeMemorySnapshot`, `ProjectionSnapshot`, `CoordinationSnapshot`, and other snapshot forms.

That does not make those snapshots the desired long-term authority.

During the migration:

- do not add new features that depend on snapshots as the only semantic write truth
- do not treat compatibility task projections as the plan-system authority
- keep new persistence logic centered on authoritative events or normalized state, then derive snapshots or projections from there

## Immediate Guidance For The Remaining Persistence Plan

- `coord-task:7` should introduce backend-neutral interfaces for authoritative state and optional snapshot or compaction loaders, not SQLite-shaped snapshot authority.
- `coord-task:2` should move native plan and coordination writes onto authoritative event-backed or normalized persistence paths, with revision-aware and idempotent mutation semantics suitable for shared runtime backends.
- `coord-task:3` should maintain projections, summaries, recommendations, compatibility views, and shared runtime read models incrementally from authoritative state.
- `coord-task:4` should hydrate runtime state from authoritative state first, then perform rebinding, runtime overlay attachment, and explicit repo/branch/worktree identity handling.
- `coord-task:5` should keep compaction and snapshots explicitly derived from canonical history.
- `coord-task:6` should validate multi-instance concurrency, leases or heartbeats, stale-session cleanup, reconnect behavior, and migration safety in addition to correctness and latency.

## Plans-Specific Interpretation

For first-class plans, the key distinction is:

- authoritative: authored plan intent, durable workflow continuity, stable published refs
- derived: hydrated graph materializations, runtime rebinding results, compatibility task views, summaries, recommendations, snapshots

That means repo-wide graph version drift or runtime convenience data must not be confused with authored plan truth.
