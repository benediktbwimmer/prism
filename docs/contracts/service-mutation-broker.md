# PRISM Service Mutation Broker

Status: normative contract  
Audience: coordination, runtime, MCP, CLI, UI, and service maintainers  
Scope: service-side authoritative mutation intake, batching, replay, acknowledgment, and commit-result fanout

---

## 1. Goal

The PRISM Service must define one explicit **Mutation Broker** role.

This role exists so that:

- authoritative writes have one orchestration home inside the service
- write coalescing and retry behavior are explicit
- the service does not become a hidden write authority

## 2. Responsibilities

The mutation broker owns:

- accepting mutation intents
- validating request shape and capability before submission
- bounded batching or coalescing of mutation intents where allowed
- routing commits through the coordination mutation protocol and authority store
- conflict replay or retry orchestration
- acknowledgment and committed-result fanout

## 3. Non-goals

The mutation broker does not own:

- low-level backend commit mechanics
- authority polling logic
- deterministic coordination evaluation
- runtime transport policy

## 4. Dependencies

This role is a client of:

- [coordination-mutation-protocol.md](./coordination-mutation-protocol.md)
- [coordination-authority-store.md](./coordination-authority-store.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [provenance.md](./provenance.md)

## 5. Accepted versus committed

The mutation broker must distinguish:

- accepted for broker processing
- committed authoritatively
- rejected

The service must not blur broker acceptance into authoritative commit.

## 6. Coalescing rule

The mutation broker may batch or coalesce mutation intents within a bounded configurable window.

It must batch intents, not blind state overwrites.
It must preserve mutation-protocol semantics.

## 7. Minimum implementation bar

This contract is considered implemented only when:

- service-side write orchestration lives in one role
- accepted versus committed is explicit
- replay and retry behavior is expressed in terms of the mutation protocol rather than ad hoc
  service logic
