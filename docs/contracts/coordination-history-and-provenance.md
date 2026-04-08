# PRISM Coordination History And Provenance

Status: normative contract  
Audience: coordination, query, runtime, MCP, CLI, UI, and future service maintainers  
Scope: retained authoritative coordination history, transaction history, provenance, and retention visibility

---

## 1. Goal

PRISM must define one explicit contract for **coordination history and provenance**.

This contract exists so that:

- current-state reads and historical reads share one authority seam
- callers do not bypass the authority store for Git- or backend-specific history inspection
- compaction and retention do not become implicit behavior

This contract is a client of:

- [coordination-authority-store.md](./coordination-authority-store.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [provenance.md](./provenance.md)

Canonical ownership:

- this document defines coordination-specific retained history, timeline, compaction, and provenance
  query behavior
- [provenance.md](./provenance.md) defines the shared provenance envelope itself

## 2. Core invariants

The history and provenance layer must preserve these rules:

1. Current-state authority and retained history belong to the same authority seam.
2. History richness may vary by backend capability, but capability differences must be explicit.
3. Compacted or unavailable history must be surfaced honestly.
4. Provenance queries must identify the authoritative object, transaction, or snapshot they are
   describing.

## 3. Required history families

The contract must support at least these history families:

- historical current-state reconstruction at a retained authority point
- object timeline query
- transaction history query
- provenance query for current or historical objects
- retention and compaction visibility

## 4. Capability asymmetry

Current-state semantics are mandatory.
Retained-history richness is capability-driven.

That means:

- every authority backend must support current authoritative reads
- a backend may provide richer or cheaper historical queries than another backend
- the authority store must surface what history is available rather than pretending all backends
  are equal

## 5. Provenance minimums

Every historical or provenance result must identify, when known:

- coordination root identity
- authority backend kind
- authoritative version, stamp, commit, transaction id, or equivalent
- object id or transaction id being described
- whether the answer is complete, compacted, or partial

## 6. Compaction and retention

History queries must surface retention boundaries explicitly.

Examples:

- requested range is fully available
- requested range is partially compacted
- requested object timeline is truncated
- only the current authoritative snapshot remains

The contract must never force callers to infer retention loss indirectly from missing rows alone.

## 7. Relationship to product surfaces

Product surfaces must obtain coordination history and provenance through the authority seam.

That includes:

- CLI history or audit commands
- MCP provenance and inspection queries
- SSR console time-travel or audit views
- future PRISM Service history APIs

## 8. Minimum implementation bar

This contract is considered implemented only when:

- product code no longer reconstructs authoritative coordination history by bypassing the authority
  store
- compaction and retention are explicit in history answers
- provenance queries identify authoritative objects and versions clearly
