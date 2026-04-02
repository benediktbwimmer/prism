# Shared Identity Future State

Status: short design note  
Audience: PRISM auth, runtime, storage, and MCP maintainers  
Scope: desired identity model once PRISM has a Postgres-backed shared runtime

---

## Summary

Once PRISM supports a Postgres shared runtime backend, principal identity should
become durable across machines without making the shared runtime itself into the
user's credential wallet.

The target split is:

- shared runtime is authoritative for principal registry state
- local machines remain authoritative for usable secret credential material
- active profile selection remains a local UX concern, not shared repo truth

This means agents and humans on different machines should be able to see the
same principal graph, credential metadata, revocation state, and capability
bindings, while still requiring an explicit local login or credential import
step before they can act as that principal.

## Desired Future State

The Postgres-backed shared runtime should store:

- principal authorities
- principals and parent relationships
- credential records and capability bindings
- credential verifiers or public keys
- credential lifecycle state such as issued, rotated, revoked, and last-used
- audit metadata for issuance and use

Local machine state should store:

- bearer tokens, private keys, or equivalent secret credential material
- active profile selection
- local convenience aliases or machine-specific profile names

PRISM should behave as follows:

- minting a principal from any PRISM checkout on machine A updates shared
  identity state for every machine using the same repo/shared backend
- machine B can discover that principal immediately through the shared runtime
- machine B still cannot act as that principal until it performs an explicit
  local credential acquisition step
- revoking a credential in shared runtime takes effect for every machine

## Non-Goals

The desired future state does not require:

- storing raw bearer tokens in Postgres
- treating "currently active profile" as shared runtime state
- automatically granting a new machine the ability to act as every principal
  already known to shared runtime

## Concrete TODOs

### 1. Separate Shared Credential Metadata From Local Secret Material

- Introduce an explicit boundary in core auth types between credential metadata
  stored in shared runtime and secret credential material stored locally.
- Make this split visible in docs, CLI help text, and API names so later code
  does not accidentally persist secrets into shared runtime.

### 2. Define The Shared Principal Registry Schema For Postgres

- Add first-class Postgres schema for principal authorities, principals,
  credentials, capability bindings, lifecycle timestamps, and revocation state.
- Preserve the same authority and principal id semantics already used by the
  SQLite shared runtime path.
- Define migration expectations from the current repo-shared SQLite registry.

### 3. Add Explicit Cross-Machine Credential Acquisition Flows

- Add a supported `prism auth export` / `prism auth import` flow, or an
  equivalent explicit handoff mechanism.
- Alternatively or additionally, support minting machine-scoped child
  credentials from an existing principal.
- Make the flow auditable so the shared runtime records which principal issued
  which credential and when.

### 4. Keep Active Login State Local

- Keep `~/.prism/credentials.toml` or its successor as the local source of truth
  for active profile selection on a machine.
- Allow multiple machines to choose different active profiles for the same repo
  without conflicting through shared runtime state.

### 5. Make Shared Revocation Immediate And Observable

- Ensure credential revocation in shared runtime invalidates later authenticated
  mutations on every connected machine.
- Add MCP and CLI introspection for "why auth failed" when a local credential
  exists but shared runtime marks it revoked or rotated away.

### 6. Document The User Story For PRISM-On-PRISM And Multi-Machine Use

- Document the difference between:
  - shared principal registry state
  - local login state
  - worktree-local execution state
- Add one short operator guide for:
  - mint on machine A
  - import or acquire on machine B
  - revoke centrally
  - verify failure on stale local credentials

## Acceptance Criteria

This future state is reached when:

- a principal minted from one machine appears in the shared registry on another
  machine without extra sync steps
- a new machine cannot act as that principal until it explicitly acquires local
  credentials
- revocation propagates through the shared runtime and blocks later use on all
  machines
- local active-profile choice remains machine-local and does not pollute shared
  runtime truth
