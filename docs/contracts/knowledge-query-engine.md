# PRISM Knowledge Query Engine

Status: normative contract  
Audience: knowledge, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: deterministic bounded queries over published knowledge and related local materialization

---

## 1. Goal

PRISM must define one backend-neutral **Knowledge Query Engine** that answers bounded questions
about published knowledge.

This contract exists so that:

- concept, contract, and memory lookups are implemented once
- MCP, CLI, UI, and future service surfaces do not reimplement knowledge reasoning ad hoc
- published knowledge answers stay deterministic even when cognition is absent

## 2. Core invariants

The knowledge query engine must preserve these rules:

1. Published knowledge queries are deterministic for equal authoritative input and parameters.
2. Published knowledge queries must not depend on live cognition unless the caller explicitly asks
   for enrichment.
3. Results must distinguish authoritative published knowledge from optional enriched suggestions.
4. Local materialization may change speed, not meaning.

## 3. Required query families

The engine should support bounded queries such as:

- concepts relevant to target X
- contracts affecting target X
- memories about target X
- published knowledge linked to task, plan, or artifact Y
- published knowledge packets in scope
- provenance and promotion source for published knowledge object Z

## 4. Relationship to cognition

Cognition may enrich published knowledge answers later, but the base answer must remain available
without cognition.

That boundary is governed by:

- [cognition-capabilities-and-degradation.md](./cognition-capabilities-and-degradation.md)
- [enrichment-contract.md](./enrichment-contract.md)

## 5. Relationship to authority and materialization

The engine is a client of:

- [knowledge-authority-store.md](./knowledge-authority-store.md)
- [knowledge-materialized-store.md](./knowledge-materialized-store.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)

## 6. Minimum implementation bar

This contract is considered implemented only when:

- published knowledge queries no longer reimplement family-specific logic across surfaces
- published answers remain available without cognition
- enrichment is optional and labeled rather than silently blended into authoritative answers
