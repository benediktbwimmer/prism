# First-Class PRISM Service And Auth Rollout

Status: in progress
Audience: service, runtime, MCP, CLI, UI, auth, storage, and deployment maintainers
Scope: implementing the first-class PRISM Service, explicit auth and session model, UI decoupling, browser-mediated human approvals, and the DB-backed authority-first read path

---

## 1. Summary

PRISM has accepted a new architectural center of gravity:

- `prism service` is a first-class product surface
- the MCP daemon is the worktree-local runtime and MCP server only
- the browser UI is served by the PRISM Service
- service participation uses explicit service and runtime sessions
- browser-session human approvals use `service_mediated_human`
- DB-backed authority reads go directly to the authority backend by default
- separate coordination materialization is disabled by default for DB-backed authority

This roadmap exists to turn that accepted architecture into shipped behavior without smearing the
work across unrelated event-engine or broader platform items.

This roadmap depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

This roadmap is a focused Phase 15 follow-through under:

- [2026-04-08-coordination-to-spec-engine-to-service.md](./2026-04-08-coordination-to-spec-engine-to-service.md)

## 2. Status

Current phase checklist:

- [ ] Phase 0: freeze ADR-aligned service and auth semantics
- [ ] Phase 1: make `prism service` a first-class lifecycle surface
- [ ] Phase 2: move UI serving fully under the service
- [ ] Phase 3: implement service endpoint selection and machine-local service state
- [ ] Phase 4: implement explicit service auth and machine-wide service sessions
- [ ] Phase 5: implement runtime-session issuance and capability-gated repo enrollment
- [ ] Phase 6: implement `service_mediated_human` approval flow and provenance
- [ ] Phase 7: cut MCP daemon over to runtime-only ownership and bridge-managed launch
- [ ] Phase 8: simplify DB-backed coordination reads to authority-first by default
- [ ] Phase 9: implement SQLite versus Postgres deployment posture and startup diagnostics

Current active phase:

- Phase 0: freeze ADR-aligned service and auth semantics

## 3. Ordering thesis

This work should be sequential.

The service cannot become first-class cleanly if:

- lifecycle is still implied by MCP
- auth is still transport-shaped instead of session-shaped
- UI approval semantics are not frozen
- DB-backed read behavior still depends on old materialization assumptions

The right order is:

1. freeze semantics
2. make the service explicit
3. make auth explicit
4. make the UI and MCP boundaries explicit
5. then simplify the DB-backed runtime path against that settled service model

## 4. Phases

### Phase 0: Freeze ADR-aligned service and auth semantics

Settle the normative behavior before further code movement.

This includes:

- first-class `prism service`
- explicit service startup
- explicit login
- no implicit service boot
- explicit service endpoint selection
- direct hosted-service connectivity
- service-owned UI
- principal-rooted service and runtime sessions
- `delegated_machine`
- `service_mediated_human`
- `human_attested`
- `service_attested`
- DB-backed no-materialization default
- SQLite single-instance posture

Exit criteria:

- ADR, contracts, and this roadmap are aligned and implementation-ready

### Phase 1: Make `prism service` a first-class lifecycle surface

Implement:

- `prism service up`
- `prism service stop`
- `prism service restart`
- `prism service status`
- `prism service health`
- optional later `prism service logs` and `prism service doctor`

Exit criteria:

- the service is no longer bootstrapped only as a side effect of the MCP daemon
- local service lifecycle is explicit and inspectable

### Phase 2: Move UI serving fully under the service

Implement:

- service-owned UI hosting
- service-owned browser transport plumbing
- removal of MCP-hosted UI serving paths

Exit criteria:

- browser UI is served by the service only
- MCP daemon is no longer a UI host

### Phase 3: Implement service endpoint selection and machine-local service state

Implement:

- explicit endpoint config
- machine-local service discovery under `PRISM_HOME` or `~/.prism`
- fail-loud behavior when an explicit endpoint is configured but unavailable
- no silent fallback from explicit hosted endpoint to local service

