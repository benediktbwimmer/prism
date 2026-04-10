# PRISM Contracts Specification

Status: product-design note
Role: concept and product-shape exploration for PRISM contracts as a feature family, not the
normative source for live contract semantics

Normative contract definitions now live under [contracts/README.md](../contracts/README.md).

## 0. Thesis

PRISM currently models:

* structure
* concepts
* plans
* memory

What it does not yet model explicitly is the **promise layer** of a repository.

A repository is not only a pile of files, symbols, tests, and outcomes. It is also a web of promises:

* this module provides this API shape
* this boundary must remain one-way
* this workflow expects these inputs
* this schema field remains stable under these assumptions
* these consumers are allowed to rely on this behavior

Those promises are more than facts and less than tests. They are not fully captured by concepts, and they are not the same thing as policies.

This document proposes **Contracts** as a first-class PRISM primitive.

## 1. Primitive Definition

A **Contract** is a reusable semantic object that describes a promise across a boundary in a repository.

A contract answers:

1. who is making the promise
2. to whom the promise matters
3. what is being guaranteed
4. under which assumptions the guarantee holds
5. how the guarantee is validated
6. what kinds of change are compatible, risky, or breaking

Contracts should be repo-agnostic in the same way concepts are repo-agnostic. The model must work across:

* libraries
* applications
* monorepos
* frontend systems
* backend systems
* infrastructure/config repositories
* language tooling and compiler repositories

The repo-specific part should be the discovered contract instances, not the core ontology.

## 2. Why Contracts Instead Of Policies First

Policies are useful, but they are downstream of something more fundamental.

A policy usually says:

* this must not happen
* this boundary must not be crossed
* this behavior is forbidden here

A contract says something more complete:

* this surface provides these guarantees
* these consumers may rely on those guarantees
* these assumptions must remain true
* these validations demonstrate the promise
* these changes are compatible and these are breaking

Policies can often be derived from contracts. Contracts cannot be reconstructed cleanly from policies alone.

If PRISM adds contracts first, later policy features can attach to the contract layer instead of inventing another disconnected rule system.

## 3. Goals

Contracts should let PRISM:

* distinguish implementation edits from contract edits
* surface downstream consumers of a promise, not only adjacent files
* explain why a change is risky in terms of broken guarantees
* connect validations to the promises they defend
* make review, planning, and impact analysis more semantic
* publish durable repo knowledge about boundaries and guarantees

## 4. Non-Goals

Contracts are not:

* a full theorem prover
* an always-blocking rule engine
* a replacement for tests
* a substitute for concepts
* a generic prose note system
* a repo-specific ADR mechanism disguised as a primitive

The first implementation should not try to prove arbitrary semantic properties. It should provide a strong, inspectable knowledge object with clear evidence and useful query integration.

## 5. Relationship To Existing PRISM Primitives

### 5.1 Concepts

Concepts answer: **what belongs together?**

Contracts answer: **what is promised across a boundary?**

A concept may own or contain many contracts. A contract may reference one or more concepts, but it should not collapse into a concept packet.

### 5.2 Plans

Plans describe intended work.

Contracts describe the promises that work may preserve, expand, migrate, or break.

Plans should be able to reference contracts explicitly when a task is contract-sensitive.

### 5.3 Memory And Outcomes

Memory records learned facts from work.

Contracts describe durable promises that should hold independent of one debugging session.

Memories and outcomes can supply evidence for contracts, especially when a failure revealed the real boundary of a promise.

### 5.4 Policies

Policies are best treated as derived guardrails over contracts:

* a contract defines the guarantee
* a policy defines how PRISM evaluates or enforces that guarantee in practice

Not every contract needs a policy. Some contracts may remain descriptive and review-oriented.

### 5.5 Decisions

Architectural decisions are useful supporting evidence, but they do not need to be a first-class primitive before contracts exist.

The first implementation can model rationale as contract provenance or supporting evidence.

### 5.6 Behaviors

Behavior classes such as network calls, storage mutation, or process spawning are valuable analysis inputs.

But they are better treated as evidence or derived signals that contracts can reference, not as a prerequisite for the contracts primitive.

## 6. Contract Object Model

The model below is intentionally repo-general.

### 6.1 Required Fields

* `id`
  Stable contract identifier.
* `name`
  Human-readable canonical name.
* `summary`
  Short explanation of the promise.
* `subject`
  The provider or governed surface making the promise.
* `kind`
  The general contract type.
* `guarantees`
  The promises consumers may rely on.

### 6.2 Recommended Fields

* `assumptions`
  Preconditions or environmental expectations under which the contract holds.
