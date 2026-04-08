# PRISM Provenance

Status: normative cross-layer contract  
Audience: coordination, knowledge, runtime, query, MCP, CLI, UI, auth, and future service maintainers  
Scope: authorship, execution context, publication source, authority base, and trust posture for durable state changes

---

## 1. Goal

PRISM must define one shared provenance contract across coordination and knowledge.

This contract exists so that:

- durable objects can explain where they came from
- authority and publication actions remain auditable
- promotion and publication do not lose their source evidence
- authorship, execution, and transport are not collapsed into one vague actor field

Canonical ownership:

- this document defines the shared provenance envelope and questions every layer should be able to
  answer
- [coordination-history-and-provenance.md](./coordination-history-and-provenance.md) specializes
  retained coordination history and provenance query behavior

## 2. Minimum provenance questions

The contract must support answering:

- which principal authorized this
- which credential or authenticator was used when relevant
- which runtime or worktree executed it when relevant
- when
- in which scope
- from what source object, evidence, or authority base
- through which publication or mutation path
- with what trust or publication posture when relevant

## 3. Core invariants

The provenance model must preserve these rules:

1. Provenance is metadata about durable objects or commits, not hidden implementation detail.
2. Provenance must not be inferred only from storage substrate conventions when explicit metadata is
   required.
3. Promotion and publication must retain provenance to their source evidence.
4. Authorship, execution lane, and transport or intermediary context must remain distinguishable.

## 4. Minimum provenance envelope

The contract should be able to represent, when relevant:

- principal identity
- credential or authenticator identity
- runtime identity
- worktree identity
- repo or project scope
- timestamp
- source object or evidence references
- authority base or committed version
- publication or trust posture

## 5. Relationship to specific layers

Coordination uses provenance for:

- authoritative commits
- history and object timelines
- review and artifact lineage

Knowledge uses provenance for:

- promotion source
- publication metadata
- curator and promoter identity

This contract relies on:

- [identity-model.md](./identity-model.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 6. Minimum implementation bar

This contract is considered implemented only when:

- durable coordination and knowledge objects can surface creator or publisher provenance
- publication and promotion provenance is queryable
- authorship and execution context remain distinguishable where relevant
