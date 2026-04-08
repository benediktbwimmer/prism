# PRISM Roadmaps

Status: active roadmap index
Audience: contributors, implementation agents, and maintainers
Scope: multi-phase implementation orderings and program-level sequencing across several specs or subsystems

---

## Purpose

`docs/roadmaps/` is the home for tracked implementation roadmaps that span multiple specs, phases,
or subsystem boundaries.

Roadmaps are different from specs:

- contracts define stable seams and invariants
- specs define one concrete implementation target
- roadmaps define a longer execution order across multiple targets

Use a roadmap when the important thing is not just what to build, but in what order it must be
built to keep the architecture clean.

## When to write a roadmap

Write or update a roadmap when the work includes:

- several dependent specs or subsystems
- a required topological implementation order
- platform-foundation work that other efforts will build on
- a cleanup or migration sequence that should be tracked in git over time

Roadmaps are especially appropriate for:

- abstraction cutovers
- multi-phase platform rewrites
- “build foundation A fully before subsystem B” programs

## Filename rules

Roadmaps should use date-prefixed filenames:

- `YYYY-MM-DD-short-name.md`

Examples:

- `2026-04-08-coordination-to-spec-engine-to-service.md`
- `2026-04-15-cross-repo-coordination-rollout.md`

## Recommended roadmap structure

Roadmaps should usually include:

1. Summary
2. Status
3. Ordering thesis
4. Phases
5. Exit criteria per phase
6. Dependency logic
7. Risks or anti-patterns

## Status tracking

Roadmaps should be trackable in git.

They should include:

- a status field
- a coarse phase checklist
- clear exit criteria

They should not become a live task board.

Fine-grained execution still belongs in PRISM plans and in the specs that each phase implements.

## Relationship to specs and plans

Use this split:

- roadmap = multi-phase ordering and sequencing
- spec = concrete implementation target for one slice
- PRISM plan = live execution state

When implementing against a roadmap:

1. read the roadmap for ordering
2. read the relevant spec for the current phase
3. create a PRISM plan for the current phase or spec
4. update the roadmap only when the phase-level status changes
