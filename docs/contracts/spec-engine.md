# PRISM Spec Engine

Status: normative contract
Audience: coordination, query, MCP, CLI, UI, storage, and future service maintainers
Scope: native PRISM handling of implementation specs as local repo artifacts, including source configuration, parsing, local materialization, querying, dependency evaluation, and coordination integration boundaries

---

## 1. Goal

PRISM must define one explicit **spec engine** contract if implementation specs are to become a
native feature rather than a loose docs convention.

This contract exists so that:

- spec files can be treated as structured local PRISM objects
- spec parsing and query behavior stay deterministic
- specs integrate cleanly with coordination without becoming shared authority
- other repos can opt into native spec support without adopting PRISM's exact docs layout

## 2. Core invariants

The spec engine must preserve these rules:

1. Specs are first-class local PRISM objects.
2. Specs are not authoritative coordination truth by default.
3. Spec state is derived from repo-local source files in the checked-out revision.
4. Spec queries must be deterministic and structured.
5. Spec materialization must be local, disposable, and rebuildable.
6. Coordination may link to specs explicitly, but branch-local spec state must not silently rewrite
   authoritative task or plan truth.
7. `spec_id` must be stable and unique within one repo.
8. Checklist item identity must remain stable enough for sync and coverage tracking beyond a raw
   positional index.
9. Checklist items must distinguish required versus informational intent through explicit metadata,
   not guesswork.

## 3. Canonical ownership

Canonical ownership:

- this document defines the stable native boundary for spec sources, spec state, and spec query
  semantics
- implementation-target details and rollout slices belong in dated docs under `docs/specs/`
- broader docs hierarchy rules belong in `docs/README.md` and `docs/specs/README.md`

## 4. Source configuration

The spec engine must support a configurable spec root.

Initial rules:

- default root: `.prism/specs/`
- repos may configure another repo-relative root
- the configured root must remain inside the repo

Native spec support must therefore not assume one hardcoded path such as `docs/specs/`.

## 5. Canonical source format

The canonical v1 source format should be:

- markdown document
- YAML frontmatter for structured fields
- markdown body for prose and checklists

Separate YAML sidecars may exist later, but they are not the normative source format in v1.

## 6. Minimum spec fields

The minimum structured fields must include:

- `id`
- `title`
- `status`
- `created`

The engine should also support, at minimum:

- `depends_on`
- `related_contracts`
- optional coordination links such as plan or task refs
- validation metadata when present

Declared `status` should use a shared minimum vocabulary:

- `draft`
- `in_progress`
- `blocked`
- `completed`
- `superseded`
- `abandoned`

## 7. Local object model

The native spec layer must support at least:

- `SpecRecord`
- `SpecChecklistItem`
- `SpecDependency`
- `SpecStatusView`
- `SpecCoverageView`
- `SpecSyncProvenance`

The exact implementation types may vary.
The semantic families should remain stable.

`SpecCoverageView` is the derived local join view that explains what parts of a spec are covered by
linked coordination objects and what parts remain uncovered or drifted.

`SpecSyncProvenance` is the local record that identifies which spec revision or checklist identity
was used when coordination-linked objects were explicitly created or synced from spec state.

## 8. Checklist identity

Checklist items must have stable local identity.

The engine should support:

- explicit checklist item ids when authors provide them
- otherwise a deterministic generated key from stable local context such as section path,
  normalized label text, and local disambiguator

Raw ordinal position alone is not sufficient.

## 9. Dependency semantics

The spec engine must support explicit spec-to-spec dependencies.

At minimum it must surface:

- resolved and complete
- resolved and incomplete
- missing
- cyclic

Derived spec posture may use dependency posture, declared status, and checklist posture together,
but the resulting state must be deterministic and explainable.

## 10. Checklist requirement levels

Checklist items must support a requirement level.

The shared minimum levels are:

- `required`
- `informational`

Rules:

- checklist items are `required` by default
- informational items must be marked explicitly
- derived completion or blocked posture must ignore informational items unless another explicit
  policy says otherwise

