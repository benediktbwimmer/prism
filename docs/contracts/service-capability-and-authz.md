# PRISM Service Capability And Authorization

Status: normative contract  
Audience: coordination, runtime, auth, MCP, CLI, UI, and future service maintainers  
Scope: capability and authorization rules for service-level coordination operations

---

## 1. Goal

PRISM must define one explicit **service capability and authorization** contract before the PRISM
Service is specified concretely.

This contract exists so that:

- service roles are policy-first instead of transport-first
- strong reads, mutations, runtime discovery, and peer-facing operations have explicit capability
  rules
- authorization does not get invented ad hoc inside handlers

This contract specializes the broader capability model in
[authorization-and-capabilities.md](./authorization-and-capabilities.md).
It also relies on:

- [identity-model.md](./identity-model.md)
- [provenance.md](./provenance.md)
- [service-auth-and-session-model.md](./service-auth-and-session-model.md)

Canonical ownership:

- this document specializes the shared capability model for service-level coordination operations
- [authorization-and-capabilities.md](./authorization-and-capabilities.md) remains the shared
  capability taxonomy and policy baseline

## 2. Core invariants

The capability and authorization layer must preserve these rules:

1. Authorization is evaluated against the requested operation, not only the transport.
2. Strong reads, mutations, runtime descriptor publication, and runtime-local inspection are
   distinct capability categories.
3. The service may broker operations; it must not silently widen the caller's authority.
4. Capability checks must occur before authoritative state changes are committed.
5. Required human or service authority must be expressed through attestation class, not reusable
   bearer elevation.

## 3. Minimum capability families

The contract must distinguish at least:

- eventual coordination read
- strong coordination read
- authoritative coordination mutation
- runtime descriptor publication and clearing
- runtime descriptor discovery
- runtime-local diagnostics or observability inspection
- future event execution control
- future runtime-targeted read or peer enrichment
- browser-session operator access
- principal and repo access administration

The contract must also distinguish authority classes for actions:

- `delegated_machine`
- `service_mediated_human`
- `human_attested`
- `service_attested`

## 4. Caller context

Every service-level operation must be evaluated with an authorization context that includes, when
relevant:

- principal identity
- service session identity when relevant
- runtime identity
- coordination root identity
- target repo or project scope
- requested operation family
- browser session identity when the caller is a human operator using the service UI

## 4.1 Browser session and login rules

The service UI must not invent a separate anonymous browser authority model.

Browser access must resolve to an effective `principal_id`.

The service may support at least two login families:

- bring-your-own PRISM principal login, for example via a signed challenge or device-style flow
- optional service-managed identities that are mapped to PRISM principals by the service

Regardless of login family:

- the authorization layer evaluates the resulting effective principal and capabilities
- UI-triggered reads and mutations use the same capability model as MCP- or CLI-triggered
  operations
- the browser session is transport state, not a second source of identity truth

## 5. Strong read rules

Strong reads may be more sensitive than eventual reads because they force a refresh against the
authority backend.

The contract should allow deployers to authorize:

- eventual reads broadly
- strong reads more narrowly

without changing the meaning of the read result itself.

## 6. Mutation rules

Authoritative mutations require explicit mutation capability.

The service must not infer mutation authority merely because:

- the caller has a live runtime
- the caller may perform eventual reads
- the caller is local to the machine

When policy requires stronger authority than delegated machine execution, the mutation must also
require the appropriate attestation class:

- `service_mediated_human`
- `human_attested`
- `service_attested`

Ordinary browser-session human approval should usually map to `service_mediated_human`, not to a
raw `human_attested` signature.

Higher-risk operations may require `human_attested` explicitly.

## 7. Runtime-local inspection rules

Runtime-local diagnostics, observability packets, and hot local detail are not the same as
authoritative coordination state.

The capability to inspect them must therefore be explicitly distinct from:

- authoritative coordination reads
- authoritative coordination writes

## 8. Relationship to the authority store

Capability checks occur before calls are allowed to hit the authority store or runtime-local
inspection paths.

The authority store defines semantics.
The authorization contract defines who may invoke which semantic surface.

## 8.1 Admin surface rules

Service-hosted admin surfaces for principal and repo access management are service-level
operations.

They should support, at minimum:

- viewing allowed principals
- viewing repo or project membership
- granting or revoking capability bundles or roles
- optional request-access or approval flows later

Those operations remain capability-gated administrative actions rather than ad hoc UI-only
behavior.

## 9. Repo enrollment rule

Runtime connection may propose repo registration automatically.

The service may only accept repo enrollment when the authenticated principal or session has the
required registration capability.

Discovery does not imply authorization.

## 10. Minimum implementation bar

This contract is considered implemented only when:

- strong reads and mutations are authorized distinctly
- runtime-local inspection is authorized distinctly
- service-mediated-human, human-attested, and service-attested requirements are modeled explicitly
  when policy demands them
- service handlers no longer invent one-off policy checks for core coordination operations
