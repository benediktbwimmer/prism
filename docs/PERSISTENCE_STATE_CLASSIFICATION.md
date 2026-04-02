# Persistence State Classification

This document is the classification artifact for `plan:01kn13pqvcx3xmnkrs493ff3ra` /
`coord-task:01kn13pz030s4q1c3qrbbg203f`.

Its job is to distinguish authoritative persisted state from derived, cache, export, and compatibility artifacts so the remaining persistence migration does not recreate dual truth between native plans and legacy snapshot-shaped flows.

This is a target classification and migration contract, not an assertion that every current
API boundary already matches the target shape.

## Three State Planes

PRISM should converge on three distinct state planes:

- Repo tree / `.prism`: published truth the repo owns and carries across clones
- Shared backend database: shared mutable runtime state for live multi-session and multi-agent coordination
- In-process memory/cache: fast local ephemeral state and convenience materializations

The most important design rule is that a shared database complements `.prism`; it does not replace it.

Within the shared backend plane, PRISM should distinguish two very different roles:

- synchronous authoritative journals for crash-sensitive mutable facts
- asynchronous checkpoints and materialized views for restart speed and cold query support

PRISM should therefore evolve from "local-first only" into "local-first plus shared-runtime capable":

- local embedded backends such as SQLite remain first-class
- shared remote backends such as Postgres become first-class deployment targets
- `.prism` remains repo-owned published truth in both modes

The runtime should therefore be memory-authoritative while it is live:

- hot in-memory state is the source of truth for live request handling
- synchronous durable writes protect only the authoritative facts that cannot be lost on crash
- asynchronous persisted views absorb rebuildable snapshots, projections, and compatibility read models

## Rules

- Authoritative persisted state is the durable truth the runtime should reconstruct from.
- Derived state may be dropped and rebuilt from authoritative state without semantic loss.
- Compatibility projections may remain useful, but they are not allowed to become the sole write truth.
- Runtime-only hydration details such as fresh handles, resolved overlays, or process-local caches must not be published as authored repo truth.
- Snapshots and compaction outputs are allowed as accelerators, bootstrap aids, or exports, but not as a second semantic authority.
- A shared backend should be treated as the mutable runtime state plane for collaboration, not as a replacement for repo-published knowledge.
- Scope should be modeled explicitly through identity and context, not inferred accidentally from storage location.
- The live daemon should answer authoritative queries from hot memory unless a surface is explicitly classified as hot-plus-cold or cold-backed.
- A crash-safe write path should append or persist the minimum authoritative fact, not force every rebuildable projection onto the request path.
- Restart correctness should come from repo-published truth plus authoritative journals, with checkpoints and materializations treated as accelerators.

## Durability Classes

The migration should classify persisted state into four concrete durability classes:

| Class | Meaning | Request-path requirement | Recovery role |
| --- | --- | --- | --- |
| Repo-published authority | Durable truth the repository carries in `.prism` | Synchronous when publishing repo-owned knowledge | Canonical repo truth across clones |
| Sync runtime authority | Crash-sensitive mutable runtime facts stored in the shared backend | Synchronous and minimal | Replayed after crash or restart |
| Async checkpoint / materialization | Rebuildable snapshots, projections, and read models | Off the critical path when correctness allows | Speeds hydration and cold queries |
| Process-local cache | Ephemeral in-memory state and convenience materializations | Hot only; never required for crash durability | Rebuilt from authoritative state |

The request path should prefer the cheapest class that preserves correctness:

- publish repo-owned knowledge to `.prism` only when the fact is durable repo truth
- synchronously persist only the minimum runtime authority needed for crash safety
- treat checkpoints and materializations as optional lagging copies
- keep transient overlays and handles in process-local memory only

## Query Classes

The runtime query surface should make query class explicit instead of relying on callers to infer it
from backend wiring:

- Hot-only queries read only in-memory authoritative runtime state and preserve bounded hot
  hydration.
- Hot-plus-cold queries merge hot runtime state with lagging persisted state when durable recall is
  required.
- Cold-backed queries consult persisted state directly and should be used intentionally for reload,
  replay, or persisted-state inspection.

Current code should converge on the following naming rule:

- `hot_*`: hot-only
- `cold_*`: cold-backed
- unprefixed query methods: merged hot-plus-cold

Representative surfaces:

