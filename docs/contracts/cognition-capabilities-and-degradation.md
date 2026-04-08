# PRISM Cognition Capabilities And Degradation

Status: normative boundary contract  
Audience: cognition, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: what cognition may provide, what it may never become, and how callers degrade when cognition is absent

---

## 1. Goal

PRISM must define one explicit outer contract for cognition before fully specifying internal
cognition architecture.

This contract exists so that:

- cognition does not quietly become authority
- product surfaces know what cognition can and cannot provide
- absence of cognition degrades predictably

## 2. Core invariants

The cognition layer must preserve these rules:

1. Cognition is never an authority plane.
2. Cognition cannot affect coordination correctness.
3. Cognition may enrich answers, but enrichment must be labeled.
4. Cognition absence must degrade to published knowledge or coordination-only answers where
   possible.

## 3. Capability taxonomy

Cognition may provide capabilities such as:

- anchor resolution
- symbol, file, or module lookup
- semantic neighborhood expansion
- impact estimation
- contract target resolution
- discovery and recommendation flows
- rebinding candidate generation

These capabilities are optional computational enrichments, not durable truth by default.

## 4. Degradation rules

When cognition is absent, stale, or unavailable:

- coordination answers must remain correct
- published knowledge answers must remain available when they do not require cognition
- cognition-dependent enrichment must be surfaced as unavailable or omitted explicitly

## 5. Relationship to enrichment

The layering of base answers plus optional cognition-derived enrichment is governed by
[enrichment-contract.md](./enrichment-contract.md).

## 6. Relationship to anchor resolution

Anchor resolution semantics are governed by
[anchor-resolution.md](./anchor-resolution.md).

## 7. Minimum implementation bar

This contract is considered implemented only when:

- cognition capability categories are explicit
- cognition absence degrades honestly
- cognition is prevented from becoming hidden authority
