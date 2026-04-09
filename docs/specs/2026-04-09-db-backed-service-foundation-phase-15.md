# DB-Backed Service Foundation Phase 15

Status: completed
Audience: service, storage, coordination, MCP, CLI, deployment, and release maintainers
Scope: establish the release-oriented DB-backed coordination authority family and refactor the current service host into explicit shell, authority-sync, read-broker, and mutation-broker roles

---

## 1. Summary

This spec is the concrete implementation target for the opening slice of roadmap Phase 15.

The goal is not to finish every future service feature in one pass. The goal is to land the
minimum clean foundation that turns the current service-backed coordination host into:

- a release-oriented DB-backed authority family
- one explicit service shell
- three explicit initial service roles:
  - authority sync
  - read broker
  - mutation broker

This phase builds on the already-settled lower seams:

- `CoordinationAuthorityStore`
- `CoordinationMaterializedStore`
- `CoordinationQueryEngine`
- `Transactional Coordination Mutation Protocol`
- service-owned coordination participation

This phase should preserve the rule that the service is the required coordination host around the
authority plane, not the authority plane itself.

## 2. Status

Current state:

- [x] service-backed coordination semantics are stable enough to build on
- [x] spec-engine surfaces are complete enough that service work does not need to absorb that gap
- [x] DB-backed authority family is now the live release-oriented path for local service-hosted
  coordination
- [x] product-facing authority-store construction no longer hardcodes Git store instantiation
- [x] the current host process now exposes an explicit workspace service shell plus initial
  authority-sync, read-broker, and mutation-broker owners
- [x] SQLite authority now exists behind the settled authority-store contract
- [x] runtime gateway foundation is complete
- [x] event-engine foundation is complete
- [ ] authoritative event-execution storage and mutation remain future service work

Current slice notes:

- this first Phase 15 spec intentionally covers the service foundation only
- it does not try to finish runtime gateway, event engine, browser login, or hosted admin flows
- the current `prism-mcp` daemon may remain the concrete host process in this slice if that is the
  cleanest path; the important change is explicit service role ownership, not crate theater
- local SQLite authority now uses a repo-scoped authority DB path that is distinct from the
  service-owned coordination materialization DB path
- release-oriented backend selection now resolves through one configured authority-provider path
- local service-hosted coordination now defaults to repo-scoped SQLite authority
- explicit daemon and bridge flags can still select Git shared refs or Postgres authority
- repo-local service configuration may also select the authority backend via `.prism/service.json`
- later Phase 15 slices now build out the dedicated event-engine namespace and richer service-role
  behavior on top of this completed foundation

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 4. Related contracts and prior specs

This spec depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-authority-sync-role.md](../contracts/service-authority-sync-role.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/service-runtime-gateway.md](../contracts/service-runtime-gateway.md)
- [../contracts/service-capability-and-authz.md](../contracts/service-capability-and-authz.md)

This spec follows:

- [2026-04-08-service-backed-coordination-cutover-phase-5.md](2026-04-08-service-backed-coordination-cutover-phase-5.md)
- [2026-04-09-trust-family-centralization-phase-6.md](2026-04-09-trust-family-centralization-phase-6.md)
- [2026-04-09-coordination-platform-freeze-phase-7.md](2026-04-09-coordination-platform-freeze-phase-7.md)

## 5. Scope

This phase includes:

- an internal DB-backed coordination authority family beneath `CoordinationAuthorityStore`
- a clean local single-instance deployment path using SQLite authority
- explicit service shell ownership around the current coordination host process
- explicit authority-sync, read-broker, and mutation-broker service role owners
- release-oriented configuration that can select DB-backed authority without changing upper-layer
  coordination semantics

This phase does not include:

- full runtime gateway implementation
- event-engine implementation inside the service
- hosted UI auth, browser session flows, or service-managed identities
- full multi-instance Postgres operations work such as migrations, HA, or hosted rollout polish
- deleting the Git authority backend

## 6. Non-goals

This phase should not:

- make the service itself authoritative
- bypass the settled authority, materialization, query, or mutation seams
- let runtimes fall back to runtime-owned coordination databases
- force a new top-level crate if an explicit role structure inside the current host crate is
  cleaner
