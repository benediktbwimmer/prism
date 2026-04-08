# PRISM Specs

Status: active spec index  
Audience: contributors and implementation agents  
Scope: concrete implementation specs and the rules for writing them

---

## Purpose

`docs/specs/` is the home for implementation-target documents.

A spec in this directory should define a concrete deliverable clearly enough that an agent or human
can create a PRISM plan to implement it and review code against it iteratively.

Specs are not the same as contracts:

- contracts define stable seams and invariants
- specs define a bounded implementation target against those contracts

## When to write a spec

Write or update a spec before a significant implementation sprint when the work includes one or
more of:

- a new subsystem or interface
- a meaningful refactor with several phases
- public API or storage-shape changes
- a rollout or migration plan
- nontrivial validation requirements

Small localized fixes usually do not need a spec.

## Filename rules

Specs should use date-prefixed filenames so they sort naturally and remain easy to scan over time.

Format:

- `YYYY-MM-DD-short-name.md`

Examples:

- `2026-04-08-coordination-authority-store-cutover.md`
- `2026-04-15-service-read-broker-phase-1.md`

Use a short descriptive suffix that names the concrete delivery slice.

## Required spec header

Each spec should start with a small header block that includes at least:

- `Status`
- `Audience`
- `Scope`

Recommended statuses:

- `draft`
- `approved`
- `in progress`
- `partially implemented`
- `completed`
- `superseded`

## Recommended spec structure

Specs should usually include these sections:

1. Summary
2. Status
3. Scope
4. Non-goals
5. Related contracts
6. Design
7. Implementation slices
8. Validation
9. Rollout or migration
10. Open questions

Not every spec needs every section, but a spec should be concrete enough to implement against.

## Status tracking

Specs should track coarse implementation status in git.

That status should stay lightweight:

- current status value
- a short milestone or checklist section
- notes on what has landed versus what remains

Good examples:

- `[x] authority trait introduced`
- `[ ] Git backend cut over`
- `[ ] Postgres backend scaffolded`
- `[ ] MCP call sites migrated`

Specs should not become a live task board.

Detailed execution state belongs in PRISM plans, not in the spec file.

## Relationship to PRISM plans

Use this split:

- spec = implementation target
- PRISM plan = live execution and ownership
- contract = stable invariant boundary

A future agent should be able to:

1. read the relevant contracts
2. read the spec
3. create a PRISM plan to implement the spec
4. update the spec's coarse status as slices land

## Writing rules

Specs in this directory should:

- be concrete and implementation-facing
- reference the contracts they depend on
- define validation expectations explicitly
- describe rollout and migration when relevant
- be revised as the implementation target changes

Specs in this directory should not:

- redefine contract-level semantics that belong in `docs/contracts/`
- become long historical essays
- become a replacement for PRISM task tracking

## Review rule

Significant implementation work should be reviewed against both:

- the relevant contract docs in `docs/contracts/`
- the concrete target spec in `docs/specs/`

If the implementation meaningfully diverges from the spec, update the spec or write a replacement
spec before continuing to fan out the implementation.
