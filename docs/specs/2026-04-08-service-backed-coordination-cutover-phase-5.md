# Service-Backed Coordination Cutover Phase 5

Status: in progress
Audience: service, coordination, runtime, session, MCP, CLI, UI, storage, and watch maintainers
Scope: complete roadmap Phase 5 by cutting the runtime and product surfaces over to the service-backed coordination platform

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 5:

- cut the coordination runtime and product surfaces over to the four completed seams:
  - authority store
  - materialized store
  - query engine
  - mutation protocol

Phase 4 settled the write protocol.
Phase 5 now makes the architecture actually live by removing the old ownership model from runtime
behavior and product surfaces.

The target is the accepted service-owned architecture:

- interactive coordination participation is service-backed
- service-owned coordination materialization is the only coordination materialization owner
- runtimes do not own coordination SQLite state
- runtimes remain responsible only for worktree-local operational and telemetry state

This is the broad cutover phase.
It is not mainly about inventing new semantics.
It is about deleting the remaining old pathways and making the implemented runtime match the
contracts and ADRs.

## 2. Status

Current state:

- [x] the authority seam exists
- [x] the materialized-store seam exists
- [x] the query engine exists
- [x] the mutation protocol exists
- [ ] session and bootstrap flows still assume too much runtime-local coordination ownership
- [ ] watch and sync flows still assume too much runtime-local coordination refresh responsibility
- [ ] service-owned coordination materialization is not the only real owner yet
- [ ] MCP, CLI, and UI paths still contain residual direct runtime or persistence assumptions
- [ ] direct SQLite or shared-ref helpers still leak through some runtime and product surfaces

Current slice notes:

- the coordination contracts now clearly state that service-owned coordination materialization is
  authoritative for eventual/local coordination reads, not per-worktree runtime SQLite state
- Phase 4 closed the write-side protocol and reduced the remaining coordination mutation surfaces
  to the canonical transaction path
- workspace startup checkpoints no longer persist or restore coordination snapshots; bootstrap now
  starts with empty in-memory coordination state and expects later service-backed hydration
- session strong reads no longer write coordination startup checkpoints or read models into the
  worktree store as a side effect
- strong coordination reads and eventual coordination reads are now intentionally split: strong
  reads no longer implicitly backfill eventual local coordination state
- unused session-level ad hoc coordination persistence helpers were removed instead of being kept
  as shadow write paths beside the mutation protocol
- shared-ref live sync now writes coordination materialization through the shared helper instead of
  reimplementing startup-checkpoint and read-model writes inside `watch.rs`
- coordination authority refresh/apply orchestration now lives in a dedicated
  `coordination_authority_sync` module used by both `session.rs` and `watch.rs`
- product-facing MCP/UI readers now prefer session-backed coordination snapshots through
  `QueryHost` helpers, but fall back to the canonical in-memory `Prism` snapshot when a workspace
  session has not yet hydrated service-owned coordination state
- the remaining work is now primarily runtime and surface cutover, not mutation semantics

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 5: Cut over the service-backed coordination runtime and product surfaces

## 4. Related contracts and ADRs

This spec depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-authority-sync-role.md](../contracts/service-authority-sync-role.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/runtime-identity-and-descriptors.md](../contracts/runtime-identity-and-descriptors.md)
- [../contracts/local-materialization.md](../contracts/local-materialization.md)
- [../contracts/authority-sync.md](../contracts/authority-sync.md)
- [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)

## 5. Scope

This phase includes:

- cutting session/bootstrap flows over to the service-backed coordination ownership model
- removing runtime assumptions that coordination materialization is worktree-local runtime state
- routing watch, sync, and refresh responsibilities through the service-owned coordination model
- cutting MCP, CLI, and UI coordination reads and writes over to the new seams fully
- removing residual direct SQLite and shared-ref access from product-facing modules
- making service-owned coordination materialization the only legitimate owner of local/eventual
  coordination state

This phase does not include:

- the trust-family cleanup in Phase 6
- the broader coordination platform freeze in Phase 7
- spec-engine implementation
- remaining richer service roles beyond what is needed to make service-backed coordination real