- overbuild Postgres deployment behavior before the DB-backed authority family shape is real

## 7. Design

### 7.1 Owner rule

The release-oriented authority family should look like:

- `CoordinationAuthorityStore`
  - `DbCoordinationAuthorityStore`
    - `SqliteCoordinationDb`
    - `PostgresCoordinationDb`
  - `GitSharedRefsCoordinationAuthorityStore`

The DB-family abstractions are internal implementation seams, not new product-facing contracts.

### 7.2 Service-shell rule

The current host process must gain one explicit service-shell owner responsible for:

- process lifecycle
- configuration and repo partitioning
- role wiring
- transport and UI endpoint wiring
- trust plumbing shared by the service roles

That shell must not absorb coordination semantics itself.

### 7.3 Initial role rule

The first concrete service-role implementation set is:

- authority sync
- read broker
- mutation broker

Runtime gateway and event engine remain later work, but the shell and the first three roles should
be named and structured so those later roles fit without another large refactor.

### 7.4 DB-first release rule

The release-oriented deployment path for this phase is:

- local service + SQLite authority
- later hosted or multi-instance service + Postgres authority

Git shared refs remain supported, but they are not the release-critical path in this phase.

## 8. Implementation slices

### Slice 1: Introduce the DB-backed authority family seam

- define the internal DB-layer traits and type families needed beneath
  `CoordinationAuthorityStore`
- keep those seams below the product-facing authority contract
- define backend selection and service configuration inputs for:
  - SQLite authority
  - Postgres authority
  - Git authority

Exit criteria:

- DB-backed authority has one internal seam instead of SQLite- and Postgres-specific logic being
  scattered upward
- status: partially implemented

### Slice 2: Implement SQLite authority as the first DB-backed backend

- implement the SQLite-backed authority path through the DB authority family
- route current-state reads, retained history, descriptor access, and transactional mutations
  through it
- preserve the settled authority-store result and metadata shapes

Exit criteria:

- one local service deployment path can use SQLite authority through the settled authority-store
  contract

### Slice 3: Introduce the service shell

- give the current host process one explicit service-shell owner
- move lifecycle/config/repo-partition wiring under that shell
- keep existing behavior stable while making role ownership explicit

Exit criteria:

- the host is no longer just a pile of daemon wiring and handlers; it has one named service-shell
  owner

### Slice 4: Extract authority-sync, read-broker, and mutation-broker owners

- wrap the existing settled lower seams with explicit service-role owners
- route service-facing coordination reads and writes through those role boundaries
- keep trust and freshness shaping shared instead of duplicated

Exit criteria:

- the initial three service roles are explicit in code and own their stated responsibilities

### Slice 5: Add release-oriented deployment configuration

- add clear configuration for selecting the DB-backed authority family in the service
- support local SQLite authority by default
- shape the Postgres authority configuration without forcing full hosted rollout machinery yet

Exit criteria:

- the release-oriented deployment path is explicit in config and code structure
- status: completed

## 9. Validation

Minimum validation for this phase:

- targeted `prism-core` tests where authority-store or service-backed coordination behavior changes
- targeted `prism-mcp` tests for service-host read and mutation behavior
- targeted `prism-cli` tests where daemon startup or service config selection changes
- targeted DB-backend tests for SQLite authority behavior
- `git diff --check`

Important regression checks:

- service roles do not bypass the settled lower seams
- runtimes still participate through the service rather than falling back to local coordination
  state
- SQLite authority preserves current authoritative read, history, mutation, and descriptor
  semantics
- the host remains one process rather than accidental microservices or ad hoc global state

## 10. Completion criteria

This opening Phase 15 spec is complete only when:

- DB-backed authority is a real internal family beneath `CoordinationAuthorityStore`
- SQLite authority is usable as the clean local release-oriented path
- the host process has one explicit service shell
- authority sync, read broker, and mutation broker are explicit role owners in code
- the service remains a thin orchestration host around the authority plane

## 11. Implementation checklist

- [x] Introduce the DB-backed authority family seam
- [x] Implement SQLite authority through that seam
- [x] Introduce the service shell
- [x] Extract explicit authority-sync, read-broker, and mutation-broker owners
- [x] Add release-oriented deployment configuration
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
