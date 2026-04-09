# PRISM Service Lifecycle Phase 1

Status: approved
Audience: service, CLI, MCP, runtime, UI, and deployment maintainers
Scope: introduce a first-class `prism service` lifecycle surface in the CLI, backed by the current host process, without yet finishing the runtime-versus-service process split

---

## 1. Summary

This spec is the first concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is not to finish the full service extraction.
The goal is to make the service a first-class CLI concept immediately.

This slice should:

- add a new `prism service` command family
- support:
  - `prism service up`
  - `prism service stop`
  - `prism service restart`
  - `prism service status`
  - `prism service health`
- route those commands through one explicit service-lifecycle owner module in `prism-cli`
- preserve the current host behavior by delegating to the existing `prism-mcp` daemon lifecycle for
  now

This slice should not:

- complete the full service versus runtime process split
- move UI serving yet
- introduce auth or session enforcement
- remove existing `prism mcp` lifecycle commands

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 1: make `prism service` a first-class lifecycle surface

## 3. Related contracts and ADRs

This spec depends on:

- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)
- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-auth-and-session-model.md](../contracts/service-auth-and-session-model.md)

This spec builds on the earlier service-foundation work in:

- [2026-04-09-db-backed-service-foundation-phase-15.md](./2026-04-09-db-backed-service-foundation-phase-15.md)

## 4. Scope

This slice includes:

- CLI grammar for `prism service`
- one dedicated CLI module that owns service-lifecycle command handling
- service-lifecycle commands translating to the current host process while the deeper extraction is
  still in progress
- consistent user-facing messaging that treats the service as the primary lifecycle surface

This slice does not include:

- removing `prism mcp start|stop|restart|status|health`
- a separate `prism-service` binary
- hosted endpoint selection changes
- UI hosting changes
- auth, login, or repo-enrollment behavior

## 5. Design constraints

- `prism service` must be first-class in the CLI, not just undocumented alias behavior.
- The implementation may delegate to the current `prism-mcp` daemon lifecycle temporarily, but that
  delegation must live under a named service-lifecycle module rather than scattering service naming
  into the existing MCP command handler.
- The user-facing framing should treat `prism service` as the primary lifecycle surface.
- Existing `prism mcp` lifecycle commands must keep working during this slice so current workflows
  are not broken while the deeper extraction remains in progress.

## 6. Implementation slices

### Slice 1: Add CLI grammar for `prism service`

- add a `Service` top-level command to the CLI
- add a `ServiceCommand` enum for:
  - `Up`
  - `Stop`
  - `Restart`
  - `Status`
  - `Health`

Exit criteria:

- `prism service ...` parses as a first-class command family

### Slice 2: Add a dedicated service lifecycle owner module

- create a `service` module in `prism-cli`
- make that module own the translation from `ServiceCommand` to the current host lifecycle path
- keep `commands.rs` thin

Exit criteria:

- `prism-cli` has one named service lifecycle owner instead of embedding this logic directly in the
  CLI dispatch

### Slice 3: Delegate to the current host process cleanly

- wire `prism service up|stop|restart|status|health` through the current `prism-mcp` lifecycle
  implementation
- preserve existing behavior for now
- update help text and user-facing messages so the service terminology is primary in this path

Exit criteria:

- users can manage the service through `prism service ...` today
- the implementation is explicit about the temporary delegation to the current host

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` parser tests for the new `prism service` command family
- targeted `prism-cli` tests for delegation behavior where practical
- direct compile coverage for any changed CLI dispatch wiring
- `git diff --check`

Important checks:

- existing `prism mcp` lifecycle command parsing still works
- `prism service` does not silently invent login or service boot behavior beyond the command the
  user explicitly ran

## 8. Completion criteria

This spec is complete when:

- `prism service` exists as a first-class CLI surface
- `up`, `stop`, `restart`, `status`, and `health` are supported
- the service lifecycle handling lives in its own module
- the implementation is explicit that this slice still delegates to the current host process

## 9. Implementation checklist

- [ ] Add `Service` top-level CLI command and `ServiceCommand`
- [ ] Add dedicated service lifecycle owner module in `prism-cli`
- [ ] Delegate `prism service` lifecycle commands to the current host path
- [ ] Validate changed `prism-cli` parsing and dispatch
- [ ] Update roadmap status after landing