- [lib.rs](/Users/bene/code/prism/crates/prism-query/src/lib.rs): `hot_lineage_history`,
  `cold_lineage_history`, `lineage_history`, `hot_history_snapshot`, `cold_history_snapshot`,
  `history_snapshot`, `hot_outcome_event`, `cold_outcome_event`, `outcome_event`
- [outcomes.rs](/Users/bene/code/prism/crates/prism-query/src/outcomes.rs): `query_hot_outcomes`,
  `query_cold_outcomes`, `query_outcomes`, `hot_task_replay`, `cold_task_replay`, `resume_task`
- [session.rs](/Users/bene/code/prism/crates/prism-core/src/session.rs): `load_hot_*`,
  `load_cold_*`, and merged `load_*` wrappers for runtime-facing callers

## Classification

| Domain | Authoritative persisted state | Derived / compatibility / export state | Notes |
| --- | --- | --- | --- |
| Structural graph and file state | Durable graph/file state and raw observed changes owned by `prism-store` | Replaced derived edges, convenience materializations, one-shot bootstrap views | Structure remains the repo/runtime authority. Derived edges stay additive. |
| Lineage and temporal identity | Durable lineage history, tombstones, and identity assignment state owned by `prism-history` | Co-change counts, lineage summaries, replay-oriented snapshots | Temporal identity is authoritative; co-change and similar aggregates are rebuildable. |
| Outcomes and workspace memory | Outcome events, append-only memory events, and published repo memory events | Episodic snapshots, recall indexes, fuzzy recall helpers, hydrated memory views | Event history is authoritative; snapshot forms are acceleration or hydration aids. |
| Curated repo knowledge | Published concept events, concept-relation events, and repo memory events under `.prism/` | Hydrated concept packets, decode lenses, search bundles, curator convenience views | Published repo knowledge travels by event log, not by hydrated packet shape. |
| Native plans and authored plan bindings | `.prism/plans/index.jsonl` plus per-plan event logs; authored plan metadata; authored node fields, including task-backed authored fields in `TaskExecution` plans; authored edges; stable published refs in bindings | Hydrated plan graphs, runtime binding overlays, compatibility task projections, `planSummary`, `planNext`, compact task guidance, plan resource views | For plans, authored intent is authoritative. For `TaskExecution` plans, repo-published plan state owns task-backed authored fields, while coordination owns live continuity overlays such as leases, claims, handoffs, reviews, artifacts, and execution overlays. Hydrated handles and runtime rebinding results are runtime-only. |
| Shared workflow continuity | Durable claim, artifact, review, handoff, and policy-relevant continuity state | Blocker summaries, inbox/task context views, conflict summaries, risk hints | Continuity that changes completion or contention semantics is authoritative; summaries are not. |
| Projections and read models | None by default; these are derived from authoritative state | Projection snapshots, co-change neighbors, validation deltas, query-oriented summaries, recommendation frontiers, compatibility read models | If a projection is rebuildable from authoritative events/state, it is not a write authority. |
| Snapshots, compaction, and exports | None by default; these remain derived | `GraphSnapshot`, `HistorySnapshot`, `OutcomeMemorySnapshot`, `ProjectionSnapshot`, `CoordinationSnapshot`, episodic/inference/curator snapshots, deterministic per-plan compaction outputs, export artifacts | Snapshots may accelerate reload or export state, but replay/event-backed state remains canonical. |

Projection contract:

- published projections are committed deterministic interfaces over published repo authority
- serving projections are latency-oriented read models over repo authority, runtime authority, or both
- ad hoc projections are parameterized historical or diff-oriented reads over authoritative state
- no projection class may become the sole semantic write authority for the domain it renders
- persisted projection materializations are accelerators and must remain safe to discard and rebuild
- projection surfaces must expose freshness or materialization state when trust depends on it

## Domain Durability Contract

The classification above becomes the following migration contract:

