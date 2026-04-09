# SQLite Versus Postgres Deployment Posture Phase 6

Status: completed
Audience: service, CLI, deployment, storage, and operations maintainers
Scope: make backend selection explicit for the first-class PRISM Service, warn loudly for
single-instance SQLite topology, and make Postgres selection the clear multi-instance path

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is to make the deployment posture explicit at service startup.

This slice should:

- select Postgres authority when `PRISM_POSTGRES_DSN` is set
- otherwise select SQLite authority by default
- warn loudly at service startup that SQLite mode is single-instance only
- make it explicit in startup/help/health surfaces that multi-instance deployments must use
  Postgres

That target has landed:

- `prism service up` and `prism service restart` now derive authority backend from
  `PRISM_POSTGRES_DSN`, otherwise hard-default to SQLite
- the `prism service` surface no longer accepts backend-selection flags
- service startup now warns loudly in SQLite mode that it is single-instance only and directs
  multi-instance operators to `PRISM_POSTGRES_DSN`
- service status now surfaces the effective coordination authority backend explicitly

This slice should not:

- implement service auth or machine-wide sessions yet
- introduce Postgres-only optional extra coordination materialization yet
- add a second fallback backend-selection mechanism beyond the accepted env/config rule

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 6: implement SQLite versus Postgres deployment posture and startup diagnostics

## 3. Related contracts and ADRs

This spec depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)

This spec follows:

- [2026-04-09-db-authority-first-read-path-phase-5.md](./2026-04-09-db-authority-first-read-path-phase-5.md)

## 4. Scope

This slice includes:

- service-side authority-backend selection rules for local and hosted use
- startup-time warnings and diagnostics for SQLite single-instance mode
- clear operator-facing messaging that Postgres is required for multi-instance deployments
- alignment of CLI and service status surfaces with that posture

This slice does not include:

- auth/session rollout
- runtime-session issuance
- service-mediated human approvals

## 5. Design constraints

- `PRISM_POSTGRES_DSN` must be the simple top-level switch for Postgres-backed service mode.
- If `PRISM_POSTGRES_DSN` is absent, the service must start in SQLite mode.
- SQLite mode must warn loudly that it is supported only for a single-instance PRISM Service
  topology.
- Multi-instance deployments must be directed to Postgres, not treated as “advanced SQLite.”
- The service must not silently mix multiple backend-selection sources with conflicting precedence.

## 6. Implementation slices

### Slice 1: Centralize service backend selection

- make service startup choose authority backend from:
  - `PRISM_POSTGRES_DSN`
  - otherwise SQLite default
- keep the chosen backend visible in service status and diagnostics

Exit criteria:

- service backend selection is explicit and deterministic

### Slice 2: Add loud SQLite single-instance warnings

- print explicit startup warnings in SQLite mode
- expose the same posture in service status or health diagnostics as appropriate

Exit criteria:

- local operators cannot plausibly mistake SQLite mode for a supported multi-instance topology

### Slice 3: Align CLI and docs-facing messaging

- keep `prism service` CLI output explicit about the current backend
- point multi-instance operators at Postgres in error/help text where useful

Exit criteria:

- deployment posture is clear from the public service lifecycle surface

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` tests for backend selection and startup/status messaging
- targeted service or MCP tests for surfaced backend posture where affected
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- service startup selects Postgres when `PRISM_POSTGRES_DSN` is present
- service startup otherwise selects SQLite
- SQLite startup warns loudly that it is single-instance only
- public service surfaces make Postgres the explicit multi-instance path

## 9. Implementation checklist

- [x] Centralize service backend selection
- [x] Add loud SQLite single-instance warnings
- [x] Align CLI and docs-facing messaging
- [x] Update roadmap and spec status after landing