## 6. Non-goals

This phase should not:

- redesign coordination semantics again
- reopen the mutation protocol design
- move coordination authority into the service itself
- reintroduce runtime-owned coordination SQLite state
- blur runtime telemetry storage with coordination materialization

## 7. Design

### 7.1 Ownership target

After this phase:

- the authority backend remains the durable source of coordination truth
- the service owns coordination materialization and eventual/local coordination reuse
- runtimes consume coordination through service-owned abstractions
- runtimes keep only worktree-local telemetry and operational state

### 7.2 Cutover rule

Any code path that still treats the runtime as a coordination-state owner must either:

- move behind the service-owned materialization boundary in this phase, or
- be documented as a temporary compatibility shim with a concrete removal target

No new runtime-owned coordination persistence may be introduced during this phase.

### 7.3 Surface rule

After this phase:

- product-facing reads use the coordination query engine and service-owned materialization path
- product-facing writes use the coordination mutation protocol and authority-store-backed commit
  path
- direct shared-ref and direct coordination SQLite helpers do not remain in product-facing modules

### 7.4 Runtime rule

Runtimes may still own:

- worktree-local telemetry
- local command/execution history
- local activity sensing
- local hints, interventions, and operational packets

Runtimes may not remain the owner of:

- coordination read models
- coordination checkpoints
- eventual coordination snapshots
- coordination-materialization freshness bookkeeping

## 8. Implementation slices

### Slice 1: Session and bootstrap cutover

- identify runtime and session startup paths that still hydrate or persist coordination state as
  runtime-local ownership
- move those paths behind the service-owned coordination materialization model
- make service reachability and service-backed coordination participation explicit at startup

Current progress:

- workspace runtime startup checkpoints no longer serialize or restore coordination snapshots
- the old checkpoint format was intentionally invalidated for this cutover so stale branch-local
  coordination state cannot be revived implicitly at runtime startup
- strong coordination reads now sync authority and read authoritatively, but no longer backfill
  coordination materialization into the runtime-owned worktree store

Exit criteria:

- session/bootstrap code no longer implies runtime-owned coordination materialization

### Slice 2: Watch, sync, and refresh cutover

- move shared-ref watch, authority refresh, and coordination runtime refresh responsibilities into
  the service-owned coordination flow
- reduce runtime-local coordination watch behavior to telemetry/operational concerns only
- make stale/current coordination refresh semantics line up with the authority-sync and read-broker
  contracts

Current progress:

- shared-ref live sync no longer reimplements coordination startup-checkpoint and read-model writes
  inline in `watch.rs`; it now goes through the shared coordination materialization helper
- session strong reads and watch polling now call the same authority-sync orchestration module
  instead of owning duplicate refresh choreography
- session eventual coordination snapshots, plan-state reads, and read-model loads now read through
  the materialized-store seam or the authority-backed published-plan loaders instead of reaching
  through the workspace store as a de facto coordination owner
- the materialized-store seam now owns effective coordination read-model fallback, including the
  derive-from-snapshot path when persisted read models are absent, so `WorkspaceSession` no
  longer rebuilds that fallback itself
- live production code no longer uses the old `coordination_reads` eventual-load wrappers;
  `WorkspaceSession` and protected-state hydration now read eventual coordination state directly
  from the materialized-store seam, and the old wrapper helpers are test-only
- dead store-backed coordination read helpers and unused authoritative persistence compatibility
  methods were removed once the session read path no longer depended on them

Exit criteria:

- watch and refresh flows no longer depend on runtimes acting like mini coordination databases

### Slice 3: Product-surface cutover

- cut MCP, CLI, and UI coordination reads and writes over to the authority, materialized, query,
  and mutation seams completely
- remove residual direct helper reads from product-facing modules
- make diagnostics/history views consume the same service-backed coordination model

Current progress:

- plans resource reads now use `QueryHost` session-aware coordination snapshot helpers instead of
  reading directly from `current_prism()`
- SSR plan markdown export now prefers session-backed coordination snapshots and canonical
  projection state, with an explicit fallback to the in-memory `Prism` snapshot for fixtures and
  transitional cases where service-owned coordination materialization has not yet been hydrated
