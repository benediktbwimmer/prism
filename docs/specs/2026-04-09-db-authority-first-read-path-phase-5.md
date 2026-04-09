# DB Authority-First Read Path Phase 5

Status: completed
Audience: service, coordination, query, runtime, MCP, CLI, and storage maintainers
Scope: collapse the default DB-backed coordination read path onto the authoritative backend,
disable separate coordination materialization by default for SQLite and Postgres authority, and
keep any future extra materialization as an explicit Postgres-only optimization

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is to remove redundant coordination materialization from the standard
DB-backed service path.

This slice should:

- make DB-backed strong coordination reads come directly from the authority backend
- make DB-backed eventual coordination reads collapse to the same authority path by default
- disable separate coordination materialization by default for SQLite and Postgres authority
- keep any future extra materialization behind explicit Postgres-only configuration

That target has landed:

- DB-backed service read-broker paths now route directly through the authority store by default
- the SQLite authority backend now treats eventual reads as authority-backed current reads unless a
  future explicit lagging projection is introduced
- DB-backed coordination mutations and runtime-descriptor writes no longer recreate local
  coordination materialization as part of the standard topology
- runtime status and CLI diagnostics now surface DB-backed coordination materialization as disabled
  rather than as a required local database

This slice should not:

- change Git-backed authority behavior
- remove the coordination materialized-store contract entirely
- introduce Postgres-specific performance caches yet

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 5: simplify DB-backed coordination reads to authority-first by default

## 3. Related contracts and ADRs

This spec depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)

This spec follows:

- [2026-04-09-runtime-only-mcp-and-bridge-launch-phase-4.md](./2026-04-09-runtime-only-mcp-and-bridge-launch-phase-4.md)

## 4. Scope

This slice includes:

- DB-backed authority read-broker routing changes
- removal of default DB-backed coordination materialization dependency from the service read path
- explicit preservation of Git-backed materialization behavior
- freshness and source reporting that remains honest even when eventual collapses to strong

This slice does not include:

- auth and session work
- SQLite versus Postgres startup warnings and backend selection UX
- new Postgres-only optional materialization config

## 5. Design constraints

- Strong versus eventual remains a semantic contract, not a storage-file choice.
- For DB-backed authority with no lagging projection configured, eventual may legitimately resolve
  through the same authority path as strong.
- The standard DB-backed path must not keep writing or depending on a second coordination SQLite
  materialization database.
- Git-backed authority may continue to use coordination materialization where it remains meaningful.

## 6. Implementation slices

### Slice 1: Route DB-backed reads directly through authority

- identify read-broker paths that still require DB-backed coordination materialization
- route DB-backed strong reads directly through the authority-backed path
- route DB-backed eventual reads through the same path by default

Exit criteria:

- the service read path no longer depends on separate DB-backed coordination materialization

### Slice 2: Disable default DB-backed coordination materialization

- stop default SQLite/Postgres service startup from expecting or creating separate coordination
  materialization for reads
- keep the materialized-store seam available for Git-backed authority and future Postgres-only opt-in

Exit criteria:

- the default DB-backed topology no longer carries redundant coordination materialization state

### Slice 3: Align freshness and diagnostics surfaces

- keep freshness/source reporting explicit
- avoid exposing strong/eventual jargon as a primary user-facing distinction
- preserve enough metadata for diagnostics and future optimization work

Exit criteria:

- DB-backed reads are simpler without making freshness reporting less honest

## 7. Validation

Minimum validation for this slice:

- targeted `prism-core` tests for DB-backed authority and read-broker behavior
- targeted `prism-mcp` tests for runtime status and coordination read surfaces affected by the new
  DB-backed path
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- DB-backed coordination reads are authority-first by default
- separate coordination materialization is no longer part of the standard SQLite/Postgres read path
- Git-backed materialization behavior remains intact where still needed
- freshness/source reporting still reflects the real read path honestly

## 9. Implementation checklist

- [x] Route DB-backed reads directly through authority
- [x] Disable default DB-backed coordination materialization
- [x] Align freshness and diagnostics surfaces
- [x] Update roadmap and spec status after landing
