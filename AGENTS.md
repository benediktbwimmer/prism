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
- Prefer PRISM-native file inspection and search when they can replace multiple shell reads with one bounded query, especially `prism.file(path).read(...)`, `prism.file(path).around(...)`, and `prism.searchText(...)`.
- Prefer `prism.file(...)` and `prism.searchText(...)` over `sed`, `cat`, and `rg` when the work can be composed into a single PRISM query call that returns the exact slice, match, or surrounding context you need.
- Keep shell reads as a fallback for raw bytes, command output, or cases where PRISM cannot yet express the needed inspection precisely.
- Keep `prism_query` read-only. Do not try to encode writes or side effects inside query snippets.
- After meaningful changes to PRISM MCP behavior or query/runtime behavior, rebuild the release binaries and restart the MCP daemon so the live PRISM server reflects the current code during the same Codex session.
- From the repo root, use these exact commands:
  - `cargo build --release -p prism-cli -p prism-mcp`
  - `./target/release/prism-cli mcp restart --internal-developer`
  - `./target/release/prism-cli mcp status`
  - `./target/release/prism-cli mcp health`
- Prefer the release binaries for restart and verification instead of `cargo run`, so the daemon and CLI are both using the freshly rebuilt release executables.

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

## Dogfooding Feedback Loop

When you use PRISM while working on PRISM, record notable validation cases immediately instead of waiting for a later replay-harness pass.

- Record a feedback entry whenever PRISM is materially wrong, stale, noisy, or unusually helpful during real repo work.
- Include the task or query context, the involved anchors, what PRISM said, what was actually true, the subsystem category (`structural`, `lineage`, `memory`, `projection`, `coordination`, `freshness`, or `other`), and whether you corrected it manually.
- Prefer `prism_mutate` with action `validation_feedback` when the PRISM MCP server is available.
- Otherwise use `prism feedback record ...` from the CLI.
- The log is append-only and lives at `.prism/validation_feedback.jsonl`; treat it as seed material for the future replay validation harness, not as scratch prose.
