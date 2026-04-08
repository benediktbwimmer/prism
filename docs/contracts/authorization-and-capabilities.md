# PRISM Authorization And Capabilities

Status: normative contract  
Audience: auth, coordination, runtime, query, MCP, CLI, UI, knowledge, and future service maintainers  
Scope: capability families, policy boundaries, authorization context, and operation gating across PRISM

---

## 1. Goal

PRISM must define one explicit **authorization and capabilities** contract because policy is part of
how PRISM decides what truth transitions and reads are allowed.

This contract exists so that:

- strong reads, mutations, publication, discovery, and runtime-local inspection are gated
  consistently
- policy lives above transport details
- the service and runtime layers do not invent one-off capability rules

Canonical ownership:

- this document defines the shared capability taxonomy and policy model across PRISM
- specialized contracts may refine it for a narrower surface, but should not redefine the shared
  families

## 2. Core invariants

The authorization and capability layer must preserve these rules:

1. Authorization is evaluated against requested operation family and scope, not just transport.
2. Capability categories must be explicit and bounded.
3. The caller's authority must not be widened silently by an intermediary service or runtime.
4. Capability checks must happen before authoritative state changes or trust-sensitive reads.

## 3. Minimum capability families

The shared minimum capability families must include:

- eventual authority read
- strong authority read
- authoritative mutation
- runtime descriptor publication and clearing
- runtime descriptor discovery
- runtime-local diagnostics and observability inspection
- knowledge publication or promotion
- trust and verification administration
- future event execution control
- future peer or runtime-targeted enrichment access

## 4. Authorization context

Every operation should be authorized against a context that can include:

- principal identity
- credential or authenticator identity when relevant
- runtime identity
- worktree identity when relevant
- repo or project scope
- requested operation family

## 5. Policy source of truth

The contract must allow policy to be explicit and inspectable.

It should be possible to answer:

- what capability was required
- what scope it was evaluated against
- why it was granted or denied

## 6. Automation and human authority

The contract must distinguish clearly between:

- human-authorized operations
- ordinary local agent execution
- future durable service-principal operations

Operations that alter trust roots, bootstrap identity, or other high-risk trust state should require
stronger human authority than ordinary coordination mutations.

## 7. Relationship to specialized contracts

This contract is the shared capability model beneath specialized contracts such as:

- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)
- [knowledge-promotion-and-publication.md](./knowledge-promotion-and-publication.md)

## 8. Minimum implementation bar

This contract is considered implemented only when:

- capability families are explicit across major surfaces
- high-risk trust and publication operations are distinguished from ordinary reads and writes
- services no longer invent ad hoc policy categories for core operations
