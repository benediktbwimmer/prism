# Runtime Gateway Foundation Phase 15

Status: draft
Audience: service, MCP, runtime, transport, auth, and coordination maintainers
Scope: establish the first explicit runtime-gateway slice inside the service-hosted coordination architecture without collapsing gateway behavior back into the service shell or coordination authority seams

---

## 1. Summary

This spec is the next concrete implementation target after the DB-backed service-foundation slice
of Phase 15.

The goal is to make the runtime gateway an explicit service role in code, not to finish every
future peer, hosted, or browser-facing transport path in one pass.

This slice should:

- give the service one named runtime-gateway owner
- centralize runtime-targeted routing and runtime-descriptor resolution behind that owner
- preserve the rule that coordination authority, read, and mutation semantics stay below the
  gateway rather than moving into transport code

This slice should not:

- implement the later event engine
- redesign auth or identity again
- introduce a second hidden authority plane in transport code

## 2. Related roadmap

This spec continues:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 15: implement the remaining PRISM Service roles and release deployment modes

## 3. Related contracts

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-runtime-gateway.md](../contracts/service-runtime-gateway.md)
- [../contracts/runtime-identity-and-descriptors.md](../contracts/runtime-identity-and-descriptors.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/service-capability-and-authz.md](../contracts/service-capability-and-authz.md)

It follows:

- [2026-04-09-db-backed-service-foundation-phase-15.md](2026-04-09-db-backed-service-foundation-phase-15.md)

## 4. Scope

This slice includes:

- one explicit runtime-gateway owner in the live service host
- centralized runtime-descriptor lookup and peer-routing entry points
- explicit routing of runtime-targeted reads through the configured authority-provider and service
  shell
- centralization of gateway-facing trust and capability checks where they are currently duplicated

This slice does not include:

- browser login flows
- service-managed identities
- full hosted fleet management UX
- hint packet, intervention packet, or archive/export protocol expansion

## 5. Design constraints

- The runtime gateway is a service role, not the service shell itself.
- The runtime gateway may depend on the configured authority provider and read broker, but must not
  bypass the settled authority, materialization, or query seams.
- Runtime-descriptor publication remains an authority family concern; the gateway consumes it but
  does not redefine it.
- Peer/runtime-targeted reads must keep their trust labeling explicit.

## 6. Implementation slices

### Slice 1: Name the runtime-gateway owner

- introduce one explicit runtime-gateway owner bound off the service shell
- move current peer-runtime routing entry points under that owner

Exit criteria:

- runtime-targeted transport code no longer hangs directly off generic host helpers

### Slice 2: Centralize runtime-descriptor resolution

- move shared descriptor lookup and stale/degraded handling under the gateway owner
- ensure configured authority-provider selection is used consistently

Exit criteria:

- gateway descriptor resolution has one owner and one trust/degradation path

### Slice 3: Centralize gateway trust and capability checks

- move gateway-facing auth/capability shaping out of scattered route helpers
- preserve the current stable error payloads and degradation behavior

Exit criteria:

- peer/runtime routing no longer rebuilds its own trust story ad hoc

## 7. Validation

Minimum validation for this slice:

- targeted `prism-mcp` tests for peer-runtime routing, descriptor lookup, and gateway auth errors
- targeted `prism-core` tests only where authority-descriptor plumbing changes
- `git diff --check`

## 8. Completion criteria

This spec is complete when:

- the service has one named runtime-gateway owner in code
- runtime-targeted routing uses that owner instead of broad host helpers
- runtime-descriptor lookup and degraded handling are centralized
- gateway trust and capability behavior are explicit and shared

## 9. Implementation checklist

- [ ] Introduce the runtime-gateway owner
- [ ] Centralize runtime-descriptor resolution
- [ ] Centralize gateway trust and capability checks
- [ ] Validate affected crates and direct downstream dependents
- [ ] Update roadmap/spec status as slices land