| Domain | Live authority while daemon is running | Sync durable write required | Async persisted state allowed | Query class | Recovery contract |
| --- | --- | --- | --- | --- | --- |
| Structural graph and file state | In-memory graph, file state, and workspace tree runtime | No full graph snapshot required on the request path; persist only raw authoritative change facts if needed for replay | Graph snapshots, per-file state tables, derived edges, workspace tree snapshots, compatibility graph views | Hot authoritative | Rebuild from source state plus authoritative journals/checkpoints |
| Lineage and temporal identity | In-memory lineage store | Yes: lineage/history deltas and tombstones | History snapshots, replay aids, co-change aggregates | Hot authoritative with cold-backed history recall | Recover exact identity continuity from journal plus latest checkpoint |
| Outcomes and authored memory | In-memory outcome/memory event state | Yes: authored outcome events and authored memory events | Outcome snapshots, episodic snapshots, recall indexes, fuzzy lookup helpers | Hot plus cold | Recover exact event history from journal; rebuild snapshots asynchronously |
| Curated repo knowledge | Published `.prism` state plus hydrated runtime view | Publish repo-quality events/records synchronously when authored | Hydrated packets, decode lenses, search bundles | Hot authoritative for hydrated repo state; repo-published source of truth | Rehydrate from `.prism` and refresh caches/materializations |
| Shared workflow continuity | In-memory coordination runtime | Yes: coordination events, claims, artifacts, reviews, handoffs, and comparable workflow continuity facts | Queue read models, inbox summaries, conflict summaries, task/plan convenience views | Hot authoritative | Recover exact continuity from journal plus optional compaction/checkpoint |
| Projections and read models | Derived in-memory indexes | No | Projection snapshots, recommendation frontiers, validation summaries, compatibility read models | Hot authoritative when hydrated, otherwise cold-backed by persisted materialization | Rebuild from authoritative state; stale projections must not change semantic truth |
| Snapshots, compaction, and exports | None; these are always derived | No | Any deterministic checkpoint, compaction, or export output | Cold/bootstrap only unless explicitly merged | Safe to lose and regenerate |

## Query Authority Classes

Every query surface should fall into one of three classes:

| Query class | Meaning | Examples | Allowed backing sources |
| --- | --- | --- | --- |
| Hot authoritative | Live daemon must answer from current in-memory state | coordination status, plan/task truth, current graph, claim conflicts, current lineage identity | Hot memory, optionally plus repo-published `.prism` state already hydrated |
| Hot plus cold | Live daemon answers from hot state first and may merge bounded persisted history or cold event data | outcome recall, task replay, lineage history recall, bounded memory recall | Hot memory plus persisted journals/checkpoints as bounded cold reads |
| Cold/bootstrap only | Only recovery, background hydration, or explicitly cold historical views may rely primarily on persisted state | startup hydration, background reloads, checkpoint inspection, export generation | Persisted journals, checkpoints, projections, `.prism` |

Rules:

- Do not route an authoritative live request to SQLite just because a persisted view exists.
- Do allow bounded cold reads for large historical domains that are intentionally not fully hydrated in memory.
- If a query merges hot and cold state, hot state wins on conflicts because it reflects the live daemon truth.

## Protected-State Runtime Import Rule

Repo-published `.prism` streams are protected-state inputs to the live runtime, not ad hoc
read-path side channels.

The runtime rule is:

- bootstrap may hydrate repo-published `.prism` state into runtime memory
- a dedicated protected-state sync path may import later `.prism` updates into live runtime state
- normal read paths must consume current runtime state only and must not opportunistically import
  `.prism` streams on a per-domain special basis

Watcher responsibilities stay split:

- the normal source watcher continues to ignore `.prism`
- a dedicated protected-state watcher or equivalent sync mechanism owns live `.prism` imports

That split is intentional. Repo-published protected streams should not trigger normal source
indexing just because they changed on disk.

## Restart And Crash Contract

The migration should assume the following restart behavior:

- restart correctness depends on repo-published authority plus sync runtime authority
- asynchronous checkpoints and materializations may lag behind the latest live state
- restart is allowed to replay authoritative journals on top of the newest compatible checkpoint
- after restart, derived projections and snapshots may be temporarily stale or absent until background rebuild catches up

This means the shared backend should optimize for:

- cheap authoritative journal appends
- bounded journal replay
- periodic compatible checkpoints
- explicit revision tracking between hot state and persisted views

It should not assume that every request-path mutation has already flushed every derived table.

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

The same architecture is already useful on a single machine when one developer has:

- multiple checkouts of the same repository
- multiple worktrees for the same repository
- more than one local Prism runtime touching the same logical repo work

That means the shared-runtime model is not only for future hosted deployments. Multiple worktrees already turn local-first into a multi-context runtime problem.

## Identity And Scope Dimensions

The persistence model should make the following identities explicit:

