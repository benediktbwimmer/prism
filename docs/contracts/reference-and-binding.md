# PRISM Reference And Binding

Status: normative cross-layer contract  
Audience: coordination, knowledge, cognition, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: durable reference identity, live binding state, rebinding, and degraded reference semantics

---

## 1. Goal

PRISM must define one explicit contract for reference identity and binding.

This contract exists so that:

- durable references survive code and workspace change
- live bindings can degrade honestly
- coordination, knowledge, and cognition use compatible reference semantics

Canonical ownership:

- this document defines the broader cross-layer model for durable reference identity, live binding,
  and rebinding semantics
- [anchor-resolution.md](./anchor-resolution.md) specializes the live resolution behavior for anchors

## 2. Core invariants

The reference and binding layer must preserve these rules:

1. Durable reference identity is distinct from current live binding.
2. Current live binding may change over time without invalidating durable reference identity.
3. Missing, stale, remapped, and ambiguous bindings must be explicit.
4. Rebinding may be suggested, but it must not silently rewrite durable identity without an
   explicit contract that allows it.

## 3. Relationship to anchor resolution

Anchor resolution is the live-resolution specialization of this broader contract and is defined in
[anchor-resolution.md](./anchor-resolution.md).

## 4. Relationship to layers

Coordination may reference:

- tasks
- artifacts
- evidence anchors

Knowledge may reference:

- concepts
- contracts
- memories
- promotion source anchors

Cognition may help compute or repair live bindings, but does not become the durable reference
identity authority by default.

## 5. Minimum implementation bar

This contract is considered implemented only when:

- durable reference identity and current live binding are no longer conflated
- degraded binding states are explicit across layers
