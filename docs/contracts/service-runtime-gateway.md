# PRISM Service Runtime Gateway

Status: normative contract  
Audience: runtime, coordination, MCP, CLI, UI, and service maintainers  
Scope: runtime connections to the service, runtime registration, runtime-targeted reads, diagnostics access, and local fanout

---

## 1. Goal

The PRISM Service must define one explicit **Runtime Gateway** role.

This role exists so that:

- runtime connectivity and runtime-targeted access have one owner inside the service
- runtime-local information does not leak into the authority path accidentally
- local and hosted runtime modeling use one gateway boundary
- runtimes participate in coordination as service clients rather than as mini coordination stores
- runtime registration and delegated runtime session handling have one owner inside the service

## 2. Responsibilities

The runtime gateway owns:

- local runtime connections
- hosted or local runtime registration through descriptors and local connection state
- delegated runtime-session issuance, renewal, or handoff to the narrower auth/session seam
- local fanout notifications to connected runtimes
- runtime-targeted read serving when allowed
- runtime-local diagnostics and packet access when allowed
- coordination-facing runtime registration and reachability checks

## 3. Non-goals

The runtime gateway does not own:

- authoritative mutation correctness
- deterministic coordination evaluation
- authority polling and verification logic
- event execution semantics

## 4. Dependencies

This role is a client of:

- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)
- [runtime-observability-packets.md](./runtime-observability-packets.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)

## 5. Authority boundary

The runtime gateway must preserve the rule that runtime-local data is not authority unless another
contract explicitly promotes a bounded summary into authority.

It must also preserve the rule that:

- interactive coordination participation requires a reachable PRISM Service
- runtimes do not fall back to runtime-owned coordination materialization when the service is
  unavailable
- runtime registration may propose repo presence, but repo enrollment is still capability-gated

## 6. Minimum implementation bar

This contract is considered implemented only when:

- runtime connectivity and runtime-targeted access live in one role
- runtime-local diagnostics are capability-gated distinctly from authoritative reads
- the gateway does not bypass authority or query seams for coordination correctness
