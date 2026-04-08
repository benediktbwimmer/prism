# Native Spec Engine

Status: draft
Audience: coordination, query, MCP, CLI, UI, storage, and future service maintainers
Scope: native PRISM support for implementation specs as local repo artifacts, including parsing, materialization, querying, dependency evaluation, and coordination integration

---

## 1. Summary

PRISM should treat implementation specs as a native local subsystem rather than as ad hoc markdown
files that only humans read.

The goal is to make specs:

- configurable by repo
- parseable into structured records
- queryable through CLI and MCP
- materialized locally for fast reads
- integratable with PRISM plans and tasks without making branch-local docs authoritative

The key rule is:

- specs are a first-class local PRISM feature
- specs are not authoritative coordination truth by default

This keeps coordination correctness in shared authority while still letting PRISM use specs as a
native execution-target layer.

## 2. Status

Current state:

- [ ] native spec source configuration exists
- [ ] spec parser and schema validation exist
- [ ] spec materialized store exists
- [ ] spec query surface exists in CLI
- [ ] spec query surface exists in MCP
- [ ] spec dependency graph and derived status rules exist
- [ ] stable checklist item identity exists
- [ ] checklist requirement levels exist
- [ ] spec coverage view exists
- [ ] explicit sync provenance exists
- [ ] coordination and spec join queries exist
- [ ] explicit spec-to-coordination sync actions exist

## 3. Scope

This spec covers:

- repo-local spec source configuration
- markdown-plus-frontmatter spec schema
- structured parsing of spec metadata and checklist items
- spec dependency modeling
- derived local spec state
- local materialization for spec queries
- CLI and MCP query surfaces
- local join surfaces between specs and coordination objects

This spec does not make specs authoritative coordination state.

## 4. Non-goals

This spec does not include:

- automatic promotion of branch-local spec state into authoritative coordination truth
- automatic blocking of shared tasks based only on local spec dependencies
- a separate YAML sidecar format as the canonical spec source
- fuzzy semantic search as the initial feature
- cross-repo shared spec authority in v1

## 5. Related contracts

This spec depends on:

- [../contracts/spec-engine.md](../contracts/spec-engine.md)
- [../contracts/coordination-query-engine.md](../contracts/coordination-query-engine.md)
- [../contracts/coordination-materialized-store.md](../contracts/coordination-materialized-store.md)
- [../contracts/local-materialization.md](../contracts/local-materialization.md)
- [../contracts/consistency-and-freshness.md](../contracts/consistency-and-freshness.md)
- [../contracts/shared-scope-and-identity.md](../contracts/shared-scope-and-identity.md)
- [../contracts/reference-and-binding.md](../contracts/reference-and-binding.md)

This spec should also remain aligned with:

- [../README.md](../README.md)
- [README.md](./README.md)

## 6. Design

### 6.1 Native object model

PRISM should add a native local spec layer with at least these concepts:

- `SpecRecord`
  - one parsed implementation spec
- `SpecChecklistItem`
  - one normalized checkbox item extracted from the spec body
- `SpecDependency`
  - one declared dependency from one spec to another
- `SpecStatusView`
  - one derived local view of spec status, checklist state, and dependency posture
- `SpecCoverageView`
  - one derived local join view that explains which parts of the spec are or are not covered by
    linked coordination objects
- `SpecSyncProvenance`
  - one local record of which spec revision and checklist identities were used when coordination
    objects were explicitly created or synced from a spec

These are local PRISM objects derived from repo files.
They are not shared coordination authority objects.

### 6.2 Source configuration

PRISM should support a configurable spec root.

Initial rules:

- default spec root: `.prism/specs/`
- a repo may override this to another repo-relative folder
- PRISM itself may configure `docs/specs/` for dogfooding

The configured root must be:

- repo-relative
- inside the repo
- deterministic for the current checkout

Spec roots are local repo configuration, not shared coordination truth.

### 6.3 Canonical source format

The canonical source format should be:

- markdown document
- YAML frontmatter for structured fields
- markdown body for narrative and checklists

PRISM should not use separate YAML sidecars as the primary source format in v1.

Reason:

- one git-traveling artifact is easier to review
- prose and structure stay together
- docs remain useful even without PRISM tooling

### 6.4 Required frontmatter and ids

The minimum frontmatter should include:

- `id`
- `title`
- `status`
- `created`

`id` should be unique within the repo, not only within one configured spec root.

Declared `status` should use a small explicit vocabulary.

Initial values:

- `draft`
- `in_progress`
- `blocked`
- `completed`
- `superseded`
- `abandoned`

Recommended fields:

- `owners`
- `depends_on`
- `related_contracts`
- `coordination`
- `validation`
- `supersedes`

Example:

