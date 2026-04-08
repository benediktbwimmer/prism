# PRISM Authority Sync Contract

Status: normative contract  
Audience: coordination, runtime, storage, watch, MCP, CLI, and future service maintainers  
Scope: discovery of authoritative changes, refresh behavior, checkpoint rebuild behavior, and synchronization semantics

---

## 1. Goal

PRISM must define one explicit **authority sync** contract between the authority store and local
materialization or runtime consumers.

This contract exists so that:

- refresh behavior is not embedded ad hoc in watch loops and session helpers
- self-write suppression is defined once
- startup checkpoint rebuild and invalidation behavior is explicit
- strong versus eventual read semantics have a concrete synchronization boundary

This contract is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [local-materialization.md](./local-materialization.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [signing-and-verification.md](./signing-and-verification.md)

## 2. Core invariants

Authority sync must preserve these rules:

1. Authority discovery and refresh are downstream of the active authority backend.
2. Local sync logic may detect and import new authoritative state, but may not invent authority.
3. Self-write suppression must prevent churn without hiding genuine remote advancement.
4. Verification failure must fail honestly rather than silently accepting suspect state.
5. Sync status must be explainable in terms of authority metadata and freshness state.

## 3. Responsibilities

Authority sync is responsible for:

- discovering whether authority has advanced
- obtaining a verified current authoritative snapshot or equivalent current-state view
- updating local materialization from that verified input
- rebuilding or invalidating startup checkpoints when needed
- suppressing immediate self-write loops

Authority sync is not responsible for:

- deciding coordination domain semantics
- inventing mutation meaning
- bypassing the authority store

## 4. Discovery contract

Authority sync must discover change through the authority store.

That may be implemented as:

- a dedicated poll or refresh call on the authority-store surface
- a strong metadata read compared against a locally remembered authority stamp

The contract does not require one particular mechanism.
It does require one authority-backed answer to: "has current authoritative state advanced?"

## 5. Verified current versus stale

Authority sync must map its results into the shared freshness language from
[consistency-and-freshness.md](./consistency-and-freshness.md).

At minimum it must distinguish:

- verified current
- verified stale
- unavailable

## 6. Self-write suppression

Self-write suppression exists to avoid treating a just-published local commit as a surprising
external update.

It must:

- recognize local authoritative writes recently produced by this runtime or service path
- suppress redundant re-import loops for those writes
- still allow later remote advancement to be imported normally

Self-write suppression must be bounded by authority metadata, not by hidden time-only heuristics.

## 7. Verification failure

If authoritative state cannot be verified, authority sync must:

- refuse to treat the candidate state as verified current
- preserve the most recent trusted local materialization if one exists
- surface failure metadata so callers can degrade honestly

It must not:

- silently advance local materialization from unverified input
- erase trusted local state just because verification is temporarily unavailable

## 8. Checkpoint rebuild behavior

Authority sync owns the decision of when startup checkpoints should be:

- reused
- refreshed
- invalidated
- rebuilt

Those decisions must be based on explicit authority and schema inputs rather than broad guesses.

## 9. Relationship to reads

Strong reads may drive authority sync immediately.
Eventual reads may observe the most recent successful materialization.

This preserves the rule:

- strong may force refresh
- eventual may reuse known local state

## 10. Minimum implementation bar

This contract is considered implemented only when:

- authority refresh behavior no longer depends on backend-specific watch helpers leaking upward
- self-write suppression is explicit and testable
- checkpoint rebuild behavior is driven by explicit inputs
- verification failure and stale state surface through the shared freshness contract
