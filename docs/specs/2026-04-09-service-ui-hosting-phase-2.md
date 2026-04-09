# Service UI Hosting Phase 2

Status: completed
Audience: service, UI, MCP, CLI, runtime, and deployment maintainers
Scope: move browser UI serving under the first-class `prism service` surface and stop treating the MCP daemon as the UI host

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal is to make the service, not the MCP daemon, the place that owns browser UI serving.

This slice should:

- make `prism service up` the only CLI path for starting the browser UI host
- remove `prism mcp ... --ui` as a supported product surface
- move UI-serving lifecycle framing under the service surface
- keep the current host-process reuse internal-only while the public ownership hard-cuts to the
  service surface

That cutover has landed:

- `prism service up` and `prism service restart` now always host the UI
- public `prism mcp bridge|start|restart` no longer accept `--ui`
- service-owned UI hosting is now the only public CLI story

This slice should not:

- finish the full runtime-only daemon cutover
- introduce the auth or session model yet
- complete hosted endpoint selection or browser auth

## 2. Related roadmap

This spec implements:

- [../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md)

Specifically:

- Phase 2: move UI serving fully under the service

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md](../adrs/2026-04-09-first-class-prism-service-and-db-read-path.md)

This spec follows:

- [2026-04-09-prism-service-lifecycle-phase-1.md](./2026-04-09-prism-service-lifecycle-phase-1.md)

## 4. Scope

This slice includes:

- explicit service-lifecycle handling for UI hosting
- CLI behavior that makes service-hosted UI the only public surface
- removal of public MCP UI-hosting flags and parser paths

This slice does not include:

- browser auth or session work
- runtime-session or repo-enrollment work

## 5. Design constraints

- The browser UI should be framed as a service capability, not an MCP daemon feature.
- This is a hard cutover, not a deprecation period. Public MCP CLI paths must stop accepting
  UI-hosting flags in this slice.
- This slice may still reuse the current host process internally, but that reuse must stay an
  implementation detail rather than a user-facing compatibility surface.

## 6. Implementation slices

### Slice 1: Make service lifecycle own UI hosting

- make `prism service up` and `prism service restart` always host the UI
- keep that behavior out of ad hoc CLI dispatch branches

Exit criteria:

- the service path, not the MCP path, owns the main UI-serving intent

### Slice 2: Remove public MCP UI-hosting flags

- remove `--ui` parsing from public MCP bridge/start/restart commands
- update tests so MCP CLI treats UI-hosting as unsupported

Exit criteria:

- the MCP daemon is no longer a public UI host at the CLI surface

### Slice 3: Validate service-hosted UI startup behavior

- ensure service lifecycle commands still report healthy startup clearly
- validate that UI-serving behavior still comes up through the current host path while the public
  ownership is now service-only

Exit criteria:

- the service-hosted UI path is usable and test-covered

## 7. Validation

Minimum validation for this slice:

- targeted `prism-cli` tests where service CLI parsing or UI-hosting dispatch changes
- targeted `prism-mcp` tests where startup or status messaging changes
- direct compile coverage for changed service lifecycle wiring
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- `prism service` is the sole UI-hosting lifecycle surface
- public MCP CLI commands no longer accept UI-hosting flags
- the CLI no longer frames the MCP daemon as a UI host

## 9. Implementation checklist

- [x] Make service lifecycle own UI hosting
- [x] Remove public MCP UI-hosting flags
- [x] Validate service-hosted UI startup behavior
- [x] Update roadmap and spec status after landing
