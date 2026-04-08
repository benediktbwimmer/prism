# Coordination To Spec Engine To Service

Status: draft
Audience: coordination, query, storage, runtime, MCP, CLI, UI, auth, and service maintainers
Scope: the sequential implementation order for finishing the coordination platform, then the full native spec engine, then the PRISM Service

---

## 1. Summary

PRISM should complete its next major implementation era in one foundation-first ladder:

1. finish the coordination abstractions fully
2. cut the codebase over to those abstractions completely
3. freeze coordination as the new platform
4. build the full native spec engine on top of that stable platform
5. only then implement the PRISM Service on top of those settled seams

This roadmap exists to prevent:

- temporary bypasses
- duplicated seams
- half-migrated call sites
- fake abstractions
- a second cleanup wave later

The core rule is:

- build the foundations fully before building the next layer that depends on them

## 2. Status

Current phase checklist:

- [x] Phase 0: freeze coordination semantics
- [ ] Phase 1: implement Coordination Authority Store fully
- [ ] Phase 2: implement Coordination Materialized Store fully
- [ ] Phase 3: implement Coordination Query Engine fully
- [ ] Phase 4: implement Transactional Coordination Mutation Protocol fully
- [ ] Phase 5: cut over coordination runtime and product surfaces
- [ ] Phase 6: trust-family cleanup and centralization
- [ ] Phase 7: freeze coordination as the base platform
- [ ] Phase 8: implement spec engine source, parser, and identity model
- [ ] Phase 9: implement spec local materialization
- [ ] Phase 10: implement SpecQueryEngine fully
- [ ] Phase 11: implement explicit spec-coordination linking and sync provenance
- [ ] Phase 12: implement SpecCoverageView fully
- [ ] Phase 13: implement explicit spec-to-coordination sync actions
- [ ] Phase 14: expose the spec engine fully through CLI, MCP, and UI
- [ ] Phase 15: implement the PRISM Service contracts

Current active phase:

- Phase 1: implement Coordination Authority Store fully

## 3. Ordering thesis

This work should be sequential, not heavily parallelized.

The point is not just to make the system work again.
The point is to make the architecture structurally clean.

That means:

- finish the coordination seams first
- then finish the full spec engine on top of them
- then build the service on top of the settled platform

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

### Phase 2: Implement the Coordination Materialized Store

Implement the local persistent read-model seam fully enough that all local coordination
materialization goes through it.

This includes:

- local persistent coordination snapshot and read-model access
- checkpoint bundle access
- local version and authority-key tracking
- rebuild, replace, invalidate, and refresh operations
- eventual-read-facing storage contract

Migration target:

- no product or runtime code talks directly to the coordination SQLite schema except the
  materialized-store implementation

Exit criteria:

- local coordination persistence is fully behind one disciplined seam

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

- MCP handlers, CLI commands, UI-facing reads, and runtime views stop embedding workflow evaluation
  logic directly

Exit criteria:

- all coordination reasoning lives here, not in product handlers

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

### Phase 5: Cut over the coordination runtime to those four seams

Do the broad migration pass once the four core seams exist.

Touch:

- session and bootstrap paths
- watch and sync paths
- runtime refresh paths
- MCP host, query, and tool paths
- CLI read and mutate commands
- UI-facing runtime views
- diagnostics and history views
- checkpoint rebuild flows
- shared-ref and verification call sites
- any remaining direct SQLite or `.git` coordination helpers

Exit criteria:

- the coordination layer is fully expressed through the new abstractions
- no old direct authority, materialization, query, or mutation paths remain

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

Checkpoint the coordination layer before building the spec engine on top of it.

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

### Phase 15: Implement the PRISM Service contracts

Implement in order:

1. service shell
2. authority sync role
3. read broker role
4. mutation broker role
5. runtime gateway role
6. later event engine role

Exit criteria:

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
- service depends on all of the above being stable enough not to absorb missing architecture

This is why the work is mostly sequential.

## 6. Anti-patterns to avoid

Do not:

- parallelize by adding temporary bypasses
- leave direct authority or SQLite access behind “for now”
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
8. freeze coordination platform
9. full spec engine
10. full spec-coordination coverage and sync
11. PRISM service
