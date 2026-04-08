# PRISM Service Authority Sync Role

Status: normative contract  
Audience: coordination, runtime, storage, watch, MCP, CLI, UI, and service maintainers  
Scope: service-side authority polling, verification, refresh, checkpoint triggers, and authority-change fanout

---

## 1. Goal

The PRISM Service must define one explicit **Authority Sync Role**.

This role exists so that:

- polling, fetch, verification, and refresh behavior have one clear owner inside the service
- service-side authority freshness does not get smeared across reads, writes, and runtime fanout

## 2. Responsibilities

The authority sync role owns:

- namespace- or root-scoped authority polling
- fetch and verification of authoritative updates
- verified snapshot or current-state refresh
- triggers for local materialization and checkpoint refresh
- self-write suppression
- local authority-change fanout to other roles

## 3. Non-goals

The authority sync role does not own:

- mutation semantics
- user-facing read semantics
- event scheduling policy
- runtime transport policy

## 4. Dependencies

This role is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [authority-sync.md](./authority-sync.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 5. Output contract

The role should produce:

- verified authority snapshots or equivalent refresh results
- explicit freshness and verification metadata
- refresh or invalidation triggers for materialized state
- fanout notifications that authority advanced

## 6. Minimum implementation bar

This contract is considered implemented only when:

- service-side polling and verification live in one role
- strong-read support and mutation follow-through can consume that role without reimplementing sync
- self-write suppression is explicit and testable
