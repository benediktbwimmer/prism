# Coordination Mutation Protocol Phase 4

Status: in progress
Audience: coordination, service, runtime, MCP, CLI, UI, query, and storage maintainers
Scope: complete Phase 4 implementation of the backend-neutral transactional coordination mutation protocol so authoritative coordination writes converge on one semantic path

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 4:

- implement the **Transactional Coordination Mutation Protocol** fully enough that authoritative
  coordination writes share one explicit protocol, one validation ordering, and one commit-result
  model

This is a convergence phase, not a greenfield one.

PRISM already has a partial `coordination_transaction` path in `prism-query`, and many MCP
coordination mutations already lower into it.
Phase 4 completes the protocol so that:

- the existing transaction path becomes the real semantic write boundary
- replay, rejection, commit metadata, and downstream update semantics are defined there
- convenience mutation surfaces become adapters over that protocol instead of parallel meanings
- later service-side mutation brokering can depend on a settled write model rather than ad hoc host
  mutation logic

This phase now targets the accepted architecture decision that:

- interactive coordination participation is service-backed
- service-owned coordination materialization is downstream of authoritative commit
- runtimes do not own coordination state directly

## 2. Status

Current state:

- [x] a partial `coordination_transaction` engine exists in `prism-query`
- [x] MCP already exposes `coordination_transaction` as one mutation variant
- [x] many convenience mutation kinds already lower into the transaction engine
- [ ] transaction intent shape is aligned with the contract fully
- [x] validation ordering is made explicit and centralized for shape, authorization, and identity stages
- [ ] deterministic rejection categories and codes are formalized fully
- [ ] optimistic preconditions and authority-base conflict handling are implemented coherently
- [x] explicit transaction outcome and commit metadata now flow through the protocol result
- [ ] authoritative commit-result metadata is unified across mutation surfaces
- [ ] local materialization follow-through is explicitly downstream of authoritative commit
- [ ] legacy convenience mutation semantics are fully reduced to protocol adapters

Current slice notes:

- `crates/prism-query/src/coordination_transaction.rs` already owns a real mutation core, but still
  rejects `intent_metadata` and `optimistic_preconditions` as unsupported
- the current transaction path still mutates the in-memory coordination runtime directly rather
  than presenting a fully shaped backend-neutral protocol result
- `prism-mcp` still contains substantial mutation lowering and response shaping in
  `host_mutations.rs`
- the protocol must now be finished against the service-backed architecture recorded in
  [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)
- the first implementation slice now exposes explicit transaction outcome and commit metadata from
  `prism-query` into MCP-facing response shaping; later slices still need rejected and indeterminate
  outcomes plus shared authority-stamp semantics
- the second implementation slice now makes validation ordering explicit in code for:
  - input-shape validation
  - authorization placeholder staging
  - object identity and ordered client-reference validation
  - typed rejected outcomes for those stages
  later slices still need conflict handling, indeterminate outcomes, and shared rejected-result
  shaping through all mutation surfaces

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 4: Implement the Transactional Coordination Mutation Protocol

## 4. Related contracts

This spec depends on:

- [../contracts/coordination-mutation-protocol.md](../contracts/coordination-mutation-protocol.md)
- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-artifact-review-model.md](../contracts/coordination-artifact-review-model.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/authorization-and-capabilities.md](../contracts/authorization-and-capabilities.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/identity-model.md](../contracts/identity-model.md)
- [../contracts/service-mutation-broker.md](../contracts/service-mutation-broker.md)
- [../adrs/2026-04-08-service-owned-coordination-materialization.md](../adrs/2026-04-08-service-owned-coordination-materialization.md)

## 5. Scope

This phase includes:

- finalizing the protocol-facing request and result family in code
- making transaction validation ordering explicit and centralized
- defining deterministic rejection categories and result metadata
- wiring authoritative commit metadata through one shared result shape
- aligning current MCP coordination mutation entrypoints with the protocol as adapters
- making service-owned materialization and checkpoint advancement explicitly downstream of
  authoritative commit

This phase does not include:

- the full broad service-backed runtime cutover in roadmap Phase 5
- replacing every user-facing convenience mutation surface immediately
- implementing the full PRISM Service mutation broker role
- PostgreSQL backend implementation
- native spec-engine sync actions

## 6. Non-goals

This phase should not:

- redesign plan, task, artifact, or review semantics again
- collapse the mutation protocol into service-only logic
- blur authoritative commit with service-owned materialization advancement
- leave mutation meaning split across many MCP-only helper paths
- attempt the full service deployment/runtime participation cutover yet

## 7. Design

### 7.1 Target boundary

Phase 4 should leave PRISM with one obvious write-side semantic boundary:

- authoritative coordination writes are expressed as coordination transaction intents
- those intents are validated, conflict-checked, committed, rejected, and reported through one
  shared protocol path
- convenience mutation forms are reduced to lowering adapters over that protocol

The preferred ownership split is:

- `prism-query`
  - protocol request/result types
  - validation ordering
  - staged transaction semantics
  - authoritative commit result shaping
- `prism-core`
  - authority-store commit boundary
  - service-owned materialization follow-through beneath commit
- `prism-mcp`
  - input lowering from MCP payloads into protocol intents
  - user-facing response formatting only

### 7.2 Service-backed assumption

This phase must assume the accepted service-owned model:

- interactive coordination participation is service-backed
- authoritative commit success is defined by the authority backend
- service-owned coordination materialization advances after commit
- runtimes do not own coordination SQLite state or define commit success

