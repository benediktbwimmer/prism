# Finalize SQL Coordination Authority Contract

Status: implemented
Audience: coordination, storage, runtime, query, MCP, CLI, and service maintainers
Scope: remove the last snapshot-era coupling from the SQL authority abstraction so SQLite and future Postgres target one final SQL-native contract

---

## 1. Summary

The Postgres-ready seam hardening was necessary, but it still left a few leaks:

- the hot read was still named `read_plan_state` and still returned a hydrated snapshot bundle
- the public append contract still exposed derived-state persistence policy
- normal authority transaction results still leaked `prism_store::CoordinationPersistResult`
- the primary authority surface was still one large trait
- the snapshot seam still inherited the hot authority seam

Those leaks are small individually, but together they still bias callers and future backends toward
snapshot-era assumptions.

This spec finishes that cleanup so:

- the hot read becomes a direct current-state read
- the hot SQL authority surface is split into smaller responsibilities
- snapshot access is an explicit standalone seam
- write receipts carry backend-neutral commit metadata
- SQLite and the Postgres stub both compile against that final contract

## 2. Goals

Progress note (2026-04-10):

- implementation landed by replacing `read_plan_state` with `read_current_state`, removing
  `CoordinationDerivedStateMode` from the public append contract, replacing
  `CoordinationPersistResult` with `CoordinationCommitReceipt`, splitting the authority surface
  into smaller traits, and making the snapshot seam standalone instead of inheriting the hot seam

### 2.1 Replace the misleading hot-path read

The primary authority seam should not expose `read_plan_state`.

That name suggests a shaped plan-level projection, but in practice it returned:

- the full coordination snapshot
- the canonical snapshot v2
- runtime descriptors

The contract should call that what it is:

- `read_current_state`

The type should also be the direct authority current-state struct:

- `CoordinationCurrentState`

### 2.2 Remove storage-policy leakage from event appends

The public authority append request should not contain derived-state persistence policy.

That is an implementation concern owned by the persistence/materialization pipeline, not by the
authority backend contract.

Remove:

- `CoordinationDerivedStateMode`
- `CoordinationAppendRequest.derived_state_mode`

### 2.3 Replace store-specific persistence receipts with backend-neutral commit metadata

The normal authority transaction result should not expose `prism_store::CoordinationPersistResult`.

Instead it should expose a backend-neutral receipt, for example:

- committed revision
- inserted event count
- whether the append materially applied

That keeps the contract useful for callers without forcing Postgres to emulate SQLite store types.

### 2.4 Split the primary SQL authority surface into smaller traits

Break the monolith into narrower interfaces:

- current-state reads
- mutation writes
- runtime descriptor operations
- event execution record operations
- history reads
- diagnostics

Retain one composite facade so most callers can still request one store object, but define the
contract in narrower pieces.

### 2.5 Make the snapshot seam standalone

`CoordinationAuthoritySnapshotStore` should no longer inherit the hot authority seam.

Snapshot consumers should not gain hot mutation or hot read APIs just by opening the snapshot
surface.

The snapshot seam should contain only:

- `read_snapshot`
- `read_snapshot_v2`
- `replace_current_state`

## 3. Non-goals

This spec does not:

- implement the full Postgres backend
- redesign every future read-model query into bespoke SQL projections
- remove snapshot-oriented recovery code from the repo entirely
- change coordination-domain semantics
- implement execution-substrate work

## 4. Design

### 4.1 Public authority traits

Define narrower public traits:

- `CoordinationAuthorityCurrentStateStore`
- `CoordinationAuthorityMutationStore`
- `CoordinationAuthorityRuntimeStore`
- `CoordinationAuthorityEventExecutionStore`
- `CoordinationAuthorityHistoryStore`
- `CoordinationAuthorityDiagnosticsStore`

Define `CoordinationAuthorityStore` as the composite facade over those narrower traits.

### 4.2 Current-state read contract

Replace:

- `read_plan_state(...) -> CoordinationReadEnvelope<HydratedCoordinationPlanState>`

With:

- `read_current_state(...) -> CoordinationReadEnvelope<CoordinationCurrentState>`

Callers that still need a hydrated plan state can build it explicitly with existing conversions.

### 4.3 Snapshot seam

Keep a dedicated snapshot seam, but make it standalone:

- `CoordinationAuthoritySnapshotStore`

It should not extend the hot authority store.

### 4.4 Transaction receipt

Add a backend-neutral commit receipt, for example:

- `CoordinationCommitReceipt { revision, inserted_events, applied }`

Update `CoordinationTransactionResult` to expose:

- `commit: Option<CoordinationCommitReceipt>`

And remove:

- `persisted: Option<CoordinationPersistResult>`

### 4.5 DB trait split

Mirror the same split inside `coordination_authority_store/db` so SQLite and Postgres implement
the same narrower contracts internally.

### 4.6 Call-site migration

Migrate:

- `prism-core` authority reads to `read_current_state`
- MCP read-broker authority reads to `read_current_state`
- tests that asserted on `persisted` to assert on backend-neutral commit receipts
- snapshot consumers to explicit `CoordinationCurrentState` to hydrated-plan conversions where needed

## 5. Implementation plan

1. Update the roadmap and add this spec.
2. Update doc indices.
3. Replace `read_plan_state` with `read_current_state`.
4. Remove `CoordinationDerivedStateMode` from the public authority API.
5. Introduce backend-neutral commit receipt types.
6. Split the public authority seam into smaller traits plus one composite facade.
7. Split the DB seam the same way.
8. Update SQLite implementations and tests.
9. Update the Postgres stub.
10. Migrate call sites.
11. Run targeted and full validation.

## 6. Validation

Minimum validation for this contract refactor:

- `cargo test -p prism-core -p prism-mcp -p prism-cli`

Required broader validation because this changes the shared SQL authority contract:

- `cargo test`

## 7. Exit criteria

- the hot authority seam no longer exposes `read_plan_state`
- the hot authority append request no longer leaks derived-state persistence policy
- the hot authority transaction result no longer leaks `prism_store::CoordinationPersistResult`
- the hot SQL authority seam is defined as smaller traits with one composite facade
- the snapshot seam is standalone rather than inheriting the hot seam
- SQLite and the Postgres stub compile and pass behind that contract
