# Runtime-Only MCP And Bridge Launch Phase 4

Status: completed
Audience: service, MCP, runtime, CLI, bridge, and UI maintainers
Scope: cut the MCP daemon over to runtime-only ownership, align bridge startup messaging with that
runtime model, and remove remaining service-host framing from MCP lifecycle behavior

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal of this slice is to make the worktree-local MCP daemon clearly a runtime surface rather
than a service host.

This slice should:

- align CLI help, status, and error text around MCP-as-runtime language
- make bridge-managed daemon launch and restart explicitly about the worktree-local runtime
- remove remaining service-host framing from MCP daemon lifecycle behavior
- keep service boot and auth explicit even while bridge-managed runtime launch stays automatic

This slice should not:

- change the explicit service lifecycle model
- introduce service auth yet
- change browser auth or approval flows

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 4: cut MCP daemon over to runtime-only ownership and bridge-managed launch

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-runtime-gateway.md](../contracts/service-runtime-gateway.md)
- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)

This spec follows:

- [2026-04-09-prism-service-lifecycle-phase-1.md](./2026-04-09-prism-service-lifecycle-phase-1.md)
- [2026-04-09-service-ui-hosting-phase-2.md](./2026-04-09-service-ui-hosting-phase-2.md)
- [2026-04-09-service-endpoint-selection-and-machine-state-phase-3.md](./2026-04-09-service-endpoint-selection-and-machine-state-phase-3.md)

## 4. Scope

This slice includes:

- MCP lifecycle copy and command behavior that treats the daemon as the worktree-local runtime
- bridge-managed runtime launch and restart messaging
- error guidance that distinguishes:
  - missing runtime daemon
  - missing PRISM service
  - missing login

This slice does not include:

- service login
- runtime-session issuance
- service-mediated human approvals

## 5. Design constraints

- The bridge may auto-launch or auto-restart the MCP daemon, but that behavior must be framed as
  runtime management rather than service boot.
- Missing service or login must stay explicit failures surfaced to the agent and user. The bridge
  must not silently start the service or login on the user’s behalf.
- `prism runtime` may become an alias to `prism mcp`, but there must not be a second distinct
  runtime process concept.

## 6. Implementation slices

### Slice 1: Align MCP lifecycle language to runtime ownership

- update CLI help and status output so the daemon is described as the runtime
- remove remaining service-host framing from MCP lifecycle paths

Exit criteria:

- the product language matches the accepted runtime/service split

### Slice 2: Align bridge startup and restart messaging

- make bridge startup feedback explicitly about runtime launch and restart
- keep `prism://startup` and related feedback consistent with that runtime framing

Exit criteria:

- bridge-managed daemon lifecycle is smooth without implying hidden service boot

### Slice 3: Add optional `prism runtime` alias

- expose `prism runtime` as an alias to `prism mcp`
- keep one underlying lifecycle owner

Exit criteria:

- the runtime terminology is available without creating a second process model

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` tests for MCP/runtime parsing and messaging changes
- targeted `prism-mcp` tests for startup and restart messaging changes where affected
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- the MCP daemon is consistently presented as the worktree-local runtime
- bridge-managed launch and restart are framed as runtime lifecycle only
- no CLI or bridge messaging implies implicit service boot

## 9. Implementation checklist

- [x] Align MCP lifecycle language to runtime ownership
- [x] Align bridge startup and restart messaging
- [x] Add optional `prism runtime` alias
- [x] Update roadmap and spec status after landing