Section-level defaults may exist, but item-level effective requirement level must be queryable.

## 11. Coverage semantics

The spec engine must support one derived local coverage view that can answer, at minimum:

- which checklist items are uncovered by coordination
- which checklist items are represented by linked plans or tasks
- which checklist items are in progress
- which checklist items are completed
- which checklist items are review- or artifact-backed when that information is available through
  coordination joins
- which checklist items or linked objects have drifted because the spec changed after explicit sync

Coverage is a local derived join over:

- spec checklist items and sections
- linked coordination objects
- explicit sync provenance

Coverage is not authoritative coordination truth by itself.

## 12. Sync provenance semantics

Because translating a flat markdown checklist into an explicit coordination DAG requires semantic
understanding, PRISM does not automatically "compile" specs into coordination state. Syncing is an
explicit, typically agent-driven action.

When an agent or human explicitly creates or syncs coordination objects from a spec, PRISM must retain
queryable sync provenance.

The minimum sync provenance must identify:

- `spec_id`
- source spec path
- source spec revision, commit, or equivalent source-version marker
- target coordination object id
- sync kind such as `create_from_spec` or `update_from_spec`
- checklist item ids and/or section ids used for the sync when relevant

General actor and timestamp attribution should rely on the shared provenance contract rather than
inventing a second parallel authorship model here.

## 13. Query surfaces

The spec engine must expose its own deterministic query families, typically through a dedicated
`SpecQueryEngine` or equivalent documented seam.

At minimum it must support:

- listing specs
- fetching one spec by id
- fetching checklist items
- fetching dependency graph or dependency posture
- fetching derived local spec status
- fetching spec coverage
- fetching sync provenance for linked coordination objects
- fetching local joins between specs and linked coordination objects

These queries may be surfaced through CLI, MCP, or both, but they should not be implemented as ad
hoc handler logic without a documented seam.

The coordination query engine may consume explicit join outputs from the spec engine, but it should
not absorb spec parsing or local spec evaluation semantics into itself.

## 14. Materialization

Native spec state may be materialized locally for fast reads.

That materialization may include:

- parsed frontmatter
- checklist items
- dependency edges
- derived status views
- derived coverage views
- sync provenance records
- source file metadata

This materialization is:

- local
- non-authoritative
- rebuildable from repo files

## 15. Coordination integration boundary

Plans and tasks may link to specs through explicit references.

The engine may support joins such as:

- linked spec summaries on tasks or plans
- open checklist items relevant to a linked task
- coverage summaries for one spec against linked coordination objects
- divergence warnings between local spec posture and authoritative coordination state

The engine must not, by default:

- block authoritative task readiness solely from local spec dependencies
- mark authoritative tasks complete solely from local checklist state
- derive authoritative plan completion from local spec posture

When a plan or task is explicitly created or synced from a spec, PRISM should retain sync
provenance that can identify at least:

- `spec_id`
- source spec path
- source spec revision, commit, or equivalent source-version marker
- checklist item ids or section ids used for the sync when relevant

## 16. Relationship to other contracts

This contract is a client of:

- [coordination-query-engine.md](./coordination-query-engine.md)
- [coordination-materialized-store.md](./coordination-materialized-store.md)
- [local-materialization.md](./local-materialization.md)
- [consistency-and-freshness.md](./consistency-and-freshness.md)
- [shared-scope-and-identity.md](./shared-scope-and-identity.md)
- [reference-and-binding.md](./reference-and-binding.md)

## 17. Minimum implementation bar

This contract is considered implemented only when:

- spec roots are configurable and repo-relative
- specs are parsed into deterministic local records
- `spec_id` is treated as repo-unique
- checklist identity is stable enough for sync and coverage use
- checklist requirement levels are explicit and queryable
- checklist and dependency state are queryable
- spec coverage is queryable
- explicit sync provenance is queryable
- local spec state is materialized through a disciplined seam
- coordination integration preserves the local-versus-authoritative boundary explicitly
