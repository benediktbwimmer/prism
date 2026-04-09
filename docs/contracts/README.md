# PRISM Contracts

Status: normative contract index  
Audience: PRISM core, coordination, runtime, MCP, CLI, UI, and future service maintainers  
Scope: stable implementation contracts that define the seams PRISM components must implement

---

## Purpose

`docs/contracts/` is the home for PRISM's stable implementation contracts.

These documents are different from the broader design notes and roadmap docs elsewhere under `docs/`:

- contract docs define the normative seams other components are expected to implement against
- design docs in `docs/designs/` explain motivation, rollout strategy, historical context, or future direction

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
- target architecture and design docs describe intended shape and rollout direction
- thesis, roadmap, and future-direction docs provide framing rather than exact semantics

Foundational ADRs that active contracts currently depend on:

- [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)

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

<!-- BEGIN GENERATED INDEX: docs-contracts -->
## Generated Contract Inventory

_This section is generated by `scripts/update_doc_indices.py`. Do not edit it by hand._

- [anchor-resolution.md](./anchor-resolution.md) — PRISM Anchor Resolution — `Status: normative contract`
- [authority-sync.md](./authority-sync.md) — PRISM Authority Sync Contract — `Status: normative contract`
- [authorization-and-capabilities.md](./authorization-and-capabilities.md) — PRISM Authorization And Capabilities — `Status: normative contract`
- [cognition-capabilities-and-degradation.md](./cognition-capabilities-and-degradation.md) — PRISM Cognition Capabilities And Degradation — `Status: normative boundary contract`
- [consistency-and-freshness.md](./consistency-and-freshness.md) — PRISM Consistency And Freshness — `Status: normative contract`
- [coordination-artifact-review-model.md](./coordination-artifact-review-model.md) — PRISM Coordination Artifact And Review Model — `Status: normative task, artifact, and review contract for coordination v2`
- [coordination-authority-store-implementation-spec.md](./coordination-authority-store-implementation-spec.md) — PRISM Coordination Authority Store Migration Spec — `Status: companion implementation spec`
- [coordination-authority-store.md](./coordination-authority-store.md) — PRISM Coordination Authority Store — `Status: normative contract`
- [coordination-history-and-provenance.md](./coordination-history-and-provenance.md) — PRISM Coordination History And Provenance — `Status: normative contract`
- [coordination-materialized-store.md](./coordination-materialized-store.md) — PRISM Coordination Materialized Store — `Status: normative contract`
- [coordination-mutation-protocol.md](./coordination-mutation-protocol.md) — PRISM Coordination Mutation Protocol — `Status: normative contract`
- [coordination-query-engine.md](./coordination-query-engine.md) — PRISM Coordination Query Engine — `Status: normative contract`
- [enrichment-contract.md](./enrichment-contract.md) — PRISM Enrichment Contract — `Status: normative contract`
- [event-engine.md](./event-engine.md) — PRISM Event Engine Contract — `Status: normative contract`
- [identity-model.md](./identity-model.md) — PRISM Identity Model — `Status: normative contract`
- [knowledge-authority-store.md](./knowledge-authority-store.md) — PRISM Knowledge Authority Store — `Status: normative contract`
- [knowledge-materialized-store.md](./knowledge-materialized-store.md) — PRISM Knowledge Materialized Store — `Status: normative contract`
- [knowledge-promotion-and-publication.md](./knowledge-promotion-and-publication.md) — PRISM Knowledge Promotion And Publication — `Status: normative contract`
- [knowledge-query-engine.md](./knowledge-query-engine.md) — PRISM Knowledge Query Engine — `Status: normative contract`
- [knowledge-scope.md](./knowledge-scope.md) — PRISM Knowledge Scope — `Status: normative contract`
- [local-materialization.md](./local-materialization.md) — PRISM Local Materialization Contract — `Status: normative contract`
- [provenance.md](./provenance.md) — PRISM Provenance — `Status: normative cross-layer contract`
- [reference-and-binding.md](./reference-and-binding.md) — PRISM Reference And Binding — `Status: normative cross-layer contract`
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md) — PRISM Runtime Identity And Descriptors — `Status: normative contract`
- [runtime-observability-packets.md](./runtime-observability-packets.md) — PRISM Runtime Observability Packets — `Status: normative contract`
- [service-architecture.md](./service-architecture.md) — PRISM Service Architecture — `Status: normative contract`
- [service-authority-sync-role.md](./service-authority-sync-role.md) — PRISM Service Authority Sync Role — `Status: normative contract`
- [service-capability-and-authz.md](./service-capability-and-authz.md) — PRISM Service Capability And Authorization — `Status: normative contract`
- [service-mutation-broker.md](./service-mutation-broker.md) — PRISM Service Mutation Broker — `Status: normative contract`
- [service-read-broker.md](./service-read-broker.md) — PRISM Service Read Broker — `Status: normative contract`
- [service-runtime-gateway.md](./service-runtime-gateway.md) — PRISM Service Runtime Gateway — `Status: normative contract`
- [shared-scope-and-identity.md](./shared-scope-and-identity.md) — PRISM Shared Scope And Identity — `Status: normative cross-layer contract`
- [signing-and-verification.md](./signing-and-verification.md) — PRISM Signing And Verification — `Status: normative contract`
- [spec-engine.md](./spec-engine.md) — PRISM Spec Engine — `Status: normative contract`
<!-- END GENERATED INDEX: docs-contracts -->
