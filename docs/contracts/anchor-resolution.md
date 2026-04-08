# PRISM Anchor Resolution

Status: normative contract  
Audience: cognition, knowledge, coordination, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: durable anchor identity, live resolution state, and rebinding semantics

---

## 1. Goal

PRISM must define one explicit contract for anchor resolution.

This contract exists so that:

- durable anchor identity is separated from current live binding state
- knowledge, cognition, and coordination can reference the same anchor semantics
- degraded or ambiguous bindings are surfaced explicitly

Canonical ownership:

- this document defines live anchor-resolution states and semantics
- [reference-and-binding.md](./reference-and-binding.md) defines the broader durable reference and
  rebinding model across layers

## 2. Core invariants

Anchor resolution must preserve these rules:

1. Durable anchor identity is not the same thing as current live resolution.
2. Current live resolution may change without rewriting durable identity.
3. Missing, stale, remapped, and ambiguous states must be explicit.
4. Cognition may help resolution, but cognition does not redefine durable anchor identity.

## 3. Resolution states

The shared minimum resolution states should include:

- `exact`
- `remapped`
- `stale`
- `missing`
- `ambiguous`

Implementations may have richer internal states, but they must map back to these shared states for
product consumers.

## 4. Cognition-required versus cognition-free

The contract must allow some anchors to resolve without cognition and others to require cognition.

That distinction must be explicit in results rather than inferred from failure.

## 5. Relationship to reference and binding

Durable reference identity and broader binding semantics are defined in
[reference-and-binding.md](./reference-and-binding.md).

Anchor resolution is the live-resolution specialization of that broader contract.

## 6. Minimum implementation bar

This contract is considered implemented only when:

- product surfaces can distinguish durable anchor identity from current resolution state
- degraded resolution states are explicit
- cognition-dependent resolution is labeled
