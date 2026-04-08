# PRISM Knowledge Promotion And Publication

Status: normative contract  
Audience: knowledge, runtime, coordination, MCP, CLI, UI, and future service maintainers  
Scope: draft knowledge candidates, curation, promotion, publication, and provenance

---

## 1. Goal

PRISM must define one explicit contract for how discovered or draft knowledge becomes published
knowledge.

This contract exists so that:

- runtime discovery and cognition do not silently become durable truth
- curation and promotion are explicit workflow steps
- published knowledge always carries provenance to its source evidence

This contract relies on:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 2. Core invariants

The promotion and publication flow must preserve these rules:

1. Discovery is not publication.
2. Curation is not publication.
3. Publication is the explicit step that makes knowledge durable and authoritative.
4. Every published knowledge object must carry provenance to its promotion source.
5. Promotion may be reviewed, but review does not by itself publish knowledge.

## 3. Lifecycle

The minimum knowledge lifecycle is:

- discovered or drafted candidate
- curated candidate
- published knowledge

Future richer states are allowed, but the explicit publication boundary must remain.

## 4. Promotion input

A promotion attempt should identify at least:

- source candidate or draft
- target knowledge family
- target scope
- provenance evidence
- curator or promoter identity
- authorization context sufficient to explain why publication was allowed

## 5. Publication result

A successful publication must produce:

- durable published knowledge object identity
- publication metadata
- provenance metadata
- publication scope

## 6. Relationship to authority

The publication boundary commits through
[knowledge-authority-store.md](./knowledge-authority-store.md).

The promotion contract defines the workflow and required metadata leading up to that commit.

## 7. Relationship to coordination

Knowledge promotion may be linked to coordination objects such as tasks, artifacts, or reviews.

Those links must remain explicit provenance or reference links.
They do not automatically make coordination state part of the knowledge authority plane.

## 8. Minimum implementation bar

This contract is considered implemented only when:

- draft versus published distinction is explicit
- publication requires explicit provenance
- promotion workflow no longer relies on hidden implicit persistence paths
