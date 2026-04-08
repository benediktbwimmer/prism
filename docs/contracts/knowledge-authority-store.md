# PRISM Knowledge Authority Store

Status: normative contract  
Audience: knowledge, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: authoritative published knowledge state, retained knowledge history, publication, and provenance

---

## 1. Goal

PRISM must define one explicit **KnowledgeAuthorityStore** abstraction for durable published
knowledge.

This contract exists so that:

- published concepts, contracts, memories, and other promoted knowledge have one authority seam
- knowledge publication does not get mixed with local runtime discovery or cognition
- future repo-scoped and project-scoped published knowledge share one semantic contract

The knowledge authority store is the seam for durable published knowledge, not for live cognition
or local runtime drafts.

This contract relies on:

- [identity-model.md](./identity-model.md)
- [authorization-and-capabilities.md](./authorization-and-capabilities.md)
- [provenance.md](./provenance.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 2. Core invariants

The knowledge authority store must preserve these rules:

1. Published knowledge is authoritative only after explicit promotion or publication.
2. Draft, discovered, or runtime-only knowledge is not authoritative published knowledge.
3. Current published state and retained published history belong to the same authority seam.
4. Every published knowledge object must carry provenance to its promotion source.
5. Backend choice must not change knowledge publication semantics.

## 3. Published knowledge families

The contract must support at least these published families:

- concepts
- contracts
- memories
- outcomes or curated learnings when promoted into durable knowledge
- future published knowledge packets built from those durable families

The contract does not require every knowledge family to share one storage shape.
It does require one authority model.

## 4. Responsibilities

The KnowledgeAuthorityStore owns:

- reading current published knowledge state
- reading retained published knowledge history
- publishing promoted knowledge mutations
- surfacing promotion provenance and publication metadata
- surfacing knowledge scope such as repo or project

It does not own:

- runtime discovery candidates
- cognition-only enrichment results
- local caches or indexes

## 5. Published versus draft distinction

The store must preserve an explicit distinction between:

- published knowledge
- draft or discovered knowledge candidates
- local runtime observations that have not been promoted

The authority store is only for the published side of that boundary.

## 6. Scope

Published knowledge must be explicitly scoped.

At minimum, the contract must allow:

- `repo_id`
- future `project_id`

Scope semantics are defined more broadly in
[shared-scope-and-identity.md](./shared-scope-and-identity.md) and
[knowledge-scope.md](./knowledge-scope.md).

## 7. Required read families

The contract should support:

- current published knowledge reads
- strong or current reads against published knowledge authority
- retained history reads
- provenance reads
- publication metadata reads

## 8. Required mutation family

The contract should support one publication or promotion-shaped mutation family that:

- accepts curated knowledge publication intent
- validates publication scope and provenance
- commits published knowledge atomically
- returns authoritative publication metadata

The exact transport or command surface may vary.
The underlying publication semantics should not.

## 9. Relationship to promotion

Promotion is the workflow by which draft or discovered knowledge becomes published knowledge.

The publication boundary for that workflow is governed by
[knowledge-promotion-and-publication.md](./knowledge-promotion-and-publication.md).

The authority store owns only the durable published side of that boundary.

## 10. Relationship to materialization and query

Knowledge query and local indexing should not bypass this seam for authoritative published state.

Those layers belong to:

- [knowledge-materialized-store.md](./knowledge-materialized-store.md)
- [knowledge-query-engine.md](./knowledge-query-engine.md)

## 11. Minimum implementation bar

This contract is considered implemented only when:

- published knowledge reads no longer bypass one authority seam
- publication or promotion commit semantics are explicit
- current published state and retained published history are accessed consistently
- provenance for promoted knowledge is queryable