* `consumers`
  Known consumers, dependents, or affected surfaces.
* `validations`
  Tests, checks, examples, or other evidence paths that support the contract.
* `stability`
  Experimental, internal, public, deprecated, migrating, or similar stability signal.
* `compatibility`
  Guidance on what kinds of edits are additive, risky, or breaking.
* `provenance`
  Where the contract came from: manual curation, inferred from code, inferred from tests, promoted from memory, etc.
* `evidence`
  Anchored supporting material such as tests, failures, docs, lineage, or observed outcomes.
* `status`
  Candidate, active, deprecated, retired.

### 6.3 Subject

The `subject` must be general enough to apply across repositories.

It may target:

* a symbol
* a module
* a file boundary
* a concept
* an API surface
* a schema
* a config surface
* a workflow surface
* a service boundary
* a storage boundary

The subject should be anchored through existing PRISM handles and lineage-aware references where possible.

### 6.4 Kind

The initial `kind` enum should remain compact and repo-general. A reasonable starting set is:

* `interface`
* `behavioral`
* `data_shape`
* `dependency_boundary`
* `lifecycle`
* `protocol`
* `operational`

Notes:

* `interface` covers callable/API/public-surface promises.
* `behavioral` covers semantic behavior under inputs and state.
* `data_shape` covers schemas, wire formats, configs, and persisted records.
* `dependency_boundary` covers directional architectural promises.
* `lifecycle` covers init, startup, teardown, session, migration, or state transitions.
* `protocol` covers multi-step interactions between actors or systems.
* `operational` covers build/test/deploy/runtime workflow guarantees.

The set can expand later, but the first implementation should avoid a long taxonomy.

### 6.5 Guarantees

Guarantees should be explicit, structured, and ideally list-shaped rather than opaque prose.

A guarantee entry may include:

* id
* statement
* scope
* strength
* evidenceRefs

Each guarantee clause should have a stable sub-ID so validations, evidence, compatibility notes, and later policies can attach to a specific promise rather than only the contract as a whole.

The first version can represent guarantees as compact structured text objects rather than a formal logic language.

### 6.6 Assumptions

Assumptions are first-class because many repository promises are conditional.

Examples of assumption categories:

* feature flag enabled
* environment variable present
* storage backend unchanged
* input validated upstream
* schema version at least N
* operation limited to internal callers

PRISM should not treat a contract as globally unconditional if its assumptions are part of the promise.

### 6.7 Consumers

Consumers are important because they turn a local promise into impact information.

Consumers may include:

* direct symbol callers
* downstream modules
* concepts
* tests
* workflows
* external integration surfaces

Consumer lists may be incomplete at first, but the model should allow explicit known consumers and inferred consumers to coexist.

### 6.8 Validations And Evidence

Validation is central. A contract without any validation link is weaker knowledge than a contract backed by tests, examples, or repeated outcome evidence.

Validation references may include:

* tests
* check commands
* example scripts
* static checks
* observed successful outcomes
* observed failures that sharpened the boundary

Evidence should remain anchored, inspectable, and separate from the contract summary itself.

## 6.9 Contract Health

Contracts will drift just like concepts do.

The first implementation should include a compact derived health model so contracts do not become elegant but stale prose.

A reasonable first status vocabulary is:

* `healthy`
* `watch`
* `degraded`
* `stale`
* `superseded`
* `retired`

The first health signals can stay narrow:

* validation coverage across guarantee clauses
* clause-level evidence coverage
* stale validation anchors
* superseded-by relationships

This does not need a full rule engine in V1. A small derived health view is enough if it keeps contracts inspectable and prompts refresh work before they rot.

## 7. Change Semantics

Contracts should help PRISM reason about change classes.

The model should support at least:

* `compatible`
* `additive`
* `risky`
* `breaking`
* `migrating`

This does not require full automation in the first version. Even a curated compatibility section is useful if it makes impact and review surfaces better.

## 8. Storage And Lifecycle

Contracts should follow the same broad promotion ladder PRISM already uses for other durable knowledge:

* `local`
* `session`
* `repo`

Suggested lifecycle:

1. candidate contract discovered during work
2. local or session-scoped contract recorded
3. evidence and validations attached
4. contract promoted to repo scope when stable
5. contract updated, narrowed, deprecated, or retired as the codebase changes

Repo-scoped contracts should be published knowledge, not scratch notes.

## 9. Event Model

The first implementation should prefer an append-only event model over mutable hidden state.

Suggested operations:

* `promote`
* `update`
* `retire`
* `attach_evidence`
* `attach_validation`
* `record_consumer`
* `set_status`

