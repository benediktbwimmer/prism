# Service Endpoint Selection And Machine State Phase 3

Status: approved
Audience: service, CLI, MCP, runtime, UI, storage, and deployment maintainers
Scope: make service endpoint selection explicit, move service-owned discovery state under
machine-local PRISM home, and add an explicit temporary repo-enrollment bootstrap command for
pre-auth dogfooding

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is to make service discovery and pre-auth local dogfooding explicit.

This slice should:

- define one explicit service-endpoint resolution owner for CLI and runtime-facing surfaces
- keep explicit configured service endpoints authoritative and fail-loud when unavailable
- use machine-local PRISM home state for local service discovery rather than worktree-scoped
  ad hoc files
- add an explicit temporary `prism service enroll-repo` bootstrap path for local dogfooding before
  auth-backed capability-gated enrollment exists

This slice should not:

- implement signed login or service sessions yet
- implement capability-gated runtime-issued repo enrollment yet
- finish the runtime-only daemon cutover

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 3: implement service endpoint selection and machine-local service state

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-runtime-gateway.md](../contracts/service-runtime-gateway.md)
- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)

This spec follows:

- [2026-04-09-prism-service-lifecycle-phase-1.md](./2026-04-09-prism-service-lifecycle-phase-1.md)
- [2026-04-09-service-ui-hosting-phase-2.md](./2026-04-09-service-ui-hosting-phase-2.md)

## 4. Scope

This slice includes:

- one named endpoint-resolution owner module
- explicit resolution order:
  - configured endpoint
  - otherwise machine-local discovery
  - otherwise fail clearly
- machine-local service discovery state under `PRISM_HOME`
- a temporary explicit repo-enrollment bootstrap command under `prism service`

This slice does not include:

- browser auth
- machine-wide service sessions
- runtime-session issuance
- capability-gated enrollment

## 5. Design constraints

- If an explicit service endpoint is configured, it must fail loudly when unavailable. It must not
  silently fall back to a discovered local service.
- Local discovery state for the service belongs under machine-local PRISM home, not under
  worktree-local runtime state naming.
- The temporary repo-enrollment path must be explicit and CLI-only. It must not be hidden behind
  runtime connect or bridge startup behavior.
- The temporary enrollment path should be easy to delete once capability-gated enrollment lands.

## 6. Implementation slices

### Slice 1: Add a shared service-endpoint resolution owner

- centralize endpoint selection in one owner module
- support configured endpoint versus machine-local discovery versus fail
- keep failure messages explicit about which resolution mode failed

Exit criteria:

- service endpoint selection is no longer open-coded or implied by runtime-local MCP assumptions

### Slice 2: Move local service discovery state under machine-local service naming

- add service-owned local state paths under `PRISM_HOME`
- stop presenting the service endpoint as only a worktree-local MCP URI file concern
- keep the current host-process reuse internal while moving discovery ownership to the service

Exit criteria:

- local service discovery is machine-scoped and service-named

### Slice 3: Add explicit pre-auth repo enrollment bootstrap

- add `prism service enroll-repo`
- persist an explicit machine-local enrollment record for the current repo
- surface enrollment state clearly enough for dogfooding and later removal

Exit criteria:

- local dogfooding has an explicit repo-enrollment step instead of hidden auto-enrollment

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` tests for new service command parsing and endpoint-resolution behavior
- targeted tests for machine-local service discovery state
- targeted tests for repo-enrollment bootstrap behavior
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- service endpoint selection follows the accepted explicit ordering
- machine-local service discovery state is service-owned rather than runtime-named
- pre-auth dogfooding has an explicit CLI repo-enrollment bootstrap path

## 9. Implementation checklist

- [ ] Add a shared service-endpoint resolution owner
- [ ] Move local service discovery state under machine-local service naming
- [ ] Add explicit pre-auth repo enrollment bootstrap
- [ ] Update roadmap and spec status after landing
