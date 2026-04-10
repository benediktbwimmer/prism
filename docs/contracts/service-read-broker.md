# PRISM Service Read Broker

Status: normative contract  
Audience: coordination, runtime, MCP, CLI, UI, and service maintainers  
Scope: service-side read routing, eventual versus strong semantics, strong-read coalescing, and freshness metadata

---

## 1. Goal

The PRISM Service must define one explicit **Read Broker** role.

This role exists so that:

- service-side reads have one owner for strong versus eventual routing
- strong-read coalescing is explicit
- freshness and verification metadata are attached consistently
- eventual coordination reads are served from an explicit service-owned projection when enabled,
  rather than from runtime-owned SQLite state

## 2. Responsibilities

The read broker owns:

- eventual versus strong routing
- forcing authority refresh when strong semantics require it
- coalescing strong reads targeting the same relevant authority set
- selecting between authority-backed and materialized inputs
- attaching consistency, freshness, and verification metadata to responses
- serving coordination reads to runtimes that do not own coordination materialization themselves

The canonical programmable caller for brokered reads is `prism_code`.

## 3. Non-goals

The read broker does not own:

- authority polling loop mechanics
- mutation replay and commit
- event scheduling
- runtime transport management

## 4. Dependencies

This role is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [coordination-query-engine.md](./coordination-query-engine.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [service-authority-sync-role.md](./service-authority-sync-role.md)

Any materialized input here is service-owned coordination materialization, not per-runtime
coordination storage.

## 5. Strong-read coalescing

The read broker may use a short bounded coalescing window for strong reads that target the same
relevant authority set.

Coalescing is an optimization only.
It must not weaken strong-read semantics.

## 6. Minimum implementation bar

This contract is considered implemented only when:

- strong versus eventual routing is explicit in one role
- strong-read coalescing does not change correctness semantics
- responses carry the shared consistency and freshness envelope
