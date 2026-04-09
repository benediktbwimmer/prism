# PRISM Documentation

Status: active documentation index  
Audience: all contributors and agents  
Scope: repository documentation hierarchy and placement rules

---

## Purpose

This directory contains PRISM's active documentation.

The docs hierarchy is now intentionally split by document role. New docs should generally go into a
named subdirectory, not directly into `docs/`.

## Current hierarchy

### `contracts/`

Stable normative contracts.

Use `docs/contracts/` for:

- backend-neutral seams and interfaces
- cross-cutting invariants
- required metadata and semantics
- behavior that implementations and tests must target directly

When a higher-level design note and a contract disagree, the contract is the normative source.

Start here:

- [contracts/README.md](./contracts/README.md)

### `designs/`

Active architecture and product-shape docs.

Use `docs/designs/` for:

- proposed or target architectures
- higher-level design companions to contracts
- capability-family and policy-model designs
- product-shape notes that are still guiding implementation
- active design docs that are broader than one implementation slice

Start here:

- [designs/README.md](./designs/README.md)

### `specs/`

Concrete implementation targets for bounded delivery work.

Use `docs/specs/` for:

- implementation-ready feature or refactor specs
- phased rollout plans for significant implementation sprints
- validation plans for a concrete deliverable
- current coarse implementation status for that deliverable

Specs are the documents future agents should implement against.

Start here:

- [specs/README.md](./specs/README.md)

### `roadmaps/`

Tracked multi-phase implementation orderings.

Use `docs/roadmaps/` for:

- sequencing across several specs or subsystems
- foundation-first implementation ladders
- migration programs with explicit phase order and exit criteria
- git-tracked phase-level progress across a broader effort

Start here:

- [roadmaps/README.md](./roadmaps/README.md)

### `adrs/`

Architecture decision records.

Use `docs/adrs/` for:

- accepted or superseded architecture decisions
- explicit changes in ownership boundaries or deployment assumptions
- decisions that contracts, specs, and roadmaps should cite directly

Start here:

- [adrs/README.md](./adrs/README.md)

### `archived/`

Superseded or historical docs that should no longer be treated as active guidance.

Use `docs/archived/` for:

- obsolete designs
- retired implementation plans
- historical reference material that is still worth keeping in git

### `prism/`

Product, instruction, or user-facing documentation that belongs in its own namespace.

## Placement rules

New docs should follow these rules:

1. Prefer an existing subdirectory when the doc clearly fits there.
2. Do not add new active design or implementation docs directly to `docs/` unless there is a very
   strong reason.
3. If a new class of docs does not fit `contracts/`, `specs/`, `archived/`, or another existing
   subdirectory, add a new subdirectory with its own `README.md` first.
4. Keep top-level `docs/` files only for already-existing legacy material or exceptional docs that
   have not been migrated yet.

In practice, this means:

- if it defines stable semantics, it belongs in `contracts/`
- if it defines architecture shape, rationale, or target direction, it belongs in `designs/`
- if it defines a concrete implementation target, it belongs in `specs/`
- if it defines a broader implementation ordering across several targets, it belongs in
  `roadmaps/`
- if it records a durable architectural decision, it belongs in `adrs/`
- if it is no longer active, it belongs in `archived/`

## Legacy top-level docs

Many older active and semi-active docs originally lived directly in `docs/`.

The active design-note set has now been migrated into `docs/designs/`.
The remaining top-level files are either:

- legacy material still awaiting recategorization
- umbrella references such as `SPEC.md` and `ROADMAP.md`
- active contracts, specs, validation records, or implementation notes that still need a narrower home

When those docs are substantially revised, superseded, or split:

- move stable normative parts into `contracts/`
- move active architecture and product-shape material into `designs/`
- move implementation-target material into `specs/`
- move historical material into `archived/`

## Recommended workflow

For significant implementation work:

1. Confirm the relevant contracts in `docs/contracts/`.
2. If the work spans several specs or subsystem phases, read or update the relevant roadmap in
   `docs/roadmaps/`.
3. Create or update a dated spec in `docs/specs/`.
4. Create a PRISM plan to implement that spec.
5. Review code against the contracts, the roadmap when relevant, and the spec.
6. Update the spec status and checklist as implementation lands.

This keeps the repo aligned around:

- contracts as invariants
- roadmaps as multi-phase sequencing
- specs as implementation targets
- PRISM plans as live execution state

<!-- BEGIN GENERATED INDEX: docs-root -->
## Generated Inventory

_This section is generated by `scripts/update_doc_indices.py`. Do not edit it by hand._

### Subdirectories

- [contracts/README.md](./contracts/README.md) — 34 indexed markdown documents
- [designs/README.md](./designs/README.md) — 30 indexed markdown documents
- [specs/README.md](./specs/README.md) — 57 indexed markdown documents
- [roadmaps/README.md](./roadmaps/README.md) — 4 indexed markdown documents
- [adrs/README.md](./adrs/README.md) — 2 indexed markdown documents
- [archived/README.md](./archived/README.md) — 8 indexed markdown documents

### Top-Level Docs

- [BENCHMARK.md](./BENCHMARK.md)
- [COORDINATION_MUTATION_PERSISTENCE_REWRITE.md](./COORDINATION_MUTATION_PERSISTENCE_REWRITE.md) — Coordination Mutation Persistence Rewrite — `Status: active implementation contract`
- [NEXT_IMPROVEMENTS.md](./NEXT_IMPROVEMENTS.md) — PRISM Next Improvements
- [OPTIMIZATIONS.md](./OPTIMIZATIONS.md) — OPTIMIZATIONS — `Status: proposed follow-on roadmap`
- [PACKAGING_AND_DISTRIBUTION_PLAN.md](./PACKAGING_AND_DISTRIBUTION_PLAN.md) — PRISM Packaging And Distribution Plan
- [PATH_IDENTITY_CONTRACT.md](./PATH_IDENTITY_CONTRACT.md) — Path Identity Contract
- [PERFORMANCE_MILESTONE.md](./PERFORMANCE_MILESTONE.md) — PRISM Performance Milestone
- [PRISM_COORDINATION_CONFLICT_HANDLING.md](./PRISM_COORDINATION_CONFLICT_HANDLING.md) — PRISM Coordination Conflict Handling — `Status: normative coordination write contract`
- [PRISM_COORDINATION_GRAPH_REWRITE.md](./PRISM_COORDINATION_GRAPH_REWRITE.md) — PRISM Coordination Graph Rewrite — `Status: normative implementation target`
- [PRISM_SHARED_COORDINATION_REFS.md](./PRISM_SHARED_COORDINATION_REFS.md) — PRISM Shared Coordination Refs — `Status: implemented architecture with validated coverage`
- [PRISM_SHARED_RUNTIME_SQLITE_REMOVAL_CONTRACT.md](./PRISM_SHARED_RUNTIME_SQLITE_REMOVAL_CONTRACT.md) — PRISM Shared Runtime SQLite Removal Contract — `Status: execution contract for the federated runtime cutover`
- [ROADMAP.md](./ROADMAP.md) — PRISM Roadmap
- [SHARED_COORDINATION_REFS_VALIDATION.md](./SHARED_COORDINATION_REFS_VALIDATION.md) — Shared Coordination Refs Validation
- [SPEC.md](./SPEC.md) — PRISM — Perceptual Representation & Intelligence System for Machines — `Status: product thesis and umbrella reference`
- [TODO.md](./TODO.md) — TODO
<!-- END GENERATED INDEX: docs-root -->
