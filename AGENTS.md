# AGENTS.md

## Purpose

This repository must stay highly modularized. Prefer small, focused modules with clear responsibilities over large, mixed-purpose files.

## Architectural Rule

`main.rs` and `lib.rs` files are facades only.

- Do not place core business logic, parsing logic, coordination logic, storage logic, or domain rules directly in `main.rs` or `lib.rs`.
- Use `main.rs` only to wire together the executable entrypoint, CLI/bootstrap setup, and top-level module boundaries.
- Use `lib.rs` only to declare modules, define the crate's public surface, and re-export narrowly chosen APIs.
- Move substantive logic into dedicated submodules with descriptive names.

## Modularity Expectations

- Keep modules narrowly scoped and cohesive.
- Split large files before they accumulate unrelated responsibilities.
- Favor composition between modules instead of growing monolithic entrypoint files.
- Keep public APIs minimal and intentional.
- When adding a feature, first decide which module owns it; if no clear owner exists, create one.

## Refactoring Guidance

When touching code that violates this policy, move it toward the target architecture instead of extending the violation.

## PRISM MCP Workflow

When the PRISM MCP server is available for this repo, use it as the primary repo-awareness surface.

- Start with `prism://api-reference` and `prism://session` to confirm the available query surface, active task, and session limits.
- Use `prism_query` as the default tool for all read access into PRISM state.
- Prefer `prism_query` over bespoke lookups for code structure, lineage, memory, coordination state, blockers, claims, artifacts, and review queues.
- Keep `prism_query` read-only. Do not try to encode writes or side effects inside query snippets.

When mutations make sense, use the explicit PRISM mutation tools instead of leaving the state implicit.

- Use `prism_session` with action `start_task` when beginning a meaningful unit of work and no suitable active task already exists.
- Use `prism_mutate` with actions `outcome`, `test_ran`, `failure_observed`, and `fix_validated` to record task outcomes that matter for future reasoning.
- Use `prism_mutate` with action `memory` to store anchored memory when you learn something worth preserving, especially repo-specific constraints, invariants, migration rules, repeated failure patterns, or other durable lessons.
- Use `prism_mutate` with action `infer_edge` when a new inferred relationship should be captured explicitly rather than only described in prose.
- Use `prism_mutate` with actions `coordination`, `claim`, and `artifact` when the work involves shared planning, task state, claims, handoffs, or reviewable artifacts.

Mutation guidance:

- Record memory only when the information is likely to help later tasks and is specific enough to anchor to code, lineage, files, or kinds.
- Record outcomes for meaningful tests, failures, validations, and task milestones, not for trivial intermediate noise.
- Prefer explicit anchored PRISM state over ad hoc scratch notes when the information should survive the current session.
