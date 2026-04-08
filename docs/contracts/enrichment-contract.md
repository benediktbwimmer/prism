# PRISM Enrichment Contract

Status: normative contract  
Audience: cognition, knowledge, coordination, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: optional enrichment layered over authoritative or published base answers

---

## 1. Goal

PRISM must define one explicit contract for enrichment.

This contract exists so that:

- base answers remain distinguishable from optional enrichment
- cognition and other derived layers can add value without pretending to be authority
- enrichment failure degrades cleanly

## 2. Core invariants

The enrichment layer must preserve these rules:

1. Enrichment is optional by default.
2. Enrichment is not authority.
3. Enrichment must be labeled as enrichment in the response shape.
4. Failure to provide enrichment must not corrupt the base answer.

## 3. Base answer versus enrichment

The shared pattern is:

- base answer from authority and deterministic query layers
- optional enrichment layered on top

The response must make that distinction inspectable.

## 4. Required labels

At minimum, enriched responses should surface:

- whether enrichment was requested
- whether enrichment was applied
- whether enrichment was unavailable
- enrichment freshness or availability state when relevant

## 5. Relationship to cognition

Cognition is the primary expected source of future enrichment, but the contract is intentionally
broader than cognition alone.

## 6. Minimum implementation bar

This contract is considered implemented only when:

- enriched answers remain distinguishable from base answers
- missing enrichment degrades honestly
