# Coordination To Spec Engine To Service

Status: in progress
Audience: coordination, query, storage, runtime, MCP, CLI, UI, auth, and service maintainers
Scope: the sequential implementation order for finishing the service-backed coordination platform, establishing the DB-backed release path, then the full native spec engine, then the remaining richer PRISM Service roles

---

## 1. Summary

PRISM should complete its next major implementation era in one foundation-first ladder:

1. finish the coordination abstractions fully
2. cut the codebase over to a service-backed coordination platform
3. freeze coordination as the new platform
4. make the DB-backed authority family the release-oriented backend path on top of that stable
   platform
5. build the full native spec engine on top of that stable platform
6. implement the remaining richer PRISM Service roles on top of those settled seams

This roadmap exists to prevent:

- temporary bypasses
- duplicated seams
- half-migrated call sites
- fake abstractions
- a second cleanup wave later

The core rule is:

- build the foundations fully before building the next layer that depends on them

The release-oriented backend ordering is also explicit:

- DB-backed authority first
- Git-backed authority remains important, but is not the critical path to the first robust release

This roadmap now assumes the accepted decision in:

- [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

## 2. Status

Current phase checklist:

- [x] Phase 0: freeze coordination semantics
- [x] Phase 1: implement Coordination Authority Store fully
- [x] Phase 2: implement Coordination Materialized Store fully
- [x] Phase 3: implement Coordination Query Engine fully
- [x] Phase 4: implement Transactional Coordination Mutation Protocol fully
- [x] Phase 5: cut over service-backed coordination runtime and product surfaces
- [x] Phase 6: trust-family cleanup and centralization
- [x] Phase 7: freeze coordination as the base platform
- [x] Phase 8: implement spec engine source, parser, and identity model
- [x] Phase 9: implement spec local materialization
- [x] Phase 10: implement SpecQueryEngine fully
- [x] Phase 11: implement explicit spec-coordination linking and sync provenance
- [x] Phase 12: implement SpecCoverageView fully
- [x] Phase 13: implement explicit spec-to-coordination sync actions
- [x] Phase 14: expose the spec engine fully through CLI, MCP, and UI
- [ ] Phase 15: implement the remaining PRISM Service roles and release deployment modes

Current active phase:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

Current phase spec:

- Phase 2 completed:
  [../specs/2026-04-08-coordination-materialized-store-phase-2.md](../specs/2026-04-08-coordination-materialized-store-phase-2.md)
- Phase 3 completed:
  [../specs/2026-04-08-coordination-query-engine-phase-3.md](../specs/2026-04-08-coordination-query-engine-phase-3.md)
- Phase 4 completed:
  [../specs/2026-04-08-coordination-mutation-protocol-phase-4.md](../specs/2026-04-08-coordination-mutation-protocol-phase-4.md)
- Phase 5 completed:
  [../specs/2026-04-08-service-backed-coordination-cutover-phase-5.md](../specs/2026-04-08-service-backed-coordination-cutover-phase-5.md)
- Phase 6 completed:
  [../specs/2026-04-09-trust-family-centralization-phase-6.md](../specs/2026-04-09-trust-family-centralization-phase-6.md)
- Phase 7 completed:
  [../specs/2026-04-09-coordination-platform-freeze-phase-7.md](../specs/2026-04-09-coordination-platform-freeze-phase-7.md)
- Phase 8 completed:
  [../specs/2026-04-09-spec-engine-source-parser-identity-phase-8.md](../specs/2026-04-09-spec-engine-source-parser-identity-phase-8.md)
- Phase 9 completed:
  [../specs/2026-04-09-spec-engine-local-materialization-phase-9.md](../specs/2026-04-09-spec-engine-local-materialization-phase-9.md)
- Phase 10 prerequisite:
  [../specs/2026-04-09-spec-engine-crate-extraction-pre-phase-10.md](../specs/2026-04-09-spec-engine-crate-extraction-pre-phase-10.md)
- Phase 10 completed:
  [../specs/2026-04-09-spec-query-engine-phase-10.md](../specs/2026-04-09-spec-query-engine-phase-10.md)
- Phase 11 completed:
  [../specs/2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md](../specs/2026-04-09-spec-coordination-linking-and-sync-provenance-phase-11.md)
- Phase 12 completed:
  [../specs/2026-04-09-spec-coverage-phase-12.md](../specs/2026-04-09-spec-coverage-phase-12.md)
- Phase 13 completed:
  [../specs/2026-04-09-spec-sync-actions-phase-13.md](../specs/2026-04-09-spec-sync-actions-phase-13.md)
- Phase 14 completed:
  [../specs/2026-04-09-spec-engine-surfaces-phase-14.md](../specs/2026-04-09-spec-engine-surfaces-phase-14.md)
- Phase 15 active:
  [../specs/2026-04-09-db-backed-service-foundation-phase-15.md](../specs/2026-04-09-db-backed-service-foundation-phase-15.md)
- Phase 15 runtime-gateway slice completed:
  [../specs/2026-04-09-runtime-gateway-foundation-phase-15.md](../specs/2026-04-09-runtime-gateway-foundation-phase-15.md)
- Phase 15 event-engine slice completed:
  [../specs/2026-04-09-event-engine-foundation-phase-15.md](../specs/2026-04-09-event-engine-foundation-phase-15.md)
- Phase 15 event-execution model slice completed:
  [../specs/2026-04-09-event-execution-record-model-phase-15.md](../specs/2026-04-09-event-execution-record-model-phase-15.md)
- Phase 15 event-execution storage slice completed:
  [../specs/2026-04-09-event-execution-authority-storage-phase-15.md](../specs/2026-04-09-event-execution-authority-storage-phase-15.md)

## 3. Ordering thesis

This work should be sequential, not heavily parallelized.

The point is not just to make the system work again.
The point is to make the architecture structurally clean.

That means:

- finish the coordination seams first
- then finish the service-backed coordination platform
- then make SQLite and Postgres the release-oriented authority family on top of it
- then finish the full spec engine on top of it
- then expand the remaining richer service roles

## 4. Phases

### Phase 0: Freeze coordination semantics

Settle the semantic ground before more code movement.

This includes:

- v2 plan and task semantics
- artifact and review semantics
- strong versus eventual read semantics
- authority versus local materialization semantics
- identity, provenance, verification, and freshness vocabulary

Exit criteria:

- no more meaningful coordination-domain semantic churn while abstraction work proceeds

Current assessment:

- satisfied by the current contract set for coordination authority, artifact and review behavior,
  consistency and freshness, and trust-family semantics

### Phase 1: Implement the Coordination Authority Store

Implement the authority seam fully enough that all authoritative coordination access goes through
it.

This includes:

- current authoritative state reads
- transactional mutation commit path
- authoritative history access
- runtime descriptor access
- authority metadata, provenance, and freshness shape

Migration target:

- every direct shared-ref or `.git` coordination authority read or write path routes through this
  interface

Exit criteria:

- the rest of the app can no longer talk directly to coordination authority storage

Current assessment:

- completed by the new authority-store seam and Git-backed backend cutover
- authoritative current reads, commit, history, descriptor, diagnostics, and live-sync families
  now route through the authority boundary instead of direct product-facing shared-ref helpers

### Phase 2: Implement the Coordination Materialized Store

Implement the service-owned persistent read-model seam fully enough that all coordination
materialization goes through it.

This includes:

- service-local persistent coordination snapshot and read-model access
- checkpoint bundle access
- local version and authority-key tracking
- rebuild, replace, invalidate, and refresh operations
- eventual-read-facing storage contract

Migration target:

- no product or service code talks directly to the coordination SQLite schema except the
  materialized-store implementation

Exit criteria:

- service-owned coordination persistence is fully behind one disciplined seam

Current assessment:

- completed at the seam level; ownership target is now explicitly service-owned rather than
  runtime-owned per the accepted ADR, and later cutover work should converge implementation toward
  that target

### Phase 3: Implement the Coordination Query Engine

Implement the deterministic evaluation layer fully enough that all coordination reasoning flows
through it.

This includes:

- actionable tasks
- pending reviews
- required artifacts
- evidence status
- blocker reasoning
- review scope resolution
- task brief inputs
- plan rollups and derived plan state
- reclaimable, stale, and in-progress views
- coordination history views where part of the query contract

Migration target:

- MCP handlers, CLI commands, UI-facing reads, and service-backed runtime views stop embedding
  workflow evaluation logic directly

Exit criteria:

- all coordination reasoning lives here, not in product handlers

Current assessment:

- completed by the dedicated `prism-query` engine seam, evidence/review query families, stable
  query-surface exposure of task evidence and review status, and MCP/UI task-facing cutover away
  from duplicated blocker/artifact interpretation

### Phase 4: Implement the Transactional Coordination Mutation Protocol

Implement the write-side semantic seam fully.

This includes:

- mutation intent types
- transaction envelope
- validation ordering
- replay and retry behavior
- deterministic rejection
- commit result metadata
- pending versus committed semantics
- mutation provenance
- authoritative post-commit update rules

Migration target:

- MCP mutations
- host mutation helpers
- session mutation entry points
- any direct coordination-state mutation helpers

Exit criteria:

- all coordination writes happen through one explicit transactional protocol

Current assessment:

- completed at the mutation-protocol layer
- committed, rejected, indeterminate, and stale-base conflict outcomes now flow through one
  protocol story
- current coordination mutation surfaces and ordinary query-layer task create/update helpers now
  route through the transaction engine instead of parallel live-runtime mutation paths
- automatic replay remains intentionally unsupported in Phase 4; stale-base conflicts reject
  structurally and must be restaged explicitly by a caller or later mutation broker

### Phase 5: Cut over the service-backed coordination runtime and product surfaces to those four seams

Do the broad migration pass once the four core seams exist.

Touch:

- session and bootstrap paths
- watch and sync paths
- runtime refresh paths
- service-hosted coordination materialization ownership
- MCP host, query, and tool paths
- CLI read and mutate commands
- UI-facing runtime views
- diagnostics and history views
- checkpoint rebuild flows
- shared-ref and verification call sites
- any remaining direct SQLite or `.git` coordination helpers

Exit criteria:

- the coordination layer is fully expressed through the new abstractions
- no old direct authority, runtime-owned materialization, query, or mutation paths remain

### Phase 6: Implement the trust-family contracts in code where still needed

Centralize or fully enforce:

- identity model
- capability checks
- provenance envelopes
- signing boundaries
- verification results and failure handling
- freshness and trust metadata shapes

Exit criteria:

- authority reads and writes, descriptor publication, mutation results, and diagnostics all carry
  the same trust and provenance semantics

### Phase 7: Freeze coordination and call it the new base platform

Checkpoint the service-backed coordination layer before building the spec engine on top of it.

This includes:

- deleting remaining transitional shims
- updating docs to reflect the seams as implemented reality
- ensuring tests target the new abstractions

Exit criteria:

- coordination is stable enough that the spec engine can build on it without causing another
  abstraction rewrite

### Phase 8: Implement spec engine source, parser, and identity model

Implement:

- configurable spec root
- markdown plus frontmatter parsing
- repo-unique `spec_id`
- checklist extraction
- stable checklist identity
- dependency parsing
- source file metadata and source revision metadata

Exit criteria:

- repo files deterministically become native spec objects

### Phase 9: Implement spec local materialization

Implement a full local spec materialization layer:

- spec records
- checklist items
- dependency edges
- derived status
- coverage records
- sync provenance records
- source metadata

Exit criteria:

- spec state is persistent, local, disposable, rebuildable, and queryable

### Phase 10: Implement the SpecQueryEngine fully

Implement:

- list specs
- show spec
- checklist queries
- dependency graph and posture
- derived local spec status
- coverage
- sync provenance
- local joins with linked coordination objects

Exit criteria:

- all spec reasoning lives in one deterministic query seam

### Phase 11: Implement explicit spec-coordination linking and sync provenance

Implement:

- `spec_refs` on plans and tasks where appropriate
- sync provenance storage on explicitly created or synced coordination objects
- source spec revision tracking
- checklist and section identity tracking for sync
- drift detection inputs

Exit criteria:

- PRISM can explain which coordination objects came from which spec revision and why

### Phase 12: Implement SpecCoverageView fully

Implement:

- uncovered checklist items
- represented-by-plan-or-task state
- in-progress state
- completed state
- artifact- or review-backed state where available
- drift due to spec changes after sync
- coordination-done-but-not-reflected-back cases

Exit criteria:

- PRISM can answer what parts of a spec are actually covered right now

### Phase 13: Implement explicit spec-to-coordination sync actions

Implement:

- create plan from spec
- create tasks from sections, checklist items, or milestones
- sync spec milestones into coordination summaries
- later, if desired, sync completion back into checklist state

Exit criteria:

- the intent to execution to coverage loop is operational through explicit actions

Current assessment:

- completed by the bounded sync brief, explicit spec-aware plan and task sync helpers in
  `prism-query`, and end-to-end refresh validation showing that sync-created links feed native
  coverage and sync provenance views

### Phase 14: Expose the spec engine fully through CLI, MCP, and UI

Implement:

- `prism specs ...`
- MCP spec queries
- linked spec summaries in task and plan reads
- coverage views
- divergence and drift warnings
- human-friendly UI surfaces

Exit criteria:

- the spec engine is first-class and usable as the feature-intent layer

### Phase 15: Implement the remaining PRISM Service roles and release deployment modes

Implement in order:

1. release-oriented DB-backed authority family
   - SQLite single-instance deployment
   - Postgres hosted or multi-instance deployment
2. service shell
3. authority sync role
4. read broker role
5. mutation broker role
6. runtime gateway role
7. later event engine role

Current assessment:

- partially implemented
- initial service-shell, authority-sync, read-broker, and mutation-broker owners are already
  explicit in code
- SQLite authority now exists behind `CoordinationAuthorityStore` as the first DB-backed backend
- release-oriented backend selection and configuration now route through one authority-provider
  resolver, with local service-hosted coordination defaulting to repo-scoped SQLite authority
- runtime gateway is now explicit in code and owns runtime-targeted routing, descriptor
  resolution, and gateway-facing auth/capability checks
- the event-engine role is now explicit in the service shell and host surface as the later landing
  zone for scheduling behavior
- event-execution identity, lifecycle, and record-model types now exist in the domain crates
- remaining work is now centered on authoritative event storage, mutation, and later scheduling
  behavior rather than missing service-role ownership or missing event vocabulary

Exit criteria:

- the release-oriented deployment path uses the DB-backed authority family cleanly
- the service is built on the settled lower seams and stays thin rather than becoming a hidden
  authority blob

## 5. Dependency logic

This ordering is driven by real dependency structure:

- query engine depends on authority store and materialized store
- mutation protocol depends on authority store plus trust and provenance semantics
- full coordination cutover depends on all four coordination seams existing
- spec engine depends on coordination seams being stable
- spec coverage depends on both SpecQueryEngine and CoordinationQueryEngine
- spec sync depends on mutation, query, and provenance seams being stable
- service-backed runtime participation depends on service-owned coordination materialization
- release-oriented deployment depends on the DB-backed authority family landing on the settled
  coordination seams
- remaining richer service roles depend on all of the above being stable enough not to absorb
  missing architecture

This is why the work is mostly sequential.

## 6. Anti-patterns to avoid

Do not:

- parallelize by adding temporary bypasses
- leave direct authority or SQLite access behind “for now”
- leave runtime-owned coordination materialization behind “for now”
- couple branch-local spec state into authoritative coordination by accident
- build service roles before the lower seams are actually clean
- ship fake abstractions that still require handler-specific logic everywhere

## 7. Short form

The implementation order to stake the project on is:

1. freeze coordination semantics
2. authority store
3. materialized store
4. query engine
5. mutation protocol
6. full coordination cutover
7. trust and provenance cleanup
8. freeze service-backed coordination platform
9. full spec engine
10. full spec-coordination coverage and sync
11. DB-backed release deployment and remaining richer PRISM service roles