That means commit semantics must not depend on:

- local runtime cache updates
- worktree-local checkpoint writes
- handler-local optimistic assumptions

### 7.3 Validation ordering

The implementation should make the contract’s validation ordering concrete:

1. input shape validation
2. authorization and capability validation
3. object existence and identity validation
4. staged domain validation
5. authoritative-base conflict detection
6. commit

This ordering should be visible in code and testable as protocol behavior, not only implied by call
 flow.

### 7.4 Commit result model

The protocol result should converge on one shared shape carrying at least:

- outcome class
  - committed
  - rejected
  - indeterminate when transport/authority certainty is unavailable
- authoritative post-commit stamp or version
- touched and created object ids
- rejection category and stable reason codes when rejected
- provenance-bearing commit metadata needed by later history and diagnostics surfaces

### 7.5 Convenience mutation rule

After this phase:

- `coordination_transaction` is the canonical structural write surface
- convenience mutation kinds such as `plan_create`, `task_create`, `update`, and
  `plan_bootstrap` remain allowed as input ergonomics
- but their meaning must be implemented by lowering into the same protocol path

No convenience mutation surface should retain separate hidden semantics once the protocol covers the
 same behavior.

## 8. Implementation slices

### Slice 1: Finalize protocol types

- complete the protocol request and result family
- define explicit rejection and outcome enums
- define commit metadata and post-commit authority-stamp fields

Exit criteria:

- code outside the protocol can depend on one stable transaction request/result family

Progress:

- [x] committed outcome and commit metadata now have explicit protocol types
- [ ] rejection and indeterminate outcome families remain to be added
- [ ] authority-stamp fields remain to be added

### Slice 2: Centralize validation ordering

- make shape, authz, existence, domain, and conflict validation stages explicit
- remove scattered validation-order assumptions from host mutation handlers where possible
- ensure staged whole-transaction validation is the dominant path

Exit criteria:

- validation ordering is visible and testable through the protocol boundary

Progress:

- [x] shape, authorization, and identity stages are now explicit protocol functions
- [x] stable typed rejections exist for empty transactions, unsupported fields, duplicate client ids,
  missing ids, and forward client references
- [ ] domain-stage rejected results still need to converge with the same shared rejected-result
  envelope instead of relying on downstream audit interpretation

### Slice 3: Conflict, replay, and deterministic rejection

- implement or tighten optimistic-precondition and authoritative-base handling
- define when replay is allowed versus rejected
- surface stable rejection categories and reason codes

Exit criteria:

- retryable conflict and deterministic invalid input are distinguished through one protocol result

### Slice 4: Commit result and downstream follow-through

- unify authoritative commit result metadata
- make service-owned materialization advancement explicitly downstream of committed authority
- remove remaining ambiguity between “accepted”, “committed”, and “materialized”

Exit criteria:

- authoritative commit success has one meaning and one result shape

### Slice 5: Surface convergence

- reduce MCP convenience mutation kinds to lowering adapters over the protocol
- remove obvious parallel mutation semantics from host mutation helpers
- update specs, examples, and schema surfaces to point at the canonical protocol path

Exit criteria:

- the repo has one dominant authoritative coordination write path

## 9. Validation

Minimum validation for this phase:

- targeted `prism-query` tests covering:
  - whole-transaction validation
  - rejection categories
  - optimistic-precondition or conflict behavior
  - authoritative commit result metadata
- direct downstream validation in:
  - `prism-mcp`
  - `prism-cli`
  when protocol-facing types or mutation-surface semantics change
- `git diff --check`

Important regression checks for this phase:

- convenience MCP mutations still behave the same once lowered through the protocol
- rejection results remain structured and actionable
- authoritative commit is not conflated with downstream materialization updates

## 10. Completion criteria

Phase 4 is complete only when:

- authoritative coordination writes no longer bypass the protocol semantically
- validation ordering is explicit and centralized
- conflict, replay, and rejection behavior are defined through one protocol path
- commit results carry authoritative version metadata and stable outcome classes
- service-owned materialization is clearly downstream of commit
- the Phase 4 spec and roadmap are updated to `completed`

## 11. Implementation checklist

- [ ] Finalize protocol request and result families
- [ ] Centralize validation ordering
- [ ] Implement deterministic rejection categories and codes
- [ ] Implement conflict and replay handling coherently
- [ ] Unify authoritative commit-result metadata
- [ ] Make service-owned materialization explicitly downstream of commit
- [ ] Reduce convenience mutation surfaces to protocol adapters
- [ ] Validate `prism-query`, `prism-mcp`, and `prism-cli`
- [ ] Mark Phase 4 complete in the roadmap

## 12. Current implementation status

The protocol foundation already exists:

- `crates/prism-query/src/coordination_transaction.rs` provides a real transaction core
- `prism-mcp` already lowers `coordination_transaction`, `plan_bootstrap`, `plan_create`,
  `plan_update`, `plan_archive`, `task_create`, and `update` through that core in many cases
- the MCP self-description already points callers toward `coordination_transaction` as the
  canonical structural write surface

What remains incomplete is the actual protocol convergence:

- unsupported `intent_metadata` and `optimistic_preconditions`
- incomplete explicit result taxonomy
- incomplete separation between authoritative commit and downstream follow-through
- too much surface-local lowering and response shaping in `host_mutations`
- no clearly finished service-backed mutation model yet

The next Phase 4 slice should therefore focus on protocol type/result convergence first, not on
adding new mutation kinds.
