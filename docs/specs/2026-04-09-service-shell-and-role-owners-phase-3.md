# Service Shell And Role Owners Phase 3

Status: completed
Audience: service, coordination, runtime, MCP, CLI, and host-process maintainers
Scope: make the current PRISM host process explicitly reflect the service-shell, authority-sync, read-broker, and mutation-broker role boundaries already defined in the contracts

---

## 1. Summary

This spec is the concrete implementation target for Phase 3 of:

- [../roadmaps/2026-04-09-abstraction-adoption-and-service-state-cleanup.md](../roadmaps/2026-04-09-abstraction-adoption-and-service-state-cleanup.md)

The contracts already say the PRISM Service should be:

- one host process
- not the authority plane
- a composition of narrow internal roles

The current codebase has the right seams in many places, but the host shape is still too implicit:

- service-owned coordination state exists without one explicit service-shell owner
- authority-sync orchestration exists but is still scattered through workspace-session lifecycle
- read and mutation brokering semantics exist, but the role boundaries are not named clearly enough

This phase makes those owners explicit in code.

## 2. Status

Current state:

- [x] authority-store, materialized-store, query-engine, and mutation-protocol seams exist
- [x] coordination materialization is now repo-shared and service-owned in implementation
- [x] the current host process no longer relies on broad `WorkspaceSession` ownership where
  service-shell role owners now exist
- [x] authority-sync, read-broker, and mutation-broker owners are now explicit first-class modules

Current phase notes:

- this is an ownership and composition cleanup, not a semantic redesign
- do not reintroduce backend leakage while extracting role owners
- do not split into multiple processes or microservices
- Slice 1 is now landed:
  - `WorkspaceServiceShell` is the named workspace-backed service-shell owner for current host
    composition
  - `QueryHost` no longer owns runtime binding and restored session-seed lifecycle directly
- Slice 2 is now landed:
  - `WorkspaceReadBroker` is the named workspace-backed read-broker owner
  - `QueryHost` coordination read helpers now delegate to that broker for workspace-backed hosts
  - `WorkspaceMutationBroker` is the named workspace-backed mutation-broker owner
  - `QueryHost` coordination mutation helpers now delegate to that broker for workspace-backed
    hosts
- Slice 3 is now landed:
  - `WorkspaceAuthoritySyncOwner` is the named workspace-backed authority-sync owner
  - `QueryHost` refresh and read/mutation refresh orchestration now delegate to that owner for
    workspace-backed hosts
  - startup descriptor publication no longer bypasses the service shell from `lib.rs`

## 3. Related contracts and prior specs

This spec depends on:

- [../contracts/service-architecture.md](../contracts/service-architecture.md)
- [../contracts/service-authority-sync-role.md](../contracts/service-authority-sync-role.md)
- [../contracts/service-read-broker.md](../contracts/service-read-broker.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)

This spec follows:

- [2026-04-09-service-owned-coordination-materialization-slice-1.md](2026-04-09-service-owned-coordination-materialization-slice-1.md)
- [2026-04-09-db-backed-service-foundation-phase-15.md](2026-04-09-db-backed-service-foundation-phase-15.md)

## 4. Scope

This phase includes:

- an explicit service-shell owner module for the current host-process coordination stack
- explicit owner modules for:
  - authority-sync orchestration
  - read brokering
  - mutation brokering
- clearer wiring boundaries so product-facing code stops treating `WorkspaceSession` as the service itself
- service-role composition using the existing authority/materialization/query/mutation seams

This phase does not include:

- introducing a new network protocol
- splitting the service into multiple binaries
- implementing the full runtime-gateway role
- implementing the full event-engine role
- finishing Postgres-backed service operation

## 5. Non-goals

This phase should not:

- move authority semantics back into the service shell
- make the service shell a god object
- duplicate read or mutation logic that already belongs in the query or protocol layers
- delay on speculative hosted-topology work

## 6. Design

### 6.1 Service-shell owner rule

The current host process should gain one explicit owner that:

- wires the role modules together
- owns lifecycle/config/resource composition
- does not own coordination semantics directly

### 6.2 Role-owner rule

The following owners should become explicit modules or types:

- authority-sync owner
- read-broker owner
- mutation-broker owner

They may still delegate to existing lower-level helpers, but callers should be able to depend on
the named role owner instead of broad workspace-session internals.

### 6.3 Anti-bypass rule

New product-facing code in this phase must not:

- read coordination authority directly when a broker or role owner exists
- materialize coordination state directly from UI/MCP/CLI surfaces
- treat `WorkspaceSession` as the primary semantic owner once a narrower role owner exists

Compatibility wrappers may remain temporarily, but they must sit below the named role boundary and
be marked for follow-up deletion where appropriate.

## 7. Implementation slices

### Slice 1: Define the service-shell owner boundary

- add a concrete host-process service-shell module or type
- make role wiring explicit there rather than diffused across call sites

Exit criteria:

- there is one obvious owner for service composition in the current process

### Slice 2: Extract explicit read and mutation broker owners

- wrap the existing query/protocol access through named role owners
- move product-facing callers onto those owners where practical

Exit criteria:

- read and mutation ownership is explicit in code, not only in contracts

### Slice 3: Finish authority-sync role ownership cleanup

- move remaining authority-refresh lifecycle orchestration behind the named role owner
- keep session/runtime code as a client of that role rather than its duplicate owner

Exit criteria:

- authority-sync ownership is explicit and no longer spread across session/watch/bootstrap paths

## 8. Validation

Minimum validation for this phase:

- targeted `prism-core` tests for the affected role-owner modules
- targeted `prism-mcp` tests for service-facing read and mutation surfaces
- targeted `prism-cli` tests only where service status or lifecycle wiring changes
- `git diff --check`

Important regression checks:

- authority reads still route through the authority-store seam
- coordination materialization remains repo-shared and service-owned
- read semantics remain consistent across strong/eventual surfaces
- mutation responses still surface the canonical protocol metadata

## 9. Completion criteria

This phase is complete only when:

- the current host process has one explicit service-shell owner
- read, mutation, and authority-sync owners are explicit in code
- product surfaces depend on those owners instead of broad session internals where the role seam now exists

## 10. Implementation checklist

- [x] Add an explicit service-shell owner boundary
- [x] Extract explicit read-broker ownership
- [x] Extract explicit mutation-broker ownership
- [x] Extract explicit authority-sync ownership cleanup
- [x] Validate affected crates and direct downstream dependents
- [x] Update roadmap/spec status as slices land
