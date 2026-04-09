# PRISM Consistency And Freshness

Status: normative contract  
Audience: coordination, query, runtime, MCP, CLI, UI, and future service maintainers  
Scope: shared language and required metadata for read freshness, consistency, and availability

---

## 1. Goal

PRISM must use one shared consistency and freshness vocabulary across coordination reads.

This contract exists so that:

- strong and eventual reads mean the same thing everywhere
- callers can tell whether an answer is current, stale, or unavailable
- backends and materializers can vary without changing the meaning of result metadata
- product surfaces can report freshness honestly without forcing users to think in storage jargon

This contract relies on:

- [signing-and-verification.md](./signing-and-verification.md)
- [provenance.md](./provenance.md)

## 2. Required read modes

PRISM coordination reads must support these modes semantically:

- `eventual`
  - answer from an allowed lagging derived view when one exists
- `strong`
  - refresh against the active authority backend before answering

These are semantic contracts, not storage-shape requirements.
Product surfaces may expose freshness and source metadata instead of making users choose between
these modes directly in ordinary flows.

## 3. Required freshness states

Every bounded coordination read must describe the freshness state of the authoritative input it
used.

The shared freshness states are:

- `verified_current`
  - authoritative input was checked successfully for the requested mode and is current for that
    read
- `verified_stale`
  - the system knows the answer is based on an older verified authority input
- `unavailable`
  - the system cannot currently provide the requested authority guarantees

An implementation may expose richer internal states, but it must map them back to these shared
states for product consumers.

These states have trust meaning as well as recency meaning:

- `verified_current` means current and verified under the active trust rules
- `verified_stale` means older but still trusted authoritative input
- `unavailable` means the required authority or verification guarantee could not be established

## 4. Required response envelope

Every coordination read response must expose a consistency envelope.

The envelope must include at least:

- requested read mode
- actual read mode served
- freshness state
- coordination root identity
- authority backend kind
- authority stamp, version, or equivalent identifier when known
- materialized-at timestamp when the answer came from local materialization

The envelope may also include:

- verification time
- backend-specific provenance details
- explanation of why a stronger answer could not be provided

## 5. Read semantics

### 5.1 Eventual

`eventual` means:

- serve from an allowed lagging derived view when one exists
- do not require a fresh authority refresh before answering
- surface the authority stamp or materialization basis when known

### 5.2 Strong

`strong` means:

- refresh or verify against the active authority backend before answering
- fail honestly if that guarantee cannot be provided

It does not mean:

- every downstream projection or cache is already rewritten
- every local runtime has already observed the result

When the active backend is DB-backed and no lagging projection is configured:

- `eventual` may be satisfied by the same current-authority path as `strong`

## 6. Unavailable behavior

When the requested guarantee cannot be met, the system must degrade honestly.

Examples:

- return `unavailable` for a requested strong read when the authority backend cannot currently be
  reached or verified
- return a clearly marked eventual answer only when the caller requested eventual or explicitly
  allowed degradation

The system must not silently downgrade a strong read to eventual without surfacing that downgrade in
the envelope.

## 7. Relationship to local materialization

Local materialization may accelerate reads, but it must not redefine consistency terms.

The key distinction is:

- authority determines what is true
- materialization determines what can be served quickly

## 8. Relationship to product surfaces

All coordination-facing product surfaces must use this vocabulary.

That includes:

- MCP reads
- CLI status and inspection commands
- console and API read models
- future PRISM Service read broker

Surface-specific wording may vary, but the underlying state mapping must come from this contract.

User-facing product surfaces should usually expose:

- freshness
- source class
- degraded or unavailable posture

rather than forcing explicit strong-versus-eventual mode selection in routine flows.

## 9. Minimum implementation bar

This contract is considered implemented only when:

- strong and eventual mean the same thing across coordination reads
- every coordination read exposes a consistency envelope
- stale and unavailable answers are surfaced explicitly instead of implied indirectly