```md
---
id: spec:native-spec-engine
title: Native Spec Engine
status: draft
created: 2026-04-08
owners:
  - coordination
depends_on:
  - spec:coordination-query-engine
related_contracts:
  - coordination-query-engine
  - coordination-materialized-store
coordination:
  plan: coord-plan:example
  tasks:
    - coord-task:example-1
validation:
  - cargo test -p prism-query
---
```

### 6.5 Checklist extraction and identity

PRISM should parse markdown checkbox items in the spec body into structured checklist records.

The normalized checklist item should include at least:

- owning `spec_id`
- stable local checklist key
- rendered label text
- checked or unchecked state
- section context when available

Raw index is not enough.
Checklist identity must survive ordinary editing better than position alone.

#### Preferred: explicit inline annotations

The preferred checklist identity mechanism is an explicit inline annotation on the checklist item
itself, for example:

```md
- [ ] implement plan sorting <!-- id: impl-sort -->
- [ ] add query surface for sorted plans <!-- id: sort-query -->
```

Explicit ids are stable across rewording, reordering, and section reorganization.

This matters because sync provenance and coverage tracking bind to checklist item identities. If a
human rewords a checklist item and the identity silently changes, all linked sync provenance
orphans, coverage tracking fractures, and drift detection produces false positives.

Explicit annotations eliminate that failure mode entirely.

PRISM's own specs should use explicit annotations from day one so that dogfooding exercises the
stable identity path immediately.

#### Fallback: generated keys

When no explicit annotation is present, PRISM should fall back to a deterministic generated key
based on section path, normalized label text, and a local disambiguator.

Generated keys are acceptable for repos that do not need tight sync provenance or coverage
tracking. They are not acceptable as the primary identity mechanism for specs that will drive
coordination sync.

PRISM should surface a diagnostic warning when a spec uses generated keys for checklist items that
are linked to coordination objects through sync provenance.

PRISM should not require a separate checklist schema in v1.

### 6.6 Checklist requirement levels

Checklist items should support an explicit requirement level.

Initial values:

- `required`
- `informational`

Rules:

- checklist items are `required` by default
- informational items must be marked explicitly
- derived completion or blocked posture ignores informational items unless another explicit policy
  says otherwise

The initial parser should support either:

- explicit item-level markers such as `[info]`
- or section-level defaults such as a heading suffix like `(informational)`

The materialized result must always expose the effective item-level requirement level explicitly.

### 6.7 Dependency model

Specs should support explicit dependencies through `depends_on`.

In v1:

- dependencies should point to other `spec_id` values
- dependency resolution is local to the configured spec root
- missing dependencies should be explicit and queryable

The derived dependency posture should distinguish at least:

- `resolved_complete`
- `resolved_incomplete`
- `missing`
- `cyclic`

### 6.8 Derived spec state

PRISM should derive a local spec state from:

- explicit spec `status`
- checklist completion
- dependency posture

The derived view should distinguish:

- declared status
- checklist posture
- dependency posture
- overall local spec posture

The overall local spec posture should be deterministic and explainable.

One acceptable v1 rule is:

- explicit terminal statuses such as `completed` or `superseded` win first
- otherwise unresolved dependencies make the derived posture blocked
- otherwise unchecked required checklist items keep the derived posture incomplete
- otherwise the derived posture follows the declared status

The exact mapping may evolve, but it must remain deterministic and documented.

Required checklist items participate in derived incomplete or blocked posture.
Informational items do not.

### 6.9 Spec coverage model

PRISM should derive one first-class `SpecCoverageView`.

This is the local join view that answers:

- which checklist items are still uncovered by coordination
- which are represented by linked plans or tasks
- which are in progress
- which are completed
- which are backed by artifact or review state when that information is available
- which have drifted because the spec changed after explicit sync
- which coordination objects are done but not yet reflected back into the spec

Coverage must be derived from:

- spec checklist items and sections
- linked plans or tasks
- artifact or review posture when relevant
- explicit sync provenance

Coverage is local and derived.
It is not authoritative coordination truth.

### 6.10 Coordination integration

Specs should integrate tightly with coordination, but without becoming authority.

Initial integration model:

- plans and tasks may carry `spec_refs`
- PRISM may expose local joins between specs and coordination objects
- task and plan read surfaces may include linked spec summaries, open checklist items, and spec
  divergence warnings

PRISM must not, by default:

- mark authoritative tasks blocked solely because a local spec dependency is incomplete
- mark authoritative tasks complete solely because a local checklist item is checked
- derive authoritative plan truth directly from branch-local spec state

### 6.11 Explicit agent-driven sync model and provenance

If users want stronger coordination integration, PRISM should support explicit sync actions later.

Crucially, PRISM does not deterministically "compile" a flat markdown checklist into a complex
coordination DAG. Topology, artifact requirements, and review injection require semantic understanding.
Therefore, the sync boundary is natively an **agentic action**.

