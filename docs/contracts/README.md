# PRISM Contracts

Status: normative contract index  
Audience: PRISM core, coordination, runtime, MCP, CLI, UI, and future service maintainers  
Scope: stable implementation contracts that define the seams PRISM components must implement

---

## Purpose

`docs/contracts/` is the home for PRISM's stable implementation contracts.

These documents are different from the broader design notes and roadmap docs in `docs/`:

- contract docs define the normative seams other components are expected to implement against
- design docs explain motivation, rollout strategy, historical context, or future direction

When a rule is defined both in a higher-level design doc and in a contract doc, the contract doc is
the normative source.

## Status and precedence

The repo should read document status in roughly this order of precedence:

1. `normative contract`
2. `implemented architecture` or `implemented execution contract`
3. `proposed target architecture` or `proposed target design`
4. `design note`, `future direction`, `execution doc`, or broader product thesis/reference docs

When documents overlap:

- contract docs define the normative seam
- implementation docs describe the current shipped realization of a seam
- target architecture docs describe intended shape and rollout direction
- thesis, roadmap, and future-direction docs provide framing rather than exact semantics

## Current contract set

Core coordination contracts:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-query-engine.md](./coordination-query-engine.md)
- [coordination-mutation-protocol.md](./coordination-mutation-protocol.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)
- [coordination-artifact-review-model.md](./coordination-artifact-review-model.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [local-materialization.md](./local-materialization.md)
- [authority-sync.md](./authority-sync.md)
- [coordination-history-and-provenance.md](./coordination-history-and-provenance.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)
- [runtime-observability-packets.md](./runtime-observability-packets.md)
- [event-engine.md](./event-engine.md)
- [spec-engine.md](./spec-engine.md)

Service contracts:

- [service-architecture.md](./service-architecture.md)
- [service-authority-sync-role.md](./service-authority-sync-role.md)
- [service-read-broker.md](./service-read-broker.md)
- [service-mutation-broker.md](./service-mutation-broker.md)
- [service-runtime-gateway.md](./service-runtime-gateway.md)
- [service-capability-and-authz.md](./service-capability-and-authz.md)

Knowledge contracts:

- [knowledge-authority-store.md](./knowledge-authority-store.md)
- [knowledge-materialized-store.md](./knowledge-materialized-store.md)
- [knowledge-query-engine.md](./knowledge-query-engine.md)
- [knowledge-promotion-and-publication.md](./knowledge-promotion-and-publication.md)
- [knowledge-scope.md](./knowledge-scope.md)

Cognition boundary contracts:

- [cognition-capabilities-and-degradation.md](./cognition-capabilities-and-degradation.md)
- [anchor-resolution.md](./anchor-resolution.md)
- [enrichment-contract.md](./enrichment-contract.md)

Shared cross-layer contracts:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [shared-scope-and-identity.md](./shared-scope-and-identity.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)
- [reference-and-binding.md](./reference-and-binding.md)

Companion implementation spec:

- [coordination-authority-store-implementation-spec.md](./coordination-authority-store-implementation-spec.md)

## Writing rules

Contracts in this directory should:

- define behavior, invariants, and required metadata explicitly
- be backend-neutral unless a backend-specific rule is intentionally called out
- be precise enough for tests and implementations to target directly
- avoid long historical narrative unless it is required to explain a normative rule
- link outward to higher-level design docs rather than restating those docs in full

Contracts in this directory should not:

- become migration logs or implementation diaries
- encode storage-specific mechanics as if they were universal semantics
- duplicate broad roadmap material that belongs in `docs/`

## Follow-on contracts

Likely follow-on contracts after this bundle are:

- peer or federated runtime enrichment contract
- cross-repo coordination root and membership contract
