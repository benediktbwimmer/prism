# Coordination Authority Store Phase 1

Status: in progress
Audience: coordination, runtime, query, MCP, CLI, UI, storage, and authority-backend maintainers
Scope: complete Phase 1 implementation of the `CoordinationAuthorityStore` seam so all authoritative coordination access can route through one backend-neutral interface

---

## 1. Summary

This spec is the concrete implementation target for roadmap Phase 1:

- implement the `CoordinationAuthorityStore` fully enough that all authoritative coordination
  access can go through it

The goal of this phase is not to finish the full coordination cutover yet.
The goal is to finish the authority seam itself so later phases can migrate call sites onto a real
backend-neutral interface instead of more shared-ref-specific helpers.

This phase should leave PRISM with:

- one concrete authority interface
- one Git shared-ref backend behind it
- explicit authority read, transaction, history, runtime-descriptor, and diagnostics families
- no new product code added against direct shared-ref helpers

Runtime descriptor publication and discovery are fully in scope for this phase.
They are authority families, not later service-only concerns.

## 2. Status

Current state:

- [x] `CoordinationAuthorityStore` trait and type family finalized in code
- [x] Git shared-ref backend extracted behind the authority-store seam
- [x] current-state read families implemented through the new seam
- [ ] transactional mutation commit path implemented through the new seam
- [ ] retained history families implemented through the new seam
- [ ] runtime descriptor publication and discovery implemented through the new seam
- [ ] authority diagnostics and metadata exposed through the new seam
- [ ] direct product-facing shared-ref authority calls removed or redirected

Current slice notes:

- the backend-neutral authority module now exists in `crates/prism-core/src/coordination_authority_store/`
- the stable type family and Git-backed store shell compile and are exported from `prism-core`
- authoritative current-state loads in `published_plans` now route through the Git-backed
  authority-store seam
- the Git-backed implementation still needs real transaction, history, runtime-descriptor write,
  and diagnostics call-site cutover in later slices

## 3. Related roadmap

This spec implements:

- [../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md](../roadmaps/2026-04-08-coordination-to-spec-engine-to-service.md)

Specifically:

- Phase 1: Implement the Coordination Authority Store

## 4. Related contracts

This spec depends on:

- [../contracts/coordination-authority-store.md](../contracts/coordination-authority-store.md)
- [../contracts/coordination-history-and-provenance.md](../contracts/coordination-history-and-provenance.md)
- [../contracts/runtime-identity-and-descriptors.md](../contracts/runtime-identity-and-descriptors.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/identity-model.md](../contracts/identity-model.md)
- [../contracts/authorization-and-capabilities.md](../contracts/authorization-and-capabilities.md)
- [../contracts/provenance.md](../contracts/provenance.md)
- [../contracts/signing-and-verification.md](../contracts/signing-and-verification.md)

This spec also concretizes the companion migration note:

- [../contracts/coordination-authority-store-implementation-spec.md](../contracts/coordination-authority-store-implementation-spec.md)

## 5. Scope

This phase includes:

- defining the authority-store trait and stable type families in code
- implementing the current Git shared-ref backend behind that seam
- routing authoritative current reads, transactions, retained history, runtime descriptors, and
  diagnostics through the seam
- extracting or demoting shared-ref-specific helpers so the seam becomes the only product-facing
  authority boundary

This phase does not include:

- full local coordination SQLite cutover to a dedicated materialized-store seam
- full centralization of coordination evaluation into the query engine
- full transactional mutation-protocol cutover for all mutation entrypoints
- the full broad runtime and product-surface migration in roadmap Phase 5
- the spec engine
- the PRISM service

## 6. Non-goals

This phase should not:

- redesign coordination semantics again
- solve local materialization and authority in one abstraction
- leave “temporary” direct `.git` authority reads in product-facing code
- collapse Git-specific storage details into the public authority API
- attempt PostgreSQL backend implementation yet

## 7. Design

### 7.1 Target module boundary

Add or complete the authority-store module tree under:

```text
crates/prism-core/src/coordination_authority_store/
  mod.rs
  traits.rs
  types.rs
  git_shared_refs.rs
  history.rs
  diagnostics.rs
```

The exact file split may vary, but the architectural rule should remain:

- backend-neutral authority API at the top
- Git shared-ref implementation beneath it
- no shared-ref-specific types leaking upward unless nested inside backend detail payloads

### 7.2 Public semantic families

The code should expose one seam for:

- current authoritative state reads
- transactional mutation commit
- retained authoritative history
- runtime descriptor publication and discovery
- authority diagnostics and provenance

The seam must preserve:

- one active backend per coordination root
- strong versus eventual coordination semantics
- backend-neutral commit and rejection semantics
- explicit authority metadata and trust posture

### 7.3 Git backend