Examples:

- an agent reads a spec and creates a plan or task DAG via `coordination_transaction`
- an agent syncs spec milestones into task summaries
- a human or agent updates checklist status based on completed tasks

These must be explicit actions mapping feature intent to the execution graph, not silent background
coupling by the PRISM daemon.

Whenever PRISM explicitly creates or syncs coordination objects from a spec, it should record sync
provenance that can identify at least:

- `spec_id`
- source spec path
- source spec revision or commit
- target coordination object id
- sync kind such as `create_from_spec` or `update_from_spec`
- checklist item ids or section ids used for the sync when relevant

Without this provenance, PRISM cannot explain:

- which plan reflects which version of a spec
- whether the spec changed after plan creation
- which checklist items are newly uncovered

General actor and timestamp attribution should rely on the shared provenance contract rather than a
second spec-specific authorship model.

### 6.12 Query engine boundary

PRISM should keep spec querying separate from authoritative coordination querying.

The preferred model is:

- `SpecQueryEngine`
- `CoordinationQueryEngine`
- explicit join queries between them

That is cleaner than absorbing branch-local spec semantics into the authoritative coordination
query engine.

### 6.13 Branch-local reality

Specs travel with git and are therefore branch-local.

That means:

- different branches may have different spec content
- spec state may legitimately diverge from shared coordination state
- PRISM must surface that divergence honestly

This is why native spec state is local and queryable, but not authoritative coordination truth.

## 7. Query surfaces

### 7.1 CLI

PRISM should add a docs or specs command family, for example:

- `prism specs list`
- `prism specs show <spec-id>`
- `prism specs checklist --open`
- `prism specs deps <spec-id>`
- `prism specs status`
- `prism specs check`

The CLI should support humans first.

### 7.2 MCP

PRISM should expose matching spec queries over MCP.

Minimum useful MCP families:

- list specs
- fetch one spec by id
- fetch open checklist items
- fetch dependency graph for one spec
- fetch local spec status summary
- fetch spec coverage for one spec
- fetch sync provenance for one spec or linked coordination object
- fetch linked task or plan spec summaries

### 7.3 Query engine integration

PRISM should expose deterministic local spec queries through a native query seam rather than
implementing spec inspection ad hoc in MCP handlers.

The intended shape is:

- a dedicated `SpecQueryEngine`
- the existing `CoordinationQueryEngine`
- explicit local join queries between them

The important rule is:

- spec queries must be deterministic and structured
- they must not be buried in product handlers

## 8. Materialization

PRISM should materialize parsed spec state locally for fast reads.

The materialized spec layer should include:

- parsed frontmatter fields
- checklist items
- effective requirement levels
- dependency edges
- derived status view
- derived coverage view
- sync provenance records
- source file path and source revision metadata

This materialization is:

- local
- disposable
- rebuildable from repo files

It must not be treated as authority.

## 9. Validation

The initial implementation should include:

- schema validation for required frontmatter
- filename validation for date-prefixed spec docs
- repo-unique spec id validation
- dependency resolution checks
- stable checklist identity generation tests
- required-versus-informational checklist tests
- broken-link and relative-link validation across docs
- parser tests for frontmatter and checklist extraction
- query tests for status and dependency evaluation
- query tests for spec coverage and drift detection

## 10. Implementation slices

### Slice 1: Source and parser

- add repo config for spec root
- parse markdown frontmatter and checklist items
- validate required fields
- parse required versus informational checklist semantics

### Slice 2: Local materialization

- persist parsed specs, checklist items, and dependency edges
- add derived status computation
- add stable checklist identity generation
- persist effective checklist requirement levels
- add coverage and sync provenance storage

### Slice 3: Human CLI

- add `prism specs` query commands
- add `prism docs check` or equivalent docs validation command
- add `prism specs coverage <spec-id>`

### Slice 4: MCP surface

- expose spec queries over MCP
- expose linked spec summaries in task and plan read surfaces
- expose spec coverage and divergence views

### Slice 5: Explicit coordination sync

- add explicit spec-to-plan or spec-to-task helper actions
- do not change authority semantics

## 11. Rollout

Recommended rollout:

1. implement native parsing and local query support
2. adopt it in PRISM's own `docs/specs/`
3. expose deterministic CLI and MCP queries
4. add explicit spec-to-coordination linking and sync helpers

The rollout should not require other repos to adopt PRISM's own docs hierarchy.

The native feature should work with:

- default `.prism/specs/`
- repo-configured spec roots such as `docs/specs/`

## 12. Open questions

There are no remaining open design questions for the v1 boundary in this spec.

Future refinements may still extend:

- richer checklist requirement-level vocabularies
- more expressive section and subsection defaults
- more detailed sync provenance payloads

Those are follow-on extensions, not unresolved v1 design questions.