Exit criteria:

- local and hosted service discovery are deterministic and explicit

### Phase 4: Implement explicit service auth and machine-wide service sessions

Implement:

- `prism auth login`
- signed challenge flow
- machine-wide service session storage
- session renewal and expiry
- session failure surfaces for CLI, MCP, and UI startup

Exit criteria:

- service participation no longer relies on vague local trust assumptions
- runtimes and MCP can reuse a machine-wide service session without holding the principal key

### Phase 5: Implement runtime-session issuance and capability-gated repo enrollment

Implement:

- delegated runtime sessions bound to principal, runtime, repo, and optional worktree
- runtime registration or resume under a service session
- automatic repo proposal on runtime connect
- capability-gated repo enrollment

Exit criteria:

- runtimes are no longer treated as roots of trust
- auto-registration is explicit and policy-controlled

### Phase 6: Implement `service_mediated_human` approval flow and provenance

Implement:

- browser-session-backed human approval path
- service attestation over principal, session, service identity, and canonical action digest
- policy that allows `service_mediated_human` for ordinary human-gated UI actions
- stricter `human_attested` path for higher-assurance actions
- provenance and audit surfaces that distinguish:
  - delegated agent activity
  - service-mediated human approvals
  - direct human attestation
  - service attestation

Exit criteria:

- ordinary UI human approvals are smooth
- audit trails remain principal-attributed and service-attributed

### Phase 7: Cut MCP daemon over to runtime-only ownership and bridge-managed launch

Implement:

- MCP daemon as worktree-local runtime plus MCP server only
- optional `prism runtime` alias to `prism mcp`
- bridge-managed daemon launch and restart
- `prism://startup` surfacing daemon restart, missing login, and missing service clearly
- no implicit login and no implicit service boot

Exit criteria:

- the MCP daemon is clearly a runtime, not the service host
- bridge UX is smooth without hiding the real auth and service control points

### Phase 8: Simplify DB-backed coordination reads to authority-first by default

Implement:

- DB-backed strong reads directly from the authority backend
- DB-backed eventual reads collapsing to the same authority path by default
- separate coordination materialization disabled by default for SQLite and Postgres authority
- optional future Postgres-only materialization remaining behind explicit config

Exit criteria:

- the standard DB-backed read path no longer depends on redundant coordination materialization
- freshness is surfaced honestly without exposing storage jargon as the primary UX

### Phase 9: Implement SQLite versus Postgres deployment posture and startup diagnostics

Implement:

- `PRISM_POSTGRES_DSN` backend selection
- SQLite fallback when it is absent
- loud startup warning for SQLite single-instance topology
- explicit multi-instance guidance that Postgres is required

Exit criteria:

- deployment posture is explicit in startup behavior and docs
- local single-machine and hosted multi-instance modes are cleanly separated

## 5. Dependency logic

This rollout is topological:

- lifecycle must be explicit before clients can depend on it
- endpoint and auth semantics must be explicit before runtime registration is hardened
- UI approval semantics depend on the settled auth/session model
- MCP runtime-only ownership depends on the service being first-class
- DB-backed read simplification depends on the settled service boundary and deployment model

## 6. Anti-patterns to avoid

Do not:

- keep `prism-mcp` as the de facto service host while adding a nominal `prism service`
- silently fall back from an explicit hosted endpoint to a local machine service
- silently log in or unlock a principal
- treat browser-session human approval as equivalent to direct human signature
- collapse agent and UI human provenance into one vague actor field
- keep a separate coordination SQLite materialization enabled by default in DB-backed mode
- rely on plain environment-variable service secrets for production service attestation

## 7. Exit condition

This roadmap is complete when:

- `prism service` is the explicit service host
- UI is served by the service
- MCP daemon is runtime-only
- service and runtime sessions are explicit
- `service_mediated_human` is implemented and auditable
- DB-backed authority-first reads are the standard path
- SQLite and Postgres deployment posture is explicit and enforceable
