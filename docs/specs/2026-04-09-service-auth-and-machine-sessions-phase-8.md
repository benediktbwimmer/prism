# Service Auth And Machine Sessions Phase 8

Status: not started
Audience: service, auth, CLI, MCP, UI, runtime, and provenance maintainers
Scope: implement explicit service login, machine-wide service sessions, and the first shared
session-failure surfaces for CLI, runtime, MCP, and UI startup

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is to replace the remaining vague local-trust assumption with explicit
service authentication and reusable machine-wide service sessions.

This slice should:

- add `prism auth login` as the explicit service-login entrypoint
- authenticate principals to the service through signed challenge flow
- store a machine-wide service session under `PRISM_HOME`
- make MCP/runtime/UI startup report missing or expired sessions clearly

This slice should not:

- implement runtime-session issuance yet
- implement capability-gated repo enrollment yet
- implement `service_mediated_human` approval flows yet

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 8: implement explicit service auth and machine-wide service sessions

## 3. Related contracts and ADRs

This spec depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../contracts/service-auth-and-session-model.md](../contracts/service-auth-and-session-model.md)
- [../contracts/identity-model.md](../contracts/identity-model.md)
- [../contracts/signing-and-verification.md](../contracts/signing-and-verification.md)
- [../contracts/provenance.md](../contracts/provenance.md)

This spec follows:

- [2026-04-09-sqlite-vs-postgres-deployment-posture-phase-6.md](./2026-04-09-sqlite-vs-postgres-deployment-posture-phase-6.md)

## 4. Scope

This slice includes:

- service challenge issuance and verification
- machine-scoped service-session persistence
- session reuse for local MCP/runtime clients
- explicit startup/login failure messaging

This slice does not include:

- runtime-session issuance
- capability-gated repo enrollment
- browser-mediated human approval

## 5. Design constraints

- Login must remain explicit. Service startup and bridge startup must not silently log in.
- The principal key must only be used locally to establish or renew a service session.
- Machine-wide service sessions may be reused by local runtimes and MCP clients, but must remain
  clearly machine-scoped, expiring, and principal-bound.
- Missing or expired service sessions must surface clearly through CLI and startup surfaces rather
  than degrading into implicit trust.

## 6. Implementation slices

### Slice 1: Add explicit service login and session persistence

- implement `prism auth login`
- issue a service challenge
- verify signed response
- persist a machine-wide service session

Exit criteria:

- local participation can establish an explicit reusable service session without retaining unlocked
  key material

### Slice 2: Reuse machine-wide sessions from runtime and MCP startup

- make runtime/MCP startup load the machine-wide service session
- fail clearly when the session is missing or expired

Exit criteria:

- local runtimes and MCP clients no longer depend on hidden local trust assumptions

### Slice 3: Surface session posture explicitly

- expose session posture in CLI, startup, and service-facing status where appropriate
- keep the failure path explicit enough for agents to forward to users

Exit criteria:

- missing or expired login is a clear operator-facing condition, not an ambiguous runtime failure

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` tests for login/session parsing and machine-session behavior
- targeted service or MCP tests for missing-session startup surfaces
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- service login is explicit and challenge-based
- a machine-wide service session can be established and reused
- local startup surfaces fail clearly when the session is missing or expired

## 9. Implementation checklist

- [ ] Add explicit service login and session persistence
- [ ] Reuse machine-wide sessions from runtime and MCP startup
- [ ] Surface session posture explicitly
- [ ] Update roadmap and spec status after landing