The initial backend is the current shared-ref implementation.

That backend should remain first-class, but after this phase it should be:

- invoked through `CoordinationAuthorityStore`
- treated as one backend implementation
- no longer treated as a product-facing utility surface

### 7.4 Product-facing boundary rule

After this phase:

- product code should depend on `CoordinationAuthorityStore`
- only backend implementation code should depend directly on shared-ref protocol mechanics

This rule applies to:

- session
- watch
- indexer/bootstrap
- published plans and repo-state exports
- runtime descriptor and diagnostics surfaces
- MCP and CLI authority-facing reads and writes

### 7.5 Eventual versus strong

The authority-store seam should preserve the semantics already defined in contracts:

- `eventual`
  - coordination answer from previously verified locally available authority-backed state
- `strong`
  - refresh or verify against the active authority backend before answering

This does not mean the authority store must own SQLite itself.
It means the authority store owns the semantic distinction and returns enough metadata for
downstream materialization to remain honest.

The concrete storage, reuse, and invalidation of local persistent materialization remain the concern
of Phase 2.

### 7.6 Authority-store adoption rule

This phase may use compatibility wrappers or adapters internally to preserve behavior while the seam
is being introduced.

However:

- product-facing modules must not gain new direct dependencies on shared-ref mechanics
- any temporary compatibility wrapper must live below the authority-store seam
- any newly discovered product-facing authoritative access path encountered during this phase must
  either be migrated here or be documented explicitly as deferred with a reason

This rule exists to stop new bypasses from appearing while the seam is being created.

## 8. Implementation slices

### Slice 1: Trait and type family

- finalize the authority-store trait
- finalize request and response types
- finalize capability, diagnostics, and backend-detail envelopes

Exit criteria:

- code outside the backend can compile against the trait without shared-ref-specific imports

### Slice 2: Git backend extraction

- move or wrap shared-ref logic behind the new backend implementation
- demote shared-ref helpers from product-facing modules
- preserve current Git behavior behind the seam

Exit criteria:

- Git shared-ref authority behavior is reachable through the backend implementation only

### Slice 3: Current-state read cutover

- route authoritative current reads through the authority-store seam
- expose current-state metadata and freshness shape consistently

Exit criteria:

- product-facing authoritative current reads no longer call shared-ref helpers directly

### Slice 4: Transaction cutover

- route authoritative coordination commit through the authority-store seam
- expose commit result metadata through shared types
- handle transport-uncertain commit outcomes explicitly

Exit criteria:

- authoritative commit path exists through the seam without leaking Git mechanics
- ambiguous transport outcomes do not get flattened into false success or false failure

### Slice 5: History, descriptors, diagnostics

- route retained-history queries through the seam
- route runtime descriptor publication and discovery through the seam
- route authority diagnostics through the seam

Exit criteria:

- current state, history, runtime descriptor, and diagnostics families are all reachable through the
  authority-store interface

## 9. Migration targets

The migration note already names the high-value call sites.
This phase should prioritize them in this order:

1. `crates/prism-core/src/session.rs`
2. `crates/prism-core/src/indexer.rs`
3. `crates/prism-core/src/published_plans.rs`
4. `crates/prism-core/src/watch.rs`
5. runtime descriptor and diagnostics surfaces in MCP and CLI
6. any remaining direct protected-state or shared-ref authority helpers in product-facing code

This phase does not need to finish all downstream cutover, but it must make those migrations
possible without further seam redesign.

## 10. Validation

Minimum validation for this phase:

- targeted tests for `prism-core`
- direct downstream tests for crates whose public coordination read or mutation surface depends on
  `prism-core`
- focused tests for:
  - authoritative current reads
  - authoritative transaction commit path
  - retained-history queries
  - runtime descriptor publication/discovery
  - authority diagnostics

Representative commands should likely include at least:

- `cargo test -p prism-core --no-run`
- targeted `prism-core` tests covering shared coordination refs and session authority flows
- targeted downstream `prism-mcp` tests when public coordination access types or behavior change

This phase does not require a full workspace suite by default.

## 11. Completion criteria

This phase is complete when:

- `CoordinationAuthorityStore` is a real code seam, not only a docs concept
- Git shared refs are fully reachable through that seam
- current authoritative reads, authoritative commit, retained history, runtime descriptors, and
  diagnostics all route through it
- the rest of the app no longer needs direct shared-ref helpers to perform authoritative
  coordination access

## 12. Follow-on phases

This phase intentionally prepares:

- Phase 2: Coordination Materialized Store
- Phase 3: Coordination Query Engine
- Phase 4: Transactional Coordination Mutation Protocol
- Phase 5: full coordination cutover

The expected outcome is a stable authority foundation that later phases can depend on without
another interface rewrite.