- UI fleet and overview coordination summary/queue fallbacks now read through the same
  session-aware `QueryHost` coordination snapshot helpers
- `QueryHost` now also owns session-aware coordination read-model and queue-read-model fallback so
  product-facing overview readers no longer reach into `WorkspaceSession` directly to rebuild that
  branching themselves
- product-facing integration mutation helpers now use query-layer artifact and review getters
  instead of spelunking raw coordination snapshots for review-artifact readiness decisions
- assisted-lease watcher target selection now uses explicit coordination task/claim query methods
  instead of iterating the raw live runtime snapshot directly
- UI plans agent filtering and graph touchpoint derivation now read canonical coordination graph
  state through session-aware `QueryHost` snapshot helpers instead of calling
  `prism.coordination_snapshot_v2()` directly inside view helpers
- PRISM doc export now loads coordination plan state through the session coordination read path
  instead of exporting directly from the live runtime snapshot
- the helper boundary now centralizes the transitional rule for product-facing reads:
  service-backed/session-owned coordination materialization wins when present, but empty or
  unhydrated session state falls back to the canonical in-memory snapshot during the cutover
  window
- runtime-status repo overlay counts now prefer the same session-aware/service-backed coordination
  snapshot path as other product surfaces, while worktree and session overlay counts remain
  intentionally runtime-local because they report live runtime/session bindings rather than
  service-owned eventual coordination materialization
- the old `CoordinationPersistenceBackend` snapshot/materialization compatibility helpers are now
  test-only instead of part of the live crate surface, so persistence-path compatibility no longer
  leaks into production code shape

Exit criteria:

- product-facing coordination surfaces no longer bypass the new seams

### Slice 4: Persistence-path cleanup

- remove remaining runtime-owned coordination SQLite assumptions
- collapse residual direct coordination SQLite and shared-ref helper usage below the new seams
- delete or isolate temporary compatibility wrappers

Exit criteria:

- service-owned coordination materialization is the only live owner of local/eventual coordination
  state

## 9. Validation

Minimum validation for this phase:

- targeted tests in changed crates for:
  - session/bootstrap cutover behavior
  - watch/sync and refresh behavior
  - MCP or CLI surface behavior changed by the cutover
- downstream validation for crates affected by public seam changes
- `git diff --check`

Important regression checks for this phase:

- runtimes no longer behave as coordination-state owners
- service-backed coordination startup behavior is explicit and non-mushy
- eventual/strong coordination reads still behave correctly after the ownership cutover
- product surfaces still expose the same user-facing coordination behavior after the plumbing move

## 10. Completion criteria

Phase 5 is complete only when:

- the coordination layer is fully expressed through the new abstractions
- service-owned coordination materialization is the only legitimate owner of local/eventual
  coordination state
- no product-facing direct authority, runtime-owned materialization, query, or mutation paths
  remain
- the Phase 5 spec and roadmap are updated to `completed`

## 11. Implementation checklist

- [ ] Cut over session/bootstrap paths
- [ ] Cut over watch/sync/refresh ownership
- [ ] Cut over MCP coordination surfaces fully
- [ ] Cut over CLI coordination surfaces fully
- [ ] Cut over UI/runtime-facing coordination views fully
- [ ] Remove residual direct coordination SQLite and shared-ref helper usage from product-facing code
- [ ] Validate changed crates and direct downstream dependents
- [ ] Mark Phase 5 complete in the roadmap

## 12. Current implementation status

Phase 5 starts from a stronger base than earlier phases:

- authority, materialized, query, and mutation seams now exist and are substantially implemented
- the service-owned coordination materialization ADR is accepted and reflected in the live
  contracts
- Phase 4 removed the biggest write-side protocol ambiguity by converging current coordination
  mutation surfaces on the transaction engine

The remaining work is broad but conceptually straightforward:

- cut runtime/bootstrap/watch ownership over to the accepted service-backed model
- finish the remaining product-facing bypass removal across MCP, UI/runtime views, and CLI
- finish deleting the runtime-owned coordination-materialization assumptions that no longer match
  the architecture
