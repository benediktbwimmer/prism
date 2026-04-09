# DB Authority Family And Abstraction Hardening

Status: in progress
Audience: coordination, service, storage, MCP, CLI, deployment, and authority-backend maintainers
Scope: introduce the internal DB-backed coordination authority family beneath `CoordinationAuthorityStore`, make SQLite the default functioning backend, remove migration-era compatibility layers and legacy coordination graph surfaces, add Postgres later through the same seam, and harden the authority and service abstractions along the way

---

## 1. Summary

PRISM already has a real public coordination authority seam:

- `CoordinationAuthorityStore`

PRISM also already has an accepted release-order decision:

- DB-backed coordination authority is the release-oriented path
- SQLite is the local single-instance path
- Postgres is the hosted or multi-instance path
- Git shared refs remain supported, but are not the primary launch backend

What is still missing is the implementation family beneath that public seam.

The codebase is at the point where this work now makes sense to do, but it should not be treated
as a narrow backend drop-in. The current authority seam is good enough to build on, yet still
shows several migration-era mismatches:

- backend taxonomy and diagnostics are still partly Git-shaped
- live-sync and a few authority-adjacent helpers still import shared-ref behavior directly
- default backend selection still points at Git
- some result and metadata types still reflect the first backend more than the steady-state family

This roadmap exists to finish that transition cleanly.

The end state is:

1. `CoordinationAuthorityStore` remains the only product-facing authority contract
2. one internal DB authority family sits beneath it
3. `SqliteCoordinationDb` is the first real implementation of that family
4. SQLite becomes the default functioning backend for `CoordinationAuthorityStore`
5. migration-era compatibility code is deleted rather than preserved at product edges
6. plans, tasks, artifacts, and reviews v2 are the only live coordination surface model left
7. `PostgresCoordinationDb` lands later through the same family seam
8. Git shared refs remain supported without continuing to shape upper layers incorrectly
9. the surrounding abstractions and service wiring are cleaner and harder to bypass than they are today

This roadmap follows and sharpens:

