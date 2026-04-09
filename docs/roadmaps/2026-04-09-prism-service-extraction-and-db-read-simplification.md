# PRISM Service Extraction And DB Read Simplification

Status: in progress
Audience: service, runtime, MCP, CLI, UI, auth, storage, and deployment maintainers
Scope: extracting a first-class `prism service`, decoupling UI and MCP ownership, settling service auth, and simplifying the DB-backed coordination read path

---

## 1. Summary

PRISM has crossed the architectural threshold where the service is the real coordination host, but
the repo still reflects an intermediate implementation shape:

- the MCP daemon still carries too much implicit service identity
- service lifecycle is not yet first-class in the CLI
- UI serving is not yet fully owned by the PRISM Service
- service auth and runtime-session behavior need a tighter implementation target
- DB-backed authority still carries too much materialization-era complexity in the default path

This roadmap tracks the cleanup required to make the intended product shape explicit and shippable:

1. first-class PRISM Service process and lifecycle
2. explicit auth and session model
3. MCP daemon as worktree-local runtime only
4. UI served by the service
5. DB-backed read path without separate coordination materialization by default
6. SQLite single-instance posture and Postgres multi-instance posture

This roadmap depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../adrs/2026-04-08-db-backed-coordination-authority-first.md](../adrs/2026-04-08-db-backed-coordination-authority-first.md)

## 2. Status

Current phase checklist:

- [ ] Phase 0: freeze service extraction and auth semantics
- [ ] Phase 1: make `prism service` a first-class lifecycle surface
- [ ] Phase 2: decouple UI serving from the MCP daemon
- [ ] Phase 3: implement explicit service auth and session handling
- [ ] Phase 4: cut MCP daemon over to runtime-only ownership
- [ ] Phase 5: simplify DB-backed coordination reads to authority-first by default
- [ ] Phase 6: implement backend posture and startup diagnostics for SQLite versus Postgres
- [ ] Phase 7: harden bridge-managed MCP launch and restart against the explicit service model

Current active phase:

- Phase 0: freeze service extraction and auth semantics

## 3. Ordering thesis

This work should not be treated as a grab-bag of small CLI and daemon tweaks.

The point is to cleanly separate:

- service ownership
- runtime ownership
- UI ownership
- auth ownership
- DB-backed authority read behavior

The order matters:

- freeze the semantics first
- make the service first-class
- then cut consumers over to it
- then simplify the DB-backed read path against the settled service model

## 4. Phases

### Phase 0: Freeze service extraction and auth semantics

Settle the decision set before implementation fans out.

This includes:

- one first-class PRISM Service process
- explicit service startup and login
- no implicit service boot
- direct hosted-service connectivity
- MCP daemon as worktree-local runtime only
- UI served by the service
- principal-rooted auth with service and runtime sessions
- delegated machine versus human/service attestation
- DB-backed no-materialization default
- SQLite single-instance posture

Exit criteria:

- ADR and contracts are aligned and implementation-ready

### Phase 1: Make `prism service` a first-class lifecycle surface

Implement:

- `prism service up`
- `prism service stop`
- `prism service restart`
- `prism service status`
- `prism service health`
- machine-scoped service state location under `PRISM_HOME` or `~/.prism`
- explicit endpoint discovery state for local mode

Exit criteria:

- service lifecycle is a first-class CLI surface
- service identity is no longer implied by the MCP daemon lifecycle

### Phase 2: Decouple UI serving from the MCP daemon

Implement:

- service-owned UI serving
- service-owned browser transport plumbing
- MCP daemon no longer serving the UI

Exit criteria:

- browser UI is hosted by the PRISM Service
- MCP daemon is no longer a hidden UI host

### Phase 3: Implement explicit service auth and session handling

Implement:

- `prism auth login`
- machine-wide service session storage
- principal-backed signed challenge flow
- runtime-session issuance and renewal
- capability-gated repo enrollment on runtime connect
- browser-session flow on top of service auth
- initial human-attestation plumbing boundary

Exit criteria:

- local and hosted service participation use one explicit auth/session model
- runtimes no longer act like roots of trust

### Phase 4: Cut MCP daemon over to runtime-only ownership

Implement:

- MCP daemon as worktree-local runtime plus MCP server only
- optional `prism runtime` alias to `prism mcp`
- no hidden service-host semantics in MCP lifecycle
- bridge behavior that may launch or restart MCP daemon only
- startup diagnostics that surface missing service or auth state clearly

Exit criteria:

- MCP daemon is the runtime surface, not the service host
- bridge-assisted daemon launch and restart do not hide service or auth requirements

### Phase 5: Simplify DB-backed coordination reads to authority-first by default

Implement:

- DB-backed coordination reads that go directly to the authority path by default
- no separate coordination materialized store in the default SQLite or Postgres path
- strong/eventual collapse where appropriate for DB-backed authority
- preserved consistency envelope and freshness semantics

Exit criteria:

- DB-backed authority no longer requires redundant coordination materialization by default
- strong and eventual remain semantic contracts rather than user-facing storage jargon

### Phase 6: Implement backend posture and startup diagnostics for SQLite versus Postgres

Implement:

- Postgres selected automatically when `PRISM_POSTGRES_DSN` is present
- SQLite selected otherwise
- loud startup warning for SQLite single-instance posture
- loud failure or refusal for unsupported multi-instance SQLite deployment

Exit criteria:

- backend selection is simple
- deployment constraints are explicit in CLI and service startup behavior

### Phase 7: Harden bridge-managed MCP launch and restart

Implement:

- bridge detection of missing worktree-local MCP daemon
- background daemon launch
- background daemon restart when temporarily unavailable
- `prism://startup` reporting restart and warmup clearly
- no implicit login and no implicit service boot

Exit criteria:

- agent UX is smooth for MCP daemon availability
- service and auth remain explicit control points

## 5. Dependency logic

This roadmap is mostly sequential because:

- service lifecycle should exist before clients are cut over to it
- auth/session semantics should exist before runtime registration is finalized
- MCP runtime-only ownership depends on first-class service ownership
- DB-backed read simplification depends on the settled service and authority posture

## 6. Anti-patterns to avoid

Avoid:

- keeping `prism-mcp` as the de facto service host while adding a nominal `prism service`
- silently falling back from an explicit hosted endpoint to a local machine service
- silently auto-logging in or auto-unlocking identities
- keeping a separate coordination SQLite materialization enabled by default for DB-backed
  authority
- using reusable human or service elevation bearer tokens
- putting production service signing secrets only in environment variables

## 7. Relationship to the broader roadmap

This roadmap is a focused continuation under Phase 15 of:

- [2026-04-08-coordination-to-spec-engine-to-service.md](./2026-04-08-coordination-to-spec-engine-to-service.md)

It exists because the remaining work is no longer just “add another service role.” It is now about
making the PRISM Service a first-class product surface and simplifying the DB-backed deployment
path around it.
