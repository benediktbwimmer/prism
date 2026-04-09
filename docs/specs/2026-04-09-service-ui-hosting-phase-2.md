# Service UI Hosting Phase 2

Status: approved
Audience: service, UI, MCP, CLI, runtime, and deployment maintainers
Scope: move browser UI serving under the first-class `prism service` surface and stop treating the MCP daemon as the UI host

---

## 1. Summary

This spec is the next concrete implementation slice under the
[first-class service and auth rollout roadmap](../roadmaps/2026-04-09-first-class-prism-service-and-auth-rollout.md).

The goal is to make the service, not the MCP daemon, the place that owns browser UI serving.

This slice should:

- make `prism service up` the primary path for starting the browser UI host
- stop requiring `prism mcp ... --ui` as the way to get the UI
- move UI-serving lifecycle framing under the service surface
- preserve current runtime and MCP behavior while the deeper runtime-only cutover is still pending

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
- CLI behavior that makes service-hosted UI the primary surface
- compatibility handling for existing `mcp --ui` workflows while the cutover is incomplete

This slice does not include:

- final removal of all MCP UI flags
- browser auth or session work
- runtime-session or repo-enrollment work

## 5. Design constraints

- The browser UI should be framed as a service capability, not an MCP daemon feature.
- If compatibility shims remain, they should point users toward `prism service` rather than keep
  reinforcing `prism mcp --ui` as the primary model.
- This slice may still reuse the current host process internally, but user-facing ownership must
  shift to the service surface.

## 6. Implementation slices

### Slice 1: Make service lifecycle own UI flags

- add explicit UI-hosting behavior to the service lifecycle path
- keep that behavior out of ad hoc CLI dispatch branches

Exit criteria:

- the service path, not the MCP path, owns the main UI-serving intent

### Slice 2: Add compatibility guidance for MCP UI usage

- preserve current functionality where necessary
- route help text and status text toward `prism service`

Exit criteria:

- compatibility remains, but the product story points to the service

### Slice 3: Validate service-hosted UI startup behavior

- ensure service lifecycle commands still report healthy startup clearly
- validate that UI-serving behavior still comes up through the current host path during this
  transitional slice

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

- `prism service` is the primary UI-hosting lifecycle surface
- the CLI no longer frames the MCP daemon as the canonical UI host
- compatibility with existing behavior is preserved where still required

## 9. Implementation checklist

- [ ] Make service lifecycle own the UI-hosting intent
- [ ] Add compatibility guidance for legacy `mcp --ui` usage
- [ ] Validate service-hosted UI startup behavior
- [ ] Update roadmap and spec status after landing