- [2026-04-08-coordination-to-spec-engine-to-service.md](./2026-04-08-coordination-to-spec-engine-to-service.md)
- [2026-04-09-platform-seam-follow-through.md](./2026-04-09-platform-seam-follow-through.md)
- [../specs/2026-04-09-db-backed-service-foundation-phase-15.md](../specs/2026-04-09-db-backed-service-foundation-phase-15.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

## 2. Status

Current phase checklist:

- [x] Phase 1: harden the public authority seam for a backend family
- [x] Phase 2: introduce the internal DB authority abstraction
- [x] Phase 3: implement SQLite authority and make it the default backend
- [ ] Phase 4: complete service and product-surface adoption on the SQLite default path
- [ ] Phase 5: implement Postgres through the same DB authority seam
- [ ] Phase 6: remove remaining migration mismatches, compatibility layers, and legacy coordination surfaces

Current active phase:

- Phase 6: remove remaining migration mismatches, compatibility layers, and legacy coordination surfaces

Current phase spec:

- [../specs/2026-04-09-coordination-v2-breaking-compatibility-removal-phase-6.md](../specs/2026-04-09-coordination-v2-breaking-compatibility-removal-phase-6.md)
- [../specs/2026-04-09-coordination-query-reader-v2-follow-through-phase-6.md](../specs/2026-04-09-coordination-query-reader-v2-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-task-handoff-follow-through-phase-6.md](../specs/2026-04-09-canonical-task-handoff-follow-through-phase-6.md)
- [../specs/2026-04-09-host-mutation-canonical-follow-through-phase-6.md](../specs/2026-04-09-host-mutation-canonical-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-read-surface-follow-through-phase-6.md](../specs/2026-04-09-canonical-read-surface-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-read-model-follow-through-phase-6.md](../specs/2026-04-09-canonical-read-model-follow-through-phase-6.md)
- [../specs/2026-04-09-native-task-mutation-return-v2-follow-through-phase-6.md](../specs/2026-04-09-native-task-mutation-return-v2-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-task-lease-and-live-runtime-mutation-follow-through-phase-6.md](../specs/2026-04-09-canonical-task-lease-and-live-runtime-mutation-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-spec-linkage-follow-through-phase-6.md](../specs/2026-04-09-canonical-spec-linkage-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-runtime-publish-state-follow-through-phase-6.md](../specs/2026-04-09-canonical-runtime-publish-state-follow-through-phase-6.md)
- [../specs/2026-04-09-canonical-materialization-envelope-follow-through-phase-6.md](../specs/2026-04-09-canonical-materialization-envelope-follow-through-phase-6.md)

Current assessment:

- the public `CoordinationAuthorityStore` trait is real and broad enough to build on
- the provider and backend-config selection seam already exists
- the current Git backend is already routed through that seam
- the accepted architecture and spec docs already target a DB-backed family beneath the authority store
- the remaining work is primarily implementation, seam hardening, and cleanup rather than another large semantic redesign

Latest checkpoint:

- the roadmap and phase-1 spec are now written and linked
- the authority backend taxonomy now names `GitSharedRefs`, `Sqlite`, and `Postgres`
- authority diagnostics now use a backend-family details enum rather than only Git or unavailable
- the main CLI and MCP diagnostics consumers now load authority diagnostics through the store seam
- Phase 1 is complete and the internal DB authority seam now exists under
  `coordination_authority_store/db/`
- SQL backend factory selection now routes through that DB seam while SQLite and Postgres still
  fail closed explicitly
- Phase 2 is complete and the active work moved into the first functioning SQLite authority backend
- the SQLite authority backend now reads authoritative state from the repo-shared coordination DB,
  applies authority transactions, persists runtime descriptors through the checkpoint surface,
  surfaces SQLite diagnostics and retained history, and is selected as the default authority backend
- Phase 3 is complete; the active cleanup is now service and product-surface follow-through around
  the SQLite-first default
- the first Phase 4 cleanup slice now makes authority live-sync backend-gated, skips pointless
  polling-watch startup on SQLite-default sessions, and renames backend-neutral watch/session
  ownership away from shared-ref language
- the next Phase 4 cleanup slice is runtime descriptor publication follow-through so production
  surfaces stop calling the authority-backed publication path by the legacy shared-ref sync name
- the runtime descriptor publication follow-through slice is complete; the next cleanup is removing
  diagnostic-backed descriptor discovery from peer runtime routing
- the peer-runtime descriptor discovery cleanup is complete; the next Phase 4 cleanup is demoting
  the Git-only diagnostics helper from the primary public authority surface
- the Git-diagnostics demotion cleanup is complete; the next Phase 4 cleanup is authority-neutral
  naming follow-through inside MCP runtime-status caching and trust-surface helpers
- the runtime-status authority-view cleanup is complete; backend-neutral MCP status caching and
  trust-surface shaping now use coordination-authority-oriented internal naming while preserving
  the legacy shared-ref compatibility field at the response edge
- the next Phase 4 cleanup is SQLite-default authority semantics follow-through so shared-ref tests
  explicitly opt into the Git backend and backend-neutral persistence tests assert the settled
  SQLite authority and coordination materialization behavior
- the SQLite-default authority semantics follow-through is complete; shared-ref tests now select
  Git authority explicitly, backend-neutral persistence tests now assert SQLite-first authority and
  materialization semantics directly, and the full workspace test suite is green again on the
  SQLite-default path
- the startup-checkpoint authority naming follow-through is complete; backend-neutral startup
  checkpoint provenance now resolves through coordination-authority-named helpers instead of
  shared-ref-shaped naming in the checkpoint and authority API layers
- the runtime-gateway authority diagnostics follow-through is complete; backend-neutral remote-runtime
  routing now reads degraded verification through authority diagnostics, keeps descriptor lookup on
  the authority store, and reports SQLite-default authority failures without shared-ref-shaped
  wording
- the authority-publication operation naming follow-through is complete; backend-neutral authority
  traces, test opt-in markers, skip reasons, and repo-state export wording now use
  coordination-authority language instead of shared-ref-shaped names, and git-execution tracing
  now distinguishes deferred materialization scheduling from suppressed inline materialization on
  the SQLite-default authority path
- the authority-compatibility-edge follow-through is complete; legacy
  `shared_coordination_ref` response-field access is now isolated behind one helper and
  backend-neutral session guidance now talks about coordination authority rather than the shared
  coordination ref
- the authority-sync and legacy-alias demotion follow-through is complete; backend-neutral
  authority-sync wording no longer mentions a shared-ref refresh lock, and the remaining
  shared-ref diagnostics helpers are now explicit deprecated compatibility aliases behind preferred
  Git-named exports
- the backend-neutral wording residue cleanup is complete; the next active slice is the breaking
  removal of the remaining compatibility surfaces rather than preserving them
- the roadmap now explicitly targets deleting every remaining authority-edge compatibility alias,
  deleting the legacy runtime-status shared-ref field, and cutting live MCP/query coordination
  surfaces over to v2-only plan/task/artifact/review payloads
- the first Phase 6 breaking-cut implementation is in progress: authority-edge aliases are gone,
  the runtime-status field is now `coordinationAuthority`, and the live JS/MCP coordination surface
  now returns only v2 plan/task payloads
- the reader-side query follow-through is now complete; `prism-query` risk, intent, and impact
  readers plus adjacent MCP task-context/provenance readers now load canonical v2 task and plan
  data instead of legacy plan/task projections where legacy-only fields are not required
- the canonical task handoff follow-through is complete; pending handoff is now first-class data
  on canonical task records, derived v2 task status treats pending handoff as blocked, and the
  task-brief, assisted-lease, and session-task readers no longer need legacy task lookups for
  handoff or lease metadata
- the host-mutation canonical follow-through is complete; git-execution reloads, workflow update
  reads, artifact default-anchor lookup, work declaration, and session binding now use canonical
  v2 task and plan state wherever legacy lease-holder helpers are not required
- the remaining legacy host-mutation callers are now explicitly limited to stale-same-holder
  auto-resume and the git-execution admissibility path that still depends on legacy holder logic
- the next Phase 6 cleanup is canonicalizing the remaining native mutation-return helpers and
  active-path runtime mutation reloads under `prism-query` so host and runtime mutation flows stop
  re-reading legacy `CoordinationTask` values after mutation commits
- the current active slice is narrowing that work to the transaction-backed native task mutation
  helpers under `prism-query`; the deeper live-runtime mutation returns like heartbeat and
  authoritative-only task updates remain explicitly deferred until the runtime mutation path is
  cut over more directly
- the native task mutation-return follow-through is complete; transaction-backed native task
  helpers in `prism-query` now reload canonical `CoordinationTaskV2` records after commit,
  downstream core and MCP tests have been normalized to `CoordinationTaskId` at the mutation
  boundary, and MCP stale-resume expectations now assert canonical effective task statuses rather
  than the removed legacy raw-status response surface
- the canonical task lease and live-runtime mutation follow-through is complete; stale-same-holder
  auto-resume and git-execution admissibility now run on canonical task records, canonical
  lease-holder helpers live in `prism-coordination`, and the last live-runtime native task
  helpers now return `CoordinationTaskV2`
- the canonical read-surface follow-through is complete; `prism-query` now exposes canonical
  ready-task helpers, plan activity/discovery no longer reload legacy plan/task records, MCP
  plan-resource and runtime-overlay reads use `CoordinationSnapshotV2`, and the broker no longer
  exposes the legacy snapshot as part of the current coordination surface
- the canonical spec-linkage follow-through is complete; spec refs are now first-class on
  canonical plan/task records, the spec materialization path now consumes
  `CoordinationSnapshotV2`, and workspace-backed CLI/MCP spec reads no longer depend on the
  legacy snapshot surface
- the canonical read-model follow-through is complete; coordination read and queue models now
  derive from `CoordinationSnapshotV2`, broker and materialization fallback rebuilds no longer
  need the legacy snapshot, and those read-model structs now carry canonical identifiers instead
  of legacy plan/task payloads where only summary membership is needed
- the canonical runtime publish-state follow-through is complete; runtime-state and `Prism`
  publication paths now carry canonical coordination snapshots as first-class state, and the
  shared coordination-transaction path refreshes cached canonical state before immediate v2 reads
- the canonical materialization-envelope follow-through is complete; checkpoint and adjacent
  persistence code now require canonical coordination state on production materialization
  envelopes instead of treating it as an optional fallback derived from the legacy continuity
  snapshot
- the next Phase 6 work remains the deeper purge of active-path `CoordinationSnapshot` /
  legacy coordination-model dependencies that still exist under the mutation engine,
  transaction/runtime paths, and adjacent materialization code

## 3. Ordering thesis

This work should deliberately balance two priorities at the same time:

1. establish the internal DB authority family and make SQLite the real default backend
2. harden the existing abstractions and clean up the architecture as those implementations land

It should not be done as:

- “just add SQLite quickly and clean up later”
- or “spend weeks polishing abstractions without landing the DB family”

The correct order is interleaved:

1. harden the existing public seam just enough that the DB family will fit cleanly
2. introduce the internal DB abstraction beneath that seam
3. land SQLite as the first working backend and switch the default to it
4. finish the service-shell and product-surface follow-through needed for the SQLite default path
5. add Postgres against the same internal seam
6. delete the remaining migration-era compatibility code and legacy coordination projections
7. add Postgres later against the already-settled DB seam
8. update the docs to the true steady state

The core rule is:

- no second product-facing authority abstraction

`CoordinationAuthorityStore` stays the public contract.
The DB family seam remains an internal implementation boundary.

## 4. Phases

### Phase 1: Harden the public authority seam for a backend family

Before adding the DB family, clean up the parts of the authority surface that still reflect the
first backend too directly.

This includes:

- making backend taxonomy coherent for all intended authority backends
- removing obviously Git-only assumptions from authority diagnostics and metadata envelopes
- tightening any request or result types that are too implementation-shaped to support a family
- reducing or isolating remaining Git-specific authority behavior outside the backend implementation
- making strong, eventual, history, descriptor, and diagnostics semantics explicit where the code is still thinner than the contract

Exit criteria:

- the public authority types describe a backend family cleanly instead of a Git backend plus future placeholders
- Git-specific authority details no longer shape the public seam more than necessary
- the next phase can introduce the DB family without another public API rethink

### Phase 2: Introduce the internal DB authority abstraction

Introduce one internal DB authority family beneath `CoordinationAuthorityStore`.

The target shape is:

- `CoordinationAuthorityStore`
  - `DbCoordinationAuthorityStore`
    - `SqliteCoordinationDb`
    - `PostgresCoordinationDb`
  - `GitSharedRefsCoordinationAuthorityStore`

This phase includes:

- defining the internal DB trait or traits and DB-family types
- keeping those seams below the product-facing authority contract
- centralizing shared DB-backed authority behavior in one place rather than duplicating it in SQLite- and Postgres-specific code
- shaping transaction, current-read, runtime-descriptor, retained-history, and diagnostics behavior so both SQL backends can share the same semantic adapter

Exit criteria:

- SQLite and Postgres have one obvious shared authority implementation boundary
- upper layers still depend only on `CoordinationAuthorityStore`
- there is no SQLite-specific or Postgres-specific authority logic scattered upward into product code

### Phase 3: Implement SQLite authority and make it the default backend

Implement the SQLite-backed authority path through the new DB family seam and switch the default
provider path to SQLite.

This includes:

- implementing current authoritative state reads
- implementing transactional mutation commit and deterministic conflict handling
- implementing runtime descriptor publication and discovery
- implementing retained authoritative history
- implementing authority diagnostics and provenance for the SQLite backend
- selecting SQLite as the default functioning backend in authority-store configuration

Exit criteria:

- `CoordinationAuthorityStoreProvider::default()` yields a working SQLite-backed authority path
- the normal local service path can use SQLite authority without Git shared refs
- the authority-store tests cover SQLite semantics directly

### Phase 4: Complete service and product-surface adoption on the SQLite default path

Once SQLite is the default backend, finish the cleanup needed so the rest of the system truly
behaves as if `CoordinationAuthorityStore` is backend-neutral.

This includes:

- removing or narrowing remaining default-opener and default-backend shortcuts where they still hide architectural mismatches
- cleaning up authority-adjacent live-sync wiring that still assumes Git shared refs directly
- tightening service-shell, authority-sync, read-broker, and mutation-broker ownership around the new default path
- keeping trust, provenance, freshness, and diagnostics shaping shared instead of duplicating them per backend

Exit criteria:

- the service-backed local deployment path is cleanly SQLite-first
- live product-facing code no longer relies on Git-only authority behavior unless it is explicitly in the Git backend
- the service roles remain thin orchestration owners around the settled lower seams

### Phase 5: Implement Postgres through the same DB authority seam

Add the Postgres backend through the already-landed DB family seam rather than by forking the SQLite implementation or reworking upper layers.

This includes:

- implementing the Postgres DB authority adapter
- preserving the same authority-store semantic result shapes as SQLite
- validating that backend selection is configuration-driven rather than behaviorally divergent
- proving that the DB family abstraction was real and not only a SQLite wrapper

Exit criteria:

- Postgres is a real second implementation of the same DB family seam
- no public authority-contract redesign is needed to add it
- service deployment config can select Postgres cleanly

### Phase 6: Remove remaining migration mismatches, compatibility layers, and legacy coordination surfaces

After SQLite default is real, finish the cleanup by deleting the remaining compatibility code
instead of preserving it behind migration shims. Postgres stays deferred until after this breaking
cleanup.

This includes:

- deleting deprecated shared-ref compatibility aliases rather than leaving them at the product edge
- removing the runtime-status `shared_coordination_ref` field and other shared-ref-shaped
  compatibility response fields
- deleting `PlanView`, `CoordinationTaskView`, and any other old plan/task compatibility payloads
- cutting live MCP/query/UI coordination surfaces over to v2-only plan/task/artifact/review views
- removing remaining Git-shaped assumptions from architecture surfaces that are now wrong for the default path
- updating contracts, specs, roadmaps, and architecture docs to the actual steady state
- confirming the Git backend still works as a supported alternative rather than as the invisible default assumption
- removing the dual-snapshot read path so broker/runtime/UI plan surfaces stop carrying
  `CoordinationSnapshot` as a first-class production read model

Exit criteria:

- no product-facing module depends on migration-era compatibility aliases
- no live MCP/query/UI coordination surface depends on the old plan/task projection model
- v2 plan/task/artifact/review payloads are the only supported coordination surface model
- broker/runtime/UI read paths use `CoordinationSnapshotV2` directly for plan/task overlays and
  ready-task reads rather than threading legacy snapshots through those surfaces
- the code and docs agree on the authority family shape
- SQLite is the real default
- Git remains supported without distorting the public architecture story

## 5. Dependency logic

This ordering is intentional:

- seam hardening must come before the DB family so the family lands on stable public types
- the DB family seam must come before SQLite so SQLite does not become the de facto abstraction
- SQLite default must happen before broader service cleanup so the cleanup targets the right steady state
- Postgres should come after SQLite proves the DB family is real but before final cleanup so any seam gaps are discovered while the migration context is still active
- breaking compatibility cleanup should happen before Postgres so the second DB backend lands on the real steady-state surface instead of on transitional shims
- final docs closure should come after the breaking cleanup so it describes the system that actually exists

## 6. Anti-patterns to avoid

Do not:

- add a second public authority abstraction beside `CoordinationAuthorityStore`
- treat `DbCoordinationAuthorityStore` as a public product contract
- implement SQLite directly in upper layers and call it an abstraction later
- leave Git-only diagnostics, live-sync, or metadata behavior in shared public authority types if it can be isolated below the seam
- switch the default to SQLite while still depending on hidden Git-only helper paths for normal operation
- fork SQLite and Postgres behavior early instead of first proving the shared DB family seam
- let service roles or MCP surfaces grow new backend-specific branches that bypass the authority store
- leave compatibility fields, aliases, or legacy plan/task payloads in place when there are no consumers that require them

## 7. Exit criteria

This roadmap is complete only when all of the following are true:

- `CoordinationAuthorityStore` remains the sole product-facing authority contract
- the internal DB authority family exists and is the common path for SQLite and Postgres
- SQLite is the default functioning backend for `CoordinationAuthorityStore`
- the local service deployment path uses SQLite cleanly by default
- migration-era authority-edge compatibility code is deleted
- v2 plan/task/artifact/review payloads are the only live coordination surface model
- Postgres is implemented through the same internal DB seam
- Git shared refs remain supported without shaping upper layers incorrectly
- the remaining authority and service abstractions are cleaner, narrower, and better enforced than they are today

## 8. Short form

The sequence to hold the work to is:

1. harden the public authority seam
2. introduce one internal DB authority family
3. implement SQLite and make it the default
4. clean up service and authority adoption on the SQLite-first path
5. delete the remaining compatibility code and legacy coordination projections
6. implement Postgres through the same seam later
7. document the steady state
