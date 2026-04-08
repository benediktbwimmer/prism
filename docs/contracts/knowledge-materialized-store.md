# PRISM Knowledge Materialized Store

Status: normative contract  
Audience: knowledge, runtime, query, MCP, CLI, UI, storage, and future service maintainers  
Scope: non-authoritative persistent local read models, indexes, packets, and checkpoints for published knowledge

---

## 1. Goal

PRISM must define one explicit **KnowledgeMaterializedStore** abstraction for local persisted
projections of published knowledge.

This contract exists so that:

- published knowledge lookup and indexing do not leak raw local SQLite access everywhere
- local search, packet hydration, and bounded denormalized views have one storage seam
- knowledge authority and knowledge materialization remain distinct

## 2. Core invariants

The knowledge materialized store must preserve these rules:

1. It is never a knowledge authority backend.
2. It advances only from published knowledge authority or explicitly allowed local indexing inputs.
3. It must surface the published authority version or stamp it corresponds to.
4. It must remain disposable and rebuildable.

## 3. Responsibilities

The KnowledgeMaterializedStore owns:

- local persisted lookup indexes over published knowledge
- packet hydration or denormalized local views
- local search-oriented indexes over published knowledge
- startup caches or checkpoints for knowledge serving
- materialization metadata such as authority stamp and schema version

## 4. Non-goals

It does not own:

- authoritative publication
- promotion acceptance or rejection
- knowledge curation workflow
- live cognition inference

## 5. Relationship to authority and query

The intended flow is:

- knowledge authority store yields published knowledge
- knowledge query engine evaluates bounded answers
- knowledge materialized store persists derived local views for fast serving

If the materialized store contains denormalized packets, they remain derived outputs rather than
authority.

## 6. Minimum implementation bar

This contract is considered implemented only when:

- local persisted knowledge reads no longer rely on ad hoc SQLite access
- materialized knowledge packets or indexes are explicitly downstream of published authority
- materialization metadata is queryable