- `repo_id`: the logical repository identity
- `worktree_id`: the specific checkout or worktree context
- `branch_ref` and/or checked-out commit identity: the current code reality for that context
- `session_id`: the agent session bound to a worktree context
- `instance_id`: the Prism server process or runtime instance

These identities let PRISM answer distinct questions cleanly:

- should this state be visible in every checkout of the repo?
- should it only apply to this worktree?
- is it branch-specific?
- is it only for this live session or process?

Without these dimensions, scope bleeds accidentally across checkouts or gets encoded in deployment layout instead of domain semantics.

These identities also define persistence obligations:

- `repo_id` gates repo-published truth and shared runtime scope
- `worktree_id` gates branch-diverged or checkout-specific runtime authority
- `branch_ref` explains when live mutable intent is not yet publishable repo truth
- `session_id` and `instance_id` support leases, heartbeats, replay ownership, and stale-session cleanup

## Endpoint And Worktree Contexts

PRISM should not require one MCP server per worktree.

The target local model is:

- one local Prism daemon or MCP endpoint per machine can manage many repos and many worktrees
- one shared runtime database per machine holds shared mutable runtime state
- each MCP session binds to one worktree context by default
- each bound worktree context gets its own authoritative in-memory runtime engine
- handles, runtime overlays, and mutable coordination state are scoped under that bound context

This separates:

- transport identity: which endpoint the client is talking to
- session identity: which agent conversation or runtime is active
- worktree context: which checkout, branch, and mutable code reality the session is attached to

Running one server per worktree can remain a useful fallback or debug mode, but it should not be the architectural requirement.

One daemon per machine is the default deployment shape, not a hidden semantic assumption.

The shared-runtime plane should remain correct even when multiple local MCP daemons on one machine
share the same runtime database.

The persistence plan should therefore include an explicit worktree-context binding model rather than assuming that "one server equals one workspace."

For the local-first deployment target, this means:

- one machine-scoped daemon
- one machine-scoped shared SQLite runtime store
- many worktree-scoped runtime engines under that host

That local topology should use the same backend-neutral shared-runtime contract that a future Postgres backend would implement for cross-machine coordination.

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

These capabilities matter even in the local SQLite phase, because multi-instance cleanliness on one
machine is the best rehearsal for a later Postgres-backed shared runtime.

This also means the persistence plan must account for:

- optimistic concurrency
- idempotent event handling
- lease and heartbeat renewal
- stale session cleanup
- repo, branch, and worktree identity
- session-to-worktree binding and context isolation
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
- worktree-bound coordination state for uncommitted or branch-diverged work

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

State that should move to sync runtime authority in the first migration:

- coordination continuity events and revision-aware workflow mutations
- lineage and history deltas needed to preserve semantic identity across crash/restart
- authored outcome events
- authored memory events

State that should move to async checkpoints/materializations in the first migration:

- graph snapshots and per-file graph materializations
- replaced derived-edge tables
- projection tables and recommendation/read-model summaries
- workspace tree snapshots and refresh accelerators
- episodic, inference, curator, and compatibility snapshots

State that is often worktree- or branch-scoped even when a shared backend exists:

- active claims tied to uncommitted local edits
- draft plans before publication
- stale-revision judgments against one checkout state
- branch-diverged intent and temporary execution overlays

## Current Transitional Caveats

The current store boundary is still snapshot-shaped in several places. In particular, [`crates/prism-store/src/store.rs`](/Users/bene/code/prism/crates/prism-store/src/store.rs) still exposes load/save methods for `HistorySnapshot`, `OutcomeMemorySnapshot`, `ProjectionSnapshot`, `CoordinationSnapshot`, and other snapshot forms.

That does not make those snapshots the desired long-term authority.

During the migration:

- do not add new features that depend on snapshots as the only semantic write truth
- do not treat compatibility task projections as the plan-system authority
- keep new persistence logic centered on authoritative events or normalized state, then derive snapshots or projections from there
- do not assume every `SqliteStore` read on the request path is legitimate just because the store already exposes it
- preserve bounded hot-memory behavior for large history, outcome, and projection domains instead of hydrating everything into RAM
- keep the first migration in-process and SQLite-backed for cold reads and materializations; a separate persistence worker is a later architectural decision, not a prerequisite

## Immediate Guidance For The Persistence Migration Plan