Suggested storage path:

* `.prism/contracts/events.jsonl`

Projected runtime views can materialize current contract state from the event log.

## 10. Query Surface

Contracts should be visible through normal PRISM read flows, not only through raw mutation logs.

Useful read surfaces include:

* `prism_concept(..., lens="contracts")`
* `prism_code` helpers such as `prism.contract(id)` and `prism.contractsFor(target)`
* compact integrations in `impact`, `afterEdit`, `workset`, and review-oriented views
* resource surfaces such as `prism://contracts` and `prism://schema/contracts`

The first read experience should make it easy to answer:

* what contracts govern this target?
* did this edit touch implementation or a contract?
* who consumes this promise?
* what validations defend it?
* what known assumptions limit it?

## 11. Mutation Surface

Contracts should be created and maintained through explicit mutations, not opaque side effects.

The first implementation should expose explicit `prism_code` contract operations such as:

* `promote`
* `update`
* `retire`
* `attach_evidence`
* `attach_validation`
* `record_consumer`

Compact helper tools may come later, but the initial write path should remain explicit and inspectable.

## 12. Integration Points

Contracts become useful once they appear in the agent loop.

Priority integrations:

### 12.1 Impact

`impact(...)` should surface:

* contracts touched by the edit
* known consumers of those contracts
* whether the change looks implementation-only or contract-affecting
* validations that matter for the touched contracts

### 12.2 After Edit

`afterEdit(...)` should surface:

* contract-aware next reads
* validations tied to affected contracts
* migration or compatibility notes

### 12.3 Review

Review surfaces should be able to say:

* this edit appears to widen a promise
* this edit weakens a guarantee without updating validations
* this edit changes a subject with many consumers

### 12.4 Planning

Plans and coordination tasks should be able to reference contracts explicitly when a task is:

* implementing a contract
* migrating a contract
* deprecating a contract
* repairing a broken contract

## 13. How Contracts Should Be Discovered

The first version should support both manual curation and bounded inference.

Good discovery sources:

* repeated broad-query resolutions
* stable public API surfaces
* schema boundaries
* directional dependency boundaries
* tests that encode long-lived behavioral promises
* memory/outcome clusters showing a repeated promise or failure boundary

Bad discovery sources:

* one-off temporary debugging notes
* arbitrary file groups
* unstable implementation details
* speculative rules with no evidence

## 14. Implementation Phases

### Phase 0: Schema And Event Model

Deliver:

* contract object schema
* event schema
* projection layer
* basic storage location

Do not deliver yet:

* automatic enforcement
* broad inference engine

### Phase 1: Read Surface

Deliver:

* contract resources
* `prism_code` query helpers
* concept and target lookups for contracts

Success condition:

* an agent can inspect contract state without reading raw event logs

### Phase 2: Mutation Flow

Deliver:

* explicit `prism_code` contract operations
* evidence and validation attachment
* lifecycle/status transitions

Success condition:

* agents can record and maintain contracts during normal work

### Phase 3: Query Integration

Deliver:

* `impact` integration
* `afterEdit` integration
* review integration
* contract-aware workset hints where appropriate

Success condition:

* contract knowledge changes actual agent behavior, not only stored metadata

### Phase 4: Derived Guardrails

Deliver:

* policy-like checks or warnings derived from contracts
* optional validation against touched targets

Success condition:

* PRISM can warn that a change likely violates or widens a known contract

## 15. Open Questions

Open questions that should be resolved during implementation:

1. Should `subject` allow multiple providers, or should that be represented through a concept-owned contract?
2. How much of `consumers` should be inferred automatically versus curated explicitly?
3. Should compatibility classes be global enums or contract-kind-specific vocabularies?
4. What is the minimum evidence threshold for promoting a contract to `repo` scope?
5. When should a contract split into multiple contracts instead of accreting clauses?
6. How should contracts interact with lineage drift when symbols move or split?
7. Which contract surfaces deserve compact tools, and which should stay under `prism_code` first?
8. When should contract health move from a compact derived view into stronger review or policy gates?

## 16. Success Criteria

Contracts are worth adding if they let PRISM do things it cannot do well today.

The primitive is successful when:

* agents can name the governing promise for a change, not only the touched files
* impact analysis can point to promise consumers and relevant validations
* review can distinguish implementation churn from contract change
* durable repo knowledge captures important boundaries without overfitting to one repository
* future policy features can attach to contracts instead of inventing a parallel ontology

That is the bar: not another metadata bucket, but a reusable promise layer for repository cognition.