- `coord-task:01kn13pz030s4q1c3qrbbg203f` should make this document concrete enough that later tasks do not re-decide durability or query semantics ad hoc.
- `coord-task:01kn13q61f8j93je9vws9kdqjy` should introduce backend-neutral interfaces for authoritative journals, optional checkpoint loaders, and materialized-view writers, not SQLite-shaped snapshot authority.
- `coord-task:01kn13qdfw63hwrr79cqm7rqs3` should move session, watch, and runtime request paths onto hot-memory authority first, with cold-backed reads remaining explicit and bounded.
- `coord-task:01kn13qj69y75rtwrbw6ewqzw5` and `coord-task:01kn13qsapc9fjk72wvth681yf` should put crash-sensitive workflow continuity, lineage/history deltas, and authored memory/outcome events onto synchronous durable journal paths.
- `coord-task:01kn13qykg063n2zc3h5rt507q` should maintain graph snapshots, workspace tree state, projections, compatibility views, and other read models as asynchronous coalesced materializations from authoritative state.
- `coord-task:01kn13r7qf0yf4kqqng4a3nt4w` should preserve bounded hot-memory policy and formalize which query surfaces are hot authoritative, hot plus cold, or cold/bootstrap only.
- `coord-task:01kn13rgty80e4y885n1vzb170` should hydrate from authoritative state first, replay journals over checkpoints, track lag explicitly, and bound replay time.
- `coord-task:01kn13rrg3yzbrcgrjh06ppv3h` should validate crash safety, multi-instance behavior, replay correctness, latency, and memory ceilings.
- `coord-task:01kn13ryrrk04aacsvg1ac96c2` should evaluate with the user whether persistence isolation should become a follow-up worker plan after the semantic migration is complete.

## Validation Matrix For The Migration

The migration is only acceptable when the following guarantees stay covered:

| Guarantee | Automated coverage | Manual / live validation |
| --- | --- | --- |
| Crash-safe authored outcome continuity without checkpoint flush | `reload_bounds_hot_outcomes_from_authoritative_journal_without_checkpoint_flush`, `recovery_rebuild_from_persisted_state_records_replay_bounds` | restart the release daemon and confirm `runtimeStatus().freshness.lastRefreshPath == "recovery"` with non-zero reload-work fields |
| Crash-safe coordination continuity without read-model flush | `coordination_journal_recovers_after_restart_without_read_model_flush`, `authoritative_coordination_load_prefers_event_log_over_stale_snapshot_row` | verify task/plan queries still resolve immediately after restart before forcing a materialization flush |
| Async checkpoint behavior stays off the request path | `appended_outcome_flushes_projection_materialization_off_request_path`, `refresh_fs_materializes_graph_snapshot_off_request_path`, `coordination_session_materializes_read_models_off_request_path` | inspect live refresh logs for reload work and confirm request-path refreshes do not require checkpoint writes to complete |
| Bounded hot memory with cold-backed recall | `reload_bounds_hot_outcomes_but_queries_cold_outcomes_from_store`, `reload_bounds_hot_outcomes_from_authoritative_journal_without_checkpoint_flush` | check `runtimeStatus().freshness` and process RSS after restart against a large outcome history worktree |
| Shared-runtime correctness across instances | `shared_runtime_sqlite_shares_session_memory_and_concepts_across_workspaces`, `shared_runtime_sqlite_shares_memory_events_without_checkpoint_flush` | run two workspace sessions against the same shared-runtime SQLite and confirm session-scoped events appear in the second session before any checkpoint flush |
| Restart observability and replay bounds | `hydrated_workspace_session_marks_background_refresh_pending`, `prism_runtime_views_surface_startup_recovery_work`, `prism_runtime_views_prefer_structured_runtime_state` | after rebuild/restart, verify the live daemon reports `lastRefreshLoadedBytes`, `lastRefreshReplayVolume`, and `lastRefreshWorkspaceReloaded` coherently |

Latency reduction is partly a live property rather than a unit-test property. For this migration, treat it as validated when:

- request-path tests confirm checkpoints/read models are deferred
- release-daemon restart and refresh commands stay healthy
- live refresh logs show reload work through runtime telemetry instead of synchronous checkpoint writes on the critical path

## Plans-Specific Interpretation

For first-class plans, the key distinction is:

- authoritative: authored plan intent, durable workflow continuity, stable published refs
- derived: hydrated graph materializations, runtime rebinding results, compatibility task views, summaries, recommendations, snapshots

That means repo-wide graph version drift or runtime convenience data must not be confused with authored plan truth.
